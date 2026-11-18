//! Playing against people: how long a move may take, and which of the
//! searched moves to play. Shared by the UCI front end and the web version,
//! so that the engine behaves the same in both.
//!
//! Against people the engine keeps a brisk pace (`Think Time`): a move
//! normally takes a few seconds, easy ones less and difficult ones up to
//! twice as long (see `search::Limits::max_time`). Early in the game it
//! draws its move from those nearly as good as the best (`Variety`), so that
//! games do not all open alike; at reduced strength it draws from a wider
//! choice and searches less deep.

use std::time::Duration;

use crate::board::Position;
use crate::rng::Rng;
use crate::search::strength::{Strength, choose_near_best};
use crate::search::{Limits, Outcome};
use crate::types::Move;

/// `Variety` applies up to this move number.
pub const VARIETY_MOVES: u16 = 10;

/// Shortens a clock's `limits` to `think` and lets the time adapt
/// (`Limits::max_time`): a move normally takes at most `think`, easy ones
/// less, and unsettled or difficult ones up to twice as long. The clock's
/// own limits still apply when they are shorter. No new iteration starts
/// after three quarters of `think`, since it would rarely finish in time.
pub fn with_think_time(limits: Limits, think: Duration) -> Limits {
    let (Some(soft), Some(hard)) = (limits.soft_time, limits.hard_time) else {
        return limits;
    };
    let capped_hard = hard.min(think);
    let max = hard.min(think * 2);
    Limits {
        soft_time: Some(soft.min(think * 3 / 4)),
        hard_time: Some(capped_hard),
        max_time: (max > capped_hard).then_some(max),
        ..limits
    }
}

/// The limits for a move that may take about `think`, without a clock: as
/// under a clock long enough never to be the shorter limit.
pub fn think_limits(think: Duration) -> Limits {
    let unlimited = Limits {
        soft_time: Some(Duration::MAX),
        hard_time: Some(Duration::MAX),
        ..Limits::default()
    };
    with_think_time(unlimited, think)
}

/// How the engine picks its moves.
#[derive(Clone, Copy, Debug, Default)]
pub struct Style {
    /// The playing strength in Elo (`UCI_LimitStrength` with `UCI_Elo`), or
    /// `None` for full strength.
    pub elo: Option<u32>,
    /// `Variety`, in centipawns; 0 turns it off.
    pub variety: u32,
}

/// A search fitted to a style, and how to pick its move.
#[derive(Clone, Copy, Debug)]
pub struct Plan {
    /// The limits to search with.
    pub limits: Limits,
    /// How far below the best a played move may score, if the move is drawn
    /// among several; `None` to play the best.
    margin: Option<i32>,
}

impl Style {
    /// Fits `limits` for a search of `pos` to the style. An `analysis` (`go
    /// infinite`) only wants the best move, so it is not drawn.
    ///
    /// At reduced strength the search goes less deep and the move is drawn
    /// within the strength's margin; otherwise, up to move `VARIETY_MOVES`,
    /// within `variety`. Drawing needs every root move's exact score, and
    /// the time is not stretched: the best move changes often among nearly
    /// equal ones, which is no reason to think longer when one is drawn.
    pub fn plan(&self, pos: &Position, mut limits: Limits, analysis: bool) -> Plan {
        let strength = self.elo.map(Strength::new);
        if let Some(strength) = strength {
            let depth = limits
                .depth
                .map_or(strength.depth(), |d| d.min(strength.depth()));
            limits.depth = Some(depth);
        }
        let margin = match strength {
            Some(strength) => Some(strength.margin()),
            None => (self.variety > 0 && !analysis && pos.fullmove_number() <= VARIETY_MOVES)
                .then_some(self.variety as i32),
        };
        if margin.is_some() {
            limits.exact_root_scores = true;
            limits.max_time = None;
        }
        Plan { limits, margin }
    }
}

impl Plan {
    /// Whether the move will be drawn among several.
    pub fn draws_move(&self) -> bool {
        self.margin.is_some()
    }

    /// The move to play after the search: the best one, or one drawn near
    /// it using `seed`, which should differ from game to game.
    pub fn pick(&self, outcome: &Outcome, seed: u64) -> Option<Move> {
        match self.margin {
            Some(margin) => {
                let mut rng = Rng::new(seed);
                choose_near_best(&outcome.root_scores, margin, &mut rng).or(outcome.best_move)
            }
            None => outcome.best_move,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn think_limits_are_those_of_a_long_clock() {
        let ms = Duration::from_millis;
        let limits = think_limits(ms(4000));
        assert_eq!(
            (limits.soft_time, limits.hard_time, limits.max_time),
            (Some(ms(3000)), Some(ms(4000)), Some(ms(8000)))
        );
    }

    #[test]
    fn variety_and_strength_draw_the_move() {
        let start = Position::startpos();
        let later = Position::from_fen("4k3/8/8/8/8/8/8/4K2R w K - 0 11").unwrap();
        let clock = think_limits(Duration::from_millis(4000));
        let varied = Style {
            elo: None,
            variety: 10,
        };

        // Early in the game, the move is drawn: every root move is scored
        // exactly and the time is not stretched.
        let plan = varied.plan(&start, clock, false);
        assert!(plan.draws_move() && plan.limits.exact_root_scores);
        assert_eq!(plan.limits.max_time, None);
        // Not later on, nor in an analysis, nor with variety off.
        assert!(!varied.plan(&later, clock, false).draws_move());
        assert!(!varied.plan(&start, clock, true).draws_move());
        assert!(!Style::default().plan(&start, clock, false).draws_move());
        assert_eq!(
            Style::default().plan(&start, clock, false).limits.max_time,
            clock.max_time
        );

        // Reduced strength draws at any point and limits the depth.
        let weak = Style {
            elo: Some(800),
            variety: 0,
        };
        let plan = weak.plan(&later, clock, false);
        assert!(plan.draws_move());
        assert_eq!(plan.limits.depth, Some(2));
    }
}
