//! The chess position and its FEN text format.

mod fen;
mod position;

pub use fen::FenError;
pub use position::{CastlingSide, Position};
