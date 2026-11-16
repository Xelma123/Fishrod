//! Search: looking ahead through the tree of moves for the best one.
//!
//! The core is negamax alpha-beta. Every position is scored from the point of
//! view of its side to move, so a single function serves both sides: a
//! reply's score is negated on the way back up. `alpha` is the score the side
//! to move is already sure of and `beta` the most the opponent will allow. A
//! move that scores `beta` or more refutes the opponent's previous move, so
//! the remaining moves need not be searched: a cutoff.
//!
//! When the depth runs out, a quiescence search keeps playing captures until
//! the position is quiet, so that no position is judged in the middle of an
//! exchange. Iterative deepening searches to depth 1, 2, 3, ... in turn; each
//! iteration tries the previous one's best line first, which makes cutoffs
//! come early.

mod ordering;
pub(crate) mod strength;

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use self::ordering::{OrderedMoves, tactical_score};
use crate::board::Position;
use crate::eval::evaluate;
use crate::movegen::legal_moves;
use crate::types::{Move, MoveList, PieceType};

/// The score of delivering mate right now. Mate in `n` plies scores
/// `MATE - n`, so quicker mates score higher.
pub const MATE: i32 = 32_000;
/// Above every reachable score.
pub const INFINITY: i32 = 32_001;
/// The deepest the search goes, counting quiescence plies too.
pub const MAX_PLY: usize = 128;
const DRAW: i32 = 0;

/// The clock and the stop flag are checked once per this many nodes: reading
/// the clock costs more than searching a node.
const CHECK_INTERVAL: u64 = 1024;

/// The number of moves to a forced mate, if `score` announces one: positive
/// when the side to move mates, negative when it gets mated.
pub fn mate_in(score: i32) -> Option<i32> {
    let plies = MATE - score.abs();
    if plies > MAX_PLY as i32 {
        return None;
    }
    Some(if score > 0 {
        (plies + 1) / 2
    } else {
        -(plies / 2)
    })
}

/// When to stop searching. Unset limits do not apply.
#[derive(Clone, Copy, Debug, Default)]
pub struct Limits {
    /// Iterations to complete.
    pub depth: Option<u32>,
    /// Nodes to visit.
    pub nodes: Option<u64>,
    /// No new iteration starts once this much time has passed.
    pub soft_time: Option<Duration>,
    /// The search stops once this much time has passed, even mid-iteration.
    pub hard_time: Option<Duration>,
    /// Search every root move with a full window, so that each one gets an
    /// exact score in `Outcome::root_scores`. Slower: there are no cutoffs at
    /// the root. Used to play below full strength.
    pub exact_root_scores: bool,
}

/// The result of one completed iteration, for progress output.
pub struct Report<'a> {
    pub depth: u32,
    pub score: i32,
    pub nodes: u64,
    pub time: Duration,
    /// The principal variation: the line both sides are expected to play.
    pub pv: &'a [Move],
}

/// What the search decided.
#[derive(Clone, Debug)]
pub struct Outcome {
    /// `None` when the root position has no legal moves.
    pub best_move: Option<Move>,
    pub score: i32,
    /// The deepest completed iteration.
    pub depth: u32,
    pub nodes: u64,
    /// Every root move with its exact score from the deepest completed
    /// iteration, if `Limits::exact_root_scores` was set; empty otherwise.
    pub root_scores: Vec<(Move, i32)>,
}

