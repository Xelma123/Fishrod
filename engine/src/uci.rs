//! The Universal Chess Interface (UCI): the text protocol through which a
//! chess GUI drives the engine. Commands arrive on standard input, one per
//! line; answers go to standard output.
//!
//! A search runs on its own thread, so the engine keeps reading commands
//! while it thinks: `isready` is answered at once, and `stop` ends the search,
//! which then prints its `bestmove`.

use std::fmt::Display;
use std::io::{self, BufRead, Write};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::board::Position;
use crate::eval::evaluate;
use crate::movegen::legal_moves;
use crate::search::{Limits, Report, mate_in, search};
use crate::types::{Color, Move, MoveKind};

/// Largest accepted `Hash` value, in MiB.
const HASH_MAX: u64 = 65_536;
/// Default and largest `Move Overhead`, in milliseconds: time kept in hand
/// for the delay between the engine and the GUI's clock.
const MOVE_OVERHEAD_DEFAULT: u64 = 10;
const MOVE_OVERHEAD_MAX: u64 = 5_000;
/// Moves assumed to remain until the next time control when the GUI does
/// not say (`movestogo`).
const DEFAULT_MOVES_TO_GO: u64 = 30;

/// What the command loop does after a command.
#[derive(Debug, PartialEq, Eq)]
enum Flow {
    Continue,
    Quit,
}

/// The engine's state between commands. `W` is where the output goes:
/// standard output, or a buffer in tests.
struct Uci<W> {
    /// The position the next search starts from.
    position: Position,
    /// Keys of the earlier positions of the game since the last irreversible
    /// move, oldest first. Only these can repeat, so the search checks them
    /// for repetitions.
    history: Vec<u64>,
    /// Whether castling moves are written as "king takes rook" (`UCI_Chess960`).
    chess960: bool,
    /// `Move Overhead`, in milliseconds.
    move_overhead: u64,
    /// Shared with the search thread, which prints its own lines.
    out: Arc<Mutex<W>>,
    /// The running or finished search that has not been waited for yet.
    search: Option<SearchThread>,
}

/// A search running on its own thread.
struct SearchThread {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<()>,
    /// Whether the search has no limit and runs until `stop`.
    unbounded: bool,
}

/// Reads commands from standard input until `quit` or the end of the input.
///
/// `quit` stops a running search at once. At the end of the input a search
/// with a limit still finishes and prints its move, which makes piping
/// commands into the engine by hand work; one without a limit is stopped.
pub fn run() -> io::Result<()> {
    let mut uci = Uci::new(io::stdout());
    for line in io::stdin().lock().lines() {
        match line.and_then(|line| uci.handle(&line)) {
            Ok(Flow::Continue) => {}
            Ok(Flow::Quit) => {
                uci.stop_search();
                return Ok(());
            }
            Err(err) => {
                uci.stop_search();
                return Err(err);
            }
        }
    }
    if uci.search.as_ref().is_some_and(|search| search.unbounded) {
        uci.stop_search();
    } else {
        uci.wait_for_search();
    }
    Ok(())
}

/// Writes one line and flushes it, so the GUI sees it at once.
fn write_line<W: Write>(out: &Mutex<W>, line: impl Display) -> io::Result<()> {
    // A thread that panicked while printing leaves the lock poisoned; the
    // output itself is still usable.
    let mut out = out.lock().unwrap_or_else(PoisonError::into_inner);
    writeln!(out, "{line}")?;
    out.flush()
}

