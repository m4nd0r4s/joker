//! `cargo xtask size`: the binary size gate of `jokerd` (docs/01 §21,
//! docs/13 §5, docs/26 §4 E6). Each build of `xtask/size-limits.toml` is
//! measured with `cargo bloat --release --crates` for the target, its
//! binary is stripped, and a build above its limit fails the gate with the
//! crates that weigh the most, and those that grew the most when a
//! baseline is given.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Options of the gate.
#[derive(Debug, Default)]
pub struct Options {
    /// The target triple.
    pub target: String,
    /// The limits file.
    pub limits: PathBuf,
    /// Only these builds; every build of the file when empty.
    pub builds: Vec<String>,
    /// Where to write the JSON result.
    pub json: Option<PathBuf>,
    /// A previous JSON result to compare the crates with.
    pub compare: Option<PathBuf>,
    /// How many crates to list.
    pub top: usize,
}

/// One build of the limits file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Build {
    /// Its name (`minimal`, `full`).
    pub name: String,
    /// The package.
    pub package: String,
    /// The binary.
    pub bin: String,
    /// Cargo features.
    pub features: Vec<String>,
    /// Whether the default features are off.
    pub no_default_features: bool,
    /// The limit of the stripped binary, in bytes.
    pub limit: u64,
}

/// The measurement of one build.
#[derive(Debug, Clone, PartialEq)]
pub struct Measure {
    /// The build.
    pub build: Build,
    /// Size of the stripped binary, in bytes.
    pub stripped: u64,
    /// Size of the binary that `cargo bloat` analysed, in bytes.
    pub file: u64,
    /// Size of the `.text` section, in bytes.
    pub text: u64,
    /// Size per crate in `.text`, largest first.
    pub crates: Vec<(String, u64)>,
}

impl Measure {
    /// Whether the build fits its limit.
    pub fn fits(&self) -> bool {
        self.stripped <= self.build.limit
    }
}

/// Reads the limits file: `schema = 1` and `[[build]]` entries with
/// `name`, `package`, `bin`, `features`, `no_default_features` and
/// `limit_mb` (decimal megabytes, as docs/01 §21 writes them).
pub fn read_limits(path: &Path) -> Result<Vec<Build>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let root: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| format!("{}: {}", path.display(), e.message()))?;
    if root.get("schema").and_then(toml::Value::as_integer) != Some(1) {
        return Err(format!("{}: `schema = 1` is required", path.display()));
    }
    let entries = root
        .get("build")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| format!("{}: no [[build]]", path.display()))?;
    let mut out = Vec::new();
    for entry in entries {
        let t = entry
            .as_table()
            .ok_or_else(|| format!("{}: `build` is an array of tables", path.display()))?;
        let text = |k: &str| {
            t.get(k)
                .and_then(toml::Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| format!("{}: a build needs `{k}`", path.display()))
        };
        let limit_mb = t
            .get("limit_mb")
            .and_then(toml::Value::as_integer)
            .filter(|l| *l > 0)
            .ok_or_else(|| format!("{}: a build needs a positive `limit_mb`", path.display()))?;
        let features = t
            .get("features")
            .and_then(toml::Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(toml::Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        out.push(Build {
            name: text("name")?,
            package: text("package")?,
            bin: text("bin")?,
            features,
            no_default_features: t
                .get("no_default_features")
                .and_then(toml::Value::as_bool)
                .unwrap_or(false),
            limit: limit_mb.unsigned_abs() * 1_000_000,
        });
    }
    Ok(out)
}

/// What `cargo bloat --crates` reports: the file size, the `.text` size and
/// the size per crate, largest first.
pub type Bloat = (u64, u64, Vec<(String, u64)>);

/// Parses the JSON of `cargo bloat --crates --message-format json`.
pub fn parse_bloat(json: &str) -> Result<Bloat, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("cargo bloat output: {e}"))?;
    let file = v["file-size"].as_u64().ok_or("cargo bloat: no file-size")?;
    let text = v["text-section-size"]
        .as_u64()
        .ok_or("cargo bloat: no text-section-size")?;
    let mut crates: Vec<(String, u64)> = v["crates"]
        .as_array()
        .ok_or("cargo bloat: no crates")?
        .iter()
        .filter_map(|c| Some((c["name"].as_str()?.to_owned(), c["size"].as_u64()?)))
        .collect();
    crates.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Ok((file, text, crates))
}

