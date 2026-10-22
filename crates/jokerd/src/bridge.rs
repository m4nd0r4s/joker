//! Turns manifest/profile TOML data (`joker_manifest::value::Node`) into
//! the CBOR a driver's `plan()` expects (ref/06 §2.4.3), and the resolved
//! per-device/per-point data into the owned plan-request types
//! (`joker_hal::spec::owned`) any driver's `Planner`/`Driver` can run
//! against. The driver never reads TOML; this is where the translation
//! happens, once, for every driver family.

use std::collections::BTreeMap;

use joker_hal::cbor::Value as Cbor;
use joker_manifest::value::{Node, Value};

/// Converts one TOML value into the deterministic CBOR a driver's address,
/// config or quirks schema expects. Datetimes have no CBOR equivalent in
/// this subset (ref/06 §2.4.3) and are kept as text, as written.
pub fn node_to_cbor(node: &Node) -> Cbor {
    match &node.value {
        Value::String(s) => Cbor::Text(s.clone()),
        Value::Integer(i) => Cbor::Int(*i),
        Value::Float(f) => Cbor::Float(*f),
        Value::Bool(b) => Cbor::Bool(*b),
        Value::Datetime(s) => Cbor::Text(s.clone()),
        Value::Array(items) => Cbor::Array(items.iter().map(node_to_cbor).collect()),
        Value::Table(table) => Cbor::Map(
            table
                .iter()
                .map(|(k, entry)| (k.clone(), node_to_cbor(&entry.node)))
                .collect(),
        ),
    }
}

/// Encodes a plain `key -> Node` map (as `Device::config` holds it) as one
/// CBOR table.
pub fn table_to_cbor(table: &BTreeMap<String, Node>) -> Vec<u8> {
    Cbor::Map(
        table
            .iter()
            .map(|(k, node)| (k.clone(), node_to_cbor(node)))
            .collect(),
    )
    .encode()
}
