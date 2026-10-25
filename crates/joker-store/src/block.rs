//! Blocks: the unit a series is sealed into (docs/01 §12, ADR 0081).
//!
//! A block is a 64-byte header followed by its payload. A scalar block's
//! payload is a Gorilla stream of at most [`PAYLOAD_MAX`] bytes, so the
//! whole block fits in 4 KiB; an event block's payload is zstd over rows
//! whose uncompressed size reached [`PAYLOAD_MAX`] (a single larger event
//! takes a block of its own).

use crate::codec::{
    AGG_ROW_MAX_BITS, AggEncoder, RAW_ROW_MAX_BITS, RawEncoder, decode_agg, decode_raw,
};
use crate::crc::Crc;
use crate::frame::{self, Reader};
use crate::{Aggregate, Event, Sample, Tier};

/// The size a scalar block never exceeds.
pub const BLOCK_SIZE: usize = 4096;
/// Bytes of a block header.
pub const HEADER: usize = 64;
/// The largest payload of a scalar block.
pub const PAYLOAD_MAX: usize = BLOCK_SIZE - HEADER;
const MAGIC: [u8; 4] = *b"JKB1";

/// What a block holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    /// Raw samples.
    Raw = 0,
    /// Aggregates.
    Agg = 1,
    /// Events.
    Event = 2,
}

impl BlockKind {
    fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            0 => Self::Raw,
            1 => Self::Agg,
            2 => Self::Event,
            _ => return None,
        })
    }
}

/// The header of a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// What the payload holds.
    pub kind: BlockKind,
    /// The tier.
    pub tier: Tier,
    /// The series.
    pub series: u32,
    /// Rows in the block.
    pub rows: u32,
    /// Store sequence number of the first row (for aggregates, of the
    /// first raw sample they cover).
    pub n_first: u64,
    /// Store sequence number of the last row (for aggregates, of the last
    /// raw sample they cover).
    pub n_last: u64,
    /// Earliest row instant, UTC ns.
    pub t_min: i64,
    /// Latest row instant, UTC ns.
    pub t_max: i64,
    /// Gorilla bits, or the uncompressed size of an event payload.
    pub bits: u64,
    /// Bytes of the payload.
    pub payload_len: u32,
}

impl Header {
    /// Encodes the header and the payload, with the checksum.
    pub fn encode(&self, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER + payload.len());
        out.extend_from_slice(&MAGIC);
        out.push(self.kind as u8);
        out.push(self.tier.index() as u8);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&self.series.to_le_bytes());
        out.extend_from_slice(&self.rows.to_le_bytes());
        out.extend_from_slice(&self.n_first.to_le_bytes());
        out.extend_from_slice(&self.n_last.to_le_bytes());
        out.extend_from_slice(&self.t_min.to_le_bytes());
        out.extend_from_slice(&self.t_max.to_le_bytes());
        out.extend_from_slice(&self.bits.to_le_bytes());
        out.extend_from_slice(&self.payload_len.to_le_bytes());
        let mut crc = Crc::new();
        crc.update(&out);
        crc.update(payload);
        out.extend_from_slice(&crc.finish().to_le_bytes());
        out.extend_from_slice(payload);
        out
    }

    /// Decodes a header; returns it with the expected checksum.
    pub fn decode(bytes: &[u8]) -> Option<(Self, u32)> {
        let mut r = Reader::new(bytes.get(..HEADER)?);
        if r.take(4)? != MAGIC {
            return None;
        }
        let kind = BlockKind::from_code(r.u8()?)?;
        let tier = Tier::from_index(r.u8()?)?;
        r.u16()?;
        let header = Self {
            kind,
            tier,
            series: r.u32()?,
            rows: r.u32()?,
            n_first: r.u64()?,
            n_last: r.u64()?,
            t_min: r.i64()?,
            t_max: r.i64()?,
            bits: r.u64()?,
            payload_len: r.u32()?,
        };
        Some((header, r.u32()?))
    }

    /// Whether `crc` is the checksum of this header's bytes and `payload`.
    pub fn verify(bytes: &[u8], payload: &[u8], crc: u32) -> bool {
        let Some(head) = bytes.get(..HEADER - 4) else {
            return false;
        };
        let mut c = Crc::new();
        c.update(head);
        c.update(payload);
        c.finish() == crc
    }
}

