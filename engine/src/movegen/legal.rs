//! Legal move generation.
//!
//! Only legal moves are produced, so no move has to be played and examined
//! afterwards to see whether it left the king in check. Three facts make this
//! possible:
//!
//! - The king may only step to squares the opponent does not attack.
//! - In check from one piece, any other move must capture the checker or step
//!   between it and the king. In double check only the king can move.
//! - A piece pinned to its king may only move along the pin line.
//!
//! En passant is the exception: it removes two pawns from one rank at once and
//! can expose the king sideways, so it is checked on the resulting board.

use super::attacks::{
    between, bishop_attacks, king_attacks, knight_attacks, line, pawn_attacks, rook_attacks,
};
use crate::board::{CastlingSide, Position};
use crate::types::{Bitboard, Color, Move, MoveKind, MoveList, PieceType, Square};

/// All legal moves in `pos`.
pub fn legal_moves(pos: &Position) -> MoveList {
    generate::<false>(pos)
}

/// The legal captures and promotions in `pos`, en passant included: the
/// moves a quiescence search tries. They come in the same order as in
/// `legal_moves`, which only adds the other moves among them.
pub fn legal_captures(pos: &Position) -> MoveList {
    generate::<true>(pos)
}

/// Whether the side to move has any legal move. Usually answered without
/// generating the moves: out of check, a knight or slider that is not pinned
/// and has somewhere to go settles it.
pub fn has_legal_move(pos: &Position) -> bool {
    let us = pos.side_to_move();
    if pos.checkers().is_empty() {
        let pinned = pinned_pieces(pos, pos.king_square(us));
        let free = !pos.occupied_by(us);
        let occupied = pos.occupied();
        let queens = pos.pieces_of(us, PieceType::Queen);
        let unpinned = |pieces: Bitboard| pieces & !pinned;
        let can_move = unpinned(pos.pieces_of(us, PieceType::Knight))
            .into_iter()
            .any(|from| !(knight_attacks(from) & free).is_empty())
            || unpinned(pos.pieces_of(us, PieceType::Bishop) | queens)
                .into_iter()
                .any(|from| !(bishop_attacks(from, occupied) & free).is_empty())
            || unpinned(pos.pieces_of(us, PieceType::Rook) | queens)
                .into_iter()
                .any(|from| !(rook_attacks(from, occupied) & free).is_empty());
        if can_move {
            return true;
        }
    }
    !legal_moves(pos).is_empty()
}

/// The legal moves of `pos`, or only its captures and promotions if
/// `TACTICAL` is set. A constant parameter, so that each kind is compiled
/// on its own and the full generation pays nothing for the other.
fn generate<const TACTICAL: bool>(pos: &Position) -> MoveList {
    let mut list = MoveList::new();
    let us = pos.side_to_move();
    let king = pos.king_square(us);
    let checkers = pos.checkers();

    king_moves::<TACTICAL>(pos, king, &mut list);
    if checkers.more_than_one() {
        return list;
    }

    // Where the other pieces may go: anywhere but onto our own pieces, or, in
    // check, only onto the checker or the squares between it and the king.
    let targets = if checkers.is_empty() {
        !pos.occupied_by(us)
    } else {
        between(king, checkers.lsb()) | checkers
    };
    let pinned = pinned_pieces(pos, king);
    let allowed = |from: Square| {
        if pinned.contains(from) {
            targets & line(king, from)
        } else {
            targets
        }
    };

    piece_moves::<TACTICAL>(pos, &allowed, &mut list);
    pawn_moves::<TACTICAL>(pos, &allowed, &mut list);
    if checkers.is_empty() && !TACTICAL {
        castling_moves(pos, king, &mut list);
    }
    list
}

/// The squares a move may go to: any square, or with `TACTICAL` only those
/// of the opponent's pieces.
fn destinations<const TACTICAL: bool>(pos: &Position) -> Bitboard {
    if TACTICAL {
        pos.occupied_by(!pos.side_to_move())
    } else {
        Bitboard::FULL
    }
}

