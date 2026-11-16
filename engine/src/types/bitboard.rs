//! Bitboards: sets of squares packed into one 64-bit integer.

use std::fmt;
use std::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, BitXor, BitXorAssign, Not};

use super::Square;

/// A set of squares. Bit `n` of the integer is set when square `n` is in the set.
///
/// A single machine instruction works on all 64 squares at once, which is what
/// makes bitboards fast: `a & b` is the intersection of two sets and `a | b`
/// their union.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Bitboard(pub u64);

impl Bitboard {
    /// The empty set.
    pub const EMPTY: Bitboard = Bitboard(0);
    /// All 64 squares.
    pub const FULL: Bitboard = Bitboard(u64::MAX);

    pub const FILE_A: Bitboard = Bitboard::file(0);
    pub const FILE_B: Bitboard = Bitboard::file(1);
    pub const FILE_C: Bitboard = Bitboard::file(2);
    pub const FILE_D: Bitboard = Bitboard::file(3);
    pub const FILE_E: Bitboard = Bitboard::file(4);
    pub const FILE_F: Bitboard = Bitboard::file(5);
    pub const FILE_G: Bitboard = Bitboard::file(6);
    pub const FILE_H: Bitboard = Bitboard::file(7);

    pub const RANK_1: Bitboard = Bitboard::rank(0);
    pub const RANK_2: Bitboard = Bitboard::rank(1);
    pub const RANK_3: Bitboard = Bitboard::rank(2);
    pub const RANK_4: Bitboard = Bitboard::rank(3);
    pub const RANK_5: Bitboard = Bitboard::rank(4);
    pub const RANK_6: Bitboard = Bitboard::rank(5);
    pub const RANK_7: Bitboard = Bitboard::rank(6);
    pub const RANK_8: Bitboard = Bitboard::rank(7);

    /// All squares of a file (0 = a-file).
    pub const fn file(file: u8) -> Bitboard {
        debug_assert!(file < 8, "file out of range");
        Bitboard(0x0101_0101_0101_0101 << file)
    }

    /// All squares of a rank (0 = first rank).
    pub const fn rank(rank: u8) -> Bitboard {
        debug_assert!(rank < 8, "rank out of range");
        Bitboard(0xFF << (rank * 8))
    }

    /// The set containing only `sq`.
    pub const fn from_square(sq: Square) -> Bitboard {
        Bitboard(1u64 << sq.index())
    }

    /// Whether the set has no squares.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Number of squares in the set.
    pub const fn popcount(self) -> u32 {
        self.0.count_ones()
    }

    /// Whether the set has at least two squares.
    pub const fn more_than_one(self) -> bool {
        (self.0 & self.0.wrapping_sub(1)) != 0
    }

    /// Whether `sq` is in the set.
    pub const fn contains(self, sq: Square) -> bool {
        (self.0 & (1u64 << sq.index())) != 0
    }

    /// The set with `sq` added.
    pub const fn with(self, sq: Square) -> Bitboard {
        Bitboard(self.0 | (1u64 << sq.index()))
    }

    /// The set with `sq` removed.
    pub const fn without(self, sq: Square) -> Bitboard {
        Bitboard(self.0 & !(1u64 << sq.index()))
    }

    /// The lowest square of the set. The set must not be empty.
    pub const fn lsb(self) -> Square {
        debug_assert!(!self.is_empty(), "lsb of an empty bitboard");
        Square::new(self.0.trailing_zeros() as u8)
    }

    /// Removes the lowest square from the set and returns it. The set must not be empty.
    pub const fn pop_lsb(&mut self) -> Square {
        let sq = self.lsb();
        // x & (x - 1) clears the lowest set bit.
        self.0 &= self.0 - 1;
        sq
    }

    // One-square shifts of the whole set. Moving east or west, a square on the
    // edge file would reappear on the opposite file one rank away, so that file
    // is masked out after the shift. Squares pushed past rank 1 or rank 8 fall
    // off the integer by themselves.

    /// Every square moved one step towards rank 8.
    pub const fn north(self) -> Bitboard {
        Bitboard(self.0 << 8)
    }

    /// Every square moved one step towards rank 1.
    pub const fn south(self) -> Bitboard {
        Bitboard(self.0 >> 8)
    }

