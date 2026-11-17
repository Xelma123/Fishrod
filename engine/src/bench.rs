//! The bench: a fixed set of positions, each searched to a fixed depth on a
//! single thread. The total node count is a fingerprint of the search: a
//! change meant to leave the search's behaviour alone (a cleanup, a speed-up)
//! must leave the count alone too, so every commit records it.

use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use crate::board::Position;
use crate::search::{Limits, Outcome, TranspositionTable, search};
use crate::uci::parse_move;

/// The depth `fishrod bench` uses when none is given: a few seconds in a
/// release build. Raising it changes the bench count, as expected.
pub const DEFAULT_DEPTH: u32 = 5;
/// The size of the transposition table, in MiB: the engine's default.
const HASH_MB: usize = 16;

/// Positions given as FEN: the standard perft positions, the first Chess960
/// perft positions (see `engine/tests/perft.rs` for their sources) and a few
/// endgames.
const FENS: [&str; 30] = [
    // Standard perft positions.
    "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
    "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
    "r2q1rk1/pP1p2pp/Q4n2/bbp1p3/Np6/1B3NBn/pPPP1PPP/R3K2R b KQ - 0 1",
    "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
    "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
    // Chess960 perft positions.
    "bqnb1rkr/pp3ppp/3ppn2/2p5/5P2/P2P4/NPP1P1PP/BQ1BNRKR w HFhf - 2 9",
    "2nnrbkr/p1qppppp/8/1ppb4/6PP/3PP3/PPP2P2/BQNNRBKR w HEhe - 1 9",
    "b1q1rrkb/pppppppp/3nn3/8/P7/1PPP4/4PPPP/BQNNRKRB w GE - 1 9",
    "qbbnnrkr/2pp2pp/p7/1p2pp2/8/P3PP2/1PPP1KPP/QBBNNR1R w hf - 0 9",
    "1nbbnrkr/p1p1ppp1/3p4/1p3P1p/3Pq2P/8/PPP1P1P1/QNBBNRKR w HFhf - 0 9",
    "qnbnr1kr/ppp1b1pp/4p3/3p1p2/8/2NPP3/PPP1BPPP/QNB1R1KR w HEhe - 1 9",
    "q1bnrkr1/ppppp2p/2n2p2/4b1p1/2NP4/8/PPP1PPPP/QNB1RRKB w ge - 1 9",
    "qbn1brkr/ppp1p1p1/2n4p/3p1p2/P7/6PP/QPPPPP2/1BNNBRKR w HFhf - 0 9",
    "qnnbbrkr/1p2ppp1/2pp3p/p7/1P5P/2NP4/P1P1PPP1/Q1NBBRKR w HFhf - 0 9",
    "qn1rbbkr/ppp2p1p/1n1pp1p1/8/3P4/P6P/1PP1PPPK/QNNRBB1R w hd - 2 9",
    "qnr1bkrb/pppp2pp/3np3/5p2/8/P2P2P1/NPP1PP1P/QN1RBKRB w GDg - 3 9",
    "qb1nrkbr/1pppp1p1/1n3p2/p1B4p/8/3P1P1P/PPP1P1P1/QBNNRK1R w HEhe - 0 9",
    "qnnbrk1r/1p1ppbpp/2p5/p4p2/2NP3P/8/PPP1PPP1/Q1NBRKBR w HEhe - 0 9",
    "1qnrkbbr/1pppppp1/p1n4p/8/P7/1P1N1P2/2PPP1PP/QN1RKBBR w HDhd - 0 9",
    "qn1rkrbb/pp1p1ppp/2p1p3/3n4/4P2P/2NP4/PPP2PP1/Q1NRKRBB w FDfd - 1 9",
    "bb1qnrkr/pp1p1pp1/1np1p3/4N2p/8/1P4P1/P1PPPP1P/BBNQ1RKR w HFhf - 0 9",
    "bnqbnr1r/p1p1ppkp/3p4/1p4p1/P7/3NP2P/1PPP1PP1/BNQB1RKR w HF - 0 9",
    "bnqnrbkr/1pp2pp1/p7/3pP2p/4P1P1/8/PPPP3P/BNQNRBKR w HEhe d6 0 9",
    "b1qnrrkb/ppp1pp1p/n2p1Pp1/8/8/P7/1PPPP1PP/BNQNRKRB w GE - 0 9",
    "n1bqnrkr/pp1ppp1p/2p5/6p1/2P2b2/PN6/1PNPPPPP/1BBQ1RKR w HFhf - 2 9",
    // Endgames.
    "8/5pk1/6p1/8/5P2/6P1/5K2/8 w - - 0 40",
    "6k1/5ppp/8/8/8/8/5PPP/R5K1 w - - 0 30",
    "8/8/5k2/8/2B5/3K4/8/7r w - - 0 60",
];

