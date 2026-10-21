use core::fmt;

use crate::program::{CmpOp, Op, Program, Stateful, WINDOW_HEADER, WINDOW_SAMPLE, WindowKind};
use crate::quality::{Quality, QualityCode, Reason};
use crate::value::{Output, Value};

/// One sample as the VM reads it: the slot word, its quality and when it
/// was acquired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    /// The `u64` slot encoding of the value (ref/00 §4.1).
    pub raw: u64,
    /// The quality.
    pub quality: Quality,
    /// The monotonic acquisition instant, in ns (`ts_acq`).
    pub acquired: u64,
}

/// Where a program reads its signals from: the latched snapshot of the
/// group (docs/02 §6.1).
pub trait Inputs {
    /// The sample of a signal.
    fn read(&self, signal: u32) -> Reading;

    /// The caller's last proposal for an output, or its applied value
    /// without one (`own`, docs/16 §4). Only class methods use it.
    fn own(&self, signal: u32) -> Reading {
        self.read(signal)
    }
}

/// An evaluation that could not run. A program produced by
/// [`crate::compile`] with a state arena of [`Program::state_words`]
/// words never fails; these guard against a corrupted program or arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvalError {
    /// The state arena is smaller than [`Program::state_words`].
    StateTooSmall,
    /// The program pops more than it pushed or overflows the stack.
    Stack,
    /// The program refers to a table entry that does not exist.
    BadIndex,
    /// The program leaves other than exactly one value on the stack.
    Result,
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            EvalError::StateTooSmall => "state arena too small",
            EvalError::Stack => "stack underflow or overflow",
            EvalError::BadIndex => "table index out of range",
            EvalError::Result => "program does not leave one result",
        })
    }
}

impl core::error::Error for EvalError {}

#[derive(Clone, Copy)]
struct Slot {
    value: Value,
    quality: Quality,
}

const EMPTY: Slot = Slot {
    value: Value::Bool(false),
    quality: Quality::GOOD,
};

struct Stack {
    slots: [Slot; crate::program::MAX_STACK],
    len: usize,
}

impl Stack {
    fn push(&mut self, value: Value, quality: Quality) -> Result<(), EvalError> {
        let quality = if value.is_finite() {
            quality
        } else {
            quality.worst(Quality::BAD_NAN)
        };
        *self.slots.get_mut(self.len).ok_or(EvalError::Stack)? = Slot { value, quality };
        self.len += 1;
        Ok(())
    }

    fn pop(&mut self) -> Result<Slot, EvalError> {
        self.len = self.len.checked_sub(1).ok_or(EvalError::Stack)?;
        self.slots.get(self.len).copied().ok_or(EvalError::Stack)
    }

    fn top_n(&mut self, n: u8) -> Result<&[Slot], EvalError> {
        let start = self.len.checked_sub(n as usize).ok_or(EvalError::Stack)?;
        self.len = start;
        self.slots
            .get(start..start + n as usize)
            .ok_or(EvalError::Stack)
    }
}

fn worst_of(slots: &[Slot]) -> Quality {
    slots.iter().fold(Quality::GOOD, |q, s| q.worst(s.quality))
}

fn compare(op: CmpOp, a: Value, b: Value) -> bool {
    use core::cmp::Ordering;
    let ord = match (a, b) {
        (Value::I64(x), Value::I64(y)) => Some(x.cmp(&y)),
        (Value::U64(x), Value::U64(y)) => Some(x.cmp(&y)),
        (Value::Bool(x), Value::Bool(y)) => Some(x.cmp(&y)),
        _ => a.as_f64().partial_cmp(&b.as_f64()),
    };
    match (op, ord) {
        (_, None) => op == CmpOp::Ne,
        (CmpOp::Eq, Some(o)) => o == Ordering::Equal,
        (CmpOp::Ne, Some(o)) => o != Ordering::Equal,
        (CmpOp::Lt, Some(o)) => o == Ordering::Less,
        (CmpOp::Le, Some(o)) => o != Ordering::Greater,
        (CmpOp::Gt, Some(o)) => o == Ordering::Greater,
        (CmpOp::Ge, Some(o)) => o != Ordering::Less,
    }
}

fn is_decisive(slot: &Slot, value: bool) -> bool {
    slot.quality.is_good() && slot.value.as_bool() == value
}

