//! [`Devices`] for every driver family with a complete real Linux
//! adapter: `sysfs` (`Link::Paths`), `modbus-rtu` (`Link::Serial` over
//! `TokioSerial`) and `modbus-tcp` (`Link::Tcp` over `TokioTcpConnector`).
//! `gpio` and `i2c` are not dispatched here yet: their drivers exist and
//! are tested against an in-memory double, but the real Linux chardev/
//! i2c-dev adapter (opening `/dev/gpiochipN`/`/dev/i2c-N` and issuing the
//! ioctl) is not implemented (AUR-401). A device declaring one of those
//! is reported, not silently ignored.

use std::net::SocketAddr;
use std::sync::Arc;

use joker_core::ClockQuality;
use joker_driver_modbus::{ModbusPlanner, ModbusRtu, ModbusTcp, ModbusTcpPlanner};
use joker_driver_sysfs::{SysfsDriver, SysfsPlanner};
use joker_hal::link::DevicePaths;
use joker_hal::std_host::{SerialConfig, TokioHost, TokioSerial, TokioTcpConnector};
use joker_hal::{Cbor, Driver, Link, OpenArgs, Planner};
use tokio::sync::watch;

use crate::bridge::{DevicePlan, owned_request};
use crate::devices::{DeviceSet, Devices};
use crate::drivers::{point_targets, run_driver};
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

async fn run_sysfs(
    device: DevicePlan,
    plan: Arc<Plan>,
    device_index: usize,
    shared: Arc<Shared>,
    host: Arc<TokioHost>,
    mut stop: watch::Receiver<bool>,
    sysfs_root: Arc<str>,
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
        move || *stop.borrow_and_update(),
    )
    .await;
}

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
}

impl DeviceSet for RealSet {
    fn stop(self: Box<Self>) {
        let _ = self.stop.send(true);
    }
}

impl Devices for RealDevices {
    fn start(&self, plan: &Arc<Plan>, shared: &Arc<Shared>) -> Box<dyn DeviceSet> {
        let (tx, rx) = watch::channel(false);
        let host = Arc::new(TokioHost::new(ClockQuality::Synced));
        for (i, device) in plan.devices.iter().enumerate() {
            let plan = Arc::clone(plan);
            let shared = Arc::clone(shared);
            let host = Arc::clone(&host);
            let stop = rx.clone();
            let device = device.clone();
            match device.driver.as_str() {
                "sysfs" => {
                    let root = Arc::clone(&self.sysfs_root);
                    self.handle
                        .spawn(run_sysfs(device, plan, i, shared, host, stop, root));
                }
                "modbus-rtu" => {
                    self.handle
                        .spawn(run_modbus_rtu(device, plan, i, shared, host, stop));
                }
                "modbus-tcp" => {
                    self.handle
                        .spawn(run_modbus_tcp(device, plan, i, shared, host, stop));
                }
                other => {
                    eprintln!(
                        "warning: device `{}`: driver `{other}` has no real adapter yet",
                        device.name
                    );
                }
            }
        }
        Box::new(RealSet { stop: tx })
    }
}
