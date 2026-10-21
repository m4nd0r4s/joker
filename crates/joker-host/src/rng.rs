//! The deterministic generator of an app instance (ref/04 §2.7, ADR 0052):
//! xoshiro256** seeded through SplitMix64 from the instance seed. It serves
//! `random-u64` and the virtualized `wasi:random` alike.

use core::convert::Infallible;
use core::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use sha2::{Digest, Sha256};

/// The seed of an instance: the first 8 bytes (little-endian) of
/// `SHA-256("joker-seed" ‖ generation-hash ‖ app ‖ kind ‖ implements)`, with
/// `kind` the WIT case name (`cycle`, `task`) and `implements` empty when the
/// instance implements nothing.
pub fn instance_seed(
    generation_hash: &[u8; 32],
    app: &str,
    kind: &str,
    implements: Option<&str>,
) -> u64 {
    let mut h = Sha256::new();
    h.update(b"joker-seed");
    h.update(generation_hash);
    h.update(app.as_bytes());
    h.update(kind.as_bytes());
    h.update(implements.unwrap_or("").as_bytes());
    let digest = h.finalize();
    let mut first = [0u8; 8];
    first.copy_from_slice(&digest[..8]);
    u64::from_le_bytes(first)
}

/// The generator state. Only the instance's thread touches it; the atomics
/// make it shareable with the WASI context without a lock.
#[derive(Debug)]
pub struct Xoshiro {
    s: [AtomicU64; 4],
}

impl Xoshiro {
    /// A generator whose state is expanded from `seed` with SplitMix64.
    pub fn new(seed: u64) -> Self {
        let mut x = seed;
        let mut next = || {
            x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = x;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^ (z >> 31)
        };
        Self {
            s: [
                AtomicU64::new(next()),
                AtomicU64::new(next()),
                AtomicU64::new(next()),
                AtomicU64::new(next()),
            ],
        }
    }

    /// The next number.
    pub fn next_u64(&self) -> u64 {
        let mut s = [0u64; 4];
        for (v, a) in s.iter_mut().zip(&self.s) {
            *v = a.load(Ordering::Relaxed);
        }
        let result = s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        for (v, a) in s.iter().zip(&self.s) {
            a.store(*v, Ordering::Relaxed);
        }
        result
    }
}

/// A handle on a shared [`Xoshiro`] that WASI can own.
#[derive(Clone, Debug)]
pub struct SharedRng(pub Arc<Xoshiro>);

impl rand_core::TryRng for SharedRng {
    type Error = Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Infallible> {
        Ok((self.0.next_u64() >> 32) as u32)
    }

    fn try_next_u64(&mut self) -> Result<u64, Infallible> {
        Ok(self.0.next_u64())
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Infallible> {
        for chunk in dst.chunks_mut(8) {
            let word = self.0.next_u64().to_le_bytes();
            chunk.copy_from_slice(&word[..chunk.len()]);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xoshiro_matches_the_reference_sequence() {
        let g = Xoshiro {
            s: [
                AtomicU64::new(1),
                AtomicU64::new(2),
                AtomicU64::new(3),
                AtomicU64::new(4),
            ],
        };
        let got: Vec<u64> = (0..3).map(|_| g.next_u64()).collect();
        assert_eq!(got, vec![11520, 0, 1_509_978_240]);
    }

    #[test]
    fn seed_depends_on_every_input() {
        let h = [7u8; 32];
        let base = instance_seed(&h, "light", "cycle", None);
        assert_eq!(base, instance_seed(&h, "light", "cycle", None));
        assert_ne!(base, instance_seed(&h, "light", "task", None));
        assert_ne!(base, instance_seed(&h, "lux", "cycle", None));
        assert_ne!(base, instance_seed(&h, "light", "cycle", Some("pump")));
        assert_ne!(base, instance_seed(&[8u8; 32], "light", "cycle", None));
    }

    #[test]
    fn shared_handles_draw_from_one_stream() {
        let g = Arc::new(Xoshiro::new(42));
        let reference = Xoshiro::new(42);
        let mut wasi = SharedRng(g.clone());
        let a = g.next_u64();
        let b = rand_core::TryRng::try_next_u64(&mut wasi).unwrap();
        assert_eq!((a, b), (reference.next_u64(), reference.next_u64()));
    }
}
