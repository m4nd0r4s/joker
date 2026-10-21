//! The `.jktrace` v1 format (docs/16 §7, ADR 0037): a header, an optional
//! state segment and one segment of ticks per group and recording window.
//!
//! Writing never allocates: segments are built in caller-provided buffers and
//! are uncompressed (header flag bit 0), as on an MCU. The zstd segments
//! that Linux writes are compressed and decompressed by `jokerd` (docs/16
//! §7), so the core has no zstd. Reading is zero-copy over the file bytes.

use crate::commit::OutputResult;
use crate::image::{MAX_METHOD_ARGS, MethodCall, Proposal, Source, WriterId};
use crate::quality::Quality;
use crate::registry::SignalId;
use crate::time::Instant;
use crate::value::Sample;

/// File magic.
pub const MAGIC: [u8; 4] = *b"JKTR";
/// Format version.
pub const VERSION: u16 = 1;
/// Header flag: segments are not compressed (ADR 0037).
pub const FLAG_UNCOMPRESSED: u32 = 1;
/// Header flag: a state segment follows the header.
pub const FLAG_STATE: u32 = 1 << 1;
/// Group number that marks the state segment.
pub const STATE_MARKER: u32 = 0xFFFF_FFFF;
/// Bytes in the file header.
pub const HEADER_LEN: usize = 4 + 2 + 4 + 32 + 32 + 8 + 8 + 1;
/// Bytes in a segment header.
pub const SEGMENT_HEADER_LEN: usize = 12;
/// Bytes in one input entry.
pub const INPUT_LEN: usize = 16;
/// Bytes in one proposal entry.
pub const PROPOSAL_LEN: usize = 18;
/// Bytes in one result entry.
pub const RESULT_LEN: usize = 13;
/// Bytes of the fixed part of a tick.
pub const TICK_FIXED_LEN: usize = 8 + 8 + 8 + 1;
/// Proposal flag bit 8: staged from the async inbox (ADR 0037).
pub const PROPOSAL_FLAG_ASYNC: u16 = 1 << 8;

/// Why a trace could not be written or read.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TraceError {
    /// The output buffer is too small; nothing was written.
    BufferFull,
    /// The input ends in the middle of a structure.
    Truncated,
    /// Not a `.jktrace` file.
    BadMagic,
    /// A version this reader does not know.
    UnsupportedVersion(u16),
    /// A header flag this reader does not know.
    UnknownFlags(u32),
    /// A segment's tick count does not match its bytes.
    Malformed,
    /// A segment of a group whose port count is unknown.
    UnknownGroup(u32),
    /// A compressed segment, which the core does not decompress.
    Compressed,
    /// A count does not fit its field.
    TooMany,
}

/// The file header (docs/16 §7).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Header {
    /// Header flags.
    pub flags: u32,
    /// Hash of the generation.
    pub generation_hash: [u8; 32],
    /// Hash of the registry (ADR 0031).
    pub registry_hash: [u8; 32],
    /// Monotonic epoch of the recording, in ns.
    pub epoch_mono_ns: u64,
    /// UTC epoch of the recording, in ns.
    pub epoch_utc_ns: i64,
    /// Clock quality at the epoch (ref/00 §6).
    pub clock: u8,
}

impl Header {
    /// Encodes the header.
    pub fn encode(&self) -> [u8; HEADER_LEN] {
        let mut out = [0u8; HEADER_LEN];
        let mut w = Cursor::new(&mut out);
        w.bytes(&MAGIC);
        w.u16(VERSION);
        w.u32(self.flags);
        w.bytes(&self.generation_hash);
        w.bytes(&self.registry_hash);
        w.u64(self.epoch_mono_ns);
        w.u64(self.epoch_utc_ns as u64);
        w.u8(self.clock);
        out
    }

    /// Decodes a header and returns it with the remaining bytes.
    pub fn decode(bytes: &[u8]) -> Result<(Self, &[u8]), TraceError> {
        let mut r = Reader::new(bytes);
        if r.take(4)? != MAGIC {
            return Err(TraceError::BadMagic);
        }
        let version = r.u16()?;
        if version != VERSION {
            return Err(TraceError::UnsupportedVersion(version));
        }
        let flags = r.u32()?;
        if flags & !(FLAG_UNCOMPRESSED | FLAG_STATE) != 0 {
            return Err(TraceError::UnknownFlags(flags));
        }
        let header = Self {
            flags,
            generation_hash: r.array()?,
            registry_hash: r.array()?,
            epoch_mono_ns: r.u64()?,
            epoch_utc_ns: r.u64()? as i64,
            clock: r.u8()?,
        };
        Ok((header, r.rest()))
    }

