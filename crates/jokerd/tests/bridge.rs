//! `jokerd::bridge`: turning manifest/profile TOML data into the CBOR a
//! driver's `plan()` expects.

#![allow(clippy::unwrap_used)]

use joker_manifest::source::{Sources, Span};
use joker_manifest::value::{Entry, Node, Table, Value};
use jokerd::bridge::{node_to_cbor, table_to_cbor};

fn span() -> Span {
    let mut sources = Sources::default();
    let file = sources.add("x".into(), String::new());
    Span::new(file, 0, 0)
}

fn node(value: Value) -> Node {
    Node {
        value,
        span: span(),
    }
}

#[test]
fn scalars_round_trip() {
    let bytes = node_to_cbor(&node(Value::Integer(42))).encode();
    assert_eq!(joker_hal::cbor::Cbor::new(&bytes).as_u64(), Ok(42));
    let bytes = node_to_cbor(&node(Value::Bool(true))).encode();
    assert_eq!(joker_hal::cbor::Cbor::new(&bytes).as_bool(), Ok(true));
    let bytes = node_to_cbor(&node(Value::String("hi".into()))).encode();
    assert_eq!(joker_hal::cbor::Cbor::new(&bytes).as_str(), Ok("hi"));
    let bytes = node_to_cbor(&node(Value::Float(1.5))).encode();
    assert_eq!(joker_hal::cbor::Cbor::new(&bytes).as_f64(), Ok(1.5));
}

#[test]
fn negative_integers_round_trip() {
    let bytes = node_to_cbor(&node(Value::Integer(-5))).encode();
    assert_eq!(joker_hal::cbor::Cbor::new(&bytes).as_i64(), Ok(-5));
}

#[test]
fn a_table_becomes_a_map() {
    let mut table: Table = Table::new();
    table.insert(
        "at".into(),
        Entry {
            key: span(),
            node: node(Value::Integer(0)),
        },
    );
    table.insert(
        "fc".into(),
        Entry {
            key: span(),
            node: node(Value::Integer(5)),
        },
    );
    let bytes = node_to_cbor(&node(Value::Table(table))).encode();
    let cbor = joker_hal::cbor::Cbor::new(&bytes);
    assert_eq!(cbor.get("at").unwrap().unwrap().as_u64(), Ok(0));
    assert_eq!(cbor.get("fc").unwrap().unwrap().as_u64(), Ok(5));
}

#[test]
fn an_array_becomes_an_array() {
    let bytes = node_to_cbor(&node(Value::Array(vec![
        node(Value::Integer(1)),
        node(Value::Integer(2)),
    ])))
    .encode();
    let cbor = joker_hal::cbor::Cbor::new(&bytes);
    let items: Vec<_> = cbor.array().unwrap().collect();
    assert_eq!(items.len(), 2);
}

#[test]
fn table_to_cbor_wraps_a_plain_node_map() {
    let mut map = std::collections::BTreeMap::new();
    map.insert("unit_id".to_owned(), node(Value::Integer(1)));
    let bytes = table_to_cbor(&map);
    let cbor = joker_hal::cbor::Cbor::new(&bytes);
    assert_eq!(cbor.get("unit_id").unwrap().unwrap().as_u64(), Ok(1));
}

mod device_plan {
    use joker_manifest::OsFileSystem;
    use jokerd::load::load;
    use jokerd::native::NativeTasks;

    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (path, text) in files {
            let full = dir.path().join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, text).unwrap();
        }
        dir
    }

    const JOKER_TOML: &str = r#"
[project]
name  = "bridge-test"
joker = "1.0"
mode  = "dev"

[buses.line0]
kind = "serial"
port = "/dev/ttyUSB0"
baud = 9600

[devices.pcs]
driver  = "modbus-rtu"
bus     = "line0"
profile = "acme/pcs@1"
timeout = "1s"
unit_id = 1
"#;

    const PROFILE_TOML: &str = r#"
schema = 1

[profile]
name    = "acme/pcs"
version = "1.0.0"
driver  = "modbus"

[signals.p_ac]
at   = 0
fc   = 4
type = "f32"
unit = "kW"
"#;

    const PROFILE_WITH_FALLBACK_TOML: &str = r#"
schema = 1

[profile]
name    = "acme/pcs"
version = "1.0.0"
driver  = "modbus"

