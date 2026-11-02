//! The exports of the world (ref/04 §2.6), implemented once for every app:
//! `init` checks the port table against what `#[joker::app]` declared,
//! reserves the buffers of the cycle path and creates the task states;
//! `on-cycle` dispatches to the task registered by `#[joker::cycle]`, in
//! name order (ref/04 §4.1, §5.3), without allocating (§4.13); `run-task`
//! runs the tasks of `#[joker::task]`; `on-params`, `export-state` and
//! `migrate` serve `#[joker::on_params]`, `#[joker::state]` and
//! `#[joker::migrate]`.

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::any::{Any, TypeId};
use core::cell::RefCell;
use core::future::Future;
use core::pin::Pin;

use critical_section::Mutex;
use joker_sdk_sys::cycle::{CycleGuest, CycleOut};
use joker_sdk_sys::exports::joker::runtime::guest::{
    ActionRequest, Guest, InitContext, InstanceKind, ParamChange, Trigger,
};
use joker_sdk_sys::joker::runtime::{cycle_io, types};

use crate::cbor::{Reader, Writer};
use crate::error::{Code, Error, Result};
use crate::host;
use crate::io::{App, CycleCx, CycleRun, Layout, PortDecl, ProposalBuffer, TaskCx, TaskIo};
use crate::record::Event;
use crate::state::{ParamChanges, Persist, State, StateBlob};
use crate::time::Instant;
use crate::value::{PortValue, Sample, Value};

