//! Perft: counting every sequence of legal moves of a given length.
//!
//! The counts for many positions are published, so matching them is the
//! standard test of move generation and move making. `divide` splits a count
//! by first move, which narrows a wrong total down to the move whose subtree
//! is off.

use super::legal_moves;
use crate::board::Position;
use crate::types::Move;

/// The number of legal move sequences of length `depth` from `pos`.
pub fn perft(pos: &Position, depth: u32) -> u64 {
    match depth {
        0 => 1,
        // On the last level the moves only need counting, not playing.
        1 => legal_moves(pos).len() as u64,
        _ => legal_moves(pos)
            .iter()
            .map(|&mv| perft(&pos.make_move(mv), depth - 1))
            .sum(),
    }
}

/// Each legal move of `pos` with the perft count of `depth - 1` below it.
/// Empty when `depth` is 0.
pub fn divide(pos: &Position, depth: u32) -> Vec<(Move, u64)> {
    if depth == 0 {
        return Vec::new();
    }
    legal_moves(pos)
        .iter()
        .map(|&mv| (mv, perft(&pos.make_move(mv), depth - 1)))
        .collect()
}

#[cfg(test)]
mod tests {
    // The published perft counts are checked by the suite in `engine/tests/perft.rs`.
    use super::*;

    #[test]
    fn divide_adds_up_to_perft() {
        let pos = Position::startpos();
        let divided = divide(&pos, 3);
        assert_eq!(divided.len(), 20);
        assert_eq!(divided.iter().map(|&(_, n)| n).sum::<u64>(), 8_902);
        assert_eq!(perft(&pos, 3), 8_902);
        assert!(divide(&pos, 0).is_empty());
        assert_eq!(perft(&pos, 0), 1);
    }
}
