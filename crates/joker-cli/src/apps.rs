//! The apps of a project as cycle task providers: each Wasm app is built
//! with cargo for `wasm32-wasip2` and loaded through `joker-host`; the
//! native tasks of `jokerd` stay behind them for the apps that have no
//! component.

use std::path::{Path, PathBuf};
use std::process::Command;

use joker_host::RunMode;
use joker_manifest::value::Value;
use joker_manifest::{Diagnostics, OsFileSystem, Sources};
use jokerd::native::NativeTasks;
use jokerd::task::Providers;
use jokerd::wasm::WasmTasks;

use crate::project::Entry;

/// An app of the effective manifest that runs as a Wasm component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasmApp {
    /// The app name.
    pub name: String,
    /// Its folder, relative to the current directory.
    pub dir: PathBuf,
}

fn root(entry: &Entry) -> &Path {
    entry
        .manifest
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

/// The Wasm apps of the entry's effective manifest: every `[apps.<name>]`
/// with a `path` whose `kind` is not `native` (ref/01 §5.11).
pub fn wasm_apps(entry: &Entry) -> Vec<WasmApp> {
    let mut sources = Sources::default();
    let mut diags = Diagnostics::default();
    let layers =
        joker_manifest::layers::load(&OsFileSystem, &entry.manifest, &mut sources, &mut diags);
    let merged = joker_manifest::layers::merge(&layers, &mut diags);
    let Some(Value::Table(apps)) = merged.get("apps").map(|e| &e.node.value) else {
        return Vec::new();
    };
    apps.iter()
        .filter_map(|(name, e)| {
            let Value::Table(app) = &e.node.value else {
                return None;
            };
            let field = |k: &str| match app.get(k).map(|e| &e.node.value) {
                Some(Value::String(s)) => Some(s.clone()),
                _ => None,
            };
            if field("kind").is_some_and(|k| k == "native") {
                return None;
            }
            let path = field("path")?;
            Some(WasmApp {
                name: name.clone(),
                dir: root(entry).join(path),
            })
        })
        .collect()
}

/// Builds the component of an app folder with cargo (`wasm32-wasip2`,
/// release) and returns its bytes. A folder with no `Cargo.toml` may hold
/// the component as `<name>.wasm`; `None` means the app has no component.
pub fn build(app: &WasmApp) -> Result<Option<Vec<u8>>, String> {
    let manifest = app.dir.join("Cargo.toml");
    if !manifest.is_file() {
        let wasm = app.dir.join(format!("{}.wasm", app.name));
        return match std::fs::read(&wasm) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(_) => Ok(None),
        };
    }
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args([
            "build",
            "--release",
            "--target",
            "wasm32-wasip2",
            "--message-format",
            "json-render-diagnostics",
            "--manifest-path",
        ])
        .arg(&manifest)
        .env_remove("CARGO_BUILD_TARGET")
        .output()
        .map_err(|e| format!("cargo: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "building app `{}` failed:\n{}",
            app.name,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let artifact = stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|m| m["reason"] == "compiler-artifact")
        .flat_map(|m| {
            m["filenames"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|f| f.as_str().map(PathBuf::from))
                .collect::<Vec<_>>()
        })
        .rfind(|f| f.extension().is_some_and(|e| e == "wasm"))
        .ok_or_else(|| format!("building app `{}` produced no .wasm", app.name))?;
    std::fs::read(&artifact)
        .map(Some)
        .map_err(|e| format!("{}: {e}", artifact.display()))
}

/// The task providers of a project: its Wasm apps, built, then the native
/// tasks of `jokerd`.
pub fn providers(entry: &Entry, mode: RunMode) -> Result<Providers, String> {
    let mut wasm = WasmTasks::new(mode).map_err(|e| e.0)?;
    for app in wasm_apps(entry) {
        if let Some(bytes) = build(&app)? {
            let name = wasm.add(&bytes, true).map_err(|e| e.0)?;
            if name != app.name {
                return Err(format!(
                    "the component in `{}` is app `{name}`, not `{}`",
                    app.dir.display(),
                    app.name
                ));
            }
        }
    }
    let mut providers = Providers::default();
    providers.push(Box::new(wasm));
    providers.push(Box::new(NativeTasks));
    Ok(providers)
}

/// The AOT cache of `jk` (ADR 0067): `$XDG_CACHE_HOME/joker/aot`, by
/// default `~/.cache/joker/aot`, with the node's layout
/// (`<engine>/<hex>.cwasm`, ref/02 §2.2.4).
pub fn aot_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|| PathBuf::from(".cache"));
    base.join("joker").join("aot")
}

/// The Linux target triple of a board `arch` (ref/03 §2.3.1, docs/07 §1
/// principle 8).
pub fn arch_triple(arch: &str) -> Option<&'static str> {
    match arch {
        "aarch64" => Some("aarch64-unknown-linux-gnu"),
        "x86_64" => Some("x86_64-unknown-linux-gnu"),
        "armv7" => Some("armv7-unknown-linux-gnueabihf"),
        "armv6" => Some("arm-unknown-linux-gnueabihf"),
        "riscv64" => Some("riscv64gc-unknown-linux-gnu"),
        _ => None,
    }
}

