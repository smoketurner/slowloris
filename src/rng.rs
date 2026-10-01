//! Cosmetic request jitter (varying the query string and keepalive header names
//! so connections aren't byte-for-byte identical). None of this is
//! security-sensitive.
//!
//! Random bytes come straight from the entropy source we already link —
//! `aws-lc-rs`, via rustls. The call volume is tiny (a few values per interval),
//! so there's no reason to layer a userspace PRNG on top. The clock is a
//! fallback for the (extremely unlikely) case where the entropy source fails;
//! since the output is cosmetic, a weak value there is acceptable.

/// A random `u64`, from `aws-lc-rs` with a clock-based fallback.
fn fill_u64() -> u64 {
    let mut b = [0u8; 8];
    if aws_lc_rs::rand::fill(&mut b).is_ok() {
        return u64::from_le_bytes(b);
    }
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() ^ u64::from(d.subsec_nanos()))
}

/// A value in `1..=n` (inclusive). `n` must be non-zero.
pub(crate) fn range_1(n: u32) -> u32 {
    let r = fill_u64().checked_rem(u64::from(n)).unwrap_or(0);
    // `r < n <= u32::MAX`, so the conversion is lossless and `+1` can't overflow.
    u32::try_from(r).unwrap_or(0).saturating_add(1)
}

/// An index in `0..len`. `len` must be non-zero.
pub(crate) fn index(len: usize) -> usize {
    let r = fill_u64().checked_rem(len as u64).unwrap_or(0);
    // `r < len`, so the conversion back to `usize` is lossless.
    usize::try_from(r).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_1_is_in_bounds() {
        for _ in 0..10_000 {
            let v = range_1(5000);
            assert!((1..=5000).contains(&v));
        }
    }

    #[test]
    fn index_is_in_bounds() {
        for _ in 0..10_000 {
            assert!(index(6) < 6);
        }
    }

    #[test]
    fn produces_varied_output() {
        let a = range_1(100_000);
        let mut differs = false;
        for _ in 0..20 {
            if range_1(100_000) != a {
                differs = true;
                break;
            }
        }
        assert!(differs, "RNG should not be stuck on one value");
    }
}
