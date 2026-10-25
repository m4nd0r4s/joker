//! The Gorilla encoding of scalar blocks (docs/01 §12): delta-of-delta on
//! times, sequence numbers and counts, XOR on value bits, and run-length on
//! quality words.

use crate::bits::{BitReader, BitWriter};
use crate::{Aggregate, Sample};

/// Most bits one raw row can take.
pub const RAW_ROW_MAX_BITS: u64 = 69 + 78 + 33 + 69 + 66;
/// Most bits one aggregate row can take.
pub const AGG_ROW_MAX_BITS: u64 = 69 * 3 + 78 * 5 + 33;

const DOD_WIDTHS: [(u32, u32, u64); 4] = [
    (2, 10, 0b10),
    (3, 17, 0b110),
    (4, 25, 0b1110),
    (5, 33, 0b11110),
];

fn fits(value: i64, width: u32) -> bool {
    let half = 1i64 << (width - 1);
    (-half..half).contains(&value)
}

fn sign_extend(raw: u64, width: u32) -> i64 {
    let shift = 64 - width;
    ((raw << shift) as i64) >> shift
}

/// Delta-of-delta of a 64-bit integer column.
#[derive(Debug, Clone, Copy, Default)]
struct Dod {
    prev: i64,
    delta: i64,
    rows: u8,
}

impl Dod {
    fn put(&mut self, w: &mut BitWriter, value: i64) {
        if self.rows == 0 {
            w.bits(value as u64, 64);
            self.prev = value;
            self.rows = 1;
            return;
        }
        let delta = value.wrapping_sub(self.prev);
        let dod = delta.wrapping_sub(self.delta);
        if dod == 0 {
            w.bit(false);
        } else if let Some(&(tag_bits, width, tag)) =
            DOD_WIDTHS.iter().find(|(_, width, _)| fits(dod, *width))
        {
            w.bits(tag, tag_bits);
            w.bits(dod as u64 & ((1u64 << width) - 1), width);
        } else {
            w.bits(0b11111, 5);
            w.bits(dod as u64, 64);
        }
        self.prev = value;
        self.delta = delta;
        self.rows = 2;
    }

    fn get(&mut self, r: &mut BitReader<'_>) -> Option<i64> {
        if self.rows == 0 {
            let value = r.bits(64)? as i64;
            self.prev = value;
            self.rows = 1;
            return Some(value);
        }
        let mut ones = 0;
        while ones < 5 && r.bit()? {
            ones += 1;
        }
        let dod = match ones {
            0 => 0,
            5 => r.bits(64)? as i64,
            n => {
                let (_, width, _) = DOD_WIDTHS[n - 1];
                sign_extend(r.bits(width)?, width)
            }
        };
        let delta = self.delta.wrapping_add(dod);
        let value = self.prev.wrapping_add(delta);
        self.prev = value;
        self.delta = delta;
        self.rows = 2;
        Some(value)
    }
}

/// XOR of 64-bit words against the previous one (Gorilla values).
#[derive(Debug, Clone, Copy, Default)]
struct Xor {
    prev: u64,
    leading: u32,
    trailing: u32,
    started: bool,
    window: bool,
}

impl Xor {
    fn put(&mut self, w: &mut BitWriter, value: u64) {
        if !self.started {
            w.bits(value, 64);
            self.prev = value;
            self.started = true;
            return;
        }
        let x = value ^ self.prev;
        self.prev = value;
        if x == 0 {
            w.bit(false);
            return;
        }
        w.bit(true);
        let leading = x.leading_zeros().min(63);
        let trailing = x.trailing_zeros();
        if self.window && leading >= self.leading && trailing >= self.trailing {
            w.bit(false);
            let len = 64 - self.leading - self.trailing;
            w.bits(x >> self.trailing, len);
        } else {
            w.bit(true);
            let len = 64 - leading - trailing;
            w.bits(u64::from(leading), 6);
            w.bits(u64::from(len - 1), 6);
            w.bits(x >> trailing, len);
            self.leading = leading;
            self.trailing = trailing;
            self.window = true;
        }
    }

    fn get(&mut self, r: &mut BitReader<'_>) -> Option<u64> {
        if !self.started {
            let value = r.bits(64)?;
            self.prev = value;
            self.started = true;
            return Some(value);
        }
        if !r.bit()? {
            return Some(self.prev);
        }
        let x = if !r.bit()? {
            let len = 64 - self.leading - self.trailing;
            r.bits(len)? << self.trailing
        } else {
            let leading = r.bits(6)? as u32;
            let len = r.bits(6)? as u32 + 1;
            if leading + len > 64 {
                return None;
            }
            let trailing = 64 - leading - len;
            self.leading = leading;
            self.trailing = trailing;
            self.window = true;
            r.bits(len)? << trailing
        };
        self.prev ^= x;
        Some(self.prev)
    }
}

