//! Contract tests of the M0 API subset: every response of every operation
//! the daemon serves is validated against `spec/openapi/joker-api.yaml`
//! (ref/09, ADR 0023).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use joker_core::ClockQuality;
use jokerd::api::Node;
use jokerd::control::{Config, Controller};
use jokerd::decisions::DecisionLog;
use jokerd::devices::{Devices, NoDevices};
use jokerd::native::NativeTasks;
use jokerd::pacer::{Monotonic, SystemPacer};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Contract {
    doc: Value,
}

impl Contract {
    fn load() -> Self {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/openapi/joker-api.yaml");
        let text = std::fs::read_to_string(path).unwrap();
        let docs = yaml_rust2::YamlLoader::load_from_str(&text).unwrap();
        Self {
            doc: to_json(&docs[0]),
        }
    }

    fn resolve<'a>(&'a self, v: &'a Value) -> &'a Value {
        match v.get("$ref").and_then(Value::as_str) {
            Some(r) => {
                let pointer = r.trim_start_matches('#');
                self.doc.pointer(pointer).unwrap_or(&Value::Null)
            }
            None => v,
        }
    }

    fn check(&self, method: &str, template: &str, status: u16, body: &Value) {
        let op = &self.doc["paths"][template][method];
        assert!(op.is_object(), "{method} {template} is not in the contract");
        let responses = &op["responses"];
        let response = responses
            .get(status.to_string())
            .or_else(|| responses.get("default"))
            .unwrap_or_else(|| panic!("{method} {template}: status {status} is not documented"));
        let response = self.resolve(response);
        let content = response["content"].as_object().unwrap();
        let schema = content.values().next().unwrap()["schema"].clone();
        let mut root = json!({"$schema": "https://json-schema.org/draft/2020-12/schema"});
        if let Some(m) = schema.as_object() {
            for (k, v) in m {
                root[k] = v.clone();
            }
        }
        root["components"] = self.doc["components"].clone();
        let validator = jsonschema::validator_for(&root).unwrap();
        let errors: Vec<String> = validator
            .iter_errors(body)
            .map(|e| format!("{} at {}", e, e.instance_path))
            .collect();
        assert!(
            errors.is_empty(),
            "{method} {template} {status}: {errors:#?}\n{}",
            serde_json::to_string_pretty(body).unwrap()
        );
    }
}

fn to_json(y: &yaml_rust2::Yaml) -> Value {
    use yaml_rust2::Yaml;
    match y {
        Yaml::Real(r) => r.parse::<f64>().map_or(Value::Null, |f| json!(f)),
        Yaml::Integer(i) => json!(i),
        Yaml::String(s) => json!(s),
        Yaml::Boolean(b) => json!(b),
        Yaml::Array(a) => Value::Array(a.iter().map(to_json).collect()),
        Yaml::Hash(h) => Value::Object(
            h.iter()
                .map(|(k, v)| {
                    let key = match k {
                        Yaml::String(s) => s.clone(),
                        Yaml::Integer(i) => i.to_string(),
                        Yaml::Boolean(b) => b.to_string(),
                        other => format!("{other:?}"),
                    };
                    (key, to_json(v))
                })
                .collect(),
        ),
        _ => Value::Null,
    }
}

struct Http {
    socket: PathBuf,
}

struct Response {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Response {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

impl Http {
    async fn send(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: Option<&Value>,
    ) -> Response {
        let payload = body.map(|b| b.to_string());
        self.exchange(
            method,
            path,
            headers,
            payload
                .as_deref()
                .map(|p| ("application/json", p.as_bytes())),
        )
        .await
    }

    async fn exchange(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: Option<(&str, &[u8])>,
    ) -> Response {
        let mut stream = tokio::net::UnixStream::connect(&self.socket).await.unwrap();
        let mut req = format!("{method} {path} HTTP/1.1\r\nhost: joker\r\nconnection: close\r\n");
        for (k, v) in headers {
            req.push_str(&format!("{k}: {v}\r\n"));
        }
        if let Some((kind, bytes)) = body {
            req.push_str(&format!(
                "content-type: {kind}\r\ncontent-length: {}\r\n",
                bytes.len()
            ));
        }
        req.push_str("\r\n");
        let mut raw = req.into_bytes();
        if let Some((_, bytes)) = body {
            raw.extend_from_slice(bytes);
        }
        stream.write_all(&raw).await.unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).await.unwrap();
        let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let head = String::from_utf8_lossy(&raw[..split]).to_string();
        let mut lines = head.lines();
        let status = lines
            .next()
            .unwrap()
            .split(' ')
            .nth(1)
            .unwrap()
            .parse()
            .unwrap();
        let headers: Vec<(String, String)> = lines
            .filter_map(|l| l.split_once(": "))
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();
        let mut body = raw[split + 4..].to_vec();
        if headers
            .iter()
            .any(|(k, v)| k.eq_ignore_ascii_case("transfer-encoding") && v == "chunked")
        {
            body = dechunk(&body);
        }
        Response {
            status,
            headers,
            body,
        }
    }
}

fn dechunk(mut b: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let Some(pos) = b.windows(2).position(|w| w == b"\r\n") else {
            return out;
        };
        let size =
            usize::from_str_radix(std::str::from_utf8(&b[..pos]).unwrap().trim(), 16).unwrap();
        if size == 0 {
            return out;
        }
        out.extend_from_slice(&b[pos + 2..pos + 2 + size]);
        b = &b[pos + 4 + size..];
    }
}

fn effective() -> String {
    format!(
        "{}\n[devices.pi]\ndriver = \"sim\"\n\n[devices.pi.signals]\nact_led = {{ type = \"bool\", direction = \"output\" }}\n",
        common::JOKER_TOML
    )
}

/// A numeric output that trips on an out-of-range write (docs/16 E7),
/// for `jk reset`'s own contract test — self-contained (not built on
/// [`effective`], since TOML cannot repeat a `[devices.pi.signals]`
/// table header).
fn effective_with_trippable_output() -> String {
    "[project]\n\
     name  = \"reset-test\"\n\
     joker = \"1.0\"\n\
     mode  = \"dev\"\n\
     \n\
     [outputs.\"pi.p_set\"]\n\
     safe     = \"0 kW\"\n\
     range    = [\"-100 kW\", \"100 kW\"]\n\
     on_range = \"trip\"\n\
     \n\
     [devices.pi]\n\
     driver = \"sim\"\n\
     \n\
     [devices.pi.signals]\n\
     p_set = { type = \"f64\", direction = \"output\", unit = \"kW\" }\n"
        .to_owned()
}

struct Fixture {
    http: Http,
    contract: Contract,
    _dir: tempfile::TempDir,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    logs: Arc<jokerd::logs::LogRing>,
}

async fn start() -> Fixture {
    start_with(&effective()).await
}

async fn start_with(manifest: &str) -> Fixture {
    start_with_devices(manifest, Arc::new(NoDevices)).await
}

async fn start_with_devices(manifest: &str, devices: Arc<dyn Devices>) -> Fixture {
    start_with_tasks(manifest, devices, Arc::new(NativeTasks)).await
}

async fn start_with_tasks(
    manifest: &str,
    devices: Arc<dyn Devices>,
    tasks: Arc<dyn jokerd::task::TaskProvider>,
) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let clock = Monotonic::start();
    let log = Arc::new(Mutex::new(
        DecisionLog::open(&dir.path().join("decisions.jsonl")).unwrap(),
    ));
    let mut c = Controller::new(
        dir.path(),
        log,
        tasks,
        devices,
        Box::new(move || Box::new(SystemPacer::new(clock, ClockQuality::Unsynced))),
        Config {
            min_window: Duration::from_millis(100),
            ..Config::default()
        },
    )
    .unwrap();
    let g = c
        .propose(manifest, "", Some("bench".into()), false)
        .unwrap();
    c.activate(g.number, None, None).unwrap();
    let logs = c.logs();
    let controller = Arc::new(Mutex::new(c));
    let node = Node::new(controller, clock, 0, dir.path()).unwrap();
    let socket = dir.path().join("api.sock");
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let path = socket.clone();
    tokio::spawn(async move {
        joker_api::server::serve_unix(&path, Arc::new(node), async {
            let _ = rx.await;
        })
        .await
        .unwrap();
    });
    for _ in 0..100 {
        if socket.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    Fixture {
        http: Http { socket },
        contract: Contract::load(),
        _dir: dir,
        stop: Some(tx),
        logs,
    }
}

impl Fixture {
    async fn call(
        &self,
        method: &str,
        template: &str,
        path: &str,
        body: Option<Value>,
    ) -> Response {
        self.call_with(method, template, path, &[], body).await
    }

    async fn call_with(
        &self,
        method: &str,
        template: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: Option<Value>,
    ) -> Response {
        let r = self.http.send(method, path, headers, body.as_ref()).await;
        if r.header("content-type").is_some_and(|c| c.contains("json")) {
            self.contract
                .check(&method.to_lowercase(), template, r.status, &r.json());
        }
        assert!(r.header("joker-request-id").is_some());
        assert_eq!(r.header("joker-api-version"), Some("1.0.0"));
        r
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(tx) = self.stop.take() {
            let _ = tx.send(());
        }
    }
}

/// The decision-log entries of `dir` from line `from` on.
fn decisions_from(dir: &Path, from: usize) -> Vec<Value> {
    DecisionLog::read_all(&dir.join("decisions.jsonl"))
        .unwrap()
        .into_iter()
        .skip(from)
        .collect()
}

/// The first `output` record of `output` among `entries`.
fn first_output_record<'a>(entries: &'a [Value], output: &str) -> &'a Value {
    entries
        .iter()
        .find(|e| e["record"] == "output" && e["output"] == output)
        .unwrap_or_else(|| panic!("no output record of {output} in {entries:?}"))
}

/// A daemon booted by [`boot_at`].
struct Halt {
    stop: tokio::sync::oneshot::Sender<()>,
    controller: jokerd::daemon::SharedController,
}

impl Halt {
    /// Stops the API and the running generation, as the end of the
    /// process would.
    fn halt(self) {
        let _ = self.stop.send(());
        jokerd::daemon::lock_controller(&self.controller).shutdown();
    }
}

