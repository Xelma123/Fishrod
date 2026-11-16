//! Reading and writing positions in Forsyth-Edwards Notation (FEN).
//!
//! Castling rights are read in three styles: standard `KQkq`, Shredder-FEN
//! (rook files such as `HAha`) and X-FEN (`KQkq`, with a rook file only when
//! the castling rook is not the outermost rook on its side). They are written
//! in X-FEN, which is identical to the standard style in normal chess.

use std::fmt;

use super::position::{CastlingSide, Position, castling_index};
use crate::types::{Bitboard, Color, Piece, PieceType, Square};

/// The reason a FEN string could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FenError {
    /// The string does not have 4 to 6 space-separated fields.
    FieldCount(usize),
    /// The placement field does not have 8 ranks.
    RankCount(usize),
    /// A rank (numbered 1 to 8) does not describe exactly 8 squares.
    RankLength(u8),
    /// An unknown character in the placement field.
    InvalidPiece(char),
    /// A side does not have exactly one king.
    KingCount(Color),
    /// A pawn stands on the first or the last rank.
    PawnOnBackRank(Square),
    /// The side to move is not `w` or `b`.
    InvalidSide(String),
    /// The castling field is malformed or names a missing king or rook.
    InvalidCastling(String),
    /// The en passant field is not `-` or a square on the right rank.
    InvalidEnPassant(String),
    /// A move counter is not a number.
    InvalidCounter(String),
}

impl fmt::Display for FenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FenError::FieldCount(n) => write!(f, "expected 4 to 6 fields, found {n}"),
            FenError::RankCount(n) => write!(f, "expected 8 ranks, found {n}"),
            FenError::RankLength(rank) => write!(f, "rank {rank} does not have 8 squares"),
            FenError::InvalidPiece(c) => write!(f, "invalid piece character '{c}'"),
            FenError::KingCount(color) => write!(f, "{color:?} does not have exactly one king"),
            FenError::PawnOnBackRank(sq) => write!(f, "pawn on back rank square {sq}"),
            FenError::InvalidSide(s) => write!(f, "invalid side to move '{s}'"),
            FenError::InvalidCastling(s) => write!(f, "invalid castling rights '{s}'"),
            FenError::InvalidEnPassant(s) => write!(f, "invalid en passant square '{s}'"),
            FenError::InvalidCounter(s) => write!(f, "invalid move counter '{s}'"),
        }
    }
}

impl std::error::Error for FenError {}

impl Position {
    /// Reads a position from FEN. The two move counters may be left out, as in
    /// EPD files; they then default to 0 and 1.
    ///
    /// An en passant square is kept only if the capture is actually available.
    pub fn from_fen(fen: &str) -> Result<Position, FenError> {
        let fields: Vec<&str> = fen.split_whitespace().collect();
        if !(4..=6).contains(&fields.len()) {
            return Err(FenError::FieldCount(fields.len()));
        }

        let mut pos = Position::empty();
        parse_placement(&mut pos, fields[0])?;
        pos.side_to_move = parse_side(fields[1])?;
        parse_castling(&mut pos, fields[2])?;
        pos.en_passant = parse_en_passant(&pos, fields[3])?;
        pos.halfmove_clock = parse_counter(fields.get(4).copied(), 0)?;
        pos.fullmove_number = parse_counter(fields.get(5).copied(), 1)?;
        pos.key = pos.compute_key();
        Ok(pos)
    }

