//! Command-line entry point of the Fishrod engine.

#![deny(unsafe_code)]

use std::process::ExitCode;
use std::time::Instant;

use fishrod::board::{CastlingSide, Position};
use fishrod::movegen::{attacks, perft};
use fishrod::types::{Color, Square};
use fishrod::{bench, openings};

const USAGE: &str = "\
usage: fishrod                      UCI mode, for chess GUIs
       fishrod perft <depth> [<fen>]
       fishrod bench [<depth>]
       fishrod openings <count> [<seed>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        None => fishrod::uci::run().map_err(|err| format!("I/O error: {err}")),
        Some("perft") => run_perft(&args[1..]),
        Some("bench") => run_bench(&args[1..]),
        Some("openings") => run_openings(&args[1..]),
        Some(other) => Err(format!("unknown command '{other}'")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}

/// `bench [<depth>]`: searches the bench positions and prints the total
/// node count, which every commit records, on the last line.
fn run_bench(args: &[String]) -> Result<(), String> {
    let depth: u32 = match args.first() {
        Some(text) => text.parse().map_err(|_| "depth must be a whole number")?,
        None => bench::DEFAULT_DEPTH,
    };
    attacks::init();
    let summary = bench::run(depth, |index, pos, outcome| {
        let best = outcome
            .best_move
            .map_or_else(|| "0000".to_string(), |mv| mv.to_uci(true));
        println!(
            "{:>2}  {:>10} nodes  bestmove {best:<5}  {}",
            index + 1,
            outcome.nodes,
            pos.to_fen()
        );
    });
    let seconds = summary.time.as_secs_f64();
    println!();
    println!("Time: {seconds:.3} s");
    println!(
        "{} nodes {:.0} nps",
        summary.nodes,
        summary.nodes as f64 / seconds.max(1e-9)
    );
    Ok(())
}

/// `openings <count> [<seed>]`: prints balanced opening positions, one EPD
/// line each, for engine-versus-engine tests.
fn run_openings(args: &[String]) -> Result<(), String> {
    let count: usize = args
        .first()
        .ok_or("missing count")?
        .parse()
        .map_err(|_| "count must be a whole number")?;
    let seed: u64 = match args.get(1) {
        Some(text) => text.parse().map_err(|_| "seed must be a whole number")?,
        None => openings::DEFAULT_SEED,
    };
    attacks::init();
    for pos in openings::openings(seed, openings::DEFAULT_DEPTH).take(count) {
        println!("{}", openings::to_epd(&pos));
    }
    Ok(())
}

/// `perft <depth> [<fen>]`: prints the node count below each legal move,
/// then the total, the time taken and the speed. Without a FEN it uses the
/// starting position.
fn run_perft(args: &[String]) -> Result<(), String> {
    let depth: u32 = args
        .first()
        .ok_or("missing depth")?
        .parse()
        .map_err(|_| "depth must be a whole number")?;
    let pos = match args.get(1..) {
        Some(fen) if !fen.is_empty() => {
            Position::from_fen(&fen.join(" ")).map_err(|err| format!("invalid FEN: {err}"))?
        }
        _ => Position::startpos(),
    };

    attacks::init();
    let start = Instant::now();
    let divided = perft::divide(&pos, depth);
    let seconds = start.elapsed().as_secs_f64();

    let chess960 = needs_chess960_notation(&pos);
    let mut lines: Vec<(String, u64)> = divided
        .iter()
        .map(|&(mv, nodes)| (mv.to_uci(chess960), nodes))
        .collect();
    lines.sort();
    for (mv, nodes) in &lines {
        println!("{mv}: {nodes}");
    }

    let total: u64 = if depth == 0 {
        1
    } else {
        lines.iter().map(|&(_, nodes)| nodes).sum()
    };
    println!();
    println!("Nodes: {total}");
    println!("Time: {seconds:.3} s");
    println!("NPS: {:.0}", total as f64 / seconds.max(1e-9));
    Ok(())
}

/// Whether castling moves must be written in Chess960 notation: some king or
/// castling rook stands off its standard square.
fn needs_chess960_notation(pos: &Position) -> bool {
    Color::ALL.into_iter().any(|color| {
        let rank = color.back_rank();
        CastlingSide::ALL.into_iter().any(|side| {
            let standard_rook_file = match side {
                CastlingSide::King => 7,
                CastlingSide::Queen => 0,
            };
            pos.castling_rook(color, side).is_some_and(|rook| {
                rook != Square::from_coords(standard_rook_file, rank)
                    || pos.king_square(color) != Square::from_coords(4, rank)
            })
        })
    })
}