fn cargo_args(build: &Build, target: &str) -> Vec<String> {
    let mut args = vec![
        "--release".to_owned(),
        "--target".to_owned(),
        target.to_owned(),
        "-p".to_owned(),
        build.package.clone(),
        "--bin".to_owned(),
        build.bin.clone(),
    ];
    if build.no_default_features {
        args.push("--no-default-features".to_owned());
    }
    if !build.features.is_empty() {
        args.push("--features".to_owned());
        args.push(build.features.join(","));
    }
    args
}

fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| PathBuf::from("target"), PathBuf::from)
}

/// Measures one build.
pub fn measure(build: &Build, target: &str) -> Result<Measure, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let out = Command::new(&cargo)
        .arg("bloat")
        .args(cargo_args(build, target))
        .args(["--crates", "-n", "0", "--message-format", "json"])
        .output()
        .map_err(|e| format!("cargo bloat: {e} (cargo install cargo-bloat)"))?;
    if !out.status.success() {
        return Err(format!(
            "cargo bloat for {} failed:\n{}",
            build.name,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let json = stdout
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with('{'))
        .ok_or_else(|| format!("cargo bloat for {} printed no JSON", build.name))?;
    let (file, text, crates) = parse_bloat(json)?;
    let binary = target_dir().join(target).join("release").join(&build.bin);
    let stripped = binary.with_file_name(format!("{}.{}.stripped", build.bin, build.name));
    let strip = std::env::var("STRIP").unwrap_or_else(|_| "strip".to_owned());
    let status = Command::new(&strip)
        .arg("-o")
        .arg(&stripped)
        .arg(&binary)
        .status()
        .map_err(|e| format!("{strip}: {e}"))?;
    if !status.success() {
        return Err(format!("{strip} {} failed", binary.display()));
    }
    let stripped = fs::metadata(&stripped)
        .map_err(|e| format!("{}: {e}", stripped.display()))?
        .len();
    Ok(Measure {
        build: build.clone(),
        stripped,
        file,
        text,
        crates,
    })
}

fn mb(bytes: u64) -> String {
    format!("{:.2} MB", bytes as f64 / 1_000_000.0)
}

/// The growth of every crate against a previous result of the same build,
/// largest first.
pub fn growth(now: &Measure, base: &serde_json::Value) -> Vec<(String, i64)> {
    let previous: BTreeMap<String, i64> = base["builds"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|b| b["name"].as_str() == Some(now.build.name.as_str()))
        .and_then(|b| b["crates"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|c| Some((c["name"].as_str()?.to_owned(), c["size"].as_i64()?)))
        .collect();
    let mut out: Vec<(String, i64)> = now
        .crates
        .iter()
        .map(|(name, size)| {
            let size = i64::try_from(*size).unwrap_or(i64::MAX);
            (
                name.clone(),
                size - previous.get(name).copied().unwrap_or(0),
            )
        })
        .filter(|(_, d)| *d > 0)
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
}

/// The text report of the measurements.
pub fn summary(measures: &[Measure], base: Option<&serde_json::Value>, top: usize) -> String {
    let mut s = String::new();
    for m in measures {
        let verdict = if m.fits() { "ok" } else { "ABOVE THE LIMIT" };
        let _ = writeln!(
            s,
            "{} ({}): {} stripped, limit {}, .text {}: {verdict}",
            m.build.name,
            if m.build.features.is_empty() {
                "default features".to_owned()
            } else {
                m.build.features.join(",")
            },
            mb(m.stripped),
            mb(m.build.limit),
            mb(m.text)
        );
        for (name, size) in m.crates.iter().take(top) {
            let _ = writeln!(s, "  {name:<32} {}", mb(*size));
        }
        if let Some(base) = base {
            let grown = growth(m, base);
            if !grown.is_empty() {
                let _ = writeln!(s, "  grew the most:");
                for (name, d) in grown.iter().take(top) {
                    let _ = writeln!(s, "  {name:<32} +{}", mb(d.unsigned_abs()));
                }
            }
        }
    }
    s
}

/// The JSON form of the measurements.
pub fn json(target: &str, measures: &[Measure]) -> serde_json::Value {
    serde_json::json!({
        "target": target,
        "builds": measures.iter().map(|m| serde_json::json!({
            "name": m.build.name,
            "features": m.build.features,
            "stripped": m.stripped,
            "file": m.file,
            "text": m.text,
            "limit": m.build.limit,
            "fits": m.fits(),
            "crates": m.crates.iter().map(|(n, s)| serde_json::json!({"name": n, "size": s})).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

/// Runs the gate. Returns whether every build fits its limit.
pub fn run(options: &Options) -> Result<bool, String> {
    if options.target.is_empty() {
        return Err("size needs --target TRIPLE".to_owned());
    }
    let builds: Vec<Build> = read_limits(&options.limits)?
        .into_iter()
        .filter(|b| options.builds.is_empty() || options.builds.contains(&b.name))
        .collect();
    if builds.is_empty() {
        return Err("no build selected".to_owned());
    }
    let measures = builds
        .iter()
        .map(|b| measure(b, &options.target))
        .collect::<Result<Vec<_>, _>>()?;
    let base = match &options.compare {
        Some(p) => {
            let text = fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
            Some(serde_json::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))?)
        }
        None => None,
    };
    print!("{}", summary(&measures, base.as_ref(), options.top.max(1)));
    if let Some(path) = &options.json {
        if let Some(dir) = path.parent()
            && !dir.as_os_str().is_empty()
        {
            fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        fs::write(path, format!("{:#}\n", json(&options.target, &measures)))
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(measures.iter().all(Measure::fits))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOAT: &str = r#"{"file-size":21000000,"text-section-size":12000000,"crates":[{"name":"std","size":400000},{"name":"wasmtime","size":5000000},{"name":"jokerd","size":900000}]}"#;

    fn build(limit_mb: u64) -> Build {
        Build {
            name: "full".to_owned(),
            package: "jokerd".to_owned(),
            bin: "jokerd".to_owned(),
            features: Vec::new(),
            no_default_features: false,
            limit: limit_mb * 1_000_000,
        }
    }

    #[test]
    fn the_bloat_json_is_read_largest_crate_first() {
        let (file, text, crates) = parse_bloat(BLOAT).unwrap();
        assert_eq!((file, text), (21_000_000, 12_000_000));
        assert_eq!(crates[0], ("wasmtime".to_owned(), 5_000_000));
        assert_eq!(crates[2], ("std".to_owned(), 400_000));
    }

    #[test]
    fn a_build_above_its_limit_fails_and_lists_the_growers() {
        let (file, text, crates) = parse_bloat(BLOAT).unwrap();
        let m = Measure {
            build: build(20),
            stripped: 20_000_001,
            file,
            text,
            crates,
        };
        assert!(!m.fits());
        let base = serde_json::json!({"builds": [{"name": "full", "crates": [
            {"name": "wasmtime", "size": 4_000_000},
            {"name": "jokerd", "size": 900_000},
        ]}]});
        let grown = growth(&m, &base);
        assert_eq!(
            grown,
            vec![
                ("wasmtime".to_owned(), 1_000_000),
                ("std".to_owned(), 400_000)
            ]
        );
        let text = summary(&[m], Some(&base), 2);
        assert!(text.contains("ABOVE THE LIMIT"), "{text}");
        assert!(text.contains("grew the most"), "{text}");
    }

    #[test]
    fn the_limits_of_the_repository_are_those_of_docs_01() {
        let path = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/size-limits.toml"));
        let builds = read_limits(path).unwrap();
        let limit = |n: &str| builds.iter().find(|b| b.name == n).map(|b| b.limit);
        assert_eq!(limit("full"), Some(30_000_000));
        assert!(limit("minimal").is_none_or(|l| l == 20_000_000));
    }
}