impl<W: Write + Send + 'static> Uci<W> {
    fn new(out: W) -> Uci<W> {
        Uci {
            position: Position::startpos(),
            history: Vec::new(),
            chess960: false,
            move_overhead: MOVE_OVERHEAD_DEFAULT,
            out: Arc::new(Mutex::new(out)),
            search: None,
        }
    }

    fn say(&self, line: impl Display) -> io::Result<()> {
        write_line(&self.out, line)
    }

    /// Carries out one command line.
    fn handle(&mut self, line: &str) -> io::Result<Flow> {
        let line = line.trim();
        let (command, args) = match line.split_once(char::is_whitespace) {
            Some((command, args)) => (command, args.trim()),
            None => (line, ""),
        };
        match command {
            "" => {}
            "uci" => self.identify()?,
            "isready" => self.say("readyok")?,
            "ucinewgame" => {
                self.stop_search();
                self.position = Position::startpos();
                self.history.clear();
            }
            "position" => {
                self.stop_search();
                if let Err(err) = self.set_position(args) {
                    self.say(format_args!("info string {err}"))?;
                }
            }
            "setoption" => {
                self.stop_search();
                if let Err(err) = self.set_option(args) {
                    self.say(format_args!("info string {err}"))?;
                }
            }
            "go" => self.go(args)?,
            "stop" => self.stop_search(),
            // Not part of UCI: shows the current position, for debugging by hand.
            "d" => {
                let score = evaluate(&self.position);
                let white_view = match self.position.side_to_move() {
                    Color::White => score,
                    Color::Black => -score,
                };
                self.say(format_args!(
                    "{}\nKey: {:016x}\nEval: {white_view:+} cp (white's point of view)",
                    self.position,
                    self.position.key()
                ))?;
            }
            "quit" => return Ok(Flow::Quit),
            _ => self.say(format_args!("info string unknown command '{command}'"))?,
        }
        Ok(Flow::Continue)
    }

    /// Answers `uci`: the engine's name, its options and `uciok`.
    fn identify(&self) -> io::Result<()> {
        self.say(format_args!("id name {} {}", crate::NAME, crate::VERSION))?;
        self.say(format_args!("id author {}", crate::AUTHOR))?;
        self.say(format_args!(
            "option name Hash type spin default 16 min 1 max {HASH_MAX}"
        ))?;
        self.say("option name Threads type spin default 1 min 1 max 1")?;
        self.say(format_args!(
            "option name Move Overhead type spin default {MOVE_OVERHEAD_DEFAULT} min 0 max {MOVE_OVERHEAD_MAX}"
        ))?;
        self.say("option name UCI_Chess960 type check default false")?;
        self.say("uciok")
    }

    /// `go`: starts a search of the current position on its own thread. The
    /// search prints an `info` line per completed iteration, then `bestmove`.
    fn go(&mut self, args: &str) -> io::Result<()> {
        self.stop_search();
        let command = match GoCommand::parse(args) {
            Ok(command) => command,
            Err(err) => return self.say(format_args!("info string {err}")),
        };
        let limits = command.limits(self.position.side_to_move(), self.move_overhead);

        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let out = Arc::clone(&self.out);
        let position = self.position;
        let history = self.history.clone();
        let chess960 = self.chess960;
        let infinite = command.infinite;
        let unbounded = infinite
            || (limits.depth.is_none() && limits.nodes.is_none() && limits.hard_time.is_none());
        let handle = thread::spawn(move || {
            // If the output is gone, so is the GUI: there is no one left to
            // tell, so write errors are ignored here.
            let outcome = search(&position, &history, limits, &thread_stop, |report| {
                let _ = write_line(&out, info_line(report, chess960));
            });
            // After `go infinite` the GUI expects `bestmove` only once it has
            // sent `stop`, even if the search ends by itself.
            if infinite {
                while !thread_stop.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(1));
                }
            }
            // "0000" is the UCI null move, for a position without legal moves.
            let best = outcome
                .best_move
                .map_or_else(|| "0000".to_string(), |mv| mv.to_uci(chess960));
            let _ = write_line(&out, format_args!("bestmove {best}"));
        });
        self.search = Some(SearchThread {
            stop,
            handle,
            unbounded,
        });
        Ok(())
    }

    /// Stops the search, if one is running, and waits until it has printed
    /// its `bestmove`.
    fn stop_search(&mut self) {
        if let Some(search) = self.search.take() {
            search.stop.store(true, Ordering::Relaxed);
            Self::join(search.handle);
        }
    }

    /// Waits for a search to end by itself.
    fn wait_for_search(&mut self) {
        if let Some(search) = self.search.take() {
            Self::join(search.handle);
        }
    }

    fn join(handle: JoinHandle<()>) {
        // A panic in the search is a bug: pass it on rather than hide it.
        if let Err(panic) = handle.join() {
            std::panic::resume_unwind(panic);
        }
    }

    /// `position [startpos | fen <fen>] [moves <move>...]`.
    ///
    /// An invalid FEN leaves the current position unchanged. An illegal move
    /// keeps the position reached before it and is reported.
    fn set_position(&mut self, args: &str) -> Result<(), String> {
        let mut tokens = args.split_whitespace();
        let mut pos = match tokens.next() {
            Some("startpos") => match tokens.next() {
                None | Some("moves") => Position::startpos(),
                Some(other) => return Err(format!("expected 'moves', found '{other}'")),
            },
            Some("fen") => {
                // The FEN runs up to the word "moves", which is consumed here too.
                let fen: Vec<&str> = tokens.by_ref().take_while(|&t| t != "moves").collect();
                Position::from_fen(&fen.join(" ")).map_err(|err| format!("invalid FEN: {err}"))?
            }
            _ => return Err("expected 'position startpos' or 'position fen <fen>'".to_string()),
        };

        let mut history = Vec::new();
        let mut result = Ok(());
        for text in tokens {
            let Some(mv) = parse_move(&pos, text, self.chess960) else {
                result = Err(format!("illegal or unknown move '{text}'"));
                break;
            };
            history.push(pos.key());
            pos = pos.make_move(mv);
            // After a capture or a pawn move no earlier position can come back.
            if pos.halfmove_clock() == 0 {
                history.clear();
            }
        }
        self.position = pos;
        self.history = history;
        result
    }

    /// `setoption name <name> [value <value>]`. Option names may contain spaces.
    fn set_option(&mut self, args: &str) -> Result<(), String> {
        let mut tokens = args.split_whitespace();
        if tokens.next() != Some("name") {
            return Err("expected 'setoption name <name> [value <value>]'".to_string());
        }
        let name = tokens
            .by_ref()
            .take_while(|&t| t != "value")
            .collect::<Vec<_>>()
            .join(" ");
        let value = tokens.collect::<Vec<_>>().join(" ");

        match name.to_ascii_lowercase().as_str() {
            // There is no transposition table yet: the value is checked, then unused.
            "hash" => {
                parse_spin(&value, 1, HASH_MAX)?;
            }
            "threads" => {
                parse_spin(&value, 1, 1)?;
            }
            "move overhead" => self.move_overhead = parse_spin(&value, 0, MOVE_OVERHEAD_MAX)?,
            "uci_chess960" => self.chess960 = parse_check(&value)?,
            _ => return Err(format!("unknown option '{name}'")),
        }
        Ok(())
    }
}

