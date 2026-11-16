//! The chess position, its FEN text format, playing moves on it and its Zobrist key.

mod fen;
mod make_move;
mod position;
mod zobrist;

pub use fen::FenError;
pub use position::{CastlingSide, Position};