    /// Writes the position as FEN.
    pub fn to_fen(&self) -> String {
        let mut fen = String::with_capacity(90);

        // Placement, from rank 8 down to rank 1; runs of empty squares become digits.
        for rank in (0..8).rev() {
            let mut empty = 0u8;
            for file in 0..8 {
                match self.piece_on(Square::from_coords(file, rank)) {
                    Some(piece) => {
                        if empty > 0 {
                            fen.push(char::from(b'0' + empty));
                            empty = 0;
                        }
                        fen.push(piece.to_char());
                    }
                    None => empty += 1,
                }
            }
            if empty > 0 {
                fen.push(char::from(b'0' + empty));
            }
            if rank > 0 {
                fen.push('/');
            }
        }

        fen.push(' ');
        fen.push(match self.side_to_move {
            Color::White => 'w',
            Color::Black => 'b',
        });
        fen.push(' ');
        fen.push_str(&castling_field(self));
        fen.push(' ');
        match self.en_passant {
            Some(sq) => fen.push_str(&sq.to_string()),
            None => fen.push('-'),
        }
        fen.push_str(&format!(
            " {} {}",
            self.halfmove_clock, self.fullmove_number
        ));
        fen
    }
}

/// The rook furthest from the king on the given side, if there is one.
fn outermost_rook(rooks: Bitboard, king: Square, side: CastlingSide) -> Option<Square> {
    let mut on_side = rooks.into_iter().filter(|rook| match side {
        CastlingSide::King => rook.file() > king.file(),
        CastlingSide::Queen => rook.file() < king.file(),
    });
    // Squares on one rank are ordered by file.
    match side {
        CastlingSide::King => on_side.max(),
        CastlingSide::Queen => on_side.next(),
    }
}

fn parse_placement(pos: &mut Position, placement: &str) -> Result<(), FenError> {
    let ranks: Vec<&str> = placement.split('/').collect();
    if ranks.len() != 8 {
        return Err(FenError::RankCount(ranks.len()));
    }

    // FEN lists rank 8 first.
    for (text, rank) in ranks.iter().zip((0..8u8).rev()) {
        let mut file = 0u8;
        for c in text.chars() {
            if let Some(skip) = c.to_digit(10).filter(|d| (1..=8).contains(d)) {
                file += skip as u8;
            } else {
                let piece = Piece::from_char(c).ok_or(FenError::InvalidPiece(c))?;
                if file >= 8 {
                    return Err(FenError::RankLength(rank + 1));
                }
                pos.put_piece(piece, Square::from_coords(file, rank));
                file += 1;
            }
            if file > 8 {
                return Err(FenError::RankLength(rank + 1));
            }
        }
        if file != 8 {
            return Err(FenError::RankLength(rank + 1));
        }
    }

    for color in Color::ALL {
        if pos.pieces_of(color, PieceType::King).popcount() != 1 {
            return Err(FenError::KingCount(color));
        }
    }
    let misplaced_pawns = pos.pieces(PieceType::Pawn) & (Bitboard::RANK_1 | Bitboard::RANK_8);
    if !misplaced_pawns.is_empty() {
        return Err(FenError::PawnOnBackRank(misplaced_pawns.lsb()));
    }
    Ok(())
}

fn parse_side(text: &str) -> Result<Color, FenError> {
    match text {
        "w" => Ok(Color::White),
        "b" => Ok(Color::Black),
        _ => Err(FenError::InvalidSide(text.to_string())),
    }
}

fn parse_castling(pos: &mut Position, text: &str) -> Result<(), FenError> {
    if text == "-" {
        return Ok(());
    }
    let invalid = || FenError::InvalidCastling(text.to_string());

    for c in text.chars() {
        let color = if c.is_ascii_uppercase() {
            Color::White
        } else {
            Color::Black
        };
        let back_rank = color.back_rank();
        let king = pos.king_square(color);
        if king.rank() != back_rank {
            return Err(invalid());
        }
        let rooks = pos.pieces_of(color, PieceType::Rook) & Bitboard::rank(back_rank);

        let (side, rook) = match c.to_ascii_lowercase() {
            'k' => (
                CastlingSide::King,
                outermost_rook(rooks, king, CastlingSide::King),
            ),
            'q' => (
                CastlingSide::Queen,
                outermost_rook(rooks, king, CastlingSide::Queen),
            ),
            letter @ 'a'..='h' => {
                let sq = Square::from_coords(letter as u8 - b'a', back_rank);
                let side = if sq.file() > king.file() {
                    CastlingSide::King
                } else {
                    CastlingSide::Queen
                };
                (side, rooks.contains(sq).then_some(sq))
            }
            _ => return Err(invalid()),
        };
        let rook = rook.ok_or_else(invalid)?;
        pos.castling_rooks[castling_index(color, side)] = Some(rook);
    }
    Ok(())
}

