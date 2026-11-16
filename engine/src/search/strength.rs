//! Playing below full strength (`UCI_LimitStrength`).
//!
//! Two knobs, both turned by one Elo setting: the search goes less deep, and
//! the move played is drawn at random from those that score not much worse
//! than the best, with closer moves more likely. The weaker the setting, the
//! shallower the search and the wider the choice, so weak settings miss
//! tactics and now and then play a plain mistake, much as people do.
//!
//! The Elo figures are rough labels until they are measured against rated
//! opponents.

use crate::rng::Rng;
use crate::types::Move;

/// A playing strength between `MIN_ELO` and `MAX_ELO`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Strength {
    elo: u32,
}

impl Strength {
    pub(crate) const MIN_ELO: u32 = 500;
    pub(crate) const MAX_ELO: u32 = 2000;
    pub(crate) const DEFAULT_ELO: u32 = 1500;

    /// The strength for `elo`, brought into the supported range.
    pub(crate) fn new(elo: u32) -> Strength {
        Strength {
            elo: elo.clamp(Self::MIN_ELO, Self::MAX_ELO),
        }
    }

    /// How deep to search: 1 at the bottom of the range, one more for every
    /// 300 Elo, up to 6 at the top.
    pub(crate) fn depth(self) -> u32 {
        1 + (self.elo - Self::MIN_ELO) / 300
    }

    /// How far below the best score a move may fall and still be played, in
    /// centipawns: 300 at the bottom of the range, 0 at the top.
    pub(crate) fn margin(self) -> i32 {
        ((Self::MAX_ELO - self.elo) / 5) as i32
    }

    /// Picks a move from the scored root moves. Every move within the margin
    /// of the best can be picked, with a chance that grows the closer it
    /// scores to the best. `None` if there are no moves.
    pub(crate) fn choose(self, scores: &[(Move, i32)], rng: &mut Rng) -> Option<Move> {
        let best = scores.iter().map(|&(_, score)| score).max()?;
        let margin = self.margin();
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{MoveKind, Square};

    #[test]
    fn knobs_follow_the_elo() {
        assert_eq!(Strength::new(Strength::MIN_ELO).depth(), 1);
        assert_eq!(Strength::new(Strength::MAX_ELO).depth(), 6);
        assert_eq!(Strength::new(Strength::MIN_ELO).margin(), 300);
        assert_eq!(Strength::new(Strength::MAX_ELO).margin(), 0);
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

        // Full strength always plays the best move.
        let strongest = Strength::new(Strength::MAX_ELO);
        for _ in 0..100 {
            assert_eq!(strongest.choose(&scores, &mut rng), Some(good));
        }

        // The weakest setting varies between the two close moves, but never
        // throws away 600 centipawns.
        let weakest = Strength::new(Strength::MIN_ELO);
        let picks: Vec<Move> = (0..1000)
            .map(|_| weakest.choose(&scores, &mut rng).unwrap())
            .collect();
        assert!(picks.contains(&good) && picks.contains(&close));
        assert!(!picks.contains(&blunder));

        assert_eq!(weakest.choose(&[], &mut rng), None);
    }
}
