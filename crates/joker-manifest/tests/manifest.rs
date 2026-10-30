//! End-to-end checks of the M0 manifest subset: the bench manifests of
//! docs/23 §6, the `J0107` output of docs/00 (gate 7 of docs/26 §6), the
//! effective manifest hash, and one case for every code the checker emits.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers fail the test on a broken fixture"
)]

use std::collections::BTreeSet;
use std::path::Path;

use joker_manifest::context::{
    AppInterface, BoardInfo, Direction, DriverInfo, Param, Port, PortKind, ProfileInfo, SignalInfo,
    SignalType,
};
use joker_manifest::model::BusKind;
use joker_manifest::{Context, MemoryFileSystem, Report, check};
use joker_units::{Duration, Unit};
use regex::Regex;

fn unit(symbol: &str) -> Unit {
    Unit::parse(symbol).unwrap()
}

fn signal(ty: SignalType, direction: Direction, symbol: Option<&str>) -> SignalInfo {
    let info = SignalInfo::new(ty, direction);
    match symbol {
        Some(s) => info.with_unit(unit(s)),
        None => info,
    }
}

fn port(kind: PortKind, ty: SignalType, symbol: Option<&str>, optional: bool) -> Port {
    Port {
        kind,
        ty,
        unit: symbol.map(unit),
        optional,
    }
}

fn param(
    ty: SignalType,
    symbol: Option<&str>,
    range: Option<(f64, f64)>,
    has_default: bool,
) -> Param {
    Param {
        ty,
        unit: symbol.map(unit),
        has_default,
        range,
    }
}

