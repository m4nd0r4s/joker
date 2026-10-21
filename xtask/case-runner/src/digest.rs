//! The digest of the per-tick result sequence of a case: the bytes the
//! determinism gate compares between targets and runs (docs/16 §7, ref/03
//! §2.6.4, docs/11 Phase 0 gate 2).

use joker_core::Applied;

use crate::exec::OutputTick;

const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64 over, per tick and per output, the tick index (`u64`), the
/// output index (`u32`), the result code and detail, the applied value (`0`
/// and the `u64` bits, or `1` for relinquish), the level (`0` for none) and
/// the trip flag, all little-endian. Fed one tick at a time, so a target
/// needs no storage for the timeline.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SequenceDigest {
    hash: u64,
}

impl Default for SequenceDigest {
    fn default() -> Self {
        Self::new()
    }
}

impl SequenceDigest {
    /// The digest of an empty sequence.
    pub const fn new() -> Self {
        Self { hash: OFFSET }
    }

    fn feed(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.hash ^= u64::from(*b);
            self.hash = self.hash.wrapping_mul(PRIME);
        }
    }

    /// Adds the results of one tick, in the order of the outputs of the
    /// fixture.
    pub fn tick(&mut self, tick: u64, outputs: &[OutputTick]) {
        for (o, out) in outputs.iter().enumerate() {
            let r = &out.result;
            self.feed(&tick.to_le_bytes());
            self.feed(&(o as u32).to_le_bytes());
            self.feed(&[r.outcome.code(), r.outcome.detail()]);
            match r.applied {
                Applied::Value(bits) => {
                    self.feed(&[0]);
                    self.feed(&bits.to_le_bytes());
                }
                Applied::Relinquish => self.feed(&[1]),
            }
            self.feed(&[r.level.map_or(0, |l| l.get()), u8::from(r.tripped)]);
        }
    }

    /// The digest so far.
    pub const fn value(&self) -> u64 {
        self.hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_sequence_is_the_fnv_offset() {
        assert_eq!(SequenceDigest::new().value(), OFFSET);
        let mut d = SequenceDigest::new();
        d.tick(0, &[]);
        assert_eq!(d.value(), OFFSET);
    }
}
