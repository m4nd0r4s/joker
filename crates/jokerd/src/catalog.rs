//! The drivers, profiles and boards the daemon knows, as the manifest
//! checks see them (`Context`, ref/01 §8.1).
//!
//! This is the M0 catalogue: `sim`, `sysfs`, `modbus-rtu`, `modbus-tcp`
//! (device keys of ref/01 appendix A: `address = "<host>:<port>"` and
//! `unit_id`; a TCP device without a shared bus needs no `bus`, ADR 0049),
//! `gpio`, `i2c` and `revpi` (ADR 0078), the bench profile `local/bench-esp32@1` and the bench
//! boards (docs/26 §6, docs/23 §1). The driver crates will
//! describe themselves once `joker-hal` lands; this module then only merges
//! their descriptions.
//!
//! The classes are those of `std/` this runtime embeds and, for a project
//! or a generation, the canonical contracts of its own classes
//! (`contracts/<package>/<name>@<version>.contract.json`, docs/17 §6.1),
//! which `jk` compiles from `classes/` and sends with the generation.

use std::collections::BTreeMap;
use std::path::Path;

use joker_manifest::Context;
use joker_manifest::context::Direction;
use joker_manifest::context::{
    BoardInfo, ClassInfo, DriverInfo, ProfileInfo, SignalInfo, SignalType,
};
use joker_manifest::model::BusKind;
use joker_units::Unit;

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
        "revpi".to_owned(),
        DriverInfo {
            profile_required: false,
            address: [("var".to_owned(), None)].into_iter().collect(),
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

/// The canonical file of the bench profile (docs/23 §4), with the
/// `[implements]` of track part 7 §4 that need no adapter.
const BENCH_ESP32_PROFILE: &str = include_str!("../profiles/local/bench-esp32@1.profile.toml");

/// The M0 profiles. The bench ESP32 is an implicit `std/led@1` and
/// `std/button@1` (track part 7, §4); its `analog_in@1` needs the `scale`
/// adapter, which this runtime does not run yet. The profile goes through
/// the same parser as a project's own profile files, so every signal keeps
/// its address and type; a diagnostic of that parse is returned in the
/// second element, as for a project's own file, never dropped.
pub fn profiles() -> (BTreeMap<String, ProfileInfo>, Vec<String>) {
    let files: CatalogFiles = [(
        "profiles/local/bench-esp32@1.profile.toml".to_owned(),
        BENCH_ESP32_PROFILE.to_owned(),
    )]
    .into_iter()
    .collect();
    let (profiles, _, _, errors) = parse_catalog(&files);
    (profiles, errors)
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

/// The classes of `std/` this runtime embeds, as the manifest checks see
/// them, by reference (`std/led@1`).
pub fn std_classes() -> BTreeMap<String, ClassInfo> {
    joker_jkc::stdlib::contracts()
        .iter()
        .map(|(reference, file)| (reference.clone(), joker_jkc::info::class_info(file)))
        .collect()
}

/// The class contracts a generation runs with, by reference: those of
/// `std/` and those `files` carries (`contracts/…`), whose hashes are
/// verified; a file that does not read back is left out (the checks
/// already reported it).
pub fn contracts_for_files(
    files: &CatalogFiles,
) -> BTreeMap<String, joker_jkc::contract::ContractFile> {
    let mut out = joker_jkc::stdlib::contracts();
    for (name, text) in files {
        if name.starts_with("contracts/")
            && let Ok(file) = joker_jkc::contract::ContractFile::from_json(text)
        {
            out.insert(file.contract.reference(), file);
        }
    }
    out
}

/// The context of the manifest checks: the M0 catalogue, the classes of
/// `std/`, plus the apps the task providers can run. A diagnostic of the
/// M0 catalogue's own files is returned in the second element.
pub fn context(tasks: &dyn TaskProvider) -> (Context, Vec<String>) {
    let (profiles, errors) = profiles();
    let context = Context {
        drivers: drivers(),
        profiles,
        boards: boards(),
        apps: tasks.interfaces(),
        classes: std_classes(),
        ..Context::default()
    };
    (context, errors)
}

/// The profile and board files of a project or of a generation, by their
/// path relative to its root (`profiles/acme/pcs@1.profile.toml`,
/// `boards/rpi5.board.toml`, docs/15 §2), with their text.
pub type CatalogFiles = BTreeMap<String, String>;

/// The media type of a profile file (ref/00 §7.1).
pub const PROFILE_KIND: &str = "joker.profile.v1+toml";

/// The media type of a board file (ref/00 §7.1).
pub const BOARD_KIND: &str = "joker.board.v1+toml";

/// The media type of a class contract (ref/00 §7.1, docs/15).
pub const CONTRACT_KIND: &str = "joker.class.contract.v1+json";

/// The media type of the catalog file `name` (`profiles/…`, `boards/…`
/// or `contracts/…`).
pub fn kind_of(name: &str) -> &'static str {
    if name.starts_with("boards/") {
        BOARD_KIND
    } else if name.starts_with("contracts/") {
        CONTRACT_KIND
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
            ["contracts", .., file] if segments.len() >= 3 => {
                file.ends_with(".contract.json") && file.contains('@')
            }
            _ => false,
        }
}

/// The profiles, boards and classes of `files`, keyed as the manifest
/// refers to them (`acme/pcs@1`, `rpi5`, `local/tank@1`); a file that does
/// not parse is reported in the fourth element and left out, and does not
/// stop the others.
type Parsed = (
    BTreeMap<String, ProfileInfo>,
    BTreeMap<String, BoardInfo>,
    BTreeMap<String, ClassInfo>,
    Vec<String>,
);

fn parse_catalog(files: &CatalogFiles) -> Parsed {
    let mut profiles = BTreeMap::new();
    let mut boards = BTreeMap::new();
    let mut classes = BTreeMap::new();
    let mut errors = Vec::new();
    for (name, text) in files {
        if name.starts_with("contracts/") {
            match joker_jkc::contract::ContractFile::from_json(text) {
                Ok(file) => {
                    classes.insert(
                        file.contract.reference(),
                        joker_jkc::info::class_info(&file),
                    );
                }
                Err(e) => errors.push(format!("{name}: {e}")),
            }
            continue;
        }
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
    (profiles, boards, classes, errors)
}

/// The `[gpio]` of board `name` (ref/03 §2.3.2), from its file among
/// `files` (`boards/<name>.board.toml`), when it declares one.
pub fn board_gpio(files: &CatalogFiles, name: &str) -> Option<joker_manifest::board::Gpio> {
    files.iter().find_map(|(file, text)| {
        let key = file.strip_prefix("boards/")?.strip_suffix(".board.toml")?;
        if key != name {
            return None;
        }
        joker_manifest::board::parse(Path::new(file), text)
            .peripherals
            .gpio
    })
}

/// The context of the manifest checks: the M0 catalogue, the apps of
/// `tasks`, and every profile and board of `files` (a file of `files`
/// wins over the M0 catalogue on a name collision). Files that do not
/// parse are reported in the second element and left out.
pub fn context_for_files(tasks: &dyn TaskProvider, files: &CatalogFiles) -> (Context, Vec<String>) {
    let (mut context, mut errors) = context(tasks);
    let (profiles, boards, classes, file_errors) = parse_catalog(files);
    context.profiles.extend(profiles);
    context.boards.extend(boards);
    context.classes.extend(classes);
    errors.extend(file_errors);
    (context, errors)
}

/// The canonical contracts of the project's own classes: every
/// `classes/**/*.jkc` under `root` (except `classes/std/`, which this
/// runtime embeds), compiled with `std/`, keyed
/// `contracts/<package>/<name>@<version>.contract.json`. The diagnostics
/// of a class that does not compile are returned in the second element.
pub fn project_contracts(root: &Path) -> (CatalogFiles, Vec<String>) {
    let mut sources = Vec::new();
    let mut errors = Vec::new();
    for path in toml_files(&root.join("classes"), ".jkc") {
        if path.starts_with(root.join("classes/std")) {
            continue;
        }
        let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        match std::fs::read_to_string(&path) {
            Ok(text) => sources.push((rel, text)),
            Err(_) => errors.push(format!("{}: could not read", path.display())),
        }
    }
    let mut files = CatalogFiles::new();
    if sources.is_empty() {
        return (files, errors);
    }
    let mut all = joker_jkc::stdlib::sources();
    all.extend(sources);
    let compilation = joker_jkc::compile(all);
    if !compilation.diagnostics.is_empty() {
        errors.push(compilation.render());
    }
    for file in compilation.classes.values() {
        if file.contract.package() == "std" {
            continue;
        }
        files.insert(
            format!("contracts/{}/{}", file.contract.package(), file.file_name()),
            file.to_json(),
        );
    }
    (files, errors)
}

/// Every profile and board file under `root` (`profiles/**.profile.toml`,
/// `boards/*.board.toml`), keyed by its path relative to `root`, and the
/// [`project_contracts`] of its classes. A file that cannot be read, or a
/// class that does not compile, is reported in the second element.
pub fn catalog_files(root: &Path) -> (CatalogFiles, Vec<String>) {
    let (mut files, mut errors) = project_contracts(root);
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
    let (profiles, _, _, parse_errors) = parse_catalog(&files);
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
    let (_, boards, _, parse_errors) = parse_catalog(&files);
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
    use joker_units::Duration;

    #[test]
    fn the_builtin_bench_profile_addresses_every_signal() {
        let (profiles, errors) = profiles();
        assert!(errors.is_empty(), "{errors:?}");
        let bench = &profiles["local/bench-esp32@1"];
        assert_eq!(bench.signals.len(), 7);
        for (name, info) in &bench.signals {
            assert!(info.type_name.is_some(), "{name}: no type");
            assert!(info.raw.contains_key("at"), "{name}: no address");
        }
        assert!(bench.fallback);
        assert_eq!(bench.fallback_timeout, Some(Duration::from_secs(2)));
        assert_eq!(bench.poll, Some(Duration::from_millis(100)));
        assert!(bench.implements.contains_key("std/led@1"));
        assert!(bench.implements.contains_key("std/button@1"));
    }

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

    const MANIFEST: &str = "[project]\nname = \"t\"\njoker = \"1.0\"\nmode = \"dev\"\n\n[devices.pi]\ndriver = \"sim\"\n\n[devices.pi.signals]\nact_led = { type = \"bool\", direction = \"output\" }\nlevel = { type = \"f64\", unit = \"m\" }\n\n[outputs.\"pi.act_led\"]\nsafe = false\n";

    fn check(dir: &Path, instances: &str) -> joker_manifest::Report {
        std::fs::write(dir.join("joker.toml"), format!("{MANIFEST}\n{instances}")).unwrap();
        let (context, errors) = context_for_project(&crate::native::NativeTasks, dir);
        assert!(errors.is_empty(), "{errors:?}");
        joker_manifest::check(
            &joker_manifest::OsFileSystem,
            &dir.join("joker.toml"),
            &context,
        )
    }

    #[test]
    fn a_composite_instance_of_a_std_class_checks_against_its_contract() {
        let dir = tempfile::tempdir().unwrap();
        let ok = check(
            dir.path(),
            "[instances.led_pi]\nclass = \"std/led@1\"\nmap = { on = \"pi.act_led\" }\n",
        );
        assert!(!ok.has_errors(), "{}", ok.render());
        let unbound = check(
            dir.path(),
            "[instances.led_pi]\nclass = \"led@1\"\nmap = {}\n",
        );
        assert!(unbound.codes().contains(&"J0601"), "{}", unbound.render());
        let unknown = check(
            dir.path(),
            "[instances.x]\nclass = \"std/nothing@1\"\nmap = {}\n",
        );
        assert!(unknown.codes().contains(&"J0193"), "{}", unknown.render());
    }

    #[test]
    fn a_project_class_travels_as_its_canonical_contract() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("classes/local")).unwrap();
        std::fs::write(
            dir.path().join("classes/local/tank.jkc"),
            "package local\n\n/// A tank.\nclass tank@1.2.0 {\n    in level: f64 m\n    query low: bool = level < 0.5 m\n}\n",
        )
        .unwrap();
        let (files, errors) = catalog_files(dir.path());
        assert!(errors.is_empty(), "{errors:?}");
        let name = "contracts/local/tank@1.2.0.contract.json";
        assert!(is_catalog_path(name));
        assert_eq!(kind_of(name), CONTRACT_KIND);
        assert!(files.contains_key(name), "{:?}", files.keys());
        let ok = check(
            dir.path(),
            "[instances.t1]\nclass = \"local/tank@1\"\nmap = { level = \"pi.level\" }\n",
        );
        assert!(!ok.has_errors(), "{}", ok.render());
        std::fs::write(
            dir.path().join("classes/local/tank.jkc"),
            "package local\nclass tank@1.0.0 { out x: f64 }\n",
        )
        .unwrap();
        let (_, errors) = catalog_files(dir.path());
        assert!(errors.iter().any(|e| e.contains("JC0107")), "{errors:?}");
    }

    const LAMP: &str = "[profile]\nname = \"acme/lamp\"\nversion = \"1.0.0\"\ndriver = \"modbus\"\n\n[signals]\nrelay = { at = 0, fc = 5, type = \"bool\", direction = \"output\" }\nfb = { at = 0, fc = 1, type = \"bool\" }\nt = { at = 0, fc = 4, type = \"i16\", unit = \"K\" }\n";

    fn lamp(dir: &Path, implements: &str) -> joker_manifest::Report {
        std::fs::create_dir_all(dir.join("profiles/acme")).unwrap();
        std::fs::write(
            dir.join("profiles/acme/lamp@1.profile.toml"),
            format!("{LAMP}\n{implements}"),
        )
        .unwrap();
        let device = "[devices.l1]\ndriver = \"modbus-tcp\"\naddress = \"10.0.0.9:502\"\nunit_id = 1\nprofile = \"acme/lamp@1\"\n\n[outputs.\"l1.relay\"]\nsafe = false\n";
        std::fs::write(dir.join("joker.toml"), format!("{MANIFEST}\n{device}")).unwrap();
        let (context, _) = context_for_project(&crate::native::NativeTasks, dir);
        joker_manifest::check(
            &joker_manifest::OsFileSystem,
            &dir.join("joker.toml"),
            &context,
        )
    }

    #[test]
    fn a_profile_with_implements_makes_the_device_an_implicit_instance() {
        let dir = tempfile::tempdir().unwrap();
        let ok = lamp(
            dir.path(),
            "[implements.\"std/led@1\"]\nmap = { on = \"relay\" }\n\n[implements.\"temp_sensor@1\"]\nmap = { temperature = \"t\" }\n",
        );
        assert!(!ok.has_errors(), "{}", ok.render());
        assert_eq!(ok.paths["l1.on"].canonical, "l1.relay");
        assert_eq!(ok.paths["l1.temperature"].canonical, "l1.t");
        for (implements, code) in [
            ("[implements.\"std/led@1\"]\nmap = {}\n", "J0601"),
            (
                "[implements.\"std/led@1\"]\nmap = { on = \"fb\" }\n",
                "J0824",
            ),
            (
                "[implements.\"std/led@1\"]\nmap = { on = \"relay\", brightness = \"relay\" }\n",
                "J0824",
            ),
            (
                "[implements.\"std/led@1\"]\nmap = { on = \"nope\" }\n",
                "J0101",
            ),
            (
                "[implements.\"std/led@1\"]\nmap = { on = \"const:true\" }\n",
                "J0603",
            ),
            ("[implements.\"std/nothing@1\"]\nmap = {}\n", "J0823"),
            (
                "[implements.\"std/temp_sensor@1\"]\nmap = { temperature = { signal = \"t\", scale = 0.1 } }\n",
                "J0807",
            ),
            (
                "[implements.\"std/pressure_sensor@1\"]\nmap = { pressure = \"t\" }\n",
                "J0107",
            ),
        ] {
            let report = lamp(dir.path(), implements);
            assert!(
                report.codes().contains(&code),
                "{code}: {}",
                report.render()
            );
        }
    }

    #[test]
    fn the_bench_esp32_is_an_implicit_led_and_button() {
        let dir = tempfile::tempdir().unwrap();
        let esp = "[devices.esp]\ndriver = \"modbus-tcp\"\naddress = \"10.0.0.7:502\"\nunit_id = 1\nprofile = \"local/bench-esp32@1\"\n\n[outputs.\"esp.led\"]\nsafe = false\n";
        std::fs::write(dir.path().join("joker.toml"), format!("{MANIFEST}\n{esp}")).unwrap();
        let (context, errors) = context_for_project(&crate::native::NativeTasks, dir.path());
        assert!(errors.is_empty(), "{errors:?}");
        let report = joker_manifest::check(
            &joker_manifest::OsFileSystem,
            &dir.path().join("joker.toml"),
            &context,
        );
        assert!(!report.has_errors(), "{}", report.render());
        assert_eq!(report.paths["esp.on"].canonical, "esp.led");
        assert_eq!(report.paths["esp.pressed"].canonical, "esp.button");
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
