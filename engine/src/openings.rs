//! Opening positions for engine-versus-engine tests.
//!
//! Two builds of the engine are compared by letting them play many games.
//! Starting every game from the same position would produce the same game
//! over and over, so each pair of games starts from a different opening. An
//! opening here is the starting position after a few random legal moves.
//! Random play mostly leads to lopsided positions, so each candidate is
//! searched briefly and kept only if neither side is clearly ahead.

use std::collections::HashSet;
use std::sync::atomic::AtomicBool;

use crate::board::Position;
use crate::movegen::legal_moves;
use crate::rng::Rng;
use crate::search::{Limits, TranspositionTable, search};

/// Random half-moves played from the starting position.
pub const PLIES: usize = 8;
/// The search depth of the balance check that `fishrod openings` uses.
pub const DEFAULT_DEPTH: u32 = 4;
/// The seed `fishrod openings` uses when none is given: the bytes of "openings".
pub const DEFAULT_SEED: u64 = 0x6F70_656E_696E_6773;
/// An opening is kept if the search scores it within this many centipawns of level.
const MAX_SCORE: i32 = 50;

/// An endless supply of distinct, balanced opening positions.
pub struct Openings {
    rng: Rng,
    depth: u32,
    /// Keys of the positions already looked at, kept or not.
    seen: HashSet<u64>,
    tt: TranspositionTable,
}

/// Opening positions for the given seed, with the balance of each one
/// checked by a search to `depth`. The same seed and depth always give the
/// same openings in the same order.
pub fn openings(seed: u64, depth: u32) -> Openings {
    Openings {
        rng: Rng::new(seed),
        depth,
        seen: HashSet::new(),
        tt: TranspositionTable::new(1).expect("a 1 MiB hash table fits in memory"),
    }
}

impl Iterator for Openings {
    type Item = Position;

    fn next(&mut self) -> Option<Position> {
        loop {
            if let Some(pos) = random_line(&mut self.rng)
                && self.seen.insert(pos.key())
                && is_balanced(&pos, self.depth, &self.tt)
            {
                return Some(pos);
            }
        }
    }
}

/// The starting position after `PLIES` random legal moves. `None` if the game
/// ended on the way, or if the side to move ends up in check or without moves.
fn random_line(rng: &mut Rng) -> Option<Position> {
    let mut pos = Position::startpos();
    for _ in 0..PLIES {
        let moves = legal_moves(&pos);
        if moves.is_empty() {
            return None;
        }
        let pick = (rng.next_u64() % moves.len() as u64) as usize;
        pos = pos.make_move(moves[pick]);
    }
    (pos.checkers().is_empty() && !legal_moves(&pos).is_empty()).then_some(pos)
}

/// Whether a search to `depth` scores the position close to level. The
/// table is emptied first, so the verdict depends on the position alone.
fn is_balanced(pos: &Position, depth: u32, tt: &TranspositionTable) -> bool {
    let limits = Limits {
        depth: Some(depth),
        ..Limits::default()
    };
    tt.clear();
    let outcome = search(pos, &[], limits, tt, &AtomicBool::new(false), |_| {});
    outcome.score.abs() <= MAX_SCORE
}

/// The position as an EPD line: the first four fields of its FEN.
pub fn to_epd(pos: &Position) -> String {
    let fen = pos.to_fen();
    let fields: Vec<&str> = fen.split(' ').take(4).collect();
    fields.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openings_are_distinct_playable_and_reproducible() {
        let first: Vec<Position> = openings(1, 2).take(6).collect();
        assert_eq!(first.len(), 6);
        let keys: HashSet<u64> = first.iter().map(Position::key).collect();
        assert_eq!(keys.len(), 6);
        for pos in &first {
            assert!(pos.checkers().is_empty());
            assert!(!legal_moves(pos).is_empty());
            // Eight half-moves were played: it is white's fifth move.
            assert_eq!(pos.fullmove_number(), 5);
            assert_eq!(to_epd(pos).split(' ').count(), 4);
            assert!(Position::from_fen(&to_epd(pos)).is_ok());
        }

        let again: Vec<Position> = openings(1, 2).take(6).collect();
        assert_eq!(again, first);
        let other_seed: Vec<Position> = openings(2, 2).take(6).collect();
        assert_ne!(other_seed, first);
    }
}
