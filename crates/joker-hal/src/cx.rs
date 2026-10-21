//! The host services a driver call borrows (ref/06 §3.4): the clock, the
//! deadline, sleeping, link reports, logs and cancellation.
//!
//! Time enters only through the host (ADR 0007): the same driver runs on a
//! real clock in `jokerd` and on a virtual one in tests, replay and `jk dev`.

use core::future::{Future, poll_fn};
use core::pin::pin;
use core::task::{Context, Poll};

use joker_core::{Clock, Instant};

use crate::types::{Detail, LinkState, MonoNs};

/// Marker: `Send` on std hosts (tokio multi-thread), nothing on the Joker
/// Micro (Embassy).
#[cfg(feature = "std")]
pub trait MaybeSend: Send {}
#[cfg(feature = "std")]
impl<T: Send + ?Sized> MaybeSend for T {}
/// Marker: `Send` on std hosts (tokio multi-thread), nothing on the Joker
/// Micro (Embassy).
#[cfg(not(feature = "std"))]
pub trait MaybeSend {}
#[cfg(not(feature = "std"))]
impl<T: ?Sized> MaybeSend for T {}

/// Marker: `Sync` on std hosts, nothing on the Joker Micro.
#[cfg(feature = "std")]
pub trait MaybeSync: Sync {}
#[cfg(feature = "std")]
impl<T: Sync + ?Sized> MaybeSync for T {}
/// Marker: `Sync` on std hosts, nothing on the Joker Micro.
#[cfg(not(feature = "std"))]
pub trait MaybeSync {}
#[cfg(not(feature = "std"))]
impl<T: ?Sized> MaybeSync for T {}

/// Severity of a driver log line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LogLevel {
    /// Detail for debugging.
    Debug,
    /// Normal operation.
    Info,
    /// Something to look at.
    Warn,
    /// A failure.
    Error,
}

/// What the host gives every call. Implementations never block.
pub trait Host: Clock + MaybeSync {
    /// Polls a sleep until the monotonic instant `at`; ready once
    /// `now().mono_ns >= at`.
    fn poll_sleep(&self, at: MonoNs, cx: &mut Context<'_>) -> Poll<()>;

    /// True when the core cancelled the current call.
    fn cancelled(&self) -> bool {
        false
    }

    /// Takes one log line; rate limiting and the ring are the host's
    /// (ref/06 §2.15).
    fn log(&self, level: LogLevel, message: &str) {
        let _ = (level, message);
    }
}

/// The deadline passed before the operation finished.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Elapsed;

/// Host services, borrowed for the duration of one call.
pub struct Cx<'a> {
    host: &'a dyn Host,
    deadline: MonoNs,
    link: Option<(LinkState, Detail)>,
    progress: Option<(u16, Detail)>,
}

impl core::fmt::Debug for Cx<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Cx")
            .field("deadline", &self.deadline)
            .field("link", &self.link)
            .field("progress", &self.progress)
            .finish_non_exhaustive()
    }
}

impl<'a> Cx<'a> {
    /// A context for one call with an absolute monotonic `deadline`.
    pub fn new(host: &'a dyn Host, deadline: MonoNs) -> Self {
        Self {
            host,
            deadline,
            link: None,
            progress: None,
        }
    }

    /// Runtime clock: monotonic, UTC and clock quality (ADR 0007).
    pub fn now(&self) -> Instant {
        self.host.now()
    }

    /// Absolute deadline of the current call.
    pub fn deadline(&self) -> MonoNs {
        self.deadline
    }

    /// Sleeps until the monotonic instant `at`.
    pub fn sleep_until(&self, at: MonoNs) -> impl Future<Output = ()> + MaybeSend + '_ {
        let host = self.host;
        poll_fn(move |cx| host.poll_sleep(at, cx))
    }

    /// Runs `fut` until it finishes or the monotonic instant `at` passes.
    pub async fn until<F: Future>(&self, at: MonoNs, fut: F) -> Result<F::Output, Elapsed> {
        let host = self.host;
        let mut fut = pin!(fut);
        poll_fn(move |cx| {
            if let Poll::Ready(v) = fut.as_mut().poll(cx) {
                return Poll::Ready(Ok(v));
            }
            match host.poll_sleep(at, cx) {
                Poll::Ready(()) => Poll::Ready(Err(Elapsed)),
                Poll::Pending => Poll::Pending,
            }
        })
        .await
    }

    /// Runs `fut` until it finishes or the call's deadline passes.
    pub async fn within<F: Future>(&self, fut: F) -> Result<F::Output, Elapsed> {
        self.until(self.deadline, fut).await
    }

    /// Reports the link state seen by the driver; the core takes the worst of
    /// this and its own computation (ref/06 §2.12).
    pub fn report_link(&mut self, state: LinkState, detail: &str) {
        self.link = Some((state, Detail::new(detail)));
    }

    /// The last link report of this call.
    pub fn reported_link(&self) -> Option<(LinkState, &str)> {
        self.link.as_ref().map(|(s, d)| (*s, d.as_str()))
    }

    /// Logs a line: structured, rate limited by the host, never blocks.
    pub fn log(&self, level: LogLevel, message: &str) {
        self.host.log(level, message);
    }

    /// Progress of a long action, in permille.
    pub fn progress(&mut self, permille: u16, message: &str) {
        self.progress = Some((permille.min(1000), Detail::new(message)));
    }

    /// The last progress report of this call.
    pub fn reported_progress(&self) -> Option<(u16, &str)> {
        self.progress.as_ref().map(|(p, d)| (*p, d.as_str()))
    }

    /// True when the core cancelled the current call.
    pub fn cancelled(&self) -> bool {
        self.host.cancelled()
    }
}