/// Evaluates `any` (`decisive = true`) or `all` (`decisive = false`) with
/// the generalised short-circuit of docs/16 §3.2.
fn short_circuit(slots: &[Slot], decisive: bool) -> (Value, Quality) {
    if slots.iter().any(|s| is_decisive(s, decisive)) {
        return (Value::Bool(decisive), Quality::GOOD);
    }
    let value = if decisive {
        slots.iter().any(|s| s.value.as_bool())
    } else {
        slots.iter().all(|s| s.value.as_bool())
    };
    (Value::Bool(value), worst_of(slots))
}

fn fold_f64(slots: &[Slot], init: f64, f: impl Fn(f64, f64) -> f64) -> f64 {
    slots.iter().fold(init, |acc, s| f(acc, s.value.as_f64()))
}

fn min_f64(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

fn max_f64(a: f64, b: f64) -> f64 {
    if b > a { b } else { a }
}

/// Evaluates a program at instant `now` (monotonic ns, the latch of the
/// tick) against the inputs, updating the state of its stateful
/// functions. Never allocates and never panics; runs in time linear in
/// the program, plus the amortised window updates.
pub fn eval(
    program: &Program,
    inputs: &dyn Inputs,
    now: u64,
    state: &mut [u64],
) -> Result<Output, EvalError> {
    if state.len() < program.state_words() {
        return Err(EvalError::StateTooSmall);
    }
    let mut stack = Stack {
        slots: [EMPTY; crate::program::MAX_STACK],
        len: 0,
    };
    for op in program.code() {
        step(program, inputs, now, state, &mut stack, *op)?;
    }
    if stack.len != 1 {
        return Err(EvalError::Result);
    }
    let out = stack.pop()?;
    Ok(Output::new(out.value, out.quality))
}

fn read(
    program: &Program,
    inputs: &dyn Inputs,
    i: u16,
    own: bool,
) -> Result<(Value, Quality, u64), EvalError> {
    let input = program.input(i).ok_or(EvalError::BadIndex)?;
    let r = if own {
        inputs.own(input.signal)
    } else {
        inputs.read(input.signal)
    };
    Ok((Value::decode(r.raw, input.ty), r.quality, r.acquired))
}

fn step(
    program: &Program,
    inputs: &dyn Inputs,
    now: u64,
    state: &mut [u64],
    stack: &mut Stack,
    op: Op,
) -> Result<(), EvalError> {
    match op {
        Op::Const(i) => {
            let v = program.constant(i).ok_or(EvalError::BadIndex)?;
            stack.push(v, Quality::GOOD)
        }
        Op::Load(i) | Op::Own(i) => {
            let (v, q, _) = read(program, inputs, i, matches!(op, Op::Own(_)))?;
            stack.push(v, q)
        }
        Op::IsGood(i) => {
            let (_, q, _) = read(program, inputs, i, false)?;
            stack.push(Value::Bool(q.is_good()), Quality::GOOD)
        }
        Op::QualityOf(i) => {
            let (_, q, _) = read(program, inputs, i, false)?;
            stack.push(Value::U64(q.code as u64), Quality::GOOD)
        }
        Op::Age(i) => {
            let (_, _, t) = read(program, inputs, i, false)?;
            let age = now.saturating_sub(t) as f64 / 1e9;
            stack.push(Value::F64(age), Quality::GOOD)
        }
        Op::Convert(i) => {
            let conv = program.conversion(i).ok_or(EvalError::BadIndex)?;
            let a = stack.pop()?;
            stack.push(Value::F64(conv.apply(a.value.as_f64())), a.quality)
        }
        Op::Neg => {
            let a = stack.pop()?;
            stack.push(Value::F64(-a.value.as_f64()), a.quality)
        }
        Op::Not => {
            let a = stack.pop()?;
            stack.push(Value::Bool(!a.value.as_bool()), a.quality)
        }
        Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Pow => {
            let b = stack.pop()?;
            let a = stack.pop()?;
            let (x, y) = (a.value.as_f64(), b.value.as_f64());
            let r = match op {
                Op::Add => x + y,
                Op::Sub => x - y,
                Op::Mul => x * y,
                Op::Div => x / y,
                _ => libm::pow(x, y),
            };
            stack.push(Value::F64(r), a.quality.worst(b.quality))
        }
        Op::Cmp(cmp) => {
            let b = stack.pop()?;
            let a = stack.pop()?;
            stack.push(
                Value::Bool(compare(cmp, a.value, b.value)),
                a.quality.worst(b.quality),
            )
        }
        Op::And | Op::Or => {
            let b = stack.pop()?;
            let a = stack.pop()?;
            let (v, q) = short_circuit(&[a, b], op == Op::Or);
            stack.push(v, q)
        }
        Op::Select => {
            let b = stack.pop()?;
            let a = stack.pop()?;
            let c = stack.pop()?;
            let chosen = if c.value.as_bool() { a } else { b };
            let q = match c.quality.code {
                QualityCode::Bad => c.quality,
                QualityCode::Uncertain => chosen.quality.worst(c.quality),
                QualityCode::Good => chosen.quality,
            };
            stack.push(chosen.value, q)
        }
        Op::Abs | Op::Sqrt => {
            let a = stack.pop()?;
            let x = a.value.as_f64();
            let r = if op == Op::Abs {
                x.abs()
            } else {
                libm::sqrt(x)
            };
            stack.push(Value::F64(r), a.quality)
        }
        Op::Min(n) | Op::Max(n) | Op::Maxs(n) | Op::Mins(n) => {
            let slots = stack.top_n(n)?;
            let q = worst_of(slots);
            let r = match op {
                Op::Min(_) | Op::Mins(_) => fold_f64(slots, f64::INFINITY, min_f64),
                _ => fold_f64(slots, f64::NEG_INFINITY, max_f64),
            };
            stack.push(Value::F64(r), q)
        }
        Op::Sum(n) => {
            let slots = stack.top_n(n)?;
            let q = worst_of(slots);
            let r = fold_f64(slots, 0.0, |a, b| a + b);
            stack.push(Value::F64(r), q)
        }
        Op::Count(n) => {
            let slots = stack.top_n(n)?;
            let q = worst_of(slots);
            let r = slots.iter().filter(|s| s.value.as_bool()).count() as u64;
            stack.push(Value::U64(r), q)
        }
        Op::Any(n) | Op::All(n) => {
            let slots = stack.top_n(n)?;
            let (v, q) = short_circuit(slots, matches!(op, Op::Any(_)));
            stack.push(v, q)
        }
        Op::Clamp => {
            let hi = stack.pop()?;
            let lo = stack.pop()?;
            let x = stack.pop()?;
            let r = max_f64(
                lo.value.as_f64(),
                min_f64(x.value.as_f64(), hi.value.as_f64()),
            );
            stack.push(Value::F64(r), x.quality.worst(lo.quality).worst(hi.quality))
        }
        Op::Bit(n) => {
            let a = stack.pop()?;
            let bit = a.value.as_word().checked_shr(u32::from(n)).unwrap_or(0) & 1;
            stack.push(Value::Bool(bit == 1), a.quality)
        }
        Op::Held(i) => {
            let s = program.stateful(i).ok_or(EvalError::BadIndex)?;
            let words = words(state, s.offset, 2)?;
            let c = stack.pop()?;
            let active = !c.quality.is_bad() && c.value.as_bool();
            if !active {
                words[1] = 0;
                return stack.push(Value::Bool(false), c.quality);
            }
            if words[1] == 0 {
                words[0] = now;
                words[1] = 1;
            }
            let held = now.saturating_sub(words[0]) >= s.duration.as_nanos();
            stack.push(Value::Bool(held), c.quality)
        }
        Op::Rose(i) | Op::Fell(i) => {
            let s = program.stateful(i).ok_or(EvalError::BadIndex)?;
            let words = words(state, s.offset, 1)?;
            let c = stack.pop()?;
            if c.quality.is_bad() {
                return stack.push(Value::Bool(false), c.quality);
            }
            let prev = words[0] != 0;
            let now_v = c.value.as_bool();
            words[0] = u64::from(now_v);
            let edge = if matches!(op, Op::Rose(_)) {
                !prev && now_v
            } else {
                prev && !now_v
            };
            stack.push(Value::Bool(edge), c.quality)
        }
        Op::Hyst(i) => {
            let s = program.stateful(i).ok_or(EvalError::BadIndex)?;
            let words = words(state, s.offset, 1)?;
            let hi = stack.pop()?;
            let lo = stack.pop()?;
            let x = stack.pop()?;
            let q = x.quality.worst(lo.quality).worst(hi.quality);
            if !q.is_bad() {
                let v = x.value.as_f64();
                if v > hi.value.as_f64() {
                    words[0] = 1;
                } else if v < lo.value.as_f64() {
                    words[0] = 0;
                }
            }
            stack.push(Value::Bool(words[0] != 0), q)
        }
        Op::Window(kind, i) => {
            let s = program.stateful(i).ok_or(EvalError::BadIndex)?;
            let x = stack.pop()?;
            let (v, q) = window(kind, s, state, now, x)?;
            stack.push(v, q)
        }
    }
}

fn words(state: &mut [u64], offset: u32, n: usize) -> Result<&mut [u64], EvalError> {
    let start = offset as usize;
    state
        .get_mut(start..start + n)
        .ok_or(EvalError::StateTooSmall)
}

const SEQ: usize = 0;
const LEN: usize = 1;
const BAD: usize = 2;
const UNCERTAIN: usize = 3;
const SUM: usize = 4;
const USABLE: usize = 5;
const DQ_HEAD: usize = 6;
const DQ_LEN: usize = 7;
const LAST_BAD: usize = 8;
const LAST_UNCERTAIN: usize = 9;

struct Ring<'a> {
    header: &'a mut [u64],
    samples: &'a mut [u64],
    deque: &'a mut [u64],
    capacity: u64,
}

