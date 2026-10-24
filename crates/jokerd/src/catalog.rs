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

/// The profile and board files of a project or of a generation, by their
/// path relative to its root (`profiles/acme/pcs@1.profile.toml`,
/// `boards/rpi5.board.toml`, docs/15 §2), with their text.
pub type CatalogFiles = BTreeMap<String, String>;

/// The media type of a profile file (ref/00 §7.1).
pub const PROFILE_KIND: &str = "joker.profile.v1+toml";

/// The media type of a board file (ref/00 §7.1).
pub const BOARD_KIND: &str = "joker.board.v1+toml";

/// The media type of the catalog file `name` (`profiles/…` or `boards/…`).
pub fn kind_of(name: &str) -> &'static str {
    if name.starts_with("boards/") {
        BOARD_KIND
    } else {
        PROFILE_KIND
    }
}

/// Whether `name` is the path of a profile or a board file relative to a
/// project or generation root: `profiles/<vendor>/…/<model>@<major>.profile.toml`
/// or `boards/<name>.board.toml`, `/`-separated, each segment made of
/// `a-z`, `0-9`, `.`, `_`, `@` and `-` and none of them `.` or `..`. A name
/// a generation receives is written under the generation's root, so a
/// path that could leave it is refused here.
pub fn is_catalog_path(name: &str) -> bool {
    let segments: Vec<&str> = name.split('/').collect();
    let well_formed = segments.iter().all(|s| {
        !s.is_empty()
            && *s != "."
            && *s != ".."
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._@-".contains(&b))
    });
    well_formed
        && match segments.as_slice() {
            ["profiles", .., file] if segments.len() >= 3 => {
                file.ends_with(".profile.toml") && file.contains('@')
            }
            ["boards", file] => file.ends_with(".board.toml"),
            _ => false,
        }
}

/// The profiles and boards of `files`, keyed as the manifest refers to
/// them (`acme/pcs@1`, `rpi5`); a file that does not parse is reported in
/// the third element and left out, and does not stop the others.
type Parsed = (
    BTreeMap<String, ProfileInfo>,
    BTreeMap<String, BoardInfo>,
    Vec<String>,
);

fn parse_catalog(files: &CatalogFiles) -> Parsed {
    let mut profiles = BTreeMap::new();
    let mut boards = BTreeMap::new();
    let mut errors = Vec::new();
    for (name, text) in files {
        let path = Path::new(name);
        if let Some(key) = name
            .strip_prefix("profiles/")
            .and_then(|n| n.strip_suffix(".profile.toml"))
        {
            let report = joker_manifest::profile::parse(path, text);
            match report.profile {
                Some(p) => {
                    profiles.insert(key.to_owned(), p);
                }
                None => errors.push(format!("{name}:\n{}", report.render())),
            }
        } else if let Some(key) = name
            .strip_prefix("boards/")
            .and_then(|n| n.strip_suffix(".board.toml"))
        {
            let report = joker_manifest::board::parse(path, text);
            match report.board {
                Some(b) => {
                    boards.insert(key.to_owned(), b);
                }
                None => errors.push(format!("{name}:\n{}", report.render())),
            }
        }
    }
    (profiles, boards, errors)
}

/// The context of the manifest checks: the M0 catalogue, the apps of
/// `tasks`, and every profile and board of `files` (a file of `files`
/// wins over the M0 catalogue on a name collision). Files that do not
/// parse are reported in the second element and left out.
pub fn context_for_files(tasks: &dyn TaskProvider, files: &CatalogFiles) -> (Context, Vec<String>) {
    let mut context = context(tasks);
    let (profiles, boards, errors) = parse_catalog(files);
    context.profiles.extend(profiles);
    context.boards.extend(boards);
    (context, errors)
}

/// Every profile and board file under `root` (`profiles/**.profile.toml`,
/// `boards/*.board.toml`), keyed by its path relative to `root`. A file
/// that cannot be read is reported in the second element.
pub fn catalog_files(root: &Path) -> (CatalogFiles, Vec<String>) {
    let mut files = CatalogFiles::new();
    let mut errors = Vec::new();
    let found = toml_files(&root.join("profiles"), ".profile.toml")
        .into_iter()
        .chain(
            toml_files(&root.join("boards"), ".board.toml")
                .into_iter()
                .filter(|p| p.parent() == Some(root.join("boards").as_path())),
        );
    for path in found {
        let Some(name) = path
            .strip_prefix(root)
            .ok()
            .and_then(|r| r.to_str())
            .map(|r| r.replace(std::path::MAIN_SEPARATOR, "/"))
        else {
            continue;
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                files.insert(name, text);
            }
            Err(_) => errors.push(format!("{}: could not read", path.display())),
        }
    }
    (files, errors)
}

/// [`context_for_files`] over the [`catalog_files`] of the project at
/// `root`: what `jk check` and a project-side `load` see. Errors from
/// files that cannot be read or parsed are returned alongside.
pub fn context_for_project(tasks: &dyn TaskProvider, root: &Path) -> (Context, Vec<String>) {
    let (files, mut errors) = catalog_files(root);
    let (context, parse_errors) = context_for_files(tasks, &files);
    errors.extend(parse_errors);
    (context, errors)
}

/// The profiles of the project at `root`, keyed as the manifest's
/// `profile = "…"` refers to them; files that cannot be read or parsed
/// are reported and left out.
pub fn profiles_from_dir(root: &Path) -> (BTreeMap<String, ProfileInfo>, Vec<String>) {
    let (files, mut errors) = catalog_files(root);
    let (profiles, _, parse_errors) = parse_catalog(&files);
    errors.extend(
        parse_errors
            .into_iter()
            .filter(|e| e.starts_with("profiles/")),
    );
    (profiles, errors)
}

/// The boards of the project at `root`, keyed by file stem; files that
/// cannot be read or parsed are reported and left out.
pub fn boards_from_dir(root: &Path) -> (BTreeMap<String, BoardInfo>, Vec<String>) {
    let (files, mut errors) = catalog_files(root);
    let (_, boards, parse_errors) = parse_catalog(&files);
    errors.extend(
        parse_errors
            .into_iter()
            .filter(|e| e.starts_with("boards/")),
    );
    (boards, errors)
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