/// A 32-bit word that rarely changes.
#[derive(Debug, Clone, Copy, Default)]
struct Rle {
    prev: Option<u32>,
}

impl Rle {
    fn put(&mut self, w: &mut BitWriter, value: u32) {
        if self.prev == Some(value) {
            w.bit(false);
        } else {
            w.bit(true);
            w.bits(u64::from(value), 32);
            self.prev = Some(value);
        }
    }

    fn get(&mut self, r: &mut BitReader<'_>) -> Option<u32> {
        if r.bit()? {
            self.prev = Some(r.bits(32)? as u32);
        }
        self.prev
    }
}

/// The source timestamp, as its distance to the acquisition timestamp.
#[derive(Debug, Clone, Copy, Default)]
struct Source {
    prev: Option<Option<i64>>,
}

impl Source {
    fn put(&mut self, w: &mut BitWriter, ts: i64, src: Option<i64>) {
        let rel = src.map(|s| ts.wrapping_sub(s));
        if self.prev == Some(rel) {
            w.bit(false);
            return;
        }
        w.bit(true);
        match rel {
            None => w.bit(false),
            Some(d) => {
                w.bit(true);
                w.bits(d as u64, 64);
            }
        }
        self.prev = Some(rel);
    }

    fn get(&mut self, r: &mut BitReader<'_>, ts: i64) -> Option<Option<i64>> {
        if r.bit()? {
            let rel = if r.bit()? {
                Some(r.bits(64)? as i64)
            } else {
                None
            };
            self.prev = Some(rel);
        }
        let rel = self.prev?;
        Some(rel.map(|d| ts.wrapping_sub(d)))
    }
}

/// Encodes raw samples.
#[derive(Debug, Clone, Default)]
pub struct RawEncoder {
    w: BitWriter,
    ts: Dod,
    value: Xor,
    quality: Rle,
    seq: Dod,
    source: Source,
}

impl RawEncoder {
    /// Appends one sample.
    pub fn put(&mut self, s: &Sample) {
        self.ts.put(&mut self.w, s.ts_ns);
        self.value.put(&mut self.w, s.bits);
        self.quality.put(&mut self.w, s.quality);
        self.seq.put(&mut self.w, s.seq as i64);
        self.source.put(&mut self.w, s.ts_ns, s.ts_src_ns);
    }

    /// The encoded stream and its length in bits.
    pub fn stream(&self) -> (&[u8], u64) {
        (self.w.as_bytes(), self.w.len_bits())
    }
}

/// Decodes `rows` raw samples from a stream.
pub fn decode_raw(bytes: &[u8], bits: u64, rows: usize) -> Option<Vec<Sample>> {
    let mut r = BitReader::new(bytes, bits);
    let (mut ts, mut value, mut quality, mut seq, mut source) = (
        Dod::default(),
        Xor::default(),
        Rle::default(),
        Dod::default(),
        Source::default(),
    );
    let mut out = Vec::with_capacity(rows);
    for _ in 0..rows {
        let ts_ns = ts.get(&mut r)?;
        let bits = value.get(&mut r)?;
        let q = quality.get(&mut r)?;
        let s = seq.get(&mut r)? as u64;
        let ts_src_ns = source.get(&mut r, ts_ns)?;
        out.push(Sample {
            ts_ns,
            ts_src_ns,
            bits,
            quality: q,
            seq: s,
        });
    }
    Some(out)
}

/// Encodes aggregate rows.
#[derive(Debug, Clone, Default)]
pub struct AggEncoder {
    w: BitWriter,
    ts: Dod,
    count: Dod,
    valid: Dod,
    min: Xor,
    max: Xor,
    mean: Xor,
    first: Xor,
    last: Xor,
    quality: Rle,
}

impl AggEncoder {
    /// Appends one aggregate.
    pub fn put(&mut self, a: &Aggregate) {
        self.ts.put(&mut self.w, a.ts_ns);
        self.count.put(&mut self.w, a.count as i64);
        self.valid.put(&mut self.w, a.valid as i64);
        self.min.put(&mut self.w, a.min.to_bits());
        self.max.put(&mut self.w, a.max.to_bits());
        self.mean.put(&mut self.w, a.mean.to_bits());
        self.first.put(&mut self.w, a.first);
        self.last.put(&mut self.w, a.last);
        self.quality.put(&mut self.w, a.quality);
    }

