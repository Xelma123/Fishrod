//! Board squares and their names.

use std::fmt;
use std::str::FromStr;

/// One of the 64 squares of the board.
///
/// Squares are numbered rank by rank from white's side:
/// a1 = 0, b1 = 1, ..., h1 = 7, a2 = 8, ..., h8 = 63.
/// The file is `index % 8` (a-file = 0) and the rank is `index / 8`
/// (first rank = 0). Bitboards use the same numbering: bit `n` is square `n`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Square(u8);

impl Square {
    /// Number of squares on the board.
    pub const COUNT: usize = 64;

    /// Creates a square from its index, which must be below 64.
    pub const fn new(index: u8) -> Square {
        debug_assert!(index < 64, "square index out of range");
        Square(index)
    }

    /// Creates a square from a file (0 = a-file) and a rank (0 = first rank).
    pub const fn from_coords(file: u8, rank: u8) -> Square {
        debug_assert!(file < 8 && rank < 8, "square coordinates out of range");
        Square(rank * 8 + file)
    }

    /// Index of the square (0..64), usable for array lookups.
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    /// File of the square: 0 for the a-file up to 7 for the h-file.
    pub const fn file(self) -> u8 {
        self.0 % 8
    }

    /// Rank of the square: 0 for the first rank up to 7 for the eighth.
    pub const fn rank(self) -> u8 {
        self.0 / 8
    }

    /// The square `file_step` files and `rank_step` ranks away, or `None` if
    /// that is off the board. Positive steps go towards the h-file and rank 8.
    pub const fn offset(self, file_step: i8, rank_step: i8) -> Option<Square> {
        // Stepping left of the a-file wraps around to a large number, so a
        // single `< 8` test catches both edges.
        let file = self.file().wrapping_add_signed(file_step);
        let rank = self.rank().wrapping_add_signed(rank_step);
        if file < 8 && rank < 8 {
            Some(Square::from_coords(file, rank))
        } else {
            None
        }
    }

    /// All squares from a1 to h8, in index order.
    pub fn all() -> impl Iterator<Item = Square> {
        (0..64).map(Square)
    }

    /// Builds a square from an uppercase name such as `"E4"` at compile time.
    /// Only `square_constants!` uses it, so an invalid name fails the build.
    const fn from_constant_name(name: &str) -> Square {
        let bytes = name.as_bytes();
        assert!(bytes.len() == 2, "square names have two characters");
        let file = bytes[0].wrapping_sub(b'A');
        let rank = bytes[1].wrapping_sub(b'1');
        assert!(file < 8 && rank < 8, "invalid square name");
        Square(rank * 8 + file)
    }
}

/// Defines a named constant such as `Square::E4` for every listed square.
/// Each value is computed from the constant's own name, so a name and its
/// square can never disagree.
macro_rules! square_constants {
    ($($name:ident)*) => {
        impl Square {
            $(pub const $name: Square = Square::from_constant_name(stringify!($name));)*
        }
    };
}

square_constants! {
    A1 B1 C1 D1 E1 F1 G1 H1
    A2 B2 C2 D2 E2 F2 G2 H2
    A3 B3 C3 D3 E3 F3 G3 H3
    A4 B4 C4 D4 E4 F4 G4 H4
    A5 B5 C5 D5 E5 F5 G5 H5
    A6 B6 C6 D6 E6 F6 G6 H6
    A7 B7 C7 D7 E7 F7 G7 H7
    A8 B8 C8 D8 E8 F8 G8 H8
}

impl fmt::Display for Square {
    /// Writes the square's name, such as `e4`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let file = char::from(b'a' + self.file());
        let rank = char::from(b'1' + self.rank());
        write!(f, "{file}{rank}")
    }
}

impl fmt::Debug for Square {
    /// Same as `Display`, so test failures show `e4` instead of a raw index.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// Error returned when a string is not a square name such as `e4`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseSquareError;

impl fmt::Display for ParseSquareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid square name")
    }
}

impl std::error::Error for ParseSquareError {}

impl FromStr for Square {
    type Err = ParseSquareError;

    /// Parses a lowercase square name such as `e4`.
    fn from_str(name: &str) -> Result<Square, ParseSquareError> {
        let &[file, rank] = name.as_bytes() else {
            return Err(ParseSquareError);
        };
        let file = file.wrapping_sub(b'a');
        let rank = rank.wrapping_sub(b'1');
        if file < 8 && rank < 8 {
            Ok(Square::from_coords(file, rank))
        } else {
            Err(ParseSquareError)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbering_starts_at_a1_and_ends_at_h8() {
        assert_eq!(Square::A1.index(), 0);
        assert_eq!(Square::H1.index(), 7);
        assert_eq!(Square::A2.index(), 8);
        assert_eq!(Square::E4.index(), 28);
        assert_eq!(Square::A8.index(), 56);
        assert_eq!(Square::H8.index(), 63);
    }

    #[test]
    fn file_and_rank_match_coordinates() {
        assert_eq!((Square::E4.file(), Square::E4.rank()), (4, 3));
        for sq in Square::all() {
            assert_eq!(Square::from_coords(sq.file(), sq.rank()), sq);
            assert_eq!(Square::new(sq.index() as u8), sq);
        }
        assert_eq!(Square::all().count(), Square::COUNT);
    }

    #[test]
    fn offset_stays_on_the_board() {
        assert_eq!(Square::E4.offset(1, 2), Some(Square::F6));
        assert_eq!(Square::E4.offset(-4, -3), Some(Square::A1));
        assert_eq!(Square::E4.offset(0, 0), Some(Square::E4));
        assert_eq!(Square::A1.offset(-1, 0), None);
        assert_eq!(Square::A1.offset(0, -1), None);
        assert_eq!(Square::H8.offset(0, 1), None);
        // h1 plus one file must not wrap around to a2.
        assert_eq!(Square::H1.offset(1, 0), None);
        assert_eq!(Square::G1.offset(2, 1), None);
    }

    #[test]
    fn names_round_trip() {
        assert_eq!(Square::E4.to_string(), "e4");
        assert_eq!(format!("{:?}", Square::H8), "h8");
        for sq in Square::all() {
            assert_eq!(sq.to_string().parse::<Square>(), Ok(sq));
        }
    }

    #[test]
    fn invalid_names_are_rejected() {
        for name in ["", "e", "e44", "i1", "a0", "a9", "E4", "4e", " e4"] {
            assert_eq!(name.parse::<Square>(), Err(ParseSquareError), "{name:?}");
        }
    }
}
