//! The traces of what an app emits and raises (ref/04 §6.4): events and
//! alarms.

use core::time::Duration;

use joker::Record;

/// The events one `EventOutput<E>` emitted, in order.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct EventTrace {
    events: Vec<(Duration, Vec<u8>)>,
}

impl EventTrace {
    pub(crate) fn new(events: Vec<(Duration, Vec<u8>)>) -> Self {
        Self { events }
    }

    /// The number of events.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// True when none was emitted.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// The instants of the events and their deterministic CBOR.
    pub fn payloads(&self) -> impl Iterator<Item = (Duration, &[u8])> + '_ {
        self.events.iter().map(|(t, p)| (*t, p.as_slice()))
    }

    /// The events decoded as `E`; a payload that does not decode fails the
    /// test.
    pub fn decode<E: Record>(&self) -> Vec<(Duration, E)> {
        self.events
            .iter()
            .map(|(t, p)| {
                let e = E::decode_from(p)
                    .unwrap_or_else(|| panic!("the event at {t:?} does not decode as {}", E::NAME));
                (*t, e)
            })
            .collect()
    }
}

/// The condition of one alarm of the app, tick by tick.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct AlarmTrace {
    points: Vec<(Duration, bool)>,
}

impl AlarmTrace {
    pub(crate) fn new(points: Vec<(Duration, bool)>) -> Self {
        Self { points }
    }

    /// The condition at `t`: the last one set at or before it.
    pub fn active_at(&self, t: Duration) -> bool {
        self.points
            .iter()
            .take_while(|(at, _)| *at <= t)
            .last()
            .is_some_and(|(_, a)| *a)
    }

    /// The last condition set.
    pub fn active(&self) -> bool {
        self.points.last().is_some_and(|(_, a)| *a)
    }

    /// The instants at which the condition went from false to true.
    pub fn raised(&self) -> Vec<Duration> {
        let mut was = false;
        let mut out = Vec::new();
        for (t, a) in &self.points {
            if *a && !was {
                out.push(*t);
            }
            was = *a;
        }
        out
    }

    /// The instants at which the condition went from true to false.
    pub fn cleared(&self) -> Vec<Duration> {
        let mut was = false;
        let mut out = Vec::new();
        for (t, a) in &self.points {
            if !*a && was {
                out.push(*t);
            }
            was = *a;
        }
        out
    }
}
