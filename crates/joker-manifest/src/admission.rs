//! Real-time admission (M23, ADR 0048), the memory estimate (`J0380`,
//! ADR 0048) and the bus budget (`J0370`, ADR 0049).

use std::collections::{BTreeMap, BTreeSet};

use joker_units::Duration;

use crate::check::Checker;
use crate::context::{Batch, Tier};
use crate::diag::Diagnostic;
use crate::model::{Bus, BusKind};
use crate::value::{Node, Value};

/// The margin of document 01 §4: every cost counts 1.3 times.
pub const MARGIN: f64 = 1.3;
/// The fixed core work of one tick (ADR 0048).
pub const TICK_BASE_NS: f64 = 20_000.0;
/// The latch cost of one signal.
pub const LATCH_NS: f64 = 10.0;
/// The arbiter and envelope cost of one owned output.
pub const OUTPUT_NS: f64 = 1_000.0;
/// The share of RAM the estimate may use.
pub const MEMORY_SHARE: f64 = 0.7;

const MIB: u64 = 1 << 20;

fn call_overhead_ns(tier: Tier) -> f64 {
    match tier {
        Tier::Rust => 5_000.0,
        Tier::TypeScript => 200_000.0,
        Tier::Python => 1_000_000.0,
    }
}

/// One cycle group as the admission sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    /// The name.
    pub name: String,
    /// The period, in ns.
    pub period: f64,
    /// The cost per tick, margin included, in ns.
    pub cost: f64,
    /// The explicit `SCHED_FIFO` priority, if declared.
    pub priority: Option<i64>,
}

/// The utilisation of a set of groups.
pub fn utilisation(groups: &[&Group]) -> f64 {
    groups.iter().map(|g| g.cost / g.period).sum()
}

/// The Liu & Layland bound for `n` groups.
pub fn bound(n: usize) -> f64 {
    let n = n as f64;
    n * (2f64.powf(1.0 / n) - 1.0)
}

/// Response-time analysis, groups in rate-monotonic order: the first group
/// whose response exceeds its period, with that response (ADR 0048, 3).
pub fn first_miss<'g>(groups: &[&'g Group]) -> Option<(&'g Group, f64)> {
    for (i, group) in groups.iter().enumerate() {
        let higher = &groups[..i];
        let mut response = group.cost;
        loop {
            let next = group.cost
                + higher
                    .iter()
                    .map(|h| (response / h.period).ceil() * h.cost)
                    .sum::<f64>();
            if next > group.period {
                return Some((group, next));
            }
            if (next - response).abs() < f64::EPSILON {
                break;
            }
            response = next;
        }
    }
    None
}

/// Whether a CPU's groups are schedulable: the bound first, then RTA.
pub fn schedulable(groups: &[&Group]) -> bool {
    groups.is_empty() || utilisation(groups) <= bound(groups.len()) || first_miss(groups).is_none()
}

fn rm_order(groups: &mut [&Group]) {
    groups.sort_by(|a, b| {
        a.period
            .partial_cmp(&b.period)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name.cmp(&b.name))
    });
}

fn ms(ns: f64) -> String {
    format!("{:.3} ms", ns / 1e6)
}

