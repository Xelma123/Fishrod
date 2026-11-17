//! Static exchange evaluation (SEE): what a capture wins or loses in material
//! once both sides have made every recapture on its square that pays.
//!
//! Each side recaptures with its least valuable piece first, and may stop
//! whenever going on would lose material. Pieces behind the capturers (a
//! rook behind a rook, a bishop behind a queen) join in as the ones in front
//! leave the square's lines. Pins are ignored: a pinned piece is counted as
//! a capturer. This is the swap-list method described on the Chess
//! Programming Wiki ("SEE - The Swap Algorithm").

use crate::board::Position;
use crate::types::{Bitboard, Move, MoveKind, PieceType, Square};

/// Piece values for exchanges, pawn to king. The king never gets captured:
/// it only recaptures when the square is no longer defended.
const VALUE: [i32; 6] = [100, 300, 300, 500, 900, 0];

/// The most captures an exchange on one square can take: every piece but
/// the two kings, plus room.
const MAX_SWAPS: usize = 32;

/// The material balance of `mv` for the side making it, in centipawns,
/// after the best sequence of recaptures on its target square. Quiet moves
/// and castling score 0 unless the moved piece can be taken.
pub(super) fn see(pos: &Position, mv: Move) -> i32 {
    if mv.kind() == MoveKind::Castle {
        return 0;
    }
    let (from, to) = (mv.from(), mv.to());
    let us = pos.side_to_move();
    let mut occupied = pos.occupied().without(from);

    // The first capture, and the piece then standing on the target square.
    let mut gain = [0; MAX_SWAPS];
    let mut on_square = pos
        .piece_on(from)
        .map_or(0, |p| VALUE[p.piece_type().index()]);
    match mv.kind() {
        MoveKind::EnPassant => {
            gain[0] = VALUE[PieceType::Pawn.index()];
            let captured = Square::from_coords(to.file(), from.rank());
            occupied = occupied.without(captured);
        }
        MoveKind::Promotion(piece_type) => {
            let captured = pos
                .piece_on(to)
                .map_or(0, |p| VALUE[p.piece_type().index()]);
            gain[0] = captured + VALUE[piece_type.index()] - VALUE[PieceType::Pawn.index()];
            on_square = VALUE[piece_type.index()];
        }
        _ => {
            gain[0] = pos
                .piece_on(to)
                .map_or(0, |p| VALUE[p.piece_type().index()])
        }
    }

    let mut side = !us;
    let mut depth = 0;
    loop {
        let attackers = pos.attackers_to(to, occupied) & occupied;
        let ours = attackers & pos.occupied_by(side);
        let Some((square, piece_type)) = least_valuable(pos, ours) else {
            break;
        };
        // The king may only take when nothing of the other side defends.
        if piece_type == PieceType::King && !(attackers & pos.occupied_by(!side)).is_empty() {
            break;
        }
        depth += 1;
        if depth >= MAX_SWAPS {
            break;
        }
        // Taking the piece on the square, which can then be taken back.
        gain[depth] = on_square - gain[depth - 1];
        // Neither side can do better from here: stop early.
        if (-gain[depth - 1]).max(gain[depth]) < 0 {
            break;
        }
        on_square = VALUE[piece_type.index()];
        occupied = occupied.without(square);
        side = !side;
    }

    // Each side takes back only when that pays: fold the list from the end.
    while depth > 0 {
        gain[depth - 1] = -(-gain[depth - 1]).max(gain[depth]);
        depth -= 1;
    }
    gain[0]
}

/// Whether `mv` loses no material in the exchange it starts. Taking a piece
/// worth at least the capturer never does, which spares working out the
/// whole exchange for most captures.
pub(super) fn loses_no_material(pos: &Position, mv: Move) -> bool {
    if mv.kind() == MoveKind::Normal
        && let (Some(victim), Some(capturer)) = (pos.piece_on(mv.to()), pos.piece_on(mv.from()))
        && VALUE[victim.piece_type().index()] >= VALUE[capturer.piece_type().index()]
    {
        return true;
    }
    see(pos, mv) >= 0
}

