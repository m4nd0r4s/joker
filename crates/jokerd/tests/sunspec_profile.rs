//! AUR-400, ADR 0077: the profile generated from the SunSpec discovery of
//! a simulated inverter and meter is a profile jokerd loads and plans with
//! the real `modbus-tcp` planner, and its signals are typed, with their
//! unit, at the addresses of the chain, scaled by the scale factors read
//! at discovery.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use joker_core::ValueType;
use joker_driver_modbus::ModbusTcpPlanner;
use joker_driver_modbus::sunspec::{
    Discovery, Found, Image, ReadError, Step, Walk, common_body, profile,
};
use joker_hal::Planner;
use joker_hal::acquire::OwnedRaw;
use joker_manifest::OsFileSystem;
use jokerd::drivers::{Decode, acquire};
use jokerd::load::{Loaded, load};
use jokerd::native::NativeTasks;

fn device() -> Image {
    let mut image = Image::new(40000);
    image.model(1, &common_body("ACME", "PV-30", "2.1.7", "SN-0042"));
    let mut inverter = vec![0u16; 50];
    inverter[4] = (-2i16) as u16;
    inverter[11] = (-1i16) as u16;
    inverter[13] = 1;
    inverter[15] = (-2i16) as u16;
    inverter[24] = 0;
    image.model(103, &inverter);
    let mut meter = vec![0u16; 105];
    meter[20] = 0;
    meter[52] = 3;
    image.model(203, &meter);
    image.end();
    image
}

fn discover(image: &Image) -> Found {
    let d = Discovery::default();
    let mut walk = Walk::new(&d);
    loop {
        match walk.step() {
            Step::Read { at, count } => {
                let words = image.read(at, count);
                walk.feed(words.as_deref().ok_or(ReadError::Exception));
            }
            Step::Done => return walk.found(),
            Step::Failed(e) => panic!("{e:?}"),
        }
    }
}

const MANIFEST: &str = r#"
[project]
name  = "sunspec-profile"
joker = "1.0"
mode  = "dev"

[devices.pv]
driver  = "modbus-tcp"
address = "192.168.10.30:502"
unit_id = 1
profile = "local/pv-30@1"
poll    = "1s"
"#;

fn loaded(text: &str) -> Loaded {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("joker.toml"), MANIFEST).unwrap();
    std::fs::create_dir_all(dir.path().join("profiles/local")).unwrap();
    std::fs::write(dir.path().join("profiles/local/pv-30@1.profile.toml"), text).unwrap();
    load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
        .unwrap_or_else(|e| panic!("{e}\n{text}"))
}

fn value(l: &Loaded, path: &str, raw: OwnedRaw) -> f64 {
    let p = l.plan.devices[0]
        .points
        .iter()
        .find(|p| p.path == path)
        .unwrap_or_else(|| panic!("{path}"));
    let ty = l.plan.signal(path).unwrap().ty;
    let bits = acquire(
        &raw,
        &Decode::of_point(&p.type_name, &p.addr),
        &p.acquisition,
        ty,
    )
    .unwrap();
    ty.bits_to_f64(bits).unwrap()
}

#[test]
fn the_generated_profile_loads_plans_and_reads_typed_signals() {
    let found = discover(&device());
    let mut names = BTreeMap::new();
    names.insert("103.Hz".to_owned(), "ac.f".to_owned());
    let text = profile(&found, "local/pv-30", &names);
    let l = loaded(&text);

    let w = l.plan.signal("pv.m103.w").unwrap();
    assert_eq!(w.ty, ValueType::F64, "a scaled integer is a float");
    assert_eq!(value(&l, "pv.m103.w", OwnedRaw::Regs(vec![1234])), 12_340.0);
    assert!((value(&l, "pv.m103.a", OwnedRaw::Regs(vec![1234])) - 12.34).abs() < 1e-9);
    assert!((value(&l, "pv.ac.f", OwnedRaw::Regs(vec![5001])) - 50.01).abs() < 1e-9);
    assert_eq!(
        value(
            &l,
            "pv.m203.tot_wh_imp",
            OwnedRaw::Regs(vec![0x0001, 0x0002])
        ),
        65_538_000.0,
        "a u32 counter scaled by TotWh_SF = 3"
    );
    assert_eq!(
        l.plan.signal("pv.m103.st").unwrap().ty,
        ValueType::Enum,
        "the operating state is the InverterState enum"
    );
    let p = l.plan.devices[0]
        .points
        .iter()
        .find(|p| p.path == "pv.m103.w")
        .unwrap();
    assert_eq!(
        joker_hal::Cbor::new(&p.addr)
            .get("at")
            .unwrap()
            .unwrap()
            .as_u64()
            .unwrap(),
        40072 + 12,
        "W is register 12 of the body of 103, whose id register is 40070"
    );

    let owned = jokerd::bridge::owned_request(&l.plan.devices[0]);
    let views = owned.views();
    let plan = ModbusTcpPlanner::plan(&views.request())
        .unwrap_or_else(|e| panic!("the driver plans the generated profile: {e:?}"));
    let covered: usize = plan.view().iter().map(|b| b.points.len()).sum();
    assert_eq!(
        covered,
        owned.points.len(),
        "every point of the chain is in a batch"
    );
}
