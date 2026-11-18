//! Search: looking ahead through the tree of moves for the best one.
//!
//! The core is negamax alpha-beta. Every position is scored from the point of
//! view of its side to move, so a single function serves both sides: a
//! reply's score is negated on the way back up. `alpha` is the score the side
//! to move is already sure of and `beta` the most the opponent will allow. A
//! move that scores `beta` or more refutes the opponent's previous move, so
//! the remaining moves need not be searched: a cutoff. Principal variation
//! search narrows the window to a single point for every move but the first:
//! proving a move no better than the best so far is cheaper than scoring it.
//! Null move pruning cuts off a node when even passing the turn, searched a
//! few plies shallower, keeps the score at beta, and reverse futility pruning
//! when the evaluation is far above beta near the leaves. Late move
//! reductions search the quiet moves at the back of the list less deeply.
//!
//! When the depth runs out, a quiescence search keeps playing captures until
//! the position is quiet, so that no position is judged in the middle of an
//! exchange. Iterative deepening searches to depth 1, 2, 3, ... in turn; each
//! iteration tries the previous one's best line first, which makes cutoffs
//! come early. The transposition table remembers what was found about each
//! position, across iterations and across searches.

mod ordering;
mod reductions;
mod see;
pub(crate) mod strength;
mod tt;

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use self::ordering::{History, Killers, OrderedMoves, add_killer, history_bonus, tactical_score};
use self::reductions::reduction as reduction_for;
use self::see::loses_no_material;
pub use self::tt::TranspositionTable;
use self::tt::{Bound, Entry};
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
/// No killer moves, for a fresh ply and for quiescence.
const NO_KILLERS: Killers = [Move::NULL; 2];
/// The shallowest depth at which null move pruning is tried.
const NULL_MOVE_MIN_DEPTH: i32 = 3;
/// The shallowest depth at which late moves are reduced.
const LMR_MIN_DEPTH: i32 = 3;
/// Reverse futility pruning applies up to this depth, when the evaluation
/// exceeds beta by this margin per ply left, in centipawns.
const RFP_MAX_DEPTH: i32 = 6;
const RFP_MARGIN: i32 = 80;
/// Late move pruning applies up to this depth; see `lmp_move_count`.
const LMP_MAX_DEPTH: i32 = 4;
/// Futility pruning applies up to this depth, when the evaluation plus
/// `FUTILITY_BASE + FUTILITY_MARGIN * depth` centipawns stays at or below alpha.
const FUTILITY_MAX_DEPTH: i32 = 6;
const FUTILITY_BASE: i32 = 100;
const FUTILITY_MARGIN: i32 = 100;
/// Scores beyond this announce a forced mate.
pub(crate) const MATE_BOUND: i32 = MATE - MAX_PLY as i32;
/// Aspiration windows start at this depth, this many centipawns either side
/// of the previous iteration's score.
const ASPIRATION_MIN_DEPTH: u32 = 4;
const ASPIRATION_DELTA: i32 = 25;
/// Internal iterative reduction applies from this depth.
const IIR_MIN_DEPTH: i32 = 4;
/// A search counts as in trouble (see `Limits::trouble_time`) from this
/// depth on, when its score falls by this many centipawns from one iteration
/// to the next or is this many centipawns below level.
const TROUBLE_MIN_DEPTH: u32 = 6;
const TROUBLE_DROP: i32 = 30;
const TROUBLE_SCORE: i32 = 100;

