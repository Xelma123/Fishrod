//! Move generation and the attack tables it is built on.

pub mod attacks;
mod legal;
mod magic;
pub mod perft;

pub use legal::{has_legal_move, legal_captures, legal_moves};
