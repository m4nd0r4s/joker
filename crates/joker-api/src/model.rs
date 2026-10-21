//! The request bodies of the M0 subset (ref/09 §5). Requests are strict:
//! an unknown field is `J-E1011` (ref/09 §3.1), except `x_*` extensions,
//! which the server drops before decoding.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `ManualWriteRequest` (ref/09 §5.6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManualWriteRequest {
    /// Value: a number in the output's unit, a quantity literal, a boolean or
    /// a variant.
    pub value: Value,
    /// TTL, mandatory (docs/16 A10).
    pub ttl: Option<String>,
    /// `manual:3`–`manual:5`, default `manual:4`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    /// Why, mandatory.
    pub reason: Option<String>,
    /// Take the level from another principal.
    #[serde(default)]
    pub takeover: bool,
    /// Wait for the tick that commits the write.
    #[serde(default = "yes")]
    pub wait: bool,
    /// How long to wait, default `5s`, at most `30s`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_timeout: Option<String>,
    /// Reserved for 1.1 (`J-E1017`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

fn yes() -> bool {
    true
}

/// `ReleaseRequest` (ref/09 §5.6).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseRequest {
    /// The level to release; default the one the principal holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    /// Release a level held by another principal.
    #[serde(default)]
    pub force: bool,
    /// Why.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Wait for the commit.
    #[serde(default = "yes")]
    pub wait: bool,
}

/// `ContentRef`: a blob digest or inline content.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentRef {
    /// Digest of an uploaded blob.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    /// Inline content, at most 1 MiB.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

/// `ArtifactRef`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRef {
    /// Digest.
    pub digest: String,
    /// Media type, short form.
    pub kind: String,
    /// Name in the project.
    pub name: String,
    /// Size in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Detached signature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

/// `GenerationProposal` (ref/09 §5.13).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationProposal {
    /// The effective manifest.
    pub manifest: ContentRef,
    /// The lock.
    pub lock: ContentRef,
    /// Provenance of each key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<ContentRef>,
    /// Artifacts.
    pub artifacts: Vec<ArtifactRef>,
    /// Signed descriptor (`prod`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<ContentRef>,
    /// Base generation hash.
    #[serde(default)]
    pub base: Option<String>,
    /// Message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Git source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Value>,
}

/// `ActivateRequest`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivateRequest {
    /// Confirmation window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirm_window: Option<String>,
    /// Why.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Wait for the switch.
    #[serde(default = "yes")]
    pub wait: bool,
}

/// `RollbackRequest`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RollbackRequest {
    /// Target generation; default `previous`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// Confirmation window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirm_window: Option<String>,
    /// Why, mandatory.
    pub reason: String,
}

/// `ReasonBody`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReasonBody {
    /// Why.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// `TraceRequest` (ref/09 §5.16).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceRequest {
    /// `record`, `ring`, `history` or `micro`.
    pub source: String,
    /// Recording length (`record`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<String>,
    /// Groups; default all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<String>>,
    /// Record async reads too.
    #[serde(default, rename = "async")]
    pub async_reads: bool,
    /// The last part of the ring (`ring`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<String>,
    /// Start (`ring`, `history`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// End (`ring`, `history`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// App (`history`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    /// MCU (`micro`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    /// Free label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}
