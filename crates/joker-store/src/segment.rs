//! Segment files: the sealed blocks of every series with the same tier,
//! class and `keep`, in sealing order (`seg/<tier>-<class>-<keep>/<n>.seg`).
//! A file is closed when it reaches its size or the time span of its tier;
//! closing syncs it and writes its index (`<n>.idx`) so the next open does
//! not scan it.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::block::{self, Header};
use crate::frame::{self, Reader};
use crate::{Class, Tier};

/// What a segment file holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key {
    /// The tier.
    pub tier: Tier,
    /// The class of its series.
    pub class: Class,
    /// Their `keep` for that tier, ns.
    pub keep_ns: u64,
}

impl Key {
    fn dir_name(&self) -> String {
        format!(
            "{}-{}-{}",
            self.tier.name(),
            self.class.name(),
            self.keep_ns / crate::SECOND_NS
        )
    }

    fn parse(name: &str) -> Option<Self> {
        let mut parts = name.splitn(3, '-');
        let tier = Tier::from_name(parts.next()?)?;
        let class = match parts.next()? {
            "telemetry" => Class::Telemetry,
            "alarm" => Class::Alarm,
            "decision" => Class::Decision,
            _ => return None,
        };
        let keep_s: u64 = parts.next()?.parse().ok()?;
        Some(Self {
            tier,
            class,
            keep_ns: keep_s.saturating_mul(crate::SECOND_NS),
        })
    }
}

/// Where a sealed block is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockRef {
    /// The segment file.
    pub file: u64,
    /// Offset of the block in it.
    pub offset: u64,
    /// The block's header.
    pub header: Header,
}

impl BlockRef {
    /// Bytes of the block.
    pub fn len(&self) -> u64 {
        (block::HEADER + self.header.payload_len as usize) as u64
    }
}

/// One segment file.
#[derive(Debug)]
pub struct SegFile {
    /// What it holds.
    pub key: Key,
    /// Its path.
    pub path: PathBuf,
    /// Bytes.
    pub len: u64,
    /// Bytes known to be synced.
    pub synced: u64,
    /// Earliest row instant.
    pub t_min: i64,
    /// Latest row instant.
    pub t_max: i64,
    /// Open for appends (the current file of its key).
    pub file: Option<File>,
    /// The blocks written while it is current, for its index.
    pub blocks: Vec<BlockRef>,
}

/// The segment files of a store.
#[derive(Debug)]
pub struct Segments {
    dir: PathBuf,
    /// Every file, by number.
    pub files: BTreeMap<u64, SegFile>,
    current: BTreeMap<Key, u64>,
    next: u64,
    file_bytes: u64,
}

/// What a scan found besides the blocks.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ScanReport {
    /// Files read through their index.
    pub indexed: u64,
    /// Files scanned block by block.
    pub scanned: u64,
    /// Bytes cut from torn tails.
    pub truncated: u64,
}

fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

fn read_index(path: &Path, file: u64, len: u64) -> Option<Vec<BlockRef>> {
    let bytes = fs::read(path).ok()?;
    let (frames, valid) = frame::split(&bytes);
    if valid != bytes.len() || frames.len() != 1 {
        return None;
    }
    let mut r = Reader::new(frames[0]);
    if r.u64()? != len {
        return None;
    }
    let count = r.u32()? as usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let offset = r.u64()?;
        let raw = r.take(block::HEADER)?;
        let (header, _) = Header::decode(raw)?;
        out.push(BlockRef {
            file,
            offset,
            header,
        });
    }
    r.is_empty().then_some(out)
}

