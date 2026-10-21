//! Starting and stopping the devices of a generation (the `io` domain).

use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use crate::io::{DeviceIo, SimDevice};
use crate::pacer::Monotonic;
use crate::plan::Plan;
use crate::shared::Shared;

/// The devices of one running generation.
pub trait DeviceSet: Send {
    /// Stops them; their slots stay as they were.
    fn stop(self: Box<Self>);
}

/// Starts the devices of a generation. The driver crates provide one
/// implementation per driver; [`SimDevices`] runs `driver = "sim"`.
pub trait Devices: Send + Sync {
    /// Starts every device of `plan` over the slots of `shared`.
    fn start(&self, plan: &Arc<Plan>, shared: &Arc<Shared>) -> Box<dyn DeviceSet>;
}

/// No devices at all.
#[derive(Debug, Default)]
pub struct NoDevices;

impl DeviceSet for NoDevices {
    fn stop(self: Box<Self>) {}
}

impl Devices for NoDevices {
    fn start(&self, _plan: &Arc<Plan>, _shared: &Arc<Shared>) -> Box<dyn DeviceSet> {
        Box::new(NoDevices)
    }
}

/// Every device as a simulated device on a tokio runtime.
#[derive(Clone)]
pub struct SimDevices {
    handle: tokio::runtime::Handle,
    clock: Monotonic,
    period: Duration,
}

impl fmt::Debug for SimDevices {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SimDevices")
            .field("period", &self.period)
            .finish()
    }
}

impl SimDevices {
    /// Simulated devices stepping every `period` on `handle`.
    pub fn new(handle: tokio::runtime::Handle, clock: Monotonic, period: Duration) -> Self {
        Self {
            handle,
            clock,
            period,
        }
    }
}

struct SimSet {
    stop: tokio::sync::watch::Sender<bool>,
}

impl DeviceSet for SimSet {
    fn stop(self: Box<Self>) {
        let _ = self.stop.send(true);
    }
}

impl Devices for SimDevices {
    fn start(&self, plan: &Arc<Plan>, shared: &Arc<Shared>) -> Box<dyn DeviceSet> {
        let (tx, rx) = tokio::sync::watch::channel(false);
        let devices: BTreeSet<&str> = plan
            .signals
            .iter()
            .filter_map(|s| s.device.as_deref())
            .collect();
        for device in devices {
            let io = DeviceIo::new(plan, Arc::clone(shared), self.clock, device);
            self.handle
                .spawn(SimDevice::new(io).run(self.period, rx.clone()));
        }
        Box::new(SimSet { stop: tx })
    }
}