    /// True when segments are stored uncompressed.
    pub const fn uncompressed(&self) -> bool {
        self.flags & FLAG_UNCOMPRESSED != 0
    }
}

/// One output result of a tick, as the trace stores it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TraceResult {
    /// The output.
    pub signal: SignalId,
    /// Outcome code (ref/00 §4.4).
    pub outcome: u8,
    /// Applied value bits; 0 for `relinquish` (ADR 0037).
    pub applied: u64,
}

impl From<&OutputResult> for TraceResult {
    fn from(r: &OutputResult) -> Self {
        Self {
            signal: r.signal,
            outcome: r.outcome.code(),
            applied: r.applied.bits(),
        }
    }
}

/// The trace flags of a staged proposal (ADR 0037).
pub const fn proposal_flags(p: &Proposal) -> u16 {
    (p.flags & 0xFF)
        | match p.source {
            Source::Async => PROPOSAL_FLAG_ASYNC,
            Source::Cycle => 0,
        }
}

/// What one tick contributes to the trace.
#[derive(Copy, Clone, Debug)]
pub struct TickRecord<'a> {
    /// Tick index (ADR 0032).
    pub index: u64,
    /// Latch instant.
    pub at: Instant,
    /// The latched samples, one per port, in `SignalId` order.
    pub inputs: &'a [Sample],
    /// The staged proposals, in staging order.
    pub proposals: &'a [Proposal],
    /// The output results, in `SignalId` order.
    pub results: &'a [TraceResult],
    /// The method records, in staging order (ADR 0043).
    pub methods: &'a [MethodCall],
}

impl TickRecord<'_> {
    /// Encoded length of this tick, with an empty ML section.
    pub fn encoded_len(&self) -> usize {
        TICK_FIXED_LEN
            + self.inputs.len() * INPUT_LEN
            + 2
            + self.proposals.len() * PROPOSAL_LEN
            + 2
            + self.results.len() * RESULT_LEN
            + 2
            + 2
            + self
                .methods
                .iter()
                .map(|m| METHOD_FIXED_LEN + args_len(m.args()))
                .sum::<usize>()
    }
}

/// Bytes of a method record before its arguments.
pub const METHOD_FIXED_LEN: usize = 4 + 4 + 2 + 2;

fn uint_len(v: u64) -> usize {
    match v {
        0..=23 => 1,
        24..=0xFF => 2,
        0x100..=0xFFFF => 3,
        0x1_0000..=0xFFFF_FFFF => 5,
        _ => 9,
    }
}

fn args_len(args: &[u64]) -> usize {
    1 + args.iter().map(|&v| 1 + uint_len(v)).sum::<usize>()
}

fn write_uint(w: &mut Cursor<'_>, major: u8, v: u64) {
    let m = major << 5;
    match v {
        0..=23 => w.u8(m | v as u8),
        24..=0xFF => {
            w.u8(m | 24);
            w.u8(v as u8);
        }
        0x100..=0xFFFF => {
            w.u8(m | 25);
            w.bytes(&(v as u16).to_be_bytes());
        }
        0x1_0000..=0xFFFF_FFFF => {
            w.u8(m | 26);
            w.bytes(&(v as u32).to_be_bytes());
        }
        _ => {
            w.u8(m | 27);
            w.bytes(&v.to_be_bytes());
        }
    }
}

fn read_uint(r: &mut Reader<'_>, major: u8) -> Result<u64, TraceError> {
    let head = r.u8()?;
    if head >> 5 != major {
        return Err(TraceError::Malformed);
    }
    let info = head & 0x1F;
    let v = match info {
        0..=23 => u64::from(info),
        24 => u64::from(r.u8()?),
        25 => u64::from(u16::from_be_bytes(r.array()?)),
        26 => u64::from(u32::from_be_bytes(r.array()?)),
        27 => u64::from_be_bytes(r.array()?),
        _ => return Err(TraceError::Malformed),
    };
    let minimal = match info {
        24 => v >= 24,
        25 => v > 0xFF,
        26 => v > 0xFFFF,
        27 => v > 0xFFFF_FFFF,
        _ => true,
    };
    if minimal {
        Ok(v)
    } else {
        Err(TraceError::Malformed)
    }
}

