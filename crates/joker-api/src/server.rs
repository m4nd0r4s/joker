//! The HTTP server over the Unix socket (ref/09 §2.1): routes, strict
//! requests, common headers and problems.

use std::path::Path;
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::connect_info::{ConnectInfo, Connected};
use axum::extract::{Path as UrlPath, RawQuery, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::serve::IncomingStream;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::net::UnixListener;

use crate::backend::{Backend, Principal, Query, Reply};
use crate::problem::{PROBLEM_JSON, Problem};

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

fn principal(peer: Peer) -> Principal {
    let name = peer.uid.map(user_name).unwrap_or_else(|| "unknown".into());
    Principal {
        id: format!("unix:{name}"),
        transport: "unix",
        uid: peer.uid,
    }
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

type Shared = Arc<dyn Backend>;

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

async fn write_ticket(State(b): State<Shared>, UrlPath(id): UrlPath<String>) -> Response {
    run(b, move |b| b.write_ticket(&id)).await
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
    Router::new()
        .route("/v1/info", get(info))
        .route("/v1/health", get(health))
        .route("/v1/status", get(status))
        .route("/v1/signals", get(signals))
        .route("/v1/signals/{path}", get(signal))
        .route("/v1/values", get(values))
        .route("/v1/outputs", get(outputs))
        .route("/v1/outputs/{path}", get(output))
        .route("/v1/outputs/{path}/manual", post(manual_write))
        .route("/v1/outputs/{path}/release", post(release))
        .route("/v1/manual", get(manual))
        .route("/v1/writes/{write}", get(write_ticket))
        .route("/v1/actions/{target}/{action}", post(invoke_action))
        .route("/v1/decisions", get(decisions))
        .route("/v1/generations", get(generations).post(create_generation))
        .route("/v1/generations/rollback", post(rollback))
        .route("/v1/generations/{gen}", get(generation))
        .route("/v1/generations/{gen}/activate", post(activate))
        .route("/v1/generations/{gen}/confirm", post(confirm))
        .route("/v1/traces", get(traces).post(create_trace))
        .route("/v1/traces/{trace}", get(trace))
        .route("/v1/traces/{trace}/file", get(trace_file))
        .fallback(not_found)
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
    let app = router(backend).into_make_service_with_connect_info::<Peer>();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
}