impl Checker<'_> {
    fn groups(&self) -> Vec<Group> {
        let manifest = self.manifest;
        let mut groups = Vec::new();
        for (name, cycle) in &manifest.cycles {
            let Some(period) = &cycle.period else {
                continue;
            };
            let period = period.value.as_nanos() as f64;
            let mut cost = TICK_BASE_NS;
            let mut latched = BTreeSet::new();
            for (app_name, app) in &manifest.apps {
                let interface = self.context.apps.get(app_name);
                let tier = interface.map_or(Tier::Rust, |i| i.tier);
                let tasks: Vec<&String> = app
                    .cycles
                    .iter()
                    .filter(|(_, g)| g.value == *name)
                    .map(|(t, _)| t)
                    .collect();
                for task in &tasks {
                    let key = format!("{app_name}.{task}");
                    let measured = self.context.measured.get(&key).map(|m| match tier {
                        Tier::Rust => m.p999,
                        Tier::TypeScript | Tier::Python => m.max,
                    });
                    let declared = interface.and_then(|i| i.wcet.get(*task)).copied();
                    let wcet = measured
                        .or(declared)
                        .map_or(0.2 * period, |d| d.as_nanos() as f64);
                    cost += wcet + call_overhead_ns(tier);
                }
                if !tasks.is_empty() {
                    for binding in app.bind.values() {
                        if let crate::model::Binding::Target(t) = &binding.value {
                            latched.insert(t.value.clone());
                        }
                    }
                }
            }
            let owned = manifest
                .outputs
                .values()
                .filter(|e| e.group.as_ref().is_some_and(|g| g.value == *name))
                .count();
            cost += LATCH_NS * latched.len() as f64 + OUTPUT_NS * owned as f64;
            groups.push(Group {
                name: name.clone(),
                period,
                cost: cost * MARGIN,
                priority: cycle.priority.as_ref().map(|p| p.value),
            });
        }
        groups
    }

    fn rt_cpus(&self) -> Vec<i64> {
        self.manifest
            .pending
            .get("resources")
            .and_then(Node::as_table)
            .and_then(|r| r.get("rt"))
            .and_then(|e| e.node.as_table())
            .and_then(|t| t.get("cpus"))
            .map(|e| match &e.node.value {
                Value::Array(items) => {
                    let set: BTreeSet<i64> = items
                        .iter()
                        .filter_map(|i| match i.value {
                            Value::Integer(n) => Some(n),
                            _ => None,
                        })
                        .collect();
                    set.into_iter().collect()
                }
                _ => Vec::new(),
            })
            .unwrap_or_default()
    }

    /// The real-time admission of ADR 0048 (`J0350`, `J0283`).
    pub(crate) fn admission(&mut self) {
        let groups = self.groups();
        let cpus = self.rt_cpus();
        let virtual_cpu = [-1i64];
        let cpus: &[i64] = if cpus.is_empty() { &virtual_cpu } else { &cpus };
        let mut placed: BTreeMap<i64, Vec<&Group>> =
            cpus.iter().map(|c| (*c, Vec::new())).collect();
        let mut order: Vec<&Group> = groups.iter().collect();
        rm_order(&mut order);
        for group in order {
            let explicit = self
                .manifest
                .cycles
                .get(&group.name)
                .and_then(|c| c.cpu.as_ref())
                .map(|c| c.value);
            let cpu = match explicit {
                Some(cpu) if placed.contains_key(&cpu) => cpu,
                Some(_) => continue,
                None => *placed
                    .iter()
                    .min_by(|a, b| {
                        utilisation(a.1)
                            .partial_cmp(&utilisation(b.1))
                            .unwrap_or(std::cmp::Ordering::Equal)
                            .then(a.0.cmp(b.0))
                    })
                    .map(|(c, _)| c)
                    .unwrap_or(&-1),
            };
            placed.entry(cpu).or_default().push(group);
        }
        for (cpu, mut list) in placed {
            rm_order(&mut list);
            let where_ = if cpu < 0 {
                "the shared CPU".to_owned()
            } else {
                format!("rt CPU {cpu}")
            };
            if !schedulable(&list) {
                let u = utilisation(&list);
                let (group, response) = first_miss(&list).map_or((list[0], 0.0), |(g, r)| (g, r));
                let span = self.manifest.cycles.get(&group.name).map(|c| c.key);
                let mut d = Diagnostic::new("J0350", format!("cycle groups on {where_} are not schedulable"))
                    .label(format!(
                        "utilisation {:.0}% with the 30% margin; group `{}` responds in {} for a period of {}",
                        u * 100.0,
                        group.name,
                        ms(response),
                        ms(group.period)
                    ))
                    .help("measure the tasks with `jk bench`, lengthen the periods, or spread the groups over more `rt` CPUs");
                if let Some(span) = span {
                    d = d.at(span);
                }
                self.push(d);
            }
            for (i, a) in list.iter().enumerate() {
                for b in &list[i + 1..] {
                    if let (Some(pa), Some(pb)) = (a.priority, b.priority)
                        && a.period < b.period
                        && pa < pb
                        && let Some(span) = self
                            .manifest
                            .cycles
                            .get(&b.name)
                            .and_then(|c| c.priority.as_ref())
                            .map(|p| p.span)
                    {
                        self.push(
                            Diagnostic::new("J0283", format!("`priority` of `{}` inverts the rate-monotonic order on {where_}", b.name))
                                .at(span)
                                .label(format!("`{}` has a shorter period and a lower priority", a.name)),
                        );
                    }
                }
            }
        }
    }

    /// The memory estimate of ADR 0048 (`J0380`).
    pub(crate) fn memory_estimate(&mut self) {
        let manifest = self.manifest;
        let Some(board) = manifest.target.as_ref().and_then(|t| t.board.as_ref()) else {
            return;
        };
        let Some(ram) = self
            .context
            .boards
            .get(&board.value.name)
            .and_then(|b| b.memory)
        else {
            return;
        };
        let mut total: u64 = 40 * MIB;
        for (name, app) in &manifest.apps {
            let interface = self.context.apps.get(name);
            let defaults = match interface.map_or(Tier::Rust, |i| i.tier) {
                Tier::Rust => (16 * MIB, 64 * MIB),
                Tier::TypeScript => (64 * MIB, 64 * MIB),
                Tier::Python => (128 * MIB, 128 * MIB),
            };
            let suggested = interface.and_then(|i| i.memory).unwrap_or(defaults);
            let declared = app.resources.as_ref().and_then(Node::as_table);
            let size = |key: &str| {
                declared
                    .and_then(|t| t.get(key))
                    .and_then(|e| e.node.as_str())
                    .and_then(crate::grammar::size)
            };
            let both = size("memory");
            total += size("cycle_memory").or(both).unwrap_or(suggested.0);
            total += size("async_memory").or(both).unwrap_or(suggested.1);
        }
        for cycle in manifest.cycles.values() {
            if let (Some(continuous), Some(period)) = (&cycle.trace_continuous, &cycle.period) {
                let ticks = continuous
                    .value
                    .as_nanos()
                    .div_ceil(period.value.as_nanos().max(1));
                total += ticks * (64 + 16 * manifest.devices.len() as u64);
            }
        }
        if total as f64 > MEMORY_SHARE * ram as f64 {
            self.push(
                Diagnostic::new("J0380", "estimated memory above 70% of the board's RAM")
                    .at(board.span)
                    .label(format!("{} MiB estimated, {} MiB of RAM", total / MIB, ram / MIB))
                    .help("lower `resources.memory` of the apps, shorten `trace.continuous`, or use a board with more RAM"),
            );
        }
    }

    fn batch_cost(&self, bus: &Bus, batch: &Batch, latency: f64) -> f64 {
        if let Some(cost) = batch.cost {
            return cost.as_nanos() as f64;
        }
        let bytes = f64::from(batch.request + batch.response);
        match bus.kind.value {
            BusKind::Serial => {
                let baud = bus.baud.as_ref().map_or(9600.0, |b| b.value as f64);
                let parity = if bus.parity == "none" { 0.0 } else { 1.0 };
                let bits = 1.0 + bus.data_bits as f64 + parity + bus.stop as f64;
                let turnaround = bus
                    .turnaround
                    .as_ref()
                    .map_or(3.5 * bits / baud * 1e9, |t| t.value.as_nanos() as f64);
                bytes * bits / baud * 1e9 + turnaround + latency
            }
            BusKind::I2c => (bytes + 2.0) * 9.0 / 100_000.0 * 1e9,
            BusKind::Spi => {
                let speed = bus
                    .speed
                    .as_ref()
                    .and_then(|s| joker_units::Quantity::parse(&s.value).ok())
                    .and_then(|q| q.value_in(&joker_units::Unit::parse("Hz").ok()?).ok())
                    .unwrap_or(1e6);
                bytes * 8.0 / speed * 1e9
            }
            BusKind::Ethernet => latency,
            BusKind::Can | BusKind::Ethercat => 0.0,
        }
    }

    /// The bus budget of ADR 0049 (`J0370`).
    pub(crate) fn bus_budget(&mut self) {
        let manifest = self.manifest;
        for (name, bus) in &manifest.buses {
            let mut load = 0.0;
            for (device_name, device) in &manifest.devices {
                if device.bus.as_ref().is_none_or(|b| b.value != *name) {
                    continue;
                }
                let Some(batches) = self.context.loads.get(device_name) else {
                    continue;
                };
                let profile = device
                    .profile
                    .as_ref()
                    .and_then(|p| self.context.profiles.get(&p.value));
                let latency = profile
                    .and_then(|p| p.latency)
                    .unwrap_or(Duration::from_millis(10))
                    .as_nanos() as f64;
                let device_poll = device
                    .poll
                    .as_ref()
                    .map(|p| p.value)
                    .or_else(|| profile.and_then(|p| p.poll))
                    .unwrap_or(Duration::from_secs(1));
                for batch in batches {
                    let poll = batch.poll.unwrap_or(device_poll).as_nanos().max(1) as f64;
                    let cost = match bus.kind.value {
                        BusKind::Can => {
                            let bitrate =
                                bus.bitrate.as_ref().map_or(500_000.0, |b| b.value as f64);
                            f64::from(batch.frames) * 135.0 / bitrate * 1e9
                        }
                        _ => self.batch_cost(bus, batch, latency),
                    };
                    load += cost / poll;
                }
            }
            if load > 1.0 {
                self.push(
                    Diagnostic::new("J0370", format!("bus `{name}` overloaded"))
                        .at(bus.span)
                        .label(format!("estimated occupancy {:.0}%", load * 100.0))
                        .help(format!(
                            "multiply the polls on this bus by {:.2} to reach the 80% target, or merge batches",
                            load / 0.8
                        )),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(name: &str, period_ms: f64, cost_ms: f64) -> Group {
        Group {
            name: name.into(),
            period: period_ms * 1e6,
            cost: cost_ms * 1e6,
            priority: None,
        }
    }

    #[test]
    fn harmonic_groups_pass_by_response_time() {
        let a = group("a", 10.0, 4.5);
        let b = group("b", 20.0, 9.0);
        let set = [&a, &b];
        assert!(utilisation(&set) > bound(2));
        assert!(first_miss(&set).is_none());
        assert!(schedulable(&set));
    }

    #[test]
    fn an_overloaded_cpu_fails() {
        let fast = group("fast", 1.0, 0.9);
        let slow = group("slow", 10.0, 2.0);
        let set = [&fast, &slow];
        let (g, _) = first_miss(&set).unwrap();
        assert_eq!(g.name, "slow");
        assert!(!schedulable(&set));
    }

    #[test]
    fn the_bound() {
        assert!((bound(1) - 1.0).abs() < 1e-9);
        assert!((bound(2) - 0.828_427).abs() < 1e-5);
    }
}