/// Builds one uncompressed segment of ticks in a caller-provided buffer.
#[derive(Debug)]
pub struct SegmentWriter<'b> {
    buf: &'b mut [u8],
    len: usize,
    n_ticks: u32,
}

impl<'b> SegmentWriter<'b> {
    /// Starts a segment of `group` in `buf`.
    pub fn new(group: u32, buf: &'b mut [u8]) -> Result<Self, TraceError> {
        let head = buf
            .get_mut(..SEGMENT_HEADER_LEN)
            .ok_or(TraceError::BufferFull)?;
        head[..4].copy_from_slice(&group.to_le_bytes());
        head[4..].fill(0);
        Ok(Self {
            buf,
            len: SEGMENT_HEADER_LEN,
            n_ticks: 0,
        })
    }

    /// Appends a tick, or writes nothing and returns `BufferFull`.
    pub fn push(&mut self, tick: &TickRecord<'_>) -> Result<(), TraceError> {
        if tick.proposals.len() > usize::from(u16::MAX)
            || tick.results.len() > usize::from(u16::MAX)
            || tick.methods.len() > usize::from(u16::MAX)
        {
            return Err(TraceError::TooMany);
        }
        let end = self
            .len
            .checked_add(tick.encoded_len())
            .filter(|&end| end <= self.buf.len())
            .ok_or(TraceError::BufferFull)?;
        let mut w = Cursor::new(&mut self.buf[self.len..end]);
        w.u64(tick.index);
        w.u64(tick.at.mono_ns);
        w.u64(tick.at.utc_ns as u64);
        w.u8(tick.at.clock as u8);
        for s in tick.inputs {
            w.u32(s.seq() as u32);
            w.u32(s.quality().to_bits());
            w.u64(s.transport_bits());
        }
        w.u16(tick.proposals.len() as u16);
        for p in tick.proposals {
            w.u32(p.writer.0);
            w.u32(p.signal.0);
            w.u64(p.value);
            w.u16(proposal_flags(p));
        }
        w.u16(tick.results.len() as u16);
        for r in tick.results {
            w.u32(r.signal.0);
            w.u8(r.outcome);
            w.u64(r.applied);
        }
        w.u16(0);
        w.u16(tick.methods.len() as u16);
        for m in tick.methods {
            let args = m.args();
            w.u32(m.writer.0);
            w.u32(m.instance);
            w.u16(m.method);
            w.u16(args_len(args) as u16);
            write_uint(&mut w, 5, args.len() as u64);
            for (k, &v) in args.iter().enumerate() {
                write_uint(&mut w, 0, k as u64);
                write_uint(&mut w, 0, v);
            }
        }
        self.len = end;
        self.n_ticks += 1;
        Ok(())
    }

    /// Ticks written so far.
    pub fn ticks(&self) -> u32 {
        self.n_ticks
    }

    /// The raw tick bytes, without the segment header (the input of
    /// compression).
    pub fn payload(&self) -> &[u8] {
        &self.buf[SEGMENT_HEADER_LEN..self.len]
    }

    /// Closes the segment and returns its bytes, header included.
    pub fn finish(self) -> &'b [u8] {
        let payload = (self.len - SEGMENT_HEADER_LEN) as u32;
        self.buf[4..8].copy_from_slice(&self.n_ticks.to_le_bytes());
        self.buf[8..12].copy_from_slice(&payload.to_le_bytes());
        &self.buf[..self.len]
    }
}

/// The state of an app at the start of a recording (docs/16 §7).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct AppState<'a> {
    /// App name.
    pub name: &'a str,
    /// Group of its cycle instance.
    pub group: u32,
    /// `seq` of the first recorded tick of that group.
    pub first_seq: u64,
    /// 0 exported, 1 no `persist`, 2 failed.
    pub state: u8,
    /// The `joker-state` CBOR blob (ref/04 §5).
    pub blob: &'a [u8],
}

impl AppState<'_> {
    /// Encoded length.
    pub fn encoded_len(&self) -> usize {
        1 + self.name.len() + 4 + 8 + 1 + 4 + self.blob.len()
    }
}

