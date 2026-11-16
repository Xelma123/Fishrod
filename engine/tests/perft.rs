//! Perft suite: leaf counts of the legal move tree for well-known positions.
//!
//! Sources:
//! - Standard positions: Chess Programming Wiki, "Perft Results"
//!   (<https://www.chessprogramming.org/Perft_Results>).
//! - Chess960 positions: Chess Programming Wiki, "Chess960 Perft Results"
//!   (<https://www.chessprogramming.org/Chess960_Perft_Results>), the first 20
//!   positions of the set Andrew Grant made for the Ethereal engine.
//!
//! Depths with at most `QUICK_LIMIT` leaf nodes run with every `cargo test`.
//! The deeper ones are ignored by default; run them with
//! `cargo test --release -- --ignored`.

use fishrod::board::Position;
use fishrod::movegen::perft::perft;

/// Positions with their perft counts for depth 1, 2, 3, ...
type Suite = [(&'static str, &'static [u64])];

const QUICK_LIMIT: u64 = 5_000_000;

const STANDARD: &Suite = &[
    (
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        &[20, 400, 8_902, 197_281, 4_865_609, 119_060_324],
    ),
    // "Kiwipete"
    (
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        &[48, 2_039, 97_862, 4_085_603, 193_690_690],
    ),
    (
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        &[14, 191, 2_812, 43_238, 674_624, 11_030_083, 178_633_661],
    ),
    (
        "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
        &[6, 264, 9_467, 422_333, 15_833_292],
    ),
    // The same position with colors reversed.
    (
        "r2q1rk1/pP1p2pp/Q4n2/bbp1p3/Np6/1B3NBn/pPPP1PPP/R3K2R b KQ - 0 1",
        &[6, 264, 9_467, 422_333, 15_833_292],
    ),
    (
        "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
        &[44, 1_486, 62_379, 2_103_487, 89_941_194],
    ),
    (
        "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
        &[46, 2_079, 89_890, 3_894_594, 164_075_551],
    ),
];

