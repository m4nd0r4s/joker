//! The minimal `joker.lock` (ref/02 §2.1): the canonical form round-trips on
//! the ref/02 §2.1.2 example, the same content gives the same bytes on any
//! host, and the J04xx checks of the first slice.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers fail the test on a broken fixture"
)]

use std::path::{Path, PathBuf};

use joker_manifest::context::{Direction, DriverInfo, ProfileInfo, SignalInfo, SignalType};
use joker_manifest::lock::{
    self, AppPin, DeviceFacts, DriftKind, DriverPin, Lock, Pin, Source, Tier,
};
use joker_manifest::model::{BusKind, Manifest};
use joker_manifest::{Context, MemoryFileSystem};
use regex::Regex;

fn hex(seed: u32) -> String {
    format!("sha256:{}", format!("{seed:08x}").repeat(8))
}

fn example() -> String {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/ref");
    let doc = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("02-"))
        })
        .unwrap();
    let text = std::fs::read_to_string(doc).unwrap();
    let start = text.find("#### 2.1.2").unwrap();
    let block = Regex::new(r"(?s)```toml\n(.*?)```").unwrap();
    let body = block.captures(&text[start..]).unwrap()[1].to_owned();
    let app = Regex::new(r"sha256:9f2c…").unwrap();
    let body = app.replace_all(&body, hex(0x9f2c).as_str()).into_owned();
    let mut seed = 0;
    Regex::new("sha256:…")
        .unwrap()
        .replace_all(&body, |_: &regex::Captures<'_>| {
            seed += 1;
            hex(seed)
        })
        .into_owned()
}

fn subset(text: &str) -> String {
    let mut out = String::new();
    let mut keep = true;
    for line in text.split_inclusive('\n') {
        if let Some(header) = line.strip_prefix('[') {
            let section = header.split(['.', ']']).next().unwrap_or("");
            keep = ["boards", "profiles", "apps", "drivers"].contains(&section);
        }
        if keep {
            out.push_str(line);
        }
    }
    format!("{}\n", out.trim_end())
}

fn read(text: &str) -> (Option<Lock>, Vec<&'static str>) {
    let report = lock::read(Path::new("joker.lock"), text);
    let codes = report.diagnostics.iter().map(|d| d.code).collect();
    (report.lock, codes)
}

#[test]
fn the_ref02_example_round_trips() {
    let text = example();
    let (parsed, codes) = read(&text);
    let parsed = parsed.unwrap();
    assert_eq!(parsed.canonical(), text);
    assert!(codes.is_empty(), "{codes:?}");
    assert_eq!(parsed.board("revpi-connect-5").unwrap().resolved, "1.2.0");
    assert_eq!(
        parsed.profile("joker/sunspec@1").unwrap().source,
        Source::Builtin
    );
    assert_eq!(
        parsed.app("rse_curtail").unwrap().source,
        Source::Path("../../apps/rse-curtail".into())
    );
    assert_eq!(parsed.driver("acme-bms").unwrap().jdp, Some(1));
}

#[test]
fn the_m0_subset_round_trips() {
    let text = subset(&example());
    let (parsed, codes) = read(&text);
    assert!(codes.is_empty(), "{codes:?}");
    assert_eq!(parsed.unwrap().canonical(), text);
}

