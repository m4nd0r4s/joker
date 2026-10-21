//! The cycle ABI, version 1 (ref/04 §3): the packed input image the host
//! writes before `on-cycle` and the proposal buffer it reads back.
//!
//! All integers are little-endian and unaligned. Reserved fields are written
//! as 0 and ignored on read. Nothing here allocates.

/// The cycle ABI version this host speaks (ref/04 §3.1).
pub const ABI_VERSION: u8 = 1;
/// Length of the image header.
pub const HEADER_LEN: usize = 64;
/// Length of one sample entry.
pub const SAMPLE_LEN: usize = 24;
/// Length of one result entry.
pub const RESULT_LEN: usize = 24;
/// Length of the proposal buffer header.
pub const BUFFER_HEADER_LEN: usize = 8;
/// Size reserved in the wide section for each wide value.
pub const WIDE_LEN: usize = 64;
/// Default proposal buffer capacity (ref/04 §3.3).
pub const DEFAULT_PROPOSAL_CAPACITY: u32 = 16 * 1024;
/// Smallest proposal buffer capacity an app may ask for.
pub const MIN_PROPOSAL_CAPACITY: u32 = 1024;
/// Largest proposal buffer capacity an app may ask for.
pub const MAX_PROPOSAL_CAPACITY: u32 = 1024 * 1024;
/// Largest inline value of a `WRITE` or `PUBLISH` record.
pub const MAX_INLINE: usize = 64;
/// Largest `METHOD` argument blob.
pub const MAX_ARGS: usize = 4 * 1024;
/// Largest `EMIT` payload.
pub const MAX_PAYLOAD: usize = 8 * 1024;

/// Flags of the image header (ref/04 §3.2).
pub mod image_flags {
    /// First tick of this instance in this generation.
    pub const FIRST_TICK: u16 = 1;
    /// The app runs in shadow mode (M16).
    pub const SHADOW: u16 = 1 << 1;
    /// The tick is replayed from a trace.
    pub const REPLAY: u16 = 1 << 2;
    /// The previous tick was skipped by `overrun = "skip"`.
    pub const AFTER_SKIP: u16 = 1 << 3;
}

/// Flags of a sample entry (ref/04 §3.2).
pub mod sample_flags {
    /// The value lives in the wide section.
    pub const WIDE: u16 = 1;
    /// `ts_hw` is available through `cycle-io.read`.
    pub const HAS_TS_HW: u16 = 1 << 1;
    /// `ts_src` is available through `cycle-io.read`.
    pub const HAS_TS_SRC: u16 = 1 << 2;
    /// The sequence number changed since the previous latch of this group.
    pub const CHANGED: u16 = 1 << 3;
    /// The port is not bound.
    pub const UNBOUND: u16 = 1 << 4;
}

/// Flags of a result entry (ref/04 §3.2).
pub mod result_flags {
    /// The lease of this writer is active; `mine` is valid.
    pub const MINE_ACTIVE: u8 = 1;
    /// The output is in trip.
    pub const TRIPPED: u8 = 1 << 1;
    /// The owner group is another group: the result is from its last commit.
    pub const OTHER_GROUP: u8 = 1 << 2;
}

/// Flags of `WRITE` and `PUBLISH` records (ref/04 §3.3).
pub mod record_flags {
    /// `WRITE` only: release this writer's level; the value is 0.
    pub const RELINQUISH: u8 = 1;
    /// The value is a length `L <= 64`, and `L` bytes follow.
    pub const INLINE: u8 = 1 << 1;
    /// `WRITE` only, 1.1 (`scheduled-writes`): `at_mono_ns` follows.
    pub const AT: u8 = 1 << 2;
}

/// Record types of the proposal buffer (ref/04 §3.3).
pub mod record_type {
    /// A write proposal.
    pub const WRITE: u8 = 1;
    /// A class method call.
    pub const METHOD: u8 = 2;
    /// A published signal.
    pub const PUBLISH: u8 = 3;
    /// A counter increment.
    pub const COUNTER_ADD: u8 = 4;
    /// An event.
    pub const EMIT: u8 = 5;
    /// A substituted read (`get_or`).
    pub const NOTE: u8 = 6;
}

