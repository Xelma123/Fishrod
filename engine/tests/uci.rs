//! Runs the engine as a separate process and talks to it over UCI, the way a
//! chess GUI does.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

/// A running engine with pipes to its standard input and output.
struct Engine {
    process: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Engine {
    fn start() -> Engine {
        let mut process = Command::new(env!("CARGO_BIN_EXE_fishrod"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("the engine starts");
        let input = process.stdin.take().expect("stdin is piped");
        let output = BufReader::new(process.stdout.take().expect("stdout is piped"));
        Engine {
            process,
            input,
            output,
        }
    }

    fn send(&mut self, command: &str) {
        writeln!(self.input, "{command}").expect("the engine reads its input");
        self.input.flush().expect("the engine reads its input");
    }

    /// Reads output lines up to and including the first that starts with `prefix`.
    fn read_until(&mut self, prefix: &str) -> Vec<String> {
        let mut lines = Vec::new();
        loop {
            let mut line = String::new();
            let read = self
                .output
                .read_line(&mut line)
                .expect("the output is UTF-8");
            assert!(read > 0, "the engine ended before '{prefix}': {lines:?}");
            let line = line.trim_end().to_string();
            let done = line.starts_with(prefix);
            lines.push(line);
            if done {
                return lines;
            }
        }
    }

    /// Sends `quit` and checks that the engine exits cleanly.
    fn quit(mut self) {
        self.send("quit");
        let status = self.process.wait().expect("the engine exits");
        assert!(status.success(), "exit status {status}");
    }
}

/// Starts the engine, sends all of `input` at once, closes the input, and
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
fn a_game_as_a_gui_plays_it() {
    let mut engine = Engine::start();
    engine.send("uci");
    let handshake = engine.read_until("uciok");
    assert!(
        handshake[0].starts_with("id name Fishrod "),
        "{handshake:?}"
    );
    engine.send("isready");
    assert_eq!(engine.read_until("readyok"), ["readyok"]);

    engine.send("ucinewgame");
    engine.send("position startpos moves e2e4 c7c5");
    engine.send("go depth 3");
    let search = engine.read_until("bestmove");
    assert!(
        search.iter().any(|line| line.starts_with("info depth 3 ")),
        "{search:?}"
    );

    engine.send("position startpos moves e2e4 c7c5 g1f3 d7d6");
    engine.send("go wtime 1000 btime 1000 winc 10 binc 10");
    let search = engine.read_until("bestmove");
    assert!(search.len() >= 2, "{search:?}");
    engine.quit();
}

#[test]
fn stop_ends_an_infinite_search() {
    let mut engine = Engine::start();
    engine.send("position startpos");
    engine.send("go infinite");
    engine.send("isready");
    // `readyok` comes while the search is still running.
    let lines = engine.read_until("readyok");
    assert!(
        !lines.iter().any(|line| line.starts_with("bestmove")),
        "{lines:?}"
    );
    engine.send("stop");
    engine.read_until("bestmove");
    engine.quit();
}

#[test]
fn the_debug_command_shows_the_position() {
    let out = run("position startpos moves e2e4 c7c5\nd\n");
    assert!(
        out.contains("FEN: rnbqkbnr/pp1ppppp/8/2p5/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2"),
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
    // An infinite search is stopped and still answers.
    let out = run("go infinite\n");
    assert!(
        out.lines().last().unwrap().starts_with("bestmove "),
        "{out}"
    );
    // A search with a limit is allowed to finish first.
    let out = run("position startpos\ngo depth 4\n");
    assert!(out.contains("\ninfo depth 4 "), "{out}");
    assert!(
        out.lines().last().unwrap().starts_with("bestmove "),
        "{out}"
    );
}
