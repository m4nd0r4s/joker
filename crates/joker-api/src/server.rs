//! The HTTP server over the Unix socket (ref/09 §2.1): routes, strict
//! requests, common headers and problems.

use std::path::Path;
use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::extract::DefaultBodyLimit;
use axum::extract::connect_info::{ConnectInfo, Connected};
use axum::extract::{Path as UrlPath, RawQuery, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, head, post};
use axum::serve::IncomingStream;
use axum::{Extension, Router};
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::net::UnixListener;

use crate::backend::{Backend, Principal, Query, Reply};
use crate::problem::{PROBLEM_JSON, Problem};
use crate::ws;

/// The API version this server implements.
pub const API_VERSION: &str = "1.0.0";

/// The default socket path.
pub const DEFAULT_SOCKET: &str = "/run/joker/api.sock";

/// The credentials of the peer of a socket connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Peer {
    /// Its uid.
    pub uid: Option<u32>,
}

impl Connected<IncomingStream<'_, UnixListener>> for Peer {
    fn connect_info(stream: IncomingStream<'_, UnixListener>) -> Self {
        Self {
            uid: stream.io().peer_cred().ok().map(|c| c.uid()),
        }
    }
}

pub(crate) fn principal(peer: Peer) -> Principal {
    let name = peer.uid.map(user_name).unwrap_or_else(|| "unknown".into());
    Principal {
        id: format!("unix:{name}"),
        transport: "unix",
        uid: peer.uid,
    }
}

/// The roles of ref/09 §2.3, lowest first.
pub const ROLES: [&str; 3] = ["viewer", "operator", "admin"];

/// The rank of `role` in [`ROLES`]; an unknown role ranks below `viewer`.
pub fn role_rank(role: &str) -> Option<usize> {
    ROLES.iter().position(|r| *r == role)
}

/// `[security.roles]` of the running generation (ref/01 §5.20): the
/// rules that raise a principal to `operator` and to `admin`, `None` for
/// a role the manifest does not declare. The default is no generation:
/// the defaults of `prod` hold.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SocketRoles {
    /// The generation runs in `mode = "dev"`.
    pub dev: bool,
    /// `operator`.
    pub operator: Option<Vec<String>>,
    /// `admin`.
    pub admin: Option<Vec<String>>,
}

impl SocketRoles {
    /// The role of the local `user` for whom `member(group)` says whether
    /// it belongs to `group`, under these rules (ref/01 §5.20, ADR 0063,
    /// ADR 0066): `joker` is `admin`; in `dev` without
    /// `[security.roles]` anyone is `admin`; otherwise the highest role
    /// whose rules match, `operator` defaulting to members of `joker-ops`
    /// and `admin` to members of `joker-admin`; `viewer` when none does.
    pub fn local_role(&self, user: &str, member: &dyn Fn(&str) -> bool) -> &'static str {
        if user == "joker" || (self.dev && self.operator.is_none() && self.admin.is_none()) {
            return "admin";
        }
        let matches = |rules: &Option<Vec<String>>, default: &str| {
            let rules = rules
                .clone()
                .unwrap_or_else(|| vec![format!("local:group={default}")]);
            rules.iter().any(|rule| {
                if let Some(u) = rule.strip_prefix("local:user=") {
                    u == user
                } else if let Some(g) = rule.strip_prefix("local:group=") {
                    member(g)
                } else {
                    false
                }
            })
        };
        if matches(&self.admin, "joker-admin") {
            "admin"
        } else if matches(&self.operator, "joker-ops") {
            "operator"
        } else {
            "viewer"
        }
    }
}

/// The role of a principal with no generation's rules (ref/09 §2.3, ADR
/// 0063): see [`role_with`].
pub fn role_of(who: &Principal) -> &'static str {
    role_with(who, &SocketRoles::default())
}

/// The role of a principal under `roles` (ref/09 §2.3, ADR 0063, ADR
/// 0066): on the socket, uid 0 and the user `jokerd` runs as are `admin`;
/// any other local user gets [`SocketRoles::local_role`]; a principal
/// without a uid is `viewer`.
pub fn role_with(who: &Principal, roles: &SocketRoles) -> &'static str {
    match who.uid {
        Some(uid) => socket_role(uid, roles),
        None => "viewer",
    }
}

