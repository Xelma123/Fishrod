//! Move generation and the attack tables it is built on.

pub mod attacks;
mod legal;
mod magic;
pub mod perft;

pub use legal::legal_moves;