/// Opening lines from the starting position, in UCI notation. Played out,
/// they give typical middlegame positions.
const OPENINGS: [&str; 8] = [
    // Ruy Lopez, closed.
    "e2e4 e7e5 g1f3 b8c6 f1b5 a7a6 b5a4 g8f6 e1g1 f8e7 f1e1 b7b5 a4b3 d7d6 c2c3 e8g8",
    // Sicilian, Najdorf.
    "e2e4 c7c5 g1f3 d7d6 d2d4 c5d4 f3d4 g8f6 b1c3 a7a6 c1e3 e7e5 d4b3 c8e6 f2f3",
    // Queen's Gambit Declined.
    "d2d4 d7d5 c2c4 e7e6 b1c3 g8f6 c1g5 f8e7 e2e3 e8g8 g1f3 b8d7 a1c1 c7c6",
    // King's Indian.
    "d2d4 g8f6 c2c4 g7g6 b1c3 f8g7 e2e4 d7d6 g1f3 e8g8 f1e2 e7e5 e1g1 b8c6 d4d5 c6e7",
    // French, Winawer.
    "e2e4 e7e6 d2d4 d7d5 b1c3 f8b4 e4e5 c7c5 a2a3 b4c3 b2c3 g8e7 d1g4",
    // Caro-Kann.
    "e2e4 c7c6 d2d4 d7d5 b1c3 d5e4 c3e4 c8f5 e4g3 f5g6 h2h4 h7h6 g1f3 b8d7",
    // English.
    "c2c4 e7e5 b1c3 g8f6 g1f3 b8c6 g2g3 d7d5 c4d5 f6d5 f1g2 d5b6 e1g1 f8e7",
    // Italian.
    "e2e4 e7e5 g1f3 b8c6 f1c4 f8c5 c2c3 g8f6 d2d3 d7d6 e1g1 e8g8 f1e1 a7a6",
];

/// The bench positions, in a fixed order.
pub fn positions() -> Vec<Position> {
    let mut positions: Vec<Position> = FENS
        .iter()
        .map(|fen| Position::from_fen(fen).expect("bench FENs are valid"))
        .collect();
    for line in OPENINGS {
        let mut pos = Position::startpos();
        for text in line.split_whitespace() {
            let mv = parse_move(&pos, text, false).expect("bench openings are legal");
            pos = pos.make_move(mv);
        }
        positions.push(pos);
    }
    positions
}

/// Totals of a bench run.
#[derive(Clone, Copy, Debug)]
pub struct Summary {
    pub nodes: u64,
    pub time: Duration,
}

/// Searches every bench position to `depth`, each as a fresh search without
/// game history and with an empty transposition table, and calls `report`
/// after each one.
pub fn run(depth: u32, mut report: impl FnMut(usize, &Position, &Outcome)) -> Summary {
    let limits = Limits {
        depth: Some(depth),
        ..Limits::default()
    };
    let tt = TranspositionTable::new(HASH_MB).expect("the bench hash table fits in memory");
    let never_stop = AtomicBool::new(false);
    let start = Instant::now();
    let mut nodes = 0;
    for (index, pos) in positions().iter().enumerate() {
        // An empty table makes each count independent of the positions before.
        tt.clear();
        let outcome = search(pos, &[], limits, &tt, &never_stop, |_| {});
        nodes += outcome.nodes;
        report(index, pos, &outcome);
    }
    Summary {
        nodes,
        time: start.elapsed(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::movegen::legal_moves;

    #[test]
    fn positions_are_valid_and_playable() {
        let positions = positions();
        assert_eq!(positions.len(), FENS.len() + OPENINGS.len());
        for pos in &positions {
            assert!(pos.is_consistent());
            assert!(!legal_moves(pos).is_empty(), "{pos:?}");
        }
    }

    #[test]
    fn the_node_count_is_reproducible() {
        let first = run(2, |_, _, _| {});
        let second = run(2, |_, _, _| {});
        assert_eq!(first.nodes, second.nodes);
        assert!(first.nodes > 0);
    }
}
