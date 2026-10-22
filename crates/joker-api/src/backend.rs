//! What the server asks of the daemon. Every method may block: the server
//! calls them on tokio's blocking pool.

use serde_json::Value;

use crate::model::{
    ActivateRequest, GenerationProposal, InvokeActionRequest, ManualWriteRequest, ReasonBody,
    ReleaseRequest, ResetTripRequest, RollbackRequest, TraceRequest,
};
use crate::problem::Problem;

/// Who is calling (ref/09 §2.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    /// `unix:<user>` on the socket.
    pub id: String,
    /// The transport: `unix`, `tcp` or `tunnel`.
    pub transport: &'static str,
    /// The peer's uid on the socket.
    pub uid: Option<u32>,
}

/// A successful reply.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    /// HTTP status (200, 201, 202).
    pub status: u16,
    /// JSON body.
    pub body: Value,
    /// An `ETag`, when the resource has one.
    pub etag: Option<String>,
}

impl Reply {
    /// A 200 reply.
    pub fn ok(body: Value) -> Self {
        Self {
            status: 200,
            body,
            etag: None,
        }
    }

    /// A reply with another status.
    pub fn with_status(status: u16, body: Value) -> Self {
        Self {
            status,
            body,
            etag: None,
        }
    }
}

/// The result of an operation.
pub type Result<T> = std::result::Result<T, Problem>;

/// The headers every response carries (ref/09 §3.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Headers {
    /// `"<gen> sha256:<hex>"` of the running generation, if any.
    pub generation: Option<String>,
    /// `synced`, `holdover` or `unsynced`.
    pub clock: String,
}

/// Query parameters, in order, with repeats (`sel=a&sel=b`).
pub type Query = Vec<(String, String)>;

/// The daemon behind the API: the M0 subset of ref/09 §5.
pub trait Backend: Send + Sync + 'static {
    /// Headers of every response.
    fn headers(&self) -> Headers;
    /// `GET /v1/info`.
    fn info(&self, who: &Principal) -> Result<Reply>;
    /// `GET /v1/health`.
    fn health(&self, query: &Query) -> Result<Reply>;
    /// `GET /v1/status`.
    fn status(&self) -> Result<Reply>;
    /// `GET /v1/signals`.
    fn signals(&self, query: &Query) -> Result<Reply>;
    /// `GET /v1/signals/{path}`.
    fn signal(&self, path: &str) -> Result<Reply>;
    /// `GET /v1/values`.
    fn values(&self, query: &Query) -> Result<Reply>;
    /// `GET /v1/outputs`.
    fn outputs(&self, query: &Query) -> Result<Reply>;
    /// `GET /v1/outputs/{path}`.
    fn output(&self, path: &str) -> Result<Reply>;
    /// `POST /v1/outputs/{path}/manual`.
    fn manual_write(
        &self,
        who: &Principal,
        path: &str,
        request: ManualWriteRequest,
    ) -> Result<Reply>;
    /// `POST /v1/outputs/{path}/release`.
    fn release(&self, who: &Principal, path: &str, request: ReleaseRequest) -> Result<Reply>;
    /// `POST /v1/outputs/{path}/reset` (ref/09 §5.7, docs/16 E7).
    fn reset_trip(&self, who: &Principal, path: &str, request: ResetTripRequest) -> Result<Reply>;
    /// `GET /v1/manual`.
    fn manual(&self, query: &Query) -> Result<Reply>;
    /// `GET /v1/writes/{write}`.
    fn write_ticket(&self, id: &str) -> Result<Reply>;
    /// `GET /v1/decisions`.
    fn decisions(&self, query: &Query) -> Result<Reply>;
    /// `GET /v1/generations`.
    fn generations(&self, query: &Query) -> Result<Reply>;
    /// `POST /v1/generations`.
    fn create_generation(
        &self,
        who: &Principal,
        request: GenerationProposal,
        dry_run: bool,
    ) -> Result<Reply>;
    /// `GET /v1/generations/{gen}`.
    fn generation(&self, selector: &str) -> Result<Reply>;
    /// `POST /v1/generations/{gen}/activate`.
    fn activate(
        &self,
        who: &Principal,
        selector: &str,
        if_match: Option<&str>,
        request: ActivateRequest,
    ) -> Result<Reply>;
    /// `POST /v1/generations/{gen}/confirm`.
    fn confirm(&self, who: &Principal, selector: &str, request: ReasonBody) -> Result<Reply>;
    /// `POST /v1/generations/rollback`.
    fn rollback(
        &self,
        who: &Principal,
        if_match: Option<&str>,
        request: RollbackRequest,
    ) -> Result<Reply>;
    /// `GET /v1/traces`.
    fn traces(&self) -> Result<Reply>;
    /// `POST /v1/traces`.
    fn create_trace(&self, who: &Principal, request: TraceRequest) -> Result<Reply>;
    /// `GET /v1/traces/{trace}`.
    fn trace(&self, id: &str) -> Result<Reply>;
    /// `GET /v1/traces/{trace}/file`: the bytes of a ready trace.
    fn trace_file(&self, id: &str) -> Result<Vec<u8>>;
    /// `GET /v1/actions` (ref/09 §5.10): catalog of invocable actions.
    fn actions(&self, query: &Query) -> Result<Reply>;
    /// `POST /v1/actions/{target}/{action}` (ref/09 §5.10): `idempotency_key`
    /// is the required `Idempotency-Key` header (`J-E1023` without it).
    fn invoke_action(
        &self,
        who: &Principal,
        target: &str,
        action: &str,
        idempotency_key: Option<&str>,
        request: InvokeActionRequest,
    ) -> Result<Reply>;
}
