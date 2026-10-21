//! Device fallback (docs/23 §4, M1): the LED goes off after 2 s without a
//! valid Modbus request.

/// Silence after which the fallback applies, in microseconds.
///
/// It is the `[fallback] timeout = "2s"` of the bench profile.
pub const TIMEOUT_US: u64 = 2_000_000;

/// Fallback timer, fed by every valid request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fallback {
    last_request_us: u64,
    active: bool,
}

impl Fallback {
    /// Creates the timer at boot time `now_us`.
    ///
    /// Boot counts as the last request, so a master that never speaks sees
    /// the fallback 2 s after boot.
    pub const fn new(now_us: u64) -> Self {
        Self {
            last_request_us: now_us,
            active: false,
        }
    }

    /// Returns whether the fallback is in force, that is, no request arrived
    /// since it applied.
    pub const fn is_active(&self) -> bool {
        self.active
    }

    /// Records a valid request at `now_us` and leaves the fallback.
    pub fn on_request(&mut self, now_us: u64) {
        self.last_request_us = now_us;
        self.active = false;
    }

    /// Advances the timer to `now_us`.
    ///
    /// Returns `true` once, at the first call after [`TIMEOUT_US`] of
    /// silence; the caller then drives the outputs to their fallback value.
    pub fn tick(&mut self, now_us: u64) -> bool {
        if self.active || now_us.saturating_sub(self.last_request_us) < TIMEOUT_US {
            return false;
        }
        self.active = true;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_once_after_the_timeout() {
        let mut fallback = Fallback::new(0);
        assert!(!fallback.tick(TIMEOUT_US - 1));
        assert!(fallback.tick(TIMEOUT_US));
        assert!(fallback.is_active());
        assert!(!fallback.tick(TIMEOUT_US * 10));
    }

    #[test]
    fn request_restarts_the_timer_and_leaves_the_fallback() {
        let mut fallback = Fallback::new(0);
        assert!(fallback.tick(TIMEOUT_US));
        fallback.on_request(3_000_000);
        assert!(!fallback.is_active());
        assert!(!fallback.tick(3_000_000 + TIMEOUT_US - 1));
        assert!(fallback.tick(3_000_000 + TIMEOUT_US));
    }

    #[test]
    fn clock_going_backwards_never_triggers() {
        let mut fallback = Fallback::new(5_000_000);
        assert!(!fallback.tick(0));
    }
}
