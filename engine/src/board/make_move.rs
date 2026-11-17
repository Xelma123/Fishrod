//! Playing a move: copy-make.

use super::position::{CastlingSide, Position, castling_index};
use super::zobrist::{black_to_move_key, castling_key, en_passant_key};
use crate::types::{Color, Move, MoveKind, Piece, PieceType, Square};

impl Position {
    /// The position after `mv`, which must be legal here. `self` is left
    /// unchanged: the move is played on a copy (copy-make), so taking a move
    /// back is just going back to the old position.
    pub fn make_move(&self, mv: Move) -> Position {
        let mut pos = *self;
        let us = self.side_to_move;
        let (from, to) = (mv.from(), mv.to());

        // The key follows the position: placing and removing pieces update it
        // by themselves, the side to move, castling and en passant parts here.
        pos.key ^= black_to_move_key();
        if let Some(sq) = self.en_passant {
            pos.key ^= en_passant_key(sq);
        }
        pos.en_passant = None;
        pos.halfmove_clock = pos.halfmove_clock.saturating_add(1);
        if us == Color::Black {
            pos.fullmove_number = pos.fullmove_number.saturating_add(1);
        }

        // The square a pawn skipped with a double push, for the en passant check below.
        let mut skipped = None;
        match mv.kind() {
            MoveKind::Castle => {
                // `to` holds our own rook. Both pieces are lifted first, since in
                // Chess960 either may land on the other's square.
                let side = if to.file() > from.file() {
                    CastlingSide::King
                } else {
                    CastlingSide::Queen
                };
                let king = pos.remove_piece(from);
                let rook = pos.remove_piece(to);
                pos.put_piece(king, side.king_destination(us));
                pos.put_piece(rook, side.rook_destination(us));
            }
            kind => {
                // An en passant capture takes the pawn beside us: on our rank, on the target file.
                let captured_on = match kind {
                    MoveKind::EnPassant => Square::from_coords(to.file(), from.rank()),
                    _ => to,
                };
                if pos.piece_on(captured_on).is_some() {
                    pos.remove_piece(captured_on);
                    pos.halfmove_clock = 0;
                }
                let piece = pos.remove_piece(from);
                let placed = match kind {
                    MoveKind::Promotion(piece_type) => Piece::new(us, piece_type),
                    _ => piece,
                };
                pos.put_piece(placed, to);
                if piece.piece_type() == PieceType::Pawn {
                    pos.halfmove_clock = 0;
                }
                if kind == MoveKind::DoublePush {
                    skipped = Some(Square::from_coords(
                        from.file(),
                        (from.rank() + to.rank()) / 2,
                    ));
                }
            }
        }

        // Castling rights: a king move gives up both of its own; a rook that
        // leaves its square, or is captured on it, takes its right with it.
        if self.piece_on(from) == Some(Piece::new(us, PieceType::King)) {
            for side in CastlingSide::ALL {
                pos.castling_rooks[castling_index(us, side)] = None;
            }
        }
        for rook in &mut pos.castling_rooks {
            if *rook == Some(from) || *rook == Some(to) {
                *rook = None;
            }
        }
        if pos.castling_rooks != self.castling_rooks {
            pos.key ^= castling_key(&self.castling_rooks) ^ castling_key(&pos.castling_rooks);
        }

        pos.side_to_move = !us;
        // Keep the en passant square only if the opponent can actually capture there.
        if let Some(sq) = skipped
            && pos.can_capture_en_passant(sq)
        {
            pos.en_passant = Some(sq);
            pos.key ^= en_passant_key(sq);
        }
        pos
    }

