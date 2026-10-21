//! The toolchain rules of document 13 §5.1 and of the policy §5.

use core::fmt;
use std::path::Path;

use crate::policy::{CORE, TOOLCHAIN_FILES, XTENSA_RUST};
use crate::{Error, Rule, Violation, Workspace};

/// A Rust version: `1.99` or `1.99.0`, compared field by field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    /// The major version.
    pub major: u32,
    /// The minor version.
    pub minor: u32,
    /// The patch version, if written.
    pub patch: Option<u32>,
}

impl Version {
    /// Parses `major.minor` or `major.minor.patch`; anything else is `None`.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = match parts.next() {
            Some(p) => Some(p.parse().ok()?),
            None => None,
        };
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            major,
            minor,
            patch,
        })
    }

    /// The `major.minor` part.
    pub fn minor_line(self) -> (u32, u32) {
        (self.major, self.minor)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.patch {
            Some(p) => write!(f, "{}.{}.{p}", self.major, self.minor),
            None => write!(f, "{}.{}", self.major, self.minor),
        }
    }
}

/// Checks the toolchain files under `root` (a checkout) and the
/// `rust-version` of the workspace and of the core crates in `ws`.
pub fn check_toolchains(root: &Path, ws: &Workspace) -> Result<Vec<Violation>, Error> {
    let mut out = Vec::new();
    let mut found = Vec::new();
    find_toolchain_files(root, root, &mut found)?;
    found.sort();
    for file in &found {
        if !TOOLCHAIN_FILES.contains(&file.as_str()) {
            out.push(Violation::new(
                Rule::Toolchain,
                file,
                "only the root and crates/joker-micro/esp/ have a toolchain file (one per family)",
            ));
        }
    }
    let root_file = "rust-toolchain.toml";
    let channel = if found.iter().any(|f| f == root_file) {
        channel(&root.join(root_file))?
    } else {
        out.push(Violation::new(
            Rule::Toolchain,
            root_file,
            "the root toolchain file is missing",
        ));
        None
    };
    let pinned = channel.as_deref().and_then(Version::parse);
    match (&channel, pinned) {
        (_, Some(v)) if v.patch.is_some() => {}
        (c, _) => out.push(Violation::new(
            Rule::Toolchain,
            root_file,
            format!("the channel must be an exact version (x.y.z), found {c:?}"),
        )),
    }
    let workspace = workspace_rust_version(&root.join("Cargo.toml"))?;
    match (workspace.as_deref().and_then(Version::parse), pinned) {
        (Some(w), Some(p)) if w.patch.is_none() && w.minor_line() == p.minor_line() => {}
        (w, p) => out.push(Violation::new(
            Rule::Toolchain,
            "Cargo.toml",
            format!(
                "workspace.package.rust-version must be the minor of the root toolchain: found {:?}, toolchain {:?}",
                w.map(|v| v.to_string()),
                p.map(|v| v.to_string())
            ),
        )),
    }
    let xtensa = Version::parse(XTENSA_RUST)
        .ok_or_else(|| Error::new(format!("XTENSA_RUST is not a version: {XTENSA_RUST}")))?;
    for name in CORE {
        let Some(package) = ws.member(name) else {
            continue;
        };
        match package.rust_version.as_deref().and_then(Version::parse) {
            Some(v) if v.minor_line() <= xtensa.minor_line() => {}
            v => out.push(Violation::new(
                Rule::Toolchain,
                *name,
                format!(
                    "the core MSRV must be set and at most the esp toolchain's Rust ({XTENSA_RUST}), found {:?}",
                    v.map(|v| v.to_string())
                ),
            )),
        }
    }
    Ok(out)
}

const SKIPPED_DIRS: &[&str] = &["target", ".git", ".jj", "node_modules"];

fn find_toolchain_files(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), Error> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| Error::new(format!("cannot read {}: {e}", dir.display())))?;
    for entry in entries {
        let entry = entry.map_err(|e| Error::new(format!("cannot read {}: {e}", dir.display())))?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let kind = entry
            .file_type()
            .map_err(|e| Error::new(format!("cannot stat {}: {e}", path.display())))?;
        if kind.is_dir() {
            if !SKIPPED_DIRS.contains(&name.as_ref()) {
                find_toolchain_files(root, &path, out)?;
            }
        } else if name == "rust-toolchain" || name == "rust-toolchain.toml" {
            let relative = path
                .strip_prefix(root)
                .map_err(|e| Error::new(format!("{} is outside the root: {e}", path.display())))?;
            out.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

fn read_table(path: &Path) -> Result<toml::Table, Error> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| Error::new(format!("cannot read {}: {e}", path.display())))?;
    toml::from_str(&text).map_err(|e| Error::new(format!("{} is not TOML: {e}", path.display())))
}

fn channel(path: &Path) -> Result<Option<String>, Error> {
    let table = read_table(path)?;
    Ok(table
        .get("toolchain")
        .and_then(|t| t.get("channel"))
        .and_then(|c| c.as_str())
        .map(str::to_owned))
}

fn workspace_rust_version(path: &Path) -> Result<Option<String>, Error> {
    let table = read_table(path)?;
    Ok(table
        .get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.get("rust-version"))
        .and_then(|v| v.as_str())
        .map(str::to_owned))
}

#[cfg(test)]
mod tests {
    use super::Version;

    #[test]
    fn versions_parse_and_order() {
        let a = Version::parse("1.99").unwrap();
        let b = Version::parse("1.99.0").unwrap();
        assert_eq!(a.minor_line(), b.minor_line());
        assert!(Version::parse("1.100.0").unwrap().minor_line() > b.minor_line());
        assert_eq!(b.to_string(), "1.99.0");
        assert!(Version::parse("stable").is_none());
        assert!(Version::parse("1.99.0.0").is_none());
        assert!(Version::parse("1").is_none());
    }
}
