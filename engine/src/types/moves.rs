//! Moves and move lists.

use std::fmt;
use std::ops::{Deref, DerefMut};

use super::{PieceType, Square};

/// What kind of move a [`Move`] is, beyond its two squares.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MoveKind {
    /// Any move not listed below, captures included.
    Normal,
    /// A pawn advancing two squares from its starting rank.
    DoublePush,
    /// A pawn capturing a pawn that has just advanced two squares past it.
    EnPassant,
    /// Castling. The move goes from the king's square to the castling rook's
    /// square, which names the castle without ambiguity in Chess960 too.
    Castle,
    /// A pawn reaching the last rank and turning into the given piece.
    Promotion(PieceType),
}

// Values of the kind bits.
const NORMAL: u16 = 0;
const DOUBLE_PUSH: u16 = 1;
const EN_PASSANT: u16 = 2;
const CASTLE: u16 = 3;
/// Promotions use 4 to 7: knight, bishop, rook, queen.
const PROMOTION: u16 = 4;

/// A move packed into 16 bits: origin square (bits 0-5), destination square
/// (bits 6-11) and kind (bits 12-15).
///
/// The move does not record the moving or the captured piece; the position it
/// is played in tells those.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Move(u16);

impl Move {
    /// A placeholder that is never a legal move (a1 to a1).
    pub const NULL: Move = Move(0);

    /// Packs a move. A promotion must be to a knight, bishop, rook or queen.
    pub const fn new(from: Square, to: Square, kind: MoveKind) -> Move {
        let kind_bits = match kind {
            MoveKind::Normal => NORMAL,
            MoveKind::DoublePush => DOUBLE_PUSH,
            MoveKind::EnPassant => EN_PASSANT,
            MoveKind::Castle => CASTLE,
            MoveKind::Promotion(piece_type) => {
                debug_assert!(
                    matches!(
                        piece_type,
                        PieceType::Knight | PieceType::Bishop | PieceType::Rook | PieceType::Queen
                    ),
                    "invalid promotion piece"
                );
                // Knight (index 1) becomes 4, ..., queen (index 4) becomes 7.
                PROMOTION + piece_type as u16 - 1
            }
        };
        Move(from.index() as u16 | (to.index() as u16) << 6 | kind_bits << 12)
    }

    /// The square the move starts from. For castling, the king's square.
    pub const fn from(self) -> Square {
        Square::new((self.0 & 0x3F) as u8)
    }

    /// The square the move goes to. For castling, the castling rook's square.
    pub const fn to(self) -> Square {
        Square::new((self.0 >> 6 & 0x3F) as u8)
    }

    /// The kind of the move.
    pub const fn kind(self) -> MoveKind {
        match self.0 >> 12 {
            NORMAL => MoveKind::Normal,
            DOUBLE_PUSH => MoveKind::DoublePush,
            EN_PASSANT => MoveKind::EnPassant,
            CASTLE => MoveKind::Castle,
            bits => MoveKind::Promotion(PieceType::ALL[(bits - PROMOTION + 1) as usize]),
        }
    }

    /// The move in UCI notation, such as `e2e4` or `e7e8q`.
    ///
    /// Castling is written as the king's own move (`e1g1`) in standard chess,
    /// and as the king taking its own rook (`e1h1`) when `chess960` is set, as
    /// the UCI protocol asks.
    pub fn to_uci(self, chess960: bool) -> String {
        let from = self.from();
        let mut to = self.to();
        if self.kind() == MoveKind::Castle && !chess960 {
            let king_file = if to.file() > from.file() { 6 } else { 2 };
            to = Square::from_coords(king_file, from.rank());
        }
        match self.kind() {
            MoveKind::Promotion(piece_type) => format!("{from}{to}{}", piece_type.to_char()),
            _ => format!("{from}{to}"),
        }
    }
}

impl fmt::Debug for Move {
    /// Shows the move in Chess960 UCI notation together with its kind, such as `e1h1 Castle`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {:?}", self.to_uci(true), self.kind())
    }
}

