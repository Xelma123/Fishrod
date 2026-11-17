//! Move ordering. Alpha-beta cuts off sooner when the best move comes first,
//! so moves are tried in order of promise: the move an earlier search found
//! best here, then the captures and promotions that lose no material, then
//! the killer moves, then the other quiet moves by their history, and last
//! the captures that lose material once the opponent takes back.
//!
//! Killers and history are learned during the search. A quiet move that
//! refuted the opponent's move in one position often refutes it in the
//! positions next to it in the tree too: the killer moves are the last two
//! such moves at each ply. The history keeps a score for every move by its
//! squares, raised whenever the move causes a cutoff and lowered whenever it
//! is tried in vain before another move does.

use super::see::loses_no_material;
use crate::board::Position;
use crate::types::{Color, Move, MoveKind, MoveList, PieceType};

/// Ordering score of the move an earlier search found best here.
const FIRST_MOVE_SCORE: i32 = 1_000_000;
/// Base ordering score of captures and promotions.
const TACTICAL_SCORE: i32 = 100_000;
/// Base ordering score of captures and promotions that lose material: below
/// every quiet move.
const LOSING_TACTICAL_SCORE: i32 = -100_000;
/// Ordering score of the second killer move; the first scores one more.
const KILLER_SCORE: i32 = 50_000;
/// Rough piece values used only for ordering, pawn to king.
const ORDER_VALUE: [i32; 6] = [1, 3, 3, 5, 9, 0];

/// History scores stay within `-MAX_HISTORY..=MAX_HISTORY`, below the
/// killers' ordering score.
pub(super) const MAX_HISTORY: i32 = 16_384;

/// The killer moves of one ply, most recent first. `Move::NULL` marks an
/// empty slot: it never matches a legal move.
pub(super) type Killers = [Move; 2];

/// Records `mv` as the newest killer move.
pub(super) fn add_killer(killers: &mut Killers, mv: Move) {
    if killers[0] != mv {
        killers[1] = killers[0];
        killers[0] = mv;
    }
}

/// A score for every quiet move, by side to move and squares.
pub(super) struct History {
    scores: Box<[[[i32; 64]; 64]; 2]>,
}

impl History {
    pub(super) fn new() -> History {
        History {
            scores: Box::new([[[0; 64]; 64]; 2]),
        }
    }

    pub(super) fn get(&self, side: Color, mv: Move) -> i32 {
        self.scores[side.index()][mv.from().index()][mv.to().index()]
    }

    /// Adds `bonus` (negative for a penalty) to the move's score. The step
    /// shrinks as the score nears its bound, so scores never leave it and
    /// recent results weigh more than old ones.
    pub(super) fn update(&mut self, side: Color, mv: Move, bonus: i32) {
        let bonus = bonus.clamp(-MAX_HISTORY, MAX_HISTORY);
        let score = &mut self.scores[side.index()][mv.from().index()][mv.to().index()];
        *score += bonus - *score * bonus.abs() / MAX_HISTORY;
    }
}

/// The history bonus for a cutoff at `depth`: deeper cutoffs say more.
pub(super) fn history_bonus(depth: i32) -> i32 {
    (16 * depth * depth).min(2048)
}

/// The moves of a position, handed out best first. Each call to `next`
/// picks the best of the remaining moves, so after a cutoff the rest of the
/// list is never sorted at all.
pub(super) struct OrderedMoves {
    moves: MoveList,
    scores: [i32; MoveList::CAPACITY],
    next: usize,
}

