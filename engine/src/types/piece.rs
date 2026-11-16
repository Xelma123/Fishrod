//! Colors, piece types and colored pieces.

use std::fmt;
use std::ops::Not;

/// One of the two sides.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Color {
    White,
    Black,
}

impl Color {
    /// Both colors, in index order.
    pub const ALL: [Color; 2] = [Color::White, Color::Black];

    /// Index for array lookups: 0 for white, 1 for black.
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The opposing color.
    pub const fn flip(self) -> Color {
        match self {
            Color::White => Color::Black,
            Color::Black => Color::White,
        }
    }

    /// The rank this color's pieces start on: 0 (first rank) for white, 7 for black.
    pub const fn back_rank(self) -> u8 {
        match self {
            Color::White => 0,
            Color::Black => 7,
        }
    }
}

impl Not for Color {
    type Output = Color;

    /// `!color` is the opposing color.
    fn not(self) -> Color {
        self.flip()
    }
}

/// The kind of a piece, without its color.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PieceType {
    Pawn,
    Knight,
    Bishop,
    Rook,
    Queen,
    King,
}

impl PieceType {
    /// All piece types, in index order.
    pub const ALL: [PieceType; 6] = [
        PieceType::Pawn,
        PieceType::Knight,
        PieceType::Bishop,
        PieceType::Rook,
        PieceType::Queen,
        PieceType::King,
    ];

    /// Index for array lookups: 0 for pawns up to 5 for kings.
    pub const fn index(self) -> usize {
        self as usize
    }

    /// Lowercase letter of the piece type, as used in FEN and UCI.
    pub const fn to_char(self) -> char {
        match self {
            PieceType::Pawn => 'p',
            PieceType::Knight => 'n',
            PieceType::Bishop => 'b',
            PieceType::Rook => 'r',
            PieceType::Queen => 'q',
            PieceType::King => 'k',
        }
    }

    /// Parses a piece letter in either case.
    pub const fn from_char(c: char) -> Option<PieceType> {
        match c.to_ascii_lowercase() {
            'p' => Some(PieceType::Pawn),
            'n' => Some(PieceType::Knight),
            'b' => Some(PieceType::Bishop),
            'r' => Some(PieceType::Rook),
            'q' => Some(PieceType::Queen),
            'k' => Some(PieceType::King),
            _ => None,
        }
    }
}

/// A piece of a given color.
///
/// The discriminant is `color * 6 + piece type`: white pieces are 0..6 and
/// black pieces 6..12, in the same order as [`PieceType`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Piece {
    WhitePawn,
    WhiteKnight,
    WhiteBishop,
    WhiteRook,
    WhiteQueen,
    WhiteKing,
    BlackPawn,
    BlackKnight,
    BlackBishop,
    BlackRook,
    BlackQueen,
    BlackKing,
}

impl Piece {
    /// All pieces, in index order.
    pub const ALL: [Piece; 12] = [
        Piece::WhitePawn,
        Piece::WhiteKnight,
        Piece::WhiteBishop,
        Piece::WhiteRook,
        Piece::WhiteQueen,
        Piece::WhiteKing,
        Piece::BlackPawn,
        Piece::BlackKnight,
        Piece::BlackBishop,
        Piece::BlackRook,
        Piece::BlackQueen,
        Piece::BlackKing,
    ];

    /// The piece with the given color and type.
    pub const fn new(color: Color, piece_type: PieceType) -> Piece {
        Piece::ALL[color.index() * 6 + piece_type.index()]
    }

    /// Index for array lookups (0..12).
    pub const fn index(self) -> usize {
        self as usize
    }

    /// Color of the piece.
    pub const fn color(self) -> Color {
        Color::ALL[self.index() / 6]
    }

    /// Type of the piece.
    pub const fn piece_type(self) -> PieceType {
        PieceType::ALL[self.index() % 6]
    }

    /// FEN letter of the piece: uppercase for white, lowercase for black.
    pub const fn to_char(self) -> char {
        let letter = self.piece_type().to_char();
        match self.color() {
            Color::White => letter.to_ascii_uppercase(),
            Color::Black => letter,
        }
    }

    /// Parses a FEN piece letter: uppercase for white, lowercase for black.
    pub fn from_char(c: char) -> Option<Piece> {
        let color = if c.is_ascii_uppercase() {
            Color::White
        } else {
            Color::Black
        };
        PieceType::from_char(c).map(|piece_type| Piece::new(color, piece_type))
    }
}

impl fmt::Display for Piece {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_char())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_flip() {
        assert_eq!(!Color::White, Color::Black);
        assert_eq!(!Color::Black, Color::White);
        assert_eq!(Color::White.index(), 0);
        assert_eq!(Color::Black.index(), 1);
    }

    #[test]
    fn indices_follow_declaration_order() {
        for (i, piece_type) in PieceType::ALL.into_iter().enumerate() {
            assert_eq!(piece_type.index(), i);
        }
        for (i, piece) in Piece::ALL.into_iter().enumerate() {
            assert_eq!(piece.index(), i);
        }
    }

    #[test]
    fn pieces_split_into_color_and_type() {
        assert_eq!(Piece::BlackKnight.color(), Color::Black);
        assert_eq!(Piece::BlackKnight.piece_type(), PieceType::Knight);
        for piece in Piece::ALL {
            assert_eq!(Piece::new(piece.color(), piece.piece_type()), piece);
        }
    }

    #[test]
    fn fen_letters_round_trip() {
        assert_eq!(Piece::WhiteKnight.to_char(), 'N');
        assert_eq!(Piece::BlackQueen.to_char(), 'q');
        for piece in Piece::ALL {
            assert_eq!(Piece::from_char(piece.to_char()), Some(piece));
        }
        assert_eq!(Piece::from_char('x'), None);
        assert_eq!(Piece::from_char('1'), None);
    }
}
