//! Test guest of `joker-host`: a cycle app written by hand against the raw
//! `joker:runtime` bindings, without the SDK, so the host can be tested on its
//! own. The tests of `joker-host` build it for `wasm32-wasip2` and add its
//! `joker:meta` section.
//!
//! Ports, in `joker:meta` order: `x` (input, f64 kW), `gain` (param, f64),
//! `y` (output, f64 kW), `nan` (output, u64) and `r` (output, u64). Tasks, in
//! name order: `bad`, `echo`, `empty`, `panic` and `spin`.
//!
//! The cycle tasks write into the reserved proposal buffer and do not
//! allocate: the global allocator of `joker-sdk-sys` traps with `J-E0903`
//! if they did. The crate is empty on other targets: its exports only link
//! into a component.

#![cfg(target_family = "wasm")]

use joker_sdk_sys::cycle::{CycleGuest, CycleOut};
use joker_sdk_sys::exports::joker::runtime::guest::{
    ActionRequest, Guest, InitContext, ParamChange, Trigger,
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

struct Buffer<'a> {
    bytes: &'a mut [u8],
    len: usize,
}

impl<'a> Buffer<'a> {
    fn new(bytes: &'a mut [u8]) -> Self {
        bytes[..8].copy_from_slice(&[1, 0, 0, 0, 0, 0, 0, 0]);
        Self { bytes, len: 8 }
    }

    fn push(&mut self, record: &[u8]) {
        self.bytes[self.len..self.len + record.len()].copy_from_slice(record);
        self.len += record.len();
        let n = u16::from_le_bytes([self.bytes[2], self.bytes[3]]) + 1;
        self.bytes[2..4].copy_from_slice(&n.to_le_bytes());
    }

    fn write(&mut self, port: u32, value: u64) {
        let mut r = [1u8; 14];
        r[1] = 0;
        r[2..6].copy_from_slice(&port.to_le_bytes());
        r[6..14].copy_from_slice(&value.to_le_bytes());
        self.push(&r);
    }

    fn note(&mut self, port: u32) {
        let mut r = [6u8; 6];
        r[1] = 0;
        r[2..6].copy_from_slice(&port.to_le_bytes());
        self.push(&r);
    }

    fn finish(self) -> usize {
        let len = self.len as u32;
        self.bytes[4..8].copy_from_slice(&len.to_le_bytes());
        self.len
    }
}

fn echo(image: &[u8], out: &mut CycleOut<'_>) {
    let sample = |slot: usize| {
        let at = 64 + 24 * slot;
        (u64_at(image, at), image[at + 8])
    };
    let mut buffer = Buffer::new(out.proposals());
    let mut failure = None;
    let (bits, quality) = sample(0);
    let x = if quality == 0 {
        f64::from_bits(bits)
    } else {
        buffer.note(0);
        failure = Some(("J-E0101", "x is not good"));
        0.0
    };
    let gain = f64::from_bits(sample(1).0);
    let zero = x * 0.0;
    let nan = zero / (x * 0.0);
    if cycle_io::now().mono_ns != u64_at(image, 16) {
        failure = Some(("J-E0050", "now() is not the latch instant"));
    }
    buffer.write(2, (x * gain + x.abs().sqrt()).to_bits());
    buffer.write(3, nan.to_bits());
    buffer.write(4, cycle_io::random_u64() ^ cycle_io::dt_ns());
    let len = buffer.finish();
    out.set_len(len);
    if let Some((code, message)) = failure {
        out.fail(code, message);
    }
}

impl CycleGuest for Echo {
    fn on_cycle(task: u32, image: &[u8], out: &mut CycleOut<'_>) {
        match task {
            0 => {
                out.proposals()[..8].copy_from_slice(&[1, 0, 1, 0, 8, 0, 0, 0]);
                out.set_len(8);
            }
            1 => echo(image, out),
            2 => {}
            3 => std::process::abort(),
            _ => {
                let mut n = u32_at(image, 4);
                loop {
                    n = core::hint::black_box(n.wrapping_add(1));
                }
            }
        }
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
            joker_sdk_sys::cycle::reserve(cx.image_len as usize, cx.proposal_capacity as usize);
            Ok(())
        } else {
            Err(error("J-E0008", "unexpected init-context layout"))
        }
    }

    fn on_params(changes: Vec<ParamChange>) -> Result<(), Error> {
        drop(changes);
        Ok(())
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

joker_sdk_sys::export_guest!(Echo);
