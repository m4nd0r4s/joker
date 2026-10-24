//! [`Devices`] for every driver family with a complete real Linux
//! adapter: `sysfs` (`Link::Paths`), `modbus-rtu` (`Link::Serial` over
//! `TokioSerial`), `modbus-tcp` (`Link::Tcp` over `TokioTcpConnector`)
//! and `gpio` (`Link::Gpio` over `LinuxGpio`, chardev v1). `i2c` is not
//! dispatched here yet: its driver exists and is tested against an
//! in-memory double, but the real Linux `i2c-dev` adapter (opening
//! `/dev/i2c-N` and issuing the ioctl) is not implemented (AUR-401). A
//! device declaring it is reported, not silently ignored.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use joker_core::ClockQuality;
use joker_driver_gpio::{GpioDriver, GpioPlanner};
use joker_driver_modbus::{ModbusPlanner, ModbusRtu, ModbusTcp, ModbusTcpPlanner};
use joker_driver_sysfs::{SysfsDriver, SysfsPlanner};
use joker_hal::link::DevicePaths;
use joker_hal::std_host::{LinuxGpio, SerialConfig, TokioHost, TokioSerial, TokioTcpConnector};
use joker_hal::{Cbor, Direction, Driver, GpioChip, Link, OpenArgs, Planner};
use tokio::sync::watch;

use crate::bridge::{DevicePlan, owned_request};
use crate::devices::{DeviceSet, Devices};
use crate::drivers::{ActionCall, point_targets, run_driver};
use crate::plan::Plan;
use crate::shared::Shared;

fn tcp_addr(config: &[u8]) -> Option<SocketAddr> {
    let cbor = Cbor::new(config);
    let host = cbor.get("host").ok()??.as_str().ok()?;
    let port = cbor
        .get("port")
        .ok()
        .flatten()
        .and_then(|p| p.as_u64().ok())
        .and_then(|p| u16::try_from(p).ok())
        .unwrap_or(502);
    format!("{host}:{port}").parse().ok()
}

