#![doc = include_str!("../README.md")]

mod graph;
mod licence;
mod metadata;
pub mod policy;
mod toolchain;

use core::fmt;

pub use graph::check_graph;
pub use licence::offers_one_of;
pub use metadata::{DepKind, Dependency, Package, Workspace};
pub use toolchain::{Version, check_toolchains};

/// Why a check could not run: the input is not what it should be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(String);

impl Error {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// The rule of `docs/policies/dependencies.md` §1 that a violation breaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rule {
    /// A workspace crate depends on a workspace crate that document 13 §1 does not allow.
    WorkspaceEdge,
    /// A crate other than the CLI depends on `jokerd`, or a crate depends
    /// on the CLI (ADR 0057).
    Daemon,
    /// A `no_std` crate depends on a crate that is not `no_std`, or turns on its `std`.
    NoStd,
    /// A core crate depends on a crate outside the core allowlist.
    CoreAllowlist,
    /// A crate reachable from a `no_std` crate is not available under MIT or Apache-2.0.
    McuLicence,
    /// A toolchain file is missing, extra or not pinned as the policy says.
    Toolchain,
}

/// One break of a rule: which rule, which crate or file, and what was found.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Violation {
    /// The rule that is broken.
    pub rule: Rule,
    /// The crate or the file where it is broken.
    pub subject: String,
    /// What was found.
    pub detail: String,
}

impl Violation {
    pub(crate) fn new(rule: Rule, subject: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            rule,
            subject: subject.into(),
            detail: detail.into(),
        }
    }
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}: {}", self.rule, self.subject, self.detail)
    }
}
