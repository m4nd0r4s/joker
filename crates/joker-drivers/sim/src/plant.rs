//! The simulated plant: fixed steps of virtual time (ref/03 §2.4.2), driven
//! by whatever clock the host gives (ADR 0007).
//!
//! Step `k` happens at `t0 + k · step`, where `t0` is the instant of the
//! first `connect`. Every random draw comes from a per-signal SplitMix64
//! stream seeded from the model's seed and the signal's name, and every
//! transcendental function comes from `libm`, so the same model, clock and
//! writes give the same bits on every architecture (ADR 0051, ADR 0054).

use std::boxed::Box;
use std::string::String;
use std::vec::Vec;

use joker_hal::types::{MonoNs, RawStatus, Reason};

use crate::engine::PlantInstance;
use crate::expr::{Env, Mem, StateKind, eval};
use crate::model::{Effect, Gen, Model, PanelKind, Signal, SimDirection, SimType, quality};

/// 64-bit FNV-1a, the default seed of a model (ref/03 §2.4.2).
pub fn fnv1a(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

const BOUNCE_SALT: u64 = 0xb0b0_ce00_b0b0_ce00;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn unit(&mut self) -> f64 {
        ((self.next() >> 11) as f64 + 1.0) / (1u64 << 53) as f64
    }

    fn gaussian(&mut self) -> f64 {
        let u1 = self.unit();
        let u2 = self.unit();
        (-2.0 * libm::log(u1)).sqrt() * libm::cos(2.0 * core::f64::consts::PI * u2)
    }
}

/// What one point reads now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Reading {
    /// The device is disconnected.
    Down,
    /// A value with its quality; `None` when the quality forbids a value.
    Value(Option<f64>, RawStatus),
}

/// The bounce of one change of a `bool` (ref/03 §2.4.4): an odd number of
/// edges at exact instants, the first at the change.
#[derive(Clone, Debug, PartialEq)]
struct Bounce {
    from: bool,
    edges: Vec<MonoNs>,
}

/// The plant of one `sim` device.
#[derive(Debug)]
pub struct Plant {
    model: Model,
    t0: Option<MonoNs>,
    step: u64,
    values: Vec<f64>,
    base: Vec<f64>,
    texts: Vec<Vec<u8>>,
    states: Vec<f64>,
    next_states: Vec<f64>,
    rings: Vec<(Vec<f64>, usize)>,
    mems: Vec<Mem>,
    rngs: Vec<SplitMix>,
    bounce_rngs: Vec<SplitMix>,
    bounces: Vec<Option<Bounce>>,
    overrides: Vec<Option<f64>>,
    offsets: Vec<f64>,
    momentary_until: Vec<Option<MonoNs>>,
    panel_faults: Vec<bool>,
    frozen: Vec<Option<f64>>,
    plant_out: Vec<Option<f64>>,
    wasm: Option<Box<dyn PlantInstance>>,
    wasm_failed: bool,
}

fn finish(s: &Signal, offset: f64, rng: &mut SplitMix, v: f64) -> f64 {
    let mut v = v + offset;
    if let Some(sd) = s.noise {
        v += sd * rng.gaussian();
    }
    if let Some(r) = s.resolution {
        v = (v / r).round_ties_even() * r;
    }
    v
}

impl Plant {
    /// A plant for `model`, with `seed` unless the model has its own.
    pub fn new(model: Model, seed: u64) -> Self {
        let seed = model.seed.unwrap_or(seed);
        Self::seeded(model, seed)
    }

