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
use jokerd::devices::NoDevices;
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
        let mut stream = tokio::net::UnixStream::connect(&self.socket).await.unwrap();
        let payload = body.map(|b| b.to_string()).unwrap_or_default();
        let mut req = format!("{method} {path} HTTP/1.1\r\nhost: joker\r\nconnection: close\r\n");
        for (k, v) in headers {
            req.push_str(&format!("{k}: {v}\r\n"));
        }
        if body.is_some() {
            req.push_str(&format!(
                "content-type: application/json\r\ncontent-length: {}\r\n",
                payload.len()
            ));
        }
        req.push_str("\r\n");
        req.push_str(&payload);
        stream.write_all(req.as_bytes()).await.unwrap();
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
}

async fn start() -> Fixture {
    start_with(&effective()).await
}

async fn start_with(manifest: &str) -> Fixture {
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
        .propose(manifest, "", Some("bench".into()), false)
        .unwrap();
    c.activate(g.number, None, None).unwrap();
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_action_catalog_follows_the_contract() {
    let f = start().await;
    let catalog = f.call("GET", "/v1/actions", "/v1/actions", None).await;
    assert_eq!(catalog.status, 200, "{}", catalog.json());
    // The fixture's only device is `sim`, which declares no actions.
    assert_eq!(catalog.json()["items"].as_array().unwrap().len(), 0);
}

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

    // An out-of-range manual write trips it (on_range = "trip").
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

    // Still held by the same out-of-range write: reset clears the trip
    // for a tick, but the same condition re-trips it (docs/16 E7).
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

    // Release the bad write, then reset again: this time it clears.
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

    // A reason is mandatory.
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
        .filter(|e| e["record"] == "state_write_failed")
        .collect();
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert_eq!(failures[0]["file"], "state/manual");
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
