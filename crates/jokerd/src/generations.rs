//! Stored generations and their activation history (docs/01 §15, ADR 0015,
//! ref/09 §5.13).
//!
//! A generation is the effective manifest and its lock, identified by the
//! hash of its descriptor (ref/10 §2.6.2) and numbered by creation. Each
//! lives in `<state>/generations/<number>/`; the history of activations is an
//! append-only `activations.jsonl` next to them. A rollback reactivates a
//! stored generation with the same number and hash.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The state of a generation (`generation_state`, ref/00 §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenState {
    /// Prepared, not activated.
    Pending,
    /// Active, waiting for `confirm`.
    Confirming,
    /// Active and confirmed, or confirmed before.
    Confirmed,
    /// Activated and then left without confirmation.
    RolledBack,
    /// The activation failed; the previous generation kept running.
    Failed,
    /// Never activated and dropped.
    Discarded,
}

impl GenState {
    /// The canonical name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Confirming => "confirming",
            Self::Confirmed => "confirmed",
            Self::RolledBack => "rolled_back",
            Self::Failed => "failed",
            Self::Discarded => "discarded",
        }
    }
}

impl fmt::Display for GenState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Where an activation came from (`gen-origin`, ref/09 §5.13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// The local API.
    Local,
    /// A rollback, requested or after an expired window.
    Rollback,
    /// The daemon started and resumed the confirmed generation.
    Boot,
}

/// A stored generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Generation {
    /// Creation number.
    #[serde(rename = "gen")]
    pub number: u64,
    /// `sha256:<hex>` of the descriptor.
    pub hash: String,
    /// State.
    pub state: GenState,
    /// Creation instant, UTC ns.
    pub created_utc_ns: i64,
    /// Message given at creation.
    pub message: Option<String>,
    /// Who created it.
    #[serde(default)]
    pub created_by: Option<String>,
    /// Why it failed or was rolled back.
    pub failure: Option<String>,
    /// The confirmation window of its last activation, in ns.
    pub confirm_window_ns: Option<u64>,
}

/// One activation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activation {
    /// When, UTC ns.
    pub at_utc_ns: i64,
    /// The generation activated.
    #[serde(rename = "gen")]
    pub number: u64,
    /// The generation left.
    pub from: Option<u64>,
    /// Origin.
    pub origin: Origin,
    /// Who asked.
    pub principal: Option<String>,
    /// Why.
    pub reason: Option<String>,
}

/// The generations of a node.
#[derive(Debug)]
pub struct Store {
    dir: PathBuf,
}

const MANIFEST: &str = "joker.toml";
const LOCK: &str = "joker.lock";
const META: &str = "generation.json";
const ACTIVATIONS: &str = "activations.jsonl";
const ARTIFACTS: &str = "artifacts.json";

/// One artifact a generation refers to (`ArtifactRef` of ref/09 §5.13),
/// stored with the generation so that an activation, a rollback or a boot
/// finds the same files (ADR 0067).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenArtifact {
    /// The name the manifest refers to it by (`apps.light`).
    pub name: String,
    /// Media type, short form (`joker.app.v1+wasm`).
    pub kind: String,
    /// `sha256:<hex>` of its bytes.
    pub digest: String,
}

fn utc_now_ns() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

