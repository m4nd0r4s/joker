//! The drivers, profiles and boards the daemon knows, as the manifest
//! checks see them (`Context`, ref/01 §8.1).
//!
//! This is the M0 catalogue: `sim`, `sysfs` and `modbus-rtu`, and the bench
//! profile `local/bench-esp32@1` (docs/26 §6). The driver crates will
//! describe themselves once `joker-hal` lands; this module then only merges
//! their descriptions.

use std::collections::BTreeMap;

use joker_manifest::Context;
use joker_manifest::context::Direction;
use joker_manifest::context::{DriverInfo, ProfileInfo, SignalInfo, SignalType};
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
            poll: Some(Duration::from_millis(100)),
            timeout: None,
        },
    );
    p
}

/// The context of the manifest checks: the M0 catalogue plus the apps the
/// task providers can run.
pub fn context(tasks: &dyn TaskProvider) -> Context {
    Context {
        drivers: drivers(),
        profiles: profiles(),
        apps: tasks.interfaces(),
        ..Context::default()
    }
}