[signals.p_ac]
at   = 0
fc   = 4
type = "f32"
unit = "kW"

[fallback]
timeout = "10s"
"#;

    const PROFILE_WITH_ACTION_TOML: &str = r#"
schema = 1

[profile]
name    = "acme/pcs"
version = "1.0.0"
driver  = "modbus"

[signals.p_ac]
at   = 0
fc   = 4
type = "f32"
unit = "kW"

[actions.reset_fault]
steps = [
    { write = { at = 40300, fc = 6, type = "u16", value = 1 } },
    { wait = "500ms" },
    { write = { at = 40300, fc = 6, type = "u16", value = 0 } },
]
"#;

    #[test]
    fn a_declared_fallback_gives_a_default_keepalive_of_a_fifth_the_timeout() {
        let dir = project(&[
            ("joker.toml", JOKER_TOML),
            (
                "profiles/acme/pcs@1.profile.toml",
                PROFILE_WITH_FALLBACK_TOML,
            ),
        ]);
        let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(!loaded.report.has_errors(), "{}", loaded.report.render());
        let (context, errors) = jokerd::catalog::context_for_project(&NativeTasks, dir.path());
        assert!(errors.is_empty(), "{errors:?}");
        let device = &loaded.manifest.devices["pcs"];
        let plan = jokerd::bridge::device_plan("pcs", device, &loaded.manifest, &context).unwrap();
        assert_eq!(plan.fallback_keepalive_ns, Some(2_000_000_000));
    }

    #[test]
    fn no_fallback_means_no_keepalive() {
        let dir = project(&[
            ("joker.toml", JOKER_TOML),
            ("profiles/acme/pcs@1.profile.toml", PROFILE_TOML),
        ]);
        let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
            .unwrap_or_else(|e| panic!("{e}"));
        let (context, _) = jokerd::catalog::context_for_project(&NativeTasks, dir.path());
        let device = &loaded.manifest.devices["pcs"];
        let plan = jokerd::bridge::device_plan("pcs", device, &loaded.manifest, &context).unwrap();
        assert_eq!(plan.fallback_keepalive_ns, None);
    }

    #[test]
    fn a_profile_backed_device_resolves_its_point_address_and_shape() {
        let dir = project(&[
            ("joker.toml", JOKER_TOML),
            ("profiles/acme/pcs@1.profile.toml", PROFILE_TOML),
        ]);
        let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(
            loaded.report.codes().is_empty(),
            "{}",
            loaded.report.render()
        );
        let (context, errors) = jokerd::catalog::context_for_project(&NativeTasks, dir.path());
        assert!(errors.is_empty(), "{errors:?}");
        let device = &loaded.manifest.devices["pcs"];
        let plan = jokerd::bridge::device_plan("pcs", device, &loaded.manifest, &context).unwrap();
        assert_eq!(plan.driver, "modbus-rtu");
        assert_eq!(plan.bus.as_deref(), Some("line0"));
        assert_eq!(plan.bus_kind, Some(joker_manifest::model::BusKind::Serial));
        let bus_params = joker_hal::cbor::Cbor::new(&plan.bus_params);
        assert_eq!(bus_params.get("baud").unwrap().unwrap().as_u64(), Ok(9600));
        assert_eq!(plan.timeout_ns, 1_000_000_000);
        let cbor = joker_hal::cbor::Cbor::new(&plan.config);
        assert_eq!(cbor.get("unit_id").unwrap().unwrap().as_u64(), Ok(1));
        assert_eq!(plan.points.len(), 1);
        let p = &plan.points[0];
        assert_eq!(p.path, "pcs.p_ac");
        assert_eq!(p.type_name, "f32");
        assert_eq!(p.expect, joker_hal::RawKind::Regs);
        assert_eq!(p.width, 2);
        let addr = joker_hal::cbor::Cbor::new(&p.addr);
        assert_eq!(addr.get("at").unwrap().unwrap().as_u64(), Ok(0));
        assert_eq!(addr.get("fc").unwrap().unwrap().as_u64(), Ok(4));
    }

    #[test]
    fn a_profile_action_compiles_its_steps_with_addressed_cbor() {
        let dir = project(&[
            ("joker.toml", JOKER_TOML),
            ("profiles/acme/pcs@1.profile.toml", PROFILE_WITH_ACTION_TOML),
        ]);
        let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(!loaded.report.has_errors(), "{}", loaded.report.render());
        let (context, errors) = jokerd::catalog::context_for_project(&NativeTasks, dir.path());
        assert!(errors.is_empty(), "{errors:?}");
        let device = &loaded.manifest.devices["pcs"];
        let plan = jokerd::bridge::device_plan("pcs", device, &loaded.manifest, &context).unwrap();
        assert_eq!(plan.actions.len(), 1);
        let action = &plan.actions[0];
        assert_eq!(action.name, "reset_fault");
        // 3 steps x 1s + the 500ms wait.
        assert_eq!(action.timeout_ns, 3_500_000_000);
        assert_eq!(
            action.dedup_window_ns, None,
            "no device override means no dedup_window"
        );
        assert_eq!(
            action.role, "operator",
            "no role declared falls back to the API's own floor"
        );
        assert_eq!(action.steps.len(), 3);

        use jokerd::bridge::{DeviceStep, DeviceStepValue};
        let DeviceStep::Write {
            addr,
            type_name,
            value,
        } = &action.steps[0]
        else {
            panic!("expected a write step, got {:?}", action.steps[0]);
        };
        let addr = joker_hal::cbor::Cbor::new(addr);
        assert_eq!(addr.get("at").unwrap().unwrap().as_u64(), Ok(40300));
        assert_eq!(addr.get("fc").unwrap().unwrap().as_u64(), Ok(6));
        assert_eq!(type_name.as_deref(), Some("u16"));
        assert_eq!(
            *value,
            DeviceStepValue::Literal(joker_hal::acquire::OwnedRaw::Int(1))
        );

        assert_eq!(
            action.steps[1],
            DeviceStep::Wait(joker_units::Duration::from_millis(500))
        );
    }

    const JOKER_TOML_WITH_DEDUP_OVERRIDE: &str = r#"