/// The parameters of a `go` command.
#[derive(Debug, Default)]
struct GoCommand {
    depth: Option<u32>,
    nodes: Option<u64>,
    /// Exact time for this move, in milliseconds.
    movetime: Option<u64>,
    /// Time left on each clock (white, black), in milliseconds.
    time: [Option<u64>; 2],
    /// Increment per move for each side, in milliseconds.
    increment: [Option<u64>; 2],
    movestogo: Option<u64>,
    infinite: bool,
}

impl GoCommand {
    /// Reads the parameters; unknown ones (such as `ponder`) are skipped.
    fn parse(args: &str) -> Result<GoCommand, String> {
        let mut go = GoCommand::default();
        let mut tokens = args.split_whitespace();
        while let Some(token) = tokens.next() {
            match token {
                "depth" => go.depth = Some(parse_number(tokens.next(), token)?),
                "nodes" => go.nodes = Some(parse_number(tokens.next(), token)?),
                "movetime" => go.movetime = Some(parse_number(tokens.next(), token)?),
                "wtime" => go.time[0] = Some(parse_number(tokens.next(), token)?),
                "btime" => go.time[1] = Some(parse_number(tokens.next(), token)?),
                "winc" => go.increment[0] = Some(parse_number(tokens.next(), token)?),
                "binc" => go.increment[1] = Some(parse_number(tokens.next(), token)?),
                "movestogo" => go.movestogo = Some(parse_number(tokens.next(), token)?),
                "infinite" => go.infinite = true,
                _ => {}
            }
        }
        Ok(go)
    }

