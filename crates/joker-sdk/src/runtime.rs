//! The exports of the world (ref/04 §2.6), implemented once for every app:
//! `init` checks the port table against what `#[joker::app]` declared,
//! reserves the buffers of the cycle path and creates the task states, and
//! `on-cycle` dispatches to the task registered by `#[joker::cycle]`, in
//! name order (ref/04 §4.1, §5.3), without allocating (§4.13).

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::any::Any;
use core::cell::RefCell;

use critical_section::Mutex;
use joker_sdk_sys::cycle::{CycleGuest, CycleOut};
use joker_sdk_sys::exports::joker::runtime::guest::{
    ActionRequest, Guest, InitContext, ParamChange, Trigger,
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
    /// Its `joker:meta` fragment (ref/04 §5.2), which a component also
    /// carries in its custom section; the native test harness reads it here.
    pub meta: &'static [u8],
}

/// A cycle task, as `#[joker::cycle]` registers it.
#[doc(hidden)]
#[derive(Debug)]
pub struct CycleTask {
    /// Task name.
    pub name: &'static str,
    /// Runs one tick of the task.
    pub run: fn(&mut TaskCx<'_>) -> Result<()>,
    /// Creates the state of the task at `init`, if it has one.
    pub state: fn() -> Option<Box<dyn Any + Send>>,
    /// Its `joker:meta` fragment (ref/04 §5.2).
    pub meta: &'static [u8],
}

inventory::collect!(AppEntry);
inventory::collect!(CycleTask);

struct State {
    layout: Vec<Layout>,
    tasks: Vec<&'static CycleTask>,
    states: Vec<Option<Box<dyn Any + Send>>>,
    pending: Vec<Option<u64>>,
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
        crate::log::install_panic_hook();
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
        let states = tasks.iter().map(|t| (t.state)()).collect();
        let pending = alloc::vec![None; layout.len()];
        joker_sdk_sys::cycle::reserve(cx.image_len as usize, cx.proposal_capacity as usize);
        critical_section::with(|cs| {
            STATE.borrow_ref_mut(cs).replace(State {
                layout,
                tasks,
                states,
                pending,
            });
        });
        Ok(())
    }

    fn on_params(changes: Vec<ParamChange>) -> core::result::Result<(), types::Error> {
        drop(changes);
        Ok(())
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

impl CycleGuest for Runtime {
    fn on_cycle(task: u32, image: &[u8], out: &mut CycleOut<'_>) {
        critical_section::with(|cs| {
            let mut guard = STATE.borrow_ref_mut(cs);
            let Some(state) = guard.as_mut() else {
                out.fail("J-E0008", "on-cycle before init");
                return;
            };
            let Some(entry) = state.tasks.get(task as usize).copied() else {
                out.fail("J-E0008", "no such cycle task");
                return;
            };
            let (len, full, result) = {
                let buffer = ProposalBuffer::new(out.proposals(), &mut state.pending);
                let cx = CycleCx::new(image, &state.layout, entry.name, buffer);
                let result = match state.states.get_mut(task as usize) {
                    Some(slot) => (entry.run)(&mut TaskCx::new(&cx, slot)),
                    None => Err(Error::new(
                        crate::error::Code::from_static("J-E0008"),
                        "no task state",
                    )),
                };
                let (len, full) = cx.finish();
                (len, full, result)
            };
            out.set_len(len);
            match result {
                Err(e) => out.fail(e.code().as_str(), e.message()),
                Ok(()) if full => {
                    out.fail("J-E0203", "proposal buffer full; later proposals dropped")
                }
                Ok(()) => {}
            }
        });
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
