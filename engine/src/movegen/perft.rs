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
    use super::*;

    /// Checks the published counts for the first few depths.
    fn check(fen: &str, expected: &[u64]) {
        let pos = Position::from_fen(fen).unwrap();
        for (depth, &nodes) in (1..).zip(expected) {
            assert_eq!(perft(&pos, depth), nodes, "{fen} depth {depth}");
        }
    }

    // Counts from the Chess Programming Wiki (Perft Results).

    #[test]
    fn starting_position() {
        check(Position::STARTPOS_FEN, &[20, 400, 8_902, 197_281]);
    }

    #[test]
    fn kiwipete() {
        check(
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            &[48, 2_039, 97_862],
        );
    }

    #[test]
    fn position_3() {
        check(
            "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
            &[14, 191, 2_812, 43_238, 674_624],
        );
    }

    #[test]
    fn position_4() {
        check(
            "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
            &[6, 264, 9_467],
        );
    }

    #[test]
    fn position_5() {
        check(
            "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
            &[44, 1_486, 62_379],
        );
    }

    #[test]
    fn position_6() {
        check(
            "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
            &[46, 2_079, 89_890],
        );
    }

    #[test]
    fn chess960_position() {
        check(
            "bqnb1rkr/pp3ppp/3ppn2/2p5/5P2/P2P4/NPP1P1PP/BQ1BNRKR w HFhf - 2 9",
            &[21, 528, 12_189],
        );
    }

    #[test]
    fn divide_adds_up_to_perft() {
        let pos = Position::startpos();
        let divided = divide(&pos, 3);
        assert_eq!(divided.len(), 20);
        assert_eq!(divided.iter().map(|&(_, n)| n).sum::<u64>(), perft(&pos, 3));
        assert!(divide(&pos, 0).is_empty());
    }
}
