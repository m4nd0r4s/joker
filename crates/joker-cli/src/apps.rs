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
