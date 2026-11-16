//! The Universal Chess Interface (UCI): the text protocol through which a
//! chess GUI drives the engine. Commands arrive on standard input, one per
//! line; answers go to standard output.

use std::io::{self, BufRead, Write};

use crate::board::Position;
use crate::eval::evaluate;
use crate::movegen::legal_moves;
use crate::types::{Color, Move, MoveKind};

/// Largest accepted `Hash` value, in MiB.
const HASH_MAX: u64 = 65_536;

/// What the command loop does after a command.
#[derive(Debug, PartialEq, Eq)]
enum Flow {
    Continue,
    Quit,
}

/// The engine's state between commands.
struct Uci {
    /// The position the next search starts from.
    position: Position,
    /// Keys of the earlier positions of the game since the last irreversible
    /// move, oldest first. Only these can repeat, so the search checks them
    /// for repetitions.
    history: Vec<u64>,
    /// Whether castling moves are written as "king takes rook" (`UCI_Chess960`).
    chess960: bool,
}

/// Reads commands from standard input until `quit` or the end of the input.
pub fn run() -> io::Result<()> {
    let mut uci = Uci::new();
    let mut out = io::stdout();
    for line in io::stdin().lock().lines() {
        if uci.handle(&line?, &mut out)? == Flow::Quit {
            break;
        }
    }
    Ok(())
}

impl Uci {
    fn new() -> Uci {
        Uci {
            position: Position::startpos(),
            history: Vec::new(),
            chess960: false,
        }
    }

    /// Carries out one command line, writing any answer to `out`.
    fn handle(&mut self, line: &str, out: &mut impl Write) -> io::Result<Flow> {
        let line = line.trim();
        let (command, args) = match line.split_once(char::is_whitespace) {
            Some((command, args)) => (command, args.trim()),
            None => (line, ""),
        };
        match command {
            "" => {}
            "uci" => self.identify(out)?,
            "isready" => writeln!(out, "readyok")?,
            "ucinewgame" => {
                self.position = Position::startpos();
                self.history.clear();
            }
            "position" => {
                if let Err(err) = self.set_position(args) {
                    writeln!(out, "info string {err}")?;
                }
            }
            "setoption" => {
                if let Err(err) = self.set_option(args) {
                    writeln!(out, "info string {err}")?;
                }
            }
            // Not part of UCI: shows the current position, for debugging by hand.
            "d" => {
                let score = evaluate(&self.position);
                let white_view = match self.position.side_to_move() {
                    Color::White => score,
                    Color::Black => -score,
                };
                writeln!(
                    out,
                    "{}\nKey: {:016x}\nEval: {white_view:+} cp (white's point of view)",
                    self.position,
                    self.position.key()
                )?;
            }
            "quit" => return Ok(Flow::Quit),
            _ => writeln!(out, "info string unknown command '{command}'")?,
        }
        out.flush()?;
        Ok(Flow::Continue)
    }

    /// Answers `uci`: the engine's name, its options and `uciok`.
    fn identify(&self, out: &mut impl Write) -> io::Result<()> {
        writeln!(out, "id name {} {}", crate::NAME, crate::VERSION)?;
        writeln!(out, "id author {}", crate::AUTHOR)?;
        writeln!(
            out,
            "option name Hash type spin default 16 min 1 max {HASH_MAX}"
        )?;
        writeln!(out, "option name Threads type spin default 1 min 1 max 1")?;
        writeln!(out, "option name UCI_Chess960 type check default false")?;
        writeln!(out, "uciok")
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
            "uci_chess960" => self.chess960 = parse_check(&value)?,
            _ => return Err(format!("unknown option '{name}'")),
        }
        Ok(())
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
    use super::*;

    /// Feeds the command lines to a fresh engine and returns it with its output.
    fn session(commands: &str) -> (Uci, String) {
        let mut uci = Uci::new();
        let mut out = Vec::new();
        for line in commands.lines() {
            if uci.handle(line, &mut out).unwrap() == Flow::Quit {
                break;
            }
        }
        (uci, String::from_utf8(out).unwrap())
    }

    #[test]
    fn handshake() {
        let (_, out) = session("uci\nisready");
        assert!(out.starts_with("id name Fishrod "));
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
                let mut uci = Uci::new();
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
        assert_eq!(uci.position.side_to_move(), crate::types::Color::Black);
        assert!(out.starts_with("info string invalid FEN"));

        let (_, out) = session("position\nfoo bar");
        assert_eq!(
            out,
            "info string expected 'position startpos' or 'position fen <fen>'\n\
             info string unknown command 'foo'\n"
        );
    }

    #[test]
    fn options() {
        let (uci, out) = session(
            "setoption name UCI_Chess960 value true\n\
             setoption name Hash value 64\n\
             setoption name Threads value 1",
        );
        assert!(uci.chess960);
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
    fn new_game_and_quit() {
        let (uci, _) = session("position startpos moves e2e4\nucinewgame");
        assert_eq!(uci.position, Position::startpos());
        let (_, out) = session("quit\nisready");
        assert_eq!(out, "");
    }
}
