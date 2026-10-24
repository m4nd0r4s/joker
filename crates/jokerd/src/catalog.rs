//! The drivers, profiles and boards the daemon knows, as the manifest
//! checks see them (`Context`, ref/01 §8.1).
//!
//! This is the M0 catalogue: `sim`, `sysfs`, `modbus-rtu`, `modbus-tcp`
//! (device keys of ref/01 appendix A: `address = "<host>:<port>"` and
//! `unit_id`; a TCP device without a shared bus needs no `bus`, ADR 0049),
//! `gpio` and `i2c`, the bench profile `local/bench-esp32@1` and the bench
//! boards (docs/26 §6, docs/23 §1). The driver crates will
//! describe themselves once `joker-hal` lands; this module then only merges
//! their descriptions.

use std::collections::BTreeMap;
use std::path::Path;

use joker_manifest::Context;
use joker_manifest::context::Direction;
use joker_manifest::context::{BoardInfo, DriverInfo, ProfileInfo, SignalInfo, SignalType};
use joker_manifest::model::BusKind;
use joker_units::{Duration, Unit};

use crate::task::TaskProvider;

fn signal(ty: SignalType, direction: Direction, unit: Option<&str>) -> SignalInfo {
    let info = SignalInfo::new(ty, direction);
    match unit.and_then(|u| Unit::parse(u).ok()) {
        Some(u) => info.with_unit(u),
        None => info,
    }
}

/// The M0 drivers.
pub fn drivers() -> BTreeMap<String, DriverInfo> {
    let mut d = BTreeMap::new();
    d.insert("sim".to_owned(), DriverInfo::default());
    d.insert(
        "sysfs".to_owned(),
        DriverInfo {
            address: [
                (
                    "led".to_owned(),
                    Some(signal(SignalType::Bool, Direction::Output, None)),
                ),
                (
                    "thermal".to_owned(),
                    Some(signal(SignalType::Float, Direction::Input, Some("°C"))),
                ),
            ]
            .into_iter()
            .collect(),
            ..DriverInfo::default()
        },
    );
    d.insert(
        "modbus-rtu".to_owned(),
        DriverInfo {
            buses: vec![BusKind::Serial],
            bus_required: true,
            profile_required: true,
            config: ["unit_id".to_owned()].into_iter().collect(),
            ..DriverInfo::default()
        },
    );
    d.insert(
        "modbus-tcp".to_owned(),
        DriverInfo {
            buses: vec![BusKind::Ethernet],
            bus_required: false,
            profile_required: true,
            config: ["unit_id".to_owned(), "address".to_owned()]
                .into_iter()
                .collect(),
            ..DriverInfo::default()
        },
    );
    d.insert(
        "gpio".to_owned(),
        DriverInfo {
            profile_required: false,
            config: ["chip".to_owned()].into_iter().collect(),
            address: [
                ("line".to_owned(), None),
                ("active_low".to_owned(), None),
                ("bias".to_owned(), None),
                ("drive".to_owned(), None),
            ]
            .into_iter()
            .collect(),
            ..DriverInfo::default()
        },
    );
    d.insert(
        "i2c".to_owned(),
        DriverInfo {
            buses: vec![BusKind::I2c],
            bus_required: true,
            profile_required: false,
            config: ["address".to_owned()].into_iter().collect(),
            address: [
                ("reg".to_owned(), None),
                ("len".to_owned(), None),
                ("byte_order".to_owned(), None),
            ]
            .into_iter()
            .collect(),
            ..DriverInfo::default()
        },
    );
    d
}

