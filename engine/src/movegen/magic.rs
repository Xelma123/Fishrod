//! Magic bitboards: fast attack lookups for bishops and rooks.
//!
//! A slider's attacks depend only on the pieces standing on its rays. The last
//! square of a ray never matters, since nothing lies behind it to block, so
//! each square gets a mask of the "relevant" squares: its rays without their
//! ends. Each arrangement of pieces on those squares becomes a table index
//! with one multiplication and one shift:
//!
//! ```text
//! index = ((occupied & mask) * magic) >> (64 - squares in mask)
//! ```
//!
//! The multiplication gathers the scattered mask bits into the top bits of the
//! product. A good `magic` turns this into a perfect hash: two arrangements may
//! share an index only when they produce the same attacks. Such numbers are
//! found by trial: draw a random candidate, fill the table, and start over at
//! the first entry that conflicts. All squares share one table, each from its
//! own offset.
//!
//! Candidates come from a generator with a fixed seed, so the search finds the
//! same numbers on every run.

use std::sync::LazyLock;

use crate::rng::Rng;
use crate::types::{Bitboard, Square};

/// Ray directions as (file step, rank step).
type Directions = [(i8, i8); 4];

const BISHOP_DIRECTIONS: Directions = [(1, 1), (1, -1), (-1, -1), (-1, 1)];
const ROOK_DIRECTIONS: Directions = [(0, 1), (1, 0), (0, -1), (-1, 0)];

/// Seed of the magic number search: the bytes of "fishrod!".
const SEED: u64 = 0x6669_7368_726F_6421;

/// Bishop and rook attack tables, built the first time they are used.
pub(super) static SLIDERS: LazyLock<SliderAttacks> = LazyLock::new(SliderAttacks::new);

/// Where to find one square's attack sets in the shared table.
#[derive(Clone, Copy, Debug)]
struct Magic {
    /// The squares whose occupancy changes the attacks.
    mask: Bitboard,
    magic: u64,
    /// 64 minus the number of squares in `mask`: keeps only the top bits of the product.
    shift: u32,
    /// Where this square's entries start in the shared table.
    offset: usize,
}

impl Magic {
    /// Table index of the attack set for the given occupied squares.
    fn index(&self, occupied: Bitboard) -> usize {
        let relevant = (occupied & self.mask).0;
        self.offset + (relevant.wrapping_mul(self.magic) >> self.shift) as usize
    }
}

/// Attack tables for bishops and rooks.
pub(super) struct SliderAttacks {
    bishop: [Magic; 64],
    rook: [Magic; 64],
    /// Attack sets for every square and every arrangement of its relevant squares.
    table: Vec<Bitboard>,
}

impl SliderAttacks {
    fn new() -> SliderAttacks {
        let mut rng = Rng::new(SEED);
        let mut table = Vec::new();
        let bishop = find_magics(&BISHOP_DIRECTIONS, &mut rng, &mut table);
        let rook = find_magics(&ROOK_DIRECTIONS, &mut rng, &mut table);
        SliderAttacks {
            bishop,
            rook,
            table,
        }
    }

    /// Squares a bishop on `sq` attacks, given the occupied squares.
    pub(super) fn bishop(&self, sq: Square, occupied: Bitboard) -> Bitboard {
        self.table[self.bishop[sq.index()].index(occupied)]
    }

    /// Squares a rook on `sq` attacks, given the occupied squares.
    pub(super) fn rook(&self, sq: Square, occupied: Bitboard) -> Bitboard {
        self.table[self.rook[sq.index()].index(occupied)]
    }
}

/// Finds a magic for every square and appends each square's entries to `table`.
fn find_magics(directions: &Directions, rng: &mut Rng, table: &mut Vec<Bitboard>) -> [Magic; 64] {
    std::array::from_fn(|index| find_magic(Square::new(index as u8), directions, rng, table))
}

/// Finds a magic for a slider on `sq` and appends its entries to `table`.
fn find_magic(
    sq: Square,
    directions: &Directions,
    rng: &mut Rng,
    table: &mut Vec<Bitboard>,
) -> Magic {
    let mask = relevant_mask(sq, directions);
    let arrangements = subsets(mask);
    let attacks: Vec<Bitboard> = arrangements
        .iter()
        .map(|&occupied| sliding_attacks(sq, occupied, directions))
        .collect();

    let offset = table.len();
    table.resize(offset + arrangements.len(), Bitboard::EMPTY);
    let entries = &mut table[offset..];
    // `written[i] == attempt` means entry `i` was filled during the current
    // attempt, so a failed attempt leaves nothing to clean up.
    let mut written = vec![0u32; entries.len()];
    let mut attempt = 0;
    loop {
        attempt += 1;
        let candidate = Magic {
            mask,
            magic: rng.next_sparse_u64(),
            shift: 64 - mask.popcount(),
            offset: 0,
        };
        let fits = arrangements
            .iter()
            .zip(&attacks)
            .all(|(&occupied, &attack)| {
                let i = candidate.index(occupied);
                if written[i] == attempt {
                    // Sharing an entry is fine only if the attacks are the same.
                    entries[i] == attack
                } else {
                    written[i] = attempt;
                    entries[i] = attack;
                    true
                }
            });
        if fits {
            return Magic {
                offset,
                ..candidate
            };
        }
    }
}

