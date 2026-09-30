//! A tiny non-cryptographic PRNG for cosmetic request jitter (varying the
//! query string and keepalive header names so connections aren't byte-for-byte
//! identical). None of this is security-sensitive.
//!
//! To avoid a second RNG crate, the generator is seeded once from the entropy
//! source we already link — `aws-lc-rs`, via rustls — when TLS is enabled. In
//! the `--no-default-features` (no-TLS) build, aws-lc-rs isn't present, so we
//! fall back to a time-based seed and keep that binary dependency-free.

/// splitmix64 — small, fast, good enough for jitter.
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new() -> Self {
        Rng { state: seed() }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value in `1..=n` (inclusive). `n` must be non-zero.
    pub fn range_1(&mut self, n: u32) -> u32 {
        1 + (self.next_u64() % u64::from(n)) as u32
    }

    /// An index in `0..len`. `len` must be non-zero.
    pub fn index(&mut self, len: usize) -> usize {
        (self.next_u64() % len as u64) as usize
    }
}

impl Default for Rng {
    fn default() -> Self {
        Self::new()
    }
}

fn seed() -> u64 {
    #[cfg(feature = "tls")]
    {
        let mut b = [0u8; 8];
        if aws_lc_rs::rand::fill(&mut b).is_ok() {
            return u64::from_le_bytes(b);
        }
        // Extremely unlikely; fall through to the time-based seed below.
    }

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    // Mix in a per-call counter so seeds created in the same nanosecond differ.
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    nanos ^ n.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xA5A5_A5A5_5A5A_5A5A
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_1_is_in_bounds() {
        let mut rng = Rng::new();
        for _ in 0..10_000 {
            let v = rng.range_1(5000);
            assert!((1..=5000).contains(&v));
        }
    }

    #[test]
    fn index_is_in_bounds() {
        let mut rng = Rng::new();
        for _ in 0..10_000 {
            assert!(rng.index(6) < 6);
        }
    }

    #[test]
    fn produces_varied_output() {
        let mut rng = Rng::new();
        let a = rng.range_1(100_000);
        let mut differs = false;
        for _ in 0..20 {
            if rng.range_1(100_000) != a {
                differs = true;
                break;
            }
        }
        assert!(differs, "PRNG should not be stuck on one value");
    }
}
