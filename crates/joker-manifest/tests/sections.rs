//! The v1.0 sections beyond the first slice (ref/01 §5.4–§5.22) and the
//! 1.1 reservations (§6): one case per code.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers fail the test on a broken fixture"
)]

use std::path::Path;

use joker_manifest::context::{
    AppInterface, BoardInfo, ClassInfo, ClassMember, Direction, DriverInfo, Member, MemberKind,
    ProfileInfo, SignalInfo, SignalType,
};
use joker_manifest::model::BusKind;
use joker_manifest::{Context, MemoryFileSystem, Report, check};
use joker_units::Unit;

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

fn member(kind: MemberKind, ty: SignalType) -> Member {
    Member::new(kind, ty)
}

fn context() -> Context {
    use Direction::{Input, Output};
    use SignalType::{Bool, Float, Integer};
    let mut c = Context::default();
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
        "gpio".into(),
        DriverInfo {
            config: ["chip".to_owned()].into_iter().collect(),
            address: [("line".to_owned(), None)].into_iter().collect(),
            ..DriverInfo::default()
        },
    );
    c.profiles.insert(
        "acme/pcs@1".into(),
        ProfileInfo {
            signals: [
                ("p_set", signal(Float, Output, Some("kW"))),
                ("q_set", signal(Float, Output, Some("kvar"))),
                ("apply", signal(Integer, Output, None)),
                ("p_ac", signal(Float, Input, Some("kW"))),
                ("f", signal(Float, Input, Some("Hz"))),
                ("on", signal(Bool, Input, None)),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
            fallback: true,
            actions: [
                (
                    "reset".to_owned(),
                    Some(joker_units::Duration::from_secs(5)),
                ),
                ("silent".to_owned(), None),
            ]
            .into_iter()
            .collect(),
            ..ProfileInfo::default()
        },
    );
    c.profiles.insert(
        "acme/io@1".into(),
        ProfileInfo {
            signals: [
                ("do_1", signal(Bool, Output, None)),
                ("di_1", signal(Bool, Input, None)),
                ("di_2", signal(Bool, Input, None)),
                ("ai_1", signal(Float, Input, Some("bar"))),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
            fallback: true,
            ..ProfileInfo::default()
        },
    );
    c.profiles.insert(
        "acme/fb@1".into(),
        ProfileInfo {
            signals: [
                ("do_1", signal(Bool, Output, None)),
                ("do_2", signal(Bool, Output, None)),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
            fallback: true,
            fallback_outputs: Some(["do_1".to_owned()].into_iter().collect()),
            fallback_value: Some(joker_manifest::value::Value::Bool(true)),
            ..ProfileInfo::default()
        },
    );
    c.boards.insert(
        "rpi4".into(),
        BoardInfo {
            majors: vec![1],
            arch: "aarch64".into(),
            cpus: Some(4),
            ..BoardInfo::default()
        },
    );
    c.boards.insert(
        "esp32-devkit".into(),
        BoardInfo {
            majors: vec![1],
            arch: "xtensa-esp32-none-elf".into(),
            micro: true,
            ..BoardInfo::default()
        },
    );
    let mut pressure = member(MemberKind::In, Float);
    pressure.unit = Some(unit("bar"));
    pressure.capability = Some("pressure".into());
    c.classes.insert(
        "std/pump@1".into(),
        ClassInfo {
            members: [
                ("run_cmd", member(MemberKind::Out, Bool)),
                ("running", member(MemberKind::In, Bool)),
                ("fault", member(MemberKind::In, Bool)),
                ("pressure_out", pressure),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
            capabilities: ["pressure".to_owned()].into_iter().collect(),
        },
    );
    c.classes.insert(
        "std/button@1".into(),
        ClassInfo {
            members: [("pressed".to_owned(), member(MemberKind::In, Bool))]
                .into_iter()
                .collect(),
            capabilities: ["led".to_owned()].into_iter().collect(),
        },
    );
    let mut boot = member(MemberKind::Class, Bool);
    boot.class = Some(ClassMember {
        class: "std/button@1".into(),
        min: 1,
        max: 1,
        fixed: true,
        requires: vec!["led".into()],
    });
    let mut pumps = member(MemberKind::Class, Bool);
    pumps.class = Some(ClassMember {
        class: "std/pump@1".into(),
        min: 1,
        max: 2,
        fixed: false,
        requires: Vec::new(),
    });
    pumps.optional = true;
    c.classes.insert(
        "std/panel@1".into(),
        ClassInfo {
            members: [("boot".to_owned(), boot), ("pumps".to_owned(), pumps)]
                .into_iter()
                .collect(),
            ..ClassInfo::default()
        },
    );
    c.apps.insert(
        "pump_ctl".into(),
        AppInterface {
            implements: vec!["std/pump@1".into()],
            ..AppInterface::default()
        },
    );
    c.apps.insert("other".into(), AppInterface::default());
    c.apps.insert(
        "ctl".into(),
        AppInterface {
            ports: [(
                "p".to_owned(),
                joker_manifest::context::Port {
                    kind: joker_manifest::context::PortKind::Output,
                    ty: Float,
                    unit: Some(unit("kW")),
                    optional: true,
                },
            )]
            .into_iter()
            .collect(),
            tasks: [(
                "run".to_owned(),
                Some(joker_units::Duration::from_millis(100)),
            )]
            .into_iter()
            .collect(),
            ..AppInterface::default()
        },
    );
    c.boards.insert(
        "zero".into(),
        BoardInfo {
            majors: vec![1],
            arch: "armv6".into(),
            no_wasm: true,
            ..BoardInfo::default()
        },
    );
    if let Some(p) = c.profiles.get_mut("acme/pcs@1") {
        p.fallback_timeout = Some(joker_units::Duration::from_secs(2));
    }
    c
}

fn run(text: &str) -> Report {
    let mut fs = MemoryFileSystem::default();
    fs.file("joker.toml", text).dir("apps/x");
    check(&fs, Path::new("joker.toml"), &context())
}

const BASE: &str = "[project]\nname = \"t\"\nmode = \"dev\"\n";

const FB: &str =
    "[devices.fb]\ndriver = \"modbus-tcp\"\naddress = \"10.0.0.3:502\"\nprofile = \"acme/fb@1\"\n";
const PROD: &str =
    "[project]\nname = \"t\"\njoker = \"1.0\"\nmode = \"prod\"\n[target]\nboard = \"rpi4@1\"\n";
const MICRO: &str =
    "[project]\nname = \"m\"\nmode = \"dev\"\n[target]\nboard = \"esp32-devkit@1\"\n";
const PCS: &str = "[devices.pcs]\ndriver = \"modbus-tcp\"\naddress = \"10.0.0.1:502\"\nprofile = \"acme/pcs@1\"\n[outputs.\"pcs.p_set\"]\nsafe = \"0 kW\"\n[outputs.\"pcs.q_set\"]\nsafe = \"0 kvar\"\n[outputs.\"pcs.apply\"]\nsafe = 0\n";
const IO: &str = "[devices.io]\ndriver = \"modbus-tcp\"\naddress = \"10.0.0.2:502\"\nprofile = \"acme/io@1\"\n[outputs.\"io.do_1\"]\nsafe = false\n";

fn site(extra: &str) -> String {
    format!("{BASE}{PCS}{IO}{extra}")
}

fn prod(extra: &str) -> String {
    format!("{PROD}{PCS}{IO}{extra}")
}

/// A case: the code, the manifest and what it shows.
type Case = (&'static str, String, &'static str);

fn cases() -> Vec<Case> {
    vec![
        ("J0140", site("[resources]\ngpu = { cpus = [1] }\n"), "unknown resource class"),
        ("J0360", site("[resources]\nrt = { cpus = [1] }\nbg = { cpus = [0, 1] }\n"), "rt overlapping another class"),
        ("J0144", prod("[resources]\nrt = { cpus = [7] }\n"), "CPU the board does not have"),
        ("J0145", site("[resources]\nrt = { cpus = [1], memory = \"1 GiB\" }\n"), "memory on rt"),
        ("J0290", site("[resources]\nml = { accel = [\"gpu:0\"] }\n"), "1.1 key"),
        ("J0290", site("[models.m]\nsource = \"m.jkm\"\nbackend = [\"burn:cpu\"]\ndeadline = \"1s\"\n"), "1.1 section"),
        ("J0290", site("[signals.\"pcs.p_ac\"]\ndata_class = \"personal\"\n"), "reserved data class of a signal"),
        ("J0290", site("[devices.pcs.signals]\np_ac = { data_class = \"sensitive\" }\n"), "reserved data class of a device signal"),
        ("J0290", site("[telemetry.uplink.u]\nkind = \"mqtt\"\nbroker = \"mqtts://b:8883\"\naccepts = [\"operational\"]\n"), "reserved egress classes of an uplink"),
        ("J0145", site("[clock]\nsync = \"link\"\n"), "link clock on Linux"),
        ("J0142", site("[clock]\nsync = \"ptp\"\n"), "ptp without iface"),
        ("J0152", site("[clock]\nmax_holdover = \"31d\"\n"), "holdover above 30d"),
        ("J0170", site("[devices.b]\ndriver = \"x:bacnet\"\n"), "x driver without a declaration"),
        ("J0142", site("[drivers.dlms]\ntier = \"w\"\n"), "W driver without source"),
        ("J0145", site("[drivers.bac]\ntier = \"w\"\nsource = \"oci://r/d:1\"\nexec = \"/bin/x\"\n"), "x-only key on a W driver"),
        ("J0874", site("[drivers.dlms]\ntier = \"w\"\nsource = \"oci://r/d:1\"\nmemory = \"128 MiB\"\n"), "W driver memory above 64 MiB"),
        ("J0865", site("[drivers.bac]\ntier = \"x\"\nexec = \"/bin/x\"\ngroups = [\"root\"]\n"), "privileged group"),
        ("J0141", site("[drivers.bac]\ntier = \"x\"\nexec = \"/bin/x\"\nenv = { JDP_X = \"1\" }\n"), "reserved env key"),
        ("J0184", site("[drivers.bac]\ntier = \"x\"\nexec = \"/bin/x\"\nnet = [\"not-an-ip\"]\n"), "invalid net entry"),
        ("J0101", site("[signals.\"pcs.nope\"]\ndebounce = \"20ms\"\n"), "unknown signal"),
        ("J0155", site("[signals.\"zz*\"]\ndebounce = \"20ms\"\n"), "selector without matches"),
        ("J0175", site("[devices.pcs.signals]\non = { debounce = \"10ms\" }\n[signals.\"pcs.on\"]\ndebounce = \"20ms\"\n"), "two different adjustments"),
        ("J0176", site("[calc]\nx = \"pcs.p_ac * 2\"\n[signals.\"calc.x\"]\ndebounce = \"20ms\"\n"), "adjustment on a derived signal"),
        ("J0193", site("[instances.p1]\nclass = \"ghost@1\"\n"), "unknown class"),
        ("J0193", site("[instances.p1]\nclass = \"pump@1\"\ncapabilities = [\"flying\"]\nmap = { run_cmd = \"io.do_1\", running = \"io.di_1\", fault = \"io.di_2\" }\n"), "unknown capability"),
        ("J0601", site("[instances.p1]\nclass = \"pump@1\"\nmap = { run_cmd = \"io.do_1\", running = \"io.di_1\" }\n"), "required member unbound"),
        ("J0602", site("[instances.p1]\nclass = \"pump@1\"\ncapabilities = [\"pressure\"]\nmap = { run_cmd = \"io.do_1\", running = \"io.di_1\", fault = \"io.di_2\" }\n"), "capability member unbound"),
        ("J0603", site("[instances.p1]\nclass = \"pump@1\"\nmap = { run_cmd = \"const:true\", running = \"io.di_1\", fault = \"io.di_2\" }\n"), "const on an out member"),
        ("J0194", site("[instances.p1]\nclass = \"pump@1\"\nmap = { run_cmd = \"io.di_1\", running = \"io.di_1\", fault = \"io.di_2\" }\n"), "out member mapped to an input"),
        ("J0604", site("[devices.p1]\ndriver = \"gpio\"\n[devices.p1.signals]\nrunning = { line = \"GPIO1\" }\n[instances.p1]\nclass = \"pump@1\"\nmap = { run_cmd = \"io.do_1\", running = \"io.di_1\", fault = \"io.di_2\" }\n"), "member path collision"),
        ("J0192", site("[apps.pump_ctl]\npath = \"apps/x\"\n[instances.p1]\nclass = \"pump@1\"\nimplemented_by = \"app:pump_ctl\"\nsim = true\n"), "two origins"),
        ("J0191", site("[apps.other]\npath = \"apps/x\"\n[instances.p1]\nclass = \"pump@1\"\nimplemented_by = \"app:other\"\n"), "app that does not implement the class"),
        ("J0145", prod("[instances.t]\nclass = \"button@1\"\nsim = true\n"), "simulated instance in prod"),
        ("J0650", site("[instances.b]\nclass = \"button@1\"\nmap = { pressed = \"io.di_1\" }\n[instances.panel]\nclass = \"panel@1\"\nmap = { boot = \"io.di_1\" }\n"), "class member bound to a signal"),
        ("J0651", site("[instances.p1]\nclass = \"pump@1\"\nmap = { run_cmd = \"io.do_1\", running = \"io.di_1\", fault = \"io.di_2\" }\n[instances.panel]\nclass = \"panel@1\"\nmap = { boot = \"p1\" }\n"), "class member of another class"),
        ("J0652", site("[instances.b]\nclass = \"button@1\"\nmap = { pressed = \"io.di_1\" }\n[instances.panel]\nclass = \"panel@1\"\nmap = { boot = \"b\" }\n"), "instance without the required capability"),
        ("J0653", site("[instances.b]\nclass = \"button@1\"\ncapabilities = [\"led\"]\nmap = { pressed = \"io.di_1\" }\n[instances.panel]\nclass = \"panel@1\"\nmap = { boot = [\"b\"] }\n"), "list on a fixed member"),
        ("J0654", site("[instances.a]\nclass = \"panel@1\"\nmap = { boot = \"c\" }\n[instances.c]\nclass = \"panel@1\"\nmap = { boot = \"a\" }\n"), "instance that is a member of itself"),
        ("J0190", site("[calc]\na = \"calc.b + 1 kW\"\nb = \"calc.a\"\n"), "calc cycle"),
        ("J0237", site("[calc]\nx = { expr = \"pcs.p_ac\", group = \"ghost\" }\n"), "calc group missing"),
        ("J0301", site("[calc]\nx = \"pcs.p_ac + pcs.f\"\n"), "calc dimensions"),
        ("J0302", site("[calc]\nx = \"pcs.ghost * 2\"\n"), "calc unknown signal"),
        ("J0158", site("[calc]\nx = \"pcs.p_ac *\"\n"), "calc syntax"),
        ("J0304", site("[alarms.a]\nwhen = \"pcs.p_ac\"\nseverity = \"high\"\n"), "alarm condition that is not a boolean"),
        ("J0144", site("[alarms.a]\nwhen = \"pcs.on\"\nseverity = \"urgent\"\n"), "unknown severity"),
        ("J0142", site("[alarms.a]\nwhen = \"pcs.on\"\n"), "alarm without severity"),
        ("J0302", site("[alarms.a]\nwhen = \"pcs.on\"\nseverity = \"low\"\nmessage = \"see {pcs.ghost}\"\n"), "unknown placeholder"),
        ("J0234", site("[alarms.a]\nwhen = \"pcs.on\"\nseverity = \"low\"\nactions = { trip = [\"pcs.p_ac\"] }\n"), "trip of an input"),
        ("J0196", site("[alarms.app]\nwhen = \"pcs.on\"\nseverity = \"low\"\n"), "alarm named app"),
        ("J0236", site("[outputs.\"pcs.p_set\"]\nsafe = \"0 kW\"\nreset_when = \"pcs.on\"\n").replacen("[outputs.\"pcs.p_set\"]\nsafe = \"0 kW\"\n", "", 1), "reset_when that never trips"),
        ("J0304", site("[outputs.\"pcs.p_set\"]\nsafe = \"0 kW\"\ninterlock = \"pcs.p_ac\"\n").replacen("[outputs.\"pcs.p_set\"]\nsafe = \"0 kW\"\n", "", 1), "interlock that is not a boolean"),
        ("J0301", site("[outputs.\"pcs.p_set\"]\nsafe = \"0 kW\"\nrange = [\"-pcs.f\", \"100 kW\"]\n").replacen("[outputs.\"pcs.p_set\"]\nsafe = \"0 kW\"\n", "", 1), "range bound in another dimension"),
        ("J0205", format!("{BASE}[devices.led]\ndriver = \"gpio\"\n[devices.led.signals]\nact = {{ line = \"GPIO2\", direction = \"output\" }}\n[outputs.\"led.act\"]\nsafe = false\n"), "output without a device fallback"),
        ("J0205", format!("{BASE}{FB}[outputs.\"fb.do_1\"]\nsafe = true\n[outputs.\"fb.do_2\"]\nsafe = false\n"), "output outside the fallback's applies_to"),
        ("J0205", format!("{BASE}{FB}[outputs.\"fb.do_1\"]\nsafe = false\n[outputs.\"fb.do_2\"]\nsafe = false\naccept_missing_fallback = \"indicator only, no load\"\n"), "fallback value not as safe as safe"),
        ("J0238", format!("{BASE}[devices.led]\ndriver = \"gpio\"\n[devices.led.signals]\nact = {{ line = \"GPIO2\", direction = \"output\" }}\n[outputs.\"led.act\"]\nsafe = false\nsafer = \"lower\"\naccept_missing_fallback = \"indicator only, no load\"\n"), "safer without a fallback"),
        ("J0277", site("[outputs.\"pcs.p_set\"]\nsafe = \"0 kW\"\naccept_missing_fallback = \"justified somehow\"\n").replacen("[outputs.\"pcs.p_set\"]\nsafe = \"0 kW\"\n", "", 1), "acceptance on an output with a fallback"),
        ("J0270", site("[atomic.a]\noutputs = [\"pcs.p_set\", \"pcs.q_set\"]\n[atomic.b]\noutputs = [\"pcs.p_set\", \"pcs.apply\"]\n"), "output in two atomic groups"),
        ("J0271", site("[atomic.a]\noutputs = [\"pcs.p_set\", \"io.do_1\"]\n"), "atomic group across devices"),
        ("J0272", site("[cycles.fast]\nperiod = \"50ms\"\n[atomic.a]\noutputs = [\"pcs.p_set\", \"pcs.q_set\"]\n").replacen("safe = \"0 kvar\"\n", "safe = \"0 kvar\"\ngroup = \"fast\"\n", 1), "atomic outputs with different owner groups"),
        ("J0273", site("[atomic.a]\noutputs = [\"pcs.p_set\", \"pcs.q_set\"]\norder = [\"pcs.p_set\"]\n"), "order that is not a permutation"),
        ("J0274", site("[atomic.a]\noutputs = [\"pcs.p_set\", \"pcs.q_set\"]\ncommit = { signal = \"pcs.p_ac\", value = 1 }\n"), "commit on an input"),
        ("J0195", site("[telemetry.record.\"pcs.p_ac\"]\nkeep = { raw = \"7d\" }\n"), "record without every or on_change"),
        ("J0195", site("[telemetry.record]\n\"pcs.p*c\" = { every = \"1s\" }\n\"pcs.p*\" = { every = \"5s\" }\n"), "ambiguous record selectors"),
        ("J0197", site("[telemetry.record.\"pcs.p_ac\"]\nevery = \"1s\"\nkeep = { raw = \"7d\", \"5m\" = \"30d\" }\n"), "unknown layer"),
        ("J0156", site("[telemetry.record.\"zz.**\"]\nevery = \"1s\"\n"), "record selector without matches"),
        ("J0198", site("[telemetry.uplink.u]\nkind = \"otlp\"\nendpoint = \"https://o\"\nlive = { every = \"5s\" }\n"), "live on otlp"),
        ("J0198", site("[telemetry.uplink.u]\nkind = \"mqtt\"\nbroker = \"mqtts://b:8883\"\nclasses = { urgent = \"alarm.**\" }\n"), "unknown uplink class"),
        ("J0142", site("[telemetry.uplink.u]\nkind = \"mqtt-sparkplug\"\nbroker = \"mqtts://b:8883\"\n"), "sparkplug without group_id"),
        ("J0145", site("[telemetry.uplink.u]\nkind = \"mqtt\"\nbroker = \"mqtts://b:8883\"\nendpoints = [\"x\"]\n"), "key of another uplink kind"),
        ("J0296", prod("[telemetry.uplink.u]\nkind = \"mqtt\"\nbroker = \"mqtt://b:1883\"\n"), "uplink without TLS in prod"),
        ("J0294", site("[telemetry.uplink.u]\nkind = \"mqtt\"\nbroker = \"mqtts://b:8883\"\npassword = \"hunter2\"\n"), "plain uplink password"),
        ("J0159", site("[telemetry.uplink.u]\nkind = \"mqtt\"\nbroker = \"mqtts://b:8883\"\nbudget = \"300 MB\"\n"), "invalid data volume"),
        ("J0253", site("[bridges.s]\nkind = \"opcua-server\"\nwriter = { level = \"safety\", writable = [\"pcs.p_set\"] }\nsecurity = { trust = [\"k.der\"] }\n"), "bridge in the safety band"),
        ("J0252", site("[bridges.s]\nkind = \"opcua-server\"\nwriter = { level = \"manual\", writable = [\"pcs.p_set\"] }\nsecurity = { trust = [\"k.der\"] }\n"), "bridge in the manual band"),
        ("J0199", site("[bridges.m]\nkind = \"modbus-server\"\nprofile = \"p\"\n"), "modbus server without listen or bus"),
        ("J0199", site("[bridges.r]\nkind = \"zenoh-ros2\"\nsubscribe = { \"/cmd\" = { type = \"t\", fields = [\"a\", \"b\"], to = [\"bridge.r.a\"] } }\n"), "fields and to of different lengths"),
        ("J0155", site("[bridges.s]\nkind = \"opcua-server\"\nwriter = { level = \"supervisor\", writable = [\"zz.*\"] }\nsecurity = { trust = [\"k.der\"] }\n"), "writable without matches"),
        ("J0156", site("[bridges.s]\nkind = \"opcua-server\"\nexpose = [\"zz.**\"]\n"), "expose without matches"),
        ("J0297", prod("[bridges.s]\nkind = \"opcua-server\"\nlisten = \"0.0.0.0:4840\"\nwriter = { level = \"supervisor\", writable = [\"pcs.p_set\"] }\nsecurity = { trust = [\"k.der\"] }\n"), "bridge with writes on every interface"),
        ("J0296", prod("[bridges.s]\nkind = \"opcua-server\"\nsecurity = { mode = \"None\" }\n"), "OPC UA without security"),
        ("J0144", site("[security]\ntrust = { oracles = [\"k.pub\"] }\n"), "unknown trust role"),
        ("J0157", site("[security]\napi = { listen = [\"http://0.0.0.0\"] }\n"), "invalid listener"),
        ("J0295", site("[security.roles]\noperator = [\"ops\"]\n"), "invalid principal"),
        ("J0293", site("[security.roles]\nadmin = [\"cert:CN=*\"]\n"), "role granted to any certificate"),
        ("J0140", site("[security]\napi = { listen = [], mtls = true }\n"), "removed api.mtls key"),
        ("J0145", site("[link]\nbus = \"b\"\n"), "link section on Linux"),
        ("J0720", format!("{MICRO}[resources]\nrt = {{ cpus = [0] }}\n"), "Linux section on a Joker Micro"),
        ("J0145", format!("{MICRO}[link]\nbus = \"b\"\naddress = \"udp:10.0.0.1:7471\"\n"), "link with bus and address"),
        ("J0179", format!("{MICRO}[link]\naddress = \"udp:10.0.0.1:7471\"\nauth = \"none\"\n[net.wifi]\nssid = \"s\"\npassword = \"secret:w\"\ncountry = \"PT\"\n"), "link without auth over IP"),
        ("J0241", format!("{MICRO}[link]\naddress = \"udp:10.0.0.1:7471\"\n"), "IP link without Wi-Fi"),
        ("J0240", format!("{MICRO}[net.wifi]\nssid = \"\"\npassword = \"secret:w\"\ncountry = \"PT\"\n"), "empty ssid"),
        ("J0242", format!("{MICRO}[net.wifi]\nssid = \"s\"\ncountry = \"PT\"\n"), "open Wi-Fi"),
        ("J0785", format!("{MICRO}[devices.x]\ndriver = \"extern\"\n[devices.x.signals]\nlevel = {{ type = \"f64\", direction = \"input\" }}\n"), "extern signal without unit"),
        ("J0145", format!("{BASE}[target]\naccept_unsafe_reset = \"no load at all here\"\n"), "MCU-only key on Linux"),
        ("J0442", format!("{MICRO}accept_unsafe_reset = \"ok\"\n"), "short reset justification"),
        ("J0210", site("[apps.ctl]\npath = \"apps/x\"\nbind.p = \"pcs.p_set\"\nwriter = { level = \"optimizer\" }\n[apps.pump_ctl]\npath = \"apps/x\"\nbind.q = \"pcs.p_set\"\nwriter = { level = \"optimizer\" }\n"), "two writers at the same level"),
        ("J0211", site("[apps.ctl]\npath = \"apps/x\"\nbind.p = \"pcs.p_set\"\n[apps.pump_ctl]\npath = \"apps/x\"\nbind.q = \"pcs.p_set\"\nwriter = { level = \"optimizer\" }\n"), "several writers, one without writer"),
        ("J0213", site("[apps.ctl]\npath = \"apps/x\"\nbind.p = \"pcs.p_set\"\nwriter = { lease = \"20m\" }\n"), "lease above max_lease"),
        ("J0256", site("[apps.ctl]\npath = \"apps/x\"\nbind.p = \"pcs.p_set\"\nwriter = { lease = \"10ms\" }\n"), "lease shorter than the period"),
        ("J0257", site("[apps.other]\npath = \"apps/x\"\nwriter = { level = \"optimizer\" }\n"), "writer on an app without outputs"),
        ("J0130", site("[cycles.slow]\nperiod = \"1s\"\n[apps.ctl]\npath = \"apps/x\"\ncycles = { run = \"slow\" }\n"), "period outside the tolerance"),
        ("J0188", site("[apps.ctl]\npath = \"apps/x\"\nshadow = { of = \"ctl\" }\n"), "shadow.of without a version"),
        ("J0189", site("[apps.pump_ctl]\npath = \"apps/x\"\nbind.x = \"pcs.p_ac\"\n[instances.p2]\nclass = \"pump@1\"\nimplemented_by = { app = \"pump_ctl\", bind = { x = \"pcs.f\" } }\n"), "port bound in two places"),
        ("J0243", format!("{BASE}[target]\nboard = \"zero\"\n[apps.ctl]\npath = \"apps/x\"\n"), "board without a Wasm engine"),
        ("J0276", site("[devices.pcs.fallback]\nkeepalive = \"5s\"\n").replacen("[devices.pcs]\n", "[devices.pcs]\nfallback = { keepalive = \"5s\" }\n", 1).replace("[devices.pcs.fallback]\nkeepalive = \"5s\"\n", ""), "keepalive above the device timeout"),
        ("J0276", site("").replacen("safe = \"0 kW\"\n", "safe = \"0 kW\"\nkeepalive = \"3s\"\n", 1), "output keepalive above the device timeout"),
        ("J0276", format!("{BASE}{FB}[outputs.\"fb.do_2\"]\nsafe = false\nkeepalive = \"1s\"\naccept_missing_fallback = \"bench lamp, no load behind it\"\n"), "output keepalive without a device fallback"),
        ("J0235", site("").replacen("safe = \"0 kW\"\n", "safe = \"0 kW\"\nverify = { readback = \"pcs.p_ac\" }\n", 1), "float verify without a tolerance"),
        ("J0816", site("[devices.pcs.signals]\np_ac = { stuck_detect = \"0s\" }\n"), "stuck_detect that is not positive"),
        ("J0176", site("[devices.pcs.signals]\np_ac = { limits = [\"10 kW\", \"5 kW\"] }\n"), "limits with the minimum above the maximum"),
        ("J0176", site("[devices.pcs.signals]\non = { deadband = \"1 kW\" }\n"), "deadband on a bool"),
        ("J0176", site("[devices.pcs.signals]\np_ac = { debounce = \"20ms\" }\n"), "debounce on a float"),
        ("J0145", site("[devices.pcs.signals]\np_ac = { scale = { from = [\"0 kW\", \"1 kW\"], to = [\"0 W\", \"1000 W\"] }, offset = 1 }\n"), "offset with a two-point scale"),
        ("J0107", site("[devices.pcs.signals]\np_ac = { unit = \"Hz\" }\n"), "unit of another dimension"),
        ("J0284", site("[resources]\nrt = { cpus = [3] }\n[cycles.main]\nperiod = \"20ms\"\ncpu = 1\n"), "cpu outside rt"),
        ("J0286", site("[cycles.main]\nperiod = \"20ms\"\noverrun = \"degrade\"\n"), "degrade without optional tasks"),
        ("J0287", site("[cycles.main]\nperiod = \"20ms\"\noverrun = \"catch_up\"\n").replacen("safe = \"0 kW\"\n", "safe = \"0 kW\"\nrate = \"5 kW/s\"\ngroup = \"main\"\n", 1), "catch_up with time-dependent outputs"),
        ("J0298", site("[devices.pcs.actions.reset]\nrole = \"viewer\"\n"), "action for viewers"),
        ("J0304", site("[devices.pcs.actions.reset]\npermit = \"pcs.p_ac\"\n"), "action permit that is not a boolean"),
        ("J0101", site("[devices.pcs.actions.bogus]\n"), "action not declared in the profile"),
        ("J0142", site("[devices.pcs.actions.silent]\n"), "action without a timeout and no profile default"),
    ]
}

#[test]
fn an_output_keepalive_below_the_device_timeout_is_accepted() {
    let text = site("").replacen(
        "safe = \"0 kW\"\n",
        "safe = \"0 kW\"\nkeepalive = \"1s\"\nverify = { readback = \"pcs.p_ac\", tolerance = \"1 kW\" }\n",
        1,
    );
    let report = run(&text);
    for code in ["J0276", "J0235"] {
        assert!(!report.codes().contains(&code), "{}", report.render());
    }
}

#[test]
fn every_case_reports_its_code() {
    for (code, text, what) in cases() {
        let report = run(&text);
        assert!(
            report.codes().contains(&code),
            "{code} ({what}) not reported; got {:?}\n{}",
            report.codes(),
            report.render()
        );
    }
}

fn run_1_1(text: &str) -> Report {
    let mut fs = MemoryFileSystem::default();
    fs.file("joker.toml", text).dir("apps/x");
    let mut context = context();
    context.runtime = joker_manifest::model::SchemaVersion { major: 1, minor: 1 };
    check(&fs, Path::new("joker.toml"), &context)
}

const V11: &str = "[project]\nname = \"t\"\njoker = \"1.1\"\nmode = \"dev\"\n";

fn cases_1_1() -> Vec<Case> {
    let on = |extra: &str| format!("{V11}{PCS}{IO}{extra}");
    vec![
        ("J0148", site("[resources]\nml = { accel = [\"gpu:0\"] }\n"), "1.1 key with joker = 1.0"),
        ("J0239", on("[outputs.\"pcs.p_set\"]\nsafe = \"0 kW\"\nschedule_depth = 300\non_late = \"drop\"\n").replacen("[outputs.\"pcs.p_set\"]\nsafe = \"0 kW\"\n", "", 1), "too many scheduled writes"),
        ("J0206", on("[outputs.\"pcs.p_set\"]\nsafe = \"0 kW\"\nschedule_depth = 8\n").replacen("[outputs.\"pcs.p_set\"]\nsafe = \"0 kW\"\n", "", 1), "scheduled writes without on_late"),
        ("J0254", on("[apps.ctl]\npath = \"apps/x\"\nwriter = { kind = \"proposal\", level = \"optimizer\" }\n"), "proposal writer with a level"),
        ("J0212", on("[apps.ctl]\npath = \"apps/x\"\nbind.p = \"pcs.p_set\"\nwriter = { kind = \"proposal\" }\n"), "proposal writer writing directly"),
    ]
}

#[test]
fn every_1_1_case_reports_its_code() {
    for (code, text, what) in cases_1_1() {
        let report = run_1_1(&text);
        assert!(
            report.codes().contains(&code),
            "{code} ({what}) not reported; got {:?}\n{}",
            report.codes(),
            report.render()
        );
    }
}

#[test]
fn every_ref01_code_is_emitted_or_deferred() {
    let deferred: [(&str, &str); 7] = [
        (
            "J0260",
            "reserved: data classes are not implemented yet (ADR 0079)",
        ),
        (
            "J0261",
            "reserved: data classes are not implemented yet (ADR 0079)",
        ),
        (
            "J0381",
            "storage write load: no endurance model yet (ADR 0048)",
        ),
        ("J0185", "needs the node vault (`jk check --node`)"),
        ("J0292", "needs the node trust bundle (`jk check --node`)"),
        ("J0291", "retired: the key gives J0140"),
        ("J0401", "lock drift, in `lock::drift`"),
    ];
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let doc = std::fs::read_dir(root.join("../../docs/ref"))
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("01-"))
        })
        .unwrap();
    let text = std::fs::read_to_string(doc).unwrap();
    let start = text.find("\n## 10.").unwrap();
    let end = text[start..]
        .find("\n## 11.")
        .map_or(text.len(), |e| start + e);
    let row = regex::Regex::new(r"(?m)^\| `(J\d{4})` \|").unwrap();
    let listed: Vec<String> = row
        .captures_iter(&text[start..end])
        .map(|c| c[1].to_owned())
        .collect();
    let literal = regex::Regex::new(r#""(J\d{4})""#).unwrap();
    let mut emitted = std::collections::BTreeSet::new();
    for dir in ["src", "../joker-units/src", "../joker-expr/src"] {
        for entry in std::fs::read_dir(root.join(dir)).unwrap() {
            let source = std::fs::read_to_string(entry.unwrap().path()).unwrap();
            let code = source.split("#[cfg(test)]").next().unwrap_or("");
            emitted.extend(literal.captures_iter(code).map(|c| c[1].to_owned()));
        }
    }
    let missing: Vec<&String> = listed
        .iter()
        .filter(|c| !emitted.contains(*c) && !deferred.iter().any(|(d, _)| d == c))
        .collect();
    assert!(listed.len() > 100, "ref/01 §10 not found");
    assert!(
        missing.is_empty(),
        "ref/01 §10 codes neither emitted nor deferred: {missing:?}"
    );
    let stale: Vec<&str> = deferred
        .iter()
        .map(|(c, _)| *c)
        .filter(|c| emitted.contains(*c) && *c != "J0401")
        .collect();
    assert!(
        stale.is_empty(),
        "deferred codes now emitted, drop them from the list: {stale:?}"
    );
}

#[test]
fn a_full_site_checks_clean() {
    let text = site(concat!(
        "[resources]\nrt = { cpus = [3] }\nbg = { cpus = [0, 1] }\n",
        "[clock]\nsync = \"ntp\"\nservers = [\"ntp.example.com\"]\n",
        "[signals.\"pcs.on\"]\ndebounce = \"20ms\"\n",
        "[instances.p1]\nclass = \"pump@1\"\nmap = { run_cmd = \"io.do_1\", running = \"io.di_1\", fault = \"io.di_2\" }\n",
        "[calc]\nload = \"pcs.p_ac * 2\"\nlimit = \"400 kW\"\nheadroom = \"calc.limit - calc.load\"\n",
        "[alarms.over]\nwhen = \"calc.headroom < 0 kW && p1.running\"\nseverity = \"high\"\nmessage = \"headroom {calc.headroom}\"\nactions = { trip = [\"pcs.p_set\"] }\n",
        "[atomic.cmd]\noutputs = [\"pcs.p_set\", \"pcs.q_set\"]\ncommit = { signal = \"pcs.apply\", value = 1 }\n",
        "[telemetry.record]\n\"pcs.**\" = { every = \"1s\", keep = { raw = \"7d\", \"1m\" = \"400d\" } }\n\"pcs.p_ac\" = { on_change = true }\n",
        "[telemetry.uplink.cloud]\nkind = \"mqtt-sparkplug\"\nbroker = \"mqtts://b:8883\"\ngroup_id = \"g\"\nclasses = { alarms = \"alarm.**\", high = [\"pcs.**\"] }\npassword = \"secret:mqtt\"\n",
        "[bridges.scada]\nkind = \"opcua-server\"\nexpose = [\"pcs.**\"]\nwriter = { level = \"supervisor\", priority = 7, lease = \"30s\", writable = [\"pcs.q_set\"] }\nsecurity = { trust = [\"k.der\"] }\n",
        "[security]\ntrust = { apps = [\"keys/apps.pub\"], revoked = [\"ed25519:0123456789abcdef0123456789abcdef\"] }\napi = { listen = [\"tcp:0.0.0.0:7443\"] }\n",
        "[security.roles]\noperator = [\"cert:CN=ops-*\", \"local:group=joker-ops\"]\n",
    ))
    .replace("[outputs.\"pcs.apply\"]\nsafe = 0\n", "");
    let report = run(&text);
    assert!(!report.has_errors(), "{}", report.render());
    assert!(report.paths.contains_key("p1.running"));
    assert!(report.paths.contains_key("alarm.over.active"));
    let headroom = &report.paths["calc.headroom"].info;
    assert_eq!(
        headroom.unit.map(|u| u.symbol().to_owned()),
        Some("kW".to_owned())
    );
}

#[test]
fn a_micro_manifest_checks_clean() {
    let text = format!(
        "{MICRO}[link]\naddress = \"udp:192.168.1.10:7471\"\n[net.wifi]\nssid = \"bench\"\npassword = \"secret:wifi\"\ncountry = \"PT\"\nhostname = \"esp-bench\"\n[clock]\nsync = \"link\"\n[devices.x]\ndriver = \"extern\"\n[devices.x.signals]\nlevel = {{ type = \"f64\", direction = \"input\", unit = \"m\" }}\n"
    );
    let report = run(&text);
    assert!(!report.has_errors(), "{}", report.render());
}

#[test]
fn the_json_schema_is_up_to_date() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("schema/joker-manifest.schema.json");
    let generated = joker_manifest::schema::json_schema();
    if std::env::var_os("JOKER_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &generated).unwrap();
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        committed == generated,
        "schema/joker-manifest.schema.json is stale: run `JOKER_BLESS=1 cargo test -p joker-manifest --test sections`"
    );
}

#[test]
fn a_fallback_value_as_safe_as_safe_and_an_accepted_uncovered_output_pass() {
    let text = format!(
        "{BASE}{FB}[outputs.\"fb.do_1\"]\nsafe = true\n[outputs.\"fb.do_2\"]\nsafe = false\naccept_missing_fallback = \"indicator only, no load\"\n"
    );
    let report = run(&text);
    assert!(!report.codes().contains(&"J0205"), "{}", report.render());
}