/// Length of an image with `n_slots` samples, `n_outcomes` results and
/// `n_wide` wide values, rounded up to 8 (ref/04 §3.2).
pub const fn image_len(n_slots: usize, n_outcomes: usize, n_wide: usize) -> usize {
    let raw = HEADER_LEN + SAMPLE_LEN * (n_slots + n_outcomes) + WIDE_LEN * n_wide;
    raw.div_ceil(8) * 8
}

/// Why an image or a proposal buffer is malformed.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AbiError {
    /// The buffer is shorter than its layout requires.
    Truncated,
    /// The ABI version is not [`ABI_VERSION`].
    Version(u8),
    /// The `len` field does not match the buffer length.
    Length {
        /// Length declared in the header.
        declared: u32,
        /// Actual length.
        actual: usize,
    },
    /// The records do not end exactly at the declared length.
    RecordCount,
    /// A record type this ABI version does not define.
    UnknownRecord(u8),
    /// A reserved flag is set, or a flag is not allowed on this record.
    Flags(u8),
    /// A length is above its limit.
    TooLong,
    /// The header length field is not 64.
    HeaderLength(u8),
}

/// The header of the input image.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub struct Header {
    /// [`image_flags`].
    pub flags: u16,
    /// Task index in `joker:meta`.
    pub task: u32,
    /// Tick number of the group.
    pub tick_seq: u64,
    /// Latch instant, monotonic ns.
    pub t_mono_ns: u64,
    /// Latch instant, UTC ns.
    pub t_utc_ns: i64,
    /// Effective period of the group, ns.
    pub dt_ns: u64,
    /// Clock quality code (ref/00 §6).
    pub clock: u8,
    /// Number of sample entries.
    pub n_slots: u32,
    /// Number of result entries.
    pub n_outcomes: u32,
    /// Start of the wide section.
    pub wide_off: u32,
    /// Length of the wide section.
    pub wide_len: u32,
}

impl Header {
    /// Writes the header into the first 64 bytes of `image`.
    pub fn write(&self, image: &mut [u8]) -> Result<(), AbiError> {
        let h = image.get_mut(..HEADER_LEN).ok_or(AbiError::Truncated)?;
        h.fill(0);
        h[0] = ABI_VERSION;
        h[1] = HEADER_LEN as u8;
        h[2..4].copy_from_slice(&self.flags.to_le_bytes());
        h[4..8].copy_from_slice(&self.task.to_le_bytes());
        h[8..16].copy_from_slice(&self.tick_seq.to_le_bytes());
        h[16..24].copy_from_slice(&self.t_mono_ns.to_le_bytes());
        h[24..32].copy_from_slice(&self.t_utc_ns.to_le_bytes());
        h[32..40].copy_from_slice(&self.dt_ns.to_le_bytes());
        h[40] = self.clock;
        h[44..48].copy_from_slice(&self.n_slots.to_le_bytes());
        h[48..52].copy_from_slice(&self.n_outcomes.to_le_bytes());
        let outcomes_off = HEADER_LEN as u32 + SAMPLE_LEN as u32 * self.n_slots;
        h[52..56].copy_from_slice(&outcomes_off.to_le_bytes());
        h[56..60].copy_from_slice(&self.wide_off.to_le_bytes());
        h[60..64].copy_from_slice(&self.wide_len.to_le_bytes());
        Ok(())
    }

