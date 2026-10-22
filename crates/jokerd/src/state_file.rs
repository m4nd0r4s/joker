//! Atomic, `fsync`'d state files (`<state_dir>/<name>`): trips and manual
//! levels must survive a crash mid-write, not just a clean shutdown
//! (docs/ref/02 §2.2.4, docs/16 A14).

use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// Overwrites `<state_dir>/<name>` with `bytes`: write to a sibling
/// `.tmp` file, `fsync` it, `rename(2)` over the target, then `fsync`
/// the directory (so the rename itself survives a crash, not only the
/// bytes of the file).
pub fn write_atomic(state_dir: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    let path = state_dir.join(name);
    let tmp = state_dir.join(format!("{name}.tmp"));
    let mut f = fs::File::create(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    fs::rename(&tmp, &path)?;
    fs::File::open(state_dir)?.sync_all()
}

/// `<state_dir>/<name>`'s bytes, or `None` when the file does not exist.
pub fn read(state_dir: &Path, name: &str) -> io::Result<Option<Vec<u8>>> {
    match fs::read(state_dir.join(name)) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_reads_as_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read(dir.path(), "trips").unwrap(), None);
    }

    #[test]
    fn written_bytes_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        write_atomic(dir.path(), "trips", b"hello").unwrap();
        assert_eq!(read(dir.path(), "trips").unwrap(), Some(b"hello".to_vec()));
    }

    #[test]
    fn a_second_write_overwrites_the_first() {
        let dir = tempfile::tempdir().unwrap();
        write_atomic(dir.path(), "trips", b"first").unwrap();
        write_atomic(dir.path(), "trips", b"second").unwrap();
        assert_eq!(read(dir.path(), "trips").unwrap(), Some(b"second".to_vec()));
    }

    #[test]
    fn no_tmp_file_is_left_behind_after_a_successful_write() {
        let dir = tempfile::tempdir().unwrap();
        write_atomic(dir.path(), "trips", b"x").unwrap();
        assert!(!dir.path().join("trips.tmp").exists());
    }
}