/// Builds an uncompressed state segment in a caller-provided buffer.
pub fn write_state_segment<'b>(
    apps: &[AppState<'_>],
    buf: &'b mut [u8],
) -> Result<&'b [u8], TraceError> {
    let mut len = SEGMENT_HEADER_LEN;
    for app in apps {
        if app.name.len() > usize::from(u8::MAX) || u32::try_from(app.blob.len()).is_err() {
            return Err(TraceError::TooMany);
        }
        len = len
            .checked_add(app.encoded_len())
            .ok_or(TraceError::TooMany)?;
    }
    let n_apps = u32::try_from(apps.len()).map_err(|_| TraceError::TooMany)?;
    let payload = u32::try_from(len - SEGMENT_HEADER_LEN).map_err(|_| TraceError::TooMany)?;
    let out = buf.get_mut(..len).ok_or(TraceError::BufferFull)?;
    let mut w = Cursor::new(out);
    w.u32(STATE_MARKER);
    w.u32(n_apps);
    w.u32(payload);
    for app in apps {
        w.u8(app.name.len() as u8);
        w.bytes(app.name.as_bytes());
        w.u32(app.group);
        w.u64(app.first_seq);
        w.u8(app.state);
        w.u32(app.blob.len() as u32);
        w.bytes(app.blob);
    }
    Ok(&buf[..len])
}

/// A segment as stored in the file.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Item<'a> {
    /// The state segment.
    State {
        /// Number of apps.
        n_apps: u32,
        /// Stored payload (compressed unless the header says otherwise).
        payload: &'a [u8],
    },
    /// A segment of ticks.
    Ticks {
        /// Group number.
        group: u32,
        /// Number of ticks.
        n_ticks: u32,
        /// Stored payload.
        payload: &'a [u8],
    },
}

/// Iterates over the segments that follow the header.
#[derive(Clone, Debug)]
pub struct Items<'a> {
    rest: &'a [u8],
}

impl<'a> Items<'a> {
    /// Segments in `rest`, the bytes after the header.
    pub fn new(rest: &'a [u8]) -> Self {
        Self { rest }
    }
}

impl<'a> Iterator for Items<'a> {
    type Item = Result<Item<'a>, TraceError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.rest.is_empty() {
            return None;
        }
        let mut r = Reader::new(self.rest);
        let parsed = (|| {
            let group = r.u32()?;
            let count = r.u32()?;
            let len = r.u32()? as usize;
            let payload = r.take(len)?;
            Ok(if group == STATE_MARKER {
                Item::State {
                    n_apps: count,
                    payload,
                }
            } else {
                Item::Ticks {
                    group,
                    n_ticks: count,
                    payload,
                }
            })
        })();
        match parsed {
            Ok(item) => {
                self.rest = r.rest();
                Some(Ok(item))
            }
            Err(e) => {
                self.rest = &[];
                Some(Err(e))
            }
        }
    }
}

/// One latched input of a tick.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TraceInput {
    /// Low 32 bits of the sample's sequence number.
    pub seq: u32,
    /// Quality word.
    pub quality: Quality,
    /// Value bits.
    pub value: u64,
}

/// One proposal of a tick.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TraceProposal {
    /// Writer.
    pub writer: WriterId,
    /// Output.
    pub signal: SignalId,
    /// Value bits.
    pub value: u64,
    /// Flags (ADR 0037).
    pub flags: u16,
}

impl TraceProposal {
    /// The staged proposal this entry records.
    pub fn to_proposal(&self) -> Proposal {
        Proposal {
            writer: self.writer,
            signal: self.signal,
            value: self.value,
            flags: self.flags & 0xFF,
            source: if self.flags & PROPOSAL_FLAG_ASYNC != 0 {
                Source::Async
            } else {
                Source::Cycle
            },
            ..Proposal::EMPTY
        }
    }
}

/// A decoded tick, borrowing the segment payload.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TickView<'p> {
    /// Tick index.
    pub index: u64,
    /// Monotonic latch instant, ns.
    pub t_mono_ns: u64,
    /// UTC latch instant, ns.
    pub t_utc_ns: i64,
    /// Clock quality code.
    pub clock: u8,
    inputs: &'p [u8],
    proposals: &'p [u8],
    results: &'p [u8],
    /// Raw ML section (count excluded); empty in v1 writers.
    pub ml: &'p [u8],
    /// Raw method section (count excluded); empty in v1 writers.
    pub methods: &'p [u8],
}

