#![allow(clippy::unwrap_used)]

use joker_classes_std::{AnalogIn, Button, Led, LedMethod, v1};

#[test]
fn the_generated_items_follow_the_contracts() {
    let contracts = joker_jkc::stdlib::contracts();
    let led = &contracts["std/led@1"];
    assert_eq!(Led::REFERENCE, "std/led@1");
    assert_eq!(Led::VERSION, "1.0.0");
    assert_eq!(Led::CONTRACT_HASH, led.contract_hash());
    assert_eq!(Led::CAPABILITIES, ["dimmable"]);
    assert_eq!(Led::ON, "on");
    assert_eq!(Led::BRIGHTNESS, "brightness");
    let names: Vec<&str> = LedMethod::ALL.iter().map(|m| m.name()).collect();
    let contract: Vec<&str> = led
        .contract
        .methods
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    assert_eq!(names, contract);
    assert_eq!(LedMethod::Toggle.number(), 4);
    assert_eq!(LedMethod::Dim.arity(), 1);
    assert_eq!(Button::CLICKED, "clicked");
    assert_eq!(AnalogIn::SMOOTH, "smooth");
    assert_eq!(v1::TempSensor::TEMPERATURE, "temperature");
}

#[test]
fn generation_is_deterministic() {
    let contracts = joker_jkc::stdlib::contracts();
    let a = joker_jkc::codegen::rust_package(contracts.values());
    let b = joker_jkc::codegen::rust_package(joker_jkc::stdlib::contracts().values());
    assert_eq!(a, b);
    assert!(a.contains("pub mod v1 {"));
    assert!(a.contains("pub use v1::{Led, LedMethod};"));
}
