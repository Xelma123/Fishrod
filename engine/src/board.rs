//! The chess position, its FEN text format and playing moves on it.

mod fen;
mod make_move;
mod position;

pub use fen::FenError;
pub use position::{CastlingSide, Position};
