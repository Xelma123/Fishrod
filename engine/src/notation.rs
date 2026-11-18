//! Standard algebraic notation (SAN), the notation of score sheets and PGN
//! files: `e4`, `Nf3`, `exd5`, `Nbd2`, `O-O`, `e8=Q+`, `Qh7#`.
//!
//! A move names the moving piece (nothing for a pawn), the target square and
//! an `x` for a capture; a pawn capture names the file it comes from. When
//! two pieces of the same kind could reach the target, the origin's file, or
//! else its rank, or else both, tells them apart. A check is marked `+` and a
//! mate `#`.

use crate::board::Position;
use crate::movegen::legal_moves;
use crate::types::{Move, MoveKind, PieceType};

/// The move `mv`, legal in `pos`, in standard algebraic notation.
pub fn san(pos: &Position, mv: Move) -> String {
    let mut text = match mv.kind() {
        // The move goes from the king to its rook, so the direction tells the side.
        MoveKind::Castle if mv.to().file() > mv.from().file() => "O-O".to_string(),
        MoveKind::Castle => "O-O-O".to_string(),
        kind => {
            let (from, to) = (mv.from(), mv.to());
            let piece = pos
                .piece_on(from)
                .expect("a legal move starts on a piece")
                .piece_type();
            let capture = kind == MoveKind::EnPassant || pos.piece_on(to).is_some();
            let mut text = String::new();
            if piece == PieceType::Pawn {
                if capture {
                    text.push(file_letter(from.file()));
                }
            } else {
                text.push(piece.to_char().to_ascii_uppercase());
                text.push_str(&disambiguation(pos, mv, piece));
            }
            if capture {
                text.push('x');
            }
            text.push_str(&to.to_string());
            if let MoveKind::Promotion(promoted) = kind {
                text.push('=');
                text.push(promoted.to_char().to_ascii_uppercase());
            }
            text
        }
    };
    let after = pos.make_move(mv);
    if !after.checkers().is_empty() {
        text.push(if legal_moves(&after).is_empty() {
            '#'
        } else {
            '+'
        });
    }
    text
}

/// What tells `mv` apart from the moves of other pieces of the same kind to
/// the same square: nothing, the origin's file, its rank, or both.
fn disambiguation(pos: &Position, mv: Move, piece: PieceType) -> String {
    let from = mv.from();
    let rivals: Vec<Move> = legal_moves(pos)
        .iter()
        .copied()
        .filter(|&other| {
            other.to() == mv.to()
                && other.from() != from
                && other.kind() != MoveKind::Castle
                && pos.piece_on(other.from()).map(|p| p.piece_type()) == Some(piece)
        })
        .collect();
    if rivals.is_empty() {
        return String::new();
    }
    let shares_file = rivals
        .iter()
        .any(|other| other.from().file() == from.file());
    let shares_rank = rivals
        .iter()
        .any(|other| other.from().rank() == from.rank());
    let file = file_letter(from.file()).to_string();
    let rank = (from.rank() + 1).to_string();
    match (shares_file, shares_rank) {
        (false, _) => file,
        (true, false) => rank,
        (true, true) => file + &rank,
    }
}

/// The letter of a file, `a` to `h`.
fn file_letter(file: u8) -> char {
    char::from(b'a' + file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uci::parse_move;

    /// The SAN of the UCI move `text` in the position given by `fen`.
    fn san_of(fen: &str, text: &str) -> String {
        let pos = Position::from_fen(fen).unwrap();
        let mv = parse_move(&pos, text, false).unwrap_or_else(|| panic!("{text} in {fen}"));
        san(&pos, mv)
    }

    #[test]
    fn plain_moves_and_captures() {
        let start = Position::STARTPOS_FEN;
        assert_eq!(san_of(start, "e2e4"), "e4");
        assert_eq!(san_of(start, "g1f3"), "Nf3");
        let open = "rnbqkbnr/ppp1pppp/8/3p4/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2";
        assert_eq!(san_of(open, "e4d5"), "exd5");
        assert_eq!(san_of(open, "f1b5"), "Bb5+");
        // En passant: the pawn takes on an empty square.
        assert_eq!(san_of("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1", "e5d6"), "exd6");
    }

    #[test]
    fn rival_pieces_are_told_apart() {
        // Knights on b1 and f3 can both reach d2: the file tells them apart.
        let knights = "4k3/8/8/8/8/5N2/8/1N2K3 w - - 0 1";
        assert_eq!(san_of(knights, "b1d2"), "Nbd2");
        // Rooks on a1 and a5 share the file: the rank tells them apart.
        let rooks = "4k3/8/8/R7/8/8/8/R3K3 w - - 0 1";
        assert_eq!(san_of(rooks, "a1a3"), "R1a3");
        // Queens on a1, a3 and c1 can all go to b2: a1 shares its file with
        // one and its rank with the other, so it needs both.
        let queens = "4k3/8/8/8/8/Q7/8/Q1Q1K3 w - - 0 1";
        assert_eq!(san_of(queens, "a1b2"), "Qa1b2");
        assert_eq!(san_of(queens, "a3b2"), "Q3b2");
        assert_eq!(san_of(queens, "c1b2"), "Qcb2");
        // A pinned rival does not count: only one knight can really go.
        let pinned = "4k3/4r3/8/8/8/2N5/4N3/4K3 w - - 0 1";
        assert_eq!(san_of(pinned, "c3d5"), "Nd5");
    }

    #[test]
    fn castling_promotion_check_and_mate() {
        let castles = "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1";
        assert_eq!(san_of(castles, "e1g1"), "O-O");
        assert_eq!(san_of(castles, "e1c1"), "O-O-O");
        assert_eq!(san_of("4k3/P7/8/8/8/8/8/4K3 w - - 0 1", "a7a8q"), "a8=Q+");
        assert_eq!(
            san_of("1r2k3/P7/8/8/8/8/8/4K3 w - - 0 1", "a7b8n"),
            "axb8=N"
        );
        assert_eq!(san_of("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1", "a1a8"), "Ra8#");
    }
}
