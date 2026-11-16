//! Material values and piece-square tables.
//!
//! A piece-square table gives a bonus or penalty for a piece standing on a
//! given square: a knight in the centre is worth more than one on the rim.
//! Each table has a middlegame and an endgame version. The tables here are
//! not typed in square by square: they are computed at compile time from a
//! few plain rules, written out in `bonus` below, so each number can be
//! explained and the rules can be tuned later.

use crate::types::{Color, PieceType, Square};

/// A middlegame and an endgame value, in centipawns.
pub(super) type Pair = (i32, i32);

/// Material value of each piece type, pawn to king, middlegame and endgame.
/// The king has no material value: it can never be traded.
const MATERIAL: [Pair; 6] = [
    (100, 120),
    (320, 300),
    (330, 320),
    (480, 520),
    (950, 950),
    (0, 0),
];

/// Material plus square bonus for each piece type and square, from white's
/// side of the board.
static TABLES: [[Pair; 64]; 6] = build_tables();

const fn build_tables() -> [[Pair; 64]; 6] {
    let mut tables = [[(0, 0); 64]; 6];
    let mut piece = 0;
    while piece < 6 {
        let mut sq = 0;
        while sq < 64 {
            let (mg, eg) = bonus(piece, (sq % 8) as i32, (sq / 8) as i32);
            tables[piece][sq] = (MATERIAL[piece].0 + mg, MATERIAL[piece].1 + eg);
            sq += 1;
        }
        piece += 1;
    }
    tables
}

/// The square bonus for a piece type (as an index, pawn = 0) on `file` and
/// `rank`, seen from white's side: rank 0 is the piece's own back rank.
const fn bonus(piece: usize, file: i32, rank: i32) -> Pair {
    // 0 in the corners up to 6 on the four centre squares.
    let centrality = 7 - ((2 * file - 7).abs() + (2 * rank - 7).abs()) / 2;
    // 0 on the a- and h-files up to 3 on the d- and e-files.
    let from_edge = if file < 7 - file { file } else { 7 - file };

    match piece {
        // Pawns gain value as they advance, much more so in the endgame,
        // where they head for promotion. Centre pawns are worth a little
        // extra early on.
        0 => {
            let advance = if rank > 1 { rank - 1 } else { 0 };
            let centre = if from_edge == 3 && matches!(rank, 2..=4) {
                10
            } else {
                0
            };
            (2 * advance * advance + centre, 4 * advance * advance)
        }
        // Knights reach the fewest squares from the rim.
        1 => (6 * centrality - 18, 4 * centrality - 12),
        // Bishops like the centre too, and should leave the back rank.
        2 => {
            let undeveloped = if rank == 0 { 10 } else { 0 };
            (4 * centrality - 12 - undeveloped, 3 * centrality - 9)
        }
        // Rooks like the seventh rank and, early on, the open centre files.
        3 => {
            let seventh = if rank == 6 { 15 } else { 0 };
            let centre_file = if from_edge == 3 { 5 } else { 0 };
            (seventh + centre_file, seventh * 2 / 3)
        }
        // The queen is mildly drawn to the centre, more so in the endgame.
        4 => (2 * centrality - 6, 4 * centrality - 12),
        // In the middlegame the king hides behind its pawns on the back rank,
        // best on the b- or g-file where it lands after castling. In the
        // endgame it becomes a fighting piece and heads for the centre.
        _ => {
            let file_safety = match from_edge {
                0 => 8,
                1 => 14,
                2 => 0,
                _ => -12,
            };
            (file_safety - 20 * rank, 6 * centrality - 18)
        }
    }
}

/// Material plus square bonus of a piece of `color` and `piece_type` on `sq`,
/// from that piece's own point of view.
pub(super) fn value(color: Color, piece_type: PieceType, sq: Square) -> Pair {
    // Black's tables are white's turned upside down: flipping the rank of a
    // square is an XOR of its index with 56.
    let index = match color {
        Color::White => sq.index(),
        Color::Black => sq.index() ^ 56,
    };
    TABLES[piece_type.index()][index]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn white(piece_type: PieceType, sq: Square) -> Pair {
        value(Color::White, piece_type, sq)
    }

    #[test]
    fn black_tables_mirror_white_tables() {
        for piece_type in PieceType::ALL {
            for sq in Square::all() {
                let mirrored = Square::from_coords(sq.file(), 7 - sq.rank());
                assert_eq!(
                    value(Color::Black, piece_type, mirrored),
                    white(piece_type, sq)
                );
            }
        }
    }

    #[test]
    fn tables_are_left_right_symmetric() {
        for piece_type in PieceType::ALL {
            for sq in Square::all() {
                let mirrored = Square::from_coords(7 - sq.file(), sq.rank());
                assert_eq!(white(piece_type, mirrored), white(piece_type, sq));
            }
        }
    }

    #[test]
    fn rules_point_the_right_way() {
        use PieceType as P;
        use Square as S;
        // A centralised knight beats a knight in the corner.
        assert!(white(P::Knight, S::D4).0 > white(P::Knight, S::A1).0);
        // Advanced pawns are worth more, above all in the endgame.
        assert!(white(P::Pawn, S::E6).1 > white(P::Pawn, S::E3).1);
        assert!(white(P::Pawn, S::A7).1 - white(P::Pawn, S::A2).1 >= 100);
        // A castled king is safer than one left in the centre, but in the
        // endgame the centre is where the king belongs.
        assert!(white(P::King, S::G1).0 > white(P::King, S::E1).0);
        assert!(white(P::King, S::E2).0 < white(P::King, S::E1).0);
        assert!(white(P::King, S::D5).1 > white(P::King, S::H1).1);
        // A rook on the seventh rank.
        assert!(white(P::Rook, S::C7).0 > white(P::Rook, S::C6).0);
        // Material order.
        assert!(white(P::Queen, S::A1).0 > white(P::Rook, S::H8).0);
        assert!(white(P::Rook, S::A1).0 > white(P::Bishop, S::A1).0);
    }
}