    /// The position after passing: the same pieces, the other side to move.
    /// Not a move of chess; the search uses it to ask whether a position is
    /// so good that even passing keeps it good. The side to move must not be
    /// in check, or its king could be taken.
    ///
    /// The halfmove clock starts again from zero, so that no position after
    /// the pass counts as a repetition of one before it: passing is not
    /// part of the game.
    pub fn make_null_move(&self) -> Position {
        debug_assert!(self.checkers().is_empty(), "passing while in check");
        let mut pos = *self;
        pos.key ^= black_to_move_key();
        if let Some(sq) = self.en_passant {
            pos.key ^= en_passant_key(sq);
        }
        pos.en_passant = None;
        pos.halfmove_clock = 0;
        if self.side_to_move == Color::Black {
            pos.fullmove_number = pos.fullmove_number.saturating_add(1);
        }
        pos.side_to_move = !self.side_to_move;
        pos
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plays `mv` on the position given by `fen` and returns the resulting FEN.
    fn play(fen: &str, from: Square, to: Square, kind: MoveKind) -> String {
        let pos = Position::from_fen(fen).unwrap();
        let after = pos.make_move(Move::new(from, to, kind));
        assert!(after.is_consistent());
        // The original position is untouched.
        assert_eq!(pos.to_fen(), Position::from_fen(fen).unwrap().to_fen());
        after.to_fen()
    }

    #[test]
    fn quiet_moves_and_counters() {
        // No black pawn can take on e3, so no en passant square is set.
        assert_eq!(
            play(
                Position::STARTPOS_FEN,
                Square::E2,
                Square::E4,
                MoveKind::DoublePush
            ),
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1"
        );
        // A knight move raises the halfmove clock; a black move the move number.
        assert_eq!(
            play(
                "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1",
                Square::G8,
                Square::F6,
                MoveKind::Normal
            ),
            "rnbqkb1r/pppppppp/5n2/8/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 1 2"
        );
    }

    #[test]
    fn double_push_sets_a_capturable_en_passant_square() {
        assert_eq!(
            play(
                "rnbqkbnr/ppp1pppp/8/8/3pP3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 3",
                Square::C2,
                Square::C4,
                MoveKind::DoublePush
            ),
            "rnbqkbnr/ppp1pppp/8/8/2PpP3/8/PP1P1PPP/RNBQKBNR b KQkq c3 0 3"
        );
    }

    #[test]
    fn captures_and_en_passant() {
        // exd5: an ordinary capture resets the halfmove clock.
        assert_eq!(
            play(
                "rnbqkbnr/ppp1pppp/8/3p4/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 3 2",
                Square::E4,
                Square::D5,
                MoveKind::Normal
            ),
            "rnbqkbnr/ppp1pppp/8/3P4/8/8/PPPP1PPP/RNBQKBNR b KQkq - 0 2"
        );
        // exf6 en passant: the captured pawn stood on f5, not on f6.
        assert_eq!(
            play(
                "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6 0 3",
                Square::E5,
                Square::F6,
                MoveKind::EnPassant
            ),
            "rnbqkbnr/ppp1p1pp/5P2/3p4/8/8/PPPP1PPP/RNBQKBNR b KQkq - 0 3"
        );
    }

    #[test]
    fn promotion_replaces_the_pawn() {
        assert_eq!(
            play(
                "1r2k3/P7/8/8/8/8/8/4K3 w - - 0 1",
                Square::A7,
                Square::B8,
                MoveKind::Promotion(PieceType::Knight)
            ),
            "1N2k3/8/8/8/8/8/8/4K3 b - - 0 1"
        );
    }

    #[test]
    fn castling_moves_king_and_rook() {
        let fen = "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1";
        assert_eq!(
            play(fen, Square::E1, Square::H1, MoveKind::Castle),
            "r3k2r/8/8/8/8/8/8/R4RK1 b kq - 1 1"
        );
        assert_eq!(
            play(fen, Square::E1, Square::A1, MoveKind::Castle),
            "r3k2r/8/8/8/8/8/8/2KR3R b kq - 1 1"
        );
        // Chess960: king f1 and rook g1 swap places on a king-side castle.
        assert_eq!(
            play(
                "4k3/8/8/8/8/8/8/5KR1 w G - 0 1",
                Square::F1,
                Square::G1,
                MoveKind::Castle
            ),
            "4k3/8/8/8/8/8/8/5RK1 b - - 1 1"
        );
    }

    #[test]
    fn a_null_move_passes_the_turn() {
        // Black has just played e7-e5, and the d5 pawn could take en passant.
        // Passing gives that up.
        let pos = Position::from_fen("4k3/8/8/3Pp3/8/8/8/4K3 w - e6 0 1").unwrap();
        let passed = pos.make_null_move();
        assert!(passed.is_consistent());
        assert_eq!(passed.to_fen(), "4k3/8/8/3Pp3/8/8/8/4K3 b - - 0 1");
        assert_eq!(passed.key(), passed.compute_key());
        assert_ne!(passed.key(), pos.key());

        // Passing back gives the same pieces with the same side to move.
        let twice = passed.make_null_move();
        assert_eq!(twice.to_fen(), "4k3/8/8/3Pp3/8/8/8/4K3 w - - 0 2");
        assert_eq!(twice.key(), twice.compute_key());

        // The halfmove clock starts again.
        let late = Position::from_fen("4k3/8/8/8/8/8/8/4K2R w K - 37 60").unwrap();
        assert_eq!(late.make_null_move().halfmove_clock(), 0);
    }

    #[test]
    fn castling_rights_are_lost_by_moving_or_losing_rooks() {
        let fen = "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1";
        // The king moves: both white rights go.
        assert_eq!(
            play(fen, Square::E1, Square::E2, MoveKind::Normal),
            "r3k2r/8/8/8/8/8/4K3/R6R b kq - 1 1"
        );
        // A rook moves: only its own right goes.
        assert_eq!(
            play(fen, Square::H1, Square::H5, MoveKind::Normal),
            "r3k2r/8/8/7R/8/8/8/R3K3 b Qkq - 1 1"
        );
        // A rook is captured on its square: the opponent loses that right.
        assert_eq!(
            play(fen, Square::A1, Square::A8, MoveKind::Normal),
            "R3k2r/8/8/8/8/8/8/4K2R b Kk - 0 1"
        );
    }
}