impl OrderedMoves {
    /// Orders `moves`, putting `first_move` first if it is among them. A
    /// remembered move that is not legal here, after a hash collision, is
    /// thereby ignored; so are killers that are not legal here.
    pub(super) fn new(
        pos: &Position,
        moves: MoveList,
        first_move: Option<Move>,
        killers: &Killers,
        history: &History,
    ) -> OrderedMoves {
        let side = pos.side_to_move();
        let mut scores = [0; MoveList::CAPACITY];
        for (score, &mv) in scores.iter_mut().zip(moves.iter()) {
            *score = if Some(mv) == first_move {
                FIRST_MOVE_SCORE
            } else if let Some(tactical) = tactical_score(pos, mv) {
                if loses_no_material(pos, mv) {
                    tactical
                } else {
                    tactical - TACTICAL_SCORE + LOSING_TACTICAL_SCORE
                }
            } else if mv == killers[0] {
                KILLER_SCORE + 1
            } else if mv == killers[1] {
                KILLER_SCORE
            } else {
                history.get(side, mv)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::movegen::legal_moves;
    use crate::types::Square;

    fn quiet(from: Square, to: Square) -> Move {
        Move::new(from, to, MoveKind::Normal)
    }

    #[test]
    fn history_scores_stay_bounded() {
        let mut history = History::new();
        let mv = quiet(Square::G1, Square::F3);
        for _ in 0..1000 {
            history.update(Color::White, mv, history_bonus(20));
        }
        let high = history.get(Color::White, mv);
        assert!(
            (MAX_HISTORY * 9 / 10..=MAX_HISTORY).contains(&high),
            "{high}"
        );
        // The other side's score for the same squares is separate.
        assert_eq!(history.get(Color::Black, mv), 0);
        for _ in 0..1000 {
            history.update(Color::White, mv, -history_bonus(20));
        }
        let low = history.get(Color::White, mv);
        assert!(
            (-MAX_HISTORY..=-MAX_HISTORY * 9 / 10).contains(&low),
            "{low}"
        );
    }

    #[test]
    fn killers_keep_the_two_latest_distinct_moves() {
        let mut killers = [Move::NULL; 2];
        let (a, b, c) = (
            quiet(Square::A2, Square::A3),
            quiet(Square::B2, Square::B3),
            quiet(Square::C2, Square::C3),
        );
        add_killer(&mut killers, a);
        add_killer(&mut killers, a);
        assert_eq!(killers, [a, Move::NULL]);
        add_killer(&mut killers, b);
        add_killer(&mut killers, c);
        assert_eq!(killers, [c, b]);
    }

    #[test]
    fn captures_that_lose_material_come_last() {
        // Qxd6 loses the queen to cxd6: every quiet move comes before it.
        let pos = Position::from_fen("4k3/2p5/3p4/8/8/8/8/3QK3 w - - 0 1").unwrap();
        let order: Vec<Move> = OrderedMoves::new(
            &pos,
            legal_moves(&pos),
            None,
            &[Move::NULL; 2],
            &History::new(),
        )
        .collect();
        assert_eq!(order.last(), Some(&quiet(Square::D1, Square::D6)));
    }

    #[test]
    fn moves_come_in_order_of_promise() {
        // White can take the d5 pawn with the e4 pawn or the queen.
        let pos = Position::from_fen("4k3/8/8/3p4/4P3/8/8/3QK1N1 w - - 0 1").unwrap();
        let moves = legal_moves(&pos);
        let first = quiet(Square::E1, Square::F2);
        let killer = quiet(Square::G1, Square::F3);
        let mut history = History::new();
        let favourite = quiet(Square::D1, Square::H5);
        history.update(Color::White, favourite, 500);
        let disliked = quiet(Square::D1, Square::D2);
        history.update(Color::White, disliked, -500);

        let order: Vec<Move> = OrderedMoves::new(
            &pos,
            moves.clone(),
            Some(first),
            &[killer, Move::NULL],
            &history,
        )
        .collect();
        assert_eq!(order.len(), moves.len());
        assert_eq!(order[0], first);
        assert_eq!(order[1], quiet(Square::E4, Square::D5));
        assert_eq!(order[2], quiet(Square::D1, Square::D5));
        assert_eq!(order[3], killer);
        assert_eq!(order[4], favourite);
        assert_eq!(order.last(), Some(&disliked));
    }
}