/// The triple of `--target`: a triple as it is, or a board (`rpi3b-plus`,
/// `rpi3b-plus@1`) of the catalogue or of the project's `boards/`.
pub fn target_triple(entry: &Entry, target: &str) -> Result<String, String> {
    let board = target.split('@').next().unwrap_or(target);
    let mut boards = jokerd::catalog::boards();
    boards.extend(jokerd::catalog::boards_from_dir(root(entry)).0);
    match boards.get(board) {
        Some(info) => arch_triple(&info.arch)
            .map(str::to_owned)
            .ok_or_else(|| format!("board `{board}` is `{}`, which runs no Wasm app", info.arch)),
        None if target.contains('-') => Ok(target.to_owned()),
        None => Err(format!("unknown board or triple `{target}`")),
    }
}

/// The triple `jk apply` precompiles for: the one of the `[target] board`
/// of the effective manifest, or, without a board (only in `dev`), this
/// machine's (ADR 0067).
pub fn node_triple(entry: &Entry) -> Result<String, String> {
    let manifest = crate::project::effective_toml(entry)?;
    let board = manifest.parse::<toml::Table>().ok().and_then(|t| {
        t.get("target")
            .and_then(|t| t.get("board"))
            .and_then(toml::Value::as_str)
            .map(str::to_owned)
    });
    match board {
        Some(b) => target_triple(entry, &b),
        None => joker_host::host_triple().map_err(|e| e.to_string()),
    }
}

/// An app built to its component, with its AOT artifact for one triple.
#[derive(Debug, Clone)]
pub struct Built {
    /// The app name.
    pub name: String,
    /// The component.
    pub wasm: Vec<u8>,
    /// The precompiled component, and where it is cached.
    pub cwasm: Option<(Vec<u8>, PathBuf)>,
}

/// Builds every Wasm app of the entry and, for `triple`, precompiles each
/// component into [`aot_dir`] unless it is already there (`jk build
/// --target`, docs/07 §2).
pub fn build_all(entry: &Entry, triple: Option<&str>) -> Result<Vec<Built>, String> {
    let engine = triple
        .map(joker_host::HostEngine::for_target)
        .transpose()
        .map_err(|e| e.to_string())?;
    let cache = joker_host::AotCache::new(aot_dir());
    let mut out = Vec::new();
    for app in wasm_apps(entry) {
        let Some(wasm) = build(&app)? else {
            continue;
        };
        let cwasm = match &engine {
            Some(engine) => {
                let path = cache.path(engine.id(), &joker_host::component::digest(&wasm));
                if !path.is_file() {
                    cache
                        .install(engine, &wasm)
                        .map_err(|e| format!("app `{}`: {e}", app.name))?;
                }
                let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                Some((bytes, path))
            }
            None => None,
        };
        out.push(Built {
            name: app.name,
            wasm,
            cwasm,
        });
    }
    Ok(out)
}

/// `jk build [--target <board|triple>]`: one line per app with the digest
/// of its component and, with a target, where its `.cwasm` is.
pub fn build_command(
    entry: &Entry,
    target: Option<&str>,
    json: bool,
) -> Result<u8, crate::Failure> {
    let triple = target
        .map(|t| target_triple(entry, t))
        .transpose()
        .map_err(crate::Failure::usage)?;
    let built = build_all(entry, triple.as_deref()).map_err(crate::Failure::usage)?;
    let items: Vec<serde_json::Value> = built
        .iter()
        .map(|b| {
            serde_json::json!({
                "app": b.name,
                "digest": jokerd::blobs::digest_of(&b.wasm),
                "size": b.wasm.len(),
                "aot": b.cwasm.as_ref().map(|(_, p)| p.display().to_string()),
            })
        })
        .collect();
    if json {
        println!("{}", serde_json::json!({ "target": triple, "apps": items }));
        return Ok(0);
    }
    for (b, item) in built.iter().zip(&items) {
        println!(
            "{}  {}  {} KiB",
            b.name,
            item["digest"].as_str().unwrap_or(""),
            b.wasm.len().div_ceil(1024)
        );
        if let Some((_, path)) = &b.cwasm {
            println!("  aot  {}", path.display());
        }
    }
    if built.is_empty() {
        println!("no Wasm apps");
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(dir: &Path) -> Entry {
        Entry {
            manifest: dir.join("joker.toml"),
            lock: dir.join("joker.lock"),
            from: "test",
        }
    }

    #[test]
    fn a_board_resolves_to_the_triple_of_its_arch() {
        let dir = tempfile::tempdir().unwrap();
        let e = entry(dir.path());
        assert_eq!(
            target_triple(&e, "rpi3b-plus@1").unwrap(),
            "aarch64-unknown-linux-gnu"
        );
        assert_eq!(
            target_triple(&e, "x86_64-unknown-linux-gnu").unwrap(),
            "x86_64-unknown-linux-gnu"
        );
        assert!(target_triple(&e, "nowhere").is_err());
    }

    #[test]
    fn a_manifest_with_a_board_targets_the_board() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("joker.toml"),
            "[project]\nname = \"x\"\njoker = \"1.0\"\n\n[target]\nboard = \"rpi3b-plus@1\"\n",
        )
        .unwrap();
        assert_eq!(
            node_triple(&entry(dir.path())).unwrap(),
            "aarch64-unknown-linux-gnu"
        );
        std::fs::write(
            dir.path().join("joker.toml"),
            "[project]\nname = \"x\"\njoker = \"1.0\"\nmode = \"dev\"\n",
        )
        .unwrap();
        assert_eq!(
            node_triple(&entry(dir.path())).unwrap(),
            joker_host::host_triple().unwrap()
        );
    }
}