const CHESS960: &Suite = &[
    (
        "bqnb1rkr/pp3ppp/3ppn2/2p5/5P2/P2P4/NPP1P1PP/BQ1BNRKR w HFhf - 2 9",
        &[21, 528, 12_189, 326_672, 8_146_062, 227_689_589],
    ),
    (
        "2nnrbkr/p1qppppp/8/1ppb4/6PP/3PP3/PPP2P2/BQNNRBKR w HEhe - 1 9",
        &[21, 807, 18_002, 667_366, 16_253_601, 590_751_109],
    ),
    (
        "b1q1rrkb/pppppppp/3nn3/8/P7/1PPP4/4PPPP/BQNNRKRB w GE - 1 9",
        &[20, 479, 10_471, 273_318, 6_417_013, 177_654_692],
    ),
    (
        "qbbnnrkr/2pp2pp/p7/1p2pp2/8/P3PP2/1PPP1KPP/QBBNNR1R w hf - 0 9",
        &[22, 593, 13_440, 382_958, 9_183_776, 274_103_539],
    ),
    (
        "1nbbnrkr/p1p1ppp1/3p4/1p3P1p/3Pq2P/8/PPP1P1P1/QNBBNRKR w HFhf - 0 9",
        &[28, 1_120, 31_058, 1_171_749, 34_030_312, 1_250_970_898],
    ),
    (
        "qnbnr1kr/ppp1b1pp/4p3/3p1p2/8/2NPP3/PPP1BPPP/QNB1R1KR w HEhe - 1 9",
        &[29, 899, 26_578, 824_055, 24_851_983, 775_718_317],
    ),
    (
        "q1bnrkr1/ppppp2p/2n2p2/4b1p1/2NP4/8/PPP1PPPP/QNB1RRKB w ge - 1 9",
        &[30, 860, 24_566, 732_757, 21_093_346, 649_209_803],
    ),
    (
        "qbn1brkr/ppp1p1p1/2n4p/3p1p2/P7/6PP/QPPPPP2/1BNNBRKR w HFhf - 0 9",
        &[25, 635, 17_054, 465_806, 13_203_304, 377_184_252],
    ),
    (
        "qnnbbrkr/1p2ppp1/2pp3p/p7/1P5P/2NP4/P1P1PPP1/Q1NBBRKR w HFhf - 0 9",
        &[24, 572, 15_243, 384_260, 11_110_203, 293_989_890],
    ),
    (
        "qn1rbbkr/ppp2p1p/1n1pp1p1/8/3P4/P6P/1PP1PPPK/QNNRBB1R w hd - 2 9",
        &[28, 811, 23_175, 679_699, 19_836_606, 594_527_992],
    ),
    (
        "qnr1bkrb/pppp2pp/3np3/5p2/8/P2P2P1/NPP1PP1P/QN1RBKRB w GDg - 3 9",
        &[33, 823, 26_895, 713_420, 23_114_629, 646_390_782],
    ),
    (
        "qb1nrkbr/1pppp1p1/1n3p2/p1B4p/8/3P1P1P/PPP1P1P1/QBNNRK1R w HEhe - 0 9",
        &[31, 855, 25_620, 735_703, 21_796_206, 651_054_626],
    ),
    (
        "qnnbrk1r/1p1ppbpp/2p5/p4p2/2NP3P/8/PPP1PPP1/Q1NBRKBR w HEhe - 0 9",
        &[26, 790, 21_238, 642_367, 17_819_770, 544_866_674],
    ),
    (
        "1qnrkbbr/1pppppp1/p1n4p/8/P7/1P1N1P2/2PPP1PP/QN1RKBBR w HDhd - 0 9",
        &[37, 883, 32_187, 815_535, 29_370_838, 783_201_510],
    ),
    (
        "qn1rkrbb/pp1p1ppp/2p1p3/3n4/4P2P/2NP4/PPP2PP1/Q1NRKRBB w FDfd - 1 9",
        &[24, 585, 14_769, 356_950, 9_482_310, 233_468_620],
    ),
    (
        "bb1qnrkr/pp1p1pp1/1np1p3/4N2p/8/1P4P1/P1PPPP1P/BBNQ1RKR w HFhf - 0 9",
        &[29, 864, 25_747, 799_727, 24_219_627, 776_836_316],
    ),
    (
        "bnqbnr1r/p1p1ppkp/3p4/1p4p1/P7/3NP2P/1PPP1PP1/BNQB1RKR w HF - 0 9",
        &[26, 889, 24_353, 832_956, 23_701_014, 809_194_268],
    ),
    (
        "bnqnrbkr/1pp2pp1/p7/3pP2p/4P1P1/8/PPPP3P/BNQNRBKR w HEhe d6 0 9",
        &[31, 984, 28_677, 962_591, 29_032_175, 1_008_880_643],
    ),
    (
        "b1qnrrkb/ppp1pp1p/n2p1Pp1/8/8/P7/1PPPP1PP/BNQNRKRB w GE - 0 9",
        &[20, 484, 10_532, 281_606, 6_718_715, 193_594_729],
    ),
    (
        "n1bqnrkr/pp1ppp1p/2p5/6p1/2P2b2/PN6/1PNPPPPP/1BBQ1RKR w HFhf - 2 9",
        &[23, 732, 17_746, 558_191, 14_481_581, 457_140_569],
    ),
];

/// Checks every depth of every position whose expected count `selected` accepts.
fn check(suite: &Suite, selected: impl Fn(u64) -> bool) {
    for &(fen, counts) in suite {
        let pos = Position::from_fen(fen).unwrap_or_else(|err| panic!("{fen}: {err}"));
        for (depth, &expected) in (1..).zip(counts) {
            if selected(expected) {
                assert_eq!(perft(&pos, depth), expected, "{fen} at depth {depth}");
            }
        }
    }
}

#[test]
fn standard_quick() {
    check(STANDARD, |nodes| nodes <= QUICK_LIMIT);
}

#[test]
#[ignore = "slow; run with cargo test --release -- --ignored"]
fn standard_deep() {
    check(STANDARD, |nodes| nodes > QUICK_LIMIT);
}

#[test]
fn chess960_quick() {
    check(CHESS960, |nodes| nodes <= QUICK_LIMIT);
}

#[test]
#[ignore = "slow; run with cargo test --release -- --ignored"]
fn chess960_deep() {
    check(CHESS960, |nodes| nodes > QUICK_LIMIT);
}
