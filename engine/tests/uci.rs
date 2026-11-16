//! Runs the engine as a separate process and talks to it over UCI, the way a
//! chess GUI does.

use std::io::Write;
use std::process::{Command, Stdio};

/// Starts the engine, sends `input` on its standard input, closes it, and
/// returns everything the engine wrote before exiting.
fn run(input: &str) -> String {
    let mut engine = Command::new(env!("CARGO_BIN_EXE_fishrod"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("the engine starts");
    engine
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(input.as_bytes())
        .expect("the engine reads its input");
    let output = engine.wait_with_output().expect("the engine exits");
    assert!(output.status.success(), "exit status {}", output.status);
    String::from_utf8(output.stdout).expect("the output is UTF-8")
}

#[test]
fn session_over_standard_input() {
    let out = run("uci\nisready\nposition startpos moves e2e4 c7c5\nd\ngo depth 3\nquit\n");
    assert!(out.starts_with("id name Fishrod "), "{out}");
    assert!(out.contains("\nuciok\nreadyok\n"), "{out}");
    assert!(
        out.contains("FEN: rnbqkbnr/pp1ppppp/8/2p5/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2"),
        "{out}"
    );
    assert!(out.contains("\ninfo depth 3 "), "{out}");
    assert!(
        out.lines().last().unwrap().starts_with("bestmove "),
        "{out}"
    );
}

#[test]
fn windows_line_endings_are_accepted() {
    assert_eq!(run("isready\r\nquit\r\n"), "readyok\n");
}

#[test]
fn end_of_input_stops_the_engine() {
    assert_eq!(run("isready\n"), "readyok\n");
}