    /// Every square moved one step towards the h-file.
    pub const fn east(self) -> Bitboard {
        Bitboard((self.0 << 1) & !Self::FILE_A.0)
    }

    /// Every square moved one step towards the a-file.
    pub const fn west(self) -> Bitboard {
        Bitboard((self.0 >> 1) & !Self::FILE_H.0)
    }

    /// Every square moved one step towards rank 8 and the h-file.
    pub const fn north_east(self) -> Bitboard {
        Bitboard((self.0 << 9) & !Self::FILE_A.0)
    }

    /// Every square moved one step towards rank 8 and the a-file.
    pub const fn north_west(self) -> Bitboard {
        Bitboard((self.0 << 7) & !Self::FILE_H.0)
    }

    /// Every square moved one step towards rank 1 and the h-file.
    pub const fn south_east(self) -> Bitboard {
        Bitboard((self.0 >> 7) & !Self::FILE_A.0)
    }

    /// Every square moved one step towards rank 1 and the a-file.
    pub const fn south_west(self) -> Bitboard {
        Bitboard((self.0 >> 9) & !Self::FILE_H.0)
    }
}

impl From<Square> for Bitboard {
    fn from(sq: Square) -> Bitboard {
        Bitboard::from_square(sq)
    }
}

/// Implements a set operator and its assigning form (such as `&` and `&=`) by
/// applying the same operator to the inner integers.
macro_rules! set_operator {
    ($op_trait:ident, $op_fn:ident, $op:tt, $assign_trait:ident, $assign_fn:ident, $assign_op:tt) => {
        impl $op_trait for Bitboard {
            type Output = Bitboard;

            fn $op_fn(self, rhs: Bitboard) -> Bitboard {
                Bitboard(self.0 $op rhs.0)
            }
        }

        impl $assign_trait for Bitboard {
            fn $assign_fn(&mut self, rhs: Bitboard) {
                self.0 $assign_op rhs.0;
            }
        }
    };
}

set_operator!(BitAnd, bitand, &, BitAndAssign, bitand_assign, &=);
set_operator!(BitOr, bitor, |, BitOrAssign, bitor_assign, |=);
set_operator!(BitXor, bitxor, ^, BitXorAssign, bitxor_assign, ^=);

impl Not for Bitboard {
    type Output = Bitboard;

    /// The complement: every square that is not in the set.
    fn not(self) -> Bitboard {
        Bitboard(!self.0)
    }
}

/// Iterator over the squares of a bitboard, from a1 towards h8.
#[derive(Clone, Debug)]
pub struct Squares(Bitboard);

impl Iterator for Squares {
    type Item = Square;

    fn next(&mut self) -> Option<Square> {
        if self.0.is_empty() {
            None
        } else {
            Some(self.0.pop_lsb())
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.0.popcount() as usize;
        (len, Some(len))
    }
}

impl ExactSizeIterator for Squares {}

impl IntoIterator for Bitboard {
    type Item = Square;
    type IntoIter = Squares;

    /// Lets a bitboard be used directly in a `for` loop.
    fn into_iter(self) -> Squares {
        Squares(self)
    }
}

impl fmt::Display for Bitboard {
    /// Draws the set as a board seen from white's side; `X` marks squares in the set.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for rank in (0..8).rev() {
            write!(f, "{}", rank + 1)?;
            for file in 0..8 {
                let mark = if self.contains(Square::from_coords(file, rank)) {
                    'X'
                } else {
                    '.'
                };
                write!(f, " {mark}")?;
            }
            writeln!(f)?;
        }
        write!(f, "  a b c d e f g h")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bb(sq: Square) -> Bitboard {
        Bitboard::from_square(sq)
    }

    #[test]
    fn bit_n_is_square_n() {
        assert_eq!(bb(Square::A1).0, 1u64);
        assert_eq!(bb(Square::H1).0, 1u64 << 7);
        assert_eq!(bb(Square::E4).0, 1u64 << 28);
        assert_eq!(bb(Square::H8).0, 1u64 << 63);
    }

