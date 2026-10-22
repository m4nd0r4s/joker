//! `jokerd::bridge`: turning manifest/profile TOML data into the CBOR a
//! driver's `plan()` expects.

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
