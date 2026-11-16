//! Hand-written evaluation: how good a position looks without searching.
//!
//! Each piece is worth its material plus a bonus for its square, taken from
//! a middlegame and an endgame table. The two totals are blended by game
//! phase: with all pieces on the board the middlegame total counts, and as
//! knights, bishops, rooks and queens come off, the endgame total takes over.
//! Blending avoids a sudden jump in the score at some arbitrary point.

mod tables;

use crate::board::Position;
use crate::types::{Color, PieceType};

/// How much each piece type, pawn to king, counts towards the game phase.
const PHASE_WEIGHT: [i32; 6] = [0, 1, 1, 2, 4, 0];

/// The phase of the starting position: 4 minor pieces, 2 rooks and a queen per side.
const OPENING_PHASE: i32 = 24;

/// The score of `pos` in centipawns, from the point of view of the side to
/// move: positive means the side to move stands better.
pub fn evaluate(pos: &Position) -> i32 {
    let (mut mg, mut eg, mut phase) = (0, 0, 0);
    for color in Color::ALL {
        let sign = match color {
            Color::White => 1,
            Color::Black => -1,
        };
        for piece_type in PieceType::ALL {
            for sq in pos.pieces_of(color, piece_type) {
                let (piece_mg, piece_eg) = tables::value(color, piece_type, sq);
                mg += sign * piece_mg;
                eg += sign * piece_eg;
                phase += PHASE_WEIGHT[piece_type.index()];
            }
        }
    }
    // Promotions can push the phase past its starting value.
    let phase = phase.min(OPENING_PHASE);
    let white_score = (mg * phase + eg * (OPENING_PHASE - phase)) / OPENING_PHASE;
    match pos.side_to_move() {
        Color::White => white_score,
        Color::Black => -white_score,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(fen: &str) -> i32 {
        evaluate(&Position::from_fen(fen).unwrap())
    }

    /// The same position with the colors swapped: the board turned upside
    /// down, white and black exchanged, and the other side to move.
    fn flip_fen(fen: &str) -> String {
        let swap_case = |text: &str| -> String {
            text.chars()
                .map(|c| {
                    if c.is_ascii_uppercase() {
                        c.to_ascii_lowercase()
                    } else {
                        c.to_ascii_uppercase()
                    }
                })
                .collect()
        };
        let fields: Vec<&str> = fen.split_whitespace().collect();
        let placement: Vec<String> = fields[0].split('/').rev().map(swap_case).collect();
        let side = if fields[1] == "w" { "b" } else { "w" };
        let en_passant = match fields[3].as_bytes() {
            [file, rank] => format!("{}{}", *file as char, (b'1' + b'8' - rank) as char),
            _ => "-".to_string(),
        };
        format!(
            "{} {side} {} {en_passant} {}",
            placement.join("/"),
            swap_case(fields[2]),
            fields[4..].join(" ")
        )
    }

    const POSITIONS: [&str; 6] = [
        Position::STARTPOS_FEN,
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
        "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6 0 3",
        "bqnb1rkr/pp3ppp/3ppn2/2p5/5P2/P2P4/NPP1P1PP/BQ1BNRKR w HFhf - 2 9",
    ];

    #[test]
    fn flip_fen_swaps_the_colors() {
        assert_eq!(
            flip_fen("rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6 0 3"),
            "rnbqkbnr/pppp1ppp/8/8/3PpP2/8/PPP1P1PP/RNBQKBNR b kqKQ f3 0 3"
        );
    }

    #[test]
    fn swapping_the_colors_keeps_the_score() {
        for fen in POSITIONS {
            assert_eq!(eval(fen), eval(&flip_fen(fen)), "{fen}");
        }
    }

    #[test]
    fn starting_position_is_level() {
        assert_eq!(eval(Position::STARTPOS_FEN), 0);
    }

    #[test]
    fn score_is_from_the_side_to_move() {
        // White is a queen up: good for white to move, bad for black to move.
        let white_to_move = eval("rnb1kbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
        let black_to_move = eval("rnb1kbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR b KQkq - 0 1");
        assert!(white_to_move > 800, "{white_to_move}");
        assert_eq!(black_to_move, -white_to_move);
    }

    #[test]
    fn material_and_squares_count() {
        // An extra pawn is worth having.
        assert!(eval("4k3/pp6/8/8/8/8/PPP5/4K3 w - - 0 1") > 50);
        // A centralised knight beats one on the rim, all else equal.
        assert!(eval("4k3/8/8/8/3N4/8/8/4K3 w - - 0 1") > eval("4k3/8/8/8/8/8/8/N3K3 w - - 0 1"));
        // In a pawn endgame an advanced passed pawn is worth far more.
        assert!(
            eval("4k3/P7/8/8/8/8/8/4K3 w - - 0 1") > eval("4k3/8/8/8/8/8/P7/4K3 w - - 0 1") + 80
        );
    }

    #[test]
    fn the_endgame_wants_an_active_king() {
        // Only kings and pawns: the endgame tables decide, and they want the king central.
        let central = eval("8/8/8/3k4/8/8/8/K7 b - - 0 1");
        assert!(central > 0, "{central}");
    }
}