/// How many moves a node at `depth` searches before late move pruning
/// skips its remaining quiet moves.
const fn lmp_move_count(depth: i32) -> usize {
    (3 + depth * depth) as usize
}

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
    /// More time for a search in trouble: once an iteration's score falls
    /// clearly below the previous one's, or shows a clearly worse position,
    /// the hard limit becomes this and the soft limit halfway between it and
    /// `hard_time`. Unset, the limits never change.
    pub trouble_time: Option<Duration>,
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
/// The search reads and fills `tt`; the same table should serve every search
/// of a game, and be cleared for a new one. Setting `stop` ends the search
/// early. Time limits and `stop` only take effect once the first iteration
/// is complete, so a searched move is always available. `report` is called
/// after every completed iteration.
pub fn search(
    root: &Position,
    history: &[u64],
    limits: Limits,
    tt: &TranspositionTable,
    stop: &AtomicBool,
    mut report: impl FnMut(&Report),
) -> Outcome {
    let start = Instant::now();
    tt.new_search();
    let mut searcher = Searcher {
        tt,
        nodes: 0,
        node_limit: limits.nodes.unwrap_or(u64::MAX),
        stop,
        start,
        hard_time: limits.hard_time,
        interruptible: false,
        stopped: false,
        path: history.to_vec(),
        prev_pv: MoveList::new(),
        killers: [NO_KILLERS; MAX_PLY],
        quiet_history: History::new(),
        null_child_ply: None,
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
    let mut soft_time = limits.soft_time;
    let mut in_trouble = false;
    for depth in 1..=max_depth {
        // Aspiration windows: the score rarely moves far from one iteration
        // to the next, so the search starts with a narrow window around the
        // previous score, which cuts off more. A score outside the window is
        // only a bound: the window widens on that side and the search
        // repeats. Not with exact root scores, nor around a mate score.
        let mut delta = ASPIRATION_DELTA;
        let aspirate = depth >= ASPIRATION_MIN_DEPTH
            && !limits.exact_root_scores
            && mate_in(outcome.score).is_none();
        let (mut alpha, mut beta) = if aspirate {
            (outcome.score - delta, outcome.score + delta)
        } else {
            (-INFINITY, INFINITY)
        };
        let score = loop {
            let score = searcher.negamax(root, depth as i32, alpha, beta, 0, true, &mut pv);
            if searcher.stopped {
                break score;
            }
            if score <= alpha {
                alpha = (score - delta).max(-INFINITY);
            } else if score >= beta {
                beta = (score + delta).min(INFINITY);
            } else {
                break score;
            }
            delta *= 2;
        };
        // An interrupted iteration may have missed better moves: drop it.
        if searcher.stopped {
            break;
        }
        // A search that finds itself in trouble gets more time, once.
        if let Some(trouble_time) = limits.trouble_time
            && !in_trouble
            && is_trouble(outcome.score, score, depth)
        {
            in_trouble = true;
            soft_time = limits.hard_time.map(|hard| (hard + trouble_time) / 2);
            searcher.hard_time = Some(trouble_time);
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
        let timed = soft_time.is_some();
        let soft_limit_passed = soft_time.is_some_and(|soft| start.elapsed() >= soft);
        if soft_limit_passed || (timed && root_moves.len() == 1) || stop.load(Ordering::Relaxed) {
            break;
        }
    }
    outcome.nodes = searcher.nodes;
    outcome
}

/// The state of one search.
struct Searcher<'a> {
    tt: &'a TranspositionTable,
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
    /// The killer moves of each ply, kept across iterations.
    killers: [Killers; MAX_PLY],
    /// The history scores of quiet moves, kept across iterations.
    quiet_history: History,
    /// The ply of the node reached by a null move, while it is searched:
    /// that node must not pass again.
    null_child_ply: Option<usize>,
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

        // A node searched with a window wider than one point can still
        // change the principal variation: a PV node. Every other node only
        // has to show that it fails high or low.
        let pv_node = beta - alpha > 1;

        // An earlier search of this position, at least as deep as this one,
        // may already settle it. PV nodes, the root among them, are searched
        // anyway, so that the principal variation stays whole and exact.
        let stored = self.tt.probe(pos.key(), ply);
        if !pv_node
            && let Some(entry) = stored
            && entry.depth >= depth
        {
            let settled = match entry.bound {
                Bound::Exact => true,
                Bound::Lower => entry.score >= beta,
                Bound::Upper => entry.score <= alpha,
            };
            if settled {
                return entry.score;
            }
        }

        let in_check = !pos.checkers().is_empty();
        let moves = legal_moves(pos);
        if moves.is_empty() {
            return if in_check {
                -MATE + ply as i32
            } else {
                DRAW // stalemate
            };
        }
        // Checked after mate: a mate on the hundredth half-move still counts.
        if ply > 0 && pos.halfmove_clock() >= 100 {
            return DRAW;
        }

        let mut child_pv = MoveList::new();

        // The static evaluation, for the pruning below; only outside PV
        // nodes and check, where the pruning applies.
        let static_eval = (!pv_node && !in_check).then(|| evaluate(pos));

        // Reverse futility pruning: near the leaves, a position already far
        // above beta is unlikely to drop below it in the few plies left, so
        // the evaluation is taken as the score. Not when beta is a mate score.
        if let Some(eval) = static_eval
            && depth <= RFP_MAX_DEPTH
            && mate_in(beta).is_none()
            && eval - RFP_MARGIN * depth >= beta
        {
            return eval;
        }

        // Null move pruning: let the opponent move twice in a row. If a
        // shallower search shows that even then the score stays at beta or
        // above, a real move would almost surely do as well: cut off. Not in
        // check (the king could be taken), not right after another pass, and
        // not with only pawns left besides the king, where having to move can
        // be a burden (zugzwang) and passing would flatter the position.
        if let Some(eval) = static_eval
            && eval >= beta
            && depth >= NULL_MOVE_MIN_DEPTH
            && self.null_child_ply != Some(ply)
            && has_pieces(pos)
        {
            let reduction = 3 + depth / 4;
            let saved = self.null_child_ply.replace(ply + 1);
            self.path.push(pos.key());
            let score = -self.negamax(
                &pos.make_null_move(),
                depth - 1 - reduction,
                -beta,
                -beta + 1,
                ply + 1,
                false,
                &mut child_pv,
            );
            self.path.pop();
            self.null_child_ply = saved;
            if self.stopped {
                return DRAW;
            }
            if score >= beta {
                // A mate found after passing says nothing about real moves.
                return if mate_in(score).is_some() {
                    beta
                } else {
                    score
                };
            }
        }

        let pv_move = if on_pv {
            self.prev_pv.get(ply).copied()
        } else {
            None
        };
        // The move to try first: the previous iteration's best line, or else
        // the best move the table remembers.
        let first_move = pv_move.or(stored.and_then(|entry| entry.mv));
        // Internal iterative reduction: with no move from an earlier search
        // to try first, the ordering is only a guess, so the node is searched
        // a ply shallower. A later visit finds the move this search stores.
        let depth = if depth >= IIR_MIN_DEPTH && first_move.is_none() {
            depth - 1
        } else {
            depth
        };
        // With exact root scores every root move gets the full window, so no
        // move is merely shown to be worse than the best: each is scored.
        let exact = ply == 0 && self.exact_root;
        if exact {
            self.root_scores.clear();
        }
        let original_alpha = alpha;
        let mut best = -INFINITY;
        let mut best_move = None;
        // Quiet moves searched without a cutoff, to be penalized if a later
        // move cuts off.
        let mut quiets_tried = MoveList::new();
        let ordered = OrderedMoves::new(
            pos,
            moves,
            first_move,
            &self.killers[ply],
            &self.quiet_history,
        );
        self.path.push(pos.key());
        for (index, mv) in ordered.enumerate() {
            let is_quiet = tactical_score(pos, mv).is_none();
            let child = pos.make_move(mv);
            let gives_check = !child.checkers().is_empty();
            let on_line = Some(mv) == pv_move;
            // Check extension: a move that gives check is searched a ply
            // deeper, so that forcing lines are seen to their end.
            let new_depth = depth - 1 + i32::from(gives_check);

            // Shallow pruning of quiet moves, outside PV nodes and check,
            // once a move has been searched and the node is not already lost
            // to a mate. Late move pruning: near the leaves, the quiet moves
            // at the back of the list are skipped. Futility pruning: when the
            // evaluation is so far below alpha that a quiet move cannot be
            // expected to close the gap, it is skipped.
            if let Some(eval) = static_eval
                && index > 0
                && is_quiet
                && !gives_check
                && best > -MATE_BOUND
            {
                let late = depth <= LMP_MAX_DEPTH && index >= lmp_move_count(depth);
                let futile = depth <= FUTILITY_MAX_DEPTH
                    && eval + FUTILITY_BASE + FUTILITY_MARGIN * depth <= alpha;
                if late || futile {
                    continue;
                }
            }

            let mut score;
            if exact {
                score = -self.negamax(
                    &child,
                    new_depth,
                    -INFINITY,
                    INFINITY,
                    ply + 1,
                    on_line,
                    &mut child_pv,
                );
            } else if index == 0 {
                score = -self.negamax(
                    &child,
                    new_depth,
                    -beta,
                    -alpha,
                    ply + 1,
                    on_line,
                    &mut child_pv,
                );
            } else {
                // Late move reductions: a quiet move this far down the list
                // is unlikely to be best, so it is first searched shallower.
                // Never for a move that gives check or escapes one.
                let mut reduction = 0;
                if depth >= LMR_MIN_DEPTH && is_quiet && !in_check && !gives_check {
                    reduction = reduction_for(depth, index) - i32::from(pv_node);
                    reduction = reduction.clamp(0, depth - 2);
                }
                // Principal variation search: the first move is most likely
                // the best, so a later move only has to be shown no better
                // than alpha, which a null window around alpha does quickly.
                score = -self.negamax(
                    &child,
                    new_depth - reduction,
                    -alpha - 1,
                    -alpha,
                    ply + 1,
                    on_line,
                    &mut child_pv,
                );
                // A reduced move that beats alpha after all gets its full depth.
                if reduction > 0 && score > alpha && !self.stopped {
                    score = -self.negamax(
                        &child,
                        new_depth,
                        -alpha - 1,
                        -alpha,
                        ply + 1,
                        on_line,
                        &mut child_pv,
                    );
                }
                // A move that does better than alpha at full depth is
                // searched again with the full window, for its exact score
                // and line.
                if score > alpha && score < beta && !self.stopped {
                    score = -self.negamax(
                        &child,
                        new_depth,
                        -beta,
                        -alpha,
                        ply + 1,
                        on_line,
                        &mut child_pv,
                    );
                }
            }
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
                    best_move = Some(mv);
                    pv.clear();
                    pv.push(mv);
                    for &reply in child_pv.iter() {
                        pv.push(reply);
                    }
                    if alpha >= beta {
                        if is_quiet {
                            self.reward_cutoff(pos, mv, depth, ply, &quiets_tried);
                        }
                        break;
                    }
                }
            }
            if is_quiet {
                quiets_tried.push(mv);
            }
        }
        self.path.pop();

        // An interrupted search knows nothing for sure: store nothing.
        if !self.stopped {
            let bound = if best >= beta {
                Bound::Lower
            } else if best > original_alpha {
                Bound::Exact
            } else {
                Bound::Upper
            };
            let entry = Entry {
                mv: best_move,
                score: best,
                depth,
                bound,
            };
            self.tt.store(pos.key(), entry, ply);
        }
        best
    }

    /// Learns from a quiet move that cut off at `ply`: it becomes a killer
    /// move there, its history score rises, and the quiet moves tried before
    /// it in vain lose history score.
    fn reward_cutoff(
        &mut self,
        pos: &Position,
        mv: Move,
        depth: i32,
        ply: usize,
        tried_in_vain: &MoveList,
    ) {
        add_killer(&mut self.killers[ply], mv);
        let side = pos.side_to_move();
        let bonus = history_bonus(depth);
        self.quiet_history.update(side, mv, bonus);
        for &other in tried_in_vain.iter() {
            self.quiet_history.update(side, other, -bonus);
        }
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
        // Captures that lose material once the opponent takes back cannot
        // raise the score above standing pat, so they are not searched.
        let mut candidates = MoveList::new();
        for &mv in moves.iter() {
            if in_check || (tactical_score(pos, mv).is_some() && loses_no_material(pos, mv)) {
                candidates.push(mv);
            }
        }

        let ordered = OrderedMoves::new(pos, candidates, None, &NO_KILLERS, &self.quiet_history);
        for mv in ordered {
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

/// Whether a completed iteration at `depth`, scoring `score` after the
/// previous iteration scored `previous`, shows the search in trouble: its
/// score has just fallen clearly, or the position is clearly worse. Shallow
/// iterations are too unsettled to tell.
fn is_trouble(previous: i32, score: i32, depth: u32) -> bool {
    depth >= TROUBLE_MIN_DEPTH && (score <= previous - TROUBLE_DROP || score <= -TROUBLE_SCORE)
}

/// Whether the side to move has a knight, bishop, rook or queen.
fn has_pieces(pos: &Position) -> bool {
    let us = pos.side_to_move();
    [
        PieceType::Knight,
        PieceType::Bishop,
        PieceType::Rook,
        PieceType::Queen,
    ]
    .into_iter()
    .any(|piece_type| !pos.pieces_of(us, piece_type).is_empty())
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

    fn table() -> TranspositionTable {
        TranspositionTable::new(1).unwrap()
    }

    fn search_quietly(pos: &Position, history: &[u64], limits: Limits) -> Outcome {
        search(
            pos,
            history,
            limits,
            &table(),
            &AtomicBool::new(false),
            |_| {},
        )
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
        let stop = AtomicBool::new(false);
        let outcome = search(&pos, &[], depth(4), &table(), &stop, |report| {
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
        let outcome = search(
            &Position::startpos(),
            &[],
            Limits::default(),
            &table(),
            &stop,
            |_| {},
        );
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

    #[test]
    fn the_table_carries_over_between_searches() {
        let kiwipete = Position::from_fen(
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        )
        .unwrap();
        let tt = table();
        let stop = AtomicBool::new(false);
        let first = search(&kiwipete, &[], depth(5), &tt, &stop, |_| {});
        // The second search finds most of the work already done.
        let second = search(&kiwipete, &[], depth(5), &tt, &stop, |_| {});
        assert!(
            second.nodes * 2 < first.nodes,
            "{} then {}",
            second.nodes,
            first.nodes
        );
        assert!(legal_moves(&kiwipete).contains(&second.best_move.unwrap()));

        // A cleared table is as good as a new one: same search, same count.
        tt.clear();
        let after_clear = search(&kiwipete, &[], depth(5), &tt, &stop, |_| {});
        assert_eq!(after_clear.nodes, first.nodes);
        assert_eq!(after_clear.best_move, first.best_move);
    }

    #[test]
    fn trouble_is_a_falling_score_or_a_worse_position() {
        assert!(is_trouble(20, -15, 8));
        assert!(is_trouble(0, -150, 8));
        // Small swings, rising scores and good positions are no trouble.
        assert!(!is_trouble(20, 0, 8));
        assert!(!is_trouble(-50, 40, 8));
        assert!(!is_trouble(300, 290, 12));
        // Shallow iterations do not count.
        assert!(!is_trouble(0, -500, TROUBLE_MIN_DEPTH - 1));
    }

    #[test]
    fn a_search_in_trouble_takes_more_time() {
        // Black has lost the queen: clearly worse.
        let lost =
            Position::from_fen("rnb1kbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR b KQkq - 0 1").unwrap();
        let plain = Limits {
            soft_time: Some(Duration::from_millis(100)),
            hard_time: Some(Duration::from_millis(300)),
            ..Limits::default()
        };
        let timed = |limits| {
            let start = Instant::now();
            search_quietly(&lost, &[], limits);
            start.elapsed()
        };
        // Without extra time, the hard limit holds.
        let elapsed = timed(plain);
        assert!(elapsed < Duration::from_millis(800), "{elapsed:?}");
        // With it, the search keeps going at least until the plain hard limit.
        let elapsed = timed(Limits {
            trouble_time: Some(Duration::from_millis(1200)),
            ..plain
        });
        assert!(elapsed >= Duration::from_millis(300), "{elapsed:?}");
        assert!(elapsed < Duration::from_millis(3000), "{elapsed:?}");
    }

    #[test]
    fn a_fixed_move_time_is_used_in_full() {
        // With equal soft and hard limits (`go movetime`), the search thinks
        // until the limit.
        let limits = Limits {
            soft_time: Some(Duration::from_millis(150)),
            hard_time: Some(Duration::from_millis(150)),
            ..Limits::default()
        };
        let start = Instant::now();
        search_quietly(&Position::startpos(), &[], limits);
        assert!(start.elapsed() >= Duration::from_millis(140));
    }

    #[test]
    fn null_moves_are_kept_out_of_pawn_endgames() {
        let pawns_only = Position::from_fen("4k3/4p3/8/8/8/8/4P3/4K3 w - - 0 1").unwrap();
        assert!(!has_pieces(&pawns_only));
        // Only the side to move counts: white has a knight, black does not.
        let knight = Position::from_fen("4k3/4p3/8/8/8/8/4P3/4KN2 w - - 0 1").unwrap();
        assert!(has_pieces(&knight));
        assert!(!has_pieces(&knight.make_null_move()));
    }

    #[test]
    fn the_principal_variation_stays_whole_with_a_full_table() {
        // The second search finds every position of the line already in the
        // table, deep enough; PV nodes must still be searched, not cut short.
        let pos = Position::startpos();
        let tt = table();
        let stop = AtomicBool::new(false);
        search(&pos, &[], depth(6), &tt, &stop, |_| {});
        let mut lengths = Vec::new();
        search(&pos, &[], depth(6), &tt, &stop, |report| {
            lengths.push(report.pv.len());
        });
        assert_eq!(lengths, [1, 2, 3, 4, 5, 6]);
    }
}