/// The squares whose occupancy changes the attacks of a slider on `sq`: every
/// ray square except the last one before the edge.
fn relevant_mask(sq: Square, directions: &Directions) -> Bitboard {
    let mut mask = Bitboard::EMPTY;
    for &(file_step, rank_step) in directions {
        let mut current = sq;
        while let Some(next) = current.offset(file_step, rank_step) {
            if next.offset(file_step, rank_step).is_some() {
                mask = mask.with(next);
            }
            current = next;
        }
    }
    mask
}

/// Attacks of a slider on `sq`, found by walking each ray until it leaves the
/// board or reaches an occupied square, which is included. Slow but plainly
/// correct: the tables are built from it and tested against it.
fn sliding_attacks(sq: Square, occupied: Bitboard, directions: &Directions) -> Bitboard {
    let mut attacks = Bitboard::EMPTY;
    for &(file_step, rank_step) in directions {
        let mut current = sq;
        while let Some(next) = current.offset(file_step, rank_step) {
            attacks = attacks.with(next);
            if occupied.contains(next) {
                break;
            }
            current = next;
        }
    }
    attacks
}

/// Every subset of `mask`, starting with the empty set.
fn subsets(mask: Bitboard) -> Vec<Bitboard> {
    let mut all = Vec::with_capacity(1 << mask.popcount());
    let mut subset = Bitboard::EMPTY;
    loop {
        all.push(subset);
        // Subtracting the mask and keeping only the mask bits steps to the next
        // subset, as if the mask bits were the digits of a binary counter.
        subset = Bitboard(subset.0.wrapping_sub(mask.0) & mask.0);
        if subset.is_empty() {
            return all;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_leave_out_the_ray_ends() {
        let rook_a1 = relevant_mask(Square::A1, &ROOK_DIRECTIONS);
        assert_eq!(rook_a1.popcount(), 12);
        assert!(!rook_a1.contains(Square::A1));
        assert!(!rook_a1.contains(Square::A8) && !rook_a1.contains(Square::H1));
        assert_eq!(relevant_mask(Square::E4, &ROOK_DIRECTIONS).popcount(), 10);
        assert_eq!(relevant_mask(Square::A1, &BISHOP_DIRECTIONS).popcount(), 6);
        assert_eq!(relevant_mask(Square::E4, &BISHOP_DIRECTIONS).popcount(), 9);
    }

    #[test]
    fn subsets_are_complete_and_distinct() {
        let mask = Bitboard::from_square(Square::B2)
            .with(Square::E4)
            .with(Square::G7);
        let all = subsets(mask);
        assert_eq!(all.len(), 8);
        for (i, &a) in all.iter().enumerate() {
            assert_eq!(a & mask, a);
            assert!(all[i + 1..].iter().all(|&b| b != a));
        }
    }

    #[test]
    fn table_has_one_entry_per_arrangement() {
        // 2^(squares in mask) entries per square, summed over the board.
        assert_eq!(SLIDERS.table.len(), 5_248 + 102_400);
    }

    #[test]
    fn lookups_match_ray_walking_for_every_arrangement() {
        let sliders = &*SLIDERS;
        for sq in Square::all() {
            for occupied in subsets(relevant_mask(sq, &BISHOP_DIRECTIONS)) {
                let expected = sliding_attacks(sq, occupied, &BISHOP_DIRECTIONS);
                assert_eq!(sliders.bishop(sq, occupied), expected, "bishop {sq}");
            }
            for occupied in subsets(relevant_mask(sq, &ROOK_DIRECTIONS)) {
                let expected = sliding_attacks(sq, occupied, &ROOK_DIRECTIONS);
                assert_eq!(sliders.rook(sq, occupied), expected, "rook {sq}");
            }
        }
    }

    #[test]
    fn lookups_ignore_squares_outside_the_mask() {
        // Whole-board occupancies, dense and sparse, with pieces on the ray ends
        // and off the rays as well.
        let sliders = &*SLIDERS;
        let mut rng = Rng::new(1);
        for round in 0..500 {
            let occupied = if round % 2 == 0 {
                Bitboard(rng.next_u64())
            } else {
                Bitboard(rng.next_sparse_u64())
            };
            for sq in Square::all() {
                let bishop = sliding_attacks(sq, occupied, &BISHOP_DIRECTIONS);
                let rook = sliding_attacks(sq, occupied, &ROOK_DIRECTIONS);
                assert_eq!(sliders.bishop(sq, occupied), bishop, "bishop {sq}");
                assert_eq!(sliders.rook(sq, occupied), rook, "rook {sq}");
            }
        }
    }
}