/// Searches `root` within `limits` and returns the best move found.
///
/// `history` holds the keys of the earlier positions of the game since the
/// last irreversible move, oldest first; repeating one of them is a draw.
/// Setting `stop` ends the search early. Time limits and `stop` only take
/// effect once the first iteration is complete, so a searched move is
/// always available. `report` is called after every completed iteration.
pub fn search(
    root: &Position,
    history: &[u64],
    limits: Limits,
    stop: &AtomicBool,
    mut report: impl FnMut(&Report),
) -> Outcome {
    let start = Instant::now();
    let mut searcher = Searcher {
        nodes: 0,
        node_limit: limits.nodes.unwrap_or(u64::MAX),
        stop,
        start,
        hard_time: limits.hard_time,
        interruptible: false,
        stopped: false,
        path: history.to_vec(),
        prev_pv: MoveList::new(),
        exact_root: limits.exact_root_scores,
        root_scores: Vec::new(),
    };

    // Until an iteration completes, any legal move will have to do.
    let root_moves = legal_moves(root);
    let mut outcome = Outcome {
        best_move: root_moves.first().copied(),
        score: DRAW,
        depth: 0,
        nodes: 0,
        root_scores: Vec::new(),
    };
    if outcome.best_move.is_none() {
        if !root.checkers().is_empty() {
            outcome.score = -MATE;
        }
        return outcome;
    }

    let max_depth = limits
        .depth
        .unwrap_or(MAX_PLY as u32)
        .clamp(1, MAX_PLY as u32 - 1);
    let mut pv = MoveList::new();
    for depth in 1..=max_depth {
        let score = searcher.negamax(root, depth as i32, -INFINITY, INFINITY, 0, true, &mut pv);
        // An interrupted iteration may have missed better moves: drop it.
        if searcher.stopped {
            break;
        }
        outcome = Outcome {
            best_move: pv.first().copied().or(outcome.best_move),
            score,
            depth,
            nodes: searcher.nodes,
            root_scores: std::mem::take(&mut searcher.root_scores),
        };
        report(&Report {
            depth,
            score,
            nodes: searcher.nodes,
            time: start.elapsed(),
            pv: &pv,
        });
        searcher.prev_pv = pv.clone();
        searcher.interruptible = true;

        // Another iteration would likely take several times as long as this
        // one, so none is started once the soft limit has passed. With a
        // single legal move there is nothing to choose, under a clock.
        let timed = limits.soft_time.is_some();
        let soft_limit_passed = limits.soft_time.is_some_and(|soft| start.elapsed() >= soft);
        if soft_limit_passed || (timed && root_moves.len() == 1) || stop.load(Ordering::Relaxed) {
            break;
        }
    }
    outcome.nodes = searcher.nodes;
    outcome
}

/// The state of one search.
struct Searcher<'a> {
    nodes: u64,
    node_limit: u64,
    stop: &'a AtomicBool,
    start: Instant,
    hard_time: Option<Duration>,
    /// Whether time and the stop flag may cut the search short: true once the
    /// first iteration is complete.
    interruptible: bool,
    /// Set once a limit is reached; every node then returns at once.
    stopped: bool,
    /// Keys of the positions before the current node: the game history, then
    /// the search path down to the current node's parent.
    path: Vec<u64>,
    /// The principal variation of the previous iteration, tried first.
    prev_pv: MoveList,
    /// Whether root moves get a full window (`Limits::exact_root_scores`).
    exact_root: bool,
    /// Root moves and their scores in the current iteration, when `exact_root` is set.
    root_scores: Vec<(Move, i32)>,
}

