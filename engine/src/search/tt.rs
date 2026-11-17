//! The transposition table: a large hash table of search results.
//!
//! The same position is often reached through different move orders (a
//! transposition), and iterative deepening searches every position again at
//! each new depth. The table remembers, for each position searched, the best
//! move found and the score together with how deep it was searched. A later
//! visit takes the score as it is when it was searched deeply enough, and
//! otherwise still tries the remembered move first, which makes cutoffs come
//! early.
//!
//! A position is found by its Zobrist key: the key picks a bucket of four
//! entries, and 16 more bits of it, kept in the entry, tell apart the
//! positions that share a bucket. Each entry is a single atomic 64-bit word,
//! so an entry is never seen half-written and the table can later be shared
//! by several search threads without locks.

use std::collections::TryReserveError;
use std::mem::size_of;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

use super::{MATE, MAX_PLY};
use crate::types::Move;

/// How a stored score relates to the true score of its position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Bound {
    /// The score is exact: it fell inside the search window.
    Exact,
    /// The true score is at least this: a move reached beta and cut off.
    Lower,
    /// The true score is at most this: no move reached alpha.
    Upper,
}

/// What the table knows about a position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    /// The best move found, if the search found one.
    pub(crate) mv: Option<Move>,
    pub(crate) score: i32,
    /// How many plies deep the position was searched.
    pub(crate) depth: i32,
    pub(crate) bound: Bound,
}

/// Entries per bucket.
const BUCKET_SIZE: usize = 4;
/// Ages count searches modulo this; an entry stores the age of its search.
const AGE_CYCLE: u8 = 64;
/// Scores at least this far from zero announce a forced mate.
const MATE_BOUND: i32 = MATE - MAX_PLY as i32;
const MIB: usize = 1 << 20;

/// Four entries side by side, aligned so that a bucket never straddles two
/// cache lines: a lookup then reads memory only once.
#[derive(Default)]
#[repr(align(32))]
struct Bucket([AtomicU64; BUCKET_SIZE]);

/// A hash table from positions to search results, shared by reference.
pub struct TranspositionTable {
    buckets: Vec<Bucket>,
    /// The age of the current search.
    age: AtomicU8,
}

impl TranspositionTable {
    /// An empty table taking `megabytes` MiB of memory, or the error if the
    /// memory cannot be had.
    pub fn new(megabytes: usize) -> Result<TranspositionTable, TryReserveError> {
        let count = (megabytes.saturating_mul(MIB) / size_of::<Bucket>()).max(1);
        let mut buckets = Vec::new();
        buckets.try_reserve_exact(count)?;
        buckets.resize_with(count, Bucket::default);
        Ok(TranspositionTable {
            buckets,
            age: AtomicU8::new(0),
        })
    }

    /// The number of entries the table holds.
    pub fn capacity(&self) -> usize {
        self.buckets.len() * BUCKET_SIZE
    }

    /// Empties the table, as for a new game.
    pub fn clear(&self) {
        for cell in self.buckets.iter().flat_map(|bucket| &bucket.0) {
            cell.store(0, Ordering::Relaxed);
        }
        self.age.store(0, Ordering::Relaxed);
    }

    /// Starts a new search: entries of earlier searches grow older, which
    /// lets the new search's entries replace them sooner.
    pub(crate) fn new_search(&self) {
        let age = self.age.load(Ordering::Relaxed);
        self.age.store((age + 1) % AGE_CYCLE, Ordering::Relaxed);
    }

    /// The entry for the position with `key`, if the table has one. `ply` is
    /// the distance of the position from the root of the search.
    pub(crate) fn probe(&self, key: u64, ply: usize) -> Option<Entry> {
        let check = key as u16;
        self.bucket(key)
            .0
            .iter()
            .map(|cell| cell.load(Ordering::Relaxed))
            .find(|&raw| raw != EMPTY && Packed(raw).check() == check)
            .map(|raw| Packed(raw).unpack(ply))
    }

    /// Records the result of searching the position with `key` at `ply`.
    ///
    /// The position's own entry is overwritten, keeping its move if the new
    /// result has none. Otherwise an empty entry is taken, or else the one
    /// least worth keeping: the shallowest, counting older searches' entries
    /// as shallower still.
    pub(crate) fn store(&self, key: u64, entry: Entry, ply: usize) {
        let check = key as u16;
        let age = self.age.load(Ordering::Relaxed);
        let cells = &self.bucket(key).0;

        let mut target = &cells[0];
        let mut target_worth = i32::MAX;
        for cell in cells {
            let raw = cell.load(Ordering::Relaxed);
            let old = Packed(raw);
            if raw != EMPTY && old.check() == check {
                let entry = Entry {
                    mv: entry.mv.or(old.mv()),
                    ..entry
                };
                cell.store(Packed::pack(check, entry, ply, age).0, Ordering::Relaxed);
                return;
            }
            let worth = if raw == EMPTY {
                i32::MIN
            } else {
                let searches_ago = (age + AGE_CYCLE - old.age()) % AGE_CYCLE;
                old.depth() - 8 * i32::from(searches_ago)
            };
            if worth < target_worth {
                target = cell;
                target_worth = worth;
            }
        }
        target.store(Packed::pack(check, entry, ply, age).0, Ordering::Relaxed);
    }