#[cfg(unix)]
fn socket_role(uid: u32, roles: &SocketRoles) -> &'static str {
    use nix::unistd::{Group, Uid, User, geteuid};
    if uid == 0 || uid == geteuid().as_raw() {
        return "admin";
    }
    let Ok(Some(user)) = User::from_uid(Uid::from_raw(uid)) else {
        return if roles.dev && roles.operator.is_none() && roles.admin.is_none() {
            "admin"
        } else {
            "viewer"
        };
    };
    let member = |group: &str| {
        Group::from_name(group)
            .ok()
            .flatten()
            .is_some_and(|g| g.gid == user.gid || g.mem.contains(&user.name))
    };
    roles.local_role(&user.name, &member)
}

#[cfg(not(unix))]
fn socket_role(_uid: u32, _roles: &SocketRoles) -> &'static str {
    "viewer"
}

#[cfg(unix)]
fn user_name(uid: u32) -> String {
    nix::unistd::User::from_uid(nix::unistd::Uid::from_raw(uid))
        .ok()
        .flatten()
        .map_or_else(|| uid.to_string(), |u| u.name)
}

#[cfg(not(unix))]
fn user_name(uid: u32) -> String {
    uid.to_string()
}

pub(crate) type Shared = Arc<dyn Backend>;

fn request_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    ulid(nanos)
}

pub use crate::id::{ulid, ulid_from};

fn base_headers(backend: &dyn Backend, request_id: &str) -> HeaderMap {
    let mut h = HeaderMap::new();
    let info = backend.headers();
    let mut put = |k: &'static str, v: &str| {
        if let Ok(v) = HeaderValue::from_str(v) {
            h.insert(k, v);
        }
    };
    put("joker-request-id", request_id);
    put("joker-api-version", API_VERSION);
    put("joker-clock", &info.clock);
    if let Some(g) = &info.generation {
        put("joker-generation", g);
    }
    h
}

fn problem_response(backend: &dyn Backend, problem: &Problem) -> Response {
    let id = request_id();
    let mut headers = base_headers(backend, &id);
    headers.insert("content-type", HeaderValue::from_static(PROBLEM_JSON));
    let status = StatusCode::from_u16(problem.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let body = problem.body(Some(&id)).to_string();
    (status, headers, body).into_response()
}

fn reply_response(backend: &dyn Backend, reply: Reply) -> Response {
    let id = request_id();
    let mut headers = base_headers(backend, &id);
    headers.insert("content-type", HeaderValue::from_static("application/json"));
    if let Some(etag) = reply
        .etag
        .as_deref()
        .and_then(|e| HeaderValue::from_str(&format!("\"{e}\"")).ok())
    {
        headers.insert("etag", etag);
    }
    for (name, value) in &reply.headers {
        if let Ok(value) = HeaderValue::from_str(value) {
            headers.insert(*name, value);
        }
    }
    let status = StatusCode::from_u16(reply.status).unwrap_or(StatusCode::OK);
    (status, headers, reply.body.to_string()).into_response()
}

async fn run<F>(backend: Shared, f: F) -> Response
where
    F: FnOnce(&dyn Backend) -> Result<Reply, Problem> + Send + 'static,
{
    let b = Arc::clone(&backend);
    let result = tokio::task::spawn_blocking(move || f(b.as_ref()))
        .await
        .unwrap_or_else(|_| Err(Problem::new("J-E1000", 500, "the handler panicked")));
    match result {
        Ok(reply) => reply_response(backend.as_ref(), reply),
        Err(p) => problem_response(backend.as_ref(), &p),
    }
}

/// Parses a query string strictly: every name must be in `allowed`
/// (`J-E1011`).
pub fn parse_query(raw: Option<&str>, allowed: &[&str]) -> Result<Query, Problem> {
    let mut out = Vec::new();
    for (k, v) in form_urlencoded::parse(raw.unwrap_or("").as_bytes()) {
        if !allowed.contains(&k.as_ref()) && !k.starts_with("x_") {
            return Err(
                Problem::new("J-E1011", 400, format!("unknown parameter `{k}`"))
                    .with("param", Value::String(k.into_owned())),
            );
        }
        out.push((k.into_owned(), v.into_owned()));
    }
    Ok(out)
}

/// Decodes a JSON body strictly (ref/09 §3.1): malformed JSON is
/// `J-E1010`, an unknown field `J-E1011`, a missing field `J-E1013`, a bad
/// value `J-E1012`. Top-level `x_*` fields are dropped.
pub fn decode<T: DeserializeOwned>(body: &[u8]) -> Result<T, Problem> {
    let mut value: Value = if body.is_empty() {
        Value::Object(Default::default())
    } else {
        serde_json::from_slice(body).map_err(|e| Problem::new("J-E1010", 400, e.to_string()))?
    };
    if let Value::Object(m) = &mut value {
        m.retain(|k, _| !k.starts_with("x_"));
    }
    serde_json::from_value(value).map_err(|e| {
        let text = e.to_string();
        let code = if text.starts_with("unknown field") {
            "J-E1011"
        } else if text.starts_with("missing field") {
            "J-E1013"
        } else {
            "J-E1012"
        };
        Problem::new(code, 400, text)
    })
}

fn header<'h>(headers: &'h HeaderMap, name: &str) -> Option<&'h str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