impl<'p> TickView<'p> {
    /// The inputs, in port order.
    pub fn inputs(&self) -> impl ExactSizeIterator<Item = TraceInput> + 'p {
        self.inputs
            .as_chunks::<INPUT_LEN>()
            .0
            .iter()
            .map(|c| TraceInput {
                seq: le_u32(&c[0..4]),
                quality: Quality::from_bits(le_u32(&c[4..8])),
                value: le_u64(&c[8..16]),
            })
    }

    /// The proposals, in staging order.
    pub fn proposals(&self) -> impl ExactSizeIterator<Item = TraceProposal> + 'p {
        self.proposals
            .as_chunks::<PROPOSAL_LEN>()
            .0
            .iter()
            .map(|c| TraceProposal {
                writer: WriterId(le_u32(&c[0..4])),
                signal: SignalId(le_u32(&c[4..8])),
                value: le_u64(&c[8..16]),
                flags: u16::from_le_bytes([c[16], c[17]]),
            })
    }

    /// The output results.
    pub fn results(&self) -> impl ExactSizeIterator<Item = TraceResult> + 'p {
        self.results
            .as_chunks::<RESULT_LEN>()
            .0
            .iter()
            .map(|c| TraceResult {
                signal: SignalId(le_u32(&c[0..4])),
                outcome: c[4],
                applied: le_u64(&c[5..13]),
            })
    }

    /// The method records, in staging order (ADR 0043). A record whose
    /// arguments are not the CBOR map of ADR 0043 yields an error and ends
    /// the iteration.
    pub fn method_calls(&self) -> impl Iterator<Item = Result<MethodCall, TraceError>> + 'p {
        let mut r = Reader::new(self.methods);
        core::iter::from_fn(move || {
            if r.rest().is_empty() {
                return None;
            }
            let item = decode_method(&mut r);
            if item.is_err() {
                r = Reader::new(&[]);
            }
            Some(item)
        })
    }

    /// The raw result bytes, for byte-wise replay comparison (docs/16 §7).
    pub fn result_bytes(&self) -> &'p [u8] {
        self.results
    }
}

/// Decodes the ticks of an uncompressed (or decompressed) segment payload.
#[derive(Clone, Debug)]
pub struct Ticks<'p> {
    reader: Reader<'p>,
    remaining: u32,
    ports: usize,
}

impl<'p> Ticks<'p> {
    /// Ticks of a payload holding `n_ticks` ticks of a group with `ports`
    /// latched ports (ADR 0037 §4).
    pub fn new(payload: &'p [u8], n_ticks: u32, ports: usize) -> Self {
        Self {
            reader: Reader::new(payload),
            remaining: n_ticks,
            ports,
        }
    }

    fn decode(&mut self) -> Result<TickView<'p>, TraceError> {
        let r = &mut self.reader;
        let index = r.u64()?;
        let t_mono_ns = r.u64()?;
        let t_utc_ns = r.u64()? as i64;
        let clock = r.u8()?;
        let inputs = r.take(
            self.ports
                .checked_mul(INPUT_LEN)
                .ok_or(TraceError::Malformed)?,
        )?;
        let n = usize::from(r.u16()?);
        let proposals = r.take(n * PROPOSAL_LEN)?;
        let n = usize::from(r.u16()?);
        let results = r.take(n * RESULT_LEN)?;
        let ml_start = r.position();
        for _ in 0..r.u16()? {
            r.u32()?;
            let len = r.u32()? as usize;
            r.take(len)?;
        }
        let ml = r.slice(ml_start + 2, r.position());
        let methods_start = r.position();
        for _ in 0..r.u16()? {
            r.u32()?;
            r.u32()?;
            r.u16()?;
            let len = usize::from(r.u16()?);
            r.take(len)?;
        }
        let methods = r.slice(methods_start + 2, r.position());
        Ok(TickView {
            index,
            t_mono_ns,
            t_utc_ns,
            clock,
            inputs,
            proposals,
            results,
            ml,
            methods,
        })
    }
}

impl<'p> Iterator for Ticks<'p> {
    type Item = Result<TickView<'p>, TraceError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return (!self.reader.rest().is_empty()).then_some(Err(TraceError::Malformed));
        }
        self.remaining -= 1;
        let tick = self.decode();
        if tick.is_err() {
            self.remaining = 0;
            self.reader = Reader::new(&[]);
        }
        Some(tick)
    }
}

/// Decodes the apps of a state segment payload.
#[derive(Clone, Debug)]
pub struct States<'p> {
    reader: Reader<'p>,
    remaining: u32,
}