    /// How full the table is, in permille, for the GUI. Counts the entries
    /// of the current search among the first thousand.
    pub fn hashfull(&self) -> usize {
        let age = self.age.load(Ordering::Relaxed);
        let sample = &self.buckets[..self.buckets.len().min(1000 / BUCKET_SIZE)];
        let used = sample
            .iter()
            .flat_map(|bucket| &bucket.0)
            .map(|cell| cell.load(Ordering::Relaxed))
            .filter(|&raw| raw != EMPTY && Packed(raw).age() == age)
            .count();
        used * 1000 / (sample.len() * BUCKET_SIZE)
    }

    /// The bucket for `key`. Multiplying the key by the bucket count and
    /// keeping the top 64 bits maps keys evenly onto any table size, without
    /// a division (Daniel Lemire's "fast alternative to the modulo
    /// reduction").
    fn bucket(&self, key: u64) -> &Bucket {
        let index = (u128::from(key) * self.buckets.len() as u128) >> 64;
        &self.buckets[index as usize]
    }
}

/// The bits of an empty entry. A stored entry is never all zero: its bound
/// bits are not.
const EMPTY: u64 = 0;

/// An entry packed into 64 bits:
///
/// | bits  | field                                         |
/// |-------|-----------------------------------------------|
/// | 0-15  | the low 16 bits of the position's key         |
/// | 16-31 | the best move; 0 (`Move::NULL`) for none      |
/// | 32-47 | the score, as a signed 16-bit number          |
/// | 48-55 | the depth                                     |
/// | 56-57 | the bound: 1 exact, 2 lower, 3 upper          |
/// | 58-63 | the age of the search that stored it          |
#[derive(Clone, Copy)]
struct Packed(u64);

impl Packed {
    fn pack(check: u16, entry: Entry, ply: usize, age: u8) -> Packed {
        let score = score_to_table(entry.score, ply);
        debug_assert!(i16::try_from(score).is_ok(), "score {score} out of range");
        debug_assert!((0..=i32::from(u8::MAX)).contains(&entry.depth));
        let mv = entry.mv.unwrap_or(Move::NULL).to_bits();
        let bound: u64 = match entry.bound {
            Bound::Exact => 1,
            Bound::Lower => 2,
            Bound::Upper => 3,
        };
        Packed(
            u64::from(check)
                | u64::from(mv) << 16
                | u64::from(score as i16 as u16) << 32
                | (entry.depth as u64 & 0xFF) << 48
                | bound << 56
                | u64::from(age % AGE_CYCLE) << 58,
        )
    }

    fn unpack(self, ply: usize) -> Entry {
        let bound = match self.0 >> 56 & 3 {
            1 => Bound::Exact,
            2 => Bound::Lower,
            _ => Bound::Upper,
        };
        Entry {
            mv: self.mv(),
            score: score_from_table(i32::from((self.0 >> 32) as u16 as i16), ply),
            depth: self.depth(),
            bound,
        }
    }

    fn check(self) -> u16 {
        self.0 as u16
    }

    fn mv(self) -> Option<Move> {
        let mv = Move::from_bits((self.0 >> 16) as u16);
        (mv != Move::NULL).then_some(mv)
    }

    fn depth(self) -> i32 {
        (self.0 >> 48 & 0xFF) as i32
    }

    fn age(self) -> u8 {
        (self.0 >> 58) as u8
    }
}

/// Mate scores count plies from the root ("mate in 7 plies from here"), but a
/// stored position can be reached again at another distance from the root.
/// So the table counts them from the stored position itself instead.
fn score_to_table(score: i32, ply: usize) -> i32 {
    if score >= MATE_BOUND {
        score + ply as i32
    } else if score <= -MATE_BOUND {
        score - ply as i32
    } else {
        score
    }
}

