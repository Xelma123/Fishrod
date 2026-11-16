//! Zobrist hashing: a 64-bit key that identifies a position.
//!
//! Every feature a position can have (a given piece on a given square, black
//! to move, a castling right, an en passant file) gets a random 64-bit number.
//! A position's key is the XOR of the numbers of the features it has. XOR-ing
//! a number twice cancels it, so a move updates the key by XOR-ing in only
//! what changed instead of recomputing it. Two different positions share a
//! key only by accident, with odds of about one in 2^64 for a given pair.
//!
//! The numbers come from the engine's own generator with a fixed seed and are
//! computed at compile time.

use super::position::Position;
use crate::rng::Rng;
use crate::types::{Color, Piece, Square};

/// Seed of the key generator: the bytes of "zobrist!".
const SEED: u64 = 0x5A6F_6272_6973_7421;

/// The random numbers behind the keys.
struct Keys {
    /// One per piece and square.
    pieces: [[u64; 64]; 12],
    /// One per castling rook square. A right is identified by its rook, which
    /// also tells apart Chess960 rights for two rooks on the same side.
    castling: [u64; 64],
    /// One per file, used only while an en passant capture is available.
    en_passant: [u64; 8],
    /// Included when black is to move.
    black_to_move: u64,
}

static KEYS: Keys = Keys::generate();

impl Keys {
    const fn generate() -> Keys {
        let mut rng = Rng::new(SEED);
        let mut pieces = [[0; 64]; 12];
        let mut piece = 0;
        while piece < 12 {
            let mut sq = 0;
            while sq < 64 {
                pieces[piece][sq] = rng.next_u64();
                sq += 1;
            }
            piece += 1;
        }
        let mut castling = [0; 64];
        let mut sq = 0;
        while sq < 64 {
            castling[sq] = rng.next_u64();
            sq += 1;
        }
        let mut en_passant = [0; 8];
        let mut file = 0;
        while file < 8 {
            en_passant[file] = rng.next_u64();
            file += 1;
        }
        Keys {
            pieces,
            castling,
            en_passant,
            black_to_move: rng.next_u64(),
        }
    }
}

/// Key part for `piece` standing on `sq`.
pub(super) fn piece_key(piece: Piece, sq: Square) -> u64 {
    KEYS.pieces[piece.index()][sq.index()]
}

/// Key part for a set of castling rights.
pub(super) fn castling_key(rooks: &[Option<Square>; 4]) -> u64 {
    rooks
        .iter()
        .flatten()
        .fold(0, |key, rook| key ^ KEYS.castling[rook.index()])
}

/// Key part for an available en passant capture on `sq`.
pub(super) fn en_passant_key(sq: Square) -> u64 {
    KEYS.en_passant[sq.file() as usize]
}

/// Key part for black being the side to move.
pub(super) fn black_to_move_key() -> u64 {
    KEYS.black_to_move
}

impl Position {
    /// The Zobrist key of the position, kept up to date as moves are made.
    /// The move counters are not part of it.
    pub fn key(&self) -> u64 {
        self.key
    }

    /// Computes the key from scratch. Slow; used when a position is set up
    /// and to check the incrementally updated key.
    pub fn compute_key(&self) -> u64 {
        let mut key = 0;
        for sq in Square::all() {
            if let Some(piece) = self.piece_on(sq) {
                key ^= piece_key(piece, sq);
            }
        }
        if self.side_to_move == Color::Black {
            key ^= black_to_move_key();
        }
        key ^= castling_key(&self.castling_rooks);
        if let Some(sq) = self.en_passant {
            key ^= en_passant_key(sq);
        }
        key
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::movegen::legal_moves;
    use crate::types::{Move, MoveKind};

    /// Checks the incremental key against a fresh one at every node of the move tree.
    fn check_tree(pos: &Position, depth: u32) {
        assert_eq!(pos.key(), pos.compute_key(), "{pos:?}");
        if depth > 0 {
            for &mv in &legal_moves(pos) {
                check_tree(&pos.make_move(mv), depth - 1);
            }
        }
    }

    #[test]
    fn incremental_key_matches_fresh_key() {
        let positions = [
            (Position::STARTPOS_FEN, 3),
            (
                "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
                3,
            ),
            ("8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1", 4),
            (
                "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
                3,
            ),
            (
                "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
                3,
            ),
            (
                "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6 0 3",
                3,
            ),
            (
                "bqnb1rkr/pp3ppp/3ppn2/2p5/5P2/P2P4/NPP1P1PP/BQ1BNRKR w HFhf - 2 9",
                3,
            ),
        ];
        for (fen, depth) in positions {
            check_tree(&Position::from_fen(fen).unwrap(), depth);
        }
    }

    #[test]
    fn transpositions_share_a_key() {
        let play = |moves: &[(Square, Square)]| {
            moves.iter().fold(Position::startpos(), |pos, &(from, to)| {
                pos.make_move(Move::new(from, to, MoveKind::Normal))
            })
        };
        use Square as S;
        let a = play(&[
            (S::G1, S::F3),
            (S::G8, S::F6),
            (S::B1, S::C3),
            (S::B8, S::C6),
        ]);
        let b = play(&[
            (S::B1, S::C3),
            (S::B8, S::C6),
            (S::G1, S::F3),
            (S::G8, S::F6),
        ]);
        assert_eq!(a.key(), b.key());
        // Knights out and back: the starting position again, despite the move counters.
        let back = play(&[
            (S::G1, S::F3),
            (S::G8, S::F6),
            (S::F3, S::G1),
            (S::F6, S::G8),
        ]);
        assert_eq!(back.key(), Position::startpos().key());
    }

    #[test]
    fn keys_tell_apart_side_castling_and_en_passant() {
        let key = |fen: &str| Position::from_fen(fen).unwrap().key();
        let board = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR";
        assert_ne!(
            key(&format!("{board} w KQkq -")),
            key(&format!("{board} b KQkq -"))
        );
        assert_ne!(
            key(&format!("{board} w KQkq -")),
            key(&format!("{board} w Qkq -"))
        );
        // An available en passant capture is part of the key...
        assert_ne!(
            key("rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6"),
            key("rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq -")
        );
        // ...but one nobody can make is dropped and leaves the key alone.
        assert_eq!(
            key("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3"),
            key("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq -")
        );
        // Chess960: castling with the g1 rook and with the h1 rook are different rights.
        assert_ne!(
            key("4k3/8/8/8/8/8/8/4K1RR w G -"),
            key("4k3/8/8/8/8/8/8/4K1RR w K -")
        );
    }

    #[test]
    fn all_random_numbers_differ() {
        let mut numbers: Vec<u64> = KEYS.pieces.iter().flatten().copied().collect();
        numbers.extend(KEYS.castling);
        numbers.extend(KEYS.en_passant);
        numbers.push(KEYS.black_to_move);
        let count = numbers.len();
        numbers.sort_unstable();
        numbers.dedup();
        assert_eq!(numbers.len(), count);
        assert_eq!(count, 12 * 64 + 64 + 8 + 1);
    }
}