/// The M0 profiles.
pub fn profiles() -> BTreeMap<String, ProfileInfo> {
    use Direction::{Input, Output, Param};
    use SignalType::{Bool, Integer};
    let signals = [
        ("led", signal(Bool, Output, None)),
        ("led_rb", signal(Bool, Input, None)),
        ("button", signal(Bool, Input, None)),
        ("touch", signal(Integer, Input, None)),
        ("rssi", signal(Integer, Input, None)),
        ("uptime", signal(Integer, Input, Some("s"))),
        ("fw", signal(Integer, Param, None)),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v))
    .collect();
    let mut p = BTreeMap::new();
    p.insert(
        "local/bench-esp32@1".to_owned(),
        ProfileInfo {
            signals,
            fallback: true,
            configurable: false,
            fallback_timeout: Some(Duration::from_secs(2)),
            latency: None,
            quirks: BTreeMap::new(),
            poll: Some(Duration::from_millis(100)),
            timeout: None,
            ..ProfileInfo::default()
        },
    );
    p
}

/// The boards of the development bench (docs/23 §1): the Raspberry Pi 3 B+
/// of M0 and the Pi Zero W.
pub fn boards() -> BTreeMap<String, BoardInfo> {
    let mut b = BTreeMap::new();
    b.insert(
        "rpi3b-plus".to_owned(),
        BoardInfo {
            majors: vec![1],
            arch: "aarch64".into(),
            cpus: Some(4),
            memory: Some(1 << 30),
            ..BoardInfo::default()
        },
    );
    b.insert(
        "rpi-zero-w".to_owned(),
        BoardInfo {
            majors: vec![1],
            arch: "armv6".into(),
            cpus: Some(1),
            memory: Some(512 << 20),
            ..BoardInfo::default()
        },
    );
    b
}

/// The context of the manifest checks: the M0 catalogue plus the apps the
/// task providers can run.
pub fn context(tasks: &dyn TaskProvider) -> Context {
    Context {
        drivers: drivers(),
        profiles: profiles(),
        boards: boards(),
        apps: tasks.interfaces(),
        ..Context::default()
    }
}

/// [`context`], extended with every `.profile.toml`/`.board.toml` found
/// under `root` (`profiles_from_dir`/`boards_from_dir`): a real project's
/// own files win over the M0 bench catalogue on a name collision. Errors
/// from broken files are returned alongside, for the caller to report;
/// they do not stop the daemon from starting.
pub fn context_for_project(tasks: &dyn TaskProvider, root: &Path) -> (Context, Vec<String>) {
    let mut context = context(tasks);
    let (profiles, mut errors) = profiles_from_dir(root);
    context.profiles.extend(profiles);
    let (boards, board_errors) = boards_from_dir(root);
    context.boards.extend(boards);
    errors.extend(board_errors);
    (context, errors)
}

/// `profiles/<vendor>/<model>@<major>.profile.toml` under `root` (docs/15
/// §2), parsed into the registry key the manifest's `profile = "…"` uses.
/// A file that fails to parse is skipped and reported by name; it does not
/// stop the rest of the scan.
pub fn profiles_from_dir(root: &Path) -> (BTreeMap<String, ProfileInfo>, Vec<String>) {
    let mut found = BTreeMap::new();
    let mut errors = Vec::new();
    let dir = root.join("profiles");
    for path in toml_files(&dir, ".profile.toml") {
        let Some(key) = profile_key(&dir, &path) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            errors.push(format!("{}: could not read", path.display()));
            continue;
        };
        let report = joker_manifest::profile::parse(&path, &text);
        match report.profile {
            Some(p) => {
                found.insert(key, p);
            }
            None => errors.push(format!("{}:\n{}", path.display(), report.render())),
        }
    }
    (found, errors)
}

/// `boards/<name>.board.toml` under `root`, parsed into the registry key
/// the manifest's `[target] board = "…"` uses (the file stem).
pub fn boards_from_dir(root: &Path) -> (BTreeMap<String, BoardInfo>, Vec<String>) {
    let mut found = BTreeMap::new();
    let mut errors = Vec::new();
    let dir = root.join("boards");
    for path in toml_files(&dir, ".board.toml") {
        let Some(name) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".board.toml"))
        else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            errors.push(format!("{}: could not read", path.display()));
            continue;
        };
        let report = joker_manifest::board::parse(&path, &text);
        match report.board {
            Some(b) => {
                found.insert(name.to_owned(), b);
            }
            None => errors.push(format!("{}:\n{}", path.display(), report.render())),
        }
    }
    (found, errors)
}