/// King steps to squares the opponent does not attack; with `TACTICAL`,
/// captures only.
fn king_moves<const TACTICAL: bool>(pos: &Position, king: Square, list: &mut MoveList) {
    let us = pos.side_to_move();
    let theirs = pos.occupied_by(!us);
    // Lift the king off the board first: a slider checking it along a line
    // also attacks the square behind it, which the king itself hides now.
    let without_king = pos.occupied().without(king);
    for to in king_attacks(king) & !pos.occupied_by(us) & destinations::<TACTICAL>(pos) {
        if (pos.attackers_to(to, without_king) & theirs).is_empty() {
            list.push(Move::new(king, to, MoveKind::Normal));
        }
    }
}

/// Our pieces pinned to our king: each stands alone between the king and an
/// enemy slider that would otherwise attack the king.
fn pinned_pieces(pos: &Position, king: Square) -> Bitboard {
    let us = pos.side_to_move();
    let them = !us;
    let queens = pos.pieces_of(them, PieceType::Queen);
    // Enemy sliders on a line with the king, whatever stands in between.
    let snipers = (rook_attacks(king, Bitboard::EMPTY)
        & (pos.pieces_of(them, PieceType::Rook) | queens))
        | (bishop_attacks(king, Bitboard::EMPTY)
            & (pos.pieces_of(them, PieceType::Bishop) | queens));

    let mut pinned = Bitboard::EMPTY;
    for sniper in snipers {
        let blockers = between(king, sniper) & pos.occupied();
        if blockers.popcount() == 1 {
            pinned |= blockers & pos.occupied_by(us);
        }
    }
    pinned
}

/// Knight, bishop, rook and queen moves. `allowed` gives each piece's
/// permitted destinations; with `TACTICAL`, captures only.
fn piece_moves<const TACTICAL: bool>(
    pos: &Position,
    allowed: &impl Fn(Square) -> Bitboard,
    list: &mut MoveList,
) {
    let us = pos.side_to_move();
    let occupied = pos.occupied();
    let mask = destinations::<TACTICAL>(pos);
    let mut add = |from: Square, attacks: Bitboard| {
        for to in attacks & allowed(from) & mask {
            list.push(Move::new(from, to, MoveKind::Normal));
        }
    };

    for from in pos.pieces_of(us, PieceType::Knight) {
        add(from, knight_attacks(from));
    }
    // A queen moves like a bishop and like a rook; the two sets of
    // destinations never overlap, so it is simply handled twice.
    let queens = pos.pieces_of(us, PieceType::Queen);
    for from in pos.pieces_of(us, PieceType::Bishop) | queens {
        add(from, bishop_attacks(from, occupied));
    }
    for from in pos.pieces_of(us, PieceType::Rook) | queens {
        add(from, rook_attacks(from, occupied));
    }
}

/// Pawn pushes, captures, promotions and en passant captures. With
/// `TACTICAL`, pushes are left out unless they promote.
fn pawn_moves<const TACTICAL: bool>(
    pos: &Position,
    allowed: &impl Fn(Square) -> Bitboard,
    list: &mut MoveList,
) {
    let us = pos.side_to_move();
    let occupied = pos.occupied();
    let theirs = pos.occupied_by(!us);
    let (forward, start_rank, last_rank) = match us {
        Color::White => (1, 1, 7),
        Color::Black => (-1, 6, 0),
    };

    for from in pos.pieces_of(us, PieceType::Pawn) {
        let allowed = allowed(from);

        // Pushes need empty squares; the double push also needs the square it skips to be empty.
        if let Some(one) = from.offset(0, forward)
            && !occupied.contains(one)
        {
            if allowed.contains(one) && (!TACTICAL || one.rank() == last_rank) {
                push_pawn_move(list, from, one);
            }
            if !TACTICAL
                && from.rank() == start_rank
                && let Some(two) = one.offset(0, forward)
                && !occupied.contains(two)
                && allowed.contains(two)
            {
                list.push(Move::new(from, two, MoveKind::DoublePush));
            }
        }

        for to in pawn_attacks(us, from) & theirs & allowed {
            push_pawn_move(list, from, to);
        }

        if let Some(target) = pos.en_passant()
            && pawn_attacks(us, from).contains(target)
            && en_passant_is_legal(pos, from, target)
        {
            list.push(Move::new(from, target, MoveKind::EnPassant));
        }
    }
}