/// The rows of an open block.
#[derive(Debug, Clone)]
pub enum Body {
    /// Raw samples, Gorilla-encoded as they arrive.
    Raw(RawEncoder),
    /// Aggregates, Gorilla-encoded as they close.
    Agg(AggEncoder),
    /// Events, uncompressed until the block is sealed.
    Event(Vec<u8>),
}

/// A block being filled.
#[derive(Debug, Clone)]
pub struct Open {
    /// The tier.
    pub tier: Tier,
    /// Rows so far.
    pub rows: u32,
    /// First store sequence number.
    pub n_first: u64,
    /// Last store sequence number.
    pub n_last: u64,
    /// Earliest instant.
    pub t_min: i64,
    /// Latest instant.
    pub t_max: i64,
    /// The rows.
    pub body: Body,
}

impl Open {
    /// An empty block of a tier.
    pub fn new(tier: Tier, body: Body) -> Self {
        Self {
            tier,
            rows: 0,
            n_first: 0,
            n_last: 0,
            t_min: i64::MAX,
            t_max: i64::MIN,
            body,
        }
    }

    /// An empty block of the same tier and kind.
    pub fn fresh(&self) -> Self {
        let body = match self.body {
            Body::Raw(_) => Body::Raw(RawEncoder::default()),
            Body::Agg(_) => Body::Agg(AggEncoder::default()),
            Body::Event(_) => Body::Event(Vec::new()),
        };
        Self::new(self.tier, body)
    }

    /// Whether one more row may not fit.
    pub fn is_full(&self) -> bool {
        match &self.body {
            Body::Raw(e) => e.stream().1 + RAW_ROW_MAX_BITS > (PAYLOAD_MAX as u64) * 8,
            Body::Agg(e) => e.stream().1 + AGG_ROW_MAX_BITS > (PAYLOAD_MAX as u64) * 8,
            Body::Event(rows) => rows.len() >= PAYLOAD_MAX,
        }
    }

    fn note(&mut self, n_first: u64, n_last: u64, ts: i64) {
        if self.rows == 0 {
            self.n_first = n_first;
        }
        self.rows += 1;
        self.n_last = n_last;
        self.t_min = self.t_min.min(ts);
        self.t_max = self.t_max.max(ts);
    }

    /// Appends a raw sample with its store sequence number.
    pub fn push_raw(&mut self, n: u64, s: &Sample) {
        if let Body::Raw(e) = &mut self.body {
            e.put(s);
            self.note(n, n, s.ts_ns);
        }
    }

    /// Appends an aggregate covering raw samples `n_first..=n_last`.
    pub fn push_agg(&mut self, n_first: u64, n_last: u64, a: &Aggregate) {
        if let Body::Agg(e) = &mut self.body {
            e.put(a);
            self.note(n_first, n_last, a.ts_ns);
        }
    }

    /// Appends an event with its store sequence number.
    pub fn push_event(&mut self, n: u64, e: &Event) {
        if let Body::Event(rows) = &mut self.body {
            encode_event(rows, e);
            self.note(n, n, e.ts_ns);
        }
    }

    /// The block as it would be sealed now: header and payload.
    pub fn seal(&self, series: u32) -> Option<Vec<u8>> {
        let (kind, bits, payload) = match &self.body {
            Body::Raw(e) => {
                let (bytes, bits) = e.stream();
                (BlockKind::Raw, bits, bytes.to_vec())
            }
            Body::Agg(e) => {
                let (bytes, bits) = e.stream();
                (BlockKind::Agg, bits, bytes.to_vec())
            }
            Body::Event(rows) => (
                BlockKind::Event,
                rows.len() as u64,
                zstd::bulk::compress(rows, 3).ok()?,
            ),
        };
        let header = Header {
            kind,
            tier: self.tier,
            series,
            rows: self.rows,
            n_first: self.n_first,
            n_last: self.n_last,
            t_min: self.t_min,
            t_max: self.t_max,
            bits,
            payload_len: u32::try_from(payload.len()).ok()?,
        };
        Some(header.encode(&payload))
    }

    /// The payload in the form a snapshot record stores it: the Gorilla
    /// stream or the uncompressed event rows, with its bit length.
    pub fn snapshot_payload(&self) -> (u64, &[u8]) {
        match &self.body {
            Body::Raw(e) => {
                let (bytes, bits) = e.stream();
                (bits, bytes)
            }
            Body::Agg(e) => {
                let (bytes, bits) = e.stream();
                (bits, bytes)
            }
            Body::Event(rows) => (rows.len() as u64, rows),
        }
    }
}

