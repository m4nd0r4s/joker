//! The exports of the world (ref/04 §2.6), implemented once for every app:
//! `init` checks the port table against what `#[joker::app]` declared, and
//! `on-cycle` dispatches to the task registered by `#[joker::cycle]`, in
//! name order (ref/04 §4.1, §5.3).

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::any::Any;
use core::cell::RefCell;

use critical_section::Mutex;
use joker_sdk_sys::exports::joker::runtime::guest::{
    ActionRequest, CycleResult, Guest, InitContext, ParamChange, Trigger,
};
use joker_sdk_sys::joker::runtime::{cycle_io, types};

use crate::error::{Error, Result};
use crate::io::{CycleCx, Layout, PortDecl, PortKind, ProposalBuffer, TaskCx};

/// The app, as `#[joker::app]` registers it.
#[doc(hidden)]
#[derive(Debug)]
pub struct AppEntry {
    /// App name.
    pub name: &'static str,
    /// Ports.
    pub ports: &'static [PortDecl],
}

/// A cycle task, as `#[joker::cycle]` registers it.
#[doc(hidden)]
#[derive(Debug)]
pub struct CycleTask {
    /// Task name.
    pub name: &'static str,
    /// Runs one tick of the task.
    pub run: fn(&mut TaskCx<'_>) -> Result<()>,
}

inventory::collect!(AppEntry);
inventory::collect!(CycleTask);

struct State {
    layout: Vec<Layout>,
    tasks: Vec<&'static CycleTask>,
    states: Vec<Option<Box<dyn Any + Send>>>,
    buffer: ProposalBuffer,
}

static STATE: Mutex<RefCell<Option<State>>> = Mutex::new(RefCell::new(None));

/// The implementation of the `guest` interface every app exports.
#[doc(hidden)]
#[derive(Debug)]
pub struct Runtime;

fn error(code: &str, message: impl ToString) -> types::Error {
    types::Error {
        code: code.to_string(),
        message: message.to_string(),
    }
}

fn wit_error(e: &Error) -> types::Error {
    error(e.code().as_str(), e.message())
}

fn check(cx: &InitContext, app: &AppEntry) -> core::result::Result<Vec<Layout>, String> {
    if cx.cycle_abi != 1 {
        return Err(alloc::format!(
            "cycle ABI {} is not spoken by this SDK",
            cx.cycle_abi
        ));
    }
    if cx.ports.len() != app.ports.len() {
        return Err(alloc::format!(
            "{} ports in the table, {} declared by the app",
            cx.ports.len(),
            app.ports.len()
        ));
    }
    let mut layout = Vec::with_capacity(cx.ports.len());
    for (i, (p, d)) in cx.ports.iter().zip(app.ports).enumerate() {
        let wants_outcome = d.kind == PortKind::Output;
        if p.decl as usize != i || p.slot.is_none() || p.outcome.is_some() != wants_outcome {
            return Err(alloc::format!(
                "port {} does not match its declaration",
                d.name
            ));
        }
        layout.push(Layout {
            slot: p.slot,
            outcome: p.outcome,
            bound: p.bound,
        });
    }
    Ok(layout)
}

impl Guest for Runtime {
    fn init(cx: InitContext) -> core::result::Result<(), types::Error> {
        let mut apps = inventory::iter::<AppEntry>.into_iter();
        let (Some(app), None) = (apps.next(), apps.next()) else {
            return Err(error(
                "J-E0004",
                "a component holds exactly one #[joker::app]",
            ));
        };
        let layout = check(&cx, app).map_err(|m| error("J-E0008", m))?;
        let mut tasks: Vec<&'static CycleTask> = inventory::iter::<CycleTask>.into_iter().collect();
        tasks.sort_by(|a, b| a.name.cmp(b.name));
        let states = tasks.iter().map(|_| None).collect();
        let buffer = ProposalBuffer::new(cx.proposal_capacity as usize, layout.len());
        critical_section::with(|cs| {
            STATE.borrow_ref_mut(cs).replace(State {
                layout,
                tasks,
                states,
                buffer,
            });
        });
        Ok(())
    }

    fn on_params(changes: Vec<ParamChange>) -> core::result::Result<(), types::Error> {
        drop(changes);
        Ok(())
    }

    fn on_cycle(task: u32, image: Vec<u8>) -> CycleResult {
        critical_section::with(|cs| {
            let mut guard = STATE.borrow_ref_mut(cs);
            let Some(state) = guard.as_mut() else {
                return CycleResult {
                    proposals: Vec::new(),
                    error: Some(error("J-E0008", "on-cycle before init")),
                };
            };
            let Some(entry) = state.tasks.get(task as usize).copied() else {
                return CycleResult {
                    proposals: Vec::new(),
                    error: Some(error("J-E0008", alloc::format!("no cycle task {task}"))),
                };
            };
            let buffer = core::mem::take(&mut state.buffer);
            let cx = CycleCx::new(&image, &state.layout, entry.name, buffer);
            let result = match state.states.get_mut(task as usize) {
                Some(slot) => (entry.run)(&mut TaskCx::new(&cx, slot)),
                None => Err(Error::new(
                    crate::error::Code::from_static("J-E0008"),
                    "no task state",
                )),
            };
            state.buffer = cx.into_buffer();
            let full = state.buffer.is_full();
            let proposals = state.buffer.finish();
            let error = match result {
                Err(e) => Some(wit_error(&e)),
                Ok(()) if full => Some(error(
                    "J-E0203",
                    "proposal buffer full; later proposals dropped",
                )),
                Ok(()) => None,
            };
            CycleResult { proposals, error }
        })
    }

    async fn run_task(task: u32, why: Trigger) -> core::result::Result<(), types::Error> {
        drop((task, why));
        Err(error(
            "J-E0601",
            "async tasks are not served by this SDK version",
        ))
    }

    async fn on_action(req: ActionRequest) -> core::result::Result<Vec<u8>, types::Error> {
        drop(req);
        Err(error(
            "J-E0601",
            "actions are not served by this SDK version",
        ))
    }

    fn export_state() -> core::result::Result<Vec<u8>, types::Error> {
        Ok(Vec::new())
    }

    fn migrate(previous: Vec<u8>, from_version: String) -> core::result::Result<(), types::Error> {
        drop((previous, from_version));
        Ok(())
    }
}

/// The source and hardware timestamps of a port, through `cycle-io.read`.
pub(crate) fn read(port: u32) -> (Option<i64>, Option<u64>) {
    if cfg!(target_family = "wasm") {
        let s = cycle_io::read(port);
        (s.ts_src, s.ts_hw)
    } else {
        (None, None)
    }
}

/// A number of the instance generator.
pub(crate) fn random() -> u64 {
    if cfg!(target_family = "wasm") {
        cycle_io::random_u64()
    } else {
        0
    }
}
