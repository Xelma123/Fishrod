//! The `Position` type: piece placement and game state.

use std::fmt;

use crate::types::{Bitboard, Color, Piece, PieceType, Square};

/// The side of the board a castling move goes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CastlingSide {
    /// Towards the h-file (O-O).
    King,
    /// Towards the a-file (O-O-O).
    Queen,
}

impl CastlingSide {
    /// Both sides, in index order.
    pub const ALL: [CastlingSide; 2] = [CastlingSide::King, CastlingSide::Queen];
}

/// Index into `Position::castling_rooks` for a color and a castling side.
pub(super) const fn castling_index(color: Color, side: CastlingSide) -> usize {
    color.index() * 2 + side as usize
}

/// A chess position: where the pieces stand, plus the state that decides the
/// legal moves (side to move, castling rights, en passant square) and the
/// counters for the fifty-move rule and move numbering.
///
/// Pieces are stored twice: as bitboards, for fast set operations, and as a
/// square-by-square table, for a fast answer to "what stands on this square?".
/// Both views are always updated together.
///
/// The type is small and `Copy`. Making a move produces a new position
/// (copy-make) and leaves the old one unchanged.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Position {
    /// Squares occupied by each piece type, both colors together.
    pub(super) by_type: [Bitboard; 6],
    /// Squares occupied by each color.
    pub(super) by_color: [Bitboard; 2],
    /// The piece on each square, if any.
    pub(super) board: [Option<Piece>; 64],
    pub(super) side_to_move: Color,
    /// The rook that may still castle, per color and side (see `castling_index`).
    /// `None` means the right is gone. Storing the rook's square instead of a
    /// flag lets Chess960 positions castle with rooks on any file.
    pub(super) castling_rooks: [Option<Square>; 4],
    /// The square a pawn of the side to move can capture en passant on. It is
    /// only set when such a capture is available, so two positions that allow
    /// the same moves always compare equal.
    pub(super) en_passant: Option<Square>,
    pub(super) halfmove_clock: u16,
    pub(super) fullmove_number: u16,
}

impl Position {
    /// FEN of the standard starting position.
    pub const STARTPOS_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

    /// A board with no pieces and white to move; the starting point for building a position.
    pub(super) const fn empty() -> Position {
        Position {
            by_type: [Bitboard::EMPTY; 6],
            by_color: [Bitboard::EMPTY; 2],
            board: [None; 64],
            side_to_move: Color::White,
            castling_rooks: [None; 4],
            en_passant: None,
            halfmove_clock: 0,
            fullmove_number: 1,
        }
    }

    /// The standard starting position.
    pub fn startpos() -> Position {
        Position::from_fen(Self::STARTPOS_FEN).expect("the starting position FEN is valid")
    }

    /// The side to move.
    pub fn side_to_move(&self) -> Color {
        self.side_to_move
    }

    /// The piece on `sq`, if any.
    pub fn piece_on(&self, sq: Square) -> Option<Piece> {
        self.board[sq.index()]
    }

    /// Squares holding pieces of the given type, of either color.
    pub fn pieces(&self, piece_type: PieceType) -> Bitboard {
        self.by_type[piece_type.index()]
    }

    /// Squares holding pieces of the given color and type.
    pub fn pieces_of(&self, color: Color, piece_type: PieceType) -> Bitboard {
        self.by_type[piece_type.index()] & self.by_color[color.index()]
    }

    /// Squares holding pieces of the given color.
    pub fn occupied_by(&self, color: Color) -> Bitboard {
        self.by_color[color.index()]
    }

    /// Squares holding any piece.
    pub fn occupied(&self) -> Bitboard {
        self.by_color[0] | self.by_color[1]
    }

    /// The square of the king of the given color.
    pub fn king_square(&self, color: Color) -> Square {
        self.pieces_of(color, PieceType::King).lsb()
    }

    /// The rook that may still castle on the given side, if the right remains.
    pub fn castling_rook(&self, color: Color, side: CastlingSide) -> Option<Square> {
        self.castling_rooks[castling_index(color, side)]
    }

    /// The square the side to move can capture en passant on, if any.
    pub fn en_passant(&self) -> Option<Square> {
        self.en_passant
    }

    /// Half-moves since the last capture or pawn move (for the fifty-move rule).
    pub fn halfmove_clock(&self) -> u16 {
        self.halfmove_clock
    }

    /// The move number, starting at 1 and increasing after each black move.
    pub fn fullmove_number(&self) -> u16 {
        self.fullmove_number
    }