/// Room for the moves of any position: no position is known to have more than
/// 218 legal moves.
const MAX_MOVES: usize = 256;

/// A list of moves kept in a fixed-size array, so building one never touches
/// the heap. It reads like a slice: `len()`, `iter()`, indexing and so on.
#[derive(Clone)]
pub struct MoveList {
    moves: [Move; MAX_MOVES],
    len: usize,
}

impl MoveList {
    /// The most moves a list can hold.
    pub const CAPACITY: usize = MAX_MOVES;

    /// An empty list.
    pub const fn new() -> MoveList {
        MoveList {
            moves: [Move::NULL; MAX_MOVES],
            len: 0,
        }
    }

    /// Adds a move at the end.
    pub fn push(&mut self, mv: Move) {
        self.moves[self.len] = mv;
        self.len += 1;
    }

    /// Removes every move.
    pub fn clear(&mut self) {
        self.len = 0;
    }
}

impl Default for MoveList {
    fn default() -> MoveList {
        MoveList::new()
    }
}

impl Deref for MoveList {
    type Target = [Move];

    /// The filled part of the array, as a slice.
    fn deref(&self) -> &[Move] {
        &self.moves[..self.len]
    }
}

impl DerefMut for MoveList {
    /// The filled part of the array, as a slice that can be reordered.
    fn deref_mut(&mut self) -> &mut [Move] {
        &mut self.moves[..self.len]
    }
}

impl<'a> IntoIterator for &'a MoveList {
    type Item = &'a Move;
    type IntoIter = std::slice::Iter<'a, Move>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl fmt::Debug for MoveList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KINDS: [MoveKind; 8] = [
        MoveKind::Normal,
        MoveKind::DoublePush,
        MoveKind::EnPassant,
        MoveKind::Castle,
        MoveKind::Promotion(PieceType::Knight),
        MoveKind::Promotion(PieceType::Bishop),
        MoveKind::Promotion(PieceType::Rook),
        MoveKind::Promotion(PieceType::Queen),
    ];

    #[test]
    fn fields_survive_packing() {
        for from in Square::all() {
            for to in Square::all() {
                for kind in KINDS {
                    let mv = Move::new(from, to, kind);
                    assert_eq!((mv.from(), mv.to(), mv.kind()), (from, to, kind));
                }
            }
        }
        assert_eq!(std::mem::size_of::<Move>(), 2);
    }

    #[test]
    fn uci_notation() {
        let e2e4 = Move::new(Square::E2, Square::E4, MoveKind::DoublePush);
        assert_eq!(e2e4.to_uci(false), "e2e4");
        let promotion = Move::new(
            Square::B7,
            Square::A8,
            MoveKind::Promotion(PieceType::Queen),
        );
        assert_eq!(promotion.to_uci(false), "b7a8q");
        let underpromotion = Move::new(
            Square::E2,
            Square::E1,
            MoveKind::Promotion(PieceType::Knight),
        );
        assert_eq!(underpromotion.to_uci(false), "e2e1n");

        let short = Move::new(Square::E1, Square::H1, MoveKind::Castle);
        assert_eq!(short.to_uci(false), "e1g1");
        assert_eq!(short.to_uci(true), "e1h1");
        let long = Move::new(Square::E8, Square::A8, MoveKind::Castle);
        assert_eq!(long.to_uci(false), "e8c8");
        assert_eq!(long.to_uci(true), "e8a8");
    }

    #[test]
    fn move_list_acts_like_a_slice() {
        let mut list = MoveList::new();
        assert!(list.is_empty());
        let a = Move::new(Square::G1, Square::F3, MoveKind::Normal);
        let b = Move::new(Square::E2, Square::E4, MoveKind::DoublePush);
        list.push(a);
        list.push(b);
        assert_eq!(list.len(), 2);
        assert_eq!(list[1], b);
        assert!(list.contains(&a));
        assert_eq!(list.iter().copied().collect::<Vec<_>>(), [a, b]);
    }
}
