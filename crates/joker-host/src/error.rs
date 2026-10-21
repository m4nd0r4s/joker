//! The error shape of the host (ref/00 §8): a registry code and an English
//! message that nobody parses.

use core::fmt;

/// Codes of `spec/codes.toml` used by the host.
pub mod codes {
    /// Component world is newer than this runtime.
    pub const WORLD_TOO_NEW: &str = "J-E0001";
    /// Component world or cycle ABI is outside the supported window.
    pub const OUTSIDE_WINDOW: &str = "J-E0002";
    /// Component uses an unstable feature.
    pub const UNSTABLE_FEATURE: &str = "J-E0003";
    /// `joker:meta` missing or invalid.
    pub const META_INVALID: &str = "J-E0004";
    /// `joker:meta` world does not match the component imports.
    pub const META_WORLD_MISMATCH: &str = "J-E0005";
    /// Component imports an interface that is not allowed.
    pub const IMPORT_NOT_ALLOWED: &str = "J-E0006";
    /// App name in `joker:meta` does not match the manifest.
    pub const APP_NAME_MISMATCH: &str = "J-E0007";
    /// The bindings handed to the host do not match the component's layout.
    pub const LAYOUT_MISMATCH: &str = "J-E0008";
    /// The bytes are not a valid component, or it failed to compile.
    pub const COMPONENT_INVALID: &str = "J-E0009";
    /// No usable AOT artifact for this engine and component.
    pub const AOT_UNAVAILABLE: &str = "J-E0010";
    /// The app returned an error.
    pub const APP_ERROR: &str = "J-E0050";
    /// Port is not bound.
    pub const NOT_BOUND: &str = "J-E0103";
    /// Value type does not match the port.
    pub const WRONG_TYPE: &str = "J-E0104";
    /// Proposal buffer full; later proposals dropped.
    pub const BUFFER_FULL: &str = "J-E0203";
    /// Feature not available in this runtime.
    pub const NOT_AVAILABLE: &str = "J-E0601";
    /// Async API called from the cycle instance.
    pub const ASYNC_FROM_CYCLE: &str = "J-E0901";
    /// Cycle task deadline exceeded.
    pub const DEADLINE: &str = "J-E0904";
    /// Guest panicked or called exit.
    pub const GUEST_ABORT: &str = "J-E0905";
    /// Memory limit exceeded.
    pub const MEMORY_LIMIT: &str = "J-E0906";
    /// Malformed proposal buffer or invalid port use.
    pub const MALFORMED: &str = "J-E0907";
    /// Init failed or timed out.
    pub const INIT_FAILED: &str = "J-E0909";
    /// Stack overflow.
    pub const STACK_OVERFLOW: &str = "J-E0910";
    /// Uncaught exception in a tier 2 SDK (ref/05 §9.3).
    pub const UNCAUGHT: &str = "J-E0816";
}

/// An error of the host.
#[derive(Clone, PartialEq, Eq)]
pub struct HostError {
    code: &'static str,
    message: String,
}

impl HostError {
    /// An error with a registry code and a message.
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// The registry code (`"J-E0004"`).
    pub fn code(&self) -> &'static str {
        self.code
    }

    /// The human-readable message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Debug for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for HostError {}

/// A trap raised by a host function, carried through wasmtime back to the
/// call site.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct HostTrap {
    pub(crate) code: &'static str,
    pub(crate) what: &'static str,
}

impl fmt::Display for HostTrap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.what)
    }
}

impl std::error::Error for HostTrap {}

pub(crate) fn trap(code: &'static str, what: &'static str) -> wasmtime::Error {
    wasmtime::Error::new(HostTrap { code, what })
}
