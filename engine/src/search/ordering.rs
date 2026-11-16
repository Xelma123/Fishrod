//! Move ordering. Alpha-beta cuts off sooner when the best move comes first,
//! so moves are tried in order of promise: the previous iteration's best
//! move, then captures and promotions, then quiet moves.

use crate::board::Position;
use crate::types::{Move, MoveKind, MoveList, PieceType};

/// Ordering score of the move the previous iteration found best here.
const PV_SCORE: i32 = 1_000_000;
/// Base ordering score of captures and promotions.
const TACTICAL_SCORE: i32 = 100_000;
/// Rough piece values used only for ordering, pawn to king.
const ORDER_VALUE: [i32; 6] = [1, 3, 3, 5, 9, 0];

/// The moves of a position, handed out best first. Each call to `next`
/// picks the best of the remaining moves, so after a cutoff the rest of the
/// list is never sorted at all.
pub(super) struct OrderedMoves {
    moves: MoveList,
    scores: [i32; MoveList::CAPACITY],
    next: usize,
}

impl OrderedMoves {
    pub(super) fn new(pos: &Position, moves: MoveList, pv_move: Option<Move>) -> OrderedMoves {
        let mut scores = [0; MoveList::CAPACITY];
        for (score, &mv) in scores.iter_mut().zip(moves.iter()) {
            *score = if Some(mv) == pv_move {
                PV_SCORE
            } else {
                tactical_score(pos, mv).unwrap_or(0)
            };
        }
        OrderedMoves {
            moves,
            scores,
            next: 0,
        }
    }
}

impl Iterator for OrderedMoves {
    type Item = Move;

    fn next(&mut self) -> Option<Move> {
        let start = self.next;
        if start >= self.moves.len() {
            return None;
        }
        // Ties keep the generation order, so the search stays deterministic.
        let mut best = start;
        for i in start + 1..self.moves.len() {
            if self.scores[i] > self.scores[best] {
                best = i;
            }
        }
        self.moves.swap(start, best);
        self.scores.swap(start, best);
        self.next += 1;
        Some(self.moves[start])
    }
}

/// The ordering score of a capture or promotion, or `None` for a quiet move.
///
/// MVV-LVA, "most valuable victim, least valuable attacker": taking a queen
/// comes before taking a pawn, and among captures of the same piece, taking
/// with a pawn comes before taking with a queen. A promotion counts its new
/// piece as part of the gain.
pub(super) fn tactical_score(pos: &Position, mv: Move) -> Option<i32> {
    let on_target = pos.piece_on(mv.to()).map(|piece| piece.piece_type());
    let (victim, promotion) = match mv.kind() {
        // The castling rook stands on the target square, but nothing is captured.
        MoveKind::Castle => return None,
        MoveKind::EnPassant => (Some(PieceType::Pawn), None),
        MoveKind::Promotion(piece_type) => (on_target, Some(piece_type)),
        _ => (on_target, None),
    };
    if victim.is_none() && promotion.is_none() {
        return None;
    }
    let gain = victim.map_or(0, |v| ORDER_VALUE[v.index()])
        + promotion.map_or(0, |p| ORDER_VALUE[p.index()]);
    let attacker = pos
        .piece_on(mv.from())
        .map_or(0, |piece| piece.piece_type().index() as i32);
    Some(TACTICAL_SCORE + 16 * gain - attacker)
}