    /// The encoded stream and its length in bits.
    pub fn stream(&self) -> (&[u8], u64) {
        (self.w.as_bytes(), self.w.len_bits())
    }
}

/// Decodes `rows` aggregates from a stream.
pub fn decode_agg(bytes: &[u8], bits: u64, rows: usize) -> Option<Vec<Aggregate>> {
    let mut r = BitReader::new(bytes, bits);
    let mut e = AggEncoder::default();
    let mut out = Vec::with_capacity(rows);
    for _ in 0..rows {
        let ts_ns = e.ts.get(&mut r)?;
        let count = e.count.get(&mut r)? as u64;
        let valid = e.valid.get(&mut r)? as u64;
        let min = f64::from_bits(e.min.get(&mut r)?);
        let max = f64::from_bits(e.max.get(&mut r)?);
        let mean = f64::from_bits(e.mean.get(&mut r)?);
        let first = e.first.get(&mut r)?;
        let last = e.last.get(&mut r)?;
        let quality = e.quality.get(&mut r)?;
        out.push(Aggregate {
            ts_ns,
            count,
            valid,
            min,
            max,
            mean,
            first,
            last,
            quality,
        });
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(i: i64) -> Sample {
        Sample {
            ts_ns: 1_700_000_000_000_000_000 + i * 1_000_000_000 + (i % 7) * 13_000,
            ts_src_ns: if i % 5 == 0 {
                None
            } else {
                Some(1_700_000_000_000_000_000 + i * 1_000_000_000 - 500)
            },
            bits: (230.0 + (i as f64 * 0.1).sin()).to_bits(),
            quality: if i % 50 == 0 { 0x0101 } else { 0 },
            seq: i as u64 + 1,
        }
    }

    #[test]
    fn raw_rows_round_trip() {
        let mut e = RawEncoder::default();
        let rows: Vec<Sample> = (0..500).map(sample).collect();
        for s in &rows {
            e.put(s);
        }
        let (bytes, bits) = e.stream();
        assert_eq!(decode_raw(bytes, bits, rows.len()).unwrap(), rows);
    }

    #[test]
    fn extreme_values_round_trip() {
        let mut e = RawEncoder::default();
        let rows = [
            Sample {
                ts_ns: i64::MIN,
                ts_src_ns: Some(i64::MAX),
                bits: u64::MAX,
                quality: u32::MAX,
                seq: u64::MAX,
            },
            Sample {
                ts_ns: i64::MAX,
                ts_src_ns: None,
                bits: 0,
                quality: 0,
                seq: 0,
            },
            Sample {
                ts_ns: 0,
                ts_src_ns: Some(i64::MIN),
                bits: 1,
                quality: 2,
                seq: 1 << 63,
            },
            Sample {
                ts_ns: 0,
                ts_src_ns: Some(i64::MIN),
                bits: 1 << 63,
                quality: 2,
                seq: 3,
            },
        ];
        for s in &rows {
            e.put(s);
        }
        let (bytes, bits) = e.stream();
        assert_eq!(decode_raw(bytes, bits, rows.len()).unwrap(), rows);
    }

    #[test]
    fn a_steady_signal_takes_a_few_bits_per_sample() {
        let mut e = RawEncoder::default();
        for i in 0..1000i64 {
            e.put(&Sample {
                ts_ns: i * 1_000_000_000,
                ts_src_ns: None,
                bits: 50.0f64.to_bits(),
                quality: 0,
                seq: i as u64,
            });
        }
        assert!(e.stream().1 / 1000 < 8);
    }

    #[test]
    fn aggregates_round_trip() {
        let mut e = AggEncoder::default();
        let rows: Vec<Aggregate> = (0..100)
            .map(|i| Aggregate {
                ts_ns: i * 60_000_000_000,
                count: 60,
                valid: 59 + (i as u64 % 2),
                min: i as f64,
                max: i as f64 + 2.5,
                mean: f64::NAN,
                first: 7,
                last: i as u64,
                quality: 0,
            })
            .collect();
        for a in &rows {
            e.put(a);
        }
        let (bytes, bits) = e.stream();
        let back = decode_agg(bytes, bits, rows.len()).unwrap();
        assert_eq!(back.len(), rows.len());
        for (a, b) in back.iter().zip(&rows) {
            assert_eq!(a.ts_ns, b.ts_ns);
            assert_eq!(a.min.to_bits(), b.min.to_bits());
            assert_eq!(a.mean.to_bits(), b.mean.to_bits());
            assert_eq!(
                (a.count, a.valid, a.first, a.last),
                (b.count, b.valid, b.first, b.last)
            );
        }
    }
}
