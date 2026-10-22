//! `jokerd::drivers::raw_to_bits`: a settled raw scalar into the core's
//! transport bits (ref/06 §2.7, the scalar case only).

use joker_core::ValueType;
use joker_hal::acquire::OwnedRaw;
use jokerd::drivers::raw_to_bits;

#[test]
fn bool_values_become_0_or_1() {
    assert_eq!(raw_to_bits(&OwnedRaw::Bool(true), ValueType::Bool), Some(1));
    assert_eq!(
        raw_to_bits(&OwnedRaw::Bool(false), ValueType::Bool),
        Some(0)
    );
}

#[test]
fn int_and_uint_round_trip_as_bits() {
    assert_eq!(
        raw_to_bits(&OwnedRaw::Int(-5), ValueType::I64),
        Some((-5i64) as u64)
    );
    assert_eq!(raw_to_bits(&OwnedRaw::Uint(42), ValueType::U64), Some(42));
}

#[test]
fn float_becomes_its_ieee_bits() {
    assert_eq!(
        raw_to_bits(&OwnedRaw::Float(1.5), ValueType::F64),
        Some(1.5f64.to_bits())
    );
}

#[test]
fn a_type_mismatch_is_none() {
    assert_eq!(raw_to_bits(&OwnedRaw::Bool(true), ValueType::F64), None);
    assert_eq!(raw_to_bits(&OwnedRaw::Int(1), ValueType::Bool), None);
}

#[test]
fn regs_bytes_and_text_are_not_supported_yet() {
    assert_eq!(raw_to_bits(&OwnedRaw::Regs(vec![1]), ValueType::I64), None);
    assert_eq!(
        raw_to_bits(&OwnedRaw::Bytes(vec![1, 2]), ValueType::I64),
        None
    );
    assert_eq!(raw_to_bits(&OwnedRaw::None, ValueType::Bool), None);
}