fn built(reverse: bool) -> Lock {
    let mut lock = Lock::new();
    let mut apps = vec![
        ("luz", Path::new("bancada/apps/luz")),
        ("blink", Path::new("bancada/apps/blink")),
    ];
    if reverse {
        apps.reverse();
    }
    for (name, dir) in apps {
        lock.set_app(
            name,
            &AppPin {
                version: "0.1.0".into(),
                digest: hex(7),
                wit: "joker:runtime@1.0.0".into(),
                source: Source::path(Path::new("bancada"), &PathBuf::from(dir)),
            },
        );
    }
    lock.set_board(
        "rpi3b-plus@1",
        &Pin {
            resolved: "1.0.0".into(),
            digest: hex(1),
            source: Source::Oci {
                reference: "oci://registry.example.com/joker/boards/rpi3b-plus:1.0.0".into(),
                manifest: hex(2),
            },
        },
    );
    lock.set_profile(
        "local/bench-esp32@1",
        &Pin {
            resolved: "1.0.0".into(),
            digest: hex(3),
            source: Source::path(Path::new("bancada"), Path::new("bancada/profiles/local")),
        },
    );
    lock.set_driver(
        "acme-bms",
        &DriverPin {
            tier: Tier::X,
            version: "2.1.0".into(),
            digest: hex(4),
            descriptor: hex(5),
            wit: None,
            jdp: Some(1),
            source: Source::Oci {
                reference: "oci://registry.example.com/joker/drivers/acme-bms:2.1.0".into(),
                manifest: hex(6),
            },
        },
    );
    lock
}

#[test]
fn the_same_content_gives_the_same_bytes() {
    let a = built(false).canonical();
    let b = built(true).canonical();
    assert_eq!(a, b);
    assert_eq!(built(false).digest(), built(true).digest());
    assert!(a.starts_with("# This file is generated by jk. Do not edit by hand.\nversion = 1\n\n[boards.\"rpi3b-plus@1\"]"));
    assert!(a.ends_with(
        "oci = \"sha256:0000000600000006000000060000000600000006000000060000000600000006\"\n"
    ));
    assert!(!a.contains('\\') && !a.contains('\t') && !a.contains("\r"));
    let (parsed, codes) = read(&a);
    assert!(codes.is_empty(), "{codes:?}");
    assert_eq!(parsed.unwrap(), built(false));
}

#[test]
fn a_reformatted_lock_keeps_its_digest() {
    let canonical = built(false).canonical();
    let messy = canonical
        .replace("version = 1\n", "version=1\n")
        .replace(
            "[apps.luz]\nversion = \"0.1.0\"\n",
            "[apps.luz]\nwit = \"joker:runtime@1.0.0\"\nversion = \"0.1.0\"\n",
        )
        .replacen(
            "wit = \"joker:runtime@1.0.0\"\nsource = \"path:apps/luz\"",
            "source = \"path:apps/luz\"",
            1,
        );
    assert_ne!(messy, canonical);
    let (parsed, codes) = read(&messy);
    assert_eq!(codes, ["J0413"]);
    assert_eq!(parsed.unwrap().digest(), built(false).digest());
}

#[test]
fn invalid_locks_are_reported() {
    let base = built(false).canonical();
    let cases: Vec<(String, &str)> = vec![
        (base.replace("version = 1", "version = 2"), "J0412"),
        (base.replace("version = 1\n", ""), "J0412"),
        (format!("{base}<<<<<<< HEAD\n"), "J0414"),
        (
            base.replace(
                "resolved = \"1.0.0\"",
                "resolved = \"1.0.0\"\ncolour = \"red\"",
            ),
            "J0140",
        ),
        (
            format!("{base}\n[models.m]\ndigest = \"{}\"\n", hex(9)),
            "J0490",
        ),
        (
            format!(
                "{base}\n[accepted]\nwiden.\"x.range\" = {{ reason = \"short\", by = \"user:dl\", at = \"2026-09-18T09:02:11Z\" }}\n"
            ),
            "J0442",
        ),
        (base.replace(&hex(1), "sha256:…"), "J0143"),
        (
            base.replace(&format!("oci = \"{}\"\n", hex(2)), ""),
            "J0142",
        ),
        (base.replace("jdp = 1\n", ""), "J0142"),
        (base.replace("tier = \"x\"", "tier = \"y\""), "J0144"),
        (
            base.replace("resolved = \"1.0.0\"", "resolved = \"1.0\""),
            "J0143",
        ),
        (
            base.replace("[boards.\"rpi3b-plus@1\"]", "[boards.\"rpi3b-plus@1\""),
            "J0146",
        ),
        (
            base.replace("jdp = 1\n", "jdp = 1\nwit = \"joker:driver@1.0.0\"\n"),
            "J0145",
        ),
        (
            base.replace("version = 1\n", "version = 1\nnode = \"Lab_1\"\n"),
            "J0150",
        ),
        (
            format!(
                "{base}\n[fallback.pcs]\noutputs = [\"pcs.p_set\"]\ntimeout = \"1h30m\"\nconfigured = true\n"
            ),
            "J0152",
        ),
        (
            base.replace(
                "source = \"oci://registry.example.com/joker/boards/rpi3b-plus:1.0.0\"",
                "source = \"https://x/y\"",
            ),
            "J0157",
        ),
        (
            format!(
                "{base}\n[mcu.m]\ntarget = \"mcu:rp2350\"\nmode = \"crate\"\nruntime = \"1.0.3\"\ncore = \"1.0\"\nlink = \"1.0\"\nfeatures = []\ncapacity = {{ kv_bytes = \"16 KB\" }}\n"
            ),
            "J0159",
        ),
    ];
    for (text, code) in cases {
        let (_, codes) = read(&text);
        assert!(codes.contains(&code), "{code} not in {codes:?}\n{text}");
    }
}

