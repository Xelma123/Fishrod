//! Attack tables: the squares a piece attacks from a given square.
//!
//! Knights, kings and pawns always attack the same squares from a given
//! square, so their attacks are computed at compile time into tables indexed
//! by square. Bishops, rooks and queens slide until a piece blocks them, so
//! their attacks also depend on which squares are occupied; they are looked up
//! in magic bitboard tables (see the `magic` module).
//!
//! Attack sets include occupied squares of either color. Callers remove their
//! own pieces when they need moves rather than attacks.

use super::magic::SLIDERS;
use crate::types::{Bitboard, Color, Square};

/// Knight jumps as (file step, rank step).
const KNIGHT_STEPS: [(i8, i8); 8] = [
    (1, 2),
    (2, 1),
    (2, -1),
    (1, -2),
    (-1, -2),
    (-2, -1),
    (-2, 1),
    (-1, 2),
];

/// King steps to the eight neighbouring squares.
const KING_STEPS: [(i8, i8); 8] = [
    (0, 1),
    (1, 1),
    (1, 0),
    (1, -1),
    (0, -1),
    (-1, -1),
    (-1, 0),
    (-1, 1),
];

/// Pawn captures, per color: one step diagonally forward, which is towards
/// rank 8 for white and towards rank 1 for black.
const PAWN_STEPS: [[(i8, i8); 2]; 2] = [[(-1, 1), (1, 1)], [(-1, -1), (1, -1)]];

static KNIGHT_ATTACKS: [Bitboard; 64] = leaper_table(&KNIGHT_STEPS);
static KING_ATTACKS: [Bitboard; 64] = leaper_table(&KING_STEPS);
static PAWN_ATTACKS: [[Bitboard; 64]; 2] =
    [leaper_table(&PAWN_STEPS[0]), leaper_table(&PAWN_STEPS[1])];

/// Builds the attack table of a piece that moves by fixed steps, leaving out
/// steps that would leave the board. Runs at compile time.
const fn leaper_table(steps: &[(i8, i8)]) -> [Bitboard; 64] {
    let mut table = [Bitboard::EMPTY; 64];
    // `for` loops are not allowed in `const fn` yet, so these are `while` loops.
    let mut index = 0;
    while index < 64 {
        let from = Square::new(index as u8);
        let mut attacks = Bitboard::EMPTY;
        let mut i = 0;
        while i < steps.len() {
            let (file_step, rank_step) = steps[i];
            if let Some(to) = from.offset(file_step, rank_step) {
                attacks = attacks.with(to);
            }
            i += 1;
        }
        table[index] = attacks;
        index += 1;
    }
    table
}

/// Squares a pawn of `color` on `sq` attacks.
///
/// Read the other way round, `pawn_attacks(!color, sq)` holds the squares from
/// which a pawn of `color` would attack `sq`.
pub fn pawn_attacks(color: Color, sq: Square) -> Bitboard {
    PAWN_ATTACKS[color.index()][sq.index()]
}

/// Squares a knight on `sq` attacks.
pub fn knight_attacks(sq: Square) -> Bitboard {
    KNIGHT_ATTACKS[sq.index()]
}

/// Squares a king on `sq` attacks.
pub fn king_attacks(sq: Square) -> Bitboard {
    KING_ATTACKS[sq.index()]
}

/// Squares a bishop on `sq` attacks, given the occupied squares of the board.
/// Each diagonal ends at the first occupied square, which is included.
pub fn bishop_attacks(sq: Square, occupied: Bitboard) -> Bitboard {
    SLIDERS.bishop(sq, occupied)
}

/// Squares a rook on `sq` attacks, given the occupied squares of the board.
/// Each line ends at the first occupied square, which is included.
pub fn rook_attacks(sq: Square, occupied: Bitboard) -> Bitboard {
    SLIDERS.rook(sq, occupied)
}

