//! The boundary between the drivers (the `io` domain, on tokio) and the
//! core (docs/01 §3.3, §3.4).
//!
//! A driver gets one [`DeviceIo`] per device: the only writer of that
//! device's input slots, and a reader of the values committed to its
//! outputs, latest value wins. Drivers never see proposals: only what the
//! envelope applied (AGENTS.md invariant 3).

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use joker_core::envelope::Applied;
use joker_core::registry::Direction;
use joker_core::value::NO_TS_SRC;
use joker_core::{Quality, SignalId};

use crate::pacer::Monotonic;
use crate::plan::Plan;
use crate::shared::Shared;

/// A committed value for one output of a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Command {
    /// The output, by its name inside the device (`act_led`).
    pub signal: SignalId,
    /// The applied value.
    pub applied: Applied,
}

/// The slots of one device.
#[derive(Debug)]
pub struct DeviceIo {
    device: String,
    shared: Arc<Shared>,
    clock: Monotonic,
    inputs: BTreeMap<String, SignalId>,
    outputs: Vec<(String, SignalId, u64)>,
}

impl DeviceIo {
    /// The slots of `device` in a plan.
    pub fn new(plan: &Plan, shared: Arc<Shared>, clock: Monotonic, device: &str) -> Self {
        let prefix = format!("{device}.");
        let mut inputs = BTreeMap::new();
        let mut outputs = Vec::new();
        for s in plan
            .signals
            .iter()
            .filter(|s| s.device.as_deref() == Some(device))
        {
            let Some(id) = plan.id(&s.path) else { continue };
            let name = s.path.strip_prefix(&prefix).unwrap_or(&s.path).to_owned();
            match s.direction {
                Direction::Output => outputs.push((name, id, 0)),
                _ => {
                    inputs.insert(name, id);
                }
            }
        }
        Self {
            device: device.to_owned(),
            shared,
            clock,
            inputs,
            outputs,
        }
    }

    /// The device name.
    pub fn device(&self) -> &str {
        &self.device
    }

    /// The input signal names of the device.
    pub fn inputs(&self) -> impl Iterator<Item = &str> {
        self.inputs.keys().map(String::as_str)
    }

    /// The output signal names and ids of the device.
    pub fn outputs(&self) -> impl Iterator<Item = (&str, SignalId)> {
        self.outputs.iter().map(|(n, id, _)| (n.as_str(), *id))
    }

    /// Publishes a sample of an input (acquisition stage, docs/01 §3.3).
    pub fn publish(&self, signal: &str, bits: u64, quality: Quality) -> bool {
        let Some(id) = self.inputs.get(signal) else {
            return false;
        };
        match self.shared.slots.get(id.index()) {
            Some(slot) => {
                slot.write(bits, quality, NO_TS_SRC, self.clock.now_ns());
                true
            }
            None => false,
        }
    }

    /// The outputs whose applied value changed since the last call.
    pub fn commands(&mut self) -> Vec<Command> {
        let mut out = Vec::new();
        for (_, id, seen) in &mut self.outputs {
            let Some(slot) = self.shared.slots.get(id.index()) else {
                continue;
            };
            let Ok(sample) = slot.read(8) else { continue };
            if sample.seq() == 0 || sample.seq() == *seen {
                continue;
            }
            *seen = sample.seq();
            let relinquish = self
                .shared
                .output(*id)
                .and_then(|s| s.read())
                .is_some_and(|(_, meta, _)| meta.relinquish);
            out.push(Command {
                signal: *id,
                applied: if relinquish {
                    Applied::Relinquish
                } else {
                    Applied::Value(sample.transport_bits())
                },
            });
        }
        out
    }
}

/// The simulated device of `driver = "sim"` (docs/26 §6): every output is
/// echoed to the input `<output>_rb` when the device declares one, and
/// inputs can be set from outside.
#[derive(Debug)]
pub struct SimDevice {
    io: DeviceIo,
}

impl SimDevice {
    /// A simulated device over its slots.
    pub fn new(io: DeviceIo) -> Self {
        Self { io }
    }

    /// One step: applies the committed outputs to the model.
    pub fn step(&mut self) -> Vec<Command> {
        let commands = self.io.commands();
        for c in &commands {
            let name = self
                .io
                .outputs
                .iter()
                .find(|(_, id, _)| *id == c.signal)
                .map(|(n, _, _)| format!("{n}_rb"));
            if let Some(rb) = name {
                self.io.publish(&rb, c.applied.bits(), Quality::GOOD);
            }
        }
        commands
    }

    /// Runs the model on tokio until `stop` resolves.
    pub async fn run(mut self, period: Duration, mut stop: tokio::sync::watch::Receiver<bool>) {
        let mut interval = tokio::time::interval(period);
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    self.step();
                }
                _ = stop.changed() => return,
            }
        }
    }
}