impl Searcher<'_> {
    /// Counts a node and tells whether the search must stop.
    fn visit(&mut self) -> bool {
        self.nodes += 1;
        if self.nodes >= self.node_limit {
            self.stopped = true;
        } else if self.interruptible && self.nodes.is_multiple_of(CHECK_INTERVAL) {
            let out_of_time = self
                .hard_time
                .is_some_and(|hard| self.start.elapsed() >= hard);
            if out_of_time || self.stop.load(Ordering::Relaxed) {
                self.stopped = true;
            }
        }
        self.stopped
    }

    /// Alpha-beta search of `pos` to `depth` plies. Fills `pv` with the best
    /// line found when it beats `alpha`. `on_pv` tells whether every move from
    /// the root to here followed the previous iteration's best line.
    #[allow(clippy::too_many_arguments)]
    fn negamax(
        &mut self,
        pos: &Position,
        depth: i32,
        mut alpha: i32,
        beta: i32,
        ply: usize,
        on_pv: bool,
        pv: &mut MoveList,
    ) -> i32 {
        pv.clear();
        if depth <= 0 {
            return self.quiescence(pos, alpha, beta, ply);
        }
        if self.visit() {
            return DRAW; // thrown away: the iteration is dropped
        }
        // At the root a move must be found even in a drawn position.
        if ply > 0 && (self.is_repetition(pos) || insufficient_material(pos)) {
            return DRAW;
        }
        if ply >= MAX_PLY - 1 {
            return evaluate(pos);
        }

        let moves = legal_moves(pos);
        if moves.is_empty() {
            return if pos.checkers().is_empty() {
                DRAW // stalemate
            } else {
                -MATE + ply as i32
            };
        }
        // Checked after mate: a mate on the hundredth half-move still counts.
        if ply > 0 && pos.halfmove_clock() >= 100 {
            return DRAW;
        }

        let pv_move = if on_pv {
            self.prev_pv.get(ply).copied()
        } else {
            None
        };
        // With exact root scores every root move gets the full window, so no
        // move is merely shown to be worse than the best: each is scored.
        let exact = ply == 0 && self.exact_root;
        if exact {
            self.root_scores.clear();
        }
        let mut best = -INFINITY;
        let mut child_pv = MoveList::new();
        self.path.push(pos.key());
        for mv in OrderedMoves::new(pos, moves, pv_move) {
            let (child_alpha, child_beta) = if exact {
                (-INFINITY, INFINITY)
            } else {
                (-beta, -alpha)
            };
            let score = -self.negamax(
                &pos.make_move(mv),
                depth - 1,
                child_alpha,
                child_beta,
                ply + 1,
                Some(mv) == pv_move,
                &mut child_pv,
            );
            if self.stopped {
                break;
            }
            if exact {
                self.root_scores.push((mv, score));
            }
            if score > best {
                best = score;
                if score > alpha {
                    alpha = score;
                    pv.clear();
                    pv.push(mv);
                    for &reply in child_pv.iter() {
                        pv.push(reply);
                    }
                    if alpha >= beta {
                        break;
                    }
                }
            }
        }
        self.path.pop();
        best
    }

    /// Searches only captures and promotions (all moves when in check) until
    /// the position is quiet. Outside check the side to move may also "stand
    /// pat": decline every capture and take the static evaluation.
    fn quiescence(&mut self, pos: &Position, mut alpha: i32, beta: i32, ply: usize) -> i32 {
        if self.visit() {
            return DRAW;
        }
        if ply >= MAX_PLY - 1 {
            return evaluate(pos);
        }

        let in_check = !pos.checkers().is_empty();
        let mut best = -INFINITY;
        if !in_check {
            best = evaluate(pos);
            if best >= beta {
                return best;
            }
            alpha = alpha.max(best);
        }

        let moves = legal_moves(pos);
        if moves.is_empty() {
            return if in_check { -MATE + ply as i32 } else { DRAW };
        }
        let mut candidates = MoveList::new();
        for &mv in moves.iter() {
            if in_check || tactical_score(pos, mv).is_some() {
                candidates.push(mv);
            }
        }

        for mv in OrderedMoves::new(pos, candidates, None) {
            let score = -self.quiescence(&pos.make_move(mv), -beta, -alpha, ply + 1);
            if self.stopped {
                break;
            }
            if score > best {
                best = score;
                if score > alpha {
                    alpha = score;
                    if alpha >= beta {
                        break;
                    }
                }
            }
        }
        best
    }

    /// Whether `pos` repeats an earlier position of the game or the search
    /// path. Only positions since the last capture or pawn move can match,
    /// and only every second one has the same side to move.
    fn is_repetition(&self, pos: &Position) -> bool {
        let window = usize::from(pos.halfmove_clock()).min(self.path.len());
        (2..=window)
            .step_by(2)
            .any(|distance| self.path[self.path.len() - distance] == pos.key())
    }
}