    /// A plant for `model` with exactly `seed` (a scenario's override).
    pub fn seeded(model: Model, seed: u64) -> Self {
        let n = model.signals.len();
        let rngs = model
            .signals
            .iter()
            .map(|s| SplitMix(seed ^ fnv1a(&s.name)))
            .collect();
        let bounce_rngs = model
            .signals
            .iter()
            .map(|s| SplitMix(seed ^ fnv1a(&s.name) ^ BOUNCE_SALT))
            .collect();
        let values: Vec<f64> = model.signals.iter().map(|s| s.init).collect();
        let texts = model.signals.iter().map(|s| s.init_text.clone()).collect();
        let states: Vec<f64> = model
            .dynamics
            .states
            .iter()
            .map(|c| model.signals[c.owner].init)
            .collect();
        let rings = model
            .dynamics
            .states
            .iter()
            .map(|c| match c.kind {
                StateKind::Delay { steps } => (std::vec![model.signals[c.owner].init; steps], 0),
                _ => (Vec::new(), 0),
            })
            .collect();
        let mems = model.dynamics.mems.iter().map(|k| Mem::new(*k)).collect();
        Self {
            t0: None,
            step: 0,
            base: values.clone(),
            values,
            texts,
            next_states: states.clone(),
            states,
            rings,
            mems,
            rngs,
            bounce_rngs,
            bounces: std::vec![None; n],
            overrides: std::vec![None; n],
            offsets: std::vec![0.0; n],
            momentary_until: std::vec![None; n],
            panel_faults: std::vec![false; model.panel_faults.len()],
            frozen: std::vec![None; n],
            plant_out: std::vec![None; n],
            wasm: None,
            wasm_failed: false,
            model,
        }
    }

    /// Attaches the instance of the model's `[plant]` component.
    pub fn attach(&mut self, instance: Box<dyn PlantInstance>) {
        self.wasm = Some(instance);
    }

    /// The model.
    pub fn model(&self) -> &Model {
        &self.model
    }

    /// The step the plant is at.
    pub fn step(&self) -> u64 {
        self.step
    }

    /// Starts the plant at `now` if it has not started; computes step 0.
    pub fn start(&mut self, now: MonoNs) {
        if self.t0.is_none() {
            self.t0 = Some(now);
            self.compute(0);
        }
    }

    /// Runs every step up to `now`.
    pub fn advance_to(&mut self, now: MonoNs) {
        self.start(now);
        let t0 = self.t0.unwrap_or(now);
        let target = now.saturating_sub(t0) / self.model.step_ns;
        while self.step < target {
            self.update_states();
            self.step += 1;
            self.compute(self.step);
        }
    }

    fn mono(&self, step: u64) -> MonoNs {
        self.t0.unwrap_or(0) + step * self.model.step_ns
    }

    fn run_wasm(&mut self) {
        let Some(spec) = &self.model.plant else {
            return;
        };
        let Some(instance) = self.wasm.as_mut().filter(|_| !self.wasm_failed) else {
            self.wasm_failed = true;
            return;
        };
        let inputs: Vec<f64> = spec.inputs.iter().map(|&i| self.values[i]).collect();
        match instance.step(self.model.step_ns, &inputs) {
            Ok(out) if out.len() == spec.outputs.len() => {
                for (k, &i) in spec.outputs.iter().enumerate() {
                    self.plant_out[i] = Some(out[k]);
                }
            }
            _ => self.wasm_failed = true,
        }
    }

