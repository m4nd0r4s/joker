//! Aggregation: the buckets of the `1s`, `1m` and `15m` tiers, and the
//! same arithmetic for queries with a `step` over raw samples (ref/09 §5.5).

use crate::frame::Reader;
use crate::{Aggregate, Sample, ValueKind};

/// The quality code of a quality word (ref/00 §4.3): `good` 0,
/// `uncertain` 1, `bad` 2; an unknown code reads as `bad`.
pub fn quality_code(quality: u32) -> u32 {
    match quality & 0xFF {
        0 => 0,
        1 => 1,
        _ => 2,
    }
}

/// An aggregate being built.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bucket {
    /// Start of the interval, UTC ns.
    pub start: i64,
    /// Samples.
    pub count: u64,
    /// Samples that were not `Bad`.
    pub valid: u64,
    /// Smallest valid value.
    pub min: f64,
    /// Largest valid value.
    pub max: f64,
    /// Sum of the valid values.
    pub sum: f64,
    /// Bits of the first valid sample.
    pub first: u64,
    /// Bits of the last valid sample.
    pub last: u64,
    /// Samples per quality code.
    pub qualities: [u64; 3],
    /// Store sequence number of the first sample.
    pub n_first: u64,
    /// Store sequence number of the last sample.
    pub n_last: u64,
}

impl Bucket {
    /// An empty bucket starting at `start`.
    pub fn new(start: i64) -> Self {
        Self {
            start,
            count: 0,
            valid: 0,
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
            sum: 0.0,
            first: 0,
            last: 0,
            qualities: [0; 3],
            n_first: 0,
            n_last: 0,
        }
    }

    /// Adds a sample; a `Bad` one only counts (AGENTS.md invariant 4).
    pub fn add(&mut self, kind: ValueKind, n: u64, s: &Sample) {
        if self.count == 0 {
            self.n_first = n;
        }
        self.count += 1;
        self.n_last = n;
        let code = quality_code(s.quality);
        self.qualities[code as usize] += 1;
        if code == 2 {
            return;
        }
        if self.valid == 0 {
            self.first = s.bits;
        }
        self.valid += 1;
        self.last = s.bits;
        if let Some(v) = kind.number(s.bits) {
            self.min = self.min.min(v);
            self.max = self.max.max(v);
            self.sum += v;
        }
    }

    /// The finished aggregate.
    pub fn finish(&self, kind: ValueKind) -> Aggregate {
        let numeric = kind != ValueKind::Other && self.valid > 0;
        let quality = (0..3u32)
            .max_by_key(|&c| self.qualities[c as usize])
            .unwrap_or(2);
        Aggregate {
            ts_ns: self.start,
            count: self.count,
            valid: self.valid,
            min: if numeric { self.min } else { f64::NAN },
            max: if numeric { self.max } else { f64::NAN },
            mean: if numeric {
                self.sum / self.valid as f64
            } else {
                f64::NAN
            },
            first: self.first,
            last: self.last,
            quality,
        }
    }

    /// Encodes the bucket for a snapshot record.
    pub fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.start.to_le_bytes());
        out.extend_from_slice(&self.count.to_le_bytes());
        out.extend_from_slice(&self.valid.to_le_bytes());
        out.extend_from_slice(&self.min.to_bits().to_le_bytes());
        out.extend_from_slice(&self.max.to_bits().to_le_bytes());
        out.extend_from_slice(&self.sum.to_bits().to_le_bytes());
        out.extend_from_slice(&self.first.to_le_bytes());
        out.extend_from_slice(&self.last.to_le_bytes());
        for q in self.qualities {
            out.extend_from_slice(&q.to_le_bytes());
        }
        out.extend_from_slice(&self.n_first.to_le_bytes());
        out.extend_from_slice(&self.n_last.to_le_bytes());
    }

    /// Decodes a bucket written by [`Bucket::encode`].
    pub fn decode(r: &mut Reader<'_>) -> Option<Self> {
        Some(Self {
            start: r.i64()?,
            count: r.u64()?,
            valid: r.u64()?,
            min: f64::from_bits(r.u64()?),
            max: f64::from_bits(r.u64()?),
            sum: f64::from_bits(r.u64()?),
            first: r.u64()?,
            last: r.u64()?,
            qualities: [r.u64()?, r.u64()?, r.u64()?],
            n_first: r.u64()?,
            n_last: r.u64()?,
        })
    }
}

/// The start of the interval of `resolution_ns` that holds `ts`.
pub fn bucket_start(ts: i64, resolution_ns: u64) -> i64 {
    let r = i64::try_from(resolution_ns).unwrap_or(i64::MAX).max(1);
    ts.div_euclid(r) * r
}