    /// Reads a header written by [`Header::write`].
    pub fn read(image: &[u8]) -> Result<Self, AbiError> {
        let h = image.get(..HEADER_LEN).ok_or(AbiError::Truncated)?;
        if h[0] != ABI_VERSION {
            return Err(AbiError::Version(h[0]));
        }
        if h[1] as usize != HEADER_LEN {
            return Err(AbiError::HeaderLength(h[1]));
        }
        Ok(Self {
            flags: u16_at(h, 2),
            task: u32_at(h, 4),
            tick_seq: u64_at(h, 8),
            t_mono_ns: u64_at(h, 16),
            t_utc_ns: u64_at(h, 24) as i64,
            dt_ns: u64_at(h, 32),
            clock: h[40],
            n_slots: u32_at(h, 44),
            n_outcomes: u32_at(h, 48),
            wide_off: u32_at(h, 56),
            wide_len: u32_at(h, 60),
        })
    }
}

/// One sample entry of the image.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub struct SampleEntry {
    /// Value bits (ref/00 §4.1).
    pub value: u64,
    /// Quality code (ref/00 §4.3).
    pub quality_code: u8,
    /// Quality reason (ref/00 §4.3), already projected (ref/04 §2.8).
    pub quality_reason: u8,
    /// [`sample_flags`].
    pub flags: u16,
    /// Low 32 bits of the sequence number.
    pub seq: u32,
    /// Monotonic acquisition instant, ns.
    pub ts_acq: u64,
}

impl SampleEntry {
    fn offset(index: usize) -> usize {
        HEADER_LEN + SAMPLE_LEN * index
    }

    /// Writes sample entry `index`.
    pub fn write(&self, image: &mut [u8], index: usize) -> Result<(), AbiError> {
        let at = Self::offset(index);
        let e = image
            .get_mut(at..at + SAMPLE_LEN)
            .ok_or(AbiError::Truncated)?;
        e[0..8].copy_from_slice(&self.value.to_le_bytes());
        e[8] = self.quality_code;
        e[9] = self.quality_reason;
        e[10..12].copy_from_slice(&self.flags.to_le_bytes());
        e[12..16].copy_from_slice(&self.seq.to_le_bytes());
        e[16..24].copy_from_slice(&self.ts_acq.to_le_bytes());
        Ok(())
    }

    /// Reads sample entry `index`.
    pub fn read(image: &[u8], index: usize) -> Result<Self, AbiError> {
        let at = Self::offset(index);
        let e = image.get(at..at + SAMPLE_LEN).ok_or(AbiError::Truncated)?;
        Ok(Self {
            value: u64_at(e, 0),
            quality_code: e[8],
            quality_reason: e[9],
            flags: u16_at(e, 10),
            seq: u32_at(e, 12),
            ts_acq: u64_at(e, 16),
        })
    }
}

/// One result entry of the image.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub struct ResultEntry {
    /// Outcome code (ref/00 §4.4), 0 = none.
    pub outcome: u8,
    /// Rejection reason or overriding level; otherwise 0.
    pub detail: u8,
    /// Active level that won (1–16), 0 = none.
    pub winner: u8,
    /// [`result_flags`].
    pub flags: u8,
    /// Low 32 bits of the owner group's tick of the result.
    pub commit_seq: u32,
    /// Value applied in that commit.
    pub applied: u64,
    /// This writer's active proposal, valid with `MINE_ACTIVE`.
    pub mine: u64,
}

impl ResultEntry {
    fn offset(n_slots: usize, index: usize) -> usize {
        HEADER_LEN + SAMPLE_LEN * n_slots + RESULT_LEN * index
    }

    /// Writes result entry `index` of an image with `n_slots` samples.
    pub fn write(&self, image: &mut [u8], n_slots: usize, index: usize) -> Result<(), AbiError> {
        let at = Self::offset(n_slots, index);
        let e = image
            .get_mut(at..at + RESULT_LEN)
            .ok_or(AbiError::Truncated)?;
        e[0] = self.outcome;
        e[1] = self.detail;
        e[2] = self.winner;
        e[3] = self.flags;
        e[4..8].copy_from_slice(&self.commit_seq.to_le_bytes());
        e[8..16].copy_from_slice(&self.applied.to_le_bytes());
        e[16..24].copy_from_slice(&self.mine.to_le_bytes());
        Ok(())
    }