macro_rules! get_query {
    ($name:ident, $method:ident, [$($allowed:expr),*]) => {
        async fn $name(State(b): State<Shared>, RawQuery(raw): RawQuery) -> Response {
            match parse_query(raw.as_deref(), &[$($allowed),*]) {
                Ok(q) => run(b, move |b| b.$method(&q)).await,
                Err(p) => problem_response(b.as_ref(), &p),
            }
        }
    };
}

get_query!(health, health, ["probe"]);
get_query!(
    signals,
    signals,
    [
        "sel",
        "direction",
        "origin",
        "device",
        "instance",
        "shape",
        "values",
        "mono",
        "limit",
        "cursor"
    ]
);
get_query!(values, values, ["sel", "coherent", "mono"]);
get_query!(
    outputs,
    outputs,
    ["sel", "tripped", "manual", "limit", "cursor"]
);
get_query!(manual, manual, ["sel"]);
get_query!(
    decisions,
    decisions,
    [
        "output", "cause", "writer", "outcome", "kind", "from", "to", "since", "order", "limit",
        "cursor"
    ]
);
get_query!(generations, generations, ["state", "limit", "cursor"]);
get_query!(trips, trips, ["sel"]);
get_query!(actions, actions, ["target"]);

async fn info(State(b): State<Shared>, ConnectInfo(peer): ConnectInfo<Peer>) -> Response {
    let who = principal(peer);
    run(b, move |b| b.info(&who)).await
}

async fn status(State(b): State<Shared>, RawQuery(raw): RawQuery) -> Response {
    match parse_query(raw.as_deref(), &["instances"]) {
        Ok(_) => run(b, |b| b.status()).await,
        Err(p) => problem_response(b.as_ref(), &p),
    }
}

async fn metrics(State(b): State<Shared>, Extension(engine): Extension<ws::Engine>) -> Response {
    let stats = Arc::clone(&engine.stats);
    run(b, move |b| {
        b.metrics().map(|mut r| {
            r.body = ws::with_api(r.body, &stats);
            r
        })
    })
    .await
}

async fn signal(State(b): State<Shared>, UrlPath(path): UrlPath<String>) -> Response {
    run(b, move |b| b.signal(&path)).await
}

async fn output(State(b): State<Shared>, UrlPath(path): UrlPath<String>) -> Response {
    run(b, move |b| b.output(&path)).await
}

async fn manual_write(
    State(b): State<Shared>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    UrlPath(path): UrlPath<String>,
    body: Bytes,
) -> Response {
    match decode(&body) {
        Ok(req) => {
            let who = principal(peer);
            run(b, move |b| b.manual_write(&who, &path, req)).await
        }
        Err(p) => problem_response(b.as_ref(), &p),
    }
}

async fn release(
    State(b): State<Shared>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    UrlPath(path): UrlPath<String>,
    body: Bytes,
) -> Response {
    match decode(&body) {
        Ok(req) => {
            let who = principal(peer);
            run(b, move |b| b.release(&who, &path, req)).await
        }
        Err(p) => problem_response(b.as_ref(), &p),
    }
}

async fn reset_trip(
    State(b): State<Shared>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    UrlPath(path): UrlPath<String>,
    body: Bytes,
) -> Response {
    match decode(&body) {
        Ok(req) => {
            let who = principal(peer);
            run(b, move |b| b.reset_trip(&who, &path, req)).await
        }
        Err(p) => problem_response(b.as_ref(), &p),
    }
}

async fn write_ticket(State(b): State<Shared>, UrlPath(id): UrlPath<String>) -> Response {
    run(b, move |b| b.write_ticket(&id)).await
}

