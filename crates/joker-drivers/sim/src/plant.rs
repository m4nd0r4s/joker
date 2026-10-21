//! The simulated plant: fixed steps of virtual time (ref/03 §2.4.2), driven
//! by whatever clock the host gives (ADR 0007).
//!
//! Step `k` happens at `t0 + k · step`, where `t0` is the instant of the
//! first `connect`. Every random draw comes from a per-signal SplitMix64
//! stream seeded from the model's seed and the signal's name, and every
//! transcendental function comes from `libm`, so the same model, clock and
//! writes give the same bits on every architecture.

use std::string::String;
use std::vec::Vec;

use joker_hal::types::{MonoNs, RawStatus};

use crate::expr::{StateKind, eval};
use crate::model::{Effect, Gen, Model, PanelKind, SimDirection, SimType};

/// 64-bit FNV-1a, the default seed of a model (ref/03 §2.4.2).
pub fn fnv1a(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

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

/// The plant of one `sim` device.
#[derive(Clone, Debug)]
pub struct Plant {
    model: Model,
    t0: Option<MonoNs>,
    step: u64,
    values: Vec<f64>,
    base: Vec<f64>,
    states: Vec<f64>,
    rings: Vec<(Vec<f64>, usize)>,
    rngs: Vec<SplitMix>,
    overrides: Vec<Option<f64>>,
    offsets: Vec<f64>,
    momentary_until: Vec<Option<MonoNs>>,
    panel_faults: Vec<bool>,
    frozen: Vec<Option<f64>>,
}

impl Plant {
    /// A plant for `model`, with `seed` unless the model has its own.
    pub fn new(model: Model, seed: u64) -> Self {
        let seed = model.seed.unwrap_or(seed);
        let n = model.signals.len();
        let rngs = model
            .signals
            .iter()
            .map(|s| SplitMix(seed ^ fnv1a(&s.name)))
            .collect();
        let values: Vec<f64> = model.signals.iter().map(|s| s.init).collect();
        let states = model
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
                StateKind::Delay { steps } => {
                    (std::vec![model.signals[c.owner].init; steps.max(1)], 0)
                }
                _ => (Vec::new(), 0),
            })
            .collect();
        let panel_faults = std::vec![false; model.panel_faults.len()];
        Self {
            t0: None,
            step: 0,
            base: values.clone(),
            values,
            states,
            rings,
            rngs,
            overrides: std::vec![None; n],
            offsets: std::vec![0.0; n],
            momentary_until: std::vec![None; n],
            panel_faults,
            frozen: std::vec![None; n],
            model,
        }
    }

    /// The model.
    pub fn model(&self) -> &Model {
        &self.model
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

    fn time_s(&self, step: u64) -> f64 {
        (step as f64) * (self.model.step_ns as f64) / 1e9
    }

    fn mono(&self, step: u64) -> MonoNs {
        self.t0.unwrap_or(0) + step * self.model.step_ns
    }

    fn finish(&mut self, i: usize, v: f64) -> f64 {
        let s = &self.model.signals[i];
        let mut v = v + self.offsets[i];
        if let Some(sd) = s.noise {
            v += sd * self.rngs[i].gaussian();
        }
        if let Some(r) = s.resolution {
            v = (v / r).round_ties_even() * r;
        }
        v
    }

    fn compute(&mut self, step: u64) {
        let t = self.time_s(step);
        let now = self.mono(step);
        for i in 0..self.model.signals.len() {
            let s = &self.model.signals[i];
            if s.direction == SimDirection::Output {
                continue;
            }
            if let Some(until) = self.momentary_until[i]
                && now >= until
            {
                self.momentary_until[i] = None;
                self.overrides[i] = None;
            }
            let generated = match &s.generator {
                Some(g) => {
                    let prev = self.base[i];
                    let rng = &mut self.rngs[i];
                    Some(generate(g, t, prev, step, rng))
                }
                None => None,
            };
            let base = self.overrides[i].or(generated);
            if let Some(b) = base {
                self.base[i] = b;
                self.values[i] = self.finish(i, b);
            } else if !self.has_dynamics(i) {
                self.values[i] = self.finish(i, s.init);
            }
        }
        for k in 0..self.model.dynamics.entries.len() {
            let entry = &self.model.dynamics.entries[k];
            let i = entry.signal;
            let v = match self.overrides[i] {
                Some(o) => o,
                None => eval(&entry.expr, &self.values, &self.states),
            };
            self.values[i] = self.finish(i, v);
        }
    }

    fn has_dynamics(&self, i: usize) -> bool {
        self.model.dynamics.entries.iter().any(|e| e.signal == i)
    }

    fn update_states(&mut self) {
        let dt = self.model.step_ns as f64 / 1e9;
        for k in 0..self.model.dynamics.states.len() {
            let call = &self.model.dynamics.states[k];
            let u = eval(&call.arg, &self.values, &self.states);
            let x = self.states[k];
            self.states[k] = match call.kind {
                StateKind::Integrate { min, max } => (x + u * dt).max(min).min(max),
                StateKind::Lag { tau } => x + (u - x) * dt / tau,
                StateKind::Delay { steps } => {
                    let (ring, head) = &mut self.rings[k];
                    if steps == 0 {
                        u
                    } else {
                        let out = ring[*head];
                        ring[*head] = u;
                        *head = (*head + 1) % ring.len();
                        out
                    }
                }
            };
        }
    }

    /// Writes the applied value of output `signal`.
    pub fn apply(&mut self, signal: usize, value: f64) {
        if let Some(v) = self.values.get_mut(signal) {
            *v = value;
        }
    }

    fn elapsed(&self, now: MonoNs) -> u64 {
        now.saturating_sub(self.t0.unwrap_or(now))
    }

    fn active(&self, now: MonoNs, signal: Option<usize>) -> Option<Effect> {
        let e = self.elapsed(now);
        let scheduled = self
            .model
            .faults
            .iter()
            .find(|f| f.signal == signal && e >= f.at_ns && e < f.at_ns + f.for_ns)
            .map(|f| f.effect);
        scheduled.or_else(|| {
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

    /// Extra latency of `signal` at `now`.
    pub fn delay_ns(&self, now: MonoNs, signal: usize) -> u64 {
        match self.active(now, Some(signal)) {
            Some(Effect::Delay(d)) => d,
            _ => 0,
        }
    }

    /// What `signal` reads at `now`; call [`Plant::advance_to`] first.
    pub fn read(&mut self, now: MonoNs, signal: usize) -> Reading {
        if self.disconnected(now) {
            return Reading::Down;
        }
        let v = self.values.get(signal).copied().unwrap_or(f64::NAN);
        let effect = self.active(now, Some(signal));
        if !matches!(effect, Some(Effect::Freeze | Effect::Quality(_))) {
            self.frozen[signal] = None;
        }
        match effect {
            Some(Effect::Quality(q)) if q.code == joker_hal::QualityCode::Bad => {
                Reading::Value(None, q)
            }
            Some(Effect::Quality(q)) => {
                Reading::Value(Some(*self.frozen[signal].get_or_insert(v)), q)
            }
            Some(Effect::Freeze) => {
                Reading::Value(Some(*self.frozen[signal].get_or_insert(v)), RawStatus::OK)
            }
            Some(Effect::Value(x)) => Reading::Value(Some(x), RawStatus::OK),
            Some(Effect::Offset(o)) => Reading::Value(Some(v + o), RawStatus::OK),
            _ => Reading::Value(Some(v), RawStatus::OK),
        }
    }

    /// Overrides an input with `value` until [`Plant::release`]: a scenario
    /// `set`, or a panel slider.
    pub fn set(&mut self, signal: usize, value: f64) {
        if let Some(o) = self.overrides.get_mut(signal) {
            *o = Some(value);
            self.momentary_until[signal] = None;
        }
    }

    /// Ends an override.
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

fn generate(g: &Gen, t: f64, prev: f64, step: u64, rng: &mut SplitMix) -> f64 {
    match *g {
        Gen::Constant(v) => v,
        Gen::Step { from, to, at } => {
            if t < at {
                from
            } else {
                to
            }
        }
        Gen::Ramp { from, to, over, at } => {
            if t <= at {
                from
            } else if over <= 0.0 || t >= at + over {
                to
            } else {
                from + (to - from) * (t - at) / over
            }
        }
        Gen::Sine {
            mean,
            amp,
            period,
            phase,
        } => mean + amp * libm::sin(2.0 * core::f64::consts::PI * t / period + phase.to_radians()),
        Gen::Square {
            low,
            high,
            period,
            duty,
        } => {
            let frac = (t / period) - libm::floor(t / period);
            if frac < duty { high } else { low }
        }
        Gen::Noise { mean, std } => mean + std * rng.gaussian(),
        Gen::Walk {
            start,
            std,
            min,
            max,
        } => {
            if step == 0 {
                start
            } else {
                (prev + std * rng.gaussian()).max(min).min(max)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::parse;
    use std::collections::BTreeMap;

    const PI: &str = include_str!("../tests/models/pi.sim.toml");
    const ESP: &str = include_str!("../tests/models/esp.sim.toml");
    const MS: u64 = 1_000_000;

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
        p.advance_to(5 * MS + 60_000 * MS);
        let Reading::Value(Some(t), _) = p.read(60_005 * MS, temp) else {
            panic!("no value");
        };
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
        p.advance_to(10 * MS);
        assert_eq!(
            p.read(10 * MS, button),
            Reading::Value(Some(1.0), RawStatus::OK)
        );
        p.advance_to(200 * MS);
        assert_eq!(
            p.read(200 * MS, button),
            Reading::Value(Some(0.0), RawStatus::OK)
        );
        p.press("down", 200 * MS);
        p.advance_to(300 * MS);
        let Reading::Value(Some(t), _) = p.read(300 * MS, touch) else {
            panic!("no value");
        };
        assert!((52.0..=58.0).contains(&t), "{t}");
        p.apply(led, 1.0);
        p.advance_to(310 * MS);
        assert_eq!(
            p.read(310 * MS, led_rb),
            Reading::Value(Some(1.0), RawStatus::OK)
        );
        p.advance_to(1_000 * MS);
        let Reading::Value(Some(u), _) = p.read(1_000 * MS, uptime) else {
            panic!("no value");
        };
        assert!((u - 1.0).abs() < 1e-9, "{u}");
        assert!(p.press("d", 1_000 * MS));
        assert!(p.disconnected(1_000 * MS));
        assert_eq!(p.read(1_000 * MS, uptime), Reading::Down);
        assert_eq!(p.faults_on(), ["d"]);
        p.press("d", 1_000 * MS);
        assert!(!p.disconnected(1_000 * MS));
    }

    #[test]
    fn scheduled_faults() {
        let src = r#"schema = 1
[signals]
x = { type = "f64", gen = { kind = "ramp", from = 0, to = 10, over = "10s" } }
[faults]
x = [{ at = "1s", for = "1s", quality = "bad(device_fault)" }, { at = "3s", for = "1s", freeze = true }, { at = "5s", for = "1s", value = 42 }, { at = "7s", for = "1s", delay = "50ms" }]
"*" = [{ at = "9s", for = "1s", disconnect = true }]
"#;
        let model = parse(src, &BTreeMap::new()).unwrap();
        let mut p = Plant::new(model, 0);
        p.start(0);
        let s = 1_000 * MS;
        let mut at = |t: u64| {
            p.advance_to(t);
            p.read(t, 0)
        };
        assert_eq!(at(s / 2), Reading::Value(Some(0.5), RawStatus::OK));
        assert_eq!(
            at(s + s / 2),
            Reading::Value(None, RawStatus::bad(joker_hal::Reason::DEVICE_FAULT))
        );
        assert_eq!(at(3 * s), Reading::Value(Some(3.0), RawStatus::OK));
        assert_eq!(at(3 * s + s / 2), Reading::Value(Some(3.0), RawStatus::OK));
        assert_eq!(at(5 * s), Reading::Value(Some(42.0), RawStatus::OK));
        assert_eq!(at(9 * s), Reading::Down);
        assert_eq!(at(10 * s), Reading::Value(Some(10.0), RawStatus::OK));
        assert_eq!(p.delay_ns(7 * s, 0), 50 * MS);
    }

    #[test]
    fn an_uncertain_fault_keeps_the_last_good_value() {
        let src = r#"schema = 1
[signals]
x = { type = "f64", gen = { kind = "ramp", from = 0, to = 10, over = "10s" } }
[faults]
x = [{ at = "2s", for = "2s", quality = "uncertain(stale)" }]
"#;
        let mut p = Plant::new(parse(src, &BTreeMap::new()).unwrap(), 0);
        p.start(0);
        let stale = RawStatus::uncertain(joker_hal::Reason::STALE);
        let s = 1_000 * MS;
        p.advance_to(2 * s);
        assert_eq!(p.read(2 * s, 0), Reading::Value(Some(2.0), stale));
        p.advance_to(3 * s);
        assert_eq!(p.read(3 * s, 0), Reading::Value(Some(2.0), stale));
        p.advance_to(5 * s);
        assert_eq!(p.read(5 * s, 0), Reading::Value(Some(5.0), RawStatus::OK));
    }
}