    fn compute(&mut self, step: u64) {
        let t_ns = step * self.model.step_ns;
        let t = t_ns as f64 / 1e9;
        let now = self.mono(step);
        if step > 0 {
            self.run_wasm();
        }
        let before: Vec<f64> = self.values.clone();
        for i in 0..self.model.signals.len() {
            let s = &self.model.signals[i];
            if s.direction == SimDirection::Output || s.bit_of.is_some() || s.ty.is_text() {
                continue;
            }
            if let Some(until) = self.momentary_until[i]
                && now >= until
            {
                self.momentary_until[i] = None;
                self.overrides[i] = None;
            }
            let generated = match &s.generator {
                Some(g) => Some(generate(g, t, t_ns, self.base[i], step, &mut self.rngs[i])),
                None => self.plant_out[i],
            };
            let base = self.overrides[i].or(generated);
            if let Some(b) = base {
                self.base[i] = b;
                self.values[i] = finish(s, self.offsets[i], &mut self.rngs[i], b);
            } else if !self.model.dynamics.entries.iter().any(|e| e.signal == i) {
                self.values[i] = finish(s, self.offsets[i], &mut self.rngs[i], s.init);
            }
        }
        for k in 0..self.model.dynamics.entries.len() {
            let entry = &self.model.dynamics.entries[k];
            let mut env = Env {
                values: &self.values,
                states: &self.states,
                mems: &mut self.mems,
                t_ns,
            };
            let v = eval(&entry.expr, &mut env);
            let i = entry.signal;
            let v = self.overrides[i].unwrap_or(v);
            self.base[i] = v;
            let s = &self.model.signals[i];
            self.values[i] = finish(s, self.offsets[i], &mut self.rngs[i], v);
        }
        for i in 0..self.model.signals.len() {
            if let Some((parent, bit)) = self.model.signals[i].bit_of {
                let word = self.values[parent] as i64 as u64;
                self.values[i] = f64::from(u8::from(word >> bit & 1 == 1));
            }
        }
        for (i, s) in self.model.signals.iter().enumerate() {
            let Some(bounce_ns) = s.bounce_ns else {
                continue;
            };
            if step == 0 || (before[i] != 0.0) == (self.values[i] != 0.0) {
                continue;
            }
            let rng = &mut self.bounce_rngs[i];
            let extra = 2 * (1 + rng.next() % 3) as usize;
            let mut offsets: Vec<u64> = (0..extra)
                .map(|_| (rng.unit() * bounce_ns as f64) as u64)
                .collect();
            offsets.sort_unstable();
            let mut edges = std::vec![now];
            edges.extend(offsets.iter().map(|o| now + (*o).max(1)));
            self.bounces[i] = Some(Bounce {
                from: before[i] != 0.0,
                edges,
            });
        }
    }

    fn update_states(&mut self) {
        let dt = self.model.step_ns as f64 / 1e9;
        let t_ns = self.step * self.model.step_ns;
        for k in 0..self.model.dynamics.states.len() {
            let call = &self.model.dynamics.states[k];
            let mut env = Env {
                values: &self.values,
                states: &self.states,
                mems: &mut self.mems,
                t_ns,
            };
            let u = eval(&call.arg, &mut env);
            let x = self.states[k];
            self.next_states[k] = match call.kind {
                StateKind::Integrate { min, max } => (x + u * dt).max(min).min(max),
                StateKind::Lag { tau } => x + (u - x) * dt / tau,
                StateKind::Delay { .. } => {
                    let (ring, head) = &mut self.rings[k];
                    ring[*head] = u;
                    *head = (*head + 1) % ring.len();
                    ring[*head]
                }
            };
        }
        core::mem::swap(&mut self.states, &mut self.next_states);
    }

    /// Writes the applied value of output `signal`.
    pub fn apply(&mut self, signal: usize, value: f64) {
        if let Some(v) = self.values.get_mut(signal) {
            *v = value;
        }
    }

    /// Writes the applied value of a `string` or `bytes` output.
    pub fn apply_text(&mut self, signal: usize, value: &[u8]) {
        if let Some(t) = self.texts.get_mut(signal) {
            t.clear();
            t.extend_from_slice(value);
        }
    }

    fn elapsed(&self, now: MonoNs) -> u64 {
        now.saturating_sub(self.t0.unwrap_or(now))
    }

    fn active(&self, now: MonoNs, signal: Option<usize>) -> Option<Effect> {
        let e = self.elapsed(now);
        let live = |f: &&crate::model::Fault| e >= f.at_ns && e < f.at_ns + f.for_ns;
        let own = self
            .model
            .faults
            .iter()
            .filter(live)
            .find(|f| f.signal == signal)
            .map(|f| f.effect);
        let device = || {
            signal.and_then(|i| {
                let acquired = self.model.signals[i].direction != SimDirection::Output;
                self.model
                    .faults
                    .iter()
                    .filter(live)
                    .find(|f| f.signal.is_none() && f.effect != Effect::Disconnect && acquired)
                    .map(|f| f.effect)
            })
        };
        own.or_else(device).or_else(|| {
            self.model
                .panel_faults
                .iter()
                .zip(&self.panel_faults)
                .filter(|(_, on)| **on)
                .find(|(f, _)| match (f.effect, signal) {
                    (Effect::Disconnect, None) => true,
                    (Effect::Disconnect, Some(_)) | (_, None) => false,
                    (_, Some(i)) => f.signals.contains(&i),
                })
                .map(|(f, _)| f.effect)
        })
    }