async fn reset_trips(
    State(b): State<Shared>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    body: Bytes,
) -> Response {
    match decode(&body) {
        Ok(req) => {
            let who = principal(peer);
            run(b, move |b| b.reset_trips(&who, req)).await
        }
        Err(p) => problem_response(b.as_ref(), &p),
    }
}

async fn invoke_action(
    State(b): State<Shared>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    UrlPath((target, action)): UrlPath<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let key = header(&headers, "idempotency-key").map(str::to_owned);
    if key.is_none() {
        return problem_response(
            b.as_ref(),
            &Problem::new("J-E1023", 400, "`Idempotency-Key` is required"),
        );
    }
    match decode(&body) {
        Ok(req) => {
            let who = principal(peer);
            run(b, move |b| {
                b.invoke_action(&who, &target, &action, key.as_deref(), req)
            })
            .await
        }
        Err(p) => problem_response(b.as_ref(), &p),
    }
}

async fn invocations(State(b): State<Shared>, RawQuery(raw): RawQuery) -> Response {
    match parse_query(
        raw.as_deref(),
        &["target", "state", "since", "from", "to", "limit", "cursor"],
    ) {
        Ok(q) => run(b, move |b| b.invocations(&q)).await,
        Err(p) => problem_response(b.as_ref(), &p),
    }
}

async fn invocation(State(b): State<Shared>, UrlPath(id): UrlPath<String>) -> Response {
    run(b, move |b| b.invocation(&id)).await
}

async fn cancel_invocation(
    State(b): State<Shared>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    UrlPath(id): UrlPath<String>,
    body: Bytes,
) -> Response {
    match decode(&body) {
        Ok(req) => {
            let who = principal(peer);
            run(b, move |b| b.cancel_invocation(&who, &id, req)).await
        }
        Err(p) => problem_response(b.as_ref(), &p),
    }
}

async fn create_generation(
    State(b): State<Shared>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    RawQuery(raw): RawQuery,
    body: Bytes,
) -> Response {
    let q = match parse_query(raw.as_deref(), &["dry_run"]) {
        Ok(q) => q,
        Err(p) => return problem_response(b.as_ref(), &p),
    };
    let dry_run = q.iter().any(|(k, v)| k == "dry_run" && v == "true");
    match decode(&body) {
        Ok(req) => {
            let who = principal(peer);
            run(b, move |b| b.create_generation(&who, req, dry_run)).await
        }
        Err(p) => problem_response(b.as_ref(), &p),
    }
}

/// The largest blob of a monolithic upload (ref/09 §5.13).
pub const MAX_UPLOAD: usize = 8 << 20;

/// The largest request body (ref/00 §11).
pub const MAX_BODY: usize = 16 << 20;

async fn missing_blobs(State(b): State<Shared>, body: Bytes) -> Response {
    match decode(&body) {
        Ok(req) => run(b, move |b| b.missing_blobs(req)).await,
        Err(p) => problem_response(b.as_ref(), &p),
    }
}

async fn blob_head(State(b): State<Shared>, UrlPath(digest): UrlPath<String>) -> Response {
    let backend = Arc::clone(&b);
    let result = tokio::task::spawn_blocking(move || backend.has_blob(&digest))
        .await
        .unwrap_or_else(|_| Err(Problem::new("J-E1000", 500, "the handler panicked")));
    let headers = base_headers(b.as_ref(), &request_id());
    match result {
        Ok(true) => (StatusCode::OK, headers).into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, headers).into_response(),
        Err(p) => problem_response(b.as_ref(), &p),
    }
}

async fn upload_blob(
    State(b): State<Shared>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    RawQuery(raw): RawQuery,
    body: Bytes,
) -> Response {
    let q = match parse_query(raw.as_deref(), &["digest", "kind"]) {
        Ok(q) => q,
        Err(p) => return problem_response(b.as_ref(), &p),
    };
    let value = |name: &str| q.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
    let Some(digest) = value("digest") else {
        return problem_response(
            b.as_ref(),
            &Problem::new(
                "J-E1012",
                400,
                "this node takes monolithic uploads: `?digest=` with the bytes (at most 8 MiB)",
            )
            .with("pointer", serde_json::json!("/digest")),
        );
    };
    if body.len() > MAX_UPLOAD {
        return problem_response(
            b.as_ref(),
            &Problem::new("J-E1606", 413, "a monolithic upload carries at most 8 MiB"),
        );
    }
    let kind = value("kind");
    let who = principal(peer);
    let location = format!("/v1/blobs/{digest}");
    let mut response = run(b, move |b| {
        b.upload_blob(&who, &digest, kind.as_deref(), &body)
    })
    .await;
    if response.status() == StatusCode::CREATED
        && let Ok(v) = HeaderValue::from_str(&location)
    {
        response.headers_mut().insert("location", v);
    }
    response
}