#[allow(
    clippy::too_many_arguments,
    reason = "each parameter is a distinct, independently-sourced piece of one device's dispatch (plan, index, shared state, clock, stop signal, sysfs root, action channel); a wrapper struct would only rename this list, not shrink it, matching run_driver's own allow"
)]
async fn run_sysfs(
    device: DevicePlan,
    plan: Arc<Plan>,
    device_index: usize,
    shared: Arc<Shared>,
    host: Arc<TokioHost>,
    mut stop: watch::Receiver<bool>,
    sysfs_root: Arc<str>,
    action_rx: tokio::sync::mpsc::Receiver<ActionCall>,
) {
    let owned = owned_request(&device);
    let views = owned.views();
    let Ok(hal_plan) = SysfsPlanner::plan(&views.request()) else {
        return;
    };
    let paths = DevicePaths::new(
        &*sysfs_root,
        &joker_driver_sysfs::grants(&views.request())
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    let Ok(driver) = SysfsDriver::open(
        &OpenArgs::new(views.request(), hal_plan.view()),
        Link::Paths(paths),
    ) else {
        return;
    };
    let targets = point_targets(&plan, device_index);
    run_driver(
        driver,
        &*host,
        views.request(),
        hal_plan.view(),
        device.timeout_ns,
        &targets,
        &shared,
        device_index,
        device.fallback_keepalive_ns,
        action_rx,
        move || *stop.borrow_and_update(),
    )
    .await;
}

async fn run_modbus_rtu(
    device: DevicePlan,
    plan: Arc<Plan>,
    device_index: usize,
    shared: Arc<Shared>,
    host: Arc<TokioHost>,
    mut stop: watch::Receiver<bool>,
    action_rx: tokio::sync::mpsc::Receiver<ActionCall>,
) {
    let owned = owned_request(&device);
    let views = owned.views();
    let Ok(hal_plan) = ModbusPlanner::plan(&views.request()) else {
        return;
    };
    let Ok(config) = SerialConfig::from_params(Cbor::new(&device.bus_params)) else {
        return;
    };
    let Ok(port) = TokioSerial::open(&config) else {
        return;
    };
    let Ok(driver) = ModbusRtu::open(
        &OpenArgs::new(views.request(), hal_plan.view()),
        Link::Serial(joker_hal::link::SerialPort::new(port)),
    ) else {
        return;
    };
    let targets = point_targets(&plan, device_index);
    run_driver(
        driver,
        &*host,
        views.request(),
        hal_plan.view(),
        device.timeout_ns,
        &targets,
        &shared,
        device_index,
        device.fallback_keepalive_ns,
        action_rx,
        move || *stop.borrow_and_update(),
    )
    .await;
}

async fn run_modbus_tcp(
    device: DevicePlan,
    plan: Arc<Plan>,
    device_index: usize,
    shared: Arc<Shared>,
    host: Arc<TokioHost>,
    mut stop: watch::Receiver<bool>,
    action_rx: tokio::sync::mpsc::Receiver<ActionCall>,
) {
    let owned = owned_request(&device);
    let views = owned.views();
    let Ok(hal_plan) = ModbusTcpPlanner::plan(&views.request()) else {
        return;
    };
    let Some(addr) = tcp_addr(&device.config) else {
        return;
    };
    let endpoint = joker_hal::link::TcpEndpoint::new(TokioTcpConnector::new(addr));
    let Ok(driver) = ModbusTcp::open(
        &OpenArgs::new(views.request(), hal_plan.view()),
        Link::Tcp(endpoint),
    ) else {
        return;
    };
    let targets = point_targets(&plan, device_index);
    run_driver(
        driver,
        &*host,
        views.request(),
        hal_plan.view(),
        device.timeout_ns,
        &targets,
        &shared,
        device_index,
        device.fallback_keepalive_ns,
        action_rx,
        move || *stop.borrow_and_update(),
    )
    .await;
}

/// The logical level a gpio output is requested at: its declared `safe`
/// (AGENTS.md invariant 5). A line has no state to hand back to, so
/// `safe = "relinquish"` and an output without an envelope refuse the
/// device instead of starting the line at an undeclared level.
fn gpio_safe_level(plan: &Plan, path: &str) -> Result<bool, String> {
    let output = plan
        .outputs
        .iter()
        .find(|o| o.path == path)
        .ok_or_else(|| format!("output `{path}` has no envelope, so no `safe` to start at"))?;
    gpio_safe(output.safe, path)
}

/// A gpio output's `safe` as a logical level; see [`gpio_safe_level`].
fn gpio_safe(safe: joker_core::SafeValue, path: &str) -> Result<bool, String> {
    match safe {
        joker_core::SafeValue::Value(0) => Ok(false),
        joker_core::SafeValue::Value(1) => Ok(true),
        joker_core::SafeValue::Value(_) => {
            Err(format!("output `{path}`: a gpio line's `safe` is a bool"))
        }
        joker_core::SafeValue::Relinquish => Err(format!(
            "output `{path}`: a gpio line cannot relinquish; declare `safe = true` or `false`"
        )),
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "each parameter is a distinct, independently-sourced piece of one device's dispatch; see run_sysfs's own allow"
)]
async fn run_gpio(
    device: DevicePlan,
    plan: Arc<Plan>,
    device_index: usize,
    shared: Arc<Shared>,
    host: Arc<TokioHost>,
    mut stop: watch::Receiver<bool>,
    action_rx: tokio::sync::mpsc::Receiver<ActionCall>,
) {
    let owned = owned_request(&device);
    let views = owned.views();
    let Ok(hal_plan) = GpioPlanner::plan(&views.request()) else {
        return;
    };
    let Ok(chip) = joker_driver_gpio::plan::chip_id(Cbor::new(&device.config)) else {
        return;
    };
    let mut lines = Vec::new();
    for p in views.request().points {
        let Ok(addr) = joker_driver_gpio::plan::check_point(p) else {
            return;
        };
        let safe = if p.direction == Direction::Output {
            match gpio_safe_level(&plan, p.path) {
                Ok(level) => Some(level),
                Err(e) => {
                    eprintln!("error: device `{}`: {e}", device.name);
                    return;
                }
            }
        } else {
            None
        };
        lines.push(addr.line_config(safe));
    }
    let gpio = match LinuxGpio::open(&PathBuf::from("/dev").join(&chip), &lines) {
        Ok(gpio) => gpio,
        Err(e) => {
            eprintln!(
                "error: device `{}`: cannot request the lines of `{chip}`: {e}",
                device.name
            );
            return;
        }
    };
    let Ok(driver) = GpioDriver::open(
        &OpenArgs::new(views.request(), hal_plan.view()),
        Link::Gpio(GpioChip::new(gpio)),
    ) else {
        return;
    };
    let targets = point_targets(&plan, device_index);
    run_driver(
        driver,
        &*host,
        views.request(),
        hal_plan.view(),
        device.timeout_ns,
        &targets,
        &shared,
        device_index,
        device.fallback_keepalive_ns,
        action_rx,
        move || *stop.borrow_and_update(),
    )
    .await;
}

/// How many pending action calls a device's channel buffers before a
/// caller sees the device's queue as full (`J-E1403`).
const ACTION_CHANNEL_CAPACITY: usize = 4;

/// Every real device as its real driver, on a tokio runtime.
#[derive(Clone)]
pub struct RealDevices {
    handle: tokio::runtime::Handle,
    sysfs_root: Arc<str>,
}

impl std::fmt::Debug for RealDevices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RealDevices")
            .field("sysfs_root", &self.sysfs_root)
            .finish_non_exhaustive()
    }
}

