//! The request bodies of the M0 subset (ref/09 §5). Requests are strict:
//! an unknown field is `J-E1011` (ref/09 §3.1), except `x_*` extensions,
//! which the server drops before decoding.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

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

/// `MethodCallRequest` (ref/09 §5.11): a class method called as a manual
/// writer (docs/17 §4.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodCallRequest {
    /// The method's arguments, by name; quantities accept literals.
    #[serde(default)]
    pub args: Map<String, Value>,
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
    /// Wait for the tick that expands and commits the call.
    #[serde(default = "yes")]
    pub wait: bool,
    /// How long to wait, default `5s`, at most `30s`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_timeout: Option<String>,
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

/// The artifact types of ref/00 §7.1, in short form (`ArtifactKind`,
/// ref/09 §5.13). `joker.model.v1+jkm` is reserved for 1.1.
pub const ARTIFACT_KINDS: &[&str] = &[
    "joker.config.v1+cbor",
    "joker.manifest.v1+toml",
    "joker.lock.v1+toml",
    "joker.provenance.v1+json",
    "joker.app.v1+wasm",
    "joker.app.v1+cwasm",
    "joker.driver.v1+wasm",
    "joker.driver.v1+tar",
    "joker.profile.v1+toml",
    "joker.board.v1+toml",
    "joker.sim.v1+toml",
    "joker.class.v1+jkc",
    "joker.class.contract.v1+json",
    "joker.class.conformance.v1+toml",
    "joker.micro-firmware.v1",
    "joker.generation.v1",
    "joker.secrets.v1",
    "joker.key.v1+pem",
    "joker.runtime.v1",
    "joker.trace.v1",
    "joker.model.v1+jkm",
];

/// The body of `POST /v1/blobs/missing` (ref/09 §5.13).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MissingBlobsRequest {
    /// The digests to look up, 1 to 1024.
    pub digests: Vec<String>,
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

/// `ResetRequest` (ref/09 §5.7, `openapi` schema `ResetRequest`): the
/// body of `POST /v1/outputs/{path}/reset`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResetTripRequest {
    /// Why, mandatory.
    pub reason: String,
    /// Wait for the result after the next tick of the owning group.
    #[serde(default = "yes")]
    pub wait: bool,
}

/// `BulkResetRequest` (ref/09 §5.7, `openapi` schema `BulkResetRequest`):
/// the body of `POST /v1/trips/reset`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BulkResetRequest {
    /// Selectors of the outputs to reset.
    pub sel: Vec<String>,
    /// Why, mandatory.
    pub reason: String,
}

/// `InvokeRequest` (ref/09 §5.10, `openapi` schema `InvokeRequest`): the
/// body of `POST /v1/actions/{target}/{action}`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvokeActionRequest {
    /// The action's declared arguments, by name; quantities accept
    /// literals (ADR 0066 §5).
    #[serde(default)]
    pub args: Map<String, Value>,
    /// May shorten, never lengthen, the action's configured timeout
    /// (ADR 0066 §4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,
    /// Why, recorded with the invocation and its decision record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Wait for a short action's result (ADR 0066 §6).
    #[serde(default = "yes")]
    pub wait: bool,
}

/// `AckRequest` (ref/09 §5.8), also the body of an unshelve.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AckRequest {
    /// Why.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// `ShelveRequest` (ref/09 §5.8). Both fields are required; they are
/// optional here so that a missing TTL gets its own code (`J-E1201`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShelveRequest {
    /// How long, at most the alarm's `max_shelve`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl: Option<String>,
    /// Why.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// `HistoryQuery` (ref/09 §5.5): the parameters of `GET /v1/history` in a
/// body.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryQuery {
    /// Selectors.
    pub sel: Vec<String>,
    /// Start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// End.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// Duration back from `to`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    /// Aggregation interval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    /// Aggregates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agg: Option<Vec<String>>,
    /// `auto`, `raw`, `1s`, `1m`, `15m`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    /// `none`, `previous`, `null`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    /// Points per page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    /// Page cursor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

impl HistoryQuery {
    /// The same query as `GET` parameters.
    pub fn to_query(&self) -> Vec<(String, String)> {
        let mut q: Vec<(String, String)> =
            self.sel.iter().map(|s| ("sel".into(), s.clone())).collect();
        let mut put = |k: &str, v: &Option<String>| {
            if let Some(v) = v {
                q.push((k.into(), v.clone()));
            }
        };
        put("from", &self.from);
        put("to", &self.to);
        put("since", &self.since);
        put("step", &self.step);
        put("tier", &self.tier);
        put("fill", &self.fill);
        put("cursor", &self.cursor);
        if let Some(agg) = &self.agg {
            q.push(("agg".into(), agg.join(",")));
        }
        if let Some(limit) = self.limit {
            q.push(("limit".into(), limit.to_string()));
        }
        q
    }
}
