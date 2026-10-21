//! `cargo xtask msrv`: the MSRV of the core crates is at most the Rust
//! version of the most recent Xtensa toolchain (docs/13 §5.1), so that the
//! core builds for the bench ESP32 from the same source.

use std::process::Command;

use serde_json::Value;

/// The crates of the core whose MSRV follows the Xtensa toolchain
/// (docs/13 §5.1); the ones not in the workspace yet are skipped.
pub const CORE: [&str; 5] = [
    "joker-units",
    "joker-expr",
    "joker-core",
    "joker-link-proto",
    "joker-agent-proto",
];

/// Parses `1.97`, `1.97.0`, `1.97.0-nightly` or `1.97.0.0` into
/// `(major, minor)`.
pub fn parse_version(text: &str) -> Option<(u32, u32)> {
    let mut parts = text.trim().split(['.', '-']);
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

/// Checks the `rust-version` of the core crates in `metadata` (the output
/// of `cargo metadata`) against `max`. Returns one line per crate and
/// whether all of them fit.
pub fn check(metadata: &Value, max: (u32, u32)) -> (Vec<String>, bool) {
    let mut lines = Vec::new();
    let mut ok = true;
    let packages = metadata["packages"].as_array().cloned().unwrap_or_default();
    for name in CORE {
        let Some(package) = packages.iter().find(|p| p["name"] == name) else {
            continue;
        };
        let declared = package["rust_version"].as_str();
        let fits = declared.and_then(parse_version).is_some_and(|v| v <= max);
        ok &= fits;
        lines.push(format!(
            "{name:<18} rust-version {:<8} {}",
            declared.unwrap_or("(none)"),
            if fits {
                "ok"
            } else {
                "ABOVE the Xtensa toolchain"
            }
        ));
    }
    (lines, ok)
}

/// Runs the check against the workspace.
pub fn run(max: &str) -> Result<bool, String> {
    let limit = parse_version(max).ok_or_else(|| format!("--max: invalid version {max:?}"))?;
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let metadata: Value =
        serde_json::from_slice(&output.stdout).map_err(|e| format!("cargo metadata: {e}"))?;
    let (lines, ok) = check(&metadata, limit);
    println!(
        "core MSRV against the Xtensa toolchain (Rust {}.{}):",
        limit.0, limit.1
    );
    for line in lines {
        println!("  {line}");
    }
    Ok(ok)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn versions_parse_from_every_form() {
        assert_eq!(parse_version("1.97"), Some((1, 97)));
        assert_eq!(parse_version("1.97.0-nightly"), Some((1, 97)));
        assert_eq!(parse_version("1.97.0.0"), Some((1, 97)));
        assert_eq!(parse_version("x"), None);
    }

    #[test]
    fn a_core_crate_above_the_toolchain_fails() {
        let meta = json!({"packages": [
            {"name": "joker-core", "rust_version": "1.99"},
            {"name": "joker-units", "rust_version": "1.85"},
            {"name": "jokerd", "rust_version": "1.99"},
        ]});
        let (lines, ok) = check(&meta, (1, 97));
        assert!(!ok);
        assert_eq!(lines.len(), 2);
        assert!(check(&meta, (1, 99)).1);
        let missing = json!({"packages": [{"name": "joker-core", "rust_version": null}]});
        assert!(!check(&missing, (1, 97)).1);
    }
}