impl RealDevices {
    /// Devices spawned on `handle`, with `sysfs`'s root at `/sys`.
    pub fn new(handle: tokio::runtime::Handle) -> Self {
        Self {
            handle,
            sysfs_root: Arc::from(joker_driver_sysfs::SYSFS_ROOT),
        }
    }

    /// Devices spawned on `handle`, with `sysfs`'s root overridden (a
    /// fake `/sys` tree in a test).
    pub fn with_sysfs_root(handle: tokio::runtime::Handle, sysfs_root: &str) -> Self {
        Self {
            handle,
            sysfs_root: Arc::from(sysfs_root),
        }
    }
}

struct RealSet {
    stop: watch::Sender<bool>,
    /// Every dispatched device's action-invocation channel, by name
    /// (AUR-405): the registry `Controller::invoke_action` looks up to
    /// find where to send an `ActionCall` for a given device.
    actions: BTreeMap<String, tokio::sync::mpsc::Sender<ActionCall>>,
}

impl DeviceSet for RealSet {
    fn stop(self: Box<Self>) {
        let _ = self.stop.send(true);
    }

    fn action_sender(&self, device: &str) -> Option<tokio::sync::mpsc::Sender<ActionCall>> {
        self.actions.get(device).cloned()
    }
}

impl Devices for RealDevices {
    fn start(&self, plan: &Arc<Plan>, shared: &Arc<Shared>) -> Box<dyn DeviceSet> {
        let (tx, rx) = watch::channel(false);
        let host = Arc::new(TokioHost::new(ClockQuality::Synced));
        let mut actions = BTreeMap::new();
        for (i, device) in plan.devices.iter().enumerate() {
            let plan = Arc::clone(plan);
            let shared = Arc::clone(shared);
            let host = Arc::clone(&host);
            let stop = rx.clone();
            let device = device.clone();
            let name = device.name.clone();
            let (action_tx, action_rx) = tokio::sync::mpsc::channel(ACTION_CHANNEL_CAPACITY);
            let dispatched = match device.driver.as_str() {
                "sysfs" => {
                    let root = Arc::clone(&self.sysfs_root);
                    self.handle.spawn(run_sysfs(
                        device, plan, i, shared, host, stop, root, action_rx,
                    ));
                    true
                }
                "modbus-rtu" => {
                    self.handle.spawn(run_modbus_rtu(
                        device, plan, i, shared, host, stop, action_rx,
                    ));
                    true
                }
                "modbus-tcp" => {
                    self.handle.spawn(run_modbus_tcp(
                        device, plan, i, shared, host, stop, action_rx,
                    ));
                    true
                }
                "gpio" => {
                    self.handle
                        .spawn(run_gpio(device, plan, i, shared, host, stop, action_rx));
                    true
                }
                other => {
                    eprintln!(
                        "warning: device `{}`: driver `{other}` has no real adapter yet",
                        device.name
                    );
                    false
                }
            };
            if dispatched {
                actions.insert(name, action_tx);
            }
        }
        Box::new(RealSet { stop: tx, actions })
    }
}

#[cfg(test)]
mod tests {
    use super::gpio_safe;
    use joker_core::SafeValue;

    #[test]
    fn a_gpio_output_starts_at_its_bool_safe_and_refuses_anything_else() {
        assert_eq!(gpio_safe(SafeValue::Value(0), "pi.relay"), Ok(false));
        assert_eq!(gpio_safe(SafeValue::Value(1), "pi.relay"), Ok(true));
        assert!(gpio_safe(SafeValue::Value(2), "pi.relay").is_err());
        let relinquish = gpio_safe(SafeValue::Relinquish, "pi.relay").unwrap_err();
        assert!(relinquish.contains("cannot relinquish"), "{relinquish}");
    }
}