    /// True when the whole device is disconnected at `now`.
    pub fn disconnected(&self, now: MonoNs) -> bool {
        self.active(now, None) == Some(Effect::Disconnect)
    }

    /// True once the `[plant]` component failed (`bad(device_fault)`).
    pub fn plant_failed(&self) -> bool {
        self.wasm_failed
    }

    /// Extra latency of `signal` at `now`.
    pub fn delay_ns(&self, now: MonoNs, signal: usize) -> u64 {
        match self.active(now, Some(signal)) {
            Some(Effect::Delay(d)) => d,
            _ => 0,
        }
    }

    /// The latency of `signal`: its own, or the model's.
    pub fn latency_ns(&self, signal: usize) -> u64 {
        self.model
            .signals
            .get(signal)
            .and_then(|s| s.latency_ns)
            .unwrap_or(self.model.latency_ns)
    }

    fn delivered(&self, now: MonoNs, signal: usize, v: f64) -> (f64, Option<MonoNs>) {
        match self.bounces.get(signal).and_then(Option::as_ref) {
            Some(b) => {
                let n = b.edges.iter().filter(|e| **e <= now).count();
                let value = if n % 2 == 1 { !b.from } else { b.from };
                if n == b.edges.len() {
                    (v, b.edges.last().copied())
                } else {
                    (
                        f64::from(u8::from(value)),
                        n.checked_sub(1).map(|k| b.edges[k]),
                    )
                }
            }
            None => (v, None),
        }
    }

    /// The instant of the last bounce edge of `signal` at or before `now`,
    /// when it has bounced.
    pub fn edge_at(&self, now: MonoNs, signal: usize) -> Option<MonoNs> {
        let v = self.values.get(signal).copied().unwrap_or(0.0);
        self.delivered(now, signal, v).1
    }

    /// The current value of a `string` or `bytes` signal.
    pub fn text(&self, signal: usize) -> Option<&[u8]> {
        self.texts.get(signal).map(Vec::as_slice)
    }

    /// The plant's current value of `signal`, without faults or bounce.
    pub fn value(&self, signal: usize) -> Option<f64> {
        self.values.get(signal).copied()
    }

    /// What `signal` reads at `now`; call [`Plant::advance_to`] first.
    pub fn read(&mut self, now: MonoNs, signal: usize) -> Reading {
        if self.disconnected(now) {
            return Reading::Down;
        }
        if self.wasm_failed {
            return Reading::Value(None, RawStatus::bad(Reason::DEVICE_FAULT));
        }
        let raw = self.values.get(signal).copied().unwrap_or(f64::NAN);
        let (v, _) = self.delivered(now, signal, raw);
        let effect = self.active(now, Some(signal));
        if !matches!(effect, Some(Effect::Freeze | Effect::Quality(_))) {
            self.frozen[signal] = None;
        }
        let (value, status) = match effect {
            Some(Effect::Quality(q)) if q.code == joker_hal::QualityCode::Bad => {
                return Reading::Value(None, q);
            }
            Some(Effect::Quality(q)) => (*self.frozen[signal].get_or_insert(v), q),
            Some(Effect::Freeze) => (*self.frozen[signal].get_or_insert(v), RawStatus::OK),
            Some(Effect::Value(x)) => (x, RawStatus::OK),
            Some(Effect::Offset(o)) => (v + o, RawStatus::OK),
            _ => (v, RawStatus::OK),
        };
        let s = &self.model.signals[signal];
        if s.ty.is_numeric() && !value.is_finite() {
            let nan = quality("bad(nan)").unwrap_or(RawStatus::bad(Reason::DEVICE_FAULT));
            return Reading::Value(None, nan);
        }
        if let Some((lo, hi)) = s.limits
            && !(lo..=hi).contains(&value)
        {
            return Reading::Value(None, RawStatus::bad(Reason::OUT_OF_RANGE));
        }
        Reading::Value(Some(value), status)
    }