/// The least valuable piece among `candidates`, with its square.
fn least_valuable(pos: &Position, candidates: Bitboard) -> Option<(Square, PieceType)> {
    PieceType::ALL.into_iter().find_map(|piece_type| {
        let these = candidates & pos.pieces(piece_type);
        (!these.is_empty()).then(|| (these.lsb(), piece_type))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uci::parse_move;

    fn see_of(fen: &str, mv: &str) -> i32 {
        let pos = Position::from_fen(fen).unwrap();
        let mv = parse_move(&pos, mv, false).unwrap_or_else(|| panic!("{mv} in {fen}"));
        see(&pos, mv)
    }

    #[test]
    fn undefended_pieces_are_won_outright() {
        assert_eq!(see_of("4k3/8/8/3r4/8/8/8/3RK3 w - - 0 1", "d1d5"), 500);
        assert_eq!(see_of("4k3/8/8/3p4/4P3/8/8/4K3 w - - 0 1", "e4d5"), 100);
    }

    #[test]
    fn defended_pieces_cost_the_capturer() {
        // Queen takes a pawn defended by a pawn: loses the queen for a pawn.
        assert_eq!(
            see_of("4k3/2p5/3p4/8/8/8/8/3QK3 w - - 0 1", "d1d6"),
            100 - 900
        );
        // Pawn takes a knight defended by a pawn: wins knight for pawn.
        assert_eq!(
            see_of("4k3/2p5/3n4/4P3/8/8/8/4K3 w - - 0 1", "e5d6"),
            300 - 100
        );
    }

    #[test]
    fn x_rays_join_the_exchange() {
        // Rd1xd5, Rd8xd5, Rd2xd5: the rook behind the first wins the exchange.
        // White wins a rook, loses a rook, wins a rook: +500.
        assert_eq!(see_of("3rk3/8/8/3r4/8/8/3R4/3RK3 w - - 0 1", "d2d5"), 500);
        // Without the second white rook, taking costs the rook back: 0.
        assert_eq!(see_of("3rk3/8/8/3r4/8/8/8/3RK3 w - - 0 1", "d1d5"), 0);
    }

    #[test]
    fn the_king_only_takes_undefended_pieces() {
        // Kxe2: nothing defends the rook.
        assert_eq!(see_of("4k3/8/8/8/8/8/4r3/4K3 w - - 0 1", "e1e2"), 500);
        // Qxe2 Rxe2 Kxe2: nothing else defends e2, so the king takes back.
        assert_eq!(
            see_of("4r1k1/8/8/8/8/8/4p3/3QK3 w - - 0 1", "d1e2"),
            100 - 900 + 500
        );
        // With a bishop defending e2 as well, Bxe2 ends it: the rook behind
        // still guards e2, so the king cannot take back.
        assert_eq!(
            see_of("4r1k1/8/8/7b/8/8/4p3/3QK3 w - - 0 1", "d1e2"),
            100 - 900
        );
    }

    #[test]
    fn quiet_moves_and_special_moves() {
        // A quiet move to a safe square exchanges nothing.
        assert_eq!(see_of("4k3/8/8/2p5/8/8/8/1N2K3 w - - 0 1", "b1a3"), 0);
        // One to a square a pawn attacks loses the piece.
        assert_eq!(see_of("4k3/8/8/2p5/8/3N4/8/4K3 w - - 0 1", "d3b4"), -300);
        // En passant wins the pawn.
        assert_eq!(see_of("4k3/8/8/3Pp3/8/8/8/4K3 w - e6 0 1", "d5e6"), 100);
        // An undefended promotion gains a queen for the pawn.
        assert_eq!(see_of("4k3/P7/8/8/8/8/8/4K3 w - - 0 1", "a7a8q"), 800);
        // Castling never exchanges anything.
        assert_eq!(see_of("4k3/8/8/8/8/8/8/R3K2R w KQ - 0 1", "e1g1"), 0);
    }

    #[test]
    fn the_quick_check_agrees_with_the_exchange() {
        let cases = [
            ("4k3/8/8/3r4/8/8/8/3RK3 w - - 0 1", "d1d5", true),
            ("4k3/2p5/3p4/8/8/8/8/3QK3 w - - 0 1", "d1d6", false),
            ("4k3/2p5/3n4/4P3/8/8/8/4K3 w - - 0 1", "e5d6", true),
            ("4k3/8/8/2p5/8/3N4/8/4K3 w - - 0 1", "d3b4", false),
        ];
        for (fen, text, expected) in cases {
            let pos = Position::from_fen(fen).unwrap();
            let mv = parse_move(&pos, text, false).unwrap();
            assert_eq!(loses_no_material(&pos, mv), expected, "{text} in {fen}");
        }
    }
}
