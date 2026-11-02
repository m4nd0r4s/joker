//! Method records through the proposal buffer (ref/04 §2.3, §3.3–§3.4,
//! ADR 0107): a `METHOD` record of a bound device reaches the group's
//! image as a core method call, in its instance's numbering and with its
//! arguments decoded; arguments that do not fit are `J-E0504` without a
//! trap, and a device out of range is a malformed buffer.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Bench, TASK_ECHO, TASK_STRAY, TASK_TELL, TASK_TELL_IO, leak};
use joker_core::image::{MethodCall, Source};
use joker_core::{ValueType, WriterId};
use joker_host::{
    AppComponent, CycleInstance, CycleStatus, DeviceBinding, HostEngine, InstanceLimits,
    MethodBinding, PoolConfig, RuntimeMode,
};

const LAMP_INSTANCE: u32 = 7;
const SET_METHOD: u16 = 3;

fn setup() -> (HostEngine, AppComponent, Bench) {
    let engine = HostEngine::pooled(PoolConfig { instances: 2 }).unwrap();
    let app = AppComponent::compile(&engine, &common::echo_with_lamp(), RuntimeMode::Prod).unwrap();
    assert_eq!(app.meta().devices.len(), 1);
    assert_eq!(app.meta().devices[0].methods, ["set"]);
    let mut bench = Bench::new();
    bench
        .image
        .set_method_buffer(leak(vec![MethodCall::EMPTY; 8]));
    (engine, app, bench)
}

fn lamp(engine: &HostEngine, app: &AppComponent, bench: &Bench) -> CycleInstance {
    let methods = [MethodBinding {
        method: SET_METHOD,
        params: &[ValueType::F64],
    }];
    let devices = [DeviceBinding {
        name: "lamp1",
        class_version: "1.0.0",
        instance: LAMP_INSTANCE,
        methods: &methods,
    }];
    bench
        .try_instance_with(engine, app, InstanceLimits::default(), &devices)
        .unwrap()
}

fn expected(task: u16) -> MethodCall {
    let mut call = MethodCall {
        writer: WriterId(0),
        instance: LAMP_INSTANCE,
        method: SET_METHOD,
        n_args: 1,
        source: Source::Cycle,
        task,
        ..MethodCall::EMPTY
    };
    call.args[0] = 2.5f64.to_bits();
    call
}

#[test]
fn a_method_record_becomes_a_core_method_call() {
    let (engine, app, mut bench) = setup();
    let mut instance = lamp(&engine, &app, &bench);
    instance.set_causal_task(0);
    let report = bench.run(&mut instance, 0, Some(2.5), TASK_TELL);
    assert_eq!(report.status, CycleStatus::Ok);
    assert_eq!(report.staged, 1);
    assert_eq!(bench.image.method_calls(), &[expected(0)]);
    assert_eq!(instance.rejections().len(), 1);
    assert_eq!(instance.rejections()[0].code, "J-E0504");
    assert!(!instance.is_poisoned());
}

#[test]
fn a_call_through_cycle_io_is_staged_like_a_record() {
    let (engine, app, mut bench) = setup();
    let mut instance = lamp(&engine, &app, &bench);
    instance.set_causal_task(0);
    let report = bench.run(&mut instance, 0, Some(2.5), TASK_TELL_IO);
    assert_eq!((report.status, report.staged), (CycleStatus::Ok, 1));
    assert_eq!(bench.image.method_calls(), &[expected(0)]);
    assert!(instance.rejections().is_empty());
}

#[test]
fn a_method_record_of_a_device_out_of_range_is_a_malformed_buffer() {
    let (engine, app, mut bench) = setup();
    let mut instance = lamp(&engine, &app, &bench);
    let report = bench.run(&mut instance, 0, Some(2.5), TASK_STRAY);
    assert_eq!(report.status, CycleStatus::Trap("J-E0907"));
    assert!(bench.image.method_calls().is_empty());
}

#[test]
fn the_devices_must_match_the_bindings() {
    let (engine, app, bench) = setup();
    let err = bench
        .try_instance(&engine, &app, InstanceLimits::default())
        .unwrap_err();
    assert_eq!(err.code(), "J-E0008", "{err}");
    let mut methods_short = Bench::new();
    methods_short
        .image
        .set_method_buffer(leak(vec![MethodCall::EMPTY; 8]));
    let devices = [DeviceBinding {
        name: "lamp1",
        class_version: "1.0.0",
        instance: LAMP_INSTANCE,
        methods: &[],
    }];
    let err = methods_short
        .try_instance_with(&engine, &app, InstanceLimits::default(), &devices)
        .unwrap_err();
    assert_eq!(err.code(), "J-E0008", "{err}");
}

#[test]
fn an_app_without_devices_still_writes() {
    let (engine, app, mut bench) = setup();
    let mut instance = lamp(&engine, &app, &bench);
    let report = bench.run(&mut instance, 0, Some(2.0), TASK_ECHO);
    assert_eq!((report.status, report.staged), (CycleStatus::Ok, 3));
    assert!(bench.image.method_calls().is_empty());
}