    /// Fixes an input with `value` until [`Plant::release`]: a scenario or
    /// action `set`, or a panel slider.
    pub fn set(&mut self, signal: usize, value: f64) {
        if let Some(o) = self.overrides.get_mut(signal) {
            *o = Some(value);
            self.momentary_until[signal] = None;
        }
    }

    /// Sets a `string` or `bytes` input.
    pub fn set_text(&mut self, signal: usize, value: &[u8]) {
        self.apply_text(signal, value);
    }

    /// Ends a [`Plant::set`]: the signal goes back to its generator, its
    /// dynamics or its `init`.
    pub fn release(&mut self, signal: usize) {
        if let Some(o) = self.overrides.get_mut(signal) {
            *o = None;
        }
    }

    /// The panel key `key` pressed at `now` (ref/03 §2.4.7–§2.4.8). Returns
    /// false when no control has that key.
    pub fn press(&mut self, key: &str, now: MonoNs) -> bool {
        if let Some(k) = self.model.panel_faults.iter().position(|f| f.key == key) {
            self.panel_faults[k] = !self.panel_faults[k];
            return true;
        }
        for i in 0..self.model.signals.len() {
            let s = &self.model.signals[i];
            let Some(p) = &s.panel else {
                continue;
            };
            let current = self.overrides[i].unwrap_or(self.base[i]);
            let dir = match &p.keys {
                Some((down, _)) if down == key => -1.0,
                Some((_, up)) if up == key => 1.0,
                _ => 0.0,
            };
            match p.kind {
                PanelKind::Toggle if p.key.as_deref() == Some(key) => {
                    self.overrides[i] = Some(if current != 0.0 { 0.0 } else { 1.0 });
                }
                PanelKind::Momentary if p.key.as_deref() == Some(key) => {
                    self.overrides[i] = Some(1.0);
                    self.momentary_until[i] = Some(now + p.pulse_ns);
                }
                PanelKind::Slider if dir != 0.0 => {
                    let lo = p.min.unwrap_or(f64::NEG_INFINITY);
                    let hi = p.max.unwrap_or(f64::INFINITY);
                    self.overrides[i] = Some((current + dir * p.step).max(lo).min(hi));
                }
                PanelKind::Gauge if dir != 0.0 => self.offsets[i] += dir * p.step,
                _ => continue,
            }
            return true;
        }
        false
    }

    /// Names of the panel fault keys currently on.
    pub fn faults_on(&self) -> Vec<String> {
        self.model
            .panel_faults
            .iter()
            .zip(&self.panel_faults)
            .filter(|(_, on)| **on)
            .map(|(f, _)| f.key.clone())
            .collect()
    }

    /// The type of `signal`.
    pub fn ty(&self, signal: usize) -> Option<SimType> {
        self.model.signals.get(signal).map(|s| s.ty)
    }
}