/// Boots a daemon over `dir` the way `jokerd::daemon::run` does (`boot()`
/// reactivates a confirmed generation if one is stored, else `manifest`
/// is proposed and activated fresh) — used to simulate a process restart
/// over the same state directory (A14).
async fn boot_at(dir: &Path, manifest: &str) -> (Http, Halt) {
    let clock = Monotonic::start();
    let log = Arc::new(Mutex::new(
        DecisionLog::open(&dir.join("decisions.jsonl")).unwrap(),
    ));
    let mut c = Controller::new(
        dir,
        log,
        Arc::new(NativeTasks),
        Arc::new(NoDevices),
        Box::new(move || Box::new(SystemPacer::new(clock, ClockQuality::Unsynced))),
        Config {
            min_window: Duration::from_millis(100),
            ..Config::default()
        },
    )
    .unwrap();
    if c.boot().unwrap().is_none() {
        let g = c
            .propose(manifest, "", Some("bench".into()), false)
            .unwrap();
        c.activate(g.number, None, None).unwrap();
    }
    let controller = Arc::new(Mutex::new(c));
    let node = Node::new(Arc::clone(&controller), clock, 0, dir).unwrap();
    let socket = dir.join("api.sock");
    let _ = std::fs::remove_file(&socket);
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let path = socket.clone();
    tokio::spawn(async move {
        joker_api::server::serve_unix(&path, Arc::new(node), async {
            let _ = rx.await;
        })
        .await
        .unwrap();
    });
    for _ in 0..100 {
        if socket.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    (
        Http { socket },
        Halt {
            stop: tx,
            controller,
        },
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn meta_signals_and_values_follow_the_contract() {
    let f = start().await;
    let info = f.call("GET", "/v1/info", "/v1/info", None).await;
    assert_eq!(info.status, 200);
    assert_eq!(info.json()["mode"], "dev");
    let gen_header = info.header("joker-generation").unwrap().to_owned();
    assert!(gen_header.starts_with("1 sha256:"));
    let h = f
        .call("GET", "/v1/health", "/v1/health?probe=ready", None)
        .await;
    assert_eq!(h.status, 200);
    let s = f.call("GET", "/v1/status", "/v1/status", None).await;
    assert_eq!(s.json()["groups"][0]["name"], "main");
    let m = f.call("GET", "/v1/metrics", "/v1/metrics", None).await;
    assert_eq!(m.status, 200, "{}", m.json());
    assert_eq!(m.json()["groups"][0]["group"], "main");
    assert!(m.json()["groups"][0]["ticks"].as_u64().unwrap() > 0);
    assert_eq!(m.json()["groups"][0]["consecutive_overruns"], 0);
    assert!(
        m.json().get("ml").is_some_and(serde_json::Value::is_null),
        "ref/09 §5.2: `ml` is null in 1.0"
    );
    #[cfg(target_os = "linux")]
    for field in ["disk_free", "memory_used"] {
        let text = m.json()["system"][field]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let kib: u64 = text
            .strip_suffix(" KiB")
            .and_then(|n| n.parse().ok())
            .unwrap_or_else(|| panic!("{field}: {text:?}"));
        assert!(kib > 0, "{field}: {text}");
    }
    let sig = f
        .call("GET", "/v1/signals", "/v1/signals?sel=pi.**", None)
        .await;
    assert_eq!(sig.json()["items"][0]["path"], "pi.act_led");
    let one = f
        .call("GET", "/v1/signals/{path}", "/v1/signals/pi.act_led", None)
        .await;
    assert_eq!(one.json()["direction"], "output");
    let missing = f
        .call("GET", "/v1/signals/{path}", "/v1/signals/pi.nope", None)
        .await;
    assert_eq!(missing.status, 404);
    let v = f
        .call("GET", "/v1/values", "/v1/values?sel=pi.act_led", None)
        .await;
    assert_eq!(v.json()["items"][0]["quality"], "good");
    let strict = f
        .call(
            "GET",
            "/v1/values",
            "/v1/values?sel=pi.act_led&bogus=1",
            None,
        )
        .await;
    assert_eq!(
        (strict.status, strict.json()["code"].clone()),
        (400, json!("J-E1011"))
    );
    let no_sel = f.call("GET", "/v1/values", "/v1/values", None).await;
    assert_eq!(no_sel.status, 400);
    let nope = f.http.send("GET", "/v1/nope", &[], None).await;
    assert_eq!(
        (nope.status, nope.json()["code"].clone()),
        (404, json!("J-E1005"))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn logs_are_served_from_the_ring_with_their_filters() {
    use jokerd::logs::LogLevel;
    let f = start().await;
    let now = jokerd::wire::utc_now_ns();
    f.logs
        .push(LogLevel::Info, "jokerd", "started", Vec::new(), now - 2);
    f.logs.push(
        LogLevel::Warn,
        "driver:pcs",
        "timeout",
        vec![("batch".into(), "3".into())],
        now - 1,
    );
    f.logs
        .push(LogLevel::Debug, "driver:pcs", "poll", Vec::new(), now);

    let all = f.call("GET", "/v1/logs", "/v1/logs", None).await;
    assert_eq!(all.status, 200, "{}", all.json());
    let msgs = |r: &Response| -> Vec<String> {
        r.json()["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["msg"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(msgs(&all), ["poll", "timeout", "started"]);
    let warn = &all.json()["items"][1];
    assert_eq!(warn["level"], "warn");
    assert_eq!(warn["source"], "driver:pcs");
    assert_eq!(warn["fields"]["batch"], "3");

    let pcs = f
        .call(
            "GET",
            "/v1/logs",
            "/v1/logs?source=driver:pcs&level=info",
            None,
        )
        .await;
    assert_eq!(msgs(&pcs), ["timeout"]);

    let page = f
        .call("GET", "/v1/logs", "/v1/logs?order=asc&limit=2", None)
        .await;
    assert_eq!(msgs(&page), ["started", "timeout"]);
    let cursor = page.json()["next_cursor"].as_str().unwrap().to_owned();
    let rest = f
        .call(
            "GET",
            "/v1/logs",
            &format!("/v1/logs?order=asc&limit=2&cursor={cursor}"),
            None,
        )
        .await;
    assert_eq!(msgs(&rest), ["poll"]);
    assert!(rest.json()["next_cursor"].is_null());

    let bad = f.call("GET", "/v1/logs", "/v1/logs?level=loud", None).await;
    assert_eq!(bad.json()["code"], "J-E1012");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn manual_writes_follow_a10_and_a13() {
    let f = start().await;
    let path = "/v1/outputs/pi.act_led/manual";
    let tpl = "/v1/outputs/{path}/manual";
    let no_ttl = f
        .call(
            "POST",
            tpl,
            path,
            Some(json!({"value": true, "reason": "x"})),
        )
        .await;
    assert_eq!(no_ttl.json()["code"], "J-E1101");
    let short = f
        .call(
            "POST",
            tpl,
            path,
            Some(json!({"value": true, "ttl": "500ms", "reason": "x"})),
        )
        .await;
    assert_eq!(short.json()["code"], "J-E1103");
    let long = f
        .call(
            "POST",
            tpl,
            path,
            Some(json!({"value": true, "ttl": "9h", "reason": "x"})),
        )
        .await;
    assert_eq!(long.json()["code"], "J-E1102");
    let no_reason = f
        .call(
            "POST",
            tpl,
            path,
            Some(json!({"value": true, "ttl": "10s"})),
        )
        .await;
    assert_eq!(no_reason.json()["code"], "J-E1106");
    let level = f
        .call(
            "POST",
            tpl,
            path,
            Some(json!({"value": true, "ttl": "10s", "reason": "x", "level": "control:12"})),
        )
        .await;
    assert_eq!(level.json()["code"], "J-E1104");
    let wrong = f
        .call(
            "POST",
            tpl,
            path,
            Some(json!({"value": 3, "ttl": "10s", "reason": "x"})),
        )
        .await;
    assert_eq!(wrong.json()["code"], "J-E1036");
    let unknown = f
        .call(
            "POST",
            tpl,
            path,
            Some(json!({"value": true, "ttl": "10s", "reason": "x", "bogus": 1})),
        )
        .await;
    assert_eq!(unknown.json()["code"], "J-E1011");
    let not_output = f
        .call(
            "POST",
            tpl,
            "/v1/outputs/pi.nope/manual",
            Some(json!({"value": true, "ttl": "10s", "reason": "x"})),
        )
        .await;
    assert_eq!(not_output.json()["code"], "J-E1100");

    let ok = f
        .call(
            "POST",
            tpl,
            path,
            Some(json!({"value": true, "ttl": "10s", "reason": "commissioning"})),
        )
        .await;
    assert_eq!(ok.status, 200);
    let ticket = ok.json();
    assert_eq!(ticket["state"], "committed");
    assert_eq!(ticket["outcome"]["outcome"], "accepted");
    assert_eq!(ticket["level"], "manual:4");
    let again = f
        .call(
            "GET",
            "/v1/writes/{write}",
            &format!("/v1/writes/{}", ticket["id"].as_str().unwrap()),
            None,
        )
        .await;
    assert_eq!(again.json(), ticket);

    let out = f
        .call("GET", "/v1/outputs/{path}", "/v1/outputs/pi.act_led", None)
        .await;
    assert_eq!(out.json()["active_level"], "manual:4");
    assert_eq!(out.json()["applied"], true);
    let m = f.call("GET", "/v1/metrics", "/v1/metrics", None).await;
    let envelope = &m.json()["envelope"];
    let led = envelope
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["output"] == "pi.act_led")
        .unwrap_or_else(|| panic!("{envelope}"));
    assert!(led["accepted"].as_u64().unwrap() >= 1, "{led}");
    assert_eq!(led["rejected"], 0, "{led}");
    assert_eq!(m.json()["arbiter"]["manual_active"], 1, "{}", m.json());
    let list = f.call("GET", "/v1/manual", "/v1/manual", None).await;
    assert_eq!(list.json()["items"][0]["reason"], "commissioning");
    let outputs = f.call("GET", "/v1/outputs", "/v1/outputs", None).await;
    assert_eq!(outputs.json()["items"].as_array().unwrap().len(), 1);

    let renew = f
        .call(
            "POST",
            tpl,
            path,
            Some(json!({"value": false, "ttl": "20s", "reason": "renew"})),
        )
        .await;
    assert_eq!(renew.json()["renewed"], true);

    let release = f
        .call(
            "POST",
            "/v1/outputs/{path}/release",
            "/v1/outputs/pi.act_led/release",
            Some(json!({})),
        )
        .await;
    assert_eq!(release.status, 200);
    assert_eq!(release.json()["kind"], "release");
    let none = f
        .call(
            "POST",
            "/v1/outputs/{path}/release",
            "/v1/outputs/pi.act_led/release",
            Some(json!({})),
        )
        .await;
    assert_eq!(none.json()["code"], "J-E1107");

    let d = f
        .call(
            "GET",
            "/v1/decisions",
            "/v1/decisions?output=pi.act_led&limit=5",
            None,
        )
        .await;
    let items = d.json()["items"].as_array().unwrap().clone();
    assert!(!items.is_empty());
    assert!(items.iter().any(|i| i["active_level"] == "manual:4"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_device_write_ack_is_served_as_a_decision() {
    let f = start().await;
    let path = f._dir.path().join("decisions.jsonl");
    let mut log = DecisionLog::open(&path).unwrap();
    let mut entry = serde_json::Map::new();
    for (k, v) in [
        ("gen", json!(1)),
        ("record", json!("write_ack")),
        ("output", json!("pi.act_led")),
        ("device", json!("pi")),
        ("write_seq", json!(3)),
        ("kind", json!("set")),
        ("status", json!("device_rejected")),
        ("code", json!("J0877")),
        ("at_mono_ns", json!(5)),
        ("ts", json!("2026-10-24T12:00:00.000000000Z")),
    ] {
        entry.insert(k.into(), v);
    }
    log.append(entry, true).unwrap();

    let d = f
        .call(
            "GET",
            "/v1/decisions",
            "/v1/decisions?kind=write_ack&output=pi.act_led",
            None,
        )
        .await;
    assert_eq!(d.status, 200);
    let items = d.json()["items"].as_array().unwrap().clone();
    assert_eq!(items.len(), 1, "{items:?}");
    let ack = &items[0];
    assert_eq!(ack["kind"], "write_ack");
    assert_eq!(ack["output"], "pi.act_led");
    assert_eq!(ack["ack"]["device"], "pi");
    assert_eq!(ack["ack"]["write_seq"], 3);
    assert_eq!(ack["ack"]["write_kind"], "set");
    assert_eq!(ack["ack"]["status"], "device_rejected");
    assert_eq!(ack["ack"]["code"], "J0877");
    assert_eq!(ack["generation"]["gen"], 1);

    let by_outcome = f
        .call(
            "GET",
            "/v1/decisions",
            "/v1/decisions?outcome=applied",
            None,
        )
        .await;
    assert!(
        by_outcome.json()["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["kind"] != "write_ack"),
        "an ack has no outcome"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generations_follow_the_contract() {
    let f = start().await;
    let list = f
        .call("GET", "/v1/generations", "/v1/generations", None)
        .await;
    assert_eq!(list.json()["items"][0]["state"], "confirmed");
    let current = f
        .call(
            "GET",
            "/v1/generations/{gen}",
            "/v1/generations/current",
            None,
        )
        .await;
    let hash = current.json()["hash"].as_str().unwrap().to_owned();

    let bad = f
        .call(
            "POST",
            "/v1/generations",
            "/v1/generations",
            Some(json!({"manifest": {"content": "[outputs.\"x.y\"]\nsafe = 1\n"}, "lock": {"content": ""}, "artifacts": []})),
        )
        .await;
    assert_eq!(
        (bad.status, bad.json()["code"].clone()),
        (422, json!("J-E1601"))
    );

    let proposal = json!({"manifest": {"content": effective().replace("500 ms", "250 ms")}, "lock": {"content": ""}, "artifacts": [], "message": "faster"});
    let dry = f
        .call(
            "POST",
            "/v1/generations",
            "/v1/generations?dry_run=true",
            Some(proposal.clone()),
        )
        .await;
    assert_eq!(dry.status, 200);
    let created = f
        .call("POST", "/v1/generations", "/v1/generations", Some(proposal))
        .await;
    assert_eq!(created.status, 201);
    assert_eq!(created.json()["gen"], 2);
    assert_eq!(created.json()["state"], "pending");

    let no_match = f
        .call(
            "POST",
            "/v1/generations/{gen}/activate",
            "/v1/generations/2/activate",
            Some(json!({})),
        )
        .await;
    assert_eq!(no_match.json()["code"], "J-E1027");
    let stale = f
        .call_with(
            "POST",
            "/v1/generations/{gen}/activate",
            "/v1/generations/2/activate",
            &[(
                "if-match",
                "\"sha256:0000000000000000000000000000000000000000000000000000000000000000\"",
            )],
            Some(json!({})),
        )
        .await;
    assert_eq!(stale.json()["code"], "J-E1026");
    let window = f
        .call_with(
            "POST",
            "/v1/generations/{gen}/activate",
            "/v1/generations/2/activate",
            &[("if-match", &format!("\"{hash}\""))],
            Some(json!({"confirm_window": "200ms"})),
        )
        .await;
    assert_eq!(window.status, 200, "{}", window.json());
    assert_eq!(window.json()["state"], "confirming");
    assert!(window.json()["confirm_deadline"].is_string());
    let confirm = f
        .call(
            "POST",
            "/v1/generations/{gen}/confirm",
            "/v1/generations/2/confirm",
            Some(json!({})),
        )
        .await;
    assert_eq!(confirm.json()["state"], "confirmed");
    let twice = f
        .call(
            "POST",
            "/v1/generations/{gen}/confirm",
            "/v1/generations/2/confirm",
            Some(json!({})),
        )
        .await;
    assert_eq!(twice.json()["code"], "J-E1611");

    let now = f
        .call(
            "GET",
            "/v1/generations/{gen}",
            "/v1/generations/current",
            None,
        )
        .await;
    let h2 = now.json()["hash"].as_str().unwrap().to_owned();
    let back = f
        .call_with(
            "POST",
            "/v1/generations/rollback",
            "/v1/generations/rollback",
            &[("if-match", &h2)],
            Some(json!({"reason": "back to the bench"})),
        )
        .await;
    assert_eq!(back.status, 200, "{}", back.json());
    assert_eq!(back.json()["gen"], 1);
    let origins: Vec<Value> = back.json()["activations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["origin"].clone())
        .collect();
    assert_eq!(origins, vec![json!("local"), json!("rollback")]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn traces_are_cut_from_the_ring() {
    let f = start().await;
    let t = f
        .call(
            "POST",
            "/v1/traces",
            "/v1/traces",
            Some(json!({"source": "ring", "last": "100ms"})),
        )
        .await;
    assert_eq!(t.status, 201, "{}", t.json());
    assert_eq!(t.json()["state"], "ready");
    let id = t.json()["id"].as_str().unwrap().to_owned();
    let got = f
        .call(
            "GET",
            "/v1/traces/{trace}",
            &format!("/v1/traces/{id}"),
            None,
        )
        .await;
    assert_eq!(got.json()["id"], id);
    let file = f
        .http
        .send("GET", &format!("/v1/traces/{id}/file"), &[], None)
        .await;
    assert_eq!(file.status, 200);
    assert_eq!(&file.body[..4], b"JKTR");
    let list = f.call("GET", "/v1/traces", "/v1/traces", None).await;
    assert_eq!(list.json()["items"].as_array().unwrap().len(), 1);

    let rec = f
        .call(
            "POST",
            "/v1/traces",
            "/v1/traces",
            Some(json!({"source": "record", "duration": "150ms"})),
        )
        .await;
    assert_eq!(rec.json()["state"], "recording");
    let rid = rec.json()["id"].as_str().unwrap().to_owned();
    let early = f
        .http
        .send("GET", &format!("/v1/traces/{rid}/file"), &[], None)
        .await;
    assert_eq!(
        (early.status, early.json()["code"].clone()),
        (409, json!("J-E1706"))
    );
    tokio::time::sleep(Duration::from_millis(400)).await;
    let done = f
        .call(
            "GET",
            "/v1/traces/{trace}",
            &format!("/v1/traces/{rid}"),
            None,
        )
        .await;
    assert_eq!(done.json()["state"], "ready");
    let unsupported = f
        .call(
            "POST",
            "/v1/traces",
            "/v1/traces",
            Some(json!({"source": "history"})),
        )
        .await;
    assert_eq!(unsupported.json()["code"], "J-E1017");
}

/// The fixture's only device is `sim`, which declares no actions.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_action_catalog_follows_the_contract() {
    let f = start().await;
    let catalog = f.call("GET", "/v1/actions", "/v1/actions", None).await;
    assert_eq!(catalog.status, 200, "{}", catalog.json());
    assert_eq!(catalog.json()["items"].as_array().unwrap().len(), 0);
}

/// An out-of-range manual write trips the output (`on_range = "trip"`).
/// While that write still holds, a reset clears the trip for a tick but
/// the same condition re-trips it (docs/16 E7); once the write is
/// released, a reset clears it. A reset without a reason is refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resetting_a_trip_follows_the_contract() {
    let f = start_with(&effective_with_trippable_output()).await;

    let not_tripped = f
        .call(
            "POST",
            "/v1/outputs/{path}/reset",
            "/v1/outputs/pi.p_set/reset",
            Some(json!({"reason": "probe", "wait": true})),
        )
        .await;
    assert_eq!(not_tripped.status, 409, "{}", not_tripped.json());
    assert_eq!(not_tripped.json()["code"], "J-E1120");

    let write = f
        .call(
            "POST",
            "/v1/outputs/{path}/manual",
            "/v1/outputs/pi.p_set/manual",
            Some(json!({"value": 150, "ttl": "5s", "reason": "force a trip", "wait": true})),
        )
        .await;
    assert_eq!(write.status, 200, "{}", write.json());
    assert_eq!(write.json()["outcome"]["outcome"], "tripped");

    let retripped = f
        .call(
            "POST",
            "/v1/outputs/{path}/reset",
            "/v1/outputs/pi.p_set/reset",
            Some(json!({"reason": "clear", "wait": true})),
        )
        .await;
    assert_eq!(retripped.status, 200, "{}", retripped.json());
    assert_eq!(retripped.json()["result"], "retripped");

    let released = f
        .call(
            "POST",
            "/v1/outputs/{path}/release",
            "/v1/outputs/pi.p_set/release",
            Some(json!({})),
        )
        .await;
    assert_eq!(released.status, 200, "{}", released.json());
    let cleared = f
        .call(
            "POST",
            "/v1/outputs/{path}/reset",
            "/v1/outputs/pi.p_set/reset",
            Some(json!({"reason": "clear for real", "wait": true})),
        )
        .await;
    assert_eq!(cleared.status, 200, "{}", cleared.json());
    assert_eq!(cleared.json()["result"], "cleared");

    let no_reason = f
        .call(
            "POST",
            "/v1/outputs/{path}/reset",
            "/v1/outputs/pi.p_set/reset",
            Some(json!({"reason": "", "wait": true})),
        )
        .await;
    assert_eq!(no_reason.status, 400, "{}", no_reason.json());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn listing_and_bulk_resetting_trips_follows_the_contract() {
    let f = start_with(&effective_with_trippable_output()).await;

    let empty = f.call("GET", "/v1/trips", "/v1/trips", None).await;
    assert_eq!(empty.status, 200, "{}", empty.json());
    assert_eq!(empty.json()["items"].as_array().unwrap().len(), 0);

    let write = f
        .call(
            "POST",
            "/v1/outputs/{path}/manual",
            "/v1/outputs/pi.p_set/manual",
            Some(json!({"value": 150, "ttl": "5s", "reason": "force a trip", "wait": true})),
        )
        .await;
    assert_eq!(write.json()["outcome"]["outcome"], "tripped");

    let listed = f.call("GET", "/v1/trips", "/v1/trips", None).await;
    assert_eq!(listed.status, 200, "{}", listed.json());
    let items = listed.json()["items"].as_array().unwrap().clone();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["output"], "pi.p_set");
    assert_eq!(items[0]["cause"]["kind"], "range");
    assert!(items[0]["since"].as_str().is_some());

    let released = f
        .call(
            "POST",
            "/v1/outputs/{path}/release",
            "/v1/outputs/pi.p_set/release",
            Some(json!({})),
        )
        .await;
    assert_eq!(released.status, 200, "{}", released.json());

    let bulk = f
        .call(
            "POST",
            "/v1/trips/reset",
            "/v1/trips/reset",
            Some(json!({"sel": ["pi.*"], "reason": "clear the group"})),
        )
        .await;
    assert_eq!(bulk.status, 200, "{}", bulk.json());
    let items = bulk.json()["items"].as_array().unwrap().clone();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["output"], "pi.p_set");
    assert_eq!(items[0]["result"], "cleared");

    let now_empty = f.call("GET", "/v1/trips", "/v1/trips", None).await;
    assert_eq!(now_empty.json()["items"].as_array().unwrap().len(), 0);

    let resets: Vec<Value> = decisions_from(f._dir.path(), 0)
        .into_iter()
        .filter(|e| e["record"] == "reset")
        .collect();
    assert_eq!(resets.len(), 1, "{resets:?}");
    assert_eq!(resets[0]["output"], "pi.p_set");
    assert_eq!(resets[0]["reason"], "clear the group");
    assert_eq!(resets[0]["result"], "cleared");
    assert!(
        resets[0]["principal"]
            .as_str()
            .is_some_and(|p| p.starts_with("user:")),
        "{}",
        resets[0]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invoking_an_action_follows_the_contract() {
    let f = start().await;
    let no_key = f
        .call(
            "POST",
            "/v1/actions/{target}/{action}",
            "/v1/actions/pi/reset",
            Some(json!({"args": {}})),
        )
        .await;
    assert_eq!(no_key.status, 400, "{}", no_key.json());
    assert_eq!(no_key.json()["code"], "J-E1023");

    let unknown = f
        .call_with(
            "POST",
            "/v1/actions/{target}/{action}",
            "/v1/actions/pi/reset",
            &[("Idempotency-Key", "k1")],
            Some(json!({"args": {}})),
        )
        .await;
    assert_eq!(unknown.status, 404, "{}", unknown.json());
    assert_eq!(unknown.json()["code"], "J-E1400");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_invocations_follow_the_contract() {
    let f = start().await;
    let list = f
        .call("GET", "/v1/invocations", "/v1/invocations", None)
        .await;
    assert_eq!(list.status, 200, "{}", list.json());
    assert_eq!(list.json()["items"].as_array().map(Vec::len), Some(0));

    let unknown = f
        .call(
            "GET",
            "/v1/invocations/{invocation}",
            "/v1/invocations/01HZZZZZZZZZZZZZZZZZZZZZZZ",
            None,
        )
        .await;
    assert_eq!(unknown.status, 404, "{}", unknown.json());
    assert_eq!(unknown.json()["code"], "J-E1404");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_manual_write_survives_a_jokerd_restart_within_its_ttl() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = effective();

    let (http, stop) = boot_at(dir.path(), &manifest).await;
    let write = http
        .send(
            "POST",
            "/v1/outputs/pi.act_led/manual",
            &[],
            Some(&json!({"value": true, "ttl": "30s", "reason": "restart test", "wait": true})),
        )
        .await;
    assert_eq!(write.status, 200, "{}", write.json());
    assert_eq!(write.json()["outcome"]["outcome"], "accepted");
    stop.halt();

    let before = decisions_from(dir.path(), 0).len();

    let (http2, stop2) = boot_at(dir.path(), &manifest).await;
    let after = decisions_from(dir.path(), before);
    let first = first_output_record(&after, "pi.act_led");
    assert_eq!(first["tick"], 0, "{first}");
    assert_eq!(first["level"], "manual:4", "{first}");
    assert!(
        after.iter().any(|e| e["record"] == "manual_restored"
            && e["output"] == "pi.act_led"
            && e["level"] == "manual:4"
            && e["synced"] == false),
        "no manual_restored record in {after:?}"
    );
    let out = http2.send("GET", "/v1/outputs/pi.act_led", &[], None).await;
    assert_eq!(out.json()["active_level"], "manual:4", "{}", out.json());
    assert_eq!(out.json()["applied"], true);
    let list = http2.send("GET", "/v1/manual", &[], None).await;
    assert_eq!(
        list.json()["items"][0]["reason"],
        "restart test",
        "{}",
        list.json()
    );
    stop2.halt();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_trip_survives_a_jokerd_restart() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = effective_with_trippable_output();

    let (http, stop) = boot_at(dir.path(), &manifest).await;
    let write = http
        .send(
            "POST",
            "/v1/outputs/pi.p_set/manual",
            &[],
            Some(&json!({"value": 150, "ttl": "30s", "reason": "force a trip", "wait": true})),
        )
        .await;
    assert_eq!(write.status, 200, "{}", write.json());
    assert_eq!(write.json()["outcome"]["outcome"], "tripped");
    let released = http
        .send(
            "POST",
            "/v1/outputs/pi.p_set/release",
            &[],
            Some(&json!({})),
        )
        .await;
    assert_eq!(released.status, 200, "{}", released.json());
    let still_tripped = http.send("GET", "/v1/outputs/pi.p_set", &[], None).await;
    assert_eq!(
        still_tripped.json()["tripped"],
        true,
        "{}",
        still_tripped.json()
    );
    stop.halt();

    let before = decisions_from(dir.path(), 0).len();

    let (http2, stop2) = boot_at(dir.path(), &manifest).await;
    let after = decisions_from(dir.path(), before);
    let first = first_output_record(&after, "pi.p_set");
    assert_eq!(first["tick"], 0, "{first}");
    assert_eq!(first["cause"]["kind"], "trip", "{first}");
    let out = http2.send("GET", "/v1/outputs/pi.p_set", &[], None).await;
    assert_eq!(out.json()["tripped"], true, "{}", out.json());
    let trips = http2.send("GET", "/v1/trips", &[], None).await;
    assert_eq!(
        trips.json()["items"][0]["output"],
        "pi.p_set",
        "{}",
        trips.json()
    );
    stop2.halt();
}

/// A persisted trip of an output that a new generation does not declare
/// is kept, recorded as `trip_kept`, listed with `declared: false`, and
/// cleared by a reset of its path (ADR 0070).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_trip_of_an_output_a_new_generation_removed_is_kept_listed_and_resettable() {
    let dir = tempfile::tempdir().unwrap();
    let (http, stop) = boot_at(dir.path(), &effective_with_trippable_output()).await;
    let write = http
        .send(
            "POST",
            "/v1/outputs/pi.p_set/manual",
            &[],
            Some(&json!({"value": 150, "ttl": "30s", "reason": "force a trip", "wait": true})),
        )
        .await;
    assert_eq!(write.json()["outcome"]["outcome"], "tripped");
    let trips_file = dir.path().join("state").join("trips");
    for _ in 0..100 {
        if std::fs::read_to_string(&trips_file).is_ok_and(|t| t.contains("pi.p_set")) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    {
        let mut c = jokerd::daemon::lock_controller(&stop.controller);
        let g = c
            .propose(&effective(), "", Some("drop p_set".into()), false)
            .unwrap();
        c.activate(g.number, None, None).unwrap();
    }
    let kept: Vec<Value> = decisions_from(dir.path(), 0)
        .into_iter()
        .filter(|e| e["record"] == "trip_kept")
        .collect();
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(kept[0]["output"], "pi.p_set");
    assert_eq!(kept[0]["gen"], 2);

    let contract = Contract::load();
    let listed = http.send("GET", "/v1/trips", &[], None).await;
    contract.check("get", "/v1/trips", 200, &listed.json());
    let items = listed.json()["items"].as_array().unwrap().clone();
    assert_eq!(items.len(), 1, "{}", listed.json());
    assert_eq!(items[0]["output"], "pi.p_set");
    assert_eq!(items[0]["declared"], false);

    let missing = http
        .send(
            "POST",
            "/v1/outputs/pi.nothing/reset",
            &[],
            Some(&json!({"reason": "nothing here"})),
        )
        .await;
    assert_eq!(missing.status, 404, "{}", missing.json());

    let reset = http
        .send(
            "POST",
            "/v1/outputs/pi.p_set/reset",
            &[],
            Some(&json!({"reason": "the output is gone"})),
        )
        .await;
    assert_eq!(reset.status, 200, "{}", reset.json());
    contract.check("post", "/v1/outputs/{path}/reset", 200, &reset.json());
    assert_eq!(reset.json()["result"], "cleared");
    assert!(
        !std::fs::read_to_string(&trips_file)
            .unwrap()
            .contains("pi.p_set")
    );
    let after = http.send("GET", "/v1/trips", &[], None).await;
    assert_eq!(after.json()["items"].as_array().unwrap().len(), 0);
    let resets: Vec<Value> = decisions_from(dir.path(), 0)
        .into_iter()
        .filter(|e| e["record"] == "reset")
        .collect();
    assert_eq!(resets.len(), 1, "{resets:?}");
    assert_eq!(resets[0]["result"], "cleared");
    assert_eq!(resets[0]["reason"], "the output is gone");
    stop.halt();
}

/// A manual level that cannot be written to `state/manual` still holds,
/// and the failure is recorded as `state_write_failed` instead of only
/// reaching standard error (ADR 0070).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_manual_level_that_cannot_be_persisted_is_recorded() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let (http, stop) = boot_at(dir.path(), &effective()).await;
    let state = dir.path().join("state");
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o555)).unwrap();
    let write = http
        .send(
            "POST",
            "/v1/outputs/pi.act_led/manual",
            &[],
            Some(
                &json!({"value": true, "ttl": "30s", "reason": "disk is read-only", "wait": true}),
            ),
        )
        .await;
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(write.status, 200, "{}", write.json());
    let held = http.send("GET", "/v1/manual", &[], None).await;
    assert_eq!(
        held.json()["items"].as_array().unwrap().len(),
        1,
        "{}",
        held.json()
    );
    let failures: Vec<Value> = decisions_from(dir.path(), 0)
        .into_iter()
        .filter(|e| e["record"] == "state_write_failed" && e["file"] == "state/manual")
        .collect();
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert_eq!(failures[0]["op"], "set");
    assert_eq!(failures[0]["output"], "pi.act_led");
    assert_eq!(failures[0]["gen"], 1);
    assert!(failures[0]["error"].as_str().is_some_and(|e| !e.is_empty()));
    stop.halt();
}

/// A trip reaches `state/trips` when it happens, not when someone reads
/// the API (docs/01 §7, ADR 0061): nothing here calls a GET, and the
/// write does not wait for its tick.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_trip_is_persisted_on_its_transition_without_any_read() {
    let dir = tempfile::tempdir().unwrap();
    let (http, stop) = boot_at(dir.path(), &effective_with_trippable_output()).await;
    let write = http
        .send(
            "POST",
            "/v1/outputs/pi.p_set/manual",
            &[],
            Some(&json!({"value": 150, "ttl": "30s", "reason": "force a trip", "wait": false})),
        )
        .await;
    assert_eq!(write.status, 202, "{}", write.json());
    let path = dir.path().join("state").join("trips");
    let mut persisted = false;
    for _ in 0..100 {
        if std::fs::read_to_string(&path).is_ok_and(|t| t.contains("pi.p_set")) {
            persisted = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    stop.halt();
    assert!(persisted, "state/trips never recorded the trip");
}

/// A dead-`jokerd`-and-restart cycle bumps the startup counter
/// (docs/01 §20's "startup counter"), persisted so it survives rather
/// than resetting with the process.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_boot_counter_persists_and_increments_across_restarts() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = effective();

    let (http, stop) = boot_at(dir.path(), &manifest).await;
    let first = http.send("GET", "/v1/info", &[], None).await;
    Contract::load().check("get", "/v1/info", 200, &first.json());
    let first_count = first.json()["runtime"]["boot_count"].as_u64().unwrap();
    assert_eq!(first_count, 1, "{}", first.json());
    stop.halt();

    let (http2, stop2) = boot_at(dir.path(), &manifest).await;
    let second = http2.send("GET", "/v1/info", &[], None).await;
    let second_count = second.json()["runtime"]["boot_count"].as_u64().unwrap();
    assert_eq!(second_count, first_count + 1, "{}", second.json());
    stop2.halt();
}

/// A trips file that cannot be parsed keeps the daemon from starting: a
/// node that cannot know whether an output is in trip must not come back
/// with it at its normal value (docs/01 §7, ADR 0061).
#[test]
fn a_corrupt_trips_file_keeps_the_daemon_from_starting() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("state")).unwrap();
    std::fs::write(dir.path().join("state").join("trips"), b"[{\"output\": ").unwrap();
    let clock = Monotonic::start();
    let log = Arc::new(Mutex::new(
        DecisionLog::open(&dir.path().join("decisions.jsonl")).unwrap(),
    ));
    let started = Controller::new(
        dir.path(),
        log,
        Arc::new(NativeTasks),
        Arc::new(NoDevices),
        Box::new(move || Box::new(SystemPacer::new(clock, ClockQuality::Unsynced))),
        Config::default(),
    )
    .map_err(|e| e.to_string())
    .and_then(|c| {
        Node::new(Arc::new(Mutex::new(c)), clock, 0, dir.path())
            .map(|_| ())
            .map_err(|e| e.to_string())
    });
    let e = started.unwrap_err();
    assert!(e.contains("state/trips"), "{e}");
}

#[test]
#[should_panic(expected = "get /v1/info 200")]
fn the_contract_check_rejects_a_bad_body() {
    Contract::load().check("get", "/v1/info", 200, &json!({"node": "x"}));
}

#[test]
#[should_panic(expected = "is not in the contract")]
fn the_contract_check_rejects_an_unknown_operation() {
    Contract::load().check("get", "/v1/nowhere", 200, &json!({}));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn blobs_are_uploaded_once_and_only_with_their_digest() {
    let f = start().await;
    let bytes = b"\0asm a component".to_vec();
    let digest = jokerd::blobs::digest_of(&bytes);
    let missing = f
        .call(
            "POST",
            "/v1/blobs/missing",
            "/v1/blobs/missing",
            Some(json!({"digests": [digest]})),
        )
        .await;
    assert_eq!(missing.json()["missing"], json!([digest]));
    let head = f
        .http
        .send("HEAD", &format!("/v1/blobs/{digest}"), &[], None)
        .await;
    assert_eq!(head.status, 404);

    let wrong = jokerd::blobs::digest_of(b"something else");
    let mismatch = f
        .http
        .exchange(
            "POST",
            &format!("/v1/blobs/uploads?digest={wrong}"),
            &[],
            Some(("application/octet-stream", &bytes)),
        )
        .await;
    f.contract.check(
        "post",
        "/v1/blobs/uploads",
        mismatch.status,
        &mismatch.json(),
    );
    assert_eq!(
        (mismatch.status, mismatch.json()["code"].clone()),
        (400, json!("J-E1603"))
    );
    let unknown = f
        .http
        .exchange(
            "POST",
            &format!("/v1/blobs/uploads?digest={digest}&kind=joker.app.v9%2Bwasm"),
            &[],
            Some(("application/octet-stream", &bytes)),
        )
        .await;
    assert_eq!(unknown.json()["code"], "J-E1012");

    let path = format!("/v1/blobs/uploads?digest={digest}&kind=joker.app.v1%2Bwasm");
    let up = f
        .http
        .exchange(
            "POST",
            &path,
            &[],
            Some(("application/octet-stream", &bytes)),
        )
        .await;
    f.contract
        .check("post", "/v1/blobs/uploads", up.status, &up.json());
    assert_eq!(up.status, 201, "{}", up.json());
    assert_eq!(up.json()["existed"], false);
    assert_eq!(up.json()["kind"], "joker.app.v1+wasm");
    assert_eq!(
        up.header("location"),
        Some(format!("/v1/blobs/{digest}").as_str())
    );
    let again = f
        .http
        .exchange(
            "POST",
            &path,
            &[],
            Some(("application/octet-stream", &bytes)),
        )
        .await;
    assert_eq!(again.json()["existed"], true);
    let head = f
        .http
        .send("HEAD", &format!("/v1/blobs/{digest}"), &[], None)
        .await;
    assert_eq!(head.status, 200);
    let missing = f
        .call(
            "POST",
            "/v1/blobs/missing",
            "/v1/blobs/missing",
            Some(json!({"digests": [digest, wrong]})),
        )
        .await;
    assert_eq!(missing.json()["missing"], json!([wrong]));
}

const GPIO_WITHOUT_CHIP: &str = r#"
[project]
name  = "api-devices-test"
joker = "1.0"
mode  = "dev"

[devices.hdr]
driver = "gpio"
chip   = "gpiochip0-that-does-not-exist-in-this-test"

[devices.hdr.signals]
line4 = { type = "bool", direction = "input", line = 4, poll = "20ms" }
"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn status_and_metrics_show_the_link_and_counters_of_each_device() {
    let devices = jokerd::real_devices::RealDevices::new(tokio::runtime::Handle::current());
    let f = start_with_devices(GPIO_WITHOUT_CHIP, Arc::new(devices)).await;

    let s = f.call("GET", "/v1/status", "/v1/status", None).await;
    assert_eq!(s.status, 200, "{}", s.json());
    let device = &s.json()["devices"][0];
    assert_eq!(device["name"], "hdr", "{device}");
    assert_eq!(device["link"], "stopped", "{device}");
    assert_eq!(device["reconnects"], 0);
    assert_eq!(device["dropped_samples"], 0);
    assert_eq!(device["invalid_samples"], 0);

    let m = f.call("GET", "/v1/metrics", "/v1/metrics", None).await;
    assert_eq!(m.status, 200, "{}", m.json());
    let driver = &m.json()["drivers"][0];
    assert_eq!(driver["device"], "hdr", "{driver}");
    assert_eq!(driver["link"], "stopped", "{driver}");
    assert_eq!(driver["reconnects"], 0);
    assert!(m.json()["api"]["dropped"].is_u64(), "{}", m.json());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_proposal_carries_its_profiles_as_uploaded_artifacts() {
    let f = start().await;
    let profile = "schema = 1\n\n[profile]\nname    = \"acme/pcs\"\nversion = \"1.0.0\"\ndriver  = \"modbus\"\n\n[signals.p_ac]\nat   = 0\nfc   = 4\ntype = \"f32\"\nunit = \"kW\"\n";
    let digest = jokerd::blobs::digest_of(profile.as_bytes());
    let manifest = format!(
        "{}\n[devices.pcs]\ndriver  = \"modbus-tcp\"\naddress = \"127.0.0.1:1502\"\nprofile = \"acme/pcs@1\"\n",
        effective()
    );
    let artifact = json!([{
        "name": "profiles/acme/pcs@1.profile.toml",
        "kind": "joker.profile.v1+toml",
        "digest": digest,
    }]);
    let proposal = |artifacts: Value| {
        json!({
            "manifest": {"content": manifest},
            "lock": {"content": ""},
            "artifacts": artifacts,
            "base": null,
        })
    };
    let missing = f
        .call(
            "POST",
            "/v1/generations",
            "/v1/generations?dry_run=true",
            Some(proposal(artifact.clone())),
        )
        .await;
    assert_eq!(missing.json()["code"], "J-E1602", "{}", missing.json());
    let up = f
        .http
        .exchange(
            "POST",
            &format!("/v1/blobs/uploads?digest={digest}&kind=joker.profile.v1%2Btoml"),
            &[],
            Some(("application/octet-stream", profile.as_bytes())),
        )
        .await;
    assert_eq!(up.status, 201, "{}", up.json());
    let with = f
        .call(
            "POST",
            "/v1/generations",
            "/v1/generations?dry_run=true",
            Some(proposal(artifact)),
        )
        .await;
    assert_eq!(with.status, 200, "{}", with.json());
    let without = f
        .call(
            "POST",
            "/v1/generations",
            "/v1/generations?dry_run=true",
            Some(proposal(json!([]))),
        )
        .await;
    assert_eq!(
        without.status, 422,
        "the node has no profile of its own to fall back on"
    );
}

/// A numeric output, an alarm on it, and the historian recording it.
fn effective_with_alarm() -> String {
    "[project]\n\
     name  = \"alarm-test\"\n\
     joker = \"1.0\"\n\
     mode  = \"dev\"\n\
     \n\
     [outputs.\"pi.p_set\"]\n\
     safe  = \"0 kW\"\n\
     range = [\"-100 kW\", \"100 kW\"]\n\
     \n\
     [alarms.p_high]\n\
     when       = \"pi.p_set > 50 kW\"\n\
     severity   = \"high\"\n\
     message    = \"p_set is {pi.p_set}\"\n\
     max_shelve = \"1h\"\n\
     \n\
     [telemetry.record]\n\
     \"pi.p_set\" = { on_change = true, keep = { raw = \"7d\", \"1m\" = \"30d\" } }\n\
     \n\
     [devices.pi]\n\
     driver = \"sim\"\n\
     \n\
     [devices.pi.signals]\n\
     p_set = { type = \"f64\", direction = \"output\", unit = \"kW\" }\n"
        .to_owned()
}

async fn get_json(http: &Http, path: &str) -> Value {
    http.send("GET", path, &[], None).await.json()
}

/// The native apps, with `light.blink` blocking 200 ms on every tick of
/// its 20 ms group: a hung group (ADR 0086).
struct Hanging;

struct Blocking;

impl jokerd::task::CycleTask for Blocking {
    fn run(
        &mut self,
        _cx: &mut jokerd::task::TaskContext<'_, '_, '_>,
    ) -> Result<(), jokerd::task::TaskError> {
        std::thread::sleep(Duration::from_millis(200));
        Ok(())
    }
}

impl jokerd::task::TaskProvider for Hanging {
    fn interfaces(
        &self,
    ) -> std::collections::BTreeMap<String, joker_manifest::context::AppInterface> {
        jokerd::task::TaskProvider::interfaces(&NativeTasks)
    }

    fn instantiate(
        &self,
        _spec: jokerd::task::TaskSpec<'_>,
    ) -> Result<Box<dyn jokerd::task::CycleTask>, jokerd::task::TaskError> {
        Ok(Box::new(Blocking))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hung_group_is_a_critical_alarm_in_alarms_and_status() {
    let f = start_with_tasks(&effective(), Arc::new(NoDevices), Arc::new(Hanging)).await;
    let id = "joker.group.main.stalled";
    let alarm = wait_alarm(&f.http, id, "active_unacked").await;
    assert_eq!(alarm["severity"], "critical", "{alarm}");
    assert_eq!(alarm["source"], "runtime", "{alarm}");

    let listed = f
        .call("GET", "/v1/alarms", "/v1/alarms?sel=joker.group.**", None)
        .await;
    assert!(
        listed.json()["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["id"] == id),
        "{}",
        listed.json()
    );

    let status = f.call("GET", "/v1/status", "/v1/status", None).await;
    let alarms = &status.json()["alarms"];
    assert!(alarms["unacked"].as_u64().unwrap() >= 1, "{alarms}");
    assert_eq!(
        alarms["items"][0]["id"], id,
        "the critical alarm comes first: {alarms}"
    );
}

async fn wait_alarm(http: &Http, id: &str, state: &str) -> Value {
    for _ in 0..200 {
        let a = get_json(http, &format!("/v1/alarms/{id}")).await;
        if a["state"] == state {
            return a;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!(
        "{id} never reached {state}: {}",
        get_json(http, &format!("/v1/alarms/{id}")).await
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn alarms_ack_shelve_and_unshelve_follow_the_contract() {
    let f = start_with(&effective_with_alarm()).await;
    let list = f.call("GET", "/v1/alarms", "/v1/alarms", None).await;
    assert_eq!(list.status, 200, "{}", list.json());
    assert_eq!(list.json()["items"][0]["id"], "p_high");
    assert_eq!(list.json()["items"][0]["state"], "normal");
    let missing = f
        .call("GET", "/v1/alarms/{alarm}", "/v1/alarms/nope", None)
        .await;
    assert_eq!(
        (missing.status, missing.json()["code"].clone()),
        (404, json!("J-E1200"))
    );

    let nothing = f
        .call(
            "POST",
            "/v1/alarms/{alarm}/ack",
            "/v1/alarms/p_high/ack",
            Some(json!({})),
        )
        .await;
    assert_eq!(
        (nothing.status, nothing.json()["code"].clone()),
        (409, json!("J-E1204"))
    );
    let no_ttl = f
        .call(
            "POST",
            "/v1/alarms/{alarm}/shelve",
            "/v1/alarms/p_high/shelve",
            Some(json!({"reason": "maintenance"})),
        )
        .await;
    assert_eq!(
        (no_ttl.status, no_ttl.json()["code"].clone()),
        (400, json!("J-E1201"))
    );
    let too_long = f
        .call(
            "POST",
            "/v1/alarms/{alarm}/shelve",
            "/v1/alarms/p_high/shelve",
            Some(json!({"ttl": "2h", "reason": "maintenance"})),
        )
        .await;
    assert_eq!(
        (too_long.status, too_long.json()["code"].clone()),
        (422, json!("J-E1202"))
    );
    let not_shelved = f
        .call(
            "POST",
            "/v1/alarms/{alarm}/unshelve",
            "/v1/alarms/p_high/unshelve",
            Some(json!({})),
        )
        .await;
    assert_eq!(
        (not_shelved.status, not_shelved.json()["code"].clone()),
        (409, json!("J-E1203"))
    );

    let write = f
        .call(
            "POST",
            "/v1/outputs/{path}/manual",
            "/v1/outputs/pi.p_set/manual",
            Some(json!({"value": 80, "ttl": "1h", "reason": "raise the alarm", "wait": true})),
        )
        .await;
    assert_eq!(write.status, 200, "{}", write.json());
    let active = wait_alarm(&f.http, "p_high", "active_unacked").await;
    assert_eq!(active["message"], "p_set is 80 kW");
    assert_eq!(active["active"], true);
    let status = f.call("GET", "/v1/status", "/v1/status", None).await;
    assert_eq!(status.json()["alarms"]["active"], 1);
    assert_eq!(status.json()["alarms"]["unacked"], 1);

    let acked = f
        .call(
            "POST",
            "/v1/alarms/{alarm}/ack",
            "/v1/alarms/p_high/ack",
            Some(json!({"reason": "seen"})),
        )
        .await;
    assert_eq!(acked.status, 200, "{}", acked.json());
    assert_eq!(acked.json()["state"], "active_acked");
    assert!(acked.json()["acked_by"].as_str().is_some());
    let again = f
        .call(
            "POST",
            "/v1/alarms/{alarm}/ack",
            "/v1/alarms/p_high/ack",
            None,
        )
        .await;
    assert_eq!(again.status, 200, "an ack in active_acked is idempotent");

    let shelved = f
        .call(
            "POST",
            "/v1/alarms/{alarm}/shelve",
            "/v1/alarms/p_high/shelve",
            Some(json!({"ttl": "10m", "reason": "maintenance"})),
        )
        .await;
    assert_eq!(shelved.status, 200, "{}", shelved.json());
    assert_eq!(shelved.json()["shelved"]["reason"], "maintenance");
    assert_eq!(
        shelved.json()["state"],
        "active_acked",
        "shelving does not change the state"
    );
    let filtered = f
        .call(
            "GET",
            "/v1/alarms",
            "/v1/alarms?state=shelved&severity=high",
            None,
        )
        .await;
    assert_eq!(filtered.json()["items"].as_array().unwrap().len(), 1);
    let unshelved = f
        .call(
            "POST",
            "/v1/alarms/{alarm}/unshelve",
            "/v1/alarms/p_high/unshelve",
            Some(json!({"reason": "done"})),
        )
        .await;
    assert_eq!(unshelved.status, 200, "{}", unshelved.json());
    assert!(unshelved.json()["shelved"].is_null());

    let events = f
        .call(
            "GET",
            "/v1/alarm-events",
            "/v1/alarm-events?order=asc",
            None,
        )
        .await;
    assert_eq!(events.status, 200, "{}", events.json());
    let kinds: Vec<Value> = events.json()["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].clone())
        .collect();
    assert_eq!(
        kinds,
        vec![
            json!("transition"),
            json!("ack"),
            json!("shelve"),
            json!("unshelve")
        ]
    );
    assert_eq!(events.json()["items"][2]["reason"], "maintenance");
    assert_eq!(events.json()["items"][0]["message"], "p_set is 80 kW");
    let ndjson = f
        .http
        .send(
            "GET",
            "/v1/alarm-events",
            &[("accept", "application/x-ndjson")],
            None,
        )
        .await;
    assert_eq!(ndjson.header("content-type"), Some("application/x-ndjson"));
    assert_eq!(String::from_utf8_lossy(&ndjson.body).lines().count(), 4);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn history_queries_follow_the_contract() {
    let f = start_with(&effective_with_alarm()).await;
    for v in [10, 20, 30] {
        let w = f
            .call(
                "POST",
                "/v1/outputs/{path}/manual",
                "/v1/outputs/pi.p_set/manual",
                Some(json!({"value": v, "ttl": "1h", "reason": "history", "wait": true})),
            )
            .await;
        assert_eq!(w.status, 200, "{}", w.json());
        tokio::time::sleep(Duration::from_millis(60)).await;
    }
    let mut raw = Value::Null;
    for _ in 0..200 {
        raw = f
            .call(
                "GET",
                "/v1/history",
                "/v1/history?sel=pi.p_set&from=1970-01-01T00:00:00Z",
                None,
            )
            .await
            .json();
        if raw["series"][0]["points"]
            .as_array()
            .is_some_and(|p| p.len() >= 3)
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let points = raw["series"][0]["points"].as_array().unwrap();
    let values: Vec<f64> = points.iter().filter_map(|p| p["value"].as_f64()).collect();
    assert!(values.ends_with(&[10.0, 20.0, 30.0]), "{values:?}");
    assert_eq!(raw["series"][0]["unit"], "kW");
    assert_eq!(raw["tier"], "raw");

    let agg = f
        .call(
            "POST",
            "/v1/history/query",
            "/v1/history/query",
            Some(json!({"sel": ["pi.p_set"], "from": "1970-01-01T00:00:00Z", "step": "1h", "agg": ["avg", "max", "count", "last"]})),
        )
        .await;
    assert_eq!(agg.status, 200, "{}", agg.json());
    let point = &agg.json()["series"][0]["points"][0];
    assert_eq!(point["max"], 30.0);
    assert_eq!(point["last"], 30.0);
    assert!(point["count"].as_u64().unwrap() >= 3);

    let csv = f
        .http
        .send(
            "GET",
            "/v1/history?sel=pi.p_set&from=1970-01-01T00:00:00Z",
            &[("accept", "text/csv")],
            None,
        )
        .await;
    let text = String::from_utf8_lossy(&csv.body).to_string();
    assert!(
        text.starts_with("path,ts,value,quality,reason,seq\n"),
        "{text}"
    );
    assert!(text.contains("pi.p_set,"));
    let lines = f
        .http
        .send(
            "GET",
            "/v1/history?sel=pi.p_set&from=1970-01-01T00:00:00Z&limit=2",
            &[("accept", "application/x-ndjson")],
            None,
        )
        .await;
    assert_eq!(String::from_utf8_lossy(&lines.body).lines().count(), 2);
    assert!(
        lines
            .header("link")
            .is_some_and(|l| l.contains("rel=\"next\""))
    );

    for (query, status, code) in [
        ("/v1/history", 400, "J-E1013"),
        ("/v1/history?sel=pi.p_set&step=500us", 400, "J-E1039"),
        ("/v1/history?sel=pi.p_set&agg=avg", 400, "J-E1039"),
        ("/v1/history?sel=pi.p_set&limit=200000", 422, "J-E1038"),
        ("/v1/history?sel=pi.p_set&from=-1h&since=1h", 400, "J-E1034"),
        ("/v1/history?sel=Pi.P", 400, "J-E1030"),
    ] {
        let r = f.call("GET", "/v1/history", query, None).await;
        assert_eq!(
            (r.status, r.json()["code"].clone()),
            (status, json!(code)),
            "{query}"
        );
    }
    let events = f
        .call("GET", "/v1/events", "/v1/events?sel=pi.**", None)
        .await;
    assert_eq!(events.status, 200, "{}", events.json());
}

/// The alarm book survives a restart: a pending acknowledgement and a
/// shelve come back, and so does the journal (ADR 0082).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn alarm_state_and_journal_survive_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = effective_with_alarm();
    let (http, halt) = boot_at(dir.path(), &manifest).await;
    let write = http
        .send(
            "POST",
            "/v1/outputs/pi.p_set/manual",
            &[],
            Some(&json!({"value": 80, "ttl": "1h", "reason": "raise", "wait": true})),
        )
        .await;
    assert_eq!(write.status, 200, "{}", write.json());
    wait_alarm(&http, "p_high", "active_unacked").await;
    let shelve = http
        .send(
            "POST",
            "/v1/alarms/p_high/shelve",
            &[],
            Some(&json!({"ttl": "30m", "reason": "commissioning"})),
        )
        .await;
    assert_eq!(shelve.status, 200, "{}", shelve.json());
    let release = http
        .send(
            "POST",
            "/v1/outputs/pi.p_set/release",
            &[],
            Some(&json!({})),
        )
        .await;
    assert_eq!(release.status, 200, "{}", release.json());
    wait_alarm(&http, "p_high", "rtn_unacked").await;
    halt.halt();

    let (http, halt) = boot_at(dir.path(), &manifest).await;
    let a = wait_alarm(&http, "p_high", "rtn_unacked").await;
    assert_eq!(a["shelved"]["reason"], "commissioning", "{a}");
    let events = get_json(&http, "/v1/alarm-events?order=asc").await;
    let kinds: Vec<Value> = events["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].clone())
        .collect();
    assert_eq!(
        kinds,
        vec![json!("transition"), json!("shelve"), json!("transition")]
    );
    let ack = http
        .send("POST", "/v1/alarms/p_high/ack", &[], Some(&json!({})))
        .await;
    assert_eq!(ack.status, 200, "{}", ack.json());
    assert_eq!(ack.json()["state"], "normal");
    halt.halt();
}

/// An output with an actuation budget, for the runtime alarms of
/// ref/01 §5.15 (ADR 0085).
fn effective_with_budget() -> String {
    "[project]\n\
     name  = \"runtime-alarms\"\n\
     joker = \"1.0\"\n\
     mode  = \"dev\"\n\
     \n\
     [outputs.\"pi.p_set\"]\n\
     safe           = \"0 kW\"\n\
     range          = [\"-100 kW\", \"100 kW\"]\n\
     max_actuations = \"2/h\"\n\
     \n\
     [devices.pi]\n\
     driver = \"sim\"\n\
     \n\
     [devices.pi.signals]\n\
     p_set = { type = \"f64\", direction = \"output\", unit = \"kW\" }\n"
        .to_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_alarms_rise_and_clear_through_the_cycle_loop() {
    let f = start_with(&effective_with_budget()).await;
    let mut last = Value::Null;
    for v in [10, 20, 30] {
        last = f
            .call(
                "POST",
                "/v1/outputs/{path}/manual",
                "/v1/outputs/pi.p_set/manual",
                Some(json!({"value": v, "ttl": "10m", "reason": "budget", "wait": true})),
            )
            .await
            .json();
    }
    assert_eq!(last["outcome"]["outcome"], "rejected", "{last}");
    assert_eq!(last["outcome"]["reason"], "budget", "{last}");
    let budget = wait_alarm(&f.http, "joker.output.pi.p_set.budget", "active_unacked").await;
    assert_eq!(budget["source"], "runtime");
    assert_eq!(budget["severity"], "high");
    wait_alarm(
        &f.http,
        "joker.output.pi.p_set.manual_expiring",
        "active_unacked",
    )
    .await;
    let listed = f
        .call(
            "GET",
            "/v1/alarms",
            "/v1/alarms?sel=joker.**&state=active",
            None,
        )
        .await;
    assert_eq!(
        listed.json()["items"].as_array().unwrap().len(),
        2,
        "{}",
        listed.json()
    );

    let acked = f
        .call(
            "POST",
            "/v1/alarms/{alarm}/ack",
            "/v1/alarms/joker.output.pi.p_set.budget/ack",
            Some(json!({"reason": "known"})),
        )
        .await;
    assert_eq!(acked.status, 200, "{}", acked.json());
    assert_eq!(acked.json()["state"], "active_acked");
    let too_long = f
        .call(
            "POST",
            "/v1/alarms/{alarm}/shelve",
            "/v1/alarms/joker.output.pi.p_set.budget/shelve",
            Some(json!({"ttl": "25h", "reason": "x"})),
        )
        .await;
    assert_eq!(too_long.json()["code"], "J-E1202");

    let released = f
        .call(
            "POST",
            "/v1/outputs/{path}/release",
            "/v1/outputs/pi.p_set/release",
            Some(json!({})),
        )
        .await;
    assert_eq!(released.status, 200, "{}", released.json());
    wait_alarm(&f.http, "joker.output.pi.p_set.budget", "normal").await;
    wait_alarm(
        &f.http,
        "joker.output.pi.p_set.manual_expiring",
        "rtn_unacked",
    )
    .await;

    let mut kinds: Vec<(Value, Value)> = Vec::new();
    for _ in 0..200 {
        let journal = f
            .call(
                "GET",
                "/v1/alarm-events",
                "/v1/alarm-events?sel=joker.**&order=asc",
                None,
            )
            .await;
        kinds = journal.json()["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["id"] == "joker.output.pi.p_set.budget")
            .map(|e| (e["kind"].clone(), e["to"].clone()))
            .collect();
        if kinds.len() >= 3 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        kinds,
        vec![
            (json!("transition"), json!("active_unacked")),
            (json!("ack"), json!("active_acked")),
            (json!("transition"), json!("normal")),
        ]
    );
    let mut logged: Vec<Value> = Vec::new();
    for _ in 0..200 {
        logged = decisions_from(f._dir.path(), 0)
            .into_iter()
            .filter(|e| e["record"] == "alarm" && e["id"] == "joker.output.pi.p_set.budget")
            .collect();
        if logged.len() >= 3 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(logged.len(), 3, "{logged:#?}");
}

/// A composite `std/led@1` over a simulated output and the implicit
/// `std/led@1` and `std/button@1` of the bench ESP32 (track part 7).
fn classes_manifest() -> String {
    "[project]\n\
     name  = \"classes-test\"\n\
     joker = \"1.0\"\n\
     mode  = \"dev\"\n\
     \n\
     [outputs.\"pi.act_led\"]\n\
     safe = false\n\
     \n\
     [outputs.\"esp.led\"]\n\
     safe = false\n\
     \n\
     [devices.pi]\n\
     driver = \"sim\"\n\
     \n\
     [devices.pi.signals]\n\
     act_led = { type = \"bool\", direction = \"output\" }\n\
     \n\
     [devices.esp]\n\
     driver  = \"modbus-tcp\"\n\
     address = \"127.0.0.1:1\"\n\
     unit_id = 1\n\
     profile = \"local/bench-esp32@1\"\n\
     \n\
     [instances.led_pi]\n\
     class = \"std/led@1\"\n\
     map   = { on = \"pi.act_led\" }\n"
        .to_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn class_methods_expand_into_manual_writes_of_the_caller() {
    let f = start_with(&classes_manifest()).await;
    let tpl = "/v1/instances/{instance}/methods/{method}";
    let call = |instance: &'static str, method: &'static str, body: Value| {
        let path = format!("/v1/instances/{instance}/methods/{method}");
        let f = &f;
        async move { f.call("POST", tpl, &path, Some(body)).await }
    };
    let on = call("led_pi", "on", json!({"ttl": "10s", "reason": "bench"})).await;
    assert_eq!(on.status, 200, "{}", on.json());
    let body = on.json();
    assert_eq!(body["instance"], "led_pi");
    assert_eq!(body["writes"][0]["output"], "pi.act_led");
    assert_eq!(body["writes"][0]["level"], "manual:4");
    assert_eq!(body["writes"][0]["outcome"]["outcome"], "accepted");
    assert_eq!(body["writes"][0]["value"], true);
    let toggle = call("led_pi", "toggle", json!({"ttl": "10s", "reason": "bench"})).await;
    assert_eq!(
        toggle.json()["writes"][0]["value"],
        false,
        "{}",
        toggle.json()
    );
    let set = call(
        "led_pi",
        "set",
        json!({"args": {"v": true}, "ttl": "10s", "reason": "bench"}),
    )
    .await;
    assert_eq!(set.json()["writes"][0]["value"], true, "{}", set.json());
    let implicit = call("esp", "on", json!({"ttl": "10s", "reason": "bench"})).await;
    assert_eq!(implicit.status, 200, "{}", implicit.json());
    assert_eq!(implicit.json()["writes"][0]["output"], "esp.led");
    assert_eq!(implicit.json()["writes"][0]["value"], true);
    for (instance, method, body, code) in [
        (
            "nobody",
            "on",
            json!({"ttl": "10s", "reason": "x"}),
            "J-E1410",
        ),
        (
            "led_pi",
            "fly",
            json!({"ttl": "10s", "reason": "x"}),
            "J-E1411",
        ),
        ("led_pi", "on", json!({"reason": "x"}), "J-E1101"),
        ("led_pi", "on", json!({"ttl": "10s"}), "J-E1106"),
        (
            "led_pi",
            "set",
            json!({"args": {"v": 3}, "ttl": "10s", "reason": "x"}),
            "J-E1412",
        ),
        (
            "led_pi",
            "on",
            json!({"args": {"x": 1}, "ttl": "10s", "reason": "x"}),
            "J-E1412",
        ),
    ] {
        let r = call(instance, method, body).await;
        assert_eq!(r.json()["code"], code, "{instance}.{method}: {}", r.json());
    }
}

#[derive(Default)]
struct CapturedShared(Mutex<Option<Arc<jokerd::shared::Shared>>>);

impl Devices for CapturedShared {
    fn start(
        &self,
        plan: &Arc<jokerd::plan::Plan>,
        shared: &Arc<jokerd::shared::Shared>,
    ) -> Box<dyn jokerd::devices::DeviceSet> {
        *self.0.lock().unwrap() = Some(Arc::clone(shared));
        NoDevices.start(plan, shared)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_output_state_shows_the_supervisor_force_safe_bit() {
    let manifest = r#"
[project]
name  = "force-safe"
joker = "1.0"
mode  = "dev"

[outputs."pi.act_led"]
safe = false

[devices.pi]
driver = "sysfs"

[devices.pi.signals]
act_led = { type = "bool", led = "ACT", direction = "output" }

[cycles.main]
period = "20ms"
"#;
    let captured = Arc::new(CapturedShared::default());
    let f = start_with_devices(manifest, Arc::clone(&captured) as Arc<dyn Devices>).await;
    let state = f
        .call("GET", "/v1/outputs/{path}", "/v1/outputs/pi.act_led", None)
        .await;
    assert_eq!(state.status, 200, "{}", state.json());
    assert_eq!(state.json()["force_safe"], false, "{}", state.json());

    let shared = captured.0.lock().unwrap().clone().unwrap();
    let (device, index) = shared
        .commit_bindings
        .iter()
        .enumerate()
        .find_map(|(d, b)| (!b.is_empty()).then_some((d, 0)))
        .unwrap();
    joker_hal::commit::ForceSafe::new(&shared.commit_boxes[device]).set(index, true);

    let forced = f
        .call("GET", "/v1/outputs/{path}", "/v1/outputs/pi.act_led", None)
        .await;
    assert_eq!(forced.json()["force_safe"], true, "{}", forced.json());
}

#[test]
fn a_wide_signal_is_readable_through_get_v1_values() {
    use joker_api::Backend;

    const WIDE_TOML: &str = r#"
[project]
name  = "api-wide-test"
joker = "1.0"
mode  = "dev"

[buses.i2c1]
kind = "i2c"
dev  = "/dev/i2c-1"

[devices.sensor]
driver  = "i2c"
bus     = "i2c1"
address = 0x40

[devices.sensor.signals]
temp = { type = "bytes", direction = "input", reg = 0, len = 2, poll = "20ms" }
"#;

    let dir = tempfile::tempdir().unwrap();
    let clock = Monotonic::start();
    let log = Arc::new(Mutex::new(
        DecisionLog::open(&dir.path().join("decisions.jsonl")).unwrap(),
    ));
    let mut c = Controller::new(
        dir.path(),
        log,
        Arc::new(NativeTasks),
        Arc::new(NoDevices),
        Box::new(move || Box::new(SystemPacer::new(clock, ClockQuality::Unsynced))),
        Config {
            min_window: Duration::from_millis(100),
            ..Config::default()
        },
    )
    .unwrap();
    let g = c
        .propose(WIDE_TOML, "", Some("bench".into()), false)
        .unwrap();
    c.activate(g.number, None, None).unwrap();
    let controller = Arc::new(Mutex::new(c));

    let node = Node::new(Arc::clone(&controller), clock, 0, dir.path()).unwrap();
    {
        let guard = controller.lock().unwrap();
        let (_, _, running) = guard.current().unwrap();
        let id = running.plan.id("sensor.temp").unwrap();
        running.shared.wide_slot(id).unwrap().write(
            &[0xAB, 0xCD],
            joker_core::Quality::GOOD,
            joker_core::value::NO_TS_SRC,
            clock.now_ns(),
        );
    }

    let sel = ("sel".to_owned(), "sensor.temp".to_owned());
    let reply = node.values(&vec![sel.clone()]).unwrap();
    let items = reply.body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "{}", reply.body);
    assert_eq!(items[0]["path"], json!("sensor.temp"));
    assert_eq!(items[0]["value"], json!("q80="), "{}", reply.body);
    assert_eq!(items[0]["quality"], json!("good"));
    assert_eq!(items[0]["seq"], json!(1));

    let reply = node
        .signals(&vec![sel, ("values".to_owned(), "true".to_owned())])
        .unwrap();
    let sample = &reply.body["items"][0]["sample"];
    assert_eq!(sample["value"], json!("q80="), "{}", reply.body);
    assert_eq!(sample["seq"], json!(1));
}

/// `GET /v1/groups` (ref/09 §5.2) with the measured WCET of each task,
/// and the runtime's own signals and drops (ADR 0090).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn groups_and_runtime_signals_follow_the_contract() {
    let f = start().await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let g = f.call("GET", "/v1/groups", "/v1/groups", None).await;
    assert_eq!(g.status, 200, "{}", g.json());
    let main = &g.json()["items"][0];
    assert_eq!(main["name"], "main");
    assert!(main["period"].is_string(), "{main}");
    assert!(main["stats"]["ticks"].as_u64().unwrap() > 0, "{main}");
    let task = &main["tasks"][0];
    assert!(
        task["app"].is_string() && task["task"].is_string(),
        "{main}"
    );
    assert!(task["wcet_measured_p999"].is_string(), "{main}");
    let one = f
        .call("GET", "/v1/groups/{group}", "/v1/groups/main", None)
        .await;
    assert_eq!(one.status, 200);
    assert_eq!(one.json()["name"], "main");
    let missing = f
        .call("GET", "/v1/groups/{group}", "/v1/groups/nope", None)
        .await;
    assert_eq!(missing.status, 404);

    let m = f.call("GET", "/v1/metrics", "/v1/metrics", None).await;
    assert!(
        m.json()["runtime"]["dropped"]["decisions"].is_u64(),
        "{}",
        m.json()
    );
    assert!(
        m.json()["arbiter"]["leases_expiring"].is_u64(),
        "{}",
        m.json()
    );
    let v = f
        .call(
            "GET",
            "/v1/values",
            "/v1/values?sel=joker.group.main.ticks",
            None,
        )
        .await;
    assert_eq!(v.json()["items"][0]["quality"], "good", "{}", v.json());
}

/// What the instances of [`Counting`] report: how many were made, how many
/// ticks ran, and which instance ran last.
#[derive(Default)]
struct Probe {
    made: std::sync::atomic::AtomicUsize,
    runs: std::sync::atomic::AtomicUsize,
    last: std::sync::atomic::AtomicUsize,
}

struct Counting(Arc<Probe>);

struct Counted {
    id: usize,
    probe: Arc<Probe>,
}

impl jokerd::task::CycleTask for Counted {
    fn run(
        &mut self,
        _cx: &mut jokerd::task::TaskContext<'_, '_, '_>,
    ) -> Result<(), jokerd::task::TaskError> {
        use std::sync::atomic::Ordering;
        self.probe.runs.fetch_add(1, Ordering::SeqCst);
        self.probe.last.store(self.id, Ordering::SeqCst);
        Ok(())
    }
}

impl jokerd::task::TaskProvider for Counting {
    fn interfaces(
        &self,
    ) -> std::collections::BTreeMap<String, joker_manifest::context::AppInterface> {
        jokerd::task::TaskProvider::interfaces(&NativeTasks)
    }

    fn instantiate(
        &self,
        _spec: jokerd::task::TaskSpec<'_>,
    ) -> Result<Box<dyn jokerd::task::CycleTask>, jokerd::task::TaskError> {
        let id = self
            .0
            .made
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        Ok(Box::new(Counted {
            id,
            probe: Arc::clone(&self.0),
        }))
    }
}

/// `jk app stop|start|restart` (ref/09 §5.14, ADR 0091): a stopped app's
/// tasks skip their ticks, a started one runs again, and a restart swaps a
/// fresh instance in.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_app_stops_starts_and_restarts_on_the_node() {
    use std::sync::atomic::Ordering;
    let probe = Arc::new(Probe::default());
    let f = start_with_tasks(
        &effective(),
        Arc::new(NoDevices),
        Arc::new(Counting(Arc::clone(&probe))),
    )
    .await;
    let apps = f.call("GET", "/v1/apps", "/v1/apps", None).await;
    assert_eq!(apps.status, 200, "{}", apps.json());
    let app = apps.json()["items"][0]["name"].as_str().unwrap().to_owned();
    assert_eq!(apps.json()["items"][0]["state"], "running");
    let one = f
        .call("GET", "/v1/apps/{app}", &format!("/v1/apps/{app}"), None)
        .await;
    assert_eq!(one.json()["name"], app.as_str());

    let stop = format!("/v1/apps/{app}/stop");
    let refused = f
        .call("POST", "/v1/apps/{app}/stop", &stop, Some(json!({})))
        .await;
    assert_eq!(refused.status, 400, "{}", refused.json());
    let stopped = f
        .call(
            "POST",
            "/v1/apps/{app}/stop",
            &stop,
            Some(json!({"reason": "maintenance"})),
        )
        .await;
    assert_eq!(stopped.status, 200, "{}", stopped.json());
    assert_eq!(stopped.json()["state"], "stopped");
    tokio::time::sleep(Duration::from_millis(100)).await;
    let frozen = probe.runs.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        probe.runs.load(Ordering::SeqCst),
        frozen,
        "a stopped app runs"
    );

    let started = f
        .call(
            "POST",
            "/v1/apps/{app}/start",
            &format!("/v1/apps/{app}/start"),
            Some(json!({})),
        )
        .await;
    assert_eq!(started.json()["state"], "running", "{}", started.json());
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(probe.runs.load(Ordering::SeqCst) > frozen);
    assert_eq!(probe.last.load(Ordering::SeqCst), 1);

    let restarted = f
        .call(
            "POST",
            "/v1/apps/{app}/restart",
            &format!("/v1/apps/{app}/restart"),
            Some(json!({"reason": "fresh state"})),
        )
        .await;
    assert_eq!(restarted.status, 200, "{}", restarted.json());
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(probe.made.load(Ordering::SeqCst), 2);
    assert_eq!(
        probe.last.load(Ordering::SeqCst),
        2,
        "the fresh instance runs"
    );

    let unknown = f
        .call(
            "POST",
            "/v1/apps/{app}/start",
            "/v1/apps/nope/start",
            Some(json!({})),
        )
        .await;
    assert_eq!(unknown.status, 404);
}

/// `OutputState.envelope` (ref/09 §5.6) with `reads`, what `jk why`
/// shows (ADR 0091): the interlock's inputs and `requires_good`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_output_serves_its_envelope_and_the_signals_it_reads() {
    let manifest = effective()
        .replace(
            "[outputs.\"pi.act_led\"]\nsafe = false\n",
            "[outputs.\"pi.act_led\"]\nsafe = false\ninterlock = \"pi.door\"\nrequires_good = [\"pi.temp\"]\n",
        )
        .replace(
            "act_led = { type = \"bool\", direction = \"output\" }\n",
            "act_led = { type = \"bool\", direction = \"output\" }\ndoor = { type = \"bool\", direction = \"input\" }\ntemp = { type = \"f64\", direction = \"input\", unit = \"°C\" }\n",
        );
    let f = start_with(&manifest).await;
    let o = f
        .call("GET", "/v1/outputs/{path}", "/v1/outputs/pi.act_led", None)
        .await;
    assert_eq!(o.status, 200, "{}", o.json());
    let e = &o.json()["envelope"];
    assert_eq!(e["interlock"], "pi.door", "{e}");
    assert_eq!(e["requires_good"], json!(["pi.temp"]), "{e}");
    assert_eq!(e["reads"], json!(["pi.door", "pi.temp"]), "{e}");
    assert_eq!(e["on_interlock"], "safe", "{e}");
}

/// `GET /v1/signals/{path}/lineage` (ref/09 §5.4): the bench's LED is
/// written by `light` and owned by `main`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_signal_tells_its_lineage() {
    let f = start().await;
    let l = f
        .call(
            "GET",
            "/v1/signals/{path}/lineage",
            "/v1/signals/pi.act_led/lineage",
            None,
        )
        .await;
    assert_eq!(l.status, 200, "{}", l.json());
    assert_eq!(l.json()["canonical"], "pi.act_led");
    assert_eq!(l.json()["source"]["device"], "pi");
    assert_eq!(l.json()["output"]["group"], "main");
    let missing = f
        .call(
            "GET",
            "/v1/signals/{path}/lineage",
            "/v1/signals/pi.nope/lineage",
            None,
        )
        .await;
    assert_eq!(missing.status, 404);
}

/// `jk doctor` (ref/09 §5.17): `POST /v1/doctor` starts a `doctor` job,
/// `GET /v1/jobs/{job}` follows it to its report, and `GET /v1/doctor`
/// keeps the last one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_doctor_runs_as_a_job_and_keeps_its_report() {
    let f = start().await;
    let none = f.call("GET", "/v1/doctor", "/v1/doctor", None).await;
    assert_eq!(none.status, 404);
    let job = f
        .call(
            "POST",
            "/v1/doctor",
            "/v1/doctor",
            Some(json!({"checks": ["rt", "network"], "rt_duration": "200ms"})),
        )
        .await;
    assert_eq!(job.status, 202, "{}", job.json());
    assert_eq!(job.json()["kind"], "doctor");
    let id = job.json()["id"].as_str().unwrap().to_owned();
    let done = loop {
        let j = f
            .call("GET", "/v1/jobs/{job}", &format!("/v1/jobs/{id}"), None)
            .await;
        if j.json()["state"] == "succeeded" {
            break j.json();
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let report = &done["result"];
    assert!(
        matches!(report["summary"].as_str(), Some("ok" | "warn" | "fail")),
        "{report}"
    );
    let ids: Vec<&str> = report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["id"].as_str())
        .collect();
    for id in [
        "kernel.preempt_rt",
        "disk",
        "groups",
        "rt.cyclictest",
        "network",
    ] {
        assert!(ids.contains(&id), "{id} in {ids:?}");
    }
    let rt = report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "rt.cyclictest")
        .unwrap();
    assert!(rt["measured"]["samples"].as_u64().unwrap() > 50, "{rt}");
    let groups = report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "groups")
        .unwrap();
    assert_eq!(groups["status"], "ok", "{groups}");
    let last = f.call("GET", "/v1/doctor", "/v1/doctor", None).await;
    assert_eq!(last.status, 200, "{}", last.json());
    assert_eq!(last.json()["id"], report["id"]);
    let refused = f
        .call(
            "POST",
            "/v1/doctor",
            "/v1/doctor",
            Some(json!({"checks": ["bogus"]})),
        )
        .await;
    assert_eq!(refused.status, 400);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_cause_filter_returns_the_writes_that_read_a_signal() {
    let f = start().await;
    let path = f._dir.path().join("decisions.jsonl");
    let mut log = DecisionLog::open(&path).unwrap();
    for (read, value) in [("meter.p_total", true), ("meter.q_total", false)] {
        let mut entry = serde_json::Map::new();
        for (k, v) in [
            ("gen", json!(1)),
            ("record", json!("write")),
            ("writer", json!("app:light")),
            ("output", json!("pi.act_led")),
            ("tick", json!(9)),
            ("applied", json!(value)),
            ("outcome", json!("accepted")),
            ("level", json!("control:12")),
            ("cause", json!({"kind": "proposal", "writer": "app:light"})),
            (
                "inputs",
                json!([{"path": read, "seq": 4, "value": 1.5, "quality": "good"}]),
            ),
            ("inputs_omitted", json!(3)),
            ("ts", json!("2026-10-24T12:00:00.000000000Z")),
        ] {
            entry.insert(k.into(), v);
        }
        log.append(entry, true).unwrap();
    }

    let d = f
        .call(
            "GET",
            "/v1/decisions",
            "/v1/decisions?cause=meter.p_total",
            None,
        )
        .await;
    assert_eq!(d.status, 200);
    let items = d.json()["items"].as_array().unwrap().clone();
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0]["applied"], true);
    assert_eq!(items[0]["inputs"][0]["path"], "meter.p_total");
    assert_eq!(items[0]["inputs"][0]["seq"], 4);
    assert_eq!(items[0]["inputs_omitted"], 3);

    let none = f
        .call("GET", "/v1/decisions", "/v1/decisions?cause=meter.f", None)
        .await;
    assert_eq!(none.json()["items"].as_array().unwrap().len(), 0);
}
