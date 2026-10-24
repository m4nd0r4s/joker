//! Atomic, `fsync`'d state files (`<state_dir>/state/<name>`, ref/02
//! §2.2.4, ADR 0061): trips, manual levels and the startup counter must
//! survive a crash mid-write, not just a clean shutdown (docs/01 §7,
//! docs/16 A14). Every write goes through `<state_dir>/tmp/`, on the same
//! file system, and is renamed over its target.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// The directory of the state files, under the state directory.
pub const STATE_DIR: &str = "state";
/// The directory of temporary files, under the state directory.
pub const TMP_DIR: &str = "tmp";

/// The path of state file `name` under `state_dir`.
pub fn path(state_dir: &Path, name: &str) -> PathBuf {
    state_dir.join(STATE_DIR).join(name)
}

/// Overwrites `<state_dir>/state/<name>` with `bytes`: write to
/// `<state_dir>/tmp/<name>`, `fsync` it, `rename(2)` it over the target,
/// then `fsync` the target's directory (so the rename itself survives a
/// crash, not only the bytes of the file).
pub fn write_atomic(state_dir: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    let dir = state_dir.join(STATE_DIR);
    let tmp_dir = state_dir.join(TMP_DIR);
    fs::create_dir_all(&dir)?;
    fs::create_dir_all(&tmp_dir)?;
    let tmp = tmp_dir.join(name);
    let mut f = fs::File::create(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    fs::rename(&tmp, dir.join(name))?;
    fs::File::open(&dir)?.sync_all()
}

/// The bytes of `<state_dir>/state/<name>`, or, when it does not exist
/// yet, of `<state_dir>/<name>` as an earlier build left it (ADR 0061);
/// `None` when neither exists. Any other failure is an error.
pub fn read(state_dir: &Path, name: &str) -> io::Result<Option<Vec<u8>>> {
    match read_file(&path(state_dir, name))? {
        Some(bytes) => Ok(Some(bytes)),
        None => read_file(&state_dir.join(name)),
    }
}

fn read_file(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io::Error::new(e.kind(), format!("{}: {e}", path.display()))),
    }
}

/// The error of a state file that exists but cannot be parsed: a startup
/// error, never an empty load (ADR 0061).
pub fn corrupt(state_dir: &Path, name: &str, e: impl std::fmt::Display) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("{}: {e}", path(state_dir, name).display()),
    )
}

/// Reads `<state_dir>/state/<name>` as a decimal `u64` (0 if missing),
/// persists it incremented by one, and returns the new value — the
/// daemon's own boot counter (docs/01 §20's "startup counter"), bumped
/// once per `Node::new`. A file that is not a number is an error.
pub fn bump_counter(state_dir: &Path, name: &str) -> io::Result<u64> {
    let current = match read(state_dir, name)? {
        None => 0,
        Some(bytes) => String::from_utf8(bytes)
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .ok_or_else(|| corrupt(state_dir, name, "not a decimal counter"))?,
    };
    let next = current.saturating_add(1);
    write_atomic(state_dir, name, next.to_string().as_bytes())?;
    Ok(next)
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
    fn written_bytes_round_trip_under_state() {
        let dir = tempfile::tempdir().unwrap();
        write_atomic(dir.path(), "trips", b"hello").unwrap();
        assert_eq!(read(dir.path(), "trips").unwrap(), Some(b"hello".to_vec()));
        assert_eq!(
            std::fs::read(dir.path().join("state").join("trips")).unwrap(),
            b"hello"
        );
        assert!(!dir.path().join("trips").exists());
    }

    #[test]
    fn a_second_write_overwrites_the_first() {
        let dir = tempfile::tempdir().unwrap();
        write_atomic(dir.path(), "trips", b"first").unwrap();
        write_atomic(dir.path(), "trips", b"second").unwrap();
        assert_eq!(read(dir.path(), "trips").unwrap(), Some(b"second".to_vec()));
    }

    #[test]
    fn the_temporary_file_goes_through_tmp_and_is_not_left_behind() {
        let dir = tempfile::tempdir().unwrap();
        write_atomic(dir.path(), "trips", b"x").unwrap();
        assert!(dir.path().join("tmp").is_dir());
        assert!(!dir.path().join("tmp").join("trips").exists());
        assert!(!dir.path().join("state").join("trips.tmp").exists());
    }

    #[test]
    fn a_file_left_at_the_root_by_an_earlier_build_is_still_read() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("trips"), b"legacy").unwrap();
        assert_eq!(read(dir.path(), "trips").unwrap(), Some(b"legacy".to_vec()));
        write_atomic(dir.path(), "trips", b"new").unwrap();
        assert_eq!(read(dir.path(), "trips").unwrap(), Some(b"new".to_vec()));
    }

    #[test]
    fn a_missing_counter_starts_at_one() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(bump_counter(dir.path(), "boot_count").unwrap(), 1);
    }

    #[test]
    fn a_counter_increments_and_persists_across_loads() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(bump_counter(dir.path(), "boot_count").unwrap(), 1);
        assert_eq!(bump_counter(dir.path(), "boot_count").unwrap(), 2);
        assert_eq!(bump_counter(dir.path(), "boot_count").unwrap(), 3);
        assert_eq!(read(dir.path(), "boot_count").unwrap(), Some(b"3".to_vec()));
    }

    #[test]
    fn a_counter_that_is_not_a_number_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        write_atomic(dir.path(), "boot_count", b"garbage").unwrap();
        let e = bump_counter(dir.path(), "boot_count").unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidData);
    }
}
