//! Test guest of `joker-host`: a cycle app written by hand against the raw
//! `joker:runtime` bindings, without the SDK, so the host can be tested on its
//! own. The tests of `joker-host` build it for `wasm32-wasip2` and add its
//! `joker:meta` section.
//!
//! Ports, in `joker:meta` order: `x` (input, f64 kW), `gain` (param, f64),
//! `y` (output, f64 kW), `nan` (output, u64) and `r` (output, u64). Tasks, in
//! name order: `bad`, `echo`, `empty`, `panic` and `spin`.
//!
//! The crate is empty on other targets: its exports only link into a
//! component.

#![cfg(target_family = "wasm")]

use joker_sdk_sys::exports::joker::runtime::guest::{
    ActionRequest, CycleResult, Guest, InitContext, ParamChange, Trigger,
};
use joker_sdk_sys::joker::runtime::cycle_io;
use joker_sdk_sys::joker::runtime::types::Error;

struct Echo;

const LAYOUT: [(&str, u32, Option<u32>); 5] = [
    ("x", 0, None),
    ("gain", 1, None),
    ("y", 2, Some(0)),
    ("nan", 3, Some(1)),
    ("r", 4, Some(2)),
];

fn error(code: &str, message: &str) -> Error {
    Error {
        code: code.to_owned(),
        message: message.to_owned(),
    }
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    let mut w = [0u8; 4];
    w.copy_from_slice(&b[at..at + 4]);
    u32::from_le_bytes(w)
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    let mut w = [0u8; 8];
    w.copy_from_slice(&b[at..at + 8]);
    u64::from_le_bytes(w)
}

struct Buffer(Vec<u8>);

impl Buffer {
    fn new() -> Self {
        Self(vec![1, 0, 0, 0, 0, 0, 0, 0])
    }

    fn count(&mut self) {
        let n = u16::from_le_bytes([self.0[2], self.0[3]]) + 1;
        self.0[2..4].copy_from_slice(&n.to_le_bytes());
    }

    fn write(&mut self, port: u32, value: u64) {
        self.0.extend_from_slice(&[1, 0]);
        self.0.extend_from_slice(&port.to_le_bytes());
        self.0.extend_from_slice(&value.to_le_bytes());
        self.count();
    }

    fn note(&mut self, port: u32) {
        self.0.extend_from_slice(&[6, 0]);
        self.0.extend_from_slice(&port.to_le_bytes());
        self.count();
    }

    fn finish(mut self) -> Vec<u8> {
        let len = self.0.len() as u32;
        self.0[4..8].copy_from_slice(&len.to_le_bytes());
        self.0
    }
}

fn echo(image: &[u8]) -> CycleResult {
    let sample = |slot: usize| {
        let at = 64 + 24 * slot;
        (u64_at(image, at), image[at + 8])
    };
    let mut buffer = Buffer::new();
    let mut failure = None;
    let (bits, quality) = sample(0);
    let x = if quality == 0 {
        f64::from_bits(bits)
    } else {
        buffer.note(0);
        failure = Some(error("J-E0101", "x is not good"));
        0.0
    };
    let gain = f64::from_bits(sample(1).0);
    let zero = x * 0.0;
    let nan = zero / (x * 0.0);
    if cycle_io::now().mono_ns != u64_at(image, 16) {
        failure = Some(error("J-E0050", "now() is not the latch instant"));
    }
    buffer.write(2, (x * gain + x.abs().sqrt()).to_bits());
    buffer.write(3, nan.to_bits());
    buffer.write(4, cycle_io::random_u64() ^ cycle_io::dt_ns());
    CycleResult {
        proposals: buffer.finish(),
        error: failure,
    }
}

impl Guest for Echo {
    fn init(cx: InitContext) -> Result<(), Error> {
        let layout_ok = cx.ports.len() == LAYOUT.len()
            && cx.ports.iter().zip(LAYOUT).all(|(p, (_, slot, outcome))| {
                p.bound && p.slot == Some(slot) && p.outcome == outcome
            })
            && cx.image_len == 64 + 24 * 8
            && cx.cycle_abi == 1;
        if layout_ok {
            Ok(())
        } else {
            Err(error("J-E0008", "unexpected init-context layout"))
        }
    }

    fn on_params(changes: Vec<ParamChange>) -> Result<(), Error> {
        drop(changes);
        Ok(())
    }

    fn on_cycle(task: u32, image: Vec<u8>) -> CycleResult {
        match task {
            0 => CycleResult {
                proposals: vec![1, 0, 1, 0, 8, 0, 0, 0],
                error: None,
            },
            1 => echo(&image),
            2 => CycleResult {
                proposals: Vec::new(),
                error: None,
            },
            3 => std::process::abort(),
            _ => {
                let mut n = u32_at(&image, 4);
                loop {
                    n = core::hint::black_box(n.wrapping_add(1));
                }
            }
        }
    }

    async fn run_task(task: u32, why: Trigger) -> Result<(), Error> {
        drop((task, why));
        Err(error("J-E0601", "no async tasks"))
    }

    async fn on_action(req: ActionRequest) -> Result<Vec<u8>, Error> {
        drop(req);
        Err(error("J-E0601", "no actions"))
    }

    fn export_state() -> Result<Vec<u8>, Error> {
        Ok(Vec::new())
    }

    fn migrate(previous: Vec<u8>, from_version: String) -> Result<(), Error> {
        drop((previous, from_version));
        Ok(())
    }
}

joker_sdk_sys::export!(Echo with_types_in joker_sdk_sys);