fn context() -> Context {
    use Direction::{Input, Output, Param as Par};
    use SignalType::{Bool, Float, Integer};
    let mut c = Context::default();
    c.drivers.insert(
        "sysfs".into(),
        DriverInfo {
            address: [
                ("led".to_owned(), Some(signal(Bool, Input, None))),
                ("thermal".to_owned(), Some(signal(Float, Input, Some("°C")))),
            ]
            .into_iter()
            .collect(),
            ..DriverInfo::default()
        },
    );
    c.drivers.insert(
        "modbus-rtu".into(),
        DriverInfo {
            buses: vec![BusKind::Serial],
            bus_required: true,
            profile_required: true,
            config: ["unit_id".to_owned()].into_iter().collect(),
            ..DriverInfo::default()
        },
    );
    c.drivers.insert(
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
    c.drivers.insert(
        "bacnet".into(),
        DriverInfo {
            profile_required: true,
            config: ["device_id".to_owned()].into_iter().collect(),
            discover: true,
            relinquish: true,
            ..DriverInfo::default()
        },
    );
    c.profiles.insert(
        "local/bench-esp32@1".into(),
        ProfileInfo {
            signals: [
                ("led", signal(Bool, Output, None)),
                ("led_rb", signal(Bool, Input, None)),
                ("button", signal(Bool, Input, None)),
                ("touch", signal(Integer, Input, None)),
                ("rssi", signal(Integer, Input, None)),
                ("uptime", signal(Integer, Input, Some("s"))),
                ("fw", signal(Integer, Par, None)),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
            fallback: true,
            poll: Some(Duration::from_millis(100)),
            ..ProfileInfo::default()
        },
    );
    c.profiles.insert(
        "acme/pcs-125@3".into(),
        ProfileInfo {
            signals: [
                ("p_set", signal(Float, Output, Some("kW"))),
                ("p_set_rb", signal(Float, Input, Some("kW"))),
                ("p_ac", signal(Float, Input, Some("kW"))),
                (
                    "mode",
                    signal(
                        SignalType::Enum {
                            name: "PcsMode".into(),
                            variants: vec!["off".into(), "on".into()],
                        },
                        Output,
                        None,
                    ),
                ),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
            fallback: true,
            ..ProfileInfo::default()
        },
    );
    c.profiles.insert(
        "acme/bms@1".into(),
        ProfileInfo {
            signals: [
                ("soc", signal(Float, Input, Some("%"))),
                ("soh", signal(Float, Input, Some("%"))),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
            ..ProfileInfo::default()
        },
    );
    c.profiles.insert(
        "acme/inv@1".into(),
        ProfileInfo {
            signals: [("derate".to_owned(), signal(Float, Input, Some("%")))]
                .into_iter()
                .collect(),
            ..ProfileInfo::default()
        },
    );
    c.boards.insert(
        "rpi3b-plus".into(),
        BoardInfo {
            majors: vec![1],
            arch: "aarch64".into(),
            ..BoardInfo::default()
        },
    );
    c.boards.insert(
        "rpi-zero-w".into(),
        BoardInfo {
            majors: vec![1],
            arch: "armv6".into(),
            ..BoardInfo::default()
        },
    );
    c.apps.insert(
        "light".into(),
        AppInterface {
            ports: [
                ("led", port(PortKind::Output, Bool, None, false)),
                ("temp", port(PortKind::Input, Float, Some("°C"), false)),
                ("button", port(PortKind::Input, Bool, None, false)),
                ("led_status", port(PortKind::Input, Bool, None, false)),
                ("adjust", port(PortKind::Input, Float, Some("1"), false)),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
            params: [
                ("half_period", param(Float, Some("ms"), None, true)),
                ("t_cold", param(Float, Some("°C"), None, true)),
                ("t_hot", param(Float, Some("°C"), None, true)),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
            tasks: [("blink".to_owned(), Some(Duration::from_millis(20)))]
                .into_iter()
                .collect(),
            ..AppInterface::default()
        },
    );
    c.apps.insert(
        "peak".into(),
        AppInterface {
            ports: [
                ("soc", port(PortKind::Input, Float, Some("%"), false)),
                ("battery", port(PortKind::Output, Float, Some("kW"), false)),
                ("grid", port(PortKind::Input, Float, Some("kW"), true)),
                ("all", port(PortKind::InputMany, Float, Some("%"), true)),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
            params: [(
                "limit".to_owned(),
                param(Float, Some("kW"), Some((0.0, 1000.0)), true),
            )]
            .into_iter()
            .collect(),
            tasks: [("control".to_owned(), Some(Duration::from_millis(200)))]
                .into_iter()
                .collect(),
            ..AppInterface::default()
        },
    );
    c.absent_drivers.insert("revpi".into());
    c
}

fn run(files: &[(&str, &str)], entry: &str) -> Report {
    let mut fs = MemoryFileSystem::default();
    for (path, text) in files {
        fs.file(*path, *text);
    }
    fs.dir("apps/peak")
        .dir("apps/light")
        .dir("bench/apps/light");
    check(&fs, Path::new(entry), &context())
}

fn bench_files() -> Vec<(String, String)> {
    let doc = std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs"))
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("23-"))
        })
        .unwrap();
    let text = std::fs::read_to_string(doc).unwrap();
    let block = Regex::new(r"(?s)```toml\n# [\w-]+/(joker[\w.-]*\.toml)[^\n]*\n(.*?)```").unwrap();
    block
        .captures_iter(&text)
        .map(|c| (c[1].to_owned(), c[2].to_owned()))
        .collect()
}

#[test]
fn bench_manifests_of_docs_23_pass() {
    let files = bench_files();
    let names: BTreeSet<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
    for expected in [
        "joker.toml",
        "joker.rpi.toml",
        "joker.lab.toml",
        "joker.zero.toml",
    ] {
        assert!(
            names.contains(expected),
            "docs/23 §6 has no `{expected}` block"
        );
    }
    let owned: Vec<(String, String)> = files
        .iter()
        .map(|(n, t)| (format!("bench/{n}"), t.clone()))
        .collect();
    let refs: Vec<(&str, &str)> = owned
        .iter()
        .map(|(n, t)| (n.as_str(), t.as_str()))
        .collect();
    for node in ["lab", "zero"] {
        let report = run(&refs, &format!("bench/joker.{node}.toml"));
        assert!(
            !report.has_errors(),
            "joker.{node}.toml:\n{}",
            report.render()
        );
        assert_eq!(
            report.codes(),
            ["J0205", "J0352"],
            "joker.{node}.toml:\n{}",
            report.render()
        );
        assert!(report.hash.is_some());
        assert!(report.paths.contains_key("esp.led") && report.paths.contains_key("pi.act_led"));
    }
}

#[test]
fn unit_error_of_docs_00_renders_exactly() {
    let mut lines = vec![
        "[project]",
        "name = \"site-lisboa-01\"",
        "joker = \"1.0\"",
        "mode = \"dev\"",
        "",
        "[devices.pcs]",
        "driver = \"modbus-tcp\"",
        "address = \"192.168.10.30:502\"",
        "profile = \"acme/pcs-125@3\"",
        "",
        "[devices.bms]",
        "driver = \"modbus-tcp\"",
        "address = \"192.168.10.31:502\"",
        "profile = \"acme/bms@1\"",
        "",
        "[devices.inv1]",
        "driver = \"modbus-tcp\"",
        "address = \"192.168.10.32:502\"",
        "profile = \"acme/inv@1\"",
        "",
        "[outputs.\"pcs.p_set\"]",
        "safe = \"0 kW\"",
        "",
        "[outputs.\"pcs.mode\"]",
        "safe = \"PcsMode.off\"",
        "",
    ];
    while lines.len() < 38 {
        lines.push("");
    }
    lines.extend([
        "[apps.peak]",
        "path = \"apps/peak\"",
        "bind.battery = \"pcs.p_set\"",
        "bind.soc = \"pcs.p_ac\"",
    ]);
    assert_eq!(lines[41], "bind.soc = \"pcs.p_ac\"");
    let text = lines.join("\n") + "\n";
    let report = run(&[("joker.toml", &text)], "joker.toml");
    let expected = "\
error[J0107]: incompatible unit in binding
  --> joker.toml:42:12
   |
42 | bind.soc = \"pcs.p_ac\"
   |            ^^^^^^^^^^ `pcs.p_ac` is a power (kW); port `soc` expects a fraction (%)
   |
   = help: signals with unit % on this site: bms.soc, bms.soh, inv1.derate
";
    assert_eq!(report.render(), expected);
}

const BASE: &str = "[project]\nname = \"t\"\nmode = \"dev\"\n";
const PCS: &str = "[devices.pcs]\ndriver = \"modbus-tcp\"\naddress = \"10.0.0.1:502\"\nprofile = \"acme/pcs-125@3\"\n";
const MODE_OK: &str = "[outputs.\"pcs.mode\"]\nsafe = \"PcsMode.off\"\n";
const P_SET_OK: &str = "[outputs.\"pcs.p_set\"]\nsafe = \"0 kW\"\n";

fn site(extra: &str) -> String {
    format!("{BASE}{PCS}{MODE_OK}{extra}")
}

fn with_p_set(extra: &str) -> String {
    format!("{BASE}{PCS}{MODE_OK}{P_SET_OK}{extra}")
}

fn peak(bind: &str) -> String {
    with_p_set(&format!(
        "[apps.peak]\npath = \"apps/peak\"\nbind.battery = \"pcs.p_set\"\n{bind}"
    ))
}

/// A case: the code, the files of the site (the entry last) and what it shows.
type Case = (&'static str, Vec<(String, String)>, &'static str);

fn cases() -> Vec<Case> {
    let one = |text: String| vec![("joker.toml".to_owned(), text)];
    let mut apps = with_p_set("");
    for i in 0..51 {
        apps.push_str(&format!("[apps.a{i}]\npath = \"apps/peak\"\n"));
    }
    let mut deep = Vec::new();
    for i in 0..10 {
        deep.push((
            format!("joker.l{i}.toml"),
            format!("extends = [\"joker.l{}.toml\"]\n", i + 1),
        ));
    }
    deep.push(("joker.l10.toml".to_owned(), String::new()));
    deep.push((
        "joker.toml".to_owned(),
        format!("extends = [\"joker.l0.toml\"]\n{BASE}"),
    ));
    vec![
        (
            "J0101",
            one(with_p_set("[outputs.\"pcs.nope\"]\nsafe = 1\n")),
            "envelope on an unknown path",
        ),
        (
            "J0107",
            one(peak("bind.soc = \"pcs.p_ac\"\n")),
            "binding between dimensions",
        ),
        (
            "J0108",
            one(site(
                "[outputs.\"pcs.p_set\"]\nsafe = \"0 kW\"\nrange = [\"-5 kvar\", \"5 kvar\"]\n",
            )),
            "range in the wrong quantity kind",
        ),
        (
            "J0109",
            one(site("[outputs.\"pcs.p_set\"]\nsafe = \"0 kWx\"\n")),
            "unit outside the catalogue",
        ),
        (
            "J0110",
            one(with_p_set("[apps.peak]\npath = \"apps/peak\"\n")),
            "required port not bound",
        ),
        (
            "J0120",
            one(peak(
                "bind.soc = \"pcs.p_ac\"\nparams.limit = \"2000 kW\"\n",
            )),
            "parameter outside its range",
        ),
        ("J0121", one(peak("params.nope = 1\n")), "unknown parameter"),
        (
            "J0140",
            one(format!("{BASE}[project.x]\n[devises]\n")),
            "unknown key",
        ),
        ("J0141", one(format!("{BASE}_x = 1\n")), "reserved key"),
        (
            "J0142",
            one("[project]\nmode = \"dev\"\n".to_owned()),
            "missing project name",
        ),
        (
            "J0143",
            one("[project]\nname = \"t\"\nmode = 1\n".to_owned()),
            "wrong value type",
        ),
        (
            "J0144",
            one("[project]\nname = \"t\"\nmode = \"test\"\n".to_owned()),
            "value outside the set",
        ),
        (
            "J0145",
            one(format!(
                "{BASE}[buses.c]\nkind = \"can\"\niface = \"can0\"\nbitrate = 1\nbaud = 9600\n"
            )),
            "key for another bus kind",
        ),
        ("J0146", one("[project\n".to_owned()), "invalid TOML"),
        (
            "J0147",
            one("[project]\nname = \"t\"\njoker = \"9.0\"\nmode = \"dev\"\n".to_owned()),
            "newer runtime",
        ),
        ("J0149", one(apps), "more than 50 apps"),
        (
            "J0150",
            one(format!("{BASE}[devices.Pcs]\ndriver = \"sysfs\"\n")),
            "invalid name",
        ),
        (
            "J0151",
            one(site("[outputs.\"pcs.p_set\"]\nsafe = 0\n")),
            "number without a unit",
        ),
        (
            "J0152",
            one(format!("{BASE}[cycles.main]\nperiod = \"1h30m\"\n")),
            "compound duration",
        ),
        (
            "J0153",
            one(with_p_set(
                "[devices.pcs.signals]\np_ac = { alias = \"Bad\" }\n",
            )),
            "invalid alias path",
        ),
        (
            "J0154",
            one(with_p_set("[outputs.\"**alarm.**\"]\nsafe = 1\n")),
            "invalid selector",
        ),
        (
            "J0155",
            one(with_p_set("[outputs.\"zz*.p\"]\nsafe = 1\n")),
            "selector without matches",
        ),
        (
            "J0157",
            one(format!(
                "{BASE}[devices.a]\ndriver = \"sysfs\"\nalign = \"ghost\"\n"
            )),
            "align on an unknown device",
        ),
        (
            "J0159",
            one(site(
                "[outputs.\"pcs.p_set\"]\nsafe = \"0 kW\"\nmax_actuations = \"0/h\"\n",
            )),
            "invalid budget",
        ),
        (
            "J0160",
            one(with_p_set(
                "[apps.peak]\npath = \"apps/none\"\nbind.battery = \"pcs.p_set\"\nbind.soc = \"pcs.p_ac\"\n",
            )),
            "missing app folder",
        ),
        (
            "J0161",
            vec![
                ("joker.toml".into(), "extends = [\"joker.a.toml\"]\n".into()),
                ("joker.a.toml".into(), "extends = [\"joker.toml\"]\n".into()),
            ],
            "extends cycle",
        ),
        ("J0162", deep, "extends too deep"),
        (
            "J0163",
            one(format!("{BASE}[x]\ny = {{ unset = true }}\n")),
            "unset of nothing",
        ),
        (
            "J0164",
            one(format!(
                "{BASE}[cycles.main]\nperiod = \"20ms\"\norder = [\"a\", \"a\"]\n"
            )),
            "repeated ordered element",
        ),
        (
            "J0165",
            vec![
                ("joker.base.toml".into(), format!("{BASE}[x]\ny = 1\n")),
                (
                    "joker.toml".into(),
                    "extends = [\"joker.base.toml\"]\n[x.y]\nz = 1\n".into(),
                ),
            ],
            "type change across layers",
        ),
        (
            "J0166",
            one(site(
                "[outputs.\"pcs.p_set\"]\nsafe = \"0 kW\"\nwiden = \"short\"\n",
            )),
            "short justification",
        ),
        (
            "J0169",
            vec![("site.toml".into(), BASE.into())],
            "entry name outside the rule",
        ),
        (
            "J0170",
            one(format!("{BASE}[devices.a]\ndriver = \"nope\"\n")),
            "unknown driver",
        ),
        (
            "J0850",
            one(format!("{BASE}[devices.a]\ndriver = \"revpi\"\n")),
            "driver left out of this build",
        ),
        (
            "J0171",
            one(format!(
                "{BASE}[devices.a]\ndriver = \"modbus-rtu\"\nbus = \"ghost\"\nprofile = \"acme/bms@1\"\n"
            )),
            "unknown bus",
        ),
        (
            "J0172",
            one(format!(
                "{BASE}[devices.a]\ndriver = \"modbus-tcp\"\nprofile = \"acme/none@1\"\n"
            )),
            "unknown profile",
        ),
        (
            "J0173",
            one(format!(
                "{BASE}[devices.a]\ndriver = \"sysfs\"\naddress = \"1\"\n"
            )),
            "key of another driver",
        ),
        (
            "J0174",
            one(with_p_set(
                "[devices.pcs.signals]\np_ac = { alias = \"pcs.p_set\" }\n",
            )),
            "alias collision",
        ),
        (
            "J0176",
            one(with_p_set(
                "[devices.pcs.signals]\np_ac = { direction = \"output\" }\n",
            )),
            "declaration key on a profile signal",
        ),
        (
            "J0177",
            one(with_p_set(
                "[devices.b]\ndriver = \"modbus-tcp\"\naddress = \"10.0.0.9:502\"\nprofile = \"acme/bms@1\"\ndiscover = true\n",
            )),
            "discovery unsupported",
        ),
        (
            "J0178",
            one(with_p_set(
                "[devices.b]\ndriver = \"modbus-tcp\"\naddress = \"10.0.0.1:502\"\nprofile = \"acme/bms@1\"\n",
            )),
            "two devices, one address",
        ),
        (
            "J0180",
            one(format!("{BASE}[apps.peak]\nkind = \"wasm\"\n")),
            "app without origin",
        ),
        (
            "J0181",
            one(peak("bind.soc = [\"bms.soc\"]\n")),
            "list on a scalar port",
        ),
        (
            "J0182",
            one(peak(
                "bind.soc = \"pcs.p_ac\"\ncycles.control = \"ghost\"\n",
            )),
            "unknown group",
        ),
        (
            "J0183",
            one(peak(
                "bind.soc = { signal = \"pcs.p_ac\", as = \"fraction\" }\n",
            )),
            "fraction between non-% units",
        ),
        (
            "J0184",
            one(format!(
                "{BASE}[apps.peak]\npath = \"apps/peak\"\nnet.allow = [\"*.x:1\"]\n"
            )),
            "invalid net.allow",
        ),
        (
            "J0186",
            one(format!(
                "{BASE}[apps.peak]\npath = \"apps/peak\"\nresources.async = \"rt\"\n"
            )),
            "rt for async tasks",
        ),
        (
            "J0187",
            one(format!(
                "{BASE}[apps.peak]\nkind = \"native\"\npath = \"apps/peak\"\n"
            )),
            "native app with a path",
        ),
        (
            "J0201",
            one(site(
                "[outputs.\"pcs.p_set\"]\nrange = [\"0 kW\", \"1 kW\"]\n",
            )),
            "envelope without safe",
        ),
        (
            "J0202",
            one(site(
                "[outputs.\"pcs.p_set\"]\nsafe = \"0 kW\"\nrate = \"5 kW\"\n",
            )),
            "rate without time",
        ),
        (
            "J0203",
            one(site(
                "[outputs.\"pcs.p_set\"]\nsafe = \"10 kW\"\nrange = [\"0 kW\", \"5 kW\"]\n",
            )),
            "safe outside range",
        ),
        (
            "J0207",
            one(site("[outputs.\"pcs.p_set\"]\nsafe = \"relinquish\"\n")),
            "relinquish unsupported",
        ),
        (
            "J0215",
            one(with_p_set("[outputs.\"pcs.p_*\"]\nsafe = \"0 kW\"\n")),
            "two envelopes",
        ),
        (
            "J0230",
            one(site(
                "[outputs.\"pcs.p_set\"]\nsafe = \"0 kW\"\nrange = [\"5 kW\", \"0 kW\"]\n",
            )),
            "range min above max",
        ),
        (
            "J0231",
            one(site("[outputs.\"pcs.p_set\"]\nsafe = true\n")),
            "safe of the wrong type",
        ),
        (
            "J0232",
            one(site("[outputs.\"pcs.p_set\"]\nsafe = \"pcs.p_ac * 2\"\n")),
            "safe expression",
        ),
        (
            "J0233",
            one(format!(
                "{BASE}{PCS}{P_SET_OK}[outputs.\"pcs.mode\"]\nsafe = \"off\"\nrate = \"1 kW/s\"\n"
            )),
            "rate on an enum",
        ),
        (
            "J0234",
            one(with_p_set("[outputs.\"pcs.p_ac\"]\nsafe = \"0 kW\"\n")),
            "envelope on an input",
        ),
        (
            "J0235",
            one(site(
                "[outputs.\"pcs.p_set\"]\nsafe = \"0 kW\"\nverify = { readback = \"pcs.p_set\", tolerance = \"1 kW\" }\n",
            )),
            "readback that is an output",
        ),
        (
            "J0237",
            one(site(
                "[outputs.\"pcs.p_set\"]\nsafe = \"0 kW\"\ngroup = \"ghost\"\n",
            )),
            "unknown group",
        ),
        (
            "J0250",
            one(format!(
                "{BASE}[apps.peak]\npath = \"apps/peak\"\nwriter = {{ level = \"boss\" }}\n"
            )),
            "invalid level",
        ),
        (
            "J0251",
            one(format!(
                "{BASE}[apps.peak]\npath = \"apps/peak\"\nwriter = {{ level = \"optimizer:3\" }}\n"
            )),
            "priority outside the band",
        ),
        (
            "J0252",
            one(format!(
                "{BASE}[apps.peak]\npath = \"apps/peak\"\nwriter = {{ level = \"manual\" }}\n"
            )),
            "app in the manual band",
        ),
        (
            "J0255",
            one(site(
                "[outputs.\"pcs.p_set\"]\nsafe = \"0 kW\"\nmax_lease = \"2d\"\n",
            )),
            "max_lease above 24h",
        ),
        (
            "J0275",
            one(with_p_set(
                "[devices.b]\ndriver = \"modbus-tcp\"\naddress = \"10.0.0.9:502\"\nprofile = \"acme/bms@1\"\nfallback = { configure = true }\n",
            )),
            "configure without profile fallback",
        ),
        (
            "J0280",
            one(format!("{BASE}[cycles.main]\nperiod = \"2h\"\n")),
            "period above 1h",
        ),
        (
            "J0281",
            one(format!(
                "{BASE}[cycles.main]\nperiod = \"20ms\"\norder = [\"ghost\"]\n"
            )),
            "order on an app not in the group",
        ),
        (
            "J0282",
            one(format!(
                "{BASE}[cycles.main]\nperiod = \"20ms\"\nresources = \"io\"\n"
            )),
            "group outside rt",
        ),
        (
            "J0285",
            one(format!(
                "{BASE}[cycles.main]\nperiod = \"20ms\"\ntrace = {{ continuous = \"8d\" }}\n"
            )),
            "trace above 7d",
        ),
        (
            "J0288",
            one(format!("{BASE}[cycles.default_10ms]\nperiod = \"10ms\"\n")),
            "reserved group name",
        ),
        (
            "J0290",
            one(format!("{BASE}[models.m]\nx = 1\n")),
            "reserved section",
        ),
        (
            "J0294",
            one(format!(
                "{BASE}[apps.peak]\npath = \"apps/peak\"\nsecrets.token = \"hunter2\"\n"
            )),
            "plain secret",
        ),
        (
            "J0303",
            one(format!(
                "{BASE}{PCS}{P_SET_OK}[outputs.\"pcs.mode\"]\nsafe = \"PcsMode.nope\"\n"
            )),
            "unknown variant",
        ),
        (
            "J0351",
            one(format!("{BASE}[cycles.main]\nperiod = \"5ms\"\n")),
            "fast group without PREEMPT_RT",
        ),
        (
            "J0352",
            one(format!("{BASE}[cycles.main]\nperiod = \"20ms\"\n")),
            "group without PREEMPT_RT",
        ),
        (
            "J0803",
            one(format!("extends = [\"base.toml\"]\n{BASE}")),
            "extends outside the naming rule",
        ),
    ]
}

#[test]
fn every_code_has_a_case() {
    for (code, files, what) in cases() {
        let refs: Vec<(&str, &str)> = files
            .iter()
            .map(|(n, t)| (n.as_str(), t.as_str()))
            .collect();
        let entry = refs.last().map_or("joker.toml", |(n, _)| *n);
        let report = run(&refs, entry);
        assert!(
            report.codes().contains(&code),
            "{code} ({what}) not reported; got {:?}\n{}",
            report.codes(),
            report.render()
        );
    }
}

/// `lock.rs` codes are covered by `tests/lock.rs`'s own cases, not this
/// manifest-check harness; `csv_import.rs` is a standalone tool (not
/// reachable through `run()`) with its own test suite in
/// `tests/csv_import.rs`.
#[test]
fn the_cases_cover_every_code_the_checker_emits() {
    let literal = Regex::new(r#""(J0\d{3})""#).unwrap();
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut emitted = BTreeSet::new();
    for entry in std::fs::read_dir(src).unwrap() {
        let path = entry.unwrap().path();
        if path
            .file_name()
            .is_some_and(|n| n == "lock.rs" || n == "csv_import.rs")
        {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let code = text.split("#[cfg(test)]").next().unwrap_or("");
        emitted.extend(literal.captures_iter(code).map(|c| c[1].to_owned()));
    }
    emitted.extend(["J0108".to_owned(), "J0109".to_owned()]);
    let marker = Regex::new(r#"\(\s*"(J0\d{3})","#).unwrap();
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut covered: BTreeSet<String> = BTreeSet::new();
    for entry in std::fs::read_dir(tests).unwrap() {
        let text = std::fs::read_to_string(entry.unwrap().path()).unwrap();
        covered.extend(marker.captures_iter(&text).map(|c| c[1].to_owned()));
    }
    let missing: Vec<&String> = emitted.difference(&covered).collect();
    assert!(missing.is_empty(), "codes without a case: {missing:?}");
}

#[test]
fn the_hash_ignores_order_units_layers_and_defaults() {
    let a = "[project]\nname = \"s\"\nmode = \"dev\"\n[devices.pcs]\ndriver = \"modbus-tcp\"\naddress = \"10.0.0.1:502\"\nprofile = \"acme/pcs-125@3\"\n[outputs.\"pcs.p_set\"]\nsafe = \"0 kW\"\nrange = [\"-125 kW\", \"125 kW\"]\n[outputs.\"pcs.mode\"]\nsafe = \"PcsMode.off\"\n";
    let b = "[outputs.\"pcs.mode\"]\nsafe = \"PcsMode.off\"\n[outputs.\"pcs.p_set\"]\nrange = [\"-125000 W\", \"125000 W\"]\nsafe = \"0 W\"\nmax_lease = \"15m\"\n[devices.pcs]\nprofile = \"acme/pcs-125@3\"\naddress = \"10.0.0.1:502\"\ndriver = \"modbus-tcp\"\nretries = 1\n[project]\nmode = \"dev\"\nname = \"s\"\nx_owner = \"team\"\n";
    let base = "[project]\nname = \"s\"\nmode = \"dev\"\n[devices.pcs]\ndriver = \"modbus-tcp\"\naddress = \"10.0.0.1:502\"\nprofile = \"acme/pcs-125@3\"\n";
    let top = "extends = [\"joker.base.toml\"]\n[outputs.\"pcs.p_set\"]\nsafe = \"0 kW\"\nrange = [\"-125 kW\", \"125 kW\"]\n[outputs.\"pcs.mode\"]\nsafe = \"PcsMode.off\"\n";
    let ha = run(&[("joker.toml", a)], "joker.toml").hash.unwrap();
    let hb = run(&[("joker.toml", b)], "joker.toml").hash.unwrap();
    let hc = run(
        &[("joker.base.toml", base), ("joker.toml", top)],
        "joker.toml",
    )
    .hash
    .unwrap();
    assert_eq!(ha, hb);
    assert_eq!(ha, hc);
    let changed = a.replace("\"125 kW\"]", "\"120 kW\"]");
    assert_ne!(
        ha,
        run(&[("joker.toml", &changed)], "joker.toml").hash.unwrap()
    );
    assert!(ha.starts_with("sha256:") && ha.len() == 71);
}

#[test]
fn file_paths_are_rewritten_relative_to_the_root() {
    let base = format!("{BASE}[apps.tool]\npath = \"../apps/peak\"\n");
    let report = run(
        &[
            ("templates/joker.base.toml", &base),
            ("joker.toml", "extends = [\"templates/joker.base.toml\"]\n"),
        ],
        "joker.toml",
    );
    let Some(joker_manifest::effective::Canon::Map(root)) = &report.effective else {
        panic!("{}", report.render());
    };
    let Some(joker_manifest::effective::Canon::Map(apps)) = root.get("apps") else {
        panic!("no apps");
    };
    let Some(joker_manifest::effective::Canon::Map(peak)) = apps.get("tool") else {
        panic!("no tool");
    };
    assert_eq!(
        peak.get("path"),
        Some(&joker_manifest::effective::Canon::Text("apps/peak".into()))
    );
}

#[test]
fn diagnostics_are_ordered_and_stable() {
    let text = site("[outputs.\"pcs.p_set\"]\nsafe = true\nrate = \"5 kW\"\ngroup = \"ghost\"\n");
    let first = run(&[("joker.toml", &text)], "joker.toml").render();
    for _ in 0..5 {
        assert_eq!(run(&[("joker.toml", &text)], "joker.toml").render(), first);
    }
    let codes = run(&[("joker.toml", &text)], "joker.toml").codes();
    assert_eq!(codes, ["J0231", "J0202", "J0237"]);
}

#[test]
fn error_in_prod_codes_are_warnings_in_dev() {
    let text = format!("{BASE}[apps.tool]\npath = \"apps/peak\"\nsecrets.token = \"hunter2\"\n");
    let report = run(&[("joker.toml", &text)], "joker.toml");
    assert!(!report.has_errors());
    assert!(
        report.render().starts_with("warning[W0294]"),
        "{}",
        report.render()
    );
    let prod = text.replace("mode = \"dev\"", "mode = \"prod\"");
    let report = run(&[("joker.toml", &prod)], "joker.toml");
    assert!(
        report.render().contains("error[J0294]"),
        "{}",
        report.render()
    );
}