impl<'p> States<'p> {
    /// Apps of a payload holding `n_apps` entries.
    pub fn new(payload: &'p [u8], n_apps: u32) -> Self {
        Self {
            reader: Reader::new(payload),
            remaining: n_apps,
        }
    }
}

impl<'p> Iterator for States<'p> {
    type Item = Result<AppState<'p>, TraceError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        let r = &mut self.reader;
        let parsed = (|| {
            let len = usize::from(r.u8()?);
            let name = core::str::from_utf8(r.take(len)?).map_err(|_| TraceError::Malformed)?;
            let group = r.u32()?;
            let first_seq = r.u64()?;
            let state = r.u8()?;
            let len = r.u32()? as usize;
            let blob = r.take(len)?;
            Ok(AppState {
                name,
                group,
                first_seq,
                state,
                blob,
            })
        })();
        if parsed.is_err() {
            self.remaining = 0;
        }
        Some(parsed)
    }
}

struct Cursor<'b> {
    buf: &'b mut [u8],
    pos: usize,
}

impl<'b> Cursor<'b> {
    fn new(buf: &'b mut [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn bytes(&mut self, bytes: &[u8]) {
        self.buf[self.pos..self.pos + bytes.len()].copy_from_slice(bytes);
        self.pos += bytes.len();
    }

    fn u8(&mut self, v: u8) {
        self.bytes(&[v]);
    }

    fn u16(&mut self, v: u16) {
        self.bytes(&v.to_le_bytes());
    }

    fn u32(&mut self, v: u32) {
        self.bytes(&v.to_le_bytes());
    }

    fn u64(&mut self, v: u64) {
        self.bytes(&v.to_le_bytes());
    }
}

#[derive(Clone, Debug)]
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], TraceError> {
        let end = self.pos.checked_add(n).ok_or(TraceError::Truncated)?;
        let out = self.bytes.get(self.pos..end).ok_or(TraceError::Truncated)?;
        self.pos = end;
        Ok(out)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], TraceError> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8, TraceError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, TraceError> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    fn u32(&mut self) -> Result<u32, TraceError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64, TraceError> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    fn position(&self) -> usize {
        self.pos
    }

    fn slice(&self, start: usize, end: usize) -> &'a [u8] {
        self.bytes.get(start..end).unwrap_or(&[])
    }

    fn rest(&self) -> &'a [u8] {
        self.bytes.get(self.pos..).unwrap_or(&[])
    }
}

fn decode_method(r: &mut Reader<'_>) -> Result<MethodCall, TraceError> {
    let writer = WriterId(r.u32()?);
    let instance = r.u32()?;
    let method = r.u16()?;
    let len = usize::from(r.u16()?);
    let mut a = Reader::new(r.take(len)?);
    let n = read_uint(&mut a, 5)?;
    if n > MAX_METHOD_ARGS as u64 {
        return Err(TraceError::TooMany);
    }
    let mut args = [0u64; MAX_METHOD_ARGS];
    for (k, slot) in args.iter_mut().enumerate().take(n as usize) {
        if read_uint(&mut a, 0)? != k as u64 {
            return Err(TraceError::Malformed);
        }
        *slot = read_uint(&mut a, 0)?;
    }
    if !a.rest().is_empty() {
        return Err(TraceError::Malformed);
    }
    Ok(MethodCall {
        writer,
        instance,
        method,
        args,
        n_args: n as u8,
        ..MethodCall::EMPTY
    })
}

fn le_u32(b: &[u8]) -> u32 {
    let mut a = [0u8; 4];
    a.copy_from_slice(b);
    u32::from_le_bytes(a)
}