/// The decoded rows of a block.
#[derive(Debug, Clone, PartialEq)]
pub enum Rows {
    /// Raw samples with their store sequence numbers.
    Raw(Vec<(u64, Sample)>),
    /// Aggregates.
    Agg(Vec<Aggregate>),
    /// Events with their store sequence numbers.
    Event(Vec<(u64, Event)>),
}

/// Decodes the payload of a sealed block.
pub fn decode(header: &Header, payload: &[u8]) -> Option<Rows> {
    let rows = header.rows as usize;
    match header.kind {
        BlockKind::Raw => {
            let samples = decode_raw(payload, header.bits, rows)?;
            Some(Rows::Raw(number(header.n_first, samples)))
        }
        BlockKind::Agg => Some(Rows::Agg(decode_agg(payload, header.bits, rows)?)),
        BlockKind::Event => {
            let raw = zstd::bulk::decompress(payload, usize::try_from(header.bits).ok()?).ok()?;
            Some(Rows::Event(number(
                header.n_first,
                decode_events(&raw, rows)?,
            )))
        }
    }
}

/// Decodes the payload of a snapshot of an open block of `kind`.
pub fn decode_snapshot(
    kind: BlockKind,
    n_first: u64,
    rows: usize,
    bits: u64,
    payload: &[u8],
) -> Option<Rows> {
    match kind {
        BlockKind::Raw => Some(Rows::Raw(number(n_first, decode_raw(payload, bits, rows)?))),
        BlockKind::Agg => Some(Rows::Agg(decode_agg(payload, bits, rows)?)),
        BlockKind::Event => Some(Rows::Event(number(n_first, decode_events(payload, rows)?))),
    }
}

fn number<T>(n_first: u64, rows: Vec<T>) -> Vec<(u64, T)> {
    rows.into_iter()
        .enumerate()
        .map(|(i, r)| (n_first + i as u64, r))
        .collect()
}

fn encode_event(out: &mut Vec<u8>, e: &Event) {
    out.extend_from_slice(&e.ts_ns.to_le_bytes());
    frame::put_opt_i64(out, e.ts_src_ns);
    out.extend_from_slice(&e.seq.to_le_bytes());
    frame::put_bytes(out, &e.payload);
}

fn decode_events(bytes: &[u8], rows: usize) -> Option<Vec<Event>> {
    let mut r = Reader::new(bytes);
    let mut out = Vec::with_capacity(rows);
    for _ in 0..rows {
        out.push(Event {
            ts_ns: r.i64()?,
            ts_src_ns: r.opt_i64()?,
            seq: r.u64()?,
            payload: r.bytes()?.to_vec(),
        });
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_raw_block_fits_in_4_kib() {
        let mut open = Open::new(Tier::Raw, Body::Raw(RawEncoder::default()));
        let mut n = 0;
        while !open.is_full() {
            n += 1;
            open.push_raw(
                n,
                &Sample {
                    ts_ns: n as i64 * 1_000_000_007,
                    ts_src_ns: None,
                    bits: (n as f64).sqrt().to_bits(),
                    quality: 0,
                    seq: n,
                },
            );
        }
        let block = open.seal(3).unwrap();
        assert!(block.len() <= BLOCK_SIZE, "{}", block.len());
        let (header, crc) = Header::decode(&block).unwrap();
        let payload = &block[HEADER..];
        assert!(Header::verify(&block, payload, crc));
        let Rows::Raw(rows) = decode(&header, payload).unwrap() else {
            panic!("raw rows expected");
        };
        assert_eq!(rows.len() as u64, n);
        assert_eq!(rows[0].0, 1);
        assert_eq!(rows.last().unwrap().1.seq, n);
    }

    #[test]
    fn event_blocks_round_trip_through_zstd() {
        let mut open = Open::new(Tier::Raw, Body::Event(Vec::new()));
        for i in 0..20u64 {
            open.push_event(
                100 + i,
                &Event {
                    ts_ns: i as i64,
                    ts_src_ns: (i % 2 == 0).then_some(5),
                    seq: i,
                    payload: format!("{{\"kind\":\"transition\",\"i\":{i}}}").into_bytes(),
                },
            );
        }
        let block = open.seal(1).unwrap();
        let (header, _) = Header::decode(&block).unwrap();
        let Rows::Event(rows) = decode(&header, &block[HEADER..]).unwrap() else {
            panic!("event rows expected");
        };
        assert_eq!(rows.len(), 20);
        assert_eq!(rows[19].0, 119);
        assert_eq!(rows[4].1.ts_src_ns, Some(5));
    }
}
