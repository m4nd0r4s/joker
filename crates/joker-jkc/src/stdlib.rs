//! The standard class library `std/` (docs/14 §10), embedded so the node,
//! `jk` and the tests know it without a registry: each class with its
//! conformance suite (ref/03 §2.7).

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::Compilation;
use crate::contract::ContractFile;

/// One class of `std/`: its source and its conformance suite.
#[derive(Debug, Clone, Copy)]
pub struct StdClass {
    /// The file name without the suffix (`led`).
    pub name: &'static str,
    /// The `.jkc` text.
    pub source: &'static str,
    /// The `.conformance.toml` text.
    pub conformance: &'static str,
}

macro_rules! std_class {
    ($name:literal) => {
        StdClass {
            name: $name,
            source: include_str!(concat!("../../../classes/std/", $name, ".jkc")),
            conformance: include_str!(concat!("../../../classes/std/", $name, ".conformance.toml")),
        }
    };
}

/// The classes of `std/` this runtime ships, sorted by name.
pub const CLASSES: &[StdClass] = &[
    std_class!("analog_in"),
    std_class!("analog_out"),
    std_class!("button"),
    std_class!("humidity_sensor"),
    std_class!("lamp"),
    std_class!("led"),
    std_class!("pressure_sensor"),
    std_class!("pump"),
    std_class!("relay"),
    std_class!("switch"),
    std_class!("temp_sensor"),
];

/// `classes/std/std.package.toml`.
pub const PACKAGE: &str = include_str!("../../../classes/std/std.package.toml");

/// The sources of `std/`, as [`crate::compile`] takes them.
pub fn sources() -> Vec<(PathBuf, String)> {
    CLASSES
        .iter()
        .map(|c| {
            (
                PathBuf::from(format!("classes/std/{}.jkc", c.name)),
                c.source.to_owned(),
            )
        })
        .collect()
}

/// Compiles `std/` alone.
pub fn compile() -> Compilation {
    crate::compile(sources())
}

/// The contracts of `std/`, by reference (`std/led@1`); empty if `std/`
/// does not compile, which its tests rule out.
pub fn contracts() -> BTreeMap<String, ContractFile> {
    let compilation = compile();
    if compilation.has_errors() {
        return BTreeMap::new();
    }
    compilation.classes
}
