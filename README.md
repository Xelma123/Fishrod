# Fishrod

Fishrod is a UCI chess engine written from scratch in Rust.

**Status:** Version 0.1.0. Phases 1 to 4 of 7 are complete: a classical alpha-beta search with a hand-crafted evaluation, measured at about 2500 on the CCRL 40/4 scale in matches against Stockfish's strength-limited mode. Every change that affects its strength is measured with an SPRT. Phase 5 (an NNUE evaluation trained on Fishrod's own games) is next.

## Design

- **Board representation:** bitboards, with magic bitboards for sliding-piece attacks. Move generation produces only legal moves, is verified with perft and supports Chess960.
- **Search:** alpha-beta with principal variation search, iterative deepening, quiescence search, a transposition table, and pruning and reduction techniques.
- **Evaluation:** a hand-crafted evaluation at first, later replaced by an NNUE network trained exclusively on positions from Fishrod's own self-play games.

The board, move generation, search, evaluation and network inference are original work; no code is taken from other engines. Ideas adopted from published work are credited in the commit history.

## Download

Prebuilt binaries for Windows and Linux are on the [releases page](https://github.com/Xelma123/Fishrod/releases). They run on any 64-bit x86 processor.

## Building

Fishrod requires a stable Rust toolchain that supports the 2024 edition.

```sh
cargo build --release
```

The binary is written to `target/release/fishrod` (`fishrod.exe` on Windows). To optimize for the processor of the build machine, set `RUSTFLAGS="-C target-cpu=native"` before building.

## Usage

Fishrod communicates through the Universal Chess Interface (UCI) and is intended to be used with a UCI-compatible graphical interface such as Cute Chess, Arena or En Croissant. It also provides commands for development:

| Command | Description |
|---|---|
| `fishrod` | Starts the engine in UCI mode. |
| `fishrod perft <depth> [fen]` | Counts the leaf nodes of the move tree to the given depth and prints the count for each root move. |
| `fishrod bench [depth]` | Searches a fixed set of positions to a fixed depth and prints the total node count and speed. |
| `fishrod openings <count> [seed]` | Prints balanced opening positions in EPD format, for engine-versus-engine tests. |

Two options make Fishrod more pleasant to play against. `Think Time` (in milliseconds) keeps a brisk pace under a clock: a move takes about that long, less when the choice is easy and up to twice as long when the position turns difficult. `Variety` (in centipawns) varies the first moves among nearly equal ones, so that games do not all open alike. Both are off by default, which gives the engine's full strength for engine matches and long games; 4000 and 10 suit games against people.

With `UCI_LimitStrength`, Fishrod plays at the strength set by `UCI_Elo`, from 500 to 2300. The settings are engine ratings on the CCRL 40/4 scale, measured in matches against Stockfish's strength-limited mode: from 1020 up the engine searches fewer positions per move, and below that it also picks its moves at random among those not much worse than the best, so that it makes mistakes.

## Web version

The `web` directory holds a version of Fishrod that runs entirely in the browser: the engine compiled to WebAssembly, with a board to play against it. It is published at <https://xelma123.github.io/Fishrod/>.

```sh
rustup target add wasm32-wasip1
tools/build-web.sh
python -m http.server 8000 --directory web/www
```

Then open <http://localhost:8000>.

On every push, a GitHub Actions workflow builds the web version, checks that it searches exactly like the native build, and publishes it on GitHub Pages. The page can be installed as an app from the browser and works without a connection.

## Testing

- **Perft:** move generation is checked against published node counts for standard and Chess960 test positions.
- **Bench:** the node count of the fixed-depth benchmark is deterministic and is recorded in each commit message as `Bench: <nodes>`. A change that is not intended to alter search behavior must leave it unchanged.
- **SPRT:** every change that may affect playing strength is tested in engine-versus-engine matches with a sequential probability ratio test before it is merged.

## Roadmap

1. Board representation and legal move generation
2. UCI protocol, alpha-beta search and a hand-crafted evaluation
3. Testing infrastructure for SPRT
4. Search improvements: transposition table, move ordering, pruning and reductions
5. NNUE evaluation trained on self-play data
6. Experimental features, developed behind Cargo feature flags
7. Multithreaded search (Lazy SMP), endgame tablebases and release builds

## License

Fishrod is licensed under the GNU General Public License v3.0. See [LICENSE](LICENSE).

The chess piece images of the web version are by Colin M.L. Burnett, from Wikimedia Commons, and are available under the GPL, BSD and GFDL licenses.
