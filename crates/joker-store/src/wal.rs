//! The shared write-ahead log: numbered files of checksummed frames
//! (`wal/<n>.wal`), each frame one batch of records.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::frame;

/// One WAL file.
#[derive(Debug)]
struct WalFile {
    id: u64,
    path: PathBuf,
    file: File,
    len: u64,
    synced: u64,
}

/// The WAL of a store.
#[derive(Debug)]
pub struct Wal {
    dir: PathBuf,
    current: WalFile,
    older: Vec<(u64, PathBuf, u64)>,
}

/// The frames of a WAL file found at open.
#[derive(Debug)]
pub struct Found {
    /// File number.
    pub id: u64,
    /// The file's bytes up to the end of its last valid frame.
    pub bytes: Vec<u8>,
    /// Bytes past the valid prefix (a torn or corrupt tail).
    pub torn: u64,
}

fn path_of(dir: &Path, id: u64) -> PathBuf {
    dir.join(format!("{id:016x}.wal"))
}

/// Reads every WAL file of `dir`, in order.
pub fn scan(dir: &Path) -> io::Result<Vec<Found>> {
    let mut ids = Vec::new();
    for entry in fs::read_dir(dir)? {
        let name = entry?.file_name();
        let Some(name) = name.to_str() else { continue };
        if let Some(hex) = name.strip_suffix(".wal")
            && let Ok(id) = u64::from_str_radix(hex, 16)
        {
            ids.push(id);
        }
    }
    ids.sort_unstable();
    let mut found = Vec::with_capacity(ids.len());
    for id in ids {
        let mut bytes = fs::read(path_of(dir, id))?;
        let (_, valid) = frame::split(&bytes);
        let torn = (bytes.len() - valid) as u64;
        bytes.truncate(valid);
        found.push(Found { id, bytes, torn });
    }
    Ok(found)
}

fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

impl Wal {
    /// Starts WAL file `id` in `dir`; the files `older` stay until
    /// [`Wal::trim`].
    pub fn create(dir: &Path, id: u64, older: Vec<(u64, PathBuf, u64)>) -> io::Result<Self> {
        let current = Self::new_file(dir, id)?;
        Ok(Self {
            dir: dir.to_owned(),
            current,
            older,
        })
    }

    fn new_file(dir: &Path, id: u64) -> io::Result<WalFile> {
        let path = path_of(dir, id);
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&path)?;
        sync_dir(dir)?;
        Ok(WalFile {
            id,
            path,
            file,
            len: 0,
            synced: 0,
        })
    }

    /// Bytes of the current file.
    pub fn len(&self) -> u64 {
        self.current.len
    }

    /// Bytes of every WAL file.
    pub fn total_bytes(&self) -> u64 {
        self.current.len + self.older.iter().map(|(_, _, len)| len).sum::<u64>()
    }

    /// True when the current file has bytes not yet synced.
    pub fn dirty(&self) -> bool {
        self.current.synced < self.current.len
    }

    /// Appends one frame holding `payload`. A failed write is rolled back
    /// to the previous end, so a later frame never follows a torn one.
    pub fn append(&mut self, payload: &[u8]) -> io::Result<()> {
        let mut bytes = Vec::with_capacity(frame::HEADER + payload.len());
        frame::put(&mut bytes, payload);
        let c = &mut self.current;
        if let Err(e) = c.file.write_all(&bytes) {
            let _ = c.file.set_len(c.len);
            let _ = c.file.seek(SeekFrom::Start(c.len));
            return Err(e);
        }
        c.len += bytes.len() as u64;
        Ok(())
    }

    /// Syncs the current file.
    pub fn sync(&mut self) -> io::Result<()> {
        if self.dirty() {
            self.current.file.sync_data()?;
            self.current.synced = self.current.len;
        }
        Ok(())
    }

    /// Starts the next file; the current one joins the older ones.
    pub fn rotate(&mut self) -> io::Result<()> {
        self.sync()?;
        let next = Self::new_file(&self.dir, self.current.id + 1)?;
        let old = std::mem::replace(&mut self.current, next);
        self.older.push((old.id, old.path, old.len));
        Ok(())
    }

    /// Deletes every file older than the current one.
    pub fn trim(&mut self) -> io::Result<()> {
        for (_, path, _) in self.older.drain(..) {
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        sync_dir(&self.dir)
    }

    /// Every file with the bytes known to be synced.
    pub fn durability(&self) -> Vec<(PathBuf, u64)> {
        let mut out: Vec<(PathBuf, u64)> = self
            .older
            .iter()
            .map(|(_, path, len)| (path.clone(), *len))
            .collect();
        out.push((self.current.path.clone(), self.current.synced));
        out
    }
}
