//! Native cycle tasks built into the daemon (`kind = "native"`, docs/01 §4).
//!
//! [`Light`] is the blinking app of the learning track (parts 1–5), written
//! natively so that the M0 slice runs end to end before the Wasm host
//! (`joker-host`) lands. The Wasm `light` replaces it through the same
//! [`TaskProvider`] interface.

use std::collections::BTreeMap;

use joker_core::SignalId;
use joker_manifest::context::{AppInterface, Param, Port, PortKind, SignalType};
use joker_manifest::value::Value;
use joker_units::{Duration, Literal, Unit};

use crate::task::{CycleTask, TaskContext, TaskError, TaskProvider, TaskSpec};

/// The default half period of `light`.
pub const DEFAULT_HALF_PERIOD: Duration = Duration::from_millis(500);

/// `light`: toggles its `led` output every `half_period`.
#[derive(Debug)]
pub struct Light {
    led: SignalId,
    half_period_ns: u64,
    on: bool,
    started_ns: Option<u64>,
}

impl Light {
    /// A blinker on `led`.
    pub fn new(led: SignalId, half_period: Duration) -> Self {
        Self {
            led,
            half_period_ns: half_period.as_nanos().max(1),
            on: false,
            started_ns: None,
        }
    }
}

impl CycleTask for Light {
    fn run(&mut self, cx: &mut TaskContext<'_, '_, '_>) -> Result<(), TaskError> {
        let tick = cx.tick().ok_or_else(|| TaskError("no tick".into()))?;
        let start = *self.started_ns.get_or_insert(tick.at.mono_ns);
        let phase = (tick.at.mono_ns - start) / self.half_period_ns;
        self.on = phase.is_multiple_of(2);
        cx.propose(self.led, u64::from(self.on))
            .map_err(|e| TaskError(format!("{e:?}")))
    }

    fn export_state(&mut self) -> Result<Option<Vec<u8>>, TaskError> {
        Ok(Some(self.started_ns.unwrap_or(0).to_le_bytes().to_vec()))
    }

    fn import_state(&mut self, blob: &[u8]) -> Result<(), TaskError> {
        let bytes: [u8; 8] = blob
            .try_into()
            .map_err(|_| TaskError("bad light state".into()))?;
        let start = u64::from_le_bytes(bytes);
        self.started_ns = (start != 0).then_some(start);
        Ok(())
    }
}

/// The provider of the native tasks.
#[derive(Debug, Default)]
pub struct NativeTasks;

fn light_interface() -> AppInterface {
    let mut i = AppInterface::default();
    i.ports.insert(
        "led".to_owned(),
        Port {
            kind: PortKind::Output,
            ty: SignalType::Bool,
            unit: None,
            optional: false,
        },
    );
    i.params.insert(
        "half_period".to_owned(),
        Param {
            ty: SignalType::Float,
            unit: Unit::parse("ms").ok(),
            has_default: true,
            range: None,
        },
    );
    i.tasks.insert("blink".to_owned(), None);
    i
}

impl TaskProvider for NativeTasks {
    fn interfaces(&self) -> BTreeMap<String, AppInterface> {
        [("light".to_owned(), light_interface())]
            .into_iter()
            .collect()
    }

    fn instantiate(&self, spec: TaskSpec<'_>) -> Result<Box<dyn CycleTask>, TaskError> {
        match (spec.app, spec.task) {
            ("light", "blink") => {
                let led = *spec
                    .bindings
                    .get("led")
                    .ok_or_else(|| TaskError("light: port `led` is not bound".into()))?;
                let half = spec
                    .manifest
                    .params
                    .get("half_period")
                    .and_then(|p| match &p.value.value {
                        Value::String(s) => Literal::parse(s).ok(),
                        _ => None,
                    })
                    .and_then(|l| match l {
                        Literal::Quantity(q) => q.value_in(&Unit::SECOND).ok(),
                        Literal::Number(_) => None,
                    })
                    .map_or(DEFAULT_HALF_PERIOD, |s| {
                        Duration::from_nanos((s * 1e9) as u64)
                    });
                Ok(Box::new(Light::new(led, half)))
            }
            (app, task) => Err(TaskError(format!("no native task `{app}.{task}`"))),
        }
    }
}