async fn generation(State(b): State<Shared>, UrlPath(sel): UrlPath<String>) -> Response {
    run(b, move |b| b.generation(&sel)).await
}

async fn activate(
    State(b): State<Shared>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    UrlPath(sel): UrlPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let if_match = header(&headers, "if-match").map(|s| s.trim_matches('"').to_owned());
    match decode(&body) {
        Ok(req) => {
            let who = principal(peer);
            run(b, move |b| b.activate(&who, &sel, if_match.as_deref(), req)).await
        }
        Err(p) => problem_response(b.as_ref(), &p),
    }
}

async fn confirm(
    State(b): State<Shared>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    UrlPath(sel): UrlPath<String>,
    body: Bytes,
) -> Response {
    match decode(&body) {
        Ok(req) => {
            let who = principal(peer);
            run(b, move |b| b.confirm(&who, &sel, req)).await
        }
        Err(p) => problem_response(b.as_ref(), &p),
    }
}

async fn rollback(
    State(b): State<Shared>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let if_match = header(&headers, "if-match").map(|s| s.trim_matches('"').to_owned());
    match decode(&body) {
        Ok(req) => {
            let who = principal(peer);
            run(b, move |b| b.rollback(&who, if_match.as_deref(), req)).await
        }
        Err(p) => problem_response(b.as_ref(), &p),
    }
}

async fn traces(State(b): State<Shared>) -> Response {
    run(b, |b| b.traces()).await
}

async fn create_trace(
    State(b): State<Shared>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    body: Bytes,
) -> Response {
    match decode(&body) {
        Ok(req) => {
            let who = principal(peer);
            run(b, move |b| b.create_trace(&who, req)).await
        }
        Err(p) => problem_response(b.as_ref(), &p),
    }
}

async fn trace(State(b): State<Shared>, UrlPath(id): UrlPath<String>) -> Response {
    run(b, move |b| b.trace(&id)).await
}

async fn trace_file(State(b): State<Shared>, UrlPath(id): UrlPath<String>) -> Response {
    let backend = Arc::clone(&b);
    let result = tokio::task::spawn_blocking(move || backend.trace_file(&id))
        .await
        .unwrap_or_else(|_| Err(Problem::new("J-E1000", 500, "the handler panicked")));
    match result {
        Ok(bytes) => {
            let mut headers = base_headers(b.as_ref(), &request_id());
            headers.insert(
                "content-type",
                HeaderValue::from_static("application/vnd.joker.trace.v1"),
            );
            (StatusCode::OK, headers, Body::from(bytes)).into_response()
        }
        Err(p) => problem_response(b.as_ref(), &p),
    }
}

async fn not_found(State(b): State<Shared>) -> Response {
    problem_response(
        b.as_ref(),
        &Problem::new("J-E1005", 404, "no such endpoint"),
    )
}

/// The router of the M0 subset.
pub fn router(backend: Arc<dyn Backend>) -> Router {
    router_with(backend, ws::Limits::default())
}

