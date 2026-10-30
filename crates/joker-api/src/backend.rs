//! What the server asks of the daemon. Every method may block: the server
//! calls them on tokio's blocking pool.

use serde_json::Value;

use crate::model::{
    AckRequest, ActivateRequest, AppOp, BulkResetRequest, DiscoveryRequest, DoctorRequest,
    GenerationProposal, InvokeActionRequest, ManualWriteRequest, MethodCallRequest,
    MissingBlobsRequest, ReasonBody, ReleaseRequest, ResetTripRequest, RollbackRequest,
    ShelveRequest, TraceRequest,
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
    /// Further headers (`Location`, `Joker-Idempotent-Replay`).
    pub headers: Vec<(&'static str, String)>,
}

impl Reply {
    /// A 200 reply.
    pub fn ok(body: Value) -> Self {
        Self {
            status: 200,
            body,
            etag: None,
            headers: Vec::new(),
        }
    }

    /// A reply with another status.
    pub fn with_status(status: u16, body: Value) -> Self {
        Self {
            status,
            body,
            etag: None,
            headers: Vec::new(),
        }
    }

    /// This reply with the header `name: value`.
    pub fn with_header(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
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
    /// The `[security.roles]` of the running generation, for the role of
    /// each caller (ref/09 §2.3, ADR 0066); the defaults without one.
    fn socket_roles(&self) -> crate::server::SocketRoles {
        crate::server::SocketRoles::default()
    }
    /// `GET /v1/info`.
    fn info(&self, who: &Principal) -> Result<Reply>;
    /// `GET /v1/version` (ref/09 §5.1): the versions of every surface this
    /// node supports.
    fn version(&self) -> Result<Reply> {
        Err(unavailable())
    }
    /// `GET /v1/health`.
    fn health(&self, query: &Query) -> Result<Reply>;
    /// `GET /v1/status`.
    fn status(&self) -> Result<Reply>;
    /// `GET /v1/metrics` (ref/09 §5.2).
    fn metrics(&self) -> Result<Reply>;
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
    /// `GET /v1/trips` (ref/09 §5.7, `jk trips`).
    fn trips(&self, query: &Query) -> Result<Reply>;
    /// `POST /v1/trips/reset` (ref/09 §5.7).
    fn reset_trips(&self, who: &Principal, request: BulkResetRequest) -> Result<Reply>;
    /// `GET /v1/manual`.
    fn manual(&self, query: &Query) -> Result<Reply>;
    /// `GET /v1/writes/{write}`.
    fn write_ticket(&self, id: &str) -> Result<Reply>;
    /// `GET /v1/decisions`.
    fn decisions(&self, query: &Query) -> Result<Reply>;
    /// `GET /v1/logs` (ref/09 §5.2, `jk logs`).
    fn logs(&self, query: &Query) -> Result<Reply>;
    /// `GET /v1/generations`.
    fn generations(&self, query: &Query) -> Result<Reply>;
    /// `POST /v1/generations`.
    fn create_generation(
        &self,
        who: &Principal,
        request: GenerationProposal,
        dry_run: bool,
    ) -> Result<Reply>;
    /// `POST /v1/blobs/missing`: which digests the store lacks.
    fn missing_blobs(&self, request: MissingBlobsRequest) -> Result<Reply>;
    /// `HEAD /v1/blobs/{digest}`: whether the store holds the blob.
    fn has_blob(&self, digest: &str) -> Result<bool>;
    /// `POST /v1/blobs/uploads?digest=` with the bytes: a monolithic upload
    /// (ref/09 §5.13), checked against `digest`.
    fn upload_blob(
        &self,
        who: &Principal,
        digest: &str,
        kind: Option<&str>,
        bytes: &[u8],
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
    /// `GET /v1/invocations` (ref/09 §5.10).
    fn invocations(&self, query: &Query) -> Result<Reply>;
    /// `GET /v1/invocations/{invocation}`.
    fn invocation(&self, id: &str) -> Result<Reply>;
    /// `POST /v1/invocations/{invocation}/cancel`: cancels a long action
    /// (ADR 0066 §9).
    fn cancel_invocation(&self, who: &Principal, id: &str, request: ReasonBody) -> Result<Reply>;
    /// `GET /v1/history` (ref/09 §5.5); `POST /v1/history/query` comes
    /// here with its body as parameters.
    fn history(&self, _query: &Query) -> Result<Reply> {
        Err(unavailable())
    }
    /// `GET /v1/groups` (ref/09 §5.2): the cycle groups with their tasks
    /// and statistics.
    fn groups(&self) -> Result<Reply> {
        Err(unavailable())
    }
    /// `GET /v1/groups/{group}`.
    fn group(&self, _name: &str) -> Result<Reply> {
        Err(unavailable())
    }
    /// `GET /v1/signals/{path}/lineage` (ref/09 §5.4): where a signal comes
    /// from and who reads and writes it.
    fn lineage(&self, _path: &str) -> Result<Reply> {
        Err(unavailable())
    }
    /// The records of the `signal_trace` topic (ref/09 §6) of `path`: one
    /// per recent tick that changed something about the signal, each with
    /// a `key` that orders them (ADR 0091).
    fn signal_trace(&self, _path: &str) -> Result<Reply> {
        Err(unavailable())
    }
    /// `GET /v1/generations/{gen}/manifest` (ref/09 §5.13): the effective
    /// manifest the generation was created with, as TOML text.
    fn generation_manifest(&self, _selector: &str) -> Result<String> {
        Err(unavailable())
    }
    /// `GET /v1/node/hardware` (ref/09 §5.2): the hardware inventory of
    /// the node, with reads only.
    fn node_hardware(&self) -> Result<Reply> {
        Err(unavailable())
    }
    /// `GET /v1/jobs/{job}` (ref/09 §5.17).
    fn job(&self, _id: &str) -> Result<Reply> {
        Err(unavailable())
    }
    /// `GET /v1/doctor`: the last stored report.
    fn doctor_report(&self) -> Result<Reply> {
        Err(unavailable())
    }
    /// `POST /v1/doctor`: starts a `doctor` job (`jk doctor`).
    fn run_doctor(&self, _who: &Principal, _request: DoctorRequest) -> Result<Reply> {
        Err(unavailable())
    }
    /// `POST /v1/discovery`: starts a `discovery` job (`jk discover`).
    fn run_discovery(&self, _who: &Principal, _request: DiscoveryRequest) -> Result<Reply> {
        Err(unavailable())
    }
    /// `GET /v1/apps` (ref/09 §5.14).
    fn apps(&self) -> Result<Reply> {
        Err(unavailable())
    }
    /// `GET /v1/apps/{app}`.
    fn app(&self, _app: &str) -> Result<Reply> {
        Err(unavailable())
    }
    /// `POST /v1/apps/{app}/start|stop|restart` (ref/09 §5.14, ADR 0091).
    fn app_lifecycle(
        &self,
        _who: &Principal,
        _app: &str,
        _op: AppOp,
        _request: ReasonBody,
    ) -> Result<Reply> {
        Err(unavailable())
    }
    /// `GET /v1/events` (ref/09 §5.5).
    fn events(&self, _query: &Query) -> Result<Reply> {
        Err(unavailable())
    }
    /// `GET /v1/alarms` (ref/09 §5.8).
    fn alarms(&self, _query: &Query) -> Result<Reply> {
        Err(unavailable())
    }
    /// `GET /v1/alarms/{alarm}`.
    fn alarm(&self, _id: &str) -> Result<Reply> {
        Err(unavailable())
    }
    /// `POST /v1/alarms/{alarm}/ack`.
    fn ack_alarm(&self, _who: &Principal, _id: &str, _request: AckRequest) -> Result<Reply> {
        Err(unavailable())
    }
    /// `POST /v1/alarms/{alarm}/shelve`.
    fn shelve_alarm(&self, _who: &Principal, _id: &str, _request: ShelveRequest) -> Result<Reply> {
        Err(unavailable())
    }
    /// `POST /v1/alarms/{alarm}/unshelve`.
    fn unshelve_alarm(&self, _who: &Principal, _id: &str, _request: AckRequest) -> Result<Reply> {
        Err(unavailable())
    }
    /// `GET /v1/alarm-events` (ref/09 §5.8).
    fn alarm_events(&self, _query: &Query) -> Result<Reply> {
        Err(unavailable())
    }
    /// `POST /v1/instances/{instance}/methods/{method}` (ref/09 §5.11).
    fn call_method(
        &self,
        _who: &Principal,
        _instance: &str,
        _method: &str,
        _request: MethodCallRequest,
    ) -> Result<Reply> {
        Err(unavailable())
    }
}

fn unavailable() -> Problem {
    Problem::new("J-E1005", 404, "no such endpoint on this node")
}