    /// Whether the bitboards and the square table describe the same placement.
    /// Tests call this after building or changing a position.
    pub fn is_consistent(&self) -> bool {
        let mut by_type = [Bitboard::EMPTY; 6];
        let mut by_color = [Bitboard::EMPTY; 2];
        for sq in Square::all() {
            if let Some(piece) = self.piece_on(sq) {
                by_type[piece.piece_type().index()] |= Bitboard::from_square(sq);
                by_color[piece.color().index()] |= Bitboard::from_square(sq);
            }
        }
        by_type == self.by_type && by_color == self.by_color
    }

    /// Places `piece` on the empty square `sq`.
    pub(crate) fn put_piece(&mut self, piece: Piece, sq: Square) {
        debug_assert!(self.board[sq.index()].is_none(), "{sq} is occupied");
        let bit = Bitboard::from_square(sq);
        self.board[sq.index()] = Some(piece);
        self.by_type[piece.piece_type().index()] |= bit;
        self.by_color[piece.color().index()] |= bit;
    }

    /// Whether the side to move could capture en passant on `sq`: the square is
    /// empty, the enemy pawn that just skipped it stands right behind it, and
    /// one of our pawns attacks it.
    pub(super) fn can_capture_en_passant(&self, sq: Square) -> bool {
        let us = self.side_to_move;
        let target = Bitboard::from_square(sq);
        let (pushed_pawn, attackers) = match us {
            Color::White => (target.south(), target.south_west() | target.south_east()),
            Color::Black => (target.north(), target.north_west() | target.north_east()),
        };
        self.piece_on(sq).is_none()
            && !(pushed_pawn & self.pieces_of(!us, PieceType::Pawn)).is_empty()
            && !(attackers & self.pieces_of(us, PieceType::Pawn)).is_empty()
    }
}

impl fmt::Display for Position {
    /// Draws the board from white's side, followed by the FEN.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for rank in (0..8).rev() {
            write!(f, "{}", rank + 1)?;
            for file in 0..8 {
                let symbol = self
                    .piece_on(Square::from_coords(file, rank))
                    .map_or('.', Piece::to_char);
                write!(f, " {symbol}")?;
            }
            writeln!(f)?;
        }
        writeln!(f, "  a b c d e f g h")?;
        write!(f, "\nFEN: {}", self.to_fen())
    }
}

impl fmt::Debug for Position {
    /// Shows the FEN, which is far easier to read in a failing test than the raw fields.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Position(\"{}\")", self.to_fen())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_stays_small() {
        // Copy-make copies the whole position for every move, so it must stay small.
        assert!(std::mem::size_of::<Position>() <= 192);
        assert_eq!(std::mem::size_of::<Option<Piece>>(), 1);
    }

    #[test]
    fn startpos_accessors() {
        let pos = Position::startpos();
        assert_eq!(pos.side_to_move(), Color::White);
        assert_eq!(pos.piece_on(Square::E1), Some(Piece::WhiteKing));
        assert_eq!(pos.piece_on(Square::D8), Some(Piece::BlackQueen));
        assert_eq!(pos.piece_on(Square::E4), None);
        assert_eq!(pos.occupied().popcount(), 32);
        assert_eq!(
            pos.occupied_by(Color::Black),
            Bitboard::RANK_7 | Bitboard::RANK_8
        );
        assert_eq!(
            pos.pieces(PieceType::Pawn),
            Bitboard::RANK_2 | Bitboard::RANK_7
        );
        assert_eq!(
            pos.pieces_of(Color::White, PieceType::Knight),
            Bitboard::from_square(Square::B1).with(Square::G1)
        );
        assert_eq!(pos.king_square(Color::Black), Square::E8);
        assert_eq!(
            pos.castling_rook(Color::White, CastlingSide::King),
            Some(Square::H1)
        );
        assert_eq!(
            pos.castling_rook(Color::White, CastlingSide::Queen),
            Some(Square::A1)
        );
        assert_eq!(
            pos.castling_rook(Color::Black, CastlingSide::King),
            Some(Square::H8)
        );
        assert_eq!(
            pos.castling_rook(Color::Black, CastlingSide::Queen),
            Some(Square::A8)
        );
        assert_eq!(pos.en_passant(), None);
        assert_eq!((pos.halfmove_clock(), pos.fullmove_number()), (0, 1));
        assert!(pos.is_consistent());
    }

    #[test]
    fn display_shows_board_and_fen() {
        let expected = "\
8 r n b q k b n r
7 p p p p p p p p
6 . . . . . . . .
5 . . . . . . . .
4 . . . . . . . .
3 . . . . . . . .
2 P P P P P P P P
1 R N B Q K B N R
  a b c d e f g h

FEN: rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
        assert_eq!(Position::startpos().to_string(), expected);
    }
}