/// Every file under `dir` (recursively) whose name ends with `suffix`.
fn toml_files(dir: &std::path::Path, suffix: &str) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(toml_files(&path, suffix));
        } else if path
            .file_name()
            .is_some_and(|n| n.to_str().is_some_and(|n| n.ends_with(suffix)))
        {
            out.push(path);
        }
    }
    out.sort();
    out
}

/// `profiles/acme/pcs@1.profile.toml` -> `"acme/pcs@1"`.
fn profile_key(profiles_dir: &std::path::Path, file: &std::path::Path) -> Option<String> {
    let relative = file.strip_prefix(profiles_dir).ok()?;
    let text = relative.to_str()?;
    let without_suffix = text.strip_suffix(".profile.toml")?;
    Some(without_suffix.replace(std::path::MAIN_SEPARATOR, "/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_profiles_or_boards_directory_gives_empty_maps() {
        let dir = tempfile::tempdir().unwrap();
        let (profiles, errors) = profiles_from_dir(dir.path());
        assert!(profiles.is_empty());
        assert!(errors.is_empty());
        let (boards, errors) = boards_from_dir(dir.path());
        assert!(boards.is_empty());
        assert!(errors.is_empty());
    }

    #[test]
    fn a_profile_file_is_keyed_by_vendor_model_and_major() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("profiles/acme")).unwrap();
        std::fs::write(
            dir.path().join("profiles/acme/pcs@1.profile.toml"),
            "[profile]\nname = \"acme/pcs\"\nversion = \"1.0.0\"\ndriver = \"modbus\"\n[signals.p_set]\naddress = { at = 0, fc = 6 }\ntype = \"u16\"\ndirection = \"output\"\n",
        )
        .unwrap();
        let (profiles, errors) = profiles_from_dir(dir.path());
        assert!(errors.is_empty(), "{errors:?}");
        assert!(profiles.contains_key("acme/pcs@1"));
    }

    #[test]
    fn a_broken_profile_is_reported_and_does_not_stop_the_scan() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("profiles/acme")).unwrap();
        std::fs::write(
            dir.path().join("profiles/acme/bad@1.profile.toml"),
            "not valid toml {{{",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("profiles/acme/good@1.profile.toml"),
            "[profile]\nname = \"acme/good\"\nversion = \"1.0.0\"\ndriver = \"modbus\"\n",
        )
        .unwrap();
        let (profiles, errors) = profiles_from_dir(dir.path());
        assert_eq!(errors.len(), 1);
        assert!(profiles.contains_key("acme/good@1"));
    }

    #[test]
    fn context_for_project_merges_scanned_profiles_over_the_m0_catalogue() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("profiles/acme")).unwrap();
        std::fs::write(
            dir.path().join("profiles/acme/pcs@1.profile.toml"),
            "[profile]\nname = \"acme/pcs\"\nversion = \"1.0.0\"\ndriver = \"modbus\"\n",
        )
        .unwrap();
        let (context, errors) = context_for_project(&crate::native::NativeTasks, dir.path());
        assert!(errors.is_empty(), "{errors:?}");
        assert!(context.profiles.contains_key("acme/pcs@1"));
        assert!(context.profiles.contains_key("local/bench-esp32@1"));
    }

    #[test]
    fn a_board_file_is_keyed_by_its_stem() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("boards")).unwrap();
        std::fs::write(
            dir.path().join("boards/rpi5.board.toml"),
            "[board]\nname = \"rpi5\"\nversion = \"1.0.0\"\narch = \"aarch64\"\ncpus = 4\nmemory = \"8 GiB\"\n",
        )
        .unwrap();
        let (boards, errors) = boards_from_dir(dir.path());
        assert!(errors.is_empty(), "{errors:?}");
        assert!(boards.contains_key("rpi5"));
    }
}
