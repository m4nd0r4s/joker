//! The content-addressed store of a node (`artifacts/sha256/<hex>`, ref/02
//! §2.2.4): the blobs `jk apply` uploads through `/v1/blobs/uploads`
//! (ref/09 §5.13) and the generations refer to by digest (ADR 0067).

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::generations::hex;

/// The largest blob a monolithic upload carries (ref/09 §5.13).
pub const MAX_BLOB: usize = 8 << 20;

/// The store of a state directory.
#[derive(Debug, Clone)]
pub struct Blobs {
    dir: PathBuf,
    tmp: PathBuf,
}

/// Why a blob was not stored.
#[derive(Debug)]
pub enum PutError {
    /// The bytes do not have the announced digest (`J-E1603`).
    Mismatch {
        /// The digest of what arrived.
        actual: String,
    },
    /// The blob is larger than [`MAX_BLOB`] (`J-E1606`).
    TooLarge,
    /// The store could not be written.
    Io(io::Error),
}

/// `sha256:<64 lower-case hex digits>` to its bytes.
pub fn parse_digest(text: &str) -> Option<[u8; 32]> {
    let hex = text.strip_prefix("sha256:")?;
    if hex.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        let pair = hex.get(2 * i..2 * i + 2)?;
        if pair.bytes().any(|b| b.is_ascii_uppercase()) {
            return None;
        }
        *byte = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(out)
}

/// The digest of `bytes`, as `sha256:<hex>`.
pub fn digest_of(bytes: &[u8]) -> String {
    format!("sha256:{}", hex(&Sha256::digest(bytes)))
}

impl Blobs {
    /// The store of the state directory `state_dir`.
    pub fn new(state_dir: &Path) -> Self {
        Self {
            dir: state_dir.join("artifacts").join("sha256"),
            tmp: state_dir.join("tmp"),
        }
    }

    /// Where the blob of `digest` lives.
    pub fn path(&self, digest: &[u8; 32]) -> PathBuf {
        self.dir.join(hex(digest))
    }

    /// True when the blob is stored.
    pub fn contains(&self, digest: &[u8; 32]) -> bool {
        self.path(digest).is_file()
    }

    /// The bytes of a stored blob.
    pub fn get(&self, digest: &[u8; 32]) -> io::Result<Vec<u8>> {
        fs::read(self.path(digest))
    }

    /// Stores `bytes` under `digest` after checking it, through `tmp/` and a
    /// rename (ref/02 §2.2.4). Returns whether the blob already existed.
    pub fn put(&self, digest: &[u8; 32], bytes: &[u8]) -> Result<bool, PutError> {
        if bytes.len() > MAX_BLOB {
            return Err(PutError::TooLarge);
        }
        let actual: [u8; 32] = Sha256::digest(bytes).into();
        if &actual != digest {
            return Err(PutError::Mismatch {
                actual: format!("sha256:{}", hex(&actual)),
            });
        }
        let path = self.path(digest);
        if path.is_file() {
            return Ok(true);
        }
        fs::create_dir_all(&self.dir).map_err(PutError::Io)?;
        fs::create_dir_all(&self.tmp).map_err(PutError::Io)?;
        let tmp = self.tmp.join(format!("{}.blob", hex(digest)));
        let mut file = fs::File::create(&tmp).map_err(PutError::Io)?;
        file.write_all(bytes).map_err(PutError::Io)?;
        file.sync_all().map_err(PutError::Io)?;
        fs::rename(&tmp, &path).map_err(PutError::Io)?;
        if let Ok(dir) = fs::File::open(&self.dir) {
            let _ = dir.sync_all();
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digests_parse_only_in_their_canonical_form() {
        let d = digest_of(b"light");
        assert_eq!(
            parse_digest(&d).map(|b| hex(&b)),
            d.strip_prefix("sha256:").map(str::to_owned)
        );
        assert!(parse_digest("sha256:ABCD").is_none());
        assert!(parse_digest(&d.to_uppercase().replace("SHA256", "sha256")).is_none());
        assert!(parse_digest(&d.replace("sha256:", "sha512:")).is_none());
    }

    #[test]
    fn a_blob_is_stored_once_and_only_with_its_digest() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = Blobs::new(dir.path());
        let digest = parse_digest(&digest_of(b"light")).unwrap();
        assert!(matches!(
            blobs.put(&digest, b"other"),
            Err(PutError::Mismatch { .. })
        ));
        assert!(!blobs.contains(&digest));
        assert!(!blobs.put(&digest, b"light").unwrap());
        assert!(blobs.put(&digest, b"light").unwrap());
        assert_eq!(blobs.get(&digest).unwrap(), b"light");
        assert!(
            blobs
                .path(&digest)
                .starts_with(dir.path().join("artifacts/sha256"))
        );
    }
}