fn parse_en_passant(pos: &Position, text: &str) -> Result<Option<Square>, FenError> {
    if text == "-" {
        return Ok(None);
    }
    let invalid = || FenError::InvalidEnPassant(text.to_string());
    let sq: Square = text.parse().map_err(|_| invalid())?;

    // The skipped square is on rank 6 when white is to move and on rank 3 when black is.
    let expected_rank = match pos.side_to_move {
        Color::White => 5,
        Color::Black => 2,
    };
    if sq.rank() != expected_rank {
        return Err(invalid());
    }
    Ok(pos.can_capture_en_passant(sq).then_some(sq))
}

fn parse_counter(text: Option<&str>, default: u16) -> Result<u16, FenError> {
    match text {
        None => Ok(default),
        Some(text) => text
            .parse()
            .map_err(|_| FenError::InvalidCounter(text.to_string())),
    }
}

/// The castling field in X-FEN: `K`/`Q` for the outermost rook on a side,
/// the rook's file letter otherwise; `-` when no rights remain.
fn castling_field(pos: &Position) -> String {
    let mut field = String::new();
    for color in Color::ALL {
        let king = pos.king_square(color);
        let rooks = pos.pieces_of(color, PieceType::Rook) & Bitboard::rank(color.back_rank());
        for side in CastlingSide::ALL {
            let Some(rook) = pos.castling_rook(color, side) else {
                continue;
            };
            let letter = if outermost_rook(rooks, king, side) == Some(rook) {
                match side {
                    CastlingSide::King => 'k',
                    CastlingSide::Queen => 'q',
                }
            } else {
                char::from(b'a' + rook.file())
            };
            field.push(match color {
                Color::White => letter.to_ascii_uppercase(),
                Color::Black => letter,
            });
        }
    }
    if field.is_empty() {
        field.push('-');
    }
    field
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FENs that must be written back exactly as they were read.
    const ROUND_TRIP: [&str; 9] = [
        Position::STARTPOS_FEN,
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
        "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
        "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
        // En passant captures that are available, for each side.
        "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6 0 3",
        "rnbqkbnr/pp1ppppp/8/8/2pPP3/8/PPP2PPP/RNBQKBNR b KQkq d3 0 3",
        // X-FEN: the castling rook d1 is not the outermost rook on its side.
        "4k3/8/8/8/8/8/8/1K1R3R w D - 0 1",
    ];

    fn error(fen: &str) -> FenError {
        Position::from_fen(fen).unwrap_err()
    }

    #[test]
    fn fens_round_trip() {
        for fen in ROUND_TRIP {
            let pos = Position::from_fen(fen).unwrap();
            assert!(pos.is_consistent(), "{fen}");
            assert_eq!(pos.to_fen(), fen);
            assert_eq!(Position::from_fen(&pos.to_fen()), Ok(pos));
        }
    }

    #[test]
    fn en_passant_is_kept_only_when_capturable() {
        let pos = Position::from_fen(ROUND_TRIP[6]).unwrap();
        assert_eq!(pos.en_passant(), Some(Square::F6));

        // After 1. e4 no black pawn can take on e3, so the square is dropped.
        let pos = Position::from_fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1")
            .unwrap();
        assert_eq!(pos.en_passant(), None);
        assert_eq!(
            pos.to_fen(),
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1"
        );
    }

    #[test]
    fn move_counters_are_optional() {
        let pos = Position::from_fen("4k3/8/8/8/8/8/8/4K3 w - -").unwrap();
        assert_eq!(pos.to_fen(), "4k3/8/8/8/8/8/8/4K3 w - - 0 1");
    }

    #[test]
    fn chess960_castling_styles_agree() {
        let placement = "bqnb1rkr/pp3ppp/3ppn2/2p5/5P2/P2P4/NPP1P1PP/BQ1BNRKR w";
        let shredder = Position::from_fen(&format!("{placement} HFhf - 2 9")).unwrap();
        let xfen = Position::from_fen(&format!("{placement} KQkq - 2 9")).unwrap();
        assert_eq!(shredder, xfen);
        assert_eq!(
            shredder.castling_rook(Color::White, CastlingSide::King),
            Some(Square::H1)
        );
        assert_eq!(
            shredder.castling_rook(Color::White, CastlingSide::Queen),
            Some(Square::F1)
        );
        assert_eq!(
            shredder.castling_rook(Color::Black, CastlingSide::King),
            Some(Square::H8)
        );
        assert_eq!(
            shredder.castling_rook(Color::Black, CastlingSide::Queen),
            Some(Square::F8)
        );
        assert_eq!(shredder.to_fen(), format!("{placement} KQkq - 2 9"));
    }

    #[test]
    fn malformed_fens_are_rejected() {
        let start_board = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR";
        assert_eq!(error(""), FenError::FieldCount(0));
        assert_eq!(error(&format!("{start_board} w")), FenError::FieldCount(2));
        assert_eq!(
            error("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP w KQkq - 0 1"),
            FenError::RankCount(7)
        );
        assert_eq!(
            error("rnbqkbnrr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"),
            FenError::RankLength(8)
        );
        assert_eq!(
            error("rnbqkbnr/ppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"),
            FenError::RankLength(7)
        );
        assert_eq!(
            error("rnbqkbnr/pppppppp1/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"),
            FenError::RankLength(7)
        );
        assert_eq!(
            error("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNx w - - 0 1"),
            FenError::InvalidPiece('x')
        );
        assert_eq!(
            error("rnbq1bnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w - - 0 1"),
            FenError::KingCount(Color::Black)
        );
        assert_eq!(
            error("4k3/8/8/8/8/8/8/K3K3 w - - 0 1"),
            FenError::KingCount(Color::White)
        );
        assert_eq!(
            error("P3k3/8/8/8/8/8/8/4K3 w - - 0 1"),
            FenError::PawnOnBackRank(Square::A8)
        );
        assert_eq!(
            error("4k3/8/8/8/8/8/8/4K3 x - - 0 1"),
            FenError::InvalidSide("x".to_string())
        );
    }

    #[test]
    fn bad_castling_en_passant_and_counters_are_rejected() {
        // No rook to castle with.
        assert_eq!(
            error("4k3/8/8/8/8/8/8/4K3 w K - 0 1"),
            FenError::InvalidCastling("K".to_string())
        );
        // The king has left its back rank.
        assert_eq!(
            error("4k3/8/8/8/8/8/4K3/7R w K - 0 1"),
            FenError::InvalidCastling("K".to_string())
        );
        assert_eq!(
            error("4k3/8/8/8/8/8/8/4K2R w X - 0 1"),
            FenError::InvalidCastling("X".to_string())
        );
        // With white to move, the skipped square must be on rank 6.
        assert_eq!(
            error("4k3/8/8/8/8/8/8/4K3 w - e3 0 1"),
            FenError::InvalidEnPassant("e3".to_string())
        );
        assert_eq!(
            error("4k3/8/8/8/8/8/8/4K3 w - z9 0 1"),
            FenError::InvalidEnPassant("z9".to_string())
        );
        assert_eq!(
            error("4k3/8/8/8/8/8/8/4K3 w - - x 1"),
            FenError::InvalidCounter("x".to_string())
        );
    }
}