impl Store {
    /// Opens the store under `dir`, creating it.
    pub fn open(dir: &Path) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        Ok(Self {
            dir: dir.to_owned(),
        })
    }

    fn gen_dir(&self, number: u64) -> PathBuf {
        self.dir.join(number.to_string())
    }

    /// Every stored generation, by number.
    pub fn list(&self) -> io::Result<Vec<Generation>> {
        let mut out = Vec::new();
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            if entry
                .file_name()
                .to_str()
                .and_then(|n| n.parse::<u64>().ok())
                .is_none()
            {
                continue;
            }
            if let Ok(text) = fs::read_to_string(entry.path().join(META))
                && let Ok(g) = serde_json::from_str::<Generation>(&text)
            {
                out.push(g);
            }
        }
        out.sort_by_key(|g| g.number);
        Ok(out)
    }

    /// One generation.
    pub fn get(&self, number: u64) -> io::Result<Generation> {
        let text = fs::read_to_string(self.gen_dir(number).join(META))?;
        serde_json::from_str(&text).map_err(io::Error::other)
    }

    /// The manifest and lock of a generation.
    pub fn content(&self, number: u64) -> io::Result<(String, String)> {
        let dir = self.gen_dir(number);
        Ok((
            fs::read_to_string(dir.join(MANIFEST))?,
            fs::read_to_string(dir.join(LOCK)).unwrap_or_default(),
        ))
    }

    /// Stores a new `pending` generation and returns it.
    pub fn create(
        &self,
        hash: &str,
        manifest: &str,
        lock: &str,
        message: Option<String>,
        created_by: Option<String>,
    ) -> io::Result<Generation> {
        let number = self.list()?.last().map_or(1, |g| g.number + 1);
        let dir = self.gen_dir(number);
        fs::create_dir_all(&dir)?;
        write_synced(&dir.join(MANIFEST), manifest.as_bytes())?;
        write_synced(&dir.join(LOCK), lock.as_bytes())?;
        let g = Generation {
            number,
            hash: hash.to_owned(),
            state: GenState::Pending,
            created_utc_ns: utc_now_ns(),
            message,
            created_by,
            failure: None,
            confirm_window_ns: None,
        };
        self.save(&g)?;
        Ok(g)
    }

    /// Stores the artifacts a generation refers to.
    pub fn save_artifacts(&self, number: u64, artifacts: &[GenArtifact]) -> io::Result<()> {
        let text = serde_json::to_string_pretty(artifacts).map_err(io::Error::other)?;
        write_synced(&self.gen_dir(number).join(ARTIFACTS), text.as_bytes())
    }

    /// The artifacts a generation refers to; none for a generation stored
    /// without any.
    pub fn artifacts(&self, number: u64) -> io::Result<Vec<GenArtifact>> {
        match fs::read_to_string(self.gen_dir(number).join(ARTIFACTS)) {
            Ok(text) => serde_json::from_str(&text).map_err(io::Error::other),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    /// Writes a generation's metadata.
    pub fn save(&self, g: &Generation) -> io::Result<()> {
        let text = serde_json::to_string_pretty(g).map_err(io::Error::other)?;
        write_synced(&self.gen_dir(g.number).join(META), text.as_bytes())
    }

    /// Changes the state of a generation.
    pub fn set_state(
        &self,
        number: u64,
        state: GenState,
        failure: Option<String>,
    ) -> io::Result<Generation> {
        let mut g = self.get(number)?;
        g.state = state;
        if failure.is_some() {
            g.failure = failure;
        }
        self.save(&g)?;
        Ok(g)
    }

    /// Appends an activation to the history.
    pub fn record_activation(&self, a: &Activation) -> io::Result<()> {
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join(ACTIVATIONS))?;
        let mut line = serde_json::to_vec(a).map_err(io::Error::other)?;
        line.push(b'\n');
        f.write_all(&line)?;
        f.sync_data()
    }

    /// The activation history, oldest first.
    pub fn activations(&self) -> io::Result<Vec<Activation>> {
        match fs::read_to_string(self.dir.join(ACTIVATIONS)) {
            Ok(text) => text
                .lines()
                .map(|l| serde_json::from_str(l).map_err(io::Error::other))
                .collect(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    /// The newest generation in a state.
    pub fn latest(&self, state: GenState) -> io::Result<Option<Generation>> {
        Ok(self.list()?.into_iter().rev().find(|g| g.state == state))
    }
}

fn write_synced(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    let mut f = fs::File::create(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    fs::rename(&tmp, path)
}

/// One artifact of a descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DescriptorArtifact {
    /// Name as the manifest refers to it.
    pub name: String,
    /// Media type, short form.
    pub media_type: String,
    /// SHA-256.
    pub digest: [u8; 32],
}

/// The hash of a generation descriptor (ref/10 §2.6.2): `manifest_cbor` is
/// the deterministic CBOR of the effective manifest, `lock_digest` the
/// canonical digest of the lock.
pub fn descriptor_hash(
    manifest_cbor: &[u8],
    lock_digest: [u8; 32],
    artifacts: &[DescriptorArtifact],
    dev: bool,
) -> String {
    let mut c = Vec::new();
    c.push(0xA7);
    uint(&mut c, 0, 1);
    uint(&mut c, 0, 1);
    uint(&mut c, 0, 2);
    uint(&mut c, 0, 0);
    uint(&mut c, 0, 3);
    c.extend_from_slice(manifest_cbor);
    uint(&mut c, 0, 4);
    bstr(&mut c, &lock_digest);
    uint(&mut c, 0, 5);
    uint(&mut c, 4, 0);
    uint(&mut c, 0, 6);
    let mut sorted: Vec<&DescriptorArtifact> = artifacts.iter().collect();
    sorted.sort_by(|a, b| a.name.as_bytes().cmp(b.name.as_bytes()));
    uint(&mut c, 4, sorted.len() as u64);
    for a in sorted {
        c.push(0xA3);
        uint(&mut c, 0, 1);
        tstr(&mut c, &a.name);
        uint(&mut c, 0, 2);
        tstr(&mut c, &a.media_type);
        uint(&mut c, 0, 3);
        bstr(&mut c, &a.digest);
    }
    uint(&mut c, 0, 7);
    uint(&mut c, 0, u64::from(!dev));
    format!("sha256:{}", hex(&Sha256::digest(&c)))
}

fn uint(out: &mut Vec<u8>, major: u8, n: u64) {
    let m = major << 5;
    match n {
        0..=23 => out.push(m | n as u8),
        24..=0xFF => out.extend_from_slice(&[m | 24, n as u8]),
        0x100..=0xFFFF => {
            out.push(m | 25);
            out.extend_from_slice(&(n as u16).to_be_bytes());
        }
        0x1_0000..=0xFFFF_FFFF => {
            out.push(m | 26);
            out.extend_from_slice(&(n as u32).to_be_bytes());
        }
        _ => {
            out.push(m | 27);
            out.extend_from_slice(&n.to_be_bytes());
        }
    }
}

fn bstr(out: &mut Vec<u8>, b: &[u8]) {
    uint(out, 2, b.len() as u64);
    out.extend_from_slice(b);
}

fn tstr(out: &mut Vec<u8>, s: &str) {
    uint(out, 3, s.len() as u64);
    out.extend_from_slice(s.as_bytes());
}

/// Lower-case hex.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
