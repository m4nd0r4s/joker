//! Action dispatch (M19, docs/02 §8): not arbitrated, serialized per
//! device, gated by a permissive and deduplicated by an idempotency key
//! within `dedup_window` (ref/01 §5.8.2, docs/16 E9).
//!
//! This only decides *whether* an invocation runs; executing the
//! profile's step language (ref/03 §2.2.12) against a driver is separate
//! and still open.

use std::collections::{BTreeMap, BTreeSet};

/// Why an invocation did not run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejected {
    /// The device already has an action in flight.
    Busy,
    /// `permit` evaluated to false (`Uncertain` counts as false, ref/01
    /// §5.8.2).
    NotPermitted,
}

/// What [`Dispatcher::begin`] decided for one invocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision<T> {
    /// Run it: no cached result, the device is free and `permit` held.
    Admit,
    /// The same idempotency key, within `dedup_window`: the result of the
    /// earlier invocation, without running anything again.
    Duplicate(T),
    /// Not run, and why.
    Rejected(Rejected),
}

/// Serializes action invocations per device and deduplicates them by
/// idempotency key within each invocation's `dedup_window`.
///
/// `recent` is never pruned: every distinct `(device, action, key)` that
/// ever completes stays until the process restarts. Acceptable for a first
/// slice; a long-running node needs an eviction pass before this is relied
/// on in production.
#[derive(Debug)]
pub struct Dispatcher<T> {
    busy: BTreeSet<String>,
    recent: BTreeMap<(String, String, String), (u64, T)>,
}

impl<T> Default for Dispatcher<T> {
    fn default() -> Self {
        Self {
            busy: BTreeSet::new(),
            recent: BTreeMap::new(),
        }
    }
}

impl<T: Clone> Dispatcher<T> {
    /// An empty dispatcher: no device busy, no cached results.
    pub fn new() -> Self {
        Self::default()
    }

    /// Decides whether `device`'s `action`, invoked with idempotency `key`
    /// at `now_mono_ns`, should run (docs/02 §8, docs/16 E9). `permitted`
    /// is the caller's evaluation of the action's `permit` expression.
    pub fn begin(
        &mut self,
        device: &str,
        action: &str,
        key: &str,
        now_mono_ns: u64,
        dedup_window_ns: u64,
        permitted: bool,
    ) -> Decision<T> {
        let cache_key = (device.to_owned(), action.to_owned(), key.to_owned());
        if let Some((completed_at, result)) = self.recent.get(&cache_key)
            && now_mono_ns.saturating_sub(*completed_at) < dedup_window_ns
        {
            return Decision::Duplicate(result.clone());
        }
        if self.busy.contains(device) {
            return Decision::Rejected(Rejected::Busy);
        }
        if !permitted {
            return Decision::Rejected(Rejected::NotPermitted);
        }
        self.busy.insert(device.to_owned());
        Decision::Admit
    }

    /// Records the outcome of an admitted invocation, frees the device and
    /// caches the result, keyed by completion time so a later [`begin`]
    /// can tell whether it is still within that invocation's
    /// `dedup_window`.
    ///
    /// [`begin`]: Self::begin
    pub fn finish(&mut self, device: &str, action: &str, key: &str, now_mono_ns: u64, result: T) {
        self.busy.remove(device);
        self.recent.insert(
            (device.to_owned(), action.to_owned(), key.to_owned()),
            (now_mono_ns, result),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_first_invocation_is_admitted() {
        let mut d: Dispatcher<u8> = Dispatcher::new();
        assert_eq!(
            d.begin("pcs", "reset", "k1", 0, 60_000_000_000, true),
            Decision::Admit
        );
    }

    #[test]
    fn permit_false_rejects_without_marking_the_device_busy() {
        let mut d: Dispatcher<u8> = Dispatcher::new();
        assert_eq!(
            d.begin("pcs", "reset", "k1", 0, 60_000_000_000, false),
            Decision::Rejected(Rejected::NotPermitted)
        );
        // Not busy: a later, permitted call for the same device admits.
        assert_eq!(
            d.begin("pcs", "reset", "k2", 1, 60_000_000_000, true),
            Decision::Admit
        );
    }

    #[test]
    fn a_busy_device_rejects_a_second_invocation() {
        let mut d: Dispatcher<u8> = Dispatcher::new();
        assert_eq!(
            d.begin("pcs", "reset", "k1", 0, 60_000_000_000, true),
            Decision::Admit
        );
        assert_eq!(
            d.begin("pcs", "calibrate", "k2", 1, 60_000_000_000, true),
            Decision::Rejected(Rejected::Busy)
        );
    }

    #[test]
    fn finishing_frees_the_device_for_the_next_invocation() {
        let mut d: Dispatcher<u8> = Dispatcher::new();
        d.begin("pcs", "reset", "k1", 0, 60_000_000_000, true);
        d.finish("pcs", "reset", "k1", 10, 7);
        assert_eq!(
            d.begin("pcs", "calibrate", "k2", 11, 60_000_000_000, true),
            Decision::Admit
        );
    }

    #[test]
    fn the_same_key_within_the_window_returns_the_cached_result() {
        let mut d: Dispatcher<u8> = Dispatcher::new();
        d.begin("pcs", "reset", "k1", 0, 60_000_000_000, true);
        d.finish("pcs", "reset", "k1", 10, 42);
        // A reinvocation with the same key, before the window elapses,
        // does not run again and does not mark the device busy.
        assert_eq!(
            d.begin("pcs", "reset", "k1", 20, 60_000_000_000, true),
            Decision::Duplicate(42)
        );
        assert_eq!(
            d.begin("pcs", "calibrate", "k2", 20, 60_000_000_000, true),
            Decision::Admit
        );
    }

    #[test]
    fn the_same_key_after_the_window_runs_again() {
        let mut d: Dispatcher<u8> = Dispatcher::new();
        d.begin("pcs", "reset", "k1", 0, 1_000, true);
        d.finish("pcs", "reset", "k1", 0, 42);
        assert_eq!(
            d.begin("pcs", "reset", "k1", 1_001, 1_000, true),
            Decision::Admit
        );
    }
}