    #[test]
    fn set_operations() {
        let set = bb(Square::E4).with(Square::A1);
        assert!(set.contains(Square::E4) && set.contains(Square::A1));
        assert!(!set.contains(Square::H8));
        assert_eq!(set.popcount(), 2);
        assert!(set.more_than_one());
        assert!(!bb(Square::E4).more_than_one());
        assert_eq!(set.without(Square::E4), bb(Square::A1));
        assert_eq!(set & bb(Square::A1), bb(Square::A1));
        assert_eq!(set ^ bb(Square::A1), bb(Square::E4));
        assert_eq!(!Bitboard::EMPTY, Bitboard::FULL);
        assert!(Bitboard::EMPTY.is_empty());
        assert_eq!(Bitboard::FULL.popcount(), 64);
    }

    #[test]
    fn iteration_is_ascending_and_complete() {
        let squares = [Square::B2, Square::E4, Square::H8];
        let mut set = Bitboard::EMPTY;
        for sq in squares {
            set |= bb(sq);
        }
        assert_eq!(set.into_iter().len(), 3);
        assert_eq!(set.into_iter().collect::<Vec<_>>(), squares);

        let every: Vec<Square> = Bitboard::FULL.into_iter().collect();
        assert_eq!(every, Square::all().collect::<Vec<_>>());
    }

    #[test]
    fn pop_lsb_removes_the_lowest_square() {
        let mut set = bb(Square::F6) | bb(Square::C3);
        assert_eq!(set.lsb(), Square::C3);
        assert_eq!(set.pop_lsb(), Square::C3);
        assert_eq!(set, bb(Square::F6));
    }

    #[test]
    fn files_and_ranks_contain_their_squares() {
        let mut files = Bitboard::EMPTY;
        let mut ranks = Bitboard::EMPTY;
        for i in 0..8 {
            assert_eq!(Bitboard::file(i).popcount(), 8);
            assert_eq!(Bitboard::rank(i).popcount(), 8);
            files |= Bitboard::file(i);
            ranks |= Bitboard::rank(i);
        }
        assert_eq!(files, Bitboard::FULL);
        assert_eq!(ranks, Bitboard::FULL);
        for sq in Square::all() {
            assert!(Bitboard::file(sq.file()).contains(sq));
            assert!(Bitboard::rank(sq.rank()).contains(sq));
        }
        assert_eq!(Bitboard::FILE_A & Bitboard::RANK_1, bb(Square::A1));
        assert_eq!(Bitboard::FILE_H & Bitboard::RANK_8, bb(Square::H8));
    }

    #[test]
    fn shifts_move_one_square() {
        let e4 = bb(Square::E4);
        assert_eq!(e4.north(), bb(Square::E5));
        assert_eq!(e4.south(), bb(Square::E3));
        assert_eq!(e4.east(), bb(Square::F4));
        assert_eq!(e4.west(), bb(Square::D4));
        assert_eq!(e4.north_east(), bb(Square::F5));
        assert_eq!(e4.north_west(), bb(Square::D5));
        assert_eq!(e4.south_east(), bb(Square::F3));
        assert_eq!(e4.south_west(), bb(Square::D3));
    }

    #[test]
    fn shifts_do_not_wrap_around_the_board() {
        // Squares pushed over an edge must disappear, not reappear on the far side.
        assert!(Bitboard::FILE_H.east().is_empty());
        assert!(Bitboard::FILE_H.north_east().is_empty());
        assert!(Bitboard::FILE_H.south_east().is_empty());
        assert!(Bitboard::FILE_A.west().is_empty());
        assert!(Bitboard::FILE_A.north_west().is_empty());
        assert!(Bitboard::FILE_A.south_west().is_empty());
        assert!(Bitboard::RANK_8.north().is_empty());
        assert!(Bitboard::RANK_1.south().is_empty());
        assert_eq!(Bitboard::FULL.east(), !Bitboard::FILE_A);
        assert_eq!(Bitboard::FULL.west(), !Bitboard::FILE_H);
        assert_eq!(Bitboard::FULL.north(), !Bitboard::RANK_1);
        assert_eq!(Bitboard::FULL.south(), !Bitboard::RANK_8);
    }

    #[test]
    fn display_draws_the_board_from_whites_side() {
        let expected = "\
8 . . . . . . . .
7 . . . . . . . .
6 . . . . . . . .
5 . . . . . . . .
4 . . . . X . . .
3 . . . . . . . .
2 . . . . . . . .
1 X . . . . . . .
  a b c d e f g h";
        assert_eq!((bb(Square::E4) | bb(Square::A1)).to_string(), expected);
    }
}