    /// Reads result entry `index` of an image with `n_slots` samples.
    pub fn read(image: &[u8], n_slots: usize, index: usize) -> Result<Self, AbiError> {
        let at = Self::offset(n_slots, index);
        let e = image.get(at..at + RESULT_LEN).ok_or(AbiError::Truncated)?;
        Ok(Self {
            outcome: e[0],
            detail: e[1],
            winner: e[2],
            flags: e[3],
            commit_seq: u32_at(e, 4),
            applied: u64_at(e, 8),
            mine: u64_at(e, 16),
        })
    }
}

/// One record of the proposal buffer.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Record<'a> {
    /// `WRITE`: a write proposal or a relinquish.
    Write {
        /// [`record_flags`].
        flags: u8,
        /// Port handle.
        port: u32,
        /// Value bits, or the inline length with `INLINE`.
        value: u64,
        /// The inline bytes; empty without `INLINE`.
        inline: &'a [u8],
        /// `at_mono_ns` with `AT`.
        at: Option<u64>,
    },
    /// `METHOD`: a class method call.
    Method {
        /// Instance handle.
        instance: u32,
        /// Method index.
        method: u16,
        /// Deterministic CBOR arguments.
        args: &'a [u8],
    },
    /// `PUBLISH`: an app-published signal.
    Publish {
        /// [`record_flags`].
        flags: u8,
        /// Port handle.
        port: u32,
        /// Value bits, or the inline length with `INLINE`.
        value: u64,
        /// The inline bytes; empty without `INLINE`.
        inline: &'a [u8],
    },
    /// `COUNTER_ADD`: adds to an app counter.
    CounterAdd {
        /// Port handle.
        port: u32,
        /// The increment.
        delta: u64,
    },
    /// `EMIT`: an event.
    Emit {
        /// Port handle.
        port: u32,
        /// Deterministic CBOR payload.
        payload: &'a [u8],
    },
    /// `NOTE`: the read of a port was substituted (`get_or`, docs/16 §3.1).
    Note {
        /// Port handle.
        port: u32,
    },
}

/// Iterator over the records of a proposal buffer. Each item is checked for
/// structure only (ref/04 §3.4, rule 4); ports are checked by the caller.
#[derive(Clone, Debug)]
pub struct Records<'a> {
    buf: &'a [u8],
    pos: usize,
    left: u16,
    done: bool,
}

/// Parses the header of a proposal buffer and returns its records. An empty
/// buffer has no records.
pub fn records(buf: &[u8]) -> Result<Records<'_>, AbiError> {
    if buf.is_empty() {
        return Ok(Records {
            buf,
            pos: 0,
            left: 0,
            done: false,
        });
    }
    let h = buf.get(..BUFFER_HEADER_LEN).ok_or(AbiError::Truncated)?;
    if h[0] != ABI_VERSION {
        return Err(AbiError::Version(h[0]));
    }
    let declared = u32_at(h, 4);
    if declared as usize != buf.len() {
        return Err(AbiError::Length {
            declared,
            actual: buf.len(),
        });
    }
    Ok(Records {
        buf,
        pos: BUFFER_HEADER_LEN,
        left: u16_at(h, 2),
        done: false,
    })
}

