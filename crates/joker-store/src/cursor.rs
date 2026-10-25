//! Persistent cursors (store-and-forward, docs/01 §12): for each consumer,
//! the store sequence number of the last record it delivered, per series.
//! A cursor file is replaced atomically: written aside, synced, renamed.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::frame::{self, Reader};

fn valid(consumer: &str) -> bool {
    !consumer.is_empty()
        && consumer.len() <= 64
        && consumer
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

fn path(dir: &Path, consumer: &str) -> io::Result<PathBuf> {
    if !valid(consumer) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid consumer name `{consumer}`"),
        ));
    }
    Ok(dir.join(format!("{consumer}.cur")))
}

/// The cursor of `consumer`; empty when it has none.
pub fn load(dir: &Path, consumer: &str) -> io::Result<BTreeMap<String, u64>> {
    let bytes = match fs::read(path(dir, consumer)?) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(e) => return Err(e),
    };
    let (frames, _) = frame::split(&bytes);
    let mut out = BTreeMap::new();
    let Some(payload) = frames.first() else {
        return Ok(out);
    };
    let mut r = Reader::new(payload);
    while !r.is_empty() {
        let corrupt = || io::Error::new(io::ErrorKind::InvalidData, "corrupt cursor");
        let name = r.bytes().ok_or_else(corrupt)?;
        let n = r.u64().ok_or_else(corrupt)?;
        let name = String::from_utf8(name.to_vec()).map_err(|_| corrupt())?;
        out.insert(name, n);
    }
    Ok(out)
}

/// Replaces the cursor of `consumer`.
pub fn save(dir: &Path, consumer: &str, cursor: &BTreeMap<String, u64>) -> io::Result<()> {
    let target = path(dir, consumer)?;
    let mut payload = Vec::new();
    for (name, n) in cursor {
        frame::put_bytes(&mut payload, name.as_bytes());
        payload.extend_from_slice(&n.to_le_bytes());
    }
    let mut out = Vec::new();
    frame::put(&mut out, &payload);
    let tmp = target.with_extension("cur.tmp");
    {
        let mut f = File::create(&tmp)?;
        f.write_all(&out)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, &target)?;
    File::open(dir)?.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cursor_round_trips_and_names_are_checked() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = BTreeMap::new();
        c.insert("meter.p".to_owned(), 42);
        c.insert("alarm:grid".to_owned(), 7);
        save(dir.path(), "cloud", &c).unwrap();
        assert_eq!(load(dir.path(), "cloud").unwrap(), c);
        assert!(load(dir.path(), "other").unwrap().is_empty());
        assert!(save(dir.path(), "../x", &c).is_err());
    }
}