/// Undoes `score_to_table` for a position at `ply`.
fn score_from_table(score: i32, ply: usize) -> i32 {
    if score >= MATE_BOUND {
        score - ply as i32
    } else if score <= -MATE_BOUND {
        score + ply as i32
    } else {
        score
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{MoveKind, PieceType, Square};

    fn table() -> TranspositionTable {
        TranspositionTable::new(1).unwrap()
    }

    fn entry(mv: Option<Move>, score: i32, depth: i32, bound: Bound) -> Entry {
        Entry {
            mv,
            score,
            depth,
            bound,
        }
    }

    /// Keys that land in the same bucket of a 1 MiB table but are different
    /// positions to it: they differ only in their low bits.
    fn same_bucket_keys() -> [u64; 6] {
        std::array::from_fn(|i| 0x1234_5678_9ABC_0000 + i as u64)
    }

    #[test]
    fn sizes() {
        assert_eq!(size_of::<Bucket>(), 32);
        assert_eq!(table().capacity(), MIB / 8);
        assert_eq!(
            TranspositionTable::new(16).unwrap().capacity(),
            16 * MIB / 8
        );
    }

    #[test]
    fn entries_pack_and_unpack() {
        let moves = [
            None,
            Some(Move::new(Square::E2, Square::E4, MoveKind::DoublePush)),
            Some(Move::new(
                Square::A7,
                Square::B8,
                MoveKind::Promotion(PieceType::Queen),
            )),
        ];
        for mv in moves {
            for score in [0, 1, -1, 731, -2500, MATE - 10, -(MATE - 3)] {
                for depth in [1, 9, 127] {
                    for bound in [Bound::Exact, Bound::Lower, Bound::Upper] {
                        for age in [0, 1, 63] {
                            let stored = entry(mv, score, depth, bound);
                            let packed = Packed::pack(0xBEEF, stored, 0, age);
                            assert_ne!(packed.0, EMPTY);
                            assert_eq!(packed.check(), 0xBEEF);
                            assert_eq!(packed.age(), age);
                            assert_eq!(packed.unpack(0), stored);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn stored_entries_are_found_again() {
        let tt = table();
        let mv = Some(Move::new(Square::G1, Square::F3, MoveKind::Normal));
        assert_eq!(tt.probe(42, 0), None);
        tt.store(42, entry(mv, 35, 6, Bound::Lower), 0);
        assert_eq!(tt.probe(42, 0), Some(entry(mv, 35, 6, Bound::Lower)));
        // Ordinary scores do not depend on the ply.
        assert_eq!(tt.probe(42, 9).unwrap().score, 35);
        // Same bucket, different position.
        assert_eq!(tt.probe(43, 0), None);

        tt.clear();
        assert_eq!(tt.probe(42, 0), None);
    }

    #[test]
    fn mate_scores_are_stored_relative_to_their_position() {
        let tt = table();
        // Found 3 plies from the root: mate 5 plies after that.
        tt.store(1, entry(None, MATE - 8, 4, Bound::Exact), 3);
        // Reached again 1 ply from the root, it is mate in 6 plies.
        assert_eq!(tt.probe(1, 1).unwrap().score, MATE - 6);
        tt.store(2, entry(None, -(MATE - 8), 4, Bound::Exact), 3);
        assert_eq!(tt.probe(2, 1).unwrap().score, -(MATE - 6));
    }

    #[test]
    fn a_position_keeps_its_move_when_stored_again_without_one() {
        let tt = table();
        let mv = Some(Move::new(Square::D2, Square::D4, MoveKind::DoublePush));
        tt.store(7, entry(mv, 20, 3, Bound::Exact), 0);
        tt.store(7, entry(None, -40, 5, Bound::Upper), 0);
        assert_eq!(tt.probe(7, 0), Some(entry(mv, -40, 5, Bound::Upper)));
    }

    #[test]
    fn the_least_valuable_entry_is_replaced() {
        let tt = table();
        let keys = same_bucket_keys();
        for (&key, depth) in keys.iter().zip([5, 3, 7, 2]) {
            tt.store(key, entry(None, 0, depth, Bound::Exact), 0);
        }
        // A full bucket: the shallowest entry goes.
        tt.store(keys[4], entry(None, 0, 1, Bound::Exact), 0);
        assert_eq!(tt.probe(keys[3], 0), None);
        for key in [keys[0], keys[1], keys[2], keys[4]] {
            assert!(tt.probe(key, 0).is_some());
        }

        // Entries of an earlier search count as shallower: the old depth-3
        // entry goes, not the new search's depth-1 entry.
        tt.new_search();
        tt.store(keys[4], entry(None, 0, 1, Bound::Exact), 0);
        tt.store(keys[5], entry(None, 0, 2, Bound::Exact), 0);
        assert_eq!(tt.probe(keys[1], 0), None);
        assert!(tt.probe(keys[4], 0).is_some());
        assert!(tt.probe(keys[5], 0).is_some());
    }

    #[test]
    fn hashfull_counts_the_current_search() {
        let tt = table();
        assert_eq!(tt.hashfull(), 0);
        // Fill the first buckets: keys spread evenly over the table.
        let step = u64::MAX / tt.buckets.len() as u64 + 1;
        for i in 0..250u64 {
            for slot in 0..4 {
                tt.store(i * step + slot, entry(None, 0, 1, Bound::Exact), 0);
            }
        }
        assert_eq!(tt.hashfull(), 1000);
        tt.new_search();
        assert_eq!(tt.hashfull(), 0);
    }
}
