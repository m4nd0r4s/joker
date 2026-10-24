//! `jokerd::drivers::raw_to_bits` and `decode_to_bits`: a settled raw
//! value into the core's transport bits (ref/06 §2.7, ref/03 §3 step 1).

#![allow(clippy::unwrap_used)]

use joker_core::ValueType;
use joker_hal::acquire::OwnedRaw;
use joker_hal::cbor::Value as Cbor;
use jokerd::drivers::{Decode, RawType, decode_to_bits, raw_to_bits};

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
fn an_f32_signal_round_trips_through_the_core_encoding() {
    let bits = raw_to_bits(&OwnedRaw::Float(21.5), ValueType::F32).unwrap();
    assert_eq!(ValueType::F32.bits_to_f64(bits), Some(21.5));
    let bits = raw_to_bits(&OwnedRaw::Float(0.1), ValueType::F32).unwrap();
    assert_eq!(ValueType::F32.bits_to_f64(bits), Some(f64::from(0.1f32)));
}

#[test]
fn a_type_mismatch_is_none() {
    assert_eq!(raw_to_bits(&OwnedRaw::Bool(true), ValueType::F64), None);
    assert_eq!(raw_to_bits(&OwnedRaw::Int(1), ValueType::Bool), None);
}

#[test]
fn raw_to_bits_only_carries_already_typed_scalars() {
    assert_eq!(raw_to_bits(&OwnedRaw::Regs(vec![1]), ValueType::I64), None);
    assert_eq!(
        raw_to_bits(&OwnedRaw::Bytes(vec![1, 2]), ValueType::I64),
        None
    );
    assert_eq!(raw_to_bits(&OwnedRaw::None, ValueType::Bool), None);
}

fn regs(type_name: &str, addr: &[(&str, &str)]) -> Decode {
    let entries = addr
        .iter()
        .map(|(k, v)| ((*k).to_owned(), Cbor::text(v)))
        .collect();
    Decode::of_point(type_name, &Cbor::Map(entries).encode())
}

#[test]
fn single_registers_decode_signed_and_unsigned() {
    let u16_ = regs("u16", &[]);
    assert_eq!(
        decode_to_bits(&OwnedRaw::Regs(vec![0x1234]), &u16_, ValueType::U64),
        Some(0x1234)
    );
    assert_eq!(
        decode_to_bits(&OwnedRaw::Regs(vec![0x1234]), &u16_, ValueType::I64),
        Some(0x1234)
    );
    let i16_ = regs("i16", &[]);
    assert_eq!(
        decode_to_bits(&OwnedRaw::Regs(vec![0xFFFE]), &i16_, ValueType::I64),
        Some((-2i64) as u64)
    );
    assert_eq!(
        decode_to_bits(&OwnedRaw::Regs(vec![0xFFFE]), &i16_, ValueType::U64),
        None,
        "a negative value never becomes an unsigned signal"
    );
}

#[test]
fn word_and_byte_order_follow_the_address() {
    let raw = OwnedRaw::Regs(vec![0x0001, 0x0002]);
    assert_eq!(
        decode_to_bits(&raw, &regs("u32", &[]), ValueType::U64),
        Some(0x0001_0002)
    );
    assert_eq!(
        decode_to_bits(
            &raw,
            &regs("u32", &[("word_order", "little")]),
            ValueType::U64
        ),
        Some(0x0002_0001)
    );
    assert_eq!(
        decode_to_bits(
            &OwnedRaw::Regs(vec![0x3412]),
            &regs("u16", &[("byte_order", "little")]),
            ValueType::U64
        ),
        Some(0x1234)
    );
}

#[test]
fn an_f32_in_two_registers_reaches_the_core_as_its_value() {
    let bits = 21.5f32.to_bits();
    let (hi, lo) = ((bits >> 16) as u16, bits as u16);
    let big = decode_to_bits(
        &OwnedRaw::Regs(vec![hi, lo]),
        &regs("f32", &[]),
        ValueType::F32,
    )
    .unwrap();
    assert_eq!(ValueType::F32.bits_to_f64(big), Some(21.5));
    let little = decode_to_bits(
        &OwnedRaw::Regs(vec![lo, hi]),
        &regs("f32", &[("word_order", "little")]),
        ValueType::F64,
    )
    .unwrap();
    assert_eq!(f64::from_bits(little), 21.5);
}

#[test]
fn a_bool_reads_one_bit_of_a_register() {
    let mut decode = regs("bool", &[]);
    decode.bit = Some(3);
    assert_eq!(
        decode_to_bits(&OwnedRaw::Regs(vec![0b1000]), &decode, ValueType::Bool),
        Some(1)
    );
    assert_eq!(
        decode_to_bits(&OwnedRaw::Regs(vec![0b0111]), &decode, ValueType::Bool),
        Some(0)
    );
}

#[test]
fn byte_buffers_decode_with_their_byte_order() {
    let raw = OwnedRaw::Bytes(vec![0x01, 0x02]);
    assert_eq!(
        decode_to_bits(&raw, &regs("u16", &[]), ValueType::U64),
        Some(0x0102)
    );
    assert_eq!(
        decode_to_bits(
            &raw,
            &regs("u16", &[("byte_order", "little")]),
            ValueType::U64
        ),
        Some(0x0201)
    );
    assert_eq!(
        decode_to_bits(
            &OwnedRaw::Bytes(vec![0xFF, 0xFE]),
            &regs("i16", &[]),
            ValueType::I64
        ),
        Some((-2i64) as u64)
    );
}

#[test]
fn a_length_that_does_not_match_the_type_is_none() {
    assert_eq!(
        decode_to_bits(&OwnedRaw::Regs(vec![1]), &regs("u32", &[]), ValueType::U64),
        None
    );
    assert_eq!(
        decode_to_bits(&OwnedRaw::Bytes(vec![1]), &regs("u16", &[]), ValueType::U64),
        None
    );
    assert_eq!(
        decode_to_bits(
            &OwnedRaw::Regs(vec![1]),
            &regs("string", &[]),
            ValueType::U64
        ),
        None
    );
    assert_eq!(RawType::parse("string"), RawType::Other);
}

#[test]
fn already_typed_values_skip_the_decoding() {
    assert_eq!(
        decode_to_bits(&OwnedRaw::Float(1.5), &Decode::TYPED, ValueType::F64),
        Some(1.5f64.to_bits())
    );
}
