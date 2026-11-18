//! The Fishrod engine for web pages.
//!
//! Compiled to WebAssembly (`cargo build --release --target wasm32-wasip1
//! -p fishrod-web`) and driven from JavaScript through one text command
//! interface. JavaScript writes a command into the buffer at
//! `fishrod_buffer()` (at most `fishrod_buffer_size()` bytes, UTF-8), calls
//! `fishrod_command` with its length, and reads the answer, whose length
//! `fishrod_command` returns, from the same buffer.
//!
//! Commands; moves are in UCI notation and played from the starting position:
//!
//! - `state <moves>`: the position after the moves. Answer, one per line:
//!   `fen <fen>`, `turn w|b`, `check 0|1`, `legal <moves>` and `status <s>`,
//!   where `<s>` is `ongoing`, `checkmate`, `stalemate`, `fifty` (the
//!   fifty-move rule), `repetition` (threefold) or `material` (neither side
//!   can mate).
//! - `play <think ms> <variety cp> <elo, 0 for full strength> <seed> <moves>`:
//!   the engine's move in that position, as against people in the UCI
//!   version (see `fishrod::play`). Answer: `bestmove <move> score <cp n |
//!   mate n> depth <d> nodes <n> time <ms>`, with `0000` when there is no
//!   legal move.
//! - `newgame`: forgets what earlier searches found. Answer: `ok`.
//! - `bench`: the speed check of `fishrod bench`. Answer: `<n> nodes <n> nps`.
//!
//! Anything else, or a malformed command, answers `error <reason>`.

#![deny(unsafe_code)]

use std::sync::atomic::AtomicBool;
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant};

use fishrod::bench;
use fishrod::board::Position;
use fishrod::movegen::legal_moves;
use fishrod::play::{Style, think_limits};
use fishrod::search::{TranspositionTable, insufficient_material, mate_in, search};
use fishrod::types::Color;
use fishrod::uci::parse_move;

/// The size of the buffer shared with JavaScript, in bytes: room for a long
/// game's moves and every legal move of a position.
const BUFFER_SIZE: usize = 1 << 16;

/// The buffer commands and answers pass through.
static BUFFER: Mutex<[u8; BUFFER_SIZE]> = Mutex::new([0; BUFFER_SIZE]);

/// Kept from one move to the next for the whole game, like the UCI version's.
static TABLE: LazyLock<TranspositionTable> =
    LazyLock::new(|| TranspositionTable::new(16).expect("a 16 MiB hash table fits in memory"));

/// The address of the shared buffer in the module's memory.
#[allow(unsafe_code)]
// SAFETY: `no_mangle` only keeps the name so that JavaScript can find the
// function; no other item of the program is named `fishrod_buffer`.
#[unsafe(no_mangle)]
pub extern "C" fn fishrod_buffer() -> usize {
    let buffer = BUFFER.lock().unwrap_or_else(PoisonError::into_inner);
    buffer.as_ptr() as usize
}

/// The size of the shared buffer, in bytes.
#[allow(unsafe_code)]
// SAFETY: as for `fishrod_buffer`: the name is unique.
#[unsafe(no_mangle)]
pub extern "C" fn fishrod_buffer_size() -> usize {
    BUFFER_SIZE
}

/// Carries out the command of `length` bytes in the shared buffer, writes
/// the answer there and returns its length.
#[allow(unsafe_code)]
// SAFETY: as for `fishrod_buffer`: the name is unique.
#[unsafe(no_mangle)]
pub extern "C" fn fishrod_command(length: usize) -> usize {
    let mut buffer = BUFFER.lock().unwrap_or_else(PoisonError::into_inner);
    let command = String::from_utf8_lossy(&buffer[..length.min(BUFFER_SIZE)]).into_owned();
    let answer = run(&command).unwrap_or_else(|reason| format!("error {reason}"));
    let bytes = answer.as_bytes();
    let length = bytes.len().min(BUFFER_SIZE);
    buffer[..length].copy_from_slice(&bytes[..length]);
    length
}

/// Carries out one command.
fn run(command: &str) -> Result<String, String> {
    let mut words = command.split_whitespace();
    match words.next() {
        Some("state") => {
            let game = Game::play(words)?;
            Ok(game.describe())
        }
        Some("play") => {
            let mut number = |name: &str| -> Result<u64, String> {
                words
                    .next()
                    .and_then(|word| word.parse().ok())
                    .ok_or_else(|| format!("'play' needs a whole number for {name}"))
            };
            let think = number("the think time")?;
            let variety = number("the variety")?;
            let elo = number("the Elo")?;
            let seed = number("the seed")?;
            let game = Game::play(words)?;
            Ok(game.best_move(think, variety, elo, seed))
        }
        Some("newgame") => {
            TABLE.clear();
            Ok("ok".to_string())
        }
        Some("bench") => {
            let summary = bench::run(bench::DEFAULT_DEPTH, |_, _, _| {});
            let millis = summary.time.as_millis().max(1);
            let nps = u128::from(summary.nodes) * 1000 / millis;
            Ok(format!("{} nodes {nps} nps", summary.nodes))
        }
        Some(other) => Err(format!("unknown command '{other}'")),
        None => Err("empty command".to_string()),
    }
}