/// Aggregates raw samples into intervals of `step_ns`, aligned on UTC
/// zero, in the order of the samples (ref/09 §5.5 with `step`).
pub fn aggregate(kind: ValueKind, samples: &[(u64, Sample)], step_ns: u64) -> Vec<Aggregate> {
    let mut out = Vec::new();
    let mut bucket: Option<Bucket> = None;
    for (n, s) in samples {
        let start = bucket_start(s.ts_ns, step_ns);
        match &mut bucket {
            Some(b) if b.start == start => b.add(kind, *n, s),
            _ => {
                if let Some(b) = bucket.take() {
                    out.push(b.finish(kind));
                }
                let mut b = Bucket::new(start);
                b.add(kind, *n, s);
                bucket = Some(b);
            }
        }
    }
    if let Some(b) = bucket {
        out.push(b.finish(kind));
    }
    out
}

/// Re-aggregates aggregates of a finer tier into intervals of `step_ns`.
pub fn reaggregate(aggs: &[Aggregate], step_ns: u64) -> Vec<Aggregate> {
    let mut out: Vec<Aggregate> = Vec::new();
    let mut weights: Vec<[u64; 3]> = Vec::new();
    for a in aggs {
        let start = bucket_start(a.ts_ns, step_ns);
        let merge = out.last().is_some_and(|o| o.ts_ns == start);
        if !merge {
            out.push(Aggregate {
                ts_ns: start,
                count: 0,
                valid: 0,
                min: a.min,
                max: a.max,
                mean: 0.0,
                first: a.first,
                last: a.last,
                quality: a.quality,
            });
            weights.push([0; 3]);
        }
        let (Some(o), Some(w)) = (out.last_mut(), weights.last_mut()) else {
            continue;
        };
        if a.valid > 0 {
            if o.valid == 0 {
                o.first = a.first;
                o.min = a.min;
                o.max = a.max;
                o.mean = 0.0;
            } else {
                o.min = o.min.min(a.min);
                o.max = o.max.max(a.max);
            }
            let total = (o.valid + a.valid) as f64;
            o.mean = (o.mean * o.valid as f64 + a.mean * a.valid as f64) / total;
            o.last = a.last;
        }
        o.count += a.count;
        o.valid += a.valid;
        w[(a.quality.min(2)) as usize] += a.count;
        o.quality = (0..3u32).max_by_key(|&c| w[c as usize]).unwrap_or(2);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(ts: i64, v: f64, quality: u32) -> Sample {
        Sample {
            ts_ns: ts,
            ts_src_ns: None,
            bits: v.to_bits(),
            quality,
            seq: 0,
        }
    }

    #[test]
    fn bad_samples_count_but_never_become_numbers() {
        let rows = vec![
            (1, s(0, 1.0, 0)),
            (2, s(100, 100.0, 0x0302)),
            (3, s(200, 3.0, 0x0101)),
        ];
        let a = aggregate(ValueKind::F64, &rows, 1_000);
        assert_eq!(a.len(), 1);
        assert_eq!((a[0].count, a[0].valid), (3, 2));
        assert_eq!((a[0].min, a[0].max, a[0].mean), (1.0, 3.0, 2.0));
        assert_eq!(f64::from_bits(a[0].last), 3.0);
    }

    #[test]
    fn the_dominant_quality_is_the_most_frequent_and_the_worse_on_a_tie() {
        let rows = vec![
            (1, s(0, 1.0, 0)),
            (2, s(1, 1.0, 1)),
            (3, s(2, 1.0, 2)),
            (4, s(3, 1.0, 2)),
        ];
        assert_eq!(aggregate(ValueKind::F64, &rows, 10)[0].quality, 2);
        let rows = vec![(1, s(0, 1.0, 0)), (2, s(1, 1.0, 1))];
        assert_eq!(aggregate(ValueKind::F64, &rows, 10)[0].quality, 1);
    }

    #[test]
    fn intervals_are_aligned_on_utc_zero() {
        let rows = vec![
            (1, s(-1, 1.0, 0)),
            (2, s(0, 2.0, 0)),
            (3, s(999, 3.0, 0)),
            (4, s(1000, 4.0, 0)),
        ];
        let a = aggregate(ValueKind::F64, &rows, 1000);
        assert_eq!(
            a.iter().map(|a| a.ts_ns).collect::<Vec<_>>(),
            vec![-1000, 0, 1000]
        );
    }

    #[test]
    fn reaggregation_weights_the_means() {
        let rows: Vec<(u64, Sample)> = (0..120)
            .map(|i| (i, s(i as i64 * 1000, i as f64, 0)))
            .collect();
        let fine = aggregate(ValueKind::F64, &rows, 10_000);
        let coarse = reaggregate(&fine, 60_000);
        let direct = aggregate(ValueKind::F64, &rows, 60_000);
        assert_eq!(coarse.len(), direct.len());
        for (a, b) in coarse.iter().zip(&direct) {
            assert_eq!(
                (a.ts_ns, a.count, a.min, a.max),
                (b.ts_ns, b.count, b.min, b.max)
            );
            assert!((a.mean - b.mean).abs() < 1e-9);
        }
    }
}