fn le_u64(b: &[u8]) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(b);
    u64::from_le_bytes(a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::RELINQUISH;
    use crate::quality::{QualityCode, QualityReason};
    use crate::slot::SeqExtender;
    use crate::time::ClockQuality;
    use crate::value::NO_TS_SRC;

    fn header() -> Header {
        Header {
            flags: FLAG_UNCOMPRESSED,
            generation_hash: [0xAA; 32],
            registry_hash: [0xBB; 32],
            epoch_mono_ns: 0x0102_0304_0506_0708,
            epoch_utc_ns: -2,
            clock: 1,
        }
    }

    #[test]
    fn header_layout_is_little_endian() {
        let bytes = header().encode();
        assert_eq!(&bytes[..4], b"JKTR");
        assert_eq!(&bytes[4..6], &[1, 0]);
        assert_eq!(&bytes[6..10], &[1, 0, 0, 0]);
        assert_eq!(&bytes[10..42], &[0xAA; 32]);
        assert_eq!(&bytes[42..74], &[0xBB; 32]);
        assert_eq!(&bytes[74..82], &[8, 7, 6, 5, 4, 3, 2, 1]);
        assert_eq!(
            &bytes[82..90],
            &[0xFE, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]
        );
        assert_eq!(bytes[90], 1);
        assert_eq!(Header::decode(&bytes), Ok((header(), &[][..])));
    }

    #[test]
    fn header_rejects_garbage() {
        let mut bytes = header().encode();
        assert_eq!(Header::decode(&bytes[..10]), Err(TraceError::Truncated));
        bytes[6] = 4;
        assert_eq!(Header::decode(&bytes), Err(TraceError::UnknownFlags(4)));
        bytes[4] = 2;
        assert_eq!(
            Header::decode(&bytes),
            Err(TraceError::UnsupportedVersion(2))
        );
        bytes[0] = b'X';
        assert_eq!(Header::decode(&bytes), Err(TraceError::BadMagic));
    }

    fn tick<'a>(
        index: u64,
        inputs: &'a [Sample],
        proposals: &'a [Proposal],
        results: &'a [TraceResult],
    ) -> TickRecord<'a> {
        TickRecord {
            index,
            at: Instant {
                mono_ns: index * 100,
                utc_ns: -1,
                clock: ClockQuality::Holdover,
            },
            inputs,
            proposals,
            results,
            methods: &[],
        }
    }

    #[test]
    fn tick_layout_and_round_trip() {
        let inputs = [Sample::new(
            7,
            Quality::new(QualityCode::Bad, QualityReason::CommFailure),
            NO_TS_SRC,
            0,
            (1 << 32) + 5,
        )];
        let proposals = [Proposal {
            writer: WriterId(3),
            signal: SignalId(9),
            value: 42,
            flags: RELINQUISH,
            source: Source::Async,
            ..Proposal::EMPTY
        }];
        let results = [TraceResult {
            signal: SignalId(9),
            outcome: 3,
            applied: 1,
        }];
        let mut buf = [0u8; 256];
        let mut w = SegmentWriter::new(4, &mut buf).unwrap();
        let t = tick(11, &inputs, &proposals, &results);
        w.push(&t).unwrap();
        let segment = w.finish();
        assert_eq!(segment.len(), SEGMENT_HEADER_LEN + t.encoded_len());
        assert_eq!(&segment[..12], &[4, 0, 0, 0, 1, 0, 0, 0, 80, 0, 0, 0]);
        let body = &segment[12..];
        assert_eq!(&body[..8], &11u64.to_le_bytes());
        assert_eq!(body[24], 1);
        assert_eq!(&body[25..29], &5u32.to_le_bytes());
        assert_eq!(&body[29..33], &0x0C02u32.to_le_bytes());
        assert_eq!(&body[41..43], &[1, 0]);
        assert_eq!(
            &body[59..61],
            &(RELINQUISH | PROPOSAL_FLAG_ASYNC).to_le_bytes()
        );
        let mut items = Items::new(segment);
        let Some(Ok(Item::Ticks {
            group,
            n_ticks,
            payload,
        })) = items.next()
        else {
            panic!("no segment");
        };
        assert_eq!((group, n_ticks), (4, 1));
        assert!(items.next().is_none());
        let view = Ticks::new(payload, n_ticks, 1).next().unwrap().unwrap();
        assert_eq!(view.index, 11);
        assert_eq!(view.t_utc_ns, -1);
        assert_eq!(view.clock, 1);
        let input = view.inputs().next().unwrap();
        assert_eq!((input.seq, input.value), (5, 7));
        let mut ext = SeqExtender::for_trace();
        ext.extend(u64::from(u32::MAX));
        assert_eq!(ext.extend(u64::from(input.seq)), (1 << 32) + 5);
        let p = view.proposals().next().unwrap().to_proposal();
        assert_eq!(p, proposals[0]);
        assert_eq!(view.results().next(), Some(results[0]));
        assert!(view.ml.is_empty() && view.methods.is_empty());
    }

    #[test]
    fn full_buffer_writes_nothing() {
        let mut buf = [0u8; SEGMENT_HEADER_LEN + TICK_FIXED_LEN + 8];
        let mut w = SegmentWriter::new(0, &mut buf).unwrap();
        assert_eq!(w.push(&tick(0, &[], &[], &[])), Ok(()));
        assert_eq!(w.push(&tick(1, &[], &[], &[])), Err(TraceError::BufferFull));
        assert_eq!(w.ticks(), 1);
        assert_eq!(w.finish().len(), SEGMENT_HEADER_LEN + TICK_FIXED_LEN + 8);
    }

    #[test]
    fn state_segment_round_trip() {
        let apps = [
            AppState {
                name: "light",
                group: 0,
                first_seq: 12,
                state: 0,
                blob: &[0xA1, 0x01, 0x02],
            },
            AppState {
                name: "peak_shaver",
                group: 1,
                first_seq: 3,
                state: 1,
                blob: &[],
            },
        ];
        let mut buf = [0u8; 128];
        let segment = write_state_segment(&apps, &mut buf).unwrap();
        assert_eq!(&segment[..4], &[0xFF; 4]);
        let Some(Ok(Item::State { n_apps, payload })) = Items::new(segment).next() else {
            panic!("no state segment");
        };
        let decoded: [AppState<'_>; 2] = {
            let mut it = States::new(payload, n_apps);
            [it.next().unwrap().unwrap(), it.next().unwrap().unwrap()]
        };
        assert_eq!(decoded, apps);
    }

    #[test]
    fn truncated_segments_are_errors() {
        let mut buf = [0u8; 128];
        let mut w = SegmentWriter::new(0, &mut buf).unwrap();
        w.push(&tick(0, &[], &[], &[])).unwrap();
        let segment = w.finish();
        let cut = &segment[..segment.len() - 1];
        assert_eq!(Items::new(cut).next(), Some(Err(TraceError::Truncated)));
        let payload = &segment[SEGMENT_HEADER_LEN..];
        let mut ticks = Ticks::new(payload, 1, 1);
        assert_eq!(ticks.next(), Some(Err(TraceError::Truncated)));
        let mut ticks = Ticks::new(payload, 0, 0);
        assert_eq!(ticks.next(), Some(Err(TraceError::Malformed)));
    }

    #[test]
    fn method_records_are_a_cbor_map_of_slot_words() {
        let mut call = MethodCall {
            writer: WriterId(3),
            instance: 2,
            method: 1,
            n_args: 3,
            ..MethodCall::EMPTY
        };
        call.args[..3].copy_from_slice(&[5, 300, 1 << 40]);
        let methods = [call];
        let record = TickRecord {
            methods: &methods,
            ..tick(0, &[], &[], &[])
        };
        let mut buf = [0u8; 256];
        let mut w = SegmentWriter::new(0, &mut buf).unwrap();
        w.push(&record).unwrap();
        let segment = w.finish();
        let body = &segment[SEGMENT_HEADER_LEN..];
        let section = &body[TICK_FIXED_LEN + 2 + 2 + 2..];
        assert_eq!(&section[..2], &[1, 0]);
        assert_eq!(&section[2..14], &[3, 0, 0, 0, 2, 0, 0, 0, 1, 0, 17, 0]);
        assert_eq!(
            &section[14..],
            &[
                0xA3, 0x00, 0x05, 0x01, 0x19, 0x01, 0x2C, 0x02, 0x1B, 0, 0, 1, 0, 0, 0, 0, 0
            ]
        );
        let view = Ticks::new(body, 1, 0).next().unwrap().unwrap();
        let calls: Result<[MethodCall; 1], _> = {
            let mut it = view.method_calls();
            it.next().unwrap().map(|c| [c])
        };
        assert_eq!(calls.unwrap(), methods);
    }

    #[test]
    fn non_deterministic_cbor_is_refused() {
        let mut r = Reader::new(&[0x18, 0x05]);
        assert_eq!(read_uint(&mut r, 0), Err(TraceError::Malformed));
        let mut r = Reader::new(&[0xA1, 0x01, 0x00]);
        assert_eq!(decode_method_args_only(&mut r), Err(TraceError::Malformed));
    }

    fn decode_method_args_only(r: &mut Reader<'_>) -> Result<(), TraceError> {
        let mut record = [0u8; 15];
        record[10] = 3;
        record[12..].copy_from_slice(r.take(3)?);
        decode_method(&mut Reader::new(&record)).map(|_| ())
    }
}
