//! The policy checks on this workspace: the CI job of
//! `docs/policies/dependencies.md` §1.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

use joker_supply_chain::{Rule, Violation, Workspace, check_graph, check_toolchains, policy};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn metadata(extra: &[&str]) -> Workspace {
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--locked"])
        .args(extra)
        .arg("--manifest-path")
        .arg(root().join("Cargo.toml"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Workspace::from_json(&String::from_utf8(output.stdout).unwrap()).unwrap()
}

fn report(violations: &[Violation]) -> String {
    violations
        .iter()
        .map(|v| format!("  {v}\n"))
        .collect::<String>()
}

#[test]
fn the_workspace_graph_follows_the_dependency_rules() {
    let ws = metadata(&["--all-features"]);
    let violations = check_graph(&ws);
    assert!(
        violations.is_empty(),
        "the dependency rules of docs/policies/dependencies.md §1 are broken:\n{}",
        report(&violations)
    );
}

#[test]
fn the_rules_see_the_crates_they_are_about() {
    let ws = metadata(&[]);
    for name in ["joker-units", "joker-expr", "joker-core"] {
        assert!(
            ws.member(name).is_some(),
            "{name} is not a workspace member"
        );
    }
}

#[test]
fn every_no_std_crate_of_the_workspace_is_no_std() {
    let ws = metadata(&[]);
    for name in policy::NO_STD {
        if ws.member(name).is_none() {
            continue;
        }
        let lib = root().join("crates").join(name).join("src/lib.rs");
        let text = std::fs::read_to_string(&lib).unwrap();
        assert!(
            text.lines().any(|l| l.trim() == "#![no_std]"),
            "{} has no #![no_std]",
            lib.display()
        );
    }
}

#[test]
fn the_toolchain_files_follow_the_policy() {
    let ws = metadata(&[]);
    let violations = check_toolchains(&root(), &ws).unwrap();
    assert!(
        violations.is_empty(),
        "the toolchain rules of docs/policies/dependencies.md §5 are broken:\n{}",
        report(&violations)
    );
}

fn scratch(name: &str, toolchain: &str, rust_version: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("toolchain-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("crates/extra")).unwrap();
    std::fs::write(
        dir.join("rust-toolchain.toml"),
        format!("[toolchain]\nchannel = \"{toolchain}\"\n"),
    )
    .unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        format!("[workspace.package]\nrust-version = \"{rust_version}\"\n"),
    )
    .unwrap();
    dir
}

fn rules(violations: Vec<Violation>) -> Vec<(Rule, String)> {
    violations
        .into_iter()
        .map(|v| (v.rule, v.subject))
        .collect()
}

#[test]
fn a_pinned_root_toolchain_with_its_minor_passes() {
    let dir = scratch("ok", "1.99.0", "1.99");
    let ws = metadata(&[]);
    assert_eq!(rules(check_toolchains(&dir, &ws).unwrap()), vec![]);
}

#[test]
fn a_floating_channel_or_another_minor_fails() {
    let ws = metadata(&[]);
    let dir = scratch("stable", "stable", "1.99");
    assert_eq!(
        rules(check_toolchains(&dir, &ws).unwrap()),
        vec![
            (Rule::Toolchain, "rust-toolchain.toml".into()),
            (Rule::Toolchain, "Cargo.toml".into()),
        ]
    );
    let dir = scratch("minor", "1.100.0", "1.99");
    assert_eq!(
        rules(check_toolchains(&dir, &ws).unwrap()),
        vec![(Rule::Toolchain, "Cargo.toml".into())]
    );
}

#[test]
fn a_third_toolchain_file_fails() {
    let dir = scratch("extra", "1.99.0", "1.99");
    std::fs::write(
        dir.join("crates/extra/rust-toolchain.toml"),
        "[toolchain]\nchannel = \"nightly\"\n",
    )
    .unwrap();
    let ws = metadata(&[]);
    assert_eq!(
        rules(check_toolchains(&dir, &ws).unwrap()),
        vec![(Rule::Toolchain, "crates/extra/rust-toolchain.toml".into())]
    );
}
