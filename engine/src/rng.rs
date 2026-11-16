//! A small pseudo-random number generator with a fixed seed.
//!
//! The engine needs numbers that look random in a few places (magic numbers,
//! hash keys), but they must come out the same on every run so that the
//! engine's behaviour can be reproduced. The algorithm is SplitMix64: a 64-bit
//! counter advanced by a fixed odd constant, then scrambled by two
//! multiply-xorshift rounds. It is fast and statistically good enough for these
//! uses; it is not meant for cryptography.

/// SplitMix64 generator. The same seed always gives the same sequence.
#[derive(Clone, Debug)]
pub(crate) struct Rng {
    state: u64,
}

impl Rng {
    /// Creates a generator. Any seed works, zero included.
    pub(crate) const fn new(seed: u64) -> Rng {
        Rng { state: seed }
    }

    /// The next number of the sequence.
    pub(crate) const fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number with only about one bit in eight set: three numbers ANDed
    /// together. Magic number candidates with few bits set succeed far more often.
    pub(crate) const fn next_sparse_u64(&mut self) -> u64 {
        self.next_u64() & self.next_u64() & self.next_u64()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_known_splitmix64_output() {
        // The first outputs for seed 0. A typo in one of the constants would change them.
        let mut rng = Rng::new(0);
        assert_eq!(rng.next_u64(), 0xE220_A839_7B1D_CDAF);
        assert_eq!(rng.next_u64(), 0x6E78_9E6A_A1B9_65F4);
        assert_eq!(rng.next_u64(), 0x06C4_5D18_8009_454F);
    }

    #[test]
    fn same_seed_gives_same_sequence() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        assert_ne!(Rng::new(42).next_u64(), Rng::new(43).next_u64());
    }

    #[test]
    fn sparse_numbers_have_few_bits() {
        let mut rng = Rng::new(7);
        let bits: u32 = (0..1000).map(|_| rng.next_sparse_u64().count_ones()).sum();
        // About 64 / 8 = 8 bits per number on average.
        assert!(
            (6000..10_000).contains(&bits),
            "{bits} bits in 1000 numbers"
        );
    }
}