/// The app, as `#[joker::app]` registers it.
#[doc(hidden)]
#[derive(Debug)]
pub struct AppEntry {
    /// App name.
    pub name: &'static str,
    /// App version.
    pub version: &'static str,
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
    pub run: fn(&mut CycleRun<'_>) -> Result<()>,
    /// Creates the state of the task at `init`, if it has one.
    pub state: fn() -> Option<Box<dyn Any + Send>>,
    /// Its `joker:meta` fragment (ref/04 §5.2).
    pub meta: &'static [u8],
}

/// The trigger of an async task (ref/04 §4.3.2).
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerDecl {
    /// `every`, with its jitter.
    Every {
        /// Period, ns.
        period_ns: u64,
        /// Jitter, ns.
        jitter_ns: u64,
    },
    /// `on`: the ports.
    On(&'static [&'static str]),
    /// `on_event`: the event port.
    OnEvent(&'static str),
    /// `at`: the cron expression.
    At(&'static str),
    /// `startup`.
    Startup,
}

/// A future of a task run.
#[doc(hidden)]
pub type TaskFuture = Pin<Box<dyn Future<Output = Result<()>>>>;

/// An async task, as `#[joker::task]` registers it.
#[doc(hidden)]
#[derive(Debug)]
pub struct AsyncTask {
    /// Task name.
    pub name: &'static str,
    /// Its trigger.
    pub trigger: TriggerDecl,
    /// Starts one run.
    pub run: fn(TaskArgs) -> TaskFuture,
    /// Creates the `Shared<S>` of the task at `init`, if it has one.
    pub state: fn() -> Option<Box<dyn Any + Send + Sync>>,
    /// Its `joker:meta` fragment (ref/04 §5.2).
    pub meta: &'static [u8],
}

/// The handler of `#[joker::on_params]`.
#[doc(hidden)]
#[derive(Debug)]
pub struct OnParams {
    /// The handler.
    pub run: fn(&ParamChanges) -> Result<()>,
}

/// The codec of a `#[joker::state]` type.
#[doc(hidden)]
#[derive(Debug)]
pub struct StateCodec {
    /// The type.
    pub type_id: fn() -> TypeId,
    /// `version = N`.
    pub version: u32,
    /// `persist`.
    pub persist: bool,
    /// Writes a state of the type.
    pub save: fn(&(dyn Any + Send), &mut Writer<'_>) -> bool,
    /// The default migration.
    pub load: fn(&StateBlob<'_>) -> Box<dyn Any + Send>,
}

/// A custom migration, `#[joker::migrate(Type)]`.
#[doc(hidden)]
#[derive(Debug)]
pub struct Migration {
    /// The state type.
    pub type_id: fn() -> TypeId,
    /// The migration.
    pub run: fn(StateBlob<'_>, &str) -> Result<Box<dyn Any + Send>>,
}

inventory::collect!(AppEntry);
inventory::collect!(CycleTask);
inventory::collect!(AsyncTask);
inventory::collect!(OnParams);
inventory::collect!(StateCodec);
inventory::collect!(Migration);

/// What a task run starts with.
#[doc(hidden)]
pub struct TaskArgs {
    layout: &'static [Layout],
    name: &'static str,
    why: Trigger,
    state: Option<&'static (dyn Any + Send + Sync)>,
}

impl core::fmt::Debug for TaskArgs {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TaskArgs")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

fn sdk(code: &'static str, message: &str) -> Error {
    Error::new(Code::from_static(code), message)
}

impl TaskArgs {
    /// The context of the run.
    pub fn cx(&self) -> TaskCx {
        TaskCx::new(self.layout, self.name)
    }

    /// The sample of an `on = "<port>"` trigger.
    pub fn sample<T: PortValue>(&self) -> Result<Sample<T>> {
        match &self.why {
            Trigger::Change(c) => Ok(host::sample(&c.sample, |v| T::from_bits(host::bits(v)))),
            _ => Err(sdk("J-E0008", "the task was not run by a change")),
        }
    }

    /// The change of an `on = ["a", "b"]` trigger.
    pub fn change<A: App>(&self) -> Result<Change> {
        match &self.why {
            Trigger::Change(c) => {
                let decl = A::PORTS
                    .get(c.port as usize)
                    .ok_or_else(|| sdk("J-E0008", "change of an unknown port"))?;
                Ok(Change {
                    port: decl.name,
                    sample: host::sample(&c.sample, |v| Value::from_bits(decl.ty, host::bits(v))),
                })
            }
            _ => Err(sdk("J-E0008", "the task was not run by a change")),
        }
    }

    /// The delivery of an `on_event` trigger.
    pub fn delivery<E: Event>(&self) -> Result<Delivery<E>> {
        match &self.why {
            Trigger::Event(d) => Ok(Delivery {
                event: E::decode_from(&d.payload)
                    .ok_or_else(|| sdk("J-E0504", "an event that does not decode"))?,
                seq: d.seq,
                at: host::instant(&d.at),
                lost: d.lost,
            }),
            _ => Err(sdk("J-E0008", "the task was not run by an event")),
        }
    }

    /// The `Shared<S>` of the task.
    pub fn state<S: Any + Send + Sync>(&self) -> Result<&'static S> {
        self.state
            .and_then(|s| (s as &dyn Any).downcast_ref::<S>())
            .ok_or_else(|| sdk("J-E0008", "task state missing or of another type"))
    }
}

/// The view of a run, for the code `#[joker::task]` generates.
#[doc(hidden)]
pub fn task_io<A: App>(cx: TaskCx) -> TaskIo<A> {
    cx.io::<A>()
}

/// A boxed task future.
#[doc(hidden)]
pub fn boxed(f: impl Future<Output = Result<()>> + 'static) -> TaskFuture {
    Box::pin(f)
}

/// A `Shared<S>` created at `init`.
#[doc(hidden)]
pub fn new_shared<S: Default + Send + 'static>() -> Option<Box<dyn Any + Send + Sync>> {
    Some(Box::new(crate::state::Shared::<S>::default()))
}

/// Writes a state of type `S` (the codec of `#[joker::state(persist)]`).
#[doc(hidden)]
pub fn save_state<S: State>(state: &(dyn Any + Send), w: &mut Writer<'_>) -> bool {
    match (state as &dyn Any).downcast_ref::<S>() {
        Some(s) => {
            s.save(w);
            true
        }
        None => false,
    }
}

/// The default migration of a state of type `S`.
#[doc(hidden)]
pub fn load_state<S: State>(blob: &StateBlob<'_>) -> Box<dyn Any + Send> {
    Box::new(S::load(blob))
}

/// One field of a persisted state.
#[doc(hidden)]
pub fn load_field<T: Persist>(blob: &StateBlob<'_>, name: &str) -> Option<T> {
    blob.find(name).and_then(|mut r| T::load(&mut r))
}

/// The result of a custom migration.
#[doc(hidden)]
pub fn migrated<S: Send + 'static>(r: Result<S>) -> Result<Box<dyn Any + Send>> {
    r.map(|s| Box::new(s) as Box<dyn Any + Send>)
}

/// One change of an `on = ["a", "b"]` trigger.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Change {
    /// The port that changed.
    pub port: &'static str,
    /// Its new sample.
    pub sample: Sample<Value>,
}

/// One event of an `on_event` trigger.
#[derive(Clone, Debug, PartialEq)]
pub struct Delivery<E> {
    /// The event.
    pub event: E,
    /// Its sequence number.
    pub seq: u64,
    /// When it was emitted.
    pub at: Instant,
    /// Events dropped for this consumer since the previous delivery.
    pub lost: u32,
}

struct CycleState {
    layout: Vec<Layout>,
    tasks: Vec<Option<&'static CycleTask>>,
    states: Vec<Option<Box<dyn Any + Send>>>,
    pending: Vec<Option<u64>>,
}

struct AsyncState {
    layout: &'static [Layout],
    tasks: Vec<Option<&'static AsyncTask>>,
    states: Vec<Option<&'static (dyn Any + Send + Sync)>>,
}

static CYCLE: Mutex<RefCell<Option<CycleState>>> = Mutex::new(RefCell::new(None));
static ASYNC: Mutex<RefCell<Option<AsyncState>>> = Mutex::new(RefCell::new(None));

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

fn wire(e: Error) -> types::Error {
    error(e.code().as_str(), e.message())
}

fn the_app() -> core::result::Result<&'static AppEntry, types::Error> {
    let mut apps = inventory::iter::<AppEntry>.into_iter();
    match (apps.next(), apps.next()) {
        (Some(app), None) => Ok(app),
        _ => Err(error(
            "J-E0004",
            "a component holds exactly one #[joker::app]",
        )),
    }
}

fn check(
    cx: &InitContext,
    app: &AppEntry,
    cycle: bool,
) -> core::result::Result<Vec<Layout>, String> {
    if cycle && cx.cycle_abi != 1 {
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
        let wants_outcome = d.kind == crate::io::PortKind::Output;
        let wants_slot = d.kind.has_sample();
        if p.decl as usize != i
            || (cycle && (p.slot.is_some() != wants_slot || p.outcome.is_some() != wants_outcome))
        {
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

/// The tasks of the app sorted by name: the index of a task in this list is
/// its id in `on-cycle` and `run-task` (ref/04 §5.3).
fn task_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = inventory::iter::<CycleTask>
        .into_iter()
        .map(|t| t.name)
        .chain(inventory::iter::<AsyncTask>.into_iter().map(|t| t.name))
        .collect();
    names.sort_unstable();
    names
}

/// The tasks of one kind, at the index of their id.
fn by_id<T: inventory::Collect>(name: fn(&T) -> &'static str) -> Vec<Option<&'static T>> {
    task_names()
        .into_iter()
        .map(|n| inventory::iter::<T>.into_iter().find(|t| name(t) == n))
        .collect()
}

fn state_type(state: &(dyn Any + Send)) -> TypeId {
    state.type_id()
}

fn codec_of(id: TypeId) -> Option<&'static StateCodec> {
    inventory::iter::<StateCodec>
        .into_iter()
        .find(|c| (c.type_id)() == id)
}

const STATE_MAX: usize = 1024 * 1024;

fn encode_growing(f: impl Fn(&mut Writer<'_>)) -> Option<Vec<u8>> {
    let mut size = 1024;
    while size <= STATE_MAX {
        let mut buf = alloc::vec![0u8; size];
        let mut w = Writer::new(&mut buf);
        f(&mut w);
        if let Some(n) = w.finish() {
            buf.truncate(n);
            return Some(buf);
        }
        size *= 2;
    }
    None
}

impl Guest for Runtime {
    fn init(cx: InitContext) -> core::result::Result<(), types::Error> {
        crate::log::install_panic_hook();
        let app = the_app()?;
        match cx.kind {
            InstanceKind::Cycle => {
                let layout = check(&cx, app, true).map_err(|m| error("J-E0008", m))?;
                let tasks = by_id::<CycleTask>(|t| t.name);
                let states = tasks.iter().map(|t| t.and_then(|t| (t.state)())).collect();
                let pending = alloc::vec![None; layout.len()];
                joker_sdk_sys::cycle::reserve(cx.image_len as usize, cx.proposal_capacity as usize);
                critical_section::with(|cs| {
                    CYCLE.borrow_ref_mut(cs).replace(CycleState {
                        layout,
                        tasks,
                        states,
                        pending,
                    });
                });
            }
            InstanceKind::Task => {
                let layout = check(&cx, app, false).map_err(|m| error("J-E0008", m))?;
                let layout: &'static [Layout] = Box::leak(layout.into_boxed_slice());
                let tasks = by_id::<AsyncTask>(|t| t.name);
                let states = tasks
                    .iter()
                    .map(|t| {
                        t.and_then(|t| (t.state)()).map(|s| {
                            let s: &'static (dyn Any + Send + Sync) = Box::leak(s);
                            s
                        })
                    })
                    .collect();
                critical_section::with(|cs| {
                    ASYNC.borrow_ref_mut(cs).replace(AsyncState {
                        layout,
                        tasks,
                        states,
                    });
                });
            }
        }
        Ok(())
    }

    fn on_params(changes: Vec<ParamChange>) -> core::result::Result<(), types::Error> {
        let app = the_app()?;
        let items = changes
            .iter()
            .filter_map(|c| {
                let decl = app.ports.get(c.port as usize)?;
                Some((decl.name, Value::from_bits(decl.ty, host::bits(&c.value))))
            })
            .collect();
        let changes = ParamChanges::new(items);
        for handler in inventory::iter::<OnParams> {
            (handler.run)(&changes).map_err(wire)?;
        }
        Ok(())
    }

    async fn run_task(task: u32, why: Trigger) -> core::result::Result<(), types::Error> {
        let found = critical_section::with(|cs| {
            let guard = ASYNC.borrow_ref(cs);
            let state = guard.as_ref()?;
            let entry = (*state.tasks.get(task as usize)?)?;
            let shared = state.states.get(task as usize).copied().flatten();
            Some((entry, state.layout, shared))
        });
        let Some((entry, layout, state)) = found else {
            return Err(error("J-E0008", "no such async task"));
        };
        let run = (entry.run)(TaskArgs {
            layout,
            name: entry.name,
            why,
            state,
        });
        run.await.map_err(wire)
    }

    async fn on_action(req: ActionRequest) -> core::result::Result<Vec<u8>, types::Error> {
        drop(req);
        Err(error(
            "J-E0601",
            "actions are not served by this SDK version",
        ))
    }

    fn export_state() -> core::result::Result<Vec<u8>, types::Error> {
        let app = the_app()?;
        let mut saved: Vec<(&'static str, u32, Vec<u8>)> = Vec::new();
        let failed = critical_section::with(|cs| {
            let guard = CYCLE.borrow_ref(cs);
            let Some(state) = guard.as_ref() else {
                return false;
            };
            for (task, st) in state.tasks.iter().zip(&state.states) {
                let (Some(task), Some(st)) = (task, st) else {
                    continue;
                };
                let Some(codec) = codec_of(state_type(st.as_ref())).filter(|c| c.persist) else {
                    continue;
                };
                match encode_growing(|w| {
                    (codec.save)(st.as_ref(), w);
                }) {
                    Some(bytes) => saved.push((task.name, codec.version, bytes)),
                    None => return true,
                }
            }
            false
        });
        if failed {
            return Err(error("J-E0915", "a task state is above 1 MiB"));
        }
        if saved.is_empty() {
            return Ok(Vec::new());
        }
        let version = saved.iter().map(|s| s.1).max().unwrap_or(1);
        encode_growing(|w| {
            w.map(5);
            w.uint(0);
            w.uint(1);
            w.uint(1);
            w.text(app.version);
            w.uint(2);
            w.uint(u64::from(version));
            w.uint(3);
            w.uint(0);
            w.uint(4);
            w.map(saved.len());
            for (name, _, bytes) in &saved {
                w.text(name);
                w.bytes(bytes);
            }
        })
        .ok_or_else(|| error("J-E0915", "the state blob is above 1 MiB"))
    }

    fn migrate(previous: Vec<u8>, from_version: String) -> core::result::Result<(), types::Error> {
        if previous.is_empty() {
            return Ok(());
        }
        let blob = parse_blob(&previous).ok_or_else(|| error("J-E0914", "invalid state blob"))?;
        let mut replaced: Vec<(usize, Box<dyn Any + Send>)> = Vec::new();
        let tasks = critical_section::with(|cs| {
            CYCLE.borrow_ref(cs).as_ref().map(|s| {
                s.tasks
                    .iter()
                    .zip(&s.states)
                    .map(|(t, st)| {
                        (
                            t.map_or("", |t| t.name),
                            st.as_ref().map(|b| state_type(b.as_ref())),
                        )
                    })
                    .collect::<Vec<_>>()
            })
        });
        for (i, (name, id)) in tasks.unwrap_or_default().into_iter().enumerate() {
            let Some(id) = id else { continue };
            let Some(codec) = codec_of(id).filter(|c| c.persist) else {
                continue;
            };
            let Some(bytes) = blob.tasks.iter().find(|(n, _)| *n == name).map(|t| t.1) else {
                continue;
            };
            let prev = StateBlob::new(blob.version, bytes);
            let custom = inventory::iter::<Migration>
                .into_iter()
                .find(|m| (m.type_id)() == id);
            let state = match custom {
                Some(m) => (m.run)(prev, &from_version).map_err(wire)?,
                None => (codec.load)(&prev),
            };
            replaced.push((i, state));
        }
        critical_section::with(|cs| {
            if let Some(s) = CYCLE.borrow_ref_mut(cs).as_mut() {
                for (i, state) in replaced {
                    if let Some(slot) = s.states.get_mut(i) {
                        *slot = Some(state);
                    }
                }
            }
        });
        Ok(())
    }
}

struct Blob<'a> {
    version: u32,
    tasks: Vec<(&'a str, &'a [u8])>,
}

fn parse_blob(bytes: &[u8]) -> Option<Blob<'_>> {
    let mut r = Reader::new(bytes);
    let n = r.map()?;
    let mut version = 1;
    let mut tasks = Vec::new();
    for _ in 0..n {
        match r.uint()? {
            0 => {
                if r.uint()? != 1 {
                    return None;
                }
            }
            2 => version = u32::try_from(r.uint()?).ok()?,
            4 => {
                let m = r.map()?;
                for _ in 0..m {
                    let name = r.text()?;
                    tasks.push((name, r.bytes()?));
                }
            }
            _ => r.skip()?,
        }
    }
    Some(Blob { version, tasks })
}

impl CycleGuest for Runtime {
    fn on_cycle(task: u32, image: &[u8], out: &mut CycleOut<'_>) {
        critical_section::with(|cs| {
            let mut guard = CYCLE.borrow_ref_mut(cs);
            let Some(state) = guard.as_mut() else {
                out.fail("J-E0008", "on-cycle before init");
                return;
            };
            let Some(entry) = state.tasks.get(task as usize).copied().flatten() else {
                out.fail("J-E0008", "no such cycle task");
                return;
            };
            let (len, full, result) = {
                let buffer = ProposalBuffer::new(out.proposals(), &mut state.pending);
                let cx = CycleCx::new(image, &state.layout, entry.name, buffer);
                let result = match state.states.get_mut(task as usize) {
                    Some(slot) => (entry.run)(&mut CycleRun::new(&cx, slot)),
                    None => Err(sdk("J-E0008", "no task state")),
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