fn write_index(path: &Path, len: u64, blocks: &[BlockRef]) -> io::Result<()> {
    let mut payload = Vec::with_capacity(12 + blocks.len() * (8 + block::HEADER));
    payload.extend_from_slice(&len.to_le_bytes());
    payload.extend_from_slice(&(blocks.len() as u32).to_le_bytes());
    for b in blocks {
        payload.extend_from_slice(&b.offset.to_le_bytes());
        let encoded = b.header.encode(&[]);
        payload.extend_from_slice(encoded.get(..block::HEADER).unwrap_or(&[]));
    }
    let mut out = Vec::new();
    frame::put(&mut out, &payload);
    let tmp = path.with_extension("idx.tmp");
    {
        let mut f = File::create(&tmp)?;
        f.write_all(&out)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

fn scan_file(path: &Path, file: u64) -> io::Result<(Vec<BlockRef>, u64)> {
    let bytes = fs::read(path)?;
    let mut out = Vec::new();
    let mut at = 0usize;
    while let Some(head) = bytes.get(at..at + block::HEADER) {
        let Some((header, crc)) = Header::decode(head) else {
            break;
        };
        let end = at + block::HEADER + header.payload_len as usize;
        let Some(payload) = bytes.get(at + block::HEADER..end) else {
            break;
        };
        if !Header::verify(head, payload, crc) {
            break;
        }
        out.push(BlockRef {
            file,
            offset: at as u64,
            header,
        });
        at = end;
    }
    Ok((out, at as u64))
}

impl Segments {
    /// Opens the segments under `dir`: every file is read through its
    /// index or scanned, and a torn tail is cut. Every file found is
    /// closed; new blocks go to new files.
    pub fn open(dir: &Path, file_bytes: u64) -> io::Result<(Self, Vec<BlockRef>, ScanReport)> {
        fs::create_dir_all(dir)?;
        let mut files = BTreeMap::new();
        let mut blocks = Vec::new();
        let mut report = ScanReport::default();
        let mut next = 0;
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let Some(key) = entry.file_name().to_str().and_then(Key::parse) else {
                continue;
            };
            for item in fs::read_dir(entry.path())? {
                let path = item?.path();
                let Some(no) = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .and_then(|n| n.strip_suffix(".seg"))
                    .and_then(|h| u64::from_str_radix(h, 16).ok())
                else {
                    continue;
                };
                next = next.max(no + 1);
                let len = fs::metadata(&path)?.len();
                let found = match read_index(&path.with_extension("idx"), no, len) {
                    Some(found) => {
                        report.indexed += 1;
                        found
                    }
                    None => {
                        report.scanned += 1;
                        let (found, valid) = scan_file(&path, no)?;
                        if valid < len {
                            report.truncated += len - valid;
                            let f = OpenOptions::new().write(true).open(&path)?;
                            f.set_len(valid)?;
                            f.sync_all()?;
                        }
                        write_index(&path.with_extension("idx"), valid, &found)?;
                        found
                    }
                };
                let len = found.last().map_or(0, |b| b.offset + b.len());
                let t_min = found
                    .iter()
                    .map(|b| b.header.t_min)
                    .min()
                    .unwrap_or(i64::MAX);
                let t_max = found
                    .iter()
                    .map(|b| b.header.t_max)
                    .max()
                    .unwrap_or(i64::MIN);
                files.insert(
                    no,
                    SegFile {
                        key,
                        path,
                        len,
                        synced: len,
                        t_min,
                        t_max,
                        file: None,
                        blocks: Vec::new(),
                    },
                );
                blocks.extend(found);
            }
        }
        Ok((
            Self {
                dir: dir.to_owned(),
                files,
                current: BTreeMap::new(),
                next,
                file_bytes,
            },
            blocks,
            report,
        ))
    }

    /// Bytes of every segment file.
    pub fn total_bytes(&self) -> u64 {
        self.files.values().map(|f| f.len).sum()
    }

    fn close(&mut self, no: u64) -> io::Result<()> {
        if let Some(f) = self.files.get_mut(&no)
            && let Some(file) = f.file.take()
        {
            file.sync_data()?;
            f.synced = f.len;
            write_index(&f.path.with_extension("idx"), f.len, &f.blocks)?;
            f.blocks = Vec::new();
        }
        Ok(())
    }

    fn start(&mut self, key: Key) -> io::Result<u64> {
        let dir = self.dir.join(key.dir_name());
        fs::create_dir_all(&dir)?;
        let no = self.next;
        self.next += 1;
        let path = dir.join(format!("{no:016x}.seg"));
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&path)?;
        sync_dir(&dir)?;
        self.files.insert(
            no,
            SegFile {
                key,
                path,
                len: 0,
                synced: 0,
                t_min: i64::MAX,
                t_max: i64::MIN,
                file: Some(file),
                blocks: Vec::new(),
            },
        );
        self.current.insert(key, no);
        Ok(no)
    }

    /// Appends a sealed block to the current file of `key`, starting a new
    /// file when the block would pass the size or the span of the tier.
    /// A failed write is rolled back.
    pub fn write(&mut self, key: Key, bytes: &[u8]) -> io::Result<BlockRef> {
        let Some((header, _)) = Header::decode(bytes) else {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "not a block"));
        };
        let current = self.current.get(&key).copied().filter(|no| {
            self.files.get(no).is_some_and(|f| {
                f.file.is_some()
                    && f.len + bytes.len() as u64 <= self.file_bytes
                    && (f.len == 0
                        || header.t_max.saturating_sub(f.t_min)
                            < i64::try_from(key.tier.file_span_ns()).unwrap_or(i64::MAX))
            })
        });
        let no = match current {
            Some(no) => no,
            None => {
                if let Some(old) = self.current.remove(&key) {
                    self.close(old)?;
                }
                self.start(key)?
            }
        };
        let Some(f) = self.files.get_mut(&no) else {
            return Err(io::Error::other("segment file vanished"));
        };
        let Some(file) = f.file.as_mut() else {
            return Err(io::Error::other("segment file closed"));
        };
        if let Err(e) = file.write_all(bytes) {
            let _ = file.set_len(f.len);
            let _ = file.seek(SeekFrom::Start(f.len));
            return Err(e);
        }
        let r = BlockRef {
            file: no,
            offset: f.len,
            header,
        };
        f.len += bytes.len() as u64;
        f.t_min = f.t_min.min(header.t_min);
        f.t_max = f.t_max.max(header.t_max);
        f.blocks.push(r);
        Ok(r)
    }

    /// Syncs every file with unsynced bytes.
    pub fn sync(&mut self) -> io::Result<()> {
        for f in self.files.values_mut() {
            if let Some(file) = &f.file
                && f.synced < f.len
            {
                file.sync_data()?;
                f.synced = f.len;
            }
        }
        Ok(())
    }

    /// Deletes a file and its index.
    pub fn delete(&mut self, no: u64) -> io::Result<()> {
        let Some(f) = self.files.remove(&no) else {
            return Ok(());
        };
        if self.current.get(&f.key) == Some(&no) {
            self.current.remove(&f.key);
        }
        for path in [f.path.clone(), f.path.with_extension("idx")] {
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// Closes every current file (clean shutdown).
    pub fn close_all(&mut self) -> io::Result<()> {
        let current: Vec<u64> = std::mem::take(&mut self.current).into_values().collect();
        for no in current {
            self.close(no)?;
        }
        Ok(())
    }

    /// Every file with the bytes known to be synced.
    pub fn durability(&self) -> Vec<(PathBuf, u64)> {
        self.files
            .values()
            .map(|f| (f.path.clone(), f.synced))
            .collect()
    }
}

/// Reads the blocks `refs` of one file, verifying each checksum; a block
/// that fails is skipped.
pub fn read_blocks(path: &Path, refs: &[BlockRef]) -> io::Result<Vec<(Header, Vec<u8>)>> {
    let mut file = File::open(path)?;
    let mut out = Vec::with_capacity(refs.len());
    for r in refs {
        file.seek(SeekFrom::Start(r.offset))?;
        let mut bytes = vec![0u8; r.len() as usize];
        if file.read_exact(&mut bytes).is_err() {
            continue;
        }
        let Some((header, crc)) = Header::decode(&bytes) else {
            continue;
        };
        let payload = bytes.split_off(block::HEADER);
        if Header::verify(&bytes, &payload, crc) {
            out.push((header, payload));
        }
    }
    Ok(out)
}
