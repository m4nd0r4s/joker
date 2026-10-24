//! The crate lists and allowlists of `docs/policies/dependencies.md`, as
//! data. A change here is a change of the policy, in the same commit.

/// The core crates (document 13 §1, ref/08 §2.11).
pub const CORE: &[&str] = &[
    "joker-units",
    "joker-expr",
    "joker-core",
    "joker-link-proto",
    "joker-agent-proto",
];

/// Every `no_std` crate of document 13 §1: the core, and the crates that are
/// `no_std` with `alloc` and `std` as features.
pub const NO_STD: &[&str] = &[
    "joker-units",
    "joker-expr",
    "joker-core",
    "joker-link-proto",
    "joker-agent-proto",
    "joker-hal",
    "joker-driver-sdk",
    "joker-micro",
];

/// The workspace crates that each crate of the core may depend on
/// (document 13 §1).
pub const CORE_EDGES: &[(&str, &[&str])] = &[
    ("joker-units", &[]),
    ("joker-expr", &["joker-units"]),
    ("joker-core", &["joker-units", "joker-expr"]),
];

/// The daemon, on which no other crate depends (document 13 §1).
pub const DAEMON: &str = "jokerd";

/// The crates that may depend on the daemon: the `jk` binary, which embeds
/// the runtime (ADR 0057), and the repository's task binary, which runs
/// the `action` conformance suites against it (ADR 0066 §25). No crate
/// may depend on them.
pub const DAEMON_USERS: &[&str] = &["joker-cli", "xtask"];

/// The external crates the core may depend on (policy §3).
pub const CORE_ALLOWLIST: &[&str] = &[
    "heapless",
    "critical-section",
    "portable-atomic",
    "minicbor",
    "libm",
];

/// The external crates any `no_std` crate may depend on without `std`
/// (policy §3): the core allowlist and the transports of `joker-hal`.
pub const NO_STD_EXTERNAL: &[&str] = &[
    "heapless",
    "critical-section",
    "portable-atomic",
    "minicbor",
    "libm",
    "embedded-io",
    "embedded-io-async",
];

/// The licences one of which every crate under a `no_std` crate must offer
/// (policy §8).
pub const MCU_LICENCES: &[&str] = &["MIT", "Apache-2.0"];

/// Target `cfg`s that only exist in test builds (the `loom` model checker),
/// whose dependencies never ship.
pub const TEST_CFGS: &[&str] = &["cfg(loom)"];

/// The feature that turns on `std`, in our crates and in external ones.
pub const STD_FEATURE: &str = "std";

/// The toolchain files that may exist, relative to the root (policy §5).
pub const TOOLCHAIN_FILES: &[&str] = &[
    "rust-toolchain.toml",
    "crates/joker-micro/esp/rust-toolchain.toml",
];

/// The Rust version of the pinned `esp` toolchain (policy §5): the core MSRV
/// may not be above it. The per-phase update changes it.
pub const XTENSA_RUST: &str = "1.99";