fn manifest(text: &str) -> Manifest {
    let mut context = Context::default();
    context.boards.insert(
        "rpi3b-plus".into(),
        joker_manifest::context::BoardInfo {
            majors: vec![1],
            arch: "aarch64".into(),
            ..joker_manifest::context::BoardInfo::default()
        },
    );
    context.drivers.insert(
        "modbus-tcp".into(),
        DriverInfo {
            buses: vec![BusKind::Ethernet],
            profile_required: true,
            config: ["address".to_owned(), "unit_id".to_owned()]
                .into_iter()
                .collect(),
            ..DriverInfo::default()
        },
    );
    context.profiles.insert(
        "local/bench-esp32@1".into(),
        ProfileInfo {
            signals: [(
                "led".to_owned(),
                SignalInfo::new(SignalType::Bool, Direction::Output),
            )]
            .into_iter()
            .collect(),
            ..ProfileInfo::default()
        },
    );
    let mut fs = MemoryFileSystem::default();
    fs.file("bancada/joker.toml", text).dir("bancada/apps/luz");
    let report = joker_manifest::check(&fs, Path::new("bancada/joker.toml"), &context);
    assert!(!report.has_errors(), "{}", report.render());
    report.manifest.unwrap()
}

const SITE: &str = "[project]\nname = \"bancada\"\nmode = \"dev\"\n[target]\nboard = \"rpi3b-plus@1\"\n[devices.esp]\ndriver = \"modbus-tcp\"\naddress = \"192.168.1.60:502\"\nprofile = \"local/bench-esp32@1\"\n[outputs.\"esp.led\"]\nsafe = false\n[apps.luz]\npath = \"apps/luz\"\n";

#[test]
fn the_lock_follows_the_manifest() {
    let site = manifest(SITE);
    let mut fs = MemoryFileSystem::default();
    fs.dir("bancada/apps/luz")
        .dir("bancada/apps/blink")
        .dir("bancada/profiles/local");
    let lock_dir = Path::new("bancada");
    let codes = |lock: &Lock, m: &Manifest| -> Vec<&'static str> {
        lock::check_manifest(lock, m, &fs, lock_dir, lock_dir)
            .iter()
            .map(|d| d.code)
            .collect()
    };
    let mut lock = built(false);
    let found = codes(&lock, &site);
    assert_eq!(
        found.iter().filter(|c| **c == "J0411").count(),
        2,
        "{found:?}"
    );
    lock.sections
        .get_mut("apps")
        .unwrap()
        .retain(|k, _| k[0] == "luz");
    lock.sections.remove("drivers");
    assert!(codes(&lock, &site).is_empty(), "{:?}", codes(&lock, &site));
    let pinned = manifest(&format!("{SITE}digest = \"{}\"\n", hex(8)));
    assert_eq!(codes(&lock, &pinned), ["J0423"]);
    let mut outside = lock.clone();
    let mut app = outside.app("luz").unwrap();
    app.source = Source::Path("../../elsewhere".into());
    outside.set_app("luz", &app);
    assert_eq!(codes(&outside, &site), ["J0415"]);
    lock.sections.remove("boards");
    assert_eq!(codes(&lock, &site), ["J0411"]);
}