/// Whether neither side has enough material left to mate: no pawns, rooks or
/// queens, and at most one knight or bishop on the board.
fn insufficient_material(pos: &Position) -> bool {
    let decisive =
        pos.pieces(PieceType::Pawn) | pos.pieces(PieceType::Rook) | pos.pieces(PieceType::Queen);
    let minors = pos.pieces(PieceType::Knight) | pos.pieces(PieceType::Bishop);
    decisive.is_empty() && !minors.more_than_one()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{MoveKind, Square};

    fn depth(depth: u32) -> Limits {
        Limits {
            depth: Some(depth),
            ..Limits::default()
        }
    }

    fn search_quietly(pos: &Position, history: &[u64], limits: Limits) -> Outcome {
        search(pos, history, limits, &AtomicBool::new(false), |_| {})
    }

    fn run(fen: &str, plies: u32) -> Outcome {
        search_quietly(&Position::from_fen(fen).unwrap(), &[], depth(plies))
    }

    fn best_move(outcome: &Outcome) -> String {
        outcome
            .best_move
            .map(|mv| mv.to_uci(false))
            .unwrap_or_default()
    }

    #[test]
    fn mate_scores_count_moves() {
        assert_eq!(mate_in(MATE - 1), Some(1));
        assert_eq!(mate_in(MATE - 3), Some(2));
        assert_eq!(mate_in(-(MATE - 2)), Some(-1));
        assert_eq!(mate_in(150), None);
        assert_eq!(mate_in(-150), None);
    }

    #[test]
    fn finds_mate_in_one() {
        // A back-rank mate: Ra8#.
        let outcome = run("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1", 3);
        assert_eq!(best_move(&outcome), "a1a8");
        assert_eq!(mate_in(outcome.score), Some(1));
    }

    #[test]
    fn finds_mate_in_two() {
        // 1. Ra7 Kg8 2. Rb8#; no mate in one exists.
        let outcome = run("7k/8/8/8/8/8/R7/1R4K1 w - - 0 1", 4);
        assert_eq!(mate_in(outcome.score), Some(2));
    }

    #[test]
    fn sees_its_own_mate_coming() {
        // Kg8 is forced, then Rb8#.
        let outcome = run("7k/R7/8/8/8/8/8/1R4K1 b - - 0 1", 3);
        assert_eq!(best_move(&outcome), "h8g8");
        assert_eq!(mate_in(outcome.score), Some(-1));
    }

    #[test]
    fn prefers_mate_to_stalemate() {
        // Qf7 would stalemate; Qe8# wins.
        let outcome = run("7k/8/4Q1K1/8/8/8/8/8 w - - 0 1", 3);
        assert_eq!(mate_in(outcome.score), Some(1));
    }

    #[test]
    fn no_move_at_the_root() {
        let stalemate = run("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1", 3);
        assert_eq!((stalemate.best_move, stalemate.score), (None, 0));
        let mated = run("R5k1/5ppp/8/8/8/8/8/6K1 b - - 0 1", 3);
        assert_eq!((mated.best_move, mated.score), (None, -MATE));
    }

    #[test]
    fn takes_a_hanging_queen() {
        let outcome = run("4k3/8/8/3q4/8/8/8/3RK3 w - - 0 1", 2);
        assert_eq!(best_move(&outcome), "d1d5");
    }

    #[test]
    fn draws_score_zero() {
        // King and bishop cannot mate.
        assert_eq!(run("8/8/4k3/8/8/2KB4/8/8 w - - 0 1", 3).score, 0);
        // Black is lost, but every move reaches the hundredth half-move.
        assert_eq!(run("7k/8/8/8/8/8/8/KQ6 b - - 99 80", 3).score, 0);
        // A mate on the hundredth half-move still wins.
        let outcome = run("7k/8/4Q1K1/8/8/8/8/8 w - - 99 80", 3);
        assert_eq!(mate_in(outcome.score), Some(1));
    }

    #[test]
    fn a_lost_side_repeats_the_position() {
        // White is far ahead. After Qd2 Nf6 Qd1, black can go back with Ng8
        // and repeat the first position: a draw, which beats every other move.
        use Square as S;
        let mut pos = Position::from_fen("6nk/8/8/8/8/8/8/K2Q4 w - - 0 1").unwrap();
        let mut history = Vec::new();
        for (from, to) in [(S::D1, S::D2), (S::G8, S::F6), (S::D2, S::D1)] {
            history.push(pos.key());
            pos = pos.make_move(Move::new(from, to, MoveKind::Normal));
        }
        let outcome = search_quietly(&pos, &history, depth(3));
        assert_eq!(
            outcome.best_move,
            Some(Move::new(S::F6, S::G8, MoveKind::Normal))
        );
        assert_eq!(outcome.score, 0);
    }

    #[test]
    fn reports_every_iteration_with_a_legal_line() {
        let pos = Position::startpos();
        let mut depths = Vec::new();
        let outcome = search(&pos, &[], depth(4), &AtomicBool::new(false), |report| {
            depths.push(report.depth);
            // Every move of the line must be legal in turn.
            let mut line_pos = pos;
            for &mv in report.pv {
                assert!(legal_moves(&line_pos).contains(&mv));
                line_pos = line_pos.make_move(mv);
            }
        });
        assert_eq!(depths, [1, 2, 3, 4]);
        assert_eq!(outcome.depth, 4);
    }

    #[test]
    fn node_limit_stops_the_search() {
        let limits = Limits {
            nodes: Some(5_000),
            ..Limits::default()
        };
        let outcome = search_quietly(&Position::startpos(), &[], limits);
        assert!(outcome.best_move.is_some());
        assert!(outcome.nodes <= 5_000);
    }

    #[test]
    fn stop_flag_ends_the_search_after_the_first_iteration() {
        let stop = AtomicBool::new(true);
        let outcome = search(&Position::startpos(), &[], Limits::default(), &stop, |_| {});
        assert_eq!(outcome.depth, 1);
        assert!(outcome.best_move.is_some());
    }

    #[test]
    fn time_limits_end_the_search() {
        let kiwipete = Position::from_fen(
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        )
        .unwrap();
        let limits = Limits {
            soft_time: Some(Duration::from_millis(20)),
            hard_time: Some(Duration::from_millis(50)),
            ..Limits::default()
        };
        let start = Instant::now();
        let outcome = search_quietly(&kiwipete, &[], limits);
        assert!(outcome.depth >= 1 && outcome.best_move.is_some());
        // Generous, so a slow debug build or a busy machine does not fail it.
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "{:?}",
            start.elapsed()
        );
    }

    #[test]
    fn exact_root_scores_cover_every_move() {
        let pos = Position::from_fen("4k3/8/8/3q4/8/8/8/3RK3 w - - 0 1").unwrap();
        let limits = Limits {
            exact_root_scores: true,
            ..depth(3)
        };
        let outcome = search_quietly(&pos, &[], limits);
        assert_eq!(outcome.root_scores.len(), legal_moves(&pos).len());
        let best = outcome.root_scores.iter().max_by_key(|&&(_, score)| score);
        assert_eq!(best, Some(&(outcome.best_move.unwrap(), outcome.score)));
        // Taking the queen is far better than any other move.
        let others = outcome
            .root_scores
            .iter()
            .filter(|&&(mv, _)| Some(mv) != outcome.best_move);
        assert!(
            others
                .into_iter()
                .all(|&(_, score)| score < outcome.score - 500)
        );
        // The same search without exact scores agrees on the move and score.
        let plain = run("4k3/8/8/3q4/8/8/8/3RK3 w - - 0 1", 3);
        assert_eq!(
            (plain.best_move, plain.score),
            (outcome.best_move, outcome.score)
        );
        assert!(plain.root_scores.is_empty());
    }

    #[test]
    fn a_single_legal_move_is_played_at_once_under_a_clock() {
        // Double check from the bishop and the rook: Kg8 is the only escape.
        let pos = Position::from_fen("7k/8/8/4B3/8/8/8/K6R b - - 0 1").unwrap();
        assert_eq!(legal_moves(&pos).len(), 1);
        let limits = Limits {
            soft_time: Some(Duration::from_secs(60)),
            hard_time: Some(Duration::from_secs(60)),
            ..Limits::default()
        };
        assert_eq!(search_quietly(&pos, &[], limits).depth, 1);
    }

    #[test]
    fn search_is_deterministic() {
        let fen = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1";
        let first = run(fen, 4);
        let second = run(fen, 4);
        assert_eq!(first.nodes, second.nodes);
        assert_eq!(first.best_move, second.best_move);
        assert_eq!(first.score, second.score);
    }
}