/// A game from the starting position.
struct Game {
    position: Position,
    /// Keys of the positions since the last capture or pawn move, oldest
    /// first: only these can repeat.
    history: Vec<u64>,
}

impl Game {
    /// The game after `moves`, or the first move that is not legal.
    fn play<'a>(moves: impl Iterator<Item = &'a str>) -> Result<Game, String> {
        let mut game = Game {
            position: Position::startpos(),
            history: Vec::new(),
        };
        for text in moves {
            let mv = parse_move(&game.position, text, false)
                .ok_or_else(|| format!("illegal or unknown move '{text}'"))?;
            game.history.push(game.position.key());
            game.position = game.position.make_move(mv);
            if game.position.halfmove_clock() == 0 {
                game.history.clear();
            }
        }
        Ok(game)
    }

    /// The answer to `state`.
    fn describe(&self) -> String {
        let pos = &self.position;
        let moves: Vec<String> = legal_moves(pos).iter().map(|mv| mv.to_uci(false)).collect();
        let turn = match pos.side_to_move() {
            Color::White => "w",
            Color::Black => "b",
        };
        let check = u8::from(!pos.checkers().is_empty());
        format!(
            "fen {}\nturn {turn}\ncheck {check}\nlegal {}\nstatus {}",
            pos.to_fen(),
            moves.join(" "),
            self.status()
        )
    }

    /// Whether and how the game is over.
    fn status(&self) -> &'static str {
        let pos = &self.position;
        if legal_moves(pos).is_empty() {
            if pos.checkers().is_empty() {
                "stalemate"
            } else {
                "checkmate"
            }
        } else if pos.halfmove_clock() >= 100 {
            "fifty"
        } else if self.history.iter().filter(|&&key| key == pos.key()).count() >= 2 {
            "repetition"
        } else if insufficient_material(pos) {
            "material"
        } else {
            "ongoing"
        }
    }

    /// The answer to `play`.
    fn best_move(&self, think: u64, variety: u64, elo: u64, seed: u64) -> String {
        let style = Style {
            elo: (elo > 0).then_some(elo as u32),
            variety: variety as u32,
        };
        let limits = think_limits(Duration::from_millis(think.max(1)));
        let plan = style.plan(&self.position, limits, false);
        let start = Instant::now();
        let outcome = search(
            &self.position,
            &self.history,
            plan.limits,
            &TABLE,
            &AtomicBool::new(false),
            |_| {},
        );
        let time = start.elapsed().as_millis();
        let played = plan.pick(&outcome, self.position.key() ^ seed);
        let best = played.map_or_else(|| "0000".to_string(), |mv| mv.to_uci(false));
        let score = match mate_in(outcome.score) {
            Some(moves) => format!("mate {moves}"),
            None => format!("cp {}", outcome.score),
        };
        format!(
            "bestmove {best} score {score} depth {} nodes {} time {time}",
            outcome.depth, outcome.nodes
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_describe_the_game() {
        let start = run("state").unwrap();
        assert!(start.contains("\nturn w\n"));
        assert!(start.ends_with("\nstatus ongoing"));
        assert_eq!(start.lines().nth(3).unwrap().split(' ').count(), 21);

        // Fool's mate.
        let mate = run("state f2f3 e7e5 g2g4 d8h4").unwrap();
        assert!(
            mate.contains("\ncheck 1\nlegal \nstatus checkmate"),
            "{mate}"
        );
        // Knights out and back twice: the starting position a third time.
        let repeated = run("state g1f3 g8f6 f3g1 f6g8 g1f3 g8f6 f3g1 f6g8").unwrap();
        assert!(repeated.ends_with("status repetition"), "{repeated}");

        assert_eq!(
            run("state e2e5").unwrap_err(),
            "illegal or unknown move 'e2e5'"
        );
        assert!(run("dance").is_err());
    }

    #[test]
    fn the_engine_plays_a_legal_move() {
        let answer = run("play 200 0 0 1 e2e4").unwrap();
        let mv = answer.split(' ').nth(1).unwrap();
        let after_e4 = Game::play(["e2e4"].into_iter()).unwrap().position;
        assert!(parse_move(&after_e4, mv, false).is_some(), "{answer}");
        assert!(answer.contains(" depth "));
        // A mate in one is played even with variety and at low strength.
        let moves = "f2f3 e7e5 g2g4";
        let mate = run(&format!("play 200 10 800 7 {moves}")).unwrap();
        assert!(mate.starts_with("bestmove d8h4 "), "{mate}");
    }

    #[test]
    fn commands_pass_through_the_buffer() {
        let command = b"state e2e4";
        BUFFER.lock().unwrap()[..command.len()].copy_from_slice(command);
        let length = fishrod_command(command.len());
        let buffer = BUFFER.lock().unwrap();
        let answer = std::str::from_utf8(&buffer[..length]).unwrap();
        assert!(answer.starts_with("fen rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b"));
    }
}
