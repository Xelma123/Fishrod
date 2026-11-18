//! Fishrod, a UCI chess engine.
//!
//! The library holds all engine logic; the `fishrod` binary is a thin
//! command-line front end over it.

#![deny(unsafe_code)]

pub mod bench;
pub mod board;
pub mod eval;
pub mod movegen;
pub mod openings;
pub mod play;
pub mod search;
pub mod types;
pub mod uci;

mod rng;

/// Engine name reported to user interfaces.
pub const NAME: &str = "Fishrod";

/// Author line reported to user interfaces.
pub const AUTHOR: &str = "the Fishrod developers";

/// Engine version, taken from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