fn generate(g: &Gen, t: f64, t_ns: u64, prev: f64, step: u64, rng: &mut SplitMix) -> f64 {
    match g {
        Gen::Constant(v) => *v,
        Gen::Step { from, to, at } => {
            if t < *at {
                *from
            } else {
                *to
            }
        }
        Gen::Ramp { from, to, over, at } => {
            if t <= *at {
                *from
            } else if *over <= 0.0 || t >= at + over {
                *to
            } else {
                from + (to - from) * (t - at) / over
            }
        }
        Gen::Sine {
            mean,
            amp,
            period,
            phase,
        } => {
            let rad = phase * (core::f64::consts::PI / 180.0);
            mean + amp * libm::sin(2.0 * core::f64::consts::PI * t / period + rad)
        }
        Gen::Square {
            low,
            high,
            period,
            duty,
        } => {
            let frac = (t / period) - libm::floor(t / period);
            if frac < *duty { *high } else { *low }
        }
        Gen::Noise { mean, std } => mean + std * rng.gaussian(),
        Gen::Walk {
            start,
            std,
            min,
            max,
        } => {
            if step == 0 {
                *start
            } else {
                (prev + std * rng.gaussian()).max(*min).min(*max)
            }
        }
        Gen::Replay(series) => series.at(t_ns),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Inputs, parse, parse_with};
    use std::collections::BTreeMap;

    const PI: &str = include_str!("../tests/models/pi.sim.toml");
    const ESP: &str = include_str!("../tests/models/esp.sim.toml");
    const MS: u64 = 1_000_000;
    const S: u64 = 1_000 * MS;

    fn plant(src: &str) -> Plant {
        Plant::new(
            parse(src, &BTreeMap::new()).unwrap_or_else(|e| panic!("{e}")),
            0,
        )
    }

    fn value(p: &mut Plant, t: u64, i: usize) -> f64 {
        p.advance_to(t);
        match p.read(t, i) {
            Reading::Value(Some(v), _) => v,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn first_order_lag_reaches_63_percent_in_one_tau() {
        let model = parse(PI, &BTreeMap::new()).unwrap();
        let load = model.signal("load").unwrap();
        let temp = model.signal("cpu_temp").unwrap();
        let mut p = Plant::new(model, fnv1a("pi"));
        p.start(5 * MS);
        assert_eq!(
            p.read(5 * MS, temp),
            Reading::Value(Some(45.0), RawStatus::OK)
        );
        p.set(load, 1.0);
        let t = value(&mut p, 5 * MS + 60 * S, temp);
        assert!(
            (t - (45.0 + 30.0 * (1.0 - (-1.0f64).exp()))).abs() < 0.01,
            "{t}"
        );
        assert!(p.press("left", 0));
        assert!(p.press("right", 0));
        assert!(!p.press("x", 0));
    }

    #[test]
    fn same_model_same_clock_same_bits() {
        let run = || {
            let model = parse(ESP, &BTreeMap::new()).unwrap();
            let names = ["rssi", "touch", "uptime"];
            let ids: Vec<usize> = names.iter().map(|n| model.signal(n).unwrap()).collect();
            let mut p = Plant::new(model, fnv1a("esp"));
            let mut out = Vec::new();
            for k in 0..500u64 {
                let now = k * 7 * MS;
                p.advance_to(now);
                for &i in &ids {
                    if let Reading::Value(Some(v), _) = p.read(now, i) {
                        out.push(v.to_bits());
                    }
                }
            }
            out
        };
        let a = run();
        assert_eq!(a, run());
        assert_eq!(a.len(), 1500);
    }

    #[test]
    fn panel_controls_and_disconnect() {
        let model = parse(ESP, &BTreeMap::new()).unwrap();
        let button = model.signal("button").unwrap();
        let touch = model.signal("touch").unwrap();
        let led = model.signal("led").unwrap();
        let led_rb = model.signal("led_rb").unwrap();
        let uptime = model.signal("uptime").unwrap();
        let mut p = Plant::new(model, 1);
        p.start(0);
        assert!(p.press("b", 0));
        p.advance_to(20 * MS);
        assert_eq!(
            p.read(20 * MS, button),
            Reading::Value(Some(1.0), RawStatus::OK)
        );
        p.advance_to(200 * MS);
        assert_eq!(
            p.read(200 * MS, button),
            Reading::Value(Some(0.0), RawStatus::OK)
        );
        p.press("down", 200 * MS);
        let t = value(&mut p, 300 * MS, touch);
        assert!((52.0..=58.0).contains(&t), "{t}");
        p.apply(led, 1.0);
        assert_eq!(value(&mut p, 310 * MS, led_rb), 1.0);
        let u = value(&mut p, S, uptime);
        assert!((u - 1.0).abs() < 1e-9, "{u}");
        assert!(p.press("d", S));
        assert!(p.disconnected(S));
        assert_eq!(p.read(S, uptime), Reading::Down);
        assert_eq!(p.faults_on(), ["d"]);
        p.press("d", S);
        assert!(!p.disconnected(S));
    }

    #[test]
    fn a_bool_with_bounce_bounces_and_settles() {
        let model = parse(ESP, &BTreeMap::new()).unwrap();
        let button = model.signal("button").unwrap();
        let mut p = Plant::new(model, 7);
        p.start(0);
        p.advance_to(100 * MS);
        assert!(p.press("b", 100 * MS));
        p.advance_to(110 * MS);
        let seen: Vec<f64> = (0..=60)
            .map(|k| {
                let t = 110 * MS + k * 100_000;
                match p.read(t, button) {
                    Reading::Value(Some(v), _) => v,
                    other => panic!("{other:?}"),
                }
            })
            .collect();
        assert!(
            seen.windows(2).filter(|w| w[0] != w[1]).count() >= 2,
            "{seen:?}"
        );
        assert_eq!(seen.last(), Some(&1.0));
        assert_eq!(p.edge_at(110 * MS, button), Some(110 * MS));
        assert!(
            p.edge_at(116 * MS, button)
                .is_some_and(|e| e > 110 * MS && e < 115 * MS)
        );
    }

    #[test]
    fn every_generator() {
        let src = r#"schema = 1
[model]
seed = 42
[signals]
c = { type = "f64", gen = { kind = "constant", value = 3 } }
st = { type = "f64", gen = { kind = "step", from = 1, to = 2, at = "1s" } }
r = { type = "f64", gen = { kind = "ramp", from = 0, to = 10, over = "10s", at = "1s" } }
si = { type = "f64", unit = "°C", gen = { kind = "sine", mean = "20 °C", amp = "2 K", period = "4s", phase = 90 } }
sq = { type = "bool", gen = { kind = "square", low = false, high = true, period = "1s", duty = 0.25 } }
n = { type = "f64", gen = { kind = "noise", mean = 5, std = 0 } }
w = { type = "f64", gen = { kind = "walk", start = 0, std = 1, min = -0.5, max = 0.5 } }
"#;
        let mut p = plant(src);
        let m = p.model().clone();
        let id = |n: &str| m.signal(n).unwrap();
        p.start(0);
        assert_eq!(value(&mut p, 0, id("c")), 3.0);
        assert_eq!(value(&mut p, 990 * MS, id("st")), 1.0);
        assert_eq!(value(&mut p, S, id("st")), 2.0);
        assert_eq!(value(&mut p, 6 * S, id("r")), 5.0);
        assert!((value(&mut p, 8 * S, id("si")) - 22.0).abs() < 1e-9);
        assert_eq!(value(&mut p, 8 * S + 200 * MS, id("sq")), 1.0);
        assert_eq!(value(&mut p, 8 * S + 300 * MS, id("sq")), 0.0);
        assert_eq!(value(&mut p, 9 * S, id("n")), 5.0);
        let w = value(&mut p, 10 * S, id("w"));
        assert!((-0.5..=0.5).contains(&w));
    }

    #[test]
    fn replay_follows_the_recording() {
        let src = "schema = 1\n[signals]\np = { type = \"f64\", gen = { kind = \"replay\", file = \"r.csv\", column = \"p\", loop = true } }\n";
        let mut inputs = Inputs::default();
        inputs
            .files
            .insert("r.csv".into(), b"t,p\n0,1\n1,2\n2,3\n".to_vec());
        let mut p = Plant::new(parse_with(src, &inputs).unwrap(), 0);
        p.start(0);
        let got: Vec<f64> = [0, 1, 2, 3, 4, 5]
            .iter()
            .map(|s| value(&mut p, s * S, 0))
            .collect();
        assert_eq!(got, [1.0, 2.0, 3.0, 1.0, 2.0, 3.0]);
    }

    #[test]
    fn scheduled_faults() {
        let src = r#"schema = 1
[signals]
x = { type = "f64", gen = { kind = "ramp", from = 0, to = 10, over = "10s" } }
[faults]
x = [{ at = "1s", for = "1s", quality = "bad(device_fault)" }, { at = "3s", for = "1s", freeze = true }, { at = "5s", for = "1s", value = 42 }, { at = "6s", for = "1s", offset = 100 }, { at = "7s", for = "1s", delay = "50ms" }]
"*" = [{ at = "9s", for = "1s", disconnect = true }, { at = "11s", for = "1s", quality = "Uncertain(Stale)" }]
"#;
        let mut p = plant(src);
        p.start(0);
        let mut at = |t: u64| {
            p.advance_to(t);
            p.read(t, 0)
        };
        assert_eq!(at(S / 2), Reading::Value(Some(0.5), RawStatus::OK));
        assert_eq!(
            at(S + S / 2),
            Reading::Value(None, RawStatus::bad(Reason::DEVICE_FAULT))
        );
        assert_eq!(at(3 * S), Reading::Value(Some(3.0), RawStatus::OK));
        assert_eq!(at(3 * S + S / 2), Reading::Value(Some(3.0), RawStatus::OK));
        assert_eq!(at(5 * S), Reading::Value(Some(42.0), RawStatus::OK));
        assert_eq!(at(6 * S), Reading::Value(Some(106.0), RawStatus::OK));
        assert_eq!(at(9 * S), Reading::Down);
        assert_eq!(at(10 * S), Reading::Value(Some(10.0), RawStatus::OK));
        assert_eq!(
            at(11 * S),
            Reading::Value(Some(10.0), RawStatus::uncertain(Reason::STALE))
        );
        assert_eq!(p.delay_ns(7 * S, 0), 50 * MS);
    }

    #[test]
    fn an_uncertain_fault_keeps_the_last_good_value() {
        let src = r#"schema = 1
[signals]
x = { type = "f64", gen = { kind = "ramp", from = 0, to = 10, over = "10s" } }
[faults]
x = [{ at = "2s", for = "2s", quality = "uncertain(stale)" }]
"#;
        let mut p = plant(src);
        p.start(0);
        let stale = RawStatus::uncertain(Reason::STALE);
        p.advance_to(2 * S);
        assert_eq!(p.read(2 * S, 0), Reading::Value(Some(2.0), stale));
        p.advance_to(3 * S);
        assert_eq!(p.read(3 * S, 0), Reading::Value(Some(2.0), stale));
        p.advance_to(5 * S);
        assert_eq!(p.read(5 * S, 0), Reading::Value(Some(5.0), RawStatus::OK));
    }

    #[test]
    fn limits_give_out_of_range() {
        let src = "schema = 1\n[signals]\nx = { type = \"f64\", limits = [0, 5], gen = { kind = \"ramp\", from = 0, to = 10, over = \"10s\" } }\n";
        let mut p = plant(src);
        p.start(0);
        assert_eq!(value(&mut p, 4 * S, 0), 4.0);
        p.advance_to(6 * S);
        assert_eq!(
            p.read(6 * S, 0),
            Reading::Value(None, RawStatus::bad(Reason::OUT_OF_RANGE))
        );
    }

    #[test]
    fn delay_integrate_and_named_bits() {
        let src = r#"schema = 1
[model]
step = "100ms"
[signals]
u = { type = "f64", gen = { kind = "step", from = 0, to = 1, at = "1s" } }
d = { type = "f64", init = 0 }
i = { type = "f64", unit = "s", init = 0 }
[dynamics]
d = "delay(u, 500ms)"
i = "integrate(u, max = 0.25)"
"#;
        let mut p = plant(src);
        let m = p.model().clone();
        p.start(0);
        let (d, i) = (m.signal("d").unwrap(), m.signal("i").unwrap());
        assert_eq!(value(&mut p, 1_400 * MS, d), 0.0);
        assert_eq!(value(&mut p, 1_500 * MS, d), 1.0);
        assert_eq!(value(&mut p, 5 * S, i), 0.25);
    }

    #[test]
    fn device_wide_value_faults_apply_to_inputs_only() {
        let src = r#"schema = 1
[signals]
a = { type = "f64", init = 1 }
b = { type = "f64", init = 2 }
o = { type = "f64", direction = "output" }
[faults]
"*" = [{ at = "1s", for = "1s", freeze = true }]
"#;
        let mut p = plant(src);
        p.start(0);
        assert_eq!(p.active(S, Some(0)), Some(Effect::Freeze));
        assert_eq!(p.active(S, Some(2)), None);
    }
}
