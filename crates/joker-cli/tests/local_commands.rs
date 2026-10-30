//! `jk init`, `jk profile check`, `jk explain <code>`, `jk config set|show`
//! and `jk diff` (docs/07 §2–§3, ref/02 §2.3).

#![allow(clippy::unwrap_used)]

mod common;

use std::path::Path;
use std::process::{Command, Output};

fn jk(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jk"))
        .current_dir(dir)
        .env("JK_CONFIG", dir.join("user-config.toml"))
        .env_remove("JK_NODE")
        .args(args)
        .output()
        .unwrap()
}

fn code(o: &Output) -> i32 {
    o.status.code().unwrap_or(-1)
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn init_makes_a_project_once() {
    let dir = tempfile::tempdir().unwrap();
    let out = jk(dir.path(), &["init", "--name", "bench"]);
    assert_eq!(code(&out), 0, "{}", text(&out));
    assert!(text(&out).contains("next step: jk app new <name>"));
    let manifest = std::fs::read_to_string(dir.path().join("joker.toml")).unwrap();
    assert!(manifest.contains("name  = \"bench\""));
    assert!(manifest.contains("mode  = \"dev\""));
    assert!(dir.path().join("sim").is_dir());
    assert!(dir.path().join(".gitignore").is_file());
    let check = jk(dir.path(), &["check"]);
    assert_eq!(code(&check), 0, "{}", text(&check));
    let again = jk(dir.path(), &["init"]);
    assert_eq!(code(&again), 1, "{}", text(&again));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("joker.toml")).unwrap(),
        manifest
    );
}

#[test]
fn profile_check_validates_without_equipment() {
    let dir = tempfile::tempdir().unwrap();
    let good = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../jokerd/profiles/local/bench-esp32@1.profile.toml");
    let out = jk(dir.path(), &["profile", "check", good.to_str().unwrap()]);
    assert_eq!(code(&out), 0, "{}", text(&out));
    assert!(text(&out).contains("✓ profile"), "{}", text(&out));
    std::fs::write(
        dir.path().join("bad@1.profile.toml"),
        "schema = 1\n\n[profile]\nname = \"local/bad\"\nversion = \"1.0.0\"\ndriver = \"modbus\"\nbogus = 1\n",
    )
    .unwrap();
    let bad = jk(dir.path(), &["profile", "check", "bad@1.profile.toml"]);
    assert_eq!(code(&bad), 2, "{}", text(&bad));
    assert!(text(&bad).contains("J0140"), "{}", text(&bad));
}

#[test]
fn explain_prints_the_page_of_a_code_without_a_node() {
    let dir = tempfile::tempdir().unwrap();
    let out = jk(dir.path(), &["explain", "J0828"]);
    assert_eq!(code(&out), 0, "{}", text(&out));
    assert!(text(&out).starts_with("J0828 · error · "), "{}", text(&out));
}

#[test]
fn config_set_validates_and_keeps_the_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("user-config.toml"),
        "version = 1\n\n[nodes.lab]\naddress = \"ssh://pi@lab.local\"\nca = \"ca.crt\"\n",
    )
    .unwrap();
    let out = jk(
        dir.path(),
        &["config", "set", "nodes.lab.address", "lab.local:7443"],
    );
    assert_eq!(code(&out), 0, "{}", text(&out));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("user-config.toml")).unwrap(),
        "version = 1\n\n[nodes.lab]\naddress = \"lab.local:7443\"\nca = \"ca.crt\"\n"
    );
    let bad = jk(dir.path(), &["config", "set", "nodes.lab.address", "lab"]);
    assert_eq!(code(&bad), 1, "{}", text(&bad));
    let unknown = jk(dir.path(), &["config", "set", "nodes.lab.port", "1"]);
    assert_eq!(code(&unknown), 1, "{}", text(&unknown));
    let project = jk(
        dir.path(),
        &["config", "set", "defaults.env", "lab", "--project"],
    );
    assert_eq!(code(&project), 0, "{}", text(&project));
    let show = jk(dir.path(), &["config", "show", "--origin"]);
    assert_eq!(code(&show), 0, "{}", text(&show));
    let shown = text(&show);
    assert!(
        shown.contains("defaults.env = \"lab\"    # .joker/config.toml"),
        "{shown}"
    );
    assert!(
        shown.contains("nodes.lab.address = \"lab.local:7443\""),
        "{shown}"
    );
}

#[test]
fn diff_compares_manifests_and_generations() {
    let dir = tempfile::tempdir().unwrap();
    let daemon = common::spawn_jokerd(dir.path());
    let node = format!("unix:{}", daemon.socket.display());
    let apply = jk(dir.path(), &["--node", &node, "apply"]);
    assert_eq!(code(&apply), 0, "{}", text(&apply));

    let same = jk(dir.path(), &["--node", &node, "diff"]);
    assert_eq!(code(&same), 0, "{}", text(&same));
    assert!(text(&same).contains("no differences"), "{}", text(&same));

    let manifest = std::fs::read_to_string(dir.path().join("joker.toml")).unwrap();
    std::fs::write(
        dir.path().join("joker.lab.toml"),
        manifest.replace("safe = false", "safe = true"),
    )
    .unwrap();
    let against = jk(dir.path(), &["diff", "--against", "joker.lab.toml"]);
    assert_eq!(code(&against), 0, "{}", text(&against));
    let shown = text(&against);
    assert!(shown.contains("- safe = true"), "{shown}");
    assert!(shown.contains("+ safe = false"), "{shown}");

    let changed = jk(dir.path(), &["--node", &node, "--env", "lab", "diff"]);
    assert_eq!(code(&changed), 0, "{}", text(&changed));
    assert!(
        text(&changed).contains("+ safe = true"),
        "{}",
        text(&changed)
    );

    let effective = jk(dir.path(), &["diff"]);
    assert_eq!(code(&effective), 0, "{}", text(&effective));
    assert!(text(&effective).contains("[project]"));
    common::terminate(daemon);
}
