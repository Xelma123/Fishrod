//! Self-play smoke test: the engine plays itself from every bench position,
//! with a small node budget per move, until the game ends by a rule of chess
//! or reaches a move cap. Every move must be legal and every position must
//! stay consistent; a panic anywhere fails the test.

use std::sync::atomic::AtomicBool;

use fishrod::bench;
use fishrod::board::Position;
use fishrod::movegen::legal_moves;
use fishrod::search::{Limits, TranspositionTable, search};

/// Nodes per move: enough for a sensible move, small enough to finish quickly.
const NODES_PER_MOVE: u64 = 3_000;
/// Games still going after this many half-moves are cut off.
const MAX_PLIES: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ending {
    Checkmate,
    Stalemate,
    FiftyMoves,
    Repetition,
    MoveCap,
}

/// Plays one game from `start` and tells how it ended.
fn play(start: Position) -> Ending {
    let limits = Limits {
        nodes: Some(NODES_PER_MOVE),
        ..Limits::default()
    };
    let never_stop = AtomicBool::new(false);
    // One table for the whole game, as in a real game.
    let tt = TranspositionTable::new(16).unwrap();
    let mut pos = start;
    // Positions since the last capture or pawn move, as the search expects.
    let mut history: Vec<u64> = Vec::new();

    for _ in 0..MAX_PLIES {
        let moves = legal_moves(&pos);
        if moves.is_empty() {
            return if pos.checkers().is_empty() {
                Ending::Stalemate
            } else {
                Ending::Checkmate
            };
        }
        if pos.halfmove_clock() >= 100 {
            return Ending::FiftyMoves;
        }
        // The third occurrence of a position: it was seen twice before.
        if history.iter().filter(|&&key| key == pos.key()).count() >= 2 {
            return Ending::Repetition;
        }

        let outcome = search(&pos, &history, limits, &tt, &never_stop, |_| {});
        let mv = outcome.best_move.expect("a move exists");
        assert!(moves.contains(&mv), "illegal {mv:?} in {pos:?}");

        history.push(pos.key());
        pos = pos.make_move(mv);
        if pos.halfmove_clock() == 0 {
            history.clear();
        }
        assert!(pos.is_consistent(), "{pos:?}");
        assert_eq!(pos.key(), pos.compute_key(), "{pos:?}");
    }
    Ending::MoveCap
}

#[test]
#[ignore = "slow; run with cargo test --release -- --ignored"]
fn self_play_games_end_by_the_rules() {
    let mut endings = Vec::new();
    for start in bench::positions() {
        endings.push(play(start));
    }
    let count = |ending| endings.iter().filter(|&&e| e == ending).count();
    println!(
        "{} games: {} checkmate, {} stalemate, {} fifty-move, {} repetition, {} cut off",
        endings.len(),
        count(Ending::Checkmate),
        count(Ending::Stalemate),
        count(Ending::FiftyMoves),
        count(Ending::Repetition),
        count(Ending::MoveCap)
    );
    assert_eq!(endings.len(), bench::positions().len());
}