impl<'a> Records<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], AbiError> {
        let end = self.pos.checked_add(n).ok_or(AbiError::Truncated)?;
        let s = self.buf.get(self.pos..end).ok_or(AbiError::Truncated)?;
        self.pos = end;
        Ok(s)
    }

    fn next_record(&mut self) -> Result<Record<'a>, AbiError> {
        let head = self.take(2)?;
        let (kind, flags) = (head[0], head[1]);
        match kind {
            record_type::WRITE | record_type::PUBLISH => {
                let allowed = if kind == record_type::WRITE {
                    record_flags::RELINQUISH | record_flags::INLINE | record_flags::AT
                } else {
                    record_flags::INLINE
                };
                if flags & !allowed != 0
                    || (flags & record_flags::RELINQUISH != 0 && flags & record_flags::INLINE != 0)
                {
                    return Err(AbiError::Flags(flags));
                }
                let body = self.take(12)?;
                let port = u32_at(body, 0);
                let value = u64_at(body, 4);
                if flags & record_flags::RELINQUISH != 0 && value != 0 {
                    return Err(AbiError::Flags(flags));
                }
                let inline = if flags & record_flags::INLINE != 0 {
                    let n = usize::try_from(value).map_err(|_| AbiError::TooLong)?;
                    if n > MAX_INLINE {
                        return Err(AbiError::TooLong);
                    }
                    self.take(n)?
                } else {
                    &[]
                };
                if kind == record_type::PUBLISH {
                    return Ok(Record::Publish {
                        flags,
                        port,
                        value,
                        inline,
                    });
                }
                let at = if flags & record_flags::AT != 0 {
                    Some(u64_at(self.take(8)?, 0))
                } else {
                    None
                };
                Ok(Record::Write {
                    flags,
                    port,
                    value,
                    inline,
                    at,
                })
            }
            record_type::METHOD => {
                if flags != 0 {
                    return Err(AbiError::Flags(flags));
                }
                let body = self.take(8)?;
                let n = u16_at(body, 6) as usize;
                if n > MAX_ARGS {
                    return Err(AbiError::TooLong);
                }
                Ok(Record::Method {
                    instance: u32_at(body, 0),
                    method: u16_at(body, 4),
                    args: self.take(n)?,
                })
            }
            record_type::COUNTER_ADD => {
                if flags != 0 {
                    return Err(AbiError::Flags(flags));
                }
                let body = self.take(12)?;
                Ok(Record::CounterAdd {
                    port: u32_at(body, 0),
                    delta: u64_at(body, 4),
                })
            }
            record_type::EMIT => {
                if flags != 0 {
                    return Err(AbiError::Flags(flags));
                }
                let body = self.take(6)?;
                let n = u16_at(body, 4) as usize;
                if n > MAX_PAYLOAD {
                    return Err(AbiError::TooLong);
                }
                Ok(Record::Emit {
                    port: u32_at(body, 0),
                    payload: self.take(n)?,
                })
            }
            record_type::NOTE => {
                if flags != 0 {
                    return Err(AbiError::Flags(flags));
                }
                Ok(Record::Note {
                    port: u32_at(self.take(4)?, 0),
                })
            }
            other => Err(AbiError::UnknownRecord(other)),
        }
    }
}