/// Adds a pawn move, or the four promotions if it reaches the last rank.
fn push_pawn_move(list: &mut MoveList, from: Square, to: Square) {
    if to.rank() == 0 || to.rank() == 7 {
        for piece_type in [
            PieceType::Queen,
            PieceType::Rook,
            PieceType::Bishop,
            PieceType::Knight,
        ] {
            list.push(Move::new(from, to, MoveKind::Promotion(piece_type)));
        }
    } else {
        list.push(Move::new(from, to, MoveKind::Normal));
    }
}

/// Whether the en passant capture from `from` to `to` leaves our king safe,
/// judged on the board as it would be after the capture.
fn en_passant_is_legal(pos: &Position, from: Square, to: Square) -> bool {
    let us = pos.side_to_move();
    let captured = Square::from_coords(to.file(), from.rank());
    let occupied = pos.occupied().without(from).without(captured).with(to);
    let enemies = pos.occupied_by(!us).without(captured);
    (pos.attackers_to(pos.king_square(us), occupied) & enemies).is_empty()
}

/// Castling moves, for standard chess and Chess960 alike. Only called when
/// the side to move is not in check.
fn castling_moves(pos: &Position, king: Square, list: &mut MoveList) {
    let us = pos.side_to_move();
    let theirs = pos.occupied_by(!us);
    for side in CastlingSide::ALL {
        let Some(rook) = pos.castling_rook(us, side) else {
            continue;
        };
        // In Chess960 the king and rook may stand on each other's path, so
        // both are left out of the board for the checks below.
        let others = pos.occupied().without(king).without(rook);
        let king_path = between(king, side.king_destination(us)).with(side.king_destination(us));
        let rook_path = between(rook, side.rook_destination(us)).with(side.rook_destination(us));
        if !((king_path | rook_path) & others).is_empty() {
            continue;
        }
        // The king may not cross or land on an attacked square. The rook stays
        // off the board here too: it may be shielding a square it is about to leave.
        let safe = king_path
            .into_iter()
            .all(|sq| (pos.attackers_to(sq, others) & theirs).is_empty());
        if safe {
            list.push(Move::new(king, rook, MoveKind::Castle));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The legal moves of the position, in UCI notation.
    fn moves(fen: &str, chess960: bool) -> Vec<String> {
        let pos = Position::from_fen(fen).unwrap();
        legal_moves(&pos)
            .iter()
            .map(|mv| mv.to_uci(chess960))
            .collect()
    }

    fn castles(fen: &str) -> usize {
        let pos = Position::from_fen(fen).unwrap();
        legal_moves(&pos)
            .iter()
            .filter(|mv| mv.kind() == MoveKind::Castle)
            .count()
    }

    #[test]
    fn starting_position() {
        let moves = moves(Position::STARTPOS_FEN, false);
        assert_eq!(moves.len(), 20);
        for mv in ["a2a3", "e2e4", "g1f3", "b1a3"] {
            assert!(moves.contains(&mv.to_string()), "{mv}");
        }
    }

    #[test]
    fn castling_rules() {
        // Both sides free.
        let both = moves("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1", false);
        assert!(both.contains(&"e1g1".to_string()) && both.contains(&"e1c1".to_string()));
        // f1 is attacked: the king may not cross it. Queen side is still fine.
        let no_short = moves("4kr2/8/8/8/8/8/8/R3K2R w KQ - 0 1", false);
        assert!(!no_short.contains(&"e1g1".to_string()));
        assert!(no_short.contains(&"e1c1".to_string()));
        // b1 is attacked, but only the rook crosses it, so castling is legal.
        assert_eq!(castles("1r2k3/8/8/8/8/8/8/R3K2R w KQ - 0 1"), 2);
        // Pieces in the way.
        assert_eq!(castles("4k3/8/8/8/8/8/8/RN2K1NR w KQ - 0 1"), 0);
        // In check.
        assert_eq!(castles("4k3/8/8/8/8/8/4r3/R3K2R w KQ - 0 1"), 0);
    }

    #[test]
    fn chess960_castling() {
        // The castling rook on b1 shields c1 from the rook on a1. Once it moves
        // away the king would stand in check on c1, so castling is illegal.
        assert_eq!(castles("4k3/8/8/8/8/8/8/rR2K3 w B - 0 1"), 0);
        assert!(moves("4k3/8/8/8/8/8/8/1R2K3 w B - 0 1", true).contains(&"e1b1".to_string()));
        // The king already stands on g1; only the rook moves.
        assert!(moves("4k3/8/8/8/8/8/8/6KR w H - 0 1", true).contains(&"g1h1".to_string()));
    }

    #[test]
    fn en_passant_rules() {
        // bxc6 would take both pawns off the fifth rank and expose the king to the rook.
        let pinned = moves("8/8/8/KPp4r/8/8/8/7k w - c6 0 1", false);
        assert!(!pinned.contains(&"b5c6".to_string()));
        assert!(pinned.contains(&"b5b6".to_string()));
        // exd3 removes the pawn that gives check.
        let evasion = moves("8/8/8/2k5/3Pp3/8/8/4K3 b - d3 0 1", false);
        assert!(evasion.contains(&"e4d3".to_string()));
    }

    #[test]
    fn pinned_pieces_stay_on_the_pin_line() {
        // The rook on e2 is pinned by the rook on e7 and may only move along the e-file.
        let pos = Position::from_fen("4k3/4r3/8/8/8/8/4R3/4K3 w - - 0 1").unwrap();
        let rook_moves: Vec<Move> = legal_moves(&pos)
            .iter()
            .copied()
            .filter(|mv| mv.from() == Square::E2)
            .collect();
        assert_eq!(rook_moves.len(), 5);
        assert!(rook_moves.iter().all(|mv| mv.to().file() == 4));
    }

    #[test]
    fn double_check_allows_only_king_moves() {
        // Knight c2 and rook e4 both give check; the rook on c1 cannot help.
        let pos = Position::from_fen("4k3/8/8/8/4r3/8/2n5/2R1K3 w - - 0 1").unwrap();
        let moves = legal_moves(&pos);
        assert_eq!(moves.len(), 4);
        assert!(moves.iter().all(|mv| mv.from() == Square::E1));
    }

    /// The legal moves of every position in the perft tree of `fen` up to
    /// `depth`, visited one by one.
    fn for_each_position(pos: &Position, depth: u32, visit: &mut impl FnMut(&Position)) {
        visit(pos);
        if depth > 0 {
            for &mv in legal_moves(pos).iter() {
                for_each_position(&pos.make_move(mv), depth - 1, visit);
            }
        }
    }

    const TREES: [(&str, u32); 5] = [
        (Position::STARTPOS_FEN, 3),
        (
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            2,
        ),
        ("8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1", 4),
        (
            "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
            3,
        ),
        (
            "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
            2,
        ),
    ];

    #[test]
    fn captures_are_the_tactical_moves_in_the_same_order() {
        for (fen, depth) in TREES {
            let root = Position::from_fen(fen).unwrap();
            for_each_position(&root, depth, &mut |pos| {
                let expected: Vec<Move> = legal_moves(pos)
                    .iter()
                    .copied()
                    .filter(|&mv| {
                        mv.kind() != MoveKind::Castle
                            && (pos.piece_on(mv.to()).is_some()
                                || matches!(
                                    mv.kind(),
                                    MoveKind::EnPassant | MoveKind::Promotion(_)
                                ))
                    })
                    .collect();
                assert_eq!(legal_captures(pos).to_vec(), expected, "{pos:?}");
            });
        }
    }

    #[test]
    fn the_quick_legal_move_check_agrees() {
        for (fen, depth) in TREES {
            let root = Position::from_fen(fen).unwrap();
            for_each_position(&root, depth, &mut |pos| {
                assert_eq!(has_legal_move(pos), !legal_moves(pos).is_empty(), "{pos:?}");
            });
        }
        // Checkmate and stalemate.
        let mated = Position::from_fen("R5k1/5ppp/8/8/8/8/8/6K1 b - - 0 1").unwrap();
        let stalemated = Position::from_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").unwrap();
        assert!(!has_legal_move(&mated) && !has_legal_move(&stalemated));
    }

    #[test]
    fn promotions_come_in_fours() {
        let moves = moves("1r2k3/P7/8/8/8/8/8/4K3 w - - 0 1", false);
        for promotion in ["a7a8q", "a7a8r", "a7a8b", "a7a8n", "a7b8q", "a7b8n"] {
            assert!(moves.contains(&promotion.to_string()), "{promotion}");
        }
    }
}