    /// The search limits for side `us`, keeping `overhead` milliseconds in
    /// hand. Without any limit (a bare `go`) the search runs until `stop`.
    fn limits(&self, us: Color, overhead: u64) -> Limits {
        let mut limits = Limits {
            depth: self.depth,
            nodes: self.nodes,
            ..Limits::default()
        };
        if self.infinite {
            return limits;
        }
        let millis = Duration::from_millis;
        if let Some(movetime) = self.movetime {
            let time = millis(movetime.saturating_sub(overhead).max(1));
            limits.soft_time = Some(time);
            limits.hard_time = Some(time);
        } else if let Some(left) = self.time[us.index()] {
            // Spread the clock over the moves still to play, and spend most
            // of each increment as it comes in.
            let available = left.saturating_sub(overhead).max(1);
            let increment = self.increment[us.index()].unwrap_or(0);
            let moves_to_go = self.movestogo.unwrap_or(DEFAULT_MOVES_TO_GO).max(1);
            let target = available / moves_to_go + increment * 3 / 4;
            // A hard stop at three times the target, but never past half of
            // the clock; no new iteration after half the target.
            let hard = (target * 3).min(available / 2).max(1);
            let soft = (target / 2).min(hard);
            limits.soft_time = Some(millis(soft));
            limits.hard_time = Some(millis(hard));
        }
        limits
    }
}

/// Finds the legal move written as `text` in UCI notation.
///
/// Castling is accepted in both notations, as the king's own move (`e1g1`) or
/// as the king taking its rook (`e1h1`); the one `chess960` selects is tried
/// first.
fn parse_move(pos: &Position, text: &str, chess960: bool) -> Option<Move> {
    let text = text.to_ascii_lowercase();
    let moves = legal_moves(pos);
    let written_as = |mv: &&Move, chess960: bool| mv.to_uci(chess960) == text;
    moves
        .iter()
        .find(|mv| written_as(mv, chess960))
        .or_else(|| {
            moves
                .iter()
                .find(|mv| mv.kind() == MoveKind::Castle && written_as(mv, !chess960))
        })
        .copied()
}

/// Parses the number that follows a `go` parameter.
fn parse_number<T: FromStr>(token: Option<&str>, name: &str) -> Result<T, String> {
    token
        .and_then(|text| text.parse().ok())
        .ok_or_else(|| format!("'{name}' needs a whole number"))
}

/// The `info` line for a completed iteration.
fn info_line(report: &Report, chess960: bool) -> String {
    let millis = report.time.as_millis();
    let nps = u128::from(report.nodes) * 1000 / millis.max(1);
    let score = match mate_in(report.score) {
        Some(moves) => format!("mate {moves}"),
        None => format!("cp {}", report.score),
    };
    let pv: Vec<String> = report.pv.iter().map(|mv| mv.to_uci(chess960)).collect();
    format!(
        "info depth {} score {score} nodes {} nps {nps} time {millis} pv {}",
        report.depth,
        report.nodes,
        pv.join(" ")
    )
}

/// Parses the value of a `spin` option, which must lie in `min..=max`.
fn parse_spin(value: &str, min: u64, max: u64) -> Result<u64, String> {
    let number: u64 = value
        .parse()
        .map_err(|_| format!("'{value}' is not a whole number"))?;
    if (min..=max).contains(&number) {
        Ok(number)
    } else {
        Err(format!("{number} is outside {min}..={max}"))
    }
}