/// Squares a queen on `sq` attacks: the union of bishop and rook attacks.
pub fn queen_attacks(sq: Square, occupied: Bitboard) -> Bitboard {
    let sliders = &*SLIDERS;
    sliders.bishop(sq, occupied) | sliders.rook(sq, occupied)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(squares: &[Square]) -> Bitboard {
        squares
            .iter()
            .fold(Bitboard::EMPTY, |set, &sq| set.with(sq))
    }

    #[test]
    fn knight_attacks_examples() {
        use Square as S;
        assert_eq!(knight_attacks(S::A1), set(&[S::B3, S::C2]));
        assert_eq!(knight_attacks(S::B1), set(&[S::A3, S::C3, S::D2]));
        assert_eq!(knight_attacks(S::H8), set(&[S::G6, S::F7]));
        assert_eq!(
            knight_attacks(S::E4),
            set(&[S::D6, S::F6, S::G5, S::G3, S::F2, S::D2, S::C3, S::C5])
        );
    }

    #[test]
    fn king_attacks_examples() {
        use Square as S;
        assert_eq!(king_attacks(S::A1), set(&[S::A2, S::B1, S::B2]));
        assert_eq!(
            king_attacks(S::H5),
            set(&[S::H4, S::H6, S::G4, S::G5, S::G6])
        );
        assert_eq!(king_attacks(S::E4).popcount(), 8);
    }

    #[test]
    fn pawn_attacks_examples() {
        use Square as S;
        assert_eq!(pawn_attacks(Color::White, S::E4), set(&[S::D5, S::F5]));
        assert_eq!(pawn_attacks(Color::White, S::A2), set(&[S::B3]));
        assert_eq!(pawn_attacks(Color::White, S::H7), set(&[S::G8]));
        assert_eq!(pawn_attacks(Color::Black, S::E5), set(&[S::D4, S::F4]));
        assert_eq!(pawn_attacks(Color::Black, S::A7), set(&[S::B6]));
        assert!(pawn_attacks(Color::White, S::C8).is_empty());
        assert!(pawn_attacks(Color::Black, S::C1).is_empty());
    }

    #[test]
    fn leaper_tables_have_the_expected_totals() {
        // Summed over the 64 squares; pieces near the edges attack fewer squares.
        let total = |attacks: fn(Square) -> Bitboard| -> u32 {
            Square::all().map(|sq| attacks(sq).popcount()).sum()
        };
        assert_eq!(total(knight_attacks), 336);
        assert_eq!(total(king_attacks), 420);
        assert_eq!(total(|sq| pawn_attacks(Color::White, sq)), 98);
        assert_eq!(total(|sq| pawn_attacks(Color::Black, sq)), 98);
    }

    #[test]
    fn leaper_attacks_are_symmetric() {
        // A knight on `a` attacks `b` exactly when a knight on `b` attacks `a`.
        // The same holds for kings, and for pawns with the colors swapped.
        for a in Square::all() {
            for b in Square::all() {
                assert_eq!(knight_attacks(a).contains(b), knight_attacks(b).contains(a));
                assert_eq!(king_attacks(a).contains(b), king_attacks(b).contains(a));
                assert_eq!(
                    pawn_attacks(Color::White, a).contains(b),
                    pawn_attacks(Color::Black, b).contains(a)
                );
            }
        }
    }

    #[test]
    fn slider_attacks_stop_at_the_first_blocker() {
        use Square as S;
        let occupied = set(&[S::E6, S::C4, S::E2, S::B7, S::G2]);
        let rook = set(&[
            S::E5,
            S::E6,
            S::E3,
            S::E2,
            S::D4,
            S::C4,
            S::F4,
            S::G4,
            S::H4,
        ]);
        let bishop = set(&[
            S::F5,
            S::G6,
            S::H7,
            S::D5,
            S::C6,
            S::B7,
            S::F3,
            S::G2,
            S::D3,
            S::C2,
            S::B1,
        ]);
        assert_eq!(rook_attacks(S::E4, occupied), rook);
        assert_eq!(bishop_attacks(S::E4, occupied), bishop);
        assert_eq!(queen_attacks(S::E4, occupied), rook | bishop);
    }

    #[test]
    fn empty_board_slider_attacks() {
        let mut bishop_total = 0;
        for sq in Square::all() {
            let rook = rook_attacks(sq, Bitboard::EMPTY);
            let bishop = bishop_attacks(sq, Bitboard::EMPTY);
            // A rook sees its whole file and rank, minus its own square.
            assert_eq!(
                rook,
                (Bitboard::file(sq.file()) | Bitboard::rank(sq.rank())).without(sq)
            );
            assert_eq!(queen_attacks(sq, Bitboard::EMPTY), rook | bishop);
            bishop_total += bishop.popcount();
        }
        assert_eq!(bishop_total, 560);
    }
}
