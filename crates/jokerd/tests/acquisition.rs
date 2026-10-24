//! `jokerd::drivers::acquire` and `Deadband`: the acquisition steps of a
//! profile point after decoding (ref/03 §3, ref/06 §2.7, ADR 0064).

#![allow(clippy::unwrap_used)]

use joker_core::{Quality, QualityCode, QualityReason, ValueType};
use joker_hal::acquire::OwnedRaw;
use joker_hal::cbor::Value as Cbor;
use joker_manifest::context::Acquisition;
use jokerd::drivers::{Deadband, Decode, acquire};

fn point(type_name: &str, addr: &[(&str, &str)]) -> Decode {
    let entries = addr
        .iter()
        .map(|(k, v)| ((*k).to_owned(), Cbor::text(v)))
        .collect();
    Decode::of_point(type_name, &Cbor::Map(entries).encode())
}

fn scaled(scale: f64, offset: f64) -> Acquisition {
    Acquisition {
        scale: Some(scale),
        offset: Some(offset),
        ..Acquisition::NONE
    }
}

fn f64_of(bits: u64) -> f64 {
    f64::from_bits(bits)
}

#[test]
fn a_scaled_register_becomes_its_engineering_value() {
    let bits = acquire(
        &OwnedRaw::Regs(vec![0xFFF6]),
        &point("i16", &[]),
        &scaled(0.1, 0.0),
        ValueType::F64,
    )
    .unwrap();
    assert!((f64_of(bits) - -1.0).abs() < 1e-12, "{}", f64_of(bits));
}

#[test]
fn scale_then_offset_on_a_two_register_value_in_little_word_order() {
    let bits = acquire(
        &OwnedRaw::Regs(vec![0x0001, 0x0000]),
        &point("u32", &[("word_order", "little")]),
        &scaled(0.5, 10.0),
        ValueType::F64,
    )
    .unwrap();
    assert_eq!(f64_of(bits), 10.5);
}

#[test]
fn an_unscaled_point_keeps_its_integer() {
    let bits = acquire(
        &OwnedRaw::Regs(vec![7]),
        &point("u16", &[]),
        &Acquisition::NONE,
        ValueType::U64,
    )
    .unwrap();
    assert_eq!(bits, 7);
}

#[test]
fn invert_flips_a_bool() {
    let invert = Acquisition {
        invert: true,
        ..Acquisition::NONE
    };
    assert_eq!(
        acquire(
            &OwnedRaw::Bool(true),
            &Decode::TYPED,
            &invert,
            ValueType::Bool
        ),
        Ok(0)
    );
    assert_eq!(
        acquire(
            &OwnedRaw::Regs(vec![0b100]),
            &point("bool", &[]),
            &invert,
            ValueType::Bool
        ),
        Ok(0)
    );
}

#[test]
fn an_enum_integer_becomes_the_position_of_its_variant() {
    let mode = Acquisition {
        enum_values: vec![0, 7, 1],
        ..Acquisition::NONE
    };
    let decode = point("u16", &[]);
    assert_eq!(
        acquire(&OwnedRaw::Regs(vec![7]), &decode, &mode, ValueType::Enum),
        Ok(1)
    );
    assert_eq!(
        acquire(&OwnedRaw::Regs(vec![1]), &decode, &mode, ValueType::Enum),
        Ok(2)
    );
}

#[test]
fn an_enum_integer_without_a_variant_is_out_of_range() {
    let mode = Acquisition {
        enum_values: vec![0, 1],
        ..Acquisition::NONE
    };
    assert_eq!(
        acquire(
            &OwnedRaw::Regs(vec![5]),
            &point("u16", &[]),
            &mode,
            ValueType::Enum
        ),
        Err(QualityReason::OutOfRange)
    );
}

#[test]
fn a_nan_is_bad_nan_and_never_a_number() {
    let nan = f32::NAN.to_bits();
    assert_eq!(
        acquire(
            &OwnedRaw::Regs(vec![(nan >> 16) as u16, nan as u16]),
            &point("f32", &[]),
            &Acquisition::NONE,
            ValueType::F64
        ),
        Err(QualityReason::Nan)
    );
    assert_eq!(
        acquire(
            &OwnedRaw::Float(f64::INFINITY),
            &Decode::TYPED,
            &scaled(2.0, 0.0),
            ValueType::F64
        ),
        Err(QualityReason::Nan)
    );
}

#[test]
fn an_integer_that_does_not_fit_the_signal_is_out_of_range() {
    assert_eq!(
        acquire(
            &OwnedRaw::Uint(u64::MAX),
            &Decode::TYPED,
            &Acquisition::NONE,
            ValueType::I64
        ),
        Err(QualityReason::OutOfRange)
    );
    assert_eq!(
        acquire(
            &OwnedRaw::Regs(vec![0xFFFF]),
            &point("i16", &[]),
            &Acquisition::NONE,
            ValueType::U64
        ),
        Err(QualityReason::OutOfRange)
    );
}

#[test]
fn a_value_of_the_wrong_form_is_a_config_error() {
    assert_eq!(
        acquire(
            &OwnedRaw::Regs(vec![1, 2, 3]),
            &point("u32", &[]),
            &Acquisition::NONE,
            ValueType::U64
        ),
        Err(QualityReason::ConfigError)
    );
    assert_eq!(
        acquire(
            &OwnedRaw::Bool(true),
            &Decode::TYPED,
            &scaled(2.0, 0.0),
            ValueType::F64
        ),
        Err(QualityReason::ConfigError)
    );
}

#[test]
fn the_deadband_keeps_the_last_value_until_a_change_leaves_it() {
    let good = Quality::new(QualityCode::Good, QualityReason::None);
    let bad = Quality::new(QualityCode::Bad, QualityReason::CommFailure);
    let ty = ValueType::F64;
    let mut band = Deadband::default();
    let first = 10.0f64.to_bits();
    assert_eq!(band.filter(Some(0.5), ty, first, good), first);
    assert_eq!(band.filter(Some(0.5), ty, 10.4f64.to_bits(), good), first);
    assert_eq!(band.filter(Some(0.5), ty, 9.7f64.to_bits(), good), first);
    let moved = 10.6f64.to_bits();
    assert_eq!(band.filter(Some(0.5), ty, moved, good), moved);
    assert_eq!(band.filter(Some(0.5), ty, 0, bad), 0);
    let after = 10.7f64.to_bits();
    assert_eq!(
        band.filter(Some(0.5), ty, after, good),
        after,
        "a bad sample resets the reference"
    );
}

#[test]
fn without_a_deadband_every_value_is_published() {
    let good = Quality::new(QualityCode::Good, QualityReason::None);
    let mut band = Deadband::default();
    assert_eq!(band.filter(None, ValueType::I64, 3, good), 3);
    assert_eq!(band.filter(None, ValueType::I64, 4, good), 4);
}