[project]
name  = "bridge-test"
joker = "1.0"
mode  = "dev"

[buses.line0]
kind = "serial"
port = "/dev/ttyUSB0"
baud = 9600

[devices.pcs]
driver  = "modbus-rtu"
bus     = "line0"
profile = "acme/pcs@1"
timeout = "1s"
unit_id = 1

[devices.pcs.actions.reset_fault]
dedup_window = "30s"
role         = "admin"
"#;

    const JOKER_TOML_WITH_VIEWER_ROLE: &str = r#"
[project]
name  = "bridge-test"
joker = "1.0"
mode  = "dev"

[buses.line0]
kind = "serial"
port = "/dev/ttyUSB0"
baud = 9600

[devices.pcs]
driver  = "modbus-rtu"
bus     = "line0"
profile = "acme/pcs@1"
timeout = "1s"
unit_id = 1

[devices.pcs.actions.reset_fault]
role = "viewer"
"#;

    #[test]
    fn a_device_can_override_an_action_s_dedup_window_and_role() {
        let dir = project(&[
            ("joker.toml", JOKER_TOML_WITH_DEDUP_OVERRIDE),
            ("profiles/acme/pcs@1.profile.toml", PROFILE_WITH_ACTION_TOML),
        ]);
        let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(!loaded.report.has_errors(), "{}", loaded.report.render());
        let (context, errors) = jokerd::catalog::context_for_project(&NativeTasks, dir.path());
        assert!(errors.is_empty(), "{errors:?}");
        let device = &loaded.manifest.devices["pcs"];
        let plan = jokerd::bridge::device_plan("pcs", device, &loaded.manifest, &context).unwrap();
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.actions[0].dedup_window_ns, Some(30_000_000_000));
        assert_eq!(plan.actions[0].role, "admin");
    }

    #[test]
    fn a_viewer_role_is_floored_to_operator() {
        let dir = project(&[
            ("joker.toml", JOKER_TOML_WITH_VIEWER_ROLE),
            ("profiles/acme/pcs@1.profile.toml", PROFILE_WITH_ACTION_TOML),
        ]);
        let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(!loaded.report.has_errors(), "{}", loaded.report.render());
        let (context, errors) = jokerd::catalog::context_for_project(&NativeTasks, dir.path());
        assert!(errors.is_empty(), "{errors:?}");
        let device = &loaded.manifest.devices["pcs"];
        let plan = jokerd::bridge::device_plan("pcs", device, &loaded.manifest, &context).unwrap();
        assert_eq!(plan.actions[0].role, "operator", "J0298's own floor rule");
    }

    const GPIO_JOKER_TOML: &str = r#"