impl<'a> Iterator for Records<'a> {
    type Item = Result<Record<'a>, AbiError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        if self.left == 0 {
            self.done = true;
            if self.pos != self.buf.len() {
                return Some(Err(AbiError::RecordCount));
            }
            return None;
        }
        self.left -= 1;
        let item = self.next_record();
        if item.is_err() {
            self.done = true;
        }
        Some(item)
    }
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    let mut w = [0u8; 8];
    w.copy_from_slice(&b[at..at + 8]);
    u64::from_le_bytes(w)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer(records: &[&[u8]]) -> Vec<u8> {
        let body: Vec<u8> = records.concat();
        let mut b = vec![ABI_VERSION, 0];
        b.extend_from_slice(&(records.len() as u16).to_le_bytes());
        b.extend_from_slice(&((BUFFER_HEADER_LEN + body.len()) as u32).to_le_bytes());
        b.extend_from_slice(&body);
        b
    }

    fn write(flags: u8, port: u32, value: u64) -> Vec<u8> {
        let mut r = vec![record_type::WRITE, flags];
        r.extend_from_slice(&port.to_le_bytes());
        r.extend_from_slice(&value.to_le_bytes());
        r
    }

    #[test]
    fn image_length_matches_the_reference_example() {
        assert_eq!(image_len(72, 12, 0), 64 + 24 * 84);
        assert_eq!(image_len(1, 1, 1), 64 + 48 + 64);
    }

    #[test]
    fn header_and_entries_round_trip() {
        let mut image = vec![0u8; image_len(2, 1, 0)];
        let header = Header {
            flags: image_flags::FIRST_TICK | image_flags::AFTER_SKIP,
            task: 3,
            tick_seq: 9,
            t_mono_ns: 10,
            t_utc_ns: -11,
            dt_ns: 12,
            clock: 1,
            n_slots: 2,
            n_outcomes: 1,
            wide_off: 136,
            wide_len: 0,
        };
        header.write(&mut image).unwrap();
        assert_eq!(Header::read(&image).unwrap(), header);
        assert_eq!(u32_at(&image, 52), 64 + 48);
        let s = SampleEntry {
            value: 7,
            quality_code: 2,
            quality_reason: 12,
            flags: sample_flags::CHANGED,
            seq: 5,
            ts_acq: 99,
        };
        s.write(&mut image, 1).unwrap();
        assert_eq!(SampleEntry::read(&image, 1).unwrap(), s);
        let r = ResultEntry {
            outcome: 8,
            detail: 4,
            winner: 13,
            flags: result_flags::TRIPPED,
            commit_seq: 8,
            applied: 1,
            mine: 0,
        };
        r.write(&mut image, 2, 0).unwrap();
        assert_eq!(ResultEntry::read(&image, 2, 0).unwrap(), r);
        assert_eq!(
            SampleEntry::default().write(&mut image, 3),
            Err(AbiError::Truncated)
        );
    }

    #[test]
    fn records_parse_in_order() {
        let mut note = vec![record_type::NOTE, 0];
        note.extend_from_slice(&2u32.to_le_bytes());
        let b = buffer(&[
            &write(0, 1, 42),
            &write(record_flags::RELINQUISH, 1, 0),
            &note,
        ]);
        let got: Vec<_> = records(&b).unwrap().collect::<Result<_, _>>().unwrap();
        assert_eq!(
            got,
            vec![
                Record::Write {
                    flags: 0,
                    port: 1,
                    value: 42,
                    inline: &[],
                    at: None
                },
                Record::Write {
                    flags: record_flags::RELINQUISH,
                    port: 1,
                    value: 0,
                    inline: &[],
                    at: None
                },
                Record::Note { port: 2 },
            ]
        );
        assert_eq!(records(&[]).unwrap().count(), 0);
    }

    #[test]
    fn malformed_buffers_are_refused() {
        let mut b = buffer(&[&write(0, 1, 42)]);
        b[4] += 1;
        assert!(matches!(records(&b), Err(AbiError::Length { .. })));
        let mut b = buffer(&[&write(0, 1, 42)]);
        b[0] = 2;
        assert_eq!(records(&b).err(), Some(AbiError::Version(2)));
        let b = buffer(&[&write(1 << 5, 1, 42)]);
        assert_eq!(
            records(&b).unwrap().next(),
            Some(Err(AbiError::Flags(1 << 5)))
        );
        let b = buffer(&[&[9, 0]]);
        assert_eq!(
            records(&b).unwrap().next(),
            Some(Err(AbiError::UnknownRecord(9)))
        );
        let mut b = buffer(&[&write(0, 1, 42), &write(0, 1, 43)]);
        b[2] = 1;
        let mut it = records(&b).unwrap();
        assert!(it.next().unwrap().is_ok());
        assert_eq!(it.next(), Some(Err(AbiError::RecordCount)));
        let mut b = buffer(&[&write(0, 1, 42)]);
        b[2] = 2;
        let mut it = records(&b).unwrap();
        assert!(it.next().unwrap().is_ok());
        assert_eq!(it.next(), Some(Err(AbiError::Truncated)));
        let b = buffer(&[&write(record_flags::INLINE, 1, 65)]);
        assert_eq!(records(&b).unwrap().next(), Some(Err(AbiError::TooLong)));
    }
}