/// Parses the value of a `check` option: `true` or `false`.
fn parse_check(value: &str) -> Result<bool, String> {
    match value.to_ascii_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("'{value}' is not true or false")),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    /// Everything the engine has printed so far.
    fn output(uci: &Uci<Vec<u8>>) -> String {
        String::from_utf8(uci.out.lock().unwrap().clone()).unwrap()
    }

    /// Feeds the command lines to a fresh engine, lets a search started by
    /// them finish, and returns the engine with its output.
    fn session(commands: &str) -> (Uci<Vec<u8>>, String) {
        let mut uci = Uci::new(Vec::new());
        for line in commands.lines() {
            if uci.handle(line).unwrap() == Flow::Quit {
                break;
            }
        }
        uci.wait_for_search();
        let out = output(&uci);
        (uci, out)
    }

    #[test]
    fn handshake() {
        let (_, out) = session("uci\nisready");
        assert!(out.starts_with("id name Fishrod "));
        assert!(out.contains("\noption name Move Overhead type spin default 10 min 0 max 5000\n"));
        assert!(out.contains("\noption name UCI_Chess960 type check default false\n"));
        assert!(out.ends_with("uciok\nreadyok\n"));
    }

    #[test]
    fn position_with_moves() {
        let (uci, out) = session("position startpos moves e2e4 e7e5 g1f3");
        assert_eq!(out, "");
        assert_eq!(
            uci.position.to_fen(),
            "rnbqkbnr/pppp1ppp/8/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq - 1 2"
        );
        // e7e5 was a pawn move, so only the position before g1f3 can repeat.
        assert_eq!(uci.history.len(), 1);

        let (uci, _) = session("position fen 4k3/8/8/8/8/8/8/R3K2R w KQ - 0 1 moves e1c1");
        assert_eq!(uci.position.to_fen(), "4k3/8/8/8/8/8/8/2KR3R b - - 1 1");
        let (uci, _) = session("position fen 4k3/8/8/8/8/8/8/R3K2R w KQ - 0 1");
        assert!(uci.history.is_empty());
    }

    #[test]
    fn repeated_positions_stay_in_the_history() {
        let (uci, _) = session("position startpos moves g1f3 g8f6 f3g1 f6g8");
        let start = Position::startpos().key();
        assert_eq!(uci.position.key(), start);
        assert_eq!(uci.history.len(), 4);
        assert_eq!(uci.history[0], start);
    }

    #[test]
    fn castling_is_read_in_both_notations() {
        let fen = "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1";
        for chess960 in [false, true] {
            for mv in ["e1g1", "e1h1"] {
                let mut uci = Uci::new(Vec::new());
                uci.chess960 = chess960;
                uci.set_position(&format!("fen {fen} moves {mv}")).unwrap();
                assert_eq!(uci.position.to_fen(), "r3k2r/8/8/8/8/8/8/R4RK1 b kq - 1 1");
            }
        }
        // Chess960: king f1, rook h1. "f1g1" is a plain king step here, while
        // castling is written "f1h1".
        let (uci, _) = session(
            "setoption name UCI_Chess960 value true\n\
             position fen 4k3/8/8/8/8/8/8/5K1R w H - 0 1 moves f1g1",
        );
        assert_eq!(uci.position.to_fen(), "4k3/8/8/8/8/8/8/6KR b - - 1 1");
        let (uci, _) = session(
            "setoption name UCI_Chess960 value true\n\
             position fen 4k3/8/8/8/8/8/8/5K1R w H - 0 1 moves f1h1",
        );
        assert_eq!(uci.position.to_fen(), "4k3/8/8/8/8/8/8/5RK1 b - - 1 1");
    }

    #[test]
    fn promotions_are_read() {
        let (uci, _) = session("position fen 4k3/P7/8/8/8/8/8/4K3 w - - 0 1 moves a7a8n");
        assert_eq!(uci.position.to_fen(), "N3k3/8/8/8/8/8/8/4K3 b - - 0 1");
    }

    #[test]
    fn bad_input_is_reported() {
        // The illegal move and everything after it are dropped.
        let (uci, out) = session("position startpos moves e2e4 e2e5 d7d5");
        assert_eq!(
            uci.position.to_fen(),
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1"
        );
        assert_eq!(out, "info string illegal or unknown move 'e2e5'\n");

        // An invalid FEN leaves the position alone.
        let (uci, out) = session("position startpos moves e2e4\nposition fen nonsense");
        assert_eq!(uci.position.side_to_move(), Color::Black);
        assert!(out.starts_with("info string invalid FEN"));

        let (_, out) = session("position\nfoo bar\ngo depth x");
        assert_eq!(
            out,
            "info string expected 'position startpos' or 'position fen <fen>'\n\
             info string unknown command 'foo'\n\
             info string 'depth' needs a whole number\n"
        );
    }

    #[test]
    fn options() {
        let (uci, out) = session(
            "setoption name UCI_Chess960 value true\n\
             setoption name Hash value 64\n\
             setoption name Threads value 1\n\
             setoption name Move Overhead value 50",
        );
        assert!(uci.chess960);
        assert_eq!(uci.move_overhead, 50);
        assert_eq!(out, "");

        let (_, out) = session(
            "setoption name Threads value 4\n\
             setoption name Hash value 0\n\
             setoption name Ponder value true\n\
             setoption name UCI_Chess960 value maybe",
        );
        assert_eq!(
            out,
            "info string 4 is outside 1..=1\n\
             info string 0 is outside 1..=65536\n\
             info string unknown option 'Ponder'\n\
             info string 'maybe' is not true or false\n"
        );
    }

    #[test]
    fn go_prints_info_and_bestmove() {
        let (_, out) = session("position startpos\ngo depth 3");
        assert!(out.starts_with("info depth 1 score cp "), "{out}");
        assert!(out.contains("\ninfo depth 3 score cp "), "{out}");
        let last = out.lines().last().unwrap();
        assert!(last.starts_with("bestmove ") && last.len() == "bestmove e2e4".len());

        let (_, out) = session("position fen 6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1\ngo depth 3");
        assert!(out.contains(" score mate 1 "), "{out}");
        assert!(out.ends_with("bestmove a1a8\n"), "{out}");

        // No legal move: the null move.
        let (_, out) = session("position fen 7k/5Q2/6K1/8/8/8/8/8 b - - 0 1\ngo depth 2");
        assert_eq!(out, "bestmove 0000\n");
    }

    #[test]
    fn infinite_search_waits_for_stop_and_still_answers_isready() {
        let mut uci = Uci::new(Vec::new());
        uci.handle("position startpos").unwrap();
        uci.handle("go infinite").unwrap();
        uci.handle("isready").unwrap();
        thread::sleep(Duration::from_millis(50));
        assert!(output(&uci).contains("readyok\n"));
        assert!(!output(&uci).contains("bestmove"));

        uci.handle("stop").unwrap();
        let out = output(&uci);
        assert!(
            out.lines().last().unwrap().starts_with("bestmove "),
            "{out}"
        );
        assert_eq!(out.matches("bestmove").count(), 1);
    }

    #[test]
    fn a_clock_ends_the_search_by_itself() {
        let start = Instant::now();
        let (_, out) = session("position startpos\ngo wtime 500 btime 500 winc 10 binc 10");
        assert!(
            out.lines().last().unwrap().starts_with("bestmove "),
            "{out}"
        );
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "{:?}",
            start.elapsed()
        );
    }

    #[test]
    fn a_new_command_stops_the_running_search() {
        let mut uci = Uci::new(Vec::new());
        uci.handle("go infinite").unwrap();
        uci.handle("position startpos moves e2e4").unwrap();
        assert!(output(&uci).contains("bestmove "));
        assert!(uci.search.is_none());
    }

    #[test]
    fn time_management() {
        let go = |args: &str| GoCommand::parse(args).unwrap();
        let ms = |millis: u64| Some(Duration::from_millis(millis));

        // White: (60000 - 10) / 30 + 1000 * 3/4 = 1999 + 750 = 2749 ms target.
        let limits = go("wtime 60000 btime 30000 winc 1000 binc 0").limits(Color::White, 10);
        assert_eq!((limits.soft_time, limits.hard_time), (ms(1374), ms(8247)));
        // Black: (30000 - 10) / 30 = 999 ms target.
        let limits = go("wtime 60000 btime 30000 winc 1000 binc 0").limits(Color::Black, 10);
        assert_eq!((limits.soft_time, limits.hard_time), (ms(499), ms(2997)));
        // The last move before the time control: never more than half of what is left.
        let limits = go("wtime 100 btime 100 movestogo 1").limits(Color::White, 10);
        assert_eq!(limits.hard_time, ms(45));

        let limits = go("movetime 500").limits(Color::White, 10);
        assert_eq!((limits.soft_time, limits.hard_time), (ms(490), ms(490)));
        let limits = go("infinite wtime 1000").limits(Color::White, 10);
        assert_eq!((limits.soft_time, limits.hard_time), (None, None));
        let limits = go("depth 7 nodes 1000").limits(Color::White, 10);
        assert_eq!(
            (limits.depth, limits.nodes, limits.hard_time),
            (Some(7), Some(1000), None)
        );
    }

    #[test]
    fn new_game_and_quit() {
        let (uci, _) = session("position startpos moves e2e4\nucinewgame");
        assert_eq!(uci.position, Position::startpos());
        let (_, out) = session("quit\nisready");
        assert_eq!(out, "");
    }
}