#[test]
fn node_and_runtime_are_checked() {
    let mut lock = Lock::new();
    lock.node = Some("lab".into());
    lock.runtime = Some("1.0.3".into());
    let codes = |node: &str, runtime: &str| -> Vec<&'static str> {
        lock::check_node(&lock, node, runtime)
            .iter()
            .map(|d| d.code)
            .collect()
    };
    assert!(codes("lab", "1.0.9").is_empty());
    assert_eq!(codes("zero", "1.1.0"), ["J0480", "J0470"]);
}

#[test]
fn drift_is_detected_by_kind() {
    let text = format!(
        "# This file is generated by jk. Do not edit by hand.\nversion = 1\n\n[discovered.pcs]\ndriver = \"modbus-tcp\"\nprofile = \"joker/sunspec@1\"\nat = \"2026-09-20T10:14:03Z\"\nvendor = \"ACME\"\nmodel = \"PCS-125\"\nserial = \"A1B2C3\"\nfirmware = \"4.12.0\"\ndigest = \"{}\"\n",
        hex(1)
    );
    let (parsed, codes) = read(&text);
    assert!(codes.is_empty(), "{codes:?}");
    let parsed = parsed.unwrap();
    let same = DeviceFacts {
        vendor: Some("ACME".into()),
        serial: Some("A1B2C3".into()),
        firmware: Some("4.12.0".into()),
        map: Some(hex(1)),
        ..DeviceFacts::default()
    };
    assert!(lock::drift(&parsed, "pcs", &same).is_empty());
    let swapped = DeviceFacts {
        serial: Some("Z9".into()),
        firmware: Some("4.13.0".into()),
        ..same
    };
    let drifts = lock::drift(&parsed, "pcs", &swapped);
    let kinds: Vec<DriftKind> = drifts.iter().map(|d| d.kind).collect();
    assert_eq!(kinds, [DriftKind::Firmware, DriftKind::Serial]);
    assert_eq!(drifts[1].diagnostic().code, "J0401");
    assert!(lock::drift(&parsed, "other", &swapped).is_empty());
}