/// The router with the given subscription limits instead of those of ref/09
/// (tests use short ones).
pub fn router_with(backend: Arc<dyn Backend>, limits: ws::Limits) -> Router {
    let engine = ws::Engine {
        limits,
        stats: Arc::default(),
    };
    Router::new()
        .route("/v1/info", get(info))
        .route("/v1/health", get(health))
        .route("/v1/status", get(status))
        .route("/v1/metrics", get(metrics))
        .route("/v1/signals", get(signals))
        .route("/v1/signals/{path}", get(signal))
        .route("/v1/values", get(values))
        .route("/v1/outputs", get(outputs))
        .route("/v1/outputs/{path}", get(output))
        .route("/v1/outputs/{path}/manual", post(manual_write))
        .route("/v1/outputs/{path}/release", post(release))
        .route("/v1/outputs/{path}/reset", post(reset_trip))
        .route("/v1/manual", get(manual))
        .route("/v1/writes/{write}", get(write_ticket))
        .route("/v1/trips", get(trips))
        .route("/v1/trips/reset", post(reset_trips))
        .route("/v1/actions", get(actions))
        .route("/v1/actions/{target}/{action}", post(invoke_action))
        .route("/v1/invocations", get(invocations))
        .route("/v1/invocations/{invocation}", get(invocation))
        .route(
            "/v1/invocations/{invocation}/cancel",
            post(cancel_invocation),
        )
        .route("/v1/decisions", get(decisions))
        .route("/v1/blobs/missing", post(missing_blobs))
        .route("/v1/blobs/{digest}", head(blob_head))
        .route(
            "/v1/blobs/uploads",
            post(upload_blob).layer(DefaultBodyLimit::max(MAX_BODY)),
        )
        .route("/v1/generations", get(generations).post(create_generation))
        .route("/v1/generations/rollback", post(rollback))
        .route("/v1/generations/{gen}", get(generation))
        .route("/v1/generations/{gen}/activate", post(activate))
        .route("/v1/generations/{gen}/confirm", post(confirm))
        .route("/v1/traces", get(traces).post(create_trace))
        .route("/v1/traces/{trace}", get(trace))
        .route("/v1/traces/{trace}/file", get(trace_file))
        .route("/v1/ws", get(ws::ws_handler))
        .route("/v1/stream", get(ws::sse_handler))
        .fallback(not_found)
        .layer(Extension(engine))
        .with_state(backend)
}

/// Serves the API on a Unix socket until `shutdown` resolves. The socket
/// is created with mode 0660: access is the file's permissions (ref/09
/// §2.1).
pub async fn serve_unix(
    socket: &Path,
    backend: Arc<dyn Backend>,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let listener = bind_unix(socket)?;
    serve_listener(listener, backend, shutdown).await
}

/// Binds the API's Unix socket with mode 0660, replacing a stale socket
/// file, so that a caller can tell its readiness (`sd_notify READY=1`)
/// only once clients can connect. Must be called within a Tokio runtime.
pub fn bind_unix(socket: &Path) -> std::io::Result<UnixListener> {
    if socket.exists() {
        std::fs::remove_file(socket)?;
    }
    if let Some(dir) = socket.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let listener = UnixListener::bind(socket)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o660))?;
    }
    Ok(listener)
}

/// Serves the API on a socket bound by [`bind_unix`] until `shutdown`
/// resolves.
pub async fn serve_listener(
    listener: UnixListener,
    backend: Arc<dyn Backend>,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    serve_router(listener, router(backend), shutdown).await
}

/// Serves `app`, a router from [`router`] or [`router_with`], on a socket
/// bound by [`bind_unix`] until `shutdown` resolves.
pub async fn serve_router(
    listener: UnixListener,
    app: Router,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let app = app.into_make_service_with_connect_info::<Peer>();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
}

#[cfg(test)]
mod role_tests {
    use super::SocketRoles;

    fn member_of(groups: &'static [&'static str]) -> impl Fn(&str) -> bool {
        move |g| groups.contains(&g)
    }

    #[test]
    fn without_a_generation_the_prod_defaults_hold() {
        let roles = SocketRoles::default();
        assert_eq!(roles.local_role("ana", &member_of(&[])), "viewer");
        assert_eq!(
            roles.local_role("ana", &member_of(&["joker-ops"])),
            "operator"
        );
        assert_eq!(
            roles.local_role("ana", &member_of(&["joker-admin"])),
            "admin"
        );
        assert_eq!(roles.local_role("joker", &member_of(&[])), "admin");
    }

    #[test]
    fn declared_rules_replace_the_defaults_of_their_role() {
        let roles = SocketRoles {
            dev: false,
            operator: Some(vec!["local:user=ana".into()]),
            admin: Some(vec!["local:group=deploy".into()]),
        };
        assert_eq!(roles.local_role("ana", &member_of(&[])), "operator");
        assert_eq!(
            roles.local_role("rui", &member_of(&["joker-ops"])),
            "viewer"
        );
        assert_eq!(roles.local_role("rui", &member_of(&["deploy"])), "admin");
    }

    #[test]
    fn dev_without_roles_makes_anyone_admin() {
        let dev = SocketRoles {
            dev: true,
            ..SocketRoles::default()
        };
        assert_eq!(dev.local_role("ana", &member_of(&[])), "admin");
        let declared = SocketRoles {
            dev: true,
            operator: Some(Vec::new()),
            admin: None,
        };
        assert_eq!(declared.local_role("ana", &member_of(&[])), "viewer");
    }
}