impl Ring<'_> {
    fn sample(&self, seq: u64) -> Option<(f64, Quality, u64)> {
        let at = (seq % self.capacity) as usize * WINDOW_SAMPLE;
        let s = self.samples.get(at..at + WINDOW_SAMPLE)?;
        Some((f64::from_bits(s[0]), Quality::from_bits(s[1] as u32), s[2]))
    }

    fn store(&mut self, seq: u64, value: f64, quality: Quality, t: u64) {
        let at = (seq % self.capacity) as usize * WINDOW_SAMPLE;
        if let Some(s) = self.samples.get_mut(at..at + WINDOW_SAMPLE) {
            s[0] = value.to_bits();
            s[1] = u64::from(quality.to_bits());
            s[2] = t;
        }
    }

    fn oldest(&self) -> u64 {
        self.header[SEQ].saturating_sub(self.header[LEN].min(self.capacity))
    }

    fn deque_get(&self, i: u64) -> u64 {
        let at =
            ((self.header[DQ_HEAD] % self.capacity + i % self.capacity) % self.capacity) as usize;
        self.deque.get(at).copied().unwrap_or(0)
    }

    fn deque_push_back(&mut self, seq: u64) {
        let at = ((self.header[DQ_HEAD] % self.capacity + self.header[DQ_LEN] % self.capacity)
            % self.capacity) as usize;
        if let Some(slot) = self.deque.get_mut(at) {
            *slot = seq;
            self.header[DQ_LEN] += 1;
        }
    }

    fn deque_pop_front(&mut self) {
        self.header[DQ_HEAD] = (self.header[DQ_HEAD] % self.capacity + 1) % self.capacity;
        self.header[DQ_LEN] = self.header[DQ_LEN].saturating_sub(1);
    }

    fn evict(&mut self) {
        let seq = self.oldest();
        let Some((v, q, _)) = self.sample(seq) else {
            return;
        };
        match q.code {
            QualityCode::Bad => self.header[BAD] = self.header[BAD].saturating_sub(1),
            QualityCode::Uncertain => {
                self.header[UNCERTAIN] = self.header[UNCERTAIN].saturating_sub(1)
            }
            QualityCode::Good => {}
        }
        if !q.is_bad() {
            self.header[USABLE] = self.header[USABLE].saturating_sub(1);
            self.header[SUM] = (f64::from_bits(self.header[SUM]) - v).to_bits();
        }
        if !self.deque.is_empty() && self.header[DQ_LEN] > 0 && self.deque_get(0) == seq {
            self.deque_pop_front();
        }
        self.header[LEN] = self.header[LEN].saturating_sub(1);
    }

    fn push(&mut self, value: f64, quality: Quality, t: u64, max: bool) {
        if self.header[LEN] >= self.capacity {
            self.header[LEN] = self.capacity;
            self.evict();
        }
        let seq = self.header[SEQ];
        self.store(seq, value, quality, t);
        self.header[SEQ] = self.header[SEQ].wrapping_add(1);
        self.header[LEN] += 1;
        match quality.code {
            QualityCode::Bad => {
                self.header[BAD] = self.header[BAD].saturating_add(1);
                self.header[LAST_BAD] = u64::from(quality.reason.0);
            }
            QualityCode::Uncertain => {
                self.header[UNCERTAIN] = self.header[UNCERTAIN].saturating_add(1);
                self.header[LAST_UNCERTAIN] = u64::from(quality.reason.0);
            }
            QualityCode::Good => {}
        }
        if !quality.is_bad() {
            self.header[USABLE] = self.header[USABLE].saturating_add(1);
            self.header[SUM] = (f64::from_bits(self.header[SUM]) + value).to_bits();
            if !self.deque.is_empty() {
                self.header[DQ_LEN] = self.header[DQ_LEN].min(self.capacity - 1);
                while self.header[DQ_LEN] > 0 {
                    let back = self.deque_get(self.header[DQ_LEN] - 1);
                    let (bv, _, _) = self.sample(back).unwrap_or((0.0, Quality::GOOD, 0));
                    let dominated = if max { bv <= value } else { bv >= value };
                    if !dominated {
                        break;
                    }
                    self.header[DQ_LEN] = self.header[DQ_LEN].saturating_sub(1);
                }
                self.deque_push_back(seq);
            }
        }
        if self.header[SEQ].is_multiple_of(self.capacity) {
            self.resum();
        }
    }

    fn resum(&mut self) {
        let mut sum = 0.0;
        for seq in self.oldest()
            ..self
                .oldest()
                .saturating_add(self.header[LEN].min(self.capacity))
        {
            if let Some((v, q, _)) = self.sample(seq)
                && !q.is_bad()
            {
                sum += v;
            }
        }
        self.header[SUM] = sum.to_bits();
    }

    fn quality(&self) -> Quality {
        let cap = self.capacity;
        let bad = self.header[BAD];
        let uncertain = self.header[UNCERTAIN];
        let missing = cap.saturating_sub(self.header[LEN]);
        if self.header[USABLE] == 0 || bad.saturating_mul(10) >= cap {
            return Quality::new(QualityCode::Bad, Reason(self.header[LAST_BAD] as u8));
        }
        if bad
            .saturating_add(uncertain)
            .saturating_add(missing)
            .saturating_mul(10)
            >= cap
        {
            let reason = if uncertain > 0 {
                Reason(self.header[LAST_UNCERTAIN] as u8)
            } else if missing > 0 {
                Reason::INITIAL
            } else {
                Reason(self.header[LAST_BAD] as u8)
            };
            return Quality::new(QualityCode::Uncertain, reason);
        }
        Quality::GOOD
    }

    fn rate(&self) -> Option<f64> {
        let mut first = None;
        let mut last = None;
        for seq in self.oldest()
            ..self
                .oldest()
                .saturating_add(self.header[LEN].min(self.capacity))
        {
            if let Some((v, q, t)) = self.sample(seq)
                && !q.is_bad()
            {
                first = Some((v, t));
                break;
            }
        }
        for seq in (self.oldest()
            ..self
                .oldest()
                .saturating_add(self.header[LEN].min(self.capacity)))
            .rev()
        {
            if let Some((v, q, t)) = self.sample(seq)
                && !q.is_bad()
            {
                last = Some((v, t));
                break;
            }
        }
        let ((v0, t0), (v1, t1)) = (first?, last?);
        if t1 <= t0 {
            return None;
        }
        Some((v1 - v0) / ((t1 - t0) as f64 / 1e9))
    }
}

