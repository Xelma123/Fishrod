//! Playing below full strength (`UCI_LimitStrength`), and picking among
//! nearly equal moves for variety (`Variety`).
//!
//! One Elo setting turns two knobs. From `NODES_FROM_ELO` up, the search
//! looks at fewer positions per move and the best move is played. Below
//! that, the budget stays at its smallest and the move is drawn at random
//! among those that score not much worse than the best, closer ones more
//! likely, so that weak settings make plain mistakes now and then, much as
//! people do.
//!
//! The tables come from matches against Stockfish's strength-limited mode,
//! which is calibrated to the CCRL 40/4 rating list, so the settings are
//! engine ratings, not human ones. Settings from 1300 up were measured
//! against it directly; weaker ones, below its range, through matches with
//! the measured settings, and are rougher.

use crate::rng::Rng;
use crate::types::Move;

/// Measured strengths of node-limited searches: (Elo, nodes per move), in
/// increasing order. Between two points the budget is interpolated on a log
/// scale, since each doubling of the budget adds about the same strength.
const NODE_BUDGETS: [(u32, u64); 9] = [
    (1020, 100),
    (1090, 300),
    (1310, 1_000),
    (1500, 3_000),
    (1660, 10_000),
    (1850, 30_000),
    (2000, 100_000),
    (2120, 300_000),
    (2340, 1_000_000),
];

/// The setting from which the node budget alone sets the strength.
const NODES_FROM_ELO: u32 = NODE_BUDGETS[0].0;

/// Below `NODES_FROM_ELO`: how far below the best score a move may fall and
/// still be played, in centipawns, at (Elo, margin) points in increasing
/// order, interpolated linearly in between.
const MARGINS: [(u32, i32); 6] = [
    (500, 220),
    (550, 200),
    (750, 100),
    (800, 50),
    (860, 25),
    (NODES_FROM_ELO, 0),
];

/// A playing strength between `MIN_ELO` and `MAX_ELO`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Strength {
    elo: u32,
}

impl Strength {
    pub(crate) const MIN_ELO: u32 = 500;
    pub(crate) const MAX_ELO: u32 = 2300;
    pub(crate) const DEFAULT_ELO: u32 = 1500;

    /// The strength for `elo`, brought into the supported range.
    pub(crate) fn new(elo: u32) -> Strength {
        Strength {
            elo: elo.clamp(Self::MIN_ELO, Self::MAX_ELO),
        }
    }

    /// How many nodes a move may search.
    pub(crate) fn nodes(self) -> u64 {
        let Some(i) = NODE_BUDGETS.windows(2).position(|w| self.elo <= w[1].0) else {
            return NODE_BUDGETS[NODE_BUDGETS.len() - 1].1;
        };
        let ((low_elo, low_nodes), (high_elo, high_nodes)) = (NODE_BUDGETS[i], NODE_BUDGETS[i + 1]);
        if self.elo <= low_elo {
            return low_nodes;
        }
        let t = f64::from(self.elo - low_elo) / f64::from(high_elo - low_elo);
        let ratio = high_nodes as f64 / low_nodes as f64;
        (low_nodes as f64 * ratio.powf(t)).round() as u64
    }

    /// How far below the best score a move may fall and still be played, in
    /// centipawns; 0 from `NODES_FROM_ELO` up, where the best move is played.
    pub(crate) fn margin(self) -> i32 {
        let Some(i) = MARGINS.windows(2).position(|w| self.elo <= w[1].0) else {
            return 0;
        };
        let ((low_elo, low_margin), (high_elo, high_margin)) = (MARGINS[i], MARGINS[i + 1]);
        let t = (self.elo.max(low_elo) - low_elo) as i32;
        let span = (high_elo - low_elo) as i32;
        low_margin + (high_margin - low_margin) * t / span
    }
}

/// Picks a move from the scored root moves. Every move within `margin`
/// centipawns of the best can be picked, with a chance that grows the closer
/// it scores to the best; with a margin of 0, the best move. `None` if there
/// are no moves.
pub(crate) fn choose_near_best(scores: &[(Move, i32)], margin: i32, rng: &mut Rng) -> Option<Move> {
    let best = scores.iter().map(|&(_, score)| score).max()?;
    let candidates: Vec<(Move, u64)> = scores
        .iter()
        .filter(|&&(_, score)| best - score <= margin)
        .map(|&(mv, score)| (mv, (margin - (best - score) + 1) as u64))
        .collect();
    let total: u64 = candidates.iter().map(|&(_, weight)| weight).sum();
    let mut ticket = rng.next_u64() % total;
    for &(mv, weight) in &candidates {
        if ticket < weight {
            return Some(mv);
        }
        ticket -= weight;
    }
    candidates.last().map(|&(mv, _)| mv)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{MoveKind, Square};

    #[test]
    fn the_budget_follows_the_measurements() {
        let nodes = |elo| Strength::new(elo).nodes();
        // At the measured points, the measured budgets.
        assert_eq!(nodes(1020), 100);
        assert_eq!(nodes(1310), 1_000);
        assert_eq!(nodes(1500), 3_000);
        assert_eq!(nodes(2000), 100_000);
        // Halfway between two points, the geometric mean.
        assert_eq!(nodes(1580), 5_477);
        // The weakest settings keep the smallest budget; the strongest stays
        // below the last point.
        assert_eq!(nodes(Strength::MIN_ELO), 100);
        assert!((300_000..1_000_000).contains(&nodes(Strength::MAX_ELO)));
        // More Elo never means fewer nodes.
        let budgets: Vec<u64> = (Strength::MIN_ELO..=Strength::MAX_ELO).map(nodes).collect();
        assert!(budgets.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn only_weak_settings_draw_their_moves() {
        let margin = |elo| Strength::new(elo).margin();
        assert_eq!(margin(Strength::MIN_ELO), 220);
        assert_eq!(margin(650), 150);
        assert_eq!(margin(830), 38);
        assert_eq!(margin(NODES_FROM_ELO), 0);
        assert_eq!(margin(Strength::MAX_ELO), 0);
        let margins: Vec<i32> = (Strength::MIN_ELO..=Strength::MAX_ELO)
            .map(margin)
            .collect();
        assert!(margins.windows(2).all(|w| w[0] >= w[1]));
        // Out-of-range values are brought into the range.
        assert_eq!(Strength::new(0), Strength::new(Strength::MIN_ELO));
        assert_eq!(Strength::new(9999), Strength::new(Strength::MAX_ELO));
    }

    #[test]
    fn choices_stay_within_the_margin() {
        let mv = |to| Move::new(Square::E2, to, MoveKind::Normal);
        let (good, close, blunder) = (mv(Square::E3), mv(Square::E4), mv(Square::D3));
        let scores = [(good, 100), (close, 90), (blunder, -500)];
        let mut rng = Rng::new(3);

        // A margin of 0 always plays the best move.
        for _ in 0..100 {
            assert_eq!(choose_near_best(&scores, 0, &mut rng), Some(good));
        }

        // The weakest setting varies between the two close moves, but never
        // throws away 600 centipawns.
        let weakest = Strength::new(Strength::MIN_ELO).margin();
        let picks: Vec<Move> = (0..1000)
            .map(|_| choose_near_best(&scores, weakest, &mut rng).unwrap())
            .collect();
        assert!(picks.contains(&good) && picks.contains(&close));
        assert!(!picks.contains(&blunder));
        // Closer moves are more likely: the best one is picked more often.
        let count = |mv| picks.iter().filter(|&&pick| pick == mv).count();
        assert!(count(good) > count(close));

        assert_eq!(choose_near_best(&[], weakest, &mut rng), None);
    }
}