[project]
name  = "bridge-test-gpio"
joker = "1.0"
mode  = "dev"

[devices.io]
driver = "gpio"
chip   = "gpiochip0"

[devices.io.signals]
button = { type = "bool", direction = "input", line = 17, active_low = true }
"#;

    #[test]
    fn a_profile_less_device_resolves_points_from_its_own_signal_table() {
        let dir = project(&[("joker.toml", GPIO_JOKER_TOML)]);
        let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(
            loaded.report.codes().is_empty(),
            "{}",
            loaded.report.render()
        );
        let (context, errors) = jokerd::catalog::context_for_project(&NativeTasks, dir.path());
        assert!(errors.is_empty(), "{errors:?}");
        let device = &loaded.manifest.devices["io"];
        let plan = jokerd::bridge::device_plan("io", device, &loaded.manifest, &context).unwrap();
        assert_eq!(plan.driver, "gpio");
        assert!(plan.bus.is_none());
        let cbor = joker_hal::cbor::Cbor::new(&plan.config);
        assert_eq!(cbor.get("chip").unwrap().unwrap().as_str(), Ok("gpiochip0"));
        assert_eq!(plan.points.len(), 1);
        let p = &plan.points[0];
        assert_eq!(p.path, "io.button");
        assert_eq!(p.expect, joker_hal::RawKind::Bool);
        assert_eq!(p.width, 0);
        let addr = joker_hal::cbor::Cbor::new(&p.addr);
        assert_eq!(addr.get("line").unwrap().unwrap().as_u64(), Ok(17));
        assert_eq!(addr.get("active_low").unwrap().unwrap().as_bool(), Ok(true));
        assert!(
            addr.get("type").unwrap().is_none(),
            "generic keys are not in the address"
        );
    }

    #[test]
    fn plan_new_resolves_real_devices_and_skips_sim() {
        let dir = project(&[
            ("joker.toml", JOKER_TOML),
            ("profiles/acme/pcs@1.profile.toml", PROFILE_TOML),
        ]);
        let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(loaded.plan.devices.len(), 1);
        assert_eq!(loaded.plan.devices[0].name, "pcs");

        let sim_dir = project(&[(
            "joker.toml",
            "[project]\nname=\"s\"\njoker=\"1.0\"\nmode=\"dev\"\n[devices.pi]\ndriver=\"sim\"\n[devices.pi.signals]\nact_led={type=\"bool\",direction=\"output\"}\n[outputs.\"pi.act_led\"]\nsafe=false\n",
        )]);
        let sim_loaded = load(
            &OsFileSystem,
            &sim_dir.path().join("joker.toml"),
            &NativeTasks,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert!(
            sim_loaded.plan.devices.is_empty(),
            "sim devices are not resolved into DevicePlan"
        );
    }

    #[test]
    fn owned_request_carries_the_bus_device_and_point_shapes() {
        let dir = project(&[
            ("joker.toml", JOKER_TOML),
            ("profiles/acme/pcs@1.profile.toml", PROFILE_TOML),
        ]);
        let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
            .unwrap_or_else(|e| panic!("{e}"));
        let (context, _) = jokerd::catalog::context_for_project(&NativeTasks, dir.path());
        let device = &loaded.manifest.devices["pcs"];
        let plan = jokerd::bridge::device_plan("pcs", device, &loaded.manifest, &context).unwrap();
        let owned = jokerd::bridge::owned_request(&plan);
        let views = owned.views();
        let req = views.request();
        let bus = req.bus.expect("a serial bus");
        assert_eq!(bus.kind, joker_hal::BusKind::Serial);
        assert_eq!(bus.params.get("baud").unwrap().unwrap().as_u64(), Ok(9600));
        assert_eq!(req.devices.len(), 1);
        assert_eq!(req.devices[0].name, "pcs");
        assert_eq!(req.points.len(), 1);
        assert_eq!(req.points[0].path, "pcs.p_ac");
        assert_eq!(req.points[0].expect, joker_hal::RawKind::Regs);
        assert_eq!(req.points[0].width, 2);
        assert_eq!(req.points[0].direction, joker_hal::Direction::Input);
    }
}
