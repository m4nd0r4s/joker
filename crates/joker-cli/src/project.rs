//! The project on disk: which manifest is the entry (ref/01 §7.7), its
//! checks, its lock, and the effective manifest sent to a node.

use std::path::{Path, PathBuf};

use joker_manifest::value::Value;
use joker_manifest::{Diagnostics, OsFileSystem, Report, Sources};
use jokerd::native::NativeTasks;

/// The entry manifest and its lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The entry layer.
    pub manifest: PathBuf,
    /// Its lock (`joker.lock`, `joker.<variant>.lock`).
    pub lock: PathBuf,
    /// How it was chosen, for the first line of node commands.
    pub from: &'static str,
}

/// Picks the entry manifest: `--env` and `--manifest` (or `JK_ENV`,
/// `JK_MANIFEST`) are one choice; both is a usage error (ref/01 §7.7).
pub fn entry(env: Option<&str>, manifest: Option<&Path>) -> Result<Entry, String> {
    let (manifest, from) = match (env, manifest) {
        (Some(_), Some(_)) => {
            return Err("`--env` and `--manifest` are the same choice: give one".into());
        }
        (Some(env), None) => (PathBuf::from(format!("joker.{env}.toml")), "--env"),
        (None, Some(m)) => (m.to_owned(), "--manifest"),
        (None, None) => (PathBuf::from("joker.toml"), "default"),
    };
    let lock = manifest.with_extension("lock");
    Ok(Entry {
        manifest,
        lock,
        from,
    })
}

/// Runs the manifest checks with the drivers and apps this build knows.
pub fn check(entry: &Entry) -> Report {
    let context = jokerd::catalog::context(&NativeTasks);
    joker_manifest::check(&OsFileSystem, &entry.manifest, &context)
}

fn to_toml(v: &Value) -> toml::Value {
    match v {
        Value::String(s) => toml::Value::String(s.clone()),
        Value::Integer(i) => toml::Value::Integer(*i),
        Value::Float(f) => toml::Value::Float(*f),
        Value::Bool(b) => toml::Value::Boolean(*b),
        Value::Datetime(d) => d
            .parse()
            .map_or_else(|_| toml::Value::String(d.clone()), toml::Value::Datetime),
        Value::Array(a) => toml::Value::Array(a.iter().map(|n| to_toml(&n.value)).collect()),
        Value::Table(t) => toml::Value::Table(
            t.iter()
                .map(|(k, e)| (k.clone(), to_toml(&e.node.value)))
                .collect(),
        ),
    }
}

/// The effective manifest as TOML: every layer of `entry` merged
/// (ref/01 §7), in manifest syntax, as a generation proposal carries it
/// (ref/09 §5.13). The node checks it again and computes the same
/// canonical form and hash.
pub fn effective_toml(entry: &Entry) -> Result<String, String> {
    let mut sources = Sources::default();
    let mut diags = Diagnostics::default();
    let layers =
        joker_manifest::layers::load(&OsFileSystem, &entry.manifest, &mut sources, &mut diags);
    let merged = joker_manifest::layers::merge(&layers, &mut diags);
    let table: toml::Table = merged
        .iter()
        .map(|(k, e)| (k.clone(), to_toml(&e.node.value)))
        .collect();
    toml::to_string(&table).map_err(|e| e.to_string())
}

/// The lock text, empty when there is none yet.
pub fn lock_text(entry: &Entry) -> String {
    std::fs::read_to_string(&entry.lock).unwrap_or_default()
}

/// The `joker*.toml` files of a project folder, for `jk dev` to watch.
pub fn layers(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("joker") && n.ends_with(".toml"))
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// `jk new`: the bench of the learning track, part 1 (a simulated Pi and the
/// blinking app `light`).
pub fn new_project(dir: &Path, name: &str) -> Result<(), String> {
    if dir.exists() {
        return Err(format!("`{}` already exists", dir.display()));
    }
    let io = |e: std::io::Error| format!("{}: {e}", dir.display());
    std::fs::create_dir_all(dir.join("apps/light")).map_err(io)?;
    std::fs::create_dir_all(dir.join("sim")).map_err(io)?;
    std::fs::write(
        dir.join("joker.toml"),
        format!(
            r#"[project]
name  = "{name}"
joker = "1.0"
mode  = "dev"

[outputs."pi.act_led"]
safe = false

[cycles.main]
period = "20ms"

[apps.light]
path   = "apps/light"
bind   = {{ led = "pi.act_led" }}
params = {{ half_period = "500 ms" }}
cycles = {{ blink = "main" }}
"#
        ),
    )
    .map_err(io)?;
    std::fs::write(
        dir.join("joker.sim.toml"),
        r#"extends = ["joker.toml"]

[devices.pi]
driver = "sim"

[devices.pi.signals]
act_led = { type = "bool", direction = "output" }
"#,
    )
    .map_err(io)?;
    std::fs::write(
        dir.join("apps/light/README.md"),
        "# light\n\nBlinks `led` every `half_period`. Until the Wasm SDK lands, the runtime runs its native `light`.\n",
    )
    .map_err(io)?;
    std::fs::write(dir.join(".gitignore"), "/.joker/dev/\n/.joker/*.sock\n").map_err(io)?;
    Ok(())
}
