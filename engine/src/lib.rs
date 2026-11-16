//! Fishrod, a UCI chess engine.
//!
//! The library holds all engine logic; the `fishrod` binary is a thin
//! command-line front end over it.

#![deny(unsafe_code)]

pub mod board;
pub mod types;

/// Engine name reported to user interfaces.
pub const NAME: &str = "Fishrod";

/// Engine version, taken from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