fn window(
    kind: WindowKind,
    spec: Stateful,
    state: &mut [u64],
    now: u64,
    x: Slot,
) -> Result<(Value, Quality), EvalError> {
    let capacity = u64::from(spec.capacity.max(1));
    let cap = capacity as usize;
    let deque_words = if matches!(kind, WindowKind::Min | WindowKind::Max) {
        cap
    } else {
        0
    };
    let total = WINDOW_HEADER + cap * WINDOW_SAMPLE + deque_words;
    let arena = words(state, spec.offset, total)?;
    let (header, rest) = arena.split_at_mut(WINDOW_HEADER);
    let (samples, deque) = rest.split_at_mut(cap * WINDOW_SAMPLE);
    let mut ring = Ring {
        header,
        samples,
        deque,
        capacity,
    };
    let max = kind == WindowKind::Max;
    ring.push(x.value.as_f64(), x.quality, now, max);
    let quality = ring.quality();
    if quality.is_bad() {
        return Ok((Value::F64(f64::NAN), quality));
    }
    let value = match kind {
        WindowKind::Avg => f64::from_bits(ring.header[SUM]) / ring.header[USABLE] as f64,
        WindowKind::Min | WindowKind::Max => {
            let seq = ring.deque_get(0);
            ring.sample(seq).map_or(f64::NAN, |(v, _, _)| v)
        }
        WindowKind::Rate => match ring.rate() {
            Some(r) => r,
            None => return Ok((Value::F64(f64::NAN), Quality::BAD_NAN)),
        },
    };
    Ok((Value::F64(value), quality))
}