#[test]
fn node_captured_sections_follow_the_manifest() {
    let mut context = Context::default();
    context.boards.insert(
        "rpi3b-plus".into(),
        joker_manifest::context::BoardInfo {
            majors: vec![1],
            arch: "aarch64".into(),
            ..joker_manifest::context::BoardInfo::default()
        },
    );
    context.drivers.insert(
        "modbus-tcp".into(),
        DriverInfo {
            buses: vec![BusKind::Ethernet],
            profile_required: true,
            config: ["address".to_owned()].into_iter().collect(),
            discover: true,
            ..DriverInfo::default()
        },
    );
    context
        .drivers
        .insert("joker-link".into(), DriverInfo::default());
    context.profiles.insert(
        "local/bench-esp32@1".into(),
        ProfileInfo {
            signals: [(
                "led".to_owned(),
                SignalInfo::new(SignalType::Bool, Direction::Output),
            )]
            .into_iter()
            .collect(),
            fallback: true,
            ..ProfileInfo::default()
        },
    );
    let mut iface = joker_manifest::context::AppInterface::default();
    iface.tasks.insert("blink".into(), None);
    context.apps.insert("luz".into(), iface);
    let text = format!(
        "{SITE}cycles = {{ blink = \"default\" }}\n[devices.mcu]\ndriver = \"joker-link\"\n"
    )
    .replace(
        "profile = \"local/bench-esp32@1\"\n",
        "profile = \"local/bench-esp32@1\"\ndiscover = true\n",
    );
    let mut fs = MemoryFileSystem::default();
    fs.file("bancada/joker.toml", text.as_str())
        .dir("bancada/apps/luz");
    let report = joker_manifest::check(&fs, Path::new("bancada/joker.toml"), &context);
    assert!(!report.has_errors(), "{}", report.render());
    let site = report.manifest.unwrap();
    let target = lock::Target {
        board: Some("rpi3b-plus".into()),
        runtime: Some("1.0.3".into()),
    };
    let codes = |lock: &Lock| -> Vec<&'static str> {
        lock::check_sections(lock, &site, &context, &target)
            .iter()
            .map(|d| d.code)
            .collect()
    };
    let (empty, _) = read(&format!("{}\n", lock::HEADER.to_owned() + "\nversion = 1"));
    let empty = empty.unwrap();
    let mut found = codes(&empty);
    found.sort_unstable();
    assert_eq!(found, ["J0411", "J0430", "J0450", "J0451"]);
    let full = format!(
        "{}\nversion = 1\n\n[apps.luz]\nversion = \"0.1.0\"\ndigest = \"{}\"\nwit = \"joker:runtime@1.0.0\"\nsource = \"path:apps/luz\"\n\n[mcu.mcu]\ntarget = \"mcu:esp32\"\nmode = \"generation\"\nruntime = \"1.0.3\"\ncore = \"1.0\"\nlink = \"1.0\"\nfeatures = [\"gpio\"]\ncapacity = {{ signals = 256 }}\n\n[discovered.esp]\ndriver = \"modbus-tcp\"\nprofile = \"other/profile@1\"\nat = \"2026-09-20T10:14:03Z\"\ndigest = \"{}\"\n\n[fallback.esp]\noutputs = [\"esp.led\"]\ntimeout = \"2s\"\nvalue = false\nconfigured = false\n\n[wcet]\n\"luz.blink\" = {{ p999 = \"38us\", max = \"51us\", samples = 100000, board = \"rpi3b-plus\", runtime = \"1.1.0\", app = \"{}\", measured = \"2026-09-20T10:20:00Z\" }}\n",
        lock::HEADER,
        hex(7),
        hex(8),
        hex(7)
    );
    let (full, read_codes) = read(&full);
    assert!(read_codes.is_empty(), "{read_codes:?}");
    let full = full.unwrap();
    let mut found = codes(&full);
    found.sort_unstable();
    assert_eq!(found, ["J0431", "J0452"]);
    let hello = lock::Hello {
        firmware: None,
        runtime: Some("1.0.4".into()),
        core: Some("1.0".into()),
        link: Some("2.0".into()),
        features: Some(vec!["gpio".into(), "i2c".into()]),
    };
    let mut found: Vec<&str> = lock::check_hello(&full, "mcu", &hello, 1)
        .iter()
        .map(|d| d.code)
        .collect();
    found.sort_unstable();
    assert_eq!(found, ["J0460", "J0460", "J0460", "J0461"]);
    assert_eq!(lock::missing(Path::new("joker.lock")).code, "J0410");
}

#[test]
fn every_lock_code_has_a_case() {
    let literal = Regex::new(r#""(J0\d{3})""#).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = std::fs::read_to_string(root.join("src/lock.rs")).unwrap();
    let code = source.split("#[cfg(test)]").next().unwrap_or("");
    let tests: String = std::fs::read_dir(root.join("tests"))
        .unwrap()
        .map(|e| std::fs::read_to_string(e.unwrap().path()).unwrap())
        .collect();
    let missing: Vec<String> = literal
        .captures_iter(code)
        .map(|c| c[1].to_owned())
        .filter(|c| !tests.contains(&format!("\"{c}\"")))
        .collect();
    assert!(missing.is_empty(), "lock codes without a case: {missing:?}");
}

#[test]
fn the_lock_sits_next_to_its_entry_manifest() {
    assert_eq!(
        lock::path_for(Path::new("joker.toml")),
        PathBuf::from("joker.lock")
    );
    assert_eq!(
        lock::path_for(Path::new("sites/lisboa-01/joker.verao.toml")),
        PathBuf::from("sites/lisboa-01/joker.verao.lock")
    );
    assert_eq!(
        lock::path_for(Path::new("site.toml")),
        PathBuf::from("site.lock")
    );
}
