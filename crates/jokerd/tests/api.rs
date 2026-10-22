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

struct Fixture {
    http: Http,
    contract: Contract,
    _dir: tempfile::TempDir,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
}

async fn start() -> Fixture {
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
        .propose(&effective(), "", Some("bench".into()), false)
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
