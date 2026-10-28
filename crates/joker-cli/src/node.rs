//! The commands that talk to a node (docs/07 §3–§4, ref/09 §9.1).

use std::io::{BufRead, Write};
use std::path::Path;
use std::time::Duration;

use joker_client::{Client, Response, Target};
use serde_json::{Value, json};

use crate::Failure;
use crate::project::{self, Entry};

/// A node and how it was chosen.
pub struct Node {
    client: Client,
    from: &'static str,
    json: bool,
}

fn print_json(v: &Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
}

fn checked(r: Response) -> Result<Response, Failure> {
    if r.ok() {
        Ok(r)
    } else {
        Err(r.problem().into())
    }
}

/// `jk check`.
pub fn check(entry: &Entry, json: bool) -> Result<u8, Failure> {
    let tasks = project::tasks(entry)?;
    let report = project::check(entry, &tasks);
    if json {
        print_json(&json!({
            "ok": !report.has_errors(),
            "codes": report.codes(),
            "hash": report.hash,
        }));
    } else {
        let rendered = report.render();
        if !rendered.is_empty() {
            eprintln!("{rendered}");
        }
    }
    if report.has_errors() {
        return Ok(2);
    }
    if !json && let Some(m) = &report.manifest {
        println!(
            "✓ manifest  {} · {} device(s), {} output(s), {} app(s), {} group(s)",
            entry.manifest.display(),
            m.devices.len(),
            m.outputs.len(),
            m.apps.len(),
            m.cycles.len(),
        );
        if let Some(h) = &report.hash {
            println!("  effective {h}");
        }
    }
    Ok(0)
}

impl Node {
    /// Connects to `--node` (or the local runtime).
    pub fn open(node: Option<&str>, timeout: Duration, json: bool) -> Result<Self, Failure> {
        let (target, from) = match node {
            Some(n) => (
                joker_client::resolve(n, std::env::current_dir().ok().as_deref())?,
                if std::env::var("JK_NODE").ok().as_deref() == Some(n) {
                    "JK_NODE"
                } else {
                    "--node"
                },
            ),
            None => (Target::Unix(joker_client::DEFAULT_SOCKET.into()), "default"),
        };
        Ok(Self {
            client: Client::connect(target, timeout)?,
            from,
            json,
        })
    }

    fn header(&self) -> Result<String, Failure> {
        let info = checked(self.client.get("/v1/info")?)?.json();
        Ok(format!(
            "→ {} ({}) · from {} · {}",
            info["node"].as_str().unwrap_or("?"),
            info["mode"].as_str().unwrap_or("?"),
            self.from,
            self.client.target()
        ))
    }

    fn current_hash(&self) -> Result<Option<String>, Failure> {
        let r = self.client.get("/v1/generations/current")?;
        if r.status == 404 {
            return Ok(None);
        }
        Ok(checked(r)?.json()["hash"].as_str().map(str::to_owned))
    }

    /// `jk apply`.
    pub fn apply(
        &self,
        entry: &Entry,
        confirm: Option<&str>,
        dry_run: bool,
        message: Option<String>,
    ) -> Result<u8, Failure> {
        let tasks = project::tasks(entry)?;
        let report = project::check(entry, &tasks);
        if report.has_errors() {
            eprintln!("{}", report.render());
            return Ok(2);
        }
        let manifest = project::effective_toml(entry).map_err(Failure::usage)?;
        if !self.json {
            println!("{}", self.header()?);
        }
        let base = self.current_hash()?;
        let artifacts = self.upload_artifacts(entry)?;
        let mut proposal = json!({
            "manifest": {"content": manifest},
            "lock": {"content": project::lock_text(entry)},
            "artifacts": artifacts,
            "base": base,
        });
        if let Some(m) = message {
            proposal["message"] = json!(m);
        }
        let path = if dry_run {
            "/v1/generations?dry_run=true"
        } else {
            "/v1/generations"
        };
        let created = checked(self.client.post(path, &proposal, &[])?)?.json();
        if dry_run {
            if self.json {
                print_json(&created);
            } else {
                println!(
                    "valid · generation would be {}",
                    created["hash"].as_str().unwrap_or("?")
                );
            }
            return Ok(0);
        }
        let number = created["gen"].as_u64().unwrap_or(0);
        if !self.json {
            println!(
                "plan of generation {number}  {}",
                created["hash"].as_str().unwrap_or("")
            );
        }
        let if_match = base.unwrap_or_else(|| "null".into());
        let mut body = json!({});
        if let Some(w) = confirm {
            body["confirm_window"] = json!(w);
        }
        let activated = checked(self.client.post(
            &format!("/v1/generations/{number}/activate"),
            &body,
            &[("if-match", &format!("\"{if_match}\""))],
        )?)?
        .json();
        if self.json {
            print_json(&activated);
        } else if activated["state"] == "confirming" {
            println!(
                "applied. confirmation pending: `jk confirm` before {}, or automatic rollback",
                activated["confirm_deadline"]
                    .as_str()
                    .unwrap_or("the deadline")
            );
        } else {
            println!(
                "applied. generation {number} is {}",
                activated["state"].as_str().unwrap_or("?")
            );
        }
        Ok(0)
    }

    /// Builds the Wasm apps, precompiles them for the node's triple, and
    /// uploads what the node lacks (ref/09 §5.13, ADR 0067), with the
    /// project's profile and board files. Returns the `artifacts` of the
    /// proposal: per app, its `.wasm` and, when the triple has an engine,
    /// its `.cwasm`; per profile or board, its file by its path in the
    /// project (`profiles/acme/pcs@1.profile.toml`), so the generation
    /// carries them (ref/10 §2.6.2).
    fn upload_artifacts(&self, entry: &Entry) -> Result<Vec<Value>, Failure> {
        let root = entry
            .manifest
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(std::path::Path::new("."));
        let (catalog, errors) = jokerd::catalog::catalog_files(root);
        if let Some(e) = errors.first() {
            return Err(Failure::usage(e.clone()));
        }
        let triple = crate::apps::node_triple(entry).map_err(Failure::usage)?;
        let built = match crate::apps::build_all(entry, Some(&triple)) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("warning: no AOT artifact for {triple}: {e}");
                crate::apps::build_all(entry, None).map_err(Failure::usage)?
            }
        };
        let mut files: Vec<(String, &'static str, &[u8])> = Vec::new();
        for app in &built {
            let name = format!("{}{}", jokerd::apps::APP_PREFIX, app.name);
            files.push((name.clone(), jokerd::apps::APP_WASM, &app.wasm));
            if let Some((cwasm, _)) = &app.cwasm {
                files.push((name, jokerd::apps::APP_CWASM, cwasm));
            }
        }
        for (name, text) in &catalog {
            files.push((
                name.clone(),
                jokerd::catalog::kind_of(name),
                text.as_bytes(),
            ));
        }
        if files.is_empty() {
            return Ok(Vec::new());
        }
        let digests: Vec<String> = files
            .iter()
            .map(|(_, _, b)| jokerd::blobs::digest_of(b))
            .collect();
        let missing = checked(self.client.post(
            "/v1/blobs/missing",
            &json!({ "digests": digests }),
            &[],
        )?)?
        .json();
        let missing: Vec<&str> = missing["missing"]
            .as_array()
            .map(|m| m.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let mut out = Vec::with_capacity(files.len());
        for ((name, kind, bytes), digest) in files.iter().zip(&digests) {
            if missing.contains(&digest.as_str()) {
                checked(self.client.post_bytes(
                    &format!(
                        "/v1/blobs/uploads?digest={digest}&kind={}",
                        kind.replace('+', "%2B")
                    ),
                    bytes,
                )?)?;
            }
            out.push(json!({
                "name": name,
                "kind": kind,
                "digest": digest,
                "size": bytes.len(),
            }));
        }
        Ok(out)
    }

    /// `jk confirm`.
    pub fn confirm(&self) -> Result<u8, Failure> {
        let r = self
            .client
            .post("/v1/generations/confirming/confirm", &json!({}), &[])?;
        if !r.ok() {
            let mut e: Failure = r.problem().into();
            if e.message.contains("rolled_back") || r.json()["code"] == "J-E1600" {
                let list = self.client.get("/v1/generations?state=rolled_back")?.json();
                if list["items"].as_array().is_some_and(|a| !a.is_empty()) {
                    e.message = format!("{}\nthe generation was rolled back", e.message);
                    e.exit = 5;
                }
            }
            return Err(e);
        }
        let g = r.json();
        if self.json {
            print_json(&g);
        } else {
            println!("generation {} confirmed", g["gen"]);
        }
        Ok(0)
    }

    /// `jk rollback`.
    pub fn rollback(&self, to: Option<&str>, reason: &str) -> Result<u8, Failure> {
        let base = self.current_hash()?.unwrap_or_else(|| "null".into());
        let mut body = json!({"reason": reason});
        if let Some(t) = to {
            body["to"] = json!(t);
        }
        let g = checked(self.client.post(
            "/v1/generations/rollback",
            &body,
            &[("if-match", &format!("\"{base}\""))],
        )?)?
        .json();
        if self.json {
            print_json(&g);
        } else {
            println!(
                "generation {} reactivated ({})",
                g["gen"],
                g["state"].as_str().unwrap_or("?")
            );
        }
        Ok(0)
    }

    /// `jk generations`.
    pub fn generations(&self) -> Result<u8, Failure> {
        let list = checked(self.client.get("/v1/generations")?)?.json();
        if self.json {
            print_json(&list);
            return Ok(0);
        }
        for g in list["items"].as_array().cloned().unwrap_or_default() {
            println!(
                "{:>4}  {:<11}  {}  {}",
                g["gen"],
                g["state"].as_str().unwrap_or(""),
                &g["hash"].as_str().unwrap_or("")[..19.min(g["hash"].as_str().unwrap_or("").len())],
                g["message"].as_str().unwrap_or("")
            );
        }
        Ok(0)
    }

    /// `jk status`.
    pub fn status(&self) -> Result<u8, Failure> {
        let s = checked(self.client.get("/v1/status")?)?.json();
        if self.json {
            print_json(&s);
            return Ok(0);
        }
        println!("{}", self.header()?);
        println!(
            "generation {} ({})",
            s["generation"]["gen"],
            s["generation"]["state"].as_str().unwrap_or("?")
        );
        println!(
            "clock {} · {}",
            s["clock"]["quality"].as_str().unwrap_or("?"),
            s["clock"]["utc"].as_str().unwrap_or("?")
        );
        for g in s["groups"].as_array().cloned().unwrap_or_default() {
            println!(
                "group {} {} · ticks {} · overruns {}",
                g["name"].as_str().unwrap_or(""),
                g["period"].as_str().unwrap_or(""),
                g["stats"]["ticks"],
                g["stats"]["overruns"]
            );
        }
        for m in s["manual"].as_array().cloned().unwrap_or_default() {
            println!(
                "manual {} {} = {} by {} until {}",
                m["output"].as_str().unwrap_or(""),
                m["level"].as_str().unwrap_or(""),
                m["value"],
                m["principal"].as_str().unwrap_or(""),
                m["until"].as_str().unwrap_or("")
            );
        }
        Ok(0)
    }

    /// `jk top` (ref/09 §9.1): a live view over `GET /v1/ws`, subscribed
    /// to `metrics`, `status` and `outputs`, redrawn as the node sends them
    /// until Ctrl-C or until the node closes the subscription (exit 4).
    /// With `--json`, prints the first `Metrics` and exits.
    pub fn top(&self) -> Result<u8, Failure> {
        let socket = self.client.socket().to_owned();
        let header = if self.json {
            String::new()
        } else {
            self.header()?
        };
        let json_mode = self.json;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| Failure::usage(format!("tokio: {e}")))?;
        runtime.block_on(top_view(&socket, &header, json_mode))
    }

    /// `jk read`.
    pub fn read(&self, selectors: &[String]) -> Result<u8, Failure> {
        let query: Vec<String> = selectors.iter().map(|s| format!("sel={s}")).collect();
        let v = checked(
            self.client
                .get(&format!("/v1/values?{}", query.join("&")))?,
        )?
        .json();
        if self.json {
            print_json(&v);
            return Ok(0);
        }
        print_samples(&v["items"]);
        Ok(0)
    }

    /// `jk read --watch`: `GET /v1/ws`'s `signals` topic (ref/09 §6),
    /// printing every `resync`/`data` as it arrives instead of reading
    /// once. Runs its own small tokio runtime; `jk`'s other commands
    /// are synchronous on purpose, and this is the one that genuinely
    /// needs to wait on a socket indefinitely.
    pub fn read_watch(&self, selectors: &[String]) -> Result<u8, Failure> {
        let socket = self.client.socket().to_owned();
        let sel: Vec<String> = selectors.to_vec();
        let json_mode = self.json;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| Failure::usage(format!("tokio: {e}")))?;
        runtime.block_on(watch_signals(&socket, &sel, json_mode))
    }

    /// `jk write`.
    #[allow(clippy::too_many_arguments)]
    pub fn write(
        &self,
        output: &str,
        value: &str,
        ttl: &str,
        reason: &str,
        level: Option<&str>,
        takeover: bool,
        yes: bool,
    ) -> Result<u8, Failure> {
        let state = checked(self.client.get(&format!("/v1/outputs/{output}"))?)?.json();
        let value: Value = match value {
            "true" => json!(true),
            "false" => json!(false),
            v => v.parse::<f64>().map_or_else(|_| json!(v), |n| json!(n)),
        };
        if !self.json {
            println!("{}", self.header()?);
            println!(
                "{output}: applied {} · active {} · safe {} · group {}",
                state["applied"],
                state["active_level"].as_str().unwrap_or("none"),
                state["safe"],
                state["group"].as_str().unwrap_or("")
            );
            println!(
                "write {value} at {} for {ttl}: {reason}",
                level.unwrap_or("manual:4")
            );
        }
        if !yes {
            eprint!("apply? [y/N] ");
            let _ = std::io::stderr().flush();
            let mut line = String::new();
            let _ = std::io::stdin().lock().read_line(&mut line);
            if !matches!(line.trim(), "y" | "Y" | "yes") {
                eprintln!("not written");
                return Ok(1);
            }
        }
        let mut body = json!({"value": value, "ttl": ttl, "reason": reason, "takeover": takeover});
        if let Some(l) = level {
            body["level"] = json!(l);
        }
        let ticket = checked(self.client.post(
            &format!("/v1/outputs/{output}/manual"),
            &body,
            &[],
        )?)?
        .json();
        if self.json {
            print_json(&ticket);
        }
        let outcome = ticket["outcome"]["outcome"].as_str().unwrap_or("queued");
        if !self.json {
            match ticket["outcome"]["applied"].clone() {
                Value::Null => println!("{outcome}"),
                applied => println!("{outcome} · applied {applied}"),
            }
        }
        Ok(match outcome {
            "rejected" | "tripped" => 3,
            "overridden" => {
                eprintln!(
                    "warning: level {} still rules",
                    ticket["outcome"]["level"].as_str().unwrap_or("?")
                );
                0
            }
            _ => 0,
        })
    }

    /// `jk release`.
    pub fn release(&self, output: &str, level: Option<&str>, force: bool) -> Result<u8, Failure> {
        let mut body = json!({"force": force});
        if let Some(l) = level {
            body["level"] = json!(l);
        }
        let t = checked(
            self.client
                .post(&format!("/v1/outputs/{output}/release"), &body, &[])?,
        )?
        .json();
        if self.json {
            print_json(&t);
        } else {
            println!("released {} on {output}", t["level"].as_str().unwrap_or(""));
        }
        Ok(0)
    }

    /// `jk reset <output>`: ends a trip (docs/16 E7, ref/09 §5.7).
    pub fn reset(&self, output: &str, reason: &str) -> Result<u8, Failure> {
        let body = json!({"reason": reason, "wait": true});
        let t = checked(
            self.client
                .post(&format!("/v1/outputs/{output}/reset"), &body, &[])?,
        )?
        .json();
        if self.json {
            print_json(&t);
        } else {
            println!("{}", self.header()?);
            println!(
                "{output}: {}",
                t["result"].as_str().unwrap_or("?").replace('_', " ")
            );
        }
        Ok(match t["result"].as_str() {
            Some("cleared") | Some("not_tripped") => 0,
            _ => 3,
        })
    }

    /// `jk trips`: lists tripped outputs (docs/16 E7).
    pub fn trips(&self) -> Result<u8, Failure> {
        let t = checked(self.client.get("/v1/trips")?)?.json();
        if self.json {
            print_json(&t);
            return Ok(0);
        }
        println!("{}", self.header()?);
        let items = t["items"].as_array().cloned().unwrap_or_default();
        if items.is_empty() {
            println!("no tripped outputs");
        }
        for item in items {
            println!(
                "{}  since {}  cause {}",
                item["output"].as_str().unwrap_or("?"),
                item["since"].as_str().unwrap_or("?"),
                item["cause"]["kind"].as_str().unwrap_or("?"),
            );
        }
        Ok(0)
    }

    /// `jk explain <output>`: the arbiter state of an output (docs/07) —
    /// owning group, occupied levels, active level, envelope and last
    /// result. The other forms docs/07 lists (a manifest key's origin,
    /// an error code's full page, a pipeline's plan) are not built yet;
    /// an argument is always read as an output path here.
    pub fn explain(&self, target: &str) -> Result<u8, Failure> {
        let state = checked(self.client.get(&format!("/v1/outputs/{target}"))?)?.json();
        if self.json {
            print_json(&state);
            return Ok(0);
        }
        println!("{}", self.header()?);
        print!("{}", render_output_explain(target, &state));
        Ok(0)
    }

    /// `jk action`.
    pub fn action(&self, target_action: &str, args: &[String], key: &str) -> Result<u8, Failure> {
        let (target, action) = split_target_action(target_action)?;
        let body = json!({"args": parse_args(args)?});
        let invocation = checked(self.client.send(
            "POST",
            &format!("/v1/actions/{target}/{action}"),
            &[("idempotency-key", key)],
            Some(&body),
        )?)?
        .json();
        if self.json {
            print_json(&invocation);
        } else {
            println!("{}", self.header()?);
            match invocation["state"].as_str().unwrap_or("?") {
                "succeeded" => println!("{target_action}: succeeded"),
                "failed" => println!(
                    "{target_action}: failed · {} ({})",
                    invocation["error"]["message"].as_str().unwrap_or(""),
                    invocation["error"]["code"].as_str().unwrap_or(""),
                ),
                other => println!("{target_action}: {other}"),
            }
        }
        Ok(match invocation["state"].as_str() {
            Some("succeeded") => 0,
            Some("failed") => 3,
            _ => 0,
        })
    }

    /// `jk call <instance>.<method> [name=value …] --ttl <t> --reason "…"`:
    /// calls a class method as a manual writer (docs/17 §4.1, ref/09 §5.11).
    pub fn call(
        &self,
        target_method: &str,
        args: &[String],
        ttl: &str,
        reason: &str,
        level: Option<&str>,
        takeover: bool,
    ) -> Result<u8, Failure> {
        let (instance, method) = split_target_action(target_method)?;
        let mut body = json!({
            "args": parse_args(args)?,
            "ttl": ttl,
            "reason": reason,
            "takeover": takeover,
        });
        if let Some(l) = level {
            body["level"] = json!(l);
        }
        let result = checked(self.client.post(
            &format!("/v1/instances/{instance}/methods/{method}"),
            &body,
            &[],
        )?)?
        .json();
        if self.json {
            print_json(&result);
            return Ok(0);
        }
        println!("{}", self.header()?);
        let mut code = 0;
        for w in result["writes"].as_array().cloned().unwrap_or_default() {
            let outcome = w["outcome"]["outcome"].as_str().unwrap_or("queued");
            println!(
                "{:<24} {:<10} {} · applied {}",
                w["output"].as_str().unwrap_or(""),
                w["value"],
                outcome,
                w["outcome"]["applied"]
            );
            if matches!(outcome, "rejected" | "tripped") {
                code = 3;
            }
        }
        Ok(code)
    }

    /// `jk decisions`.
    pub fn decisions(&self, outputs: &[String], limit: u32) -> Result<u8, Failure> {
        let mut q = vec![format!("limit={limit}")];
        q.extend(outputs.iter().map(|o| format!("output={o}")));
        let d = checked(self.client.get(&format!("/v1/decisions?{}", q.join("&")))?)?.json();
        if self.json {
            print_json(&d);
            return Ok(0);
        }
        for e in d["items"].as_array().cloned().unwrap_or_default() {
            println!(
                "{}  {:<20} {:<14} applied {:<8} level {:<14} {}",
                e["ts"].as_str().unwrap_or(""),
                e["output"].as_str().unwrap_or(""),
                e["outcome"]["outcome"]
                    .as_str()
                    .or_else(|| e["ack"]["status"].as_str())
                    .unwrap_or(""),
                e["applied"].to_string(),
                e["active_level"].as_str().unwrap_or(""),
                e["writer"]["writer"].as_str().unwrap_or("")
            );
        }
        Ok(0)
    }

    /// `jk logs` (ref/09 §9.1): one page of `GET /v1/logs`, printed oldest
    /// first.
    pub fn logs(&self, params: &[(&str, Option<String>)]) -> Result<u8, Failure> {
        let q: Vec<String> = params
            .iter()
            .filter_map(|(k, v)| v.as_ref().map(|v| format!("{k}={}", encode(v))))
            .collect();
        let page = checked(self.client.get(&format!("/v1/logs?{}", q.join("&")))?)?.json();
        if self.json {
            print_json(&page);
            return Ok(0);
        }
        let mut items = page["items"].as_array().cloned().unwrap_or_default();
        items.reverse();
        for e in items {
            let fields: String = e["fields"]
                .as_object()
                .map(|f| {
                    f.iter()
                        .map(|(k, v)| format!(" {k}={}", v.as_str().unwrap_or("")))
                        .collect()
                })
                .unwrap_or_default();
            println!(
                "{}  {:<5} {:<16} {}{fields}",
                e["ts"].as_str().unwrap_or(""),
                e["level"].as_str().unwrap_or(""),
                e["source"].as_str().unwrap_or(""),
                e["msg"].as_str().unwrap_or("")
            );
        }
        Ok(0)
    }

    /// `jk history` (ref/09 §9.1).
    pub fn history(&self, args: &HistoryArgs) -> Result<u8, Failure> {
        let mut q: Vec<String> = args
            .selectors
            .iter()
            .map(|s| format!("sel={}", encode(s)))
            .collect();
        for (key, value) in [
            ("from", &args.from),
            ("to", &args.to),
            ("step", &args.step),
            ("agg", &args.agg),
            ("tier", &args.tier),
        ] {
            if let Some(v) = value {
                q.push(format!("{key}={}", encode(v)));
            }
        }
        let h = checked(self.client.get(&format!("/v1/history?{}", q.join("&")))?)?.json();
        if self.json {
            print_json(&h);
            return Ok(0);
        }
        println!(
            "{} .. {}  tier {}{}",
            h["from"].as_str().unwrap_or(""),
            h["to"].as_str().unwrap_or(""),
            h["tier"].as_str().unwrap_or(""),
            h["step"]
                .as_str()
                .map(|s| format!("  step {s}"))
                .unwrap_or_default()
        );
        for w in h["warnings"].as_array().cloned().unwrap_or_default() {
            println!("warning: {}", w["message"].as_str().unwrap_or(""));
        }
        for series in h["series"].as_array().cloned().unwrap_or_default() {
            let unit = series["unit"].as_str().unwrap_or("");
            println!("{} [{}]", series["path"].as_str().unwrap_or(""), unit);
            for p in series["points"].as_array().cloned().unwrap_or_default() {
                let fields: Vec<String> = ["value", "avg", "min", "max", "first", "last", "count"]
                    .iter()
                    .filter_map(|k| p.get(*k).map(|v| format!("{k}={v}")))
                    .collect();
                println!(
                    "  {}  {}  {}",
                    p["ts"].as_str().unwrap_or(""),
                    fields.join(" "),
                    p["quality"].as_str().unwrap_or("")
                );
            }
        }
        if let Some(next) = h["next_cursor"].as_str() {
            println!("(more points: cursor {next})");
        }
        Ok(0)
    }

    /// `jk alarms [--active]` (ref/09 §9.1).
    pub fn alarms(&self, active: bool) -> Result<u8, Failure> {
        let path = if active {
            "/v1/alarms?state=active"
        } else {
            "/v1/alarms"
        };
        let a = checked(self.client.get(path)?)?.json();
        let events = checked(self.client.get("/v1/alarm-events?limit=10")?)?.json();
        if self.json {
            print_json(&json!({"alarms": a["items"], "events": events["items"]}));
            return Ok(0);
        }
        for e in a["items"].as_array().cloned().unwrap_or_default() {
            let shelved = e["shelved"]["until"]
                .as_str()
                .map(|u| format!("  shelved until {u}"))
                .unwrap_or_default();
            println!(
                "{:<28} {:<9} {:<15} {}{}",
                e["id"].as_str().unwrap_or(""),
                e["severity"].as_str().unwrap_or(""),
                e["state"].as_str().unwrap_or(""),
                e["message"].as_str().unwrap_or(""),
                shelved
            );
        }
        let recent = events["items"].as_array().cloned().unwrap_or_default();
        if !recent.is_empty() {
            println!();
            for e in recent {
                println!(
                    "{}  {:<28} {:<14} {} -> {}{}",
                    e["ts"].as_str().unwrap_or(""),
                    e["id"].as_str().unwrap_or(""),
                    e["kind"].as_str().unwrap_or(""),
                    e["from"].as_str().unwrap_or(""),
                    e["to"].as_str().unwrap_or(""),
                    e["by"]
                        .as_str()
                        .map(|b| format!("  by {b}"))
                        .unwrap_or_default()
                );
            }
        }
        Ok(0)
    }

    fn alarm_post(&self, id: &str, op: &str, body: Value) -> Result<u8, Failure> {
        let a = checked(self.client.post(
            &format!("/v1/alarms/{}/{op}", encode(id)),
            &body,
            &[],
        )?)?
        .json();
        if self.json {
            print_json(&a);
        } else {
            let shelved = a["shelved"]["until"]
                .as_str()
                .map(|u| format!(", shelved until {u}"))
                .unwrap_or_default();
            println!(
                "{}: {}{}",
                a["id"].as_str().unwrap_or(id),
                a["state"].as_str().unwrap_or(""),
                shelved
            );
        }
        Ok(0)
    }

    /// `jk alarm ack <id>`.
    pub fn alarm_ack(&self, id: &str, reason: Option<&str>) -> Result<u8, Failure> {
        let body = match reason {
            Some(r) => json!({"reason": r}),
            None => json!({}),
        };
        self.alarm_post(id, "ack", body)
    }

    /// `jk alarm shelve <id> <ttl> "<reason>"`.
    pub fn alarm_shelve(&self, id: &str, ttl: &str, reason: &str) -> Result<u8, Failure> {
        self.alarm_post(id, "shelve", json!({"ttl": ttl, "reason": reason}))
    }

    /// `jk alarm unshelve <id>`.
    pub fn alarm_unshelve(&self, id: &str) -> Result<u8, Failure> {
        self.alarm_post(id, "unshelve", json!({}))
    }

    fn download(
        &self,
        created: Value,
        wait: Duration,
        out: &std::path::Path,
    ) -> Result<u8, Failure> {
        let id = created["id"].as_str().unwrap_or("").to_owned();
        let start = std::time::Instant::now();
        let mut trace = created;
        while trace["state"] == "recording" || trace["state"] == "building" {
            if start.elapsed() > wait {
                return Err(Failure {
                    message: format!("trace {id} is still {}", trace["state"]),
                    exit: 4,
                });
            }
            std::thread::sleep(Duration::from_millis(200));
            trace = checked(self.client.get(&format!("/v1/traces/{id}"))?)?.json();
        }
        if trace["state"] == "failed" {
            return Err(Failure {
                message: format!("trace {id} failed"),
                exit: 3,
            });
        }
        let file = checked(self.client.get(&format!("/v1/traces/{id}/file"))?)?;
        std::fs::write(out, &file.body)
            .map_err(|e| Failure::usage(format!("{}: {e}", out.display())))?;
        if self.json {
            print_json(&trace);
        } else {
            println!(
                "{} · {} bytes · {} .. {}",
                out.display(),
                file.body.len(),
                trace["from"].as_str().unwrap_or("?"),
                trace["to"]
                    .as_str()
                    .or_else(|| trace["ends_at"].as_str())
                    .unwrap_or("?")
            );
        }
        Ok(0)
    }

    /// `jk record`.
    pub fn record(
        &self,
        duration: &str,
        groups: &[String],
        out: &std::path::Path,
    ) -> Result<u8, Failure> {
        let length = jokerd::wire::parse_duration(duration)
            .map_err(|e| Failure::usage(format!("--for {e}")))?;
        let mut body = json!({"source": "record", "duration": duration});
        if !groups.is_empty() {
            body["groups"] = json!(groups);
        }
        if !self.json {
            println!("{}", self.header()?);
            println!("recording {duration}…");
        }
        let created = checked(self.client.post("/v1/traces", &body, &[])?)?.json();
        self.download(
            created,
            Duration::from_nanos(length) + Duration::from_secs(30),
            out,
        )
    }

    /// `jk trace-dump`.
    pub fn trace_dump(&self, last: Option<&str>, out: &std::path::Path) -> Result<u8, Failure> {
        let mut body = json!({"source": "ring"});
        if let Some(l) = last {
            body["last"] = json!(l);
        }
        let created = checked(self.client.post("/v1/traces", &body, &[])?)?.json();
        self.download(created, Duration::from_secs(30), out)
    }
}

/// Renders an `OutputState` (ref/09 §5.6) as `jk explain <output>`'s
/// human-readable output.
fn render_output_explain(target: &str, state: &Value) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{target}  group {}",
        state["group"].as_str().unwrap_or("?")
    );
    let _ = writeln!(out, "  safe        {}", state["safe"]);
    let _ = writeln!(out, "  effective   {}", state["effective"]);
    let _ = writeln!(out, "  applied     {}", state["applied"]);
    let _ = writeln!(
        out,
        "  active      {}",
        state["active_level"].as_str().unwrap_or("none")
    );
    if let Some(outcome) = state.get("last_outcome").filter(|v| !v.is_null()) {
        let _ = writeln!(
            out,
            "  last result {}",
            outcome["outcome"].as_str().unwrap_or("?")
        );
    }
    let _ = writeln!(out, "  tripped     {}", state["tripped"]);
    if state["force_safe"] == true {
        let _ = writeln!(
            out,
            "  force_safe  true (the supervisor holds the output at safe)"
        );
    }
    for level in state["levels"].as_array().cloned().unwrap_or_default() {
        let value = level.get("value").cloned().unwrap_or(Value::Null);
        let active = level["active"].as_bool().unwrap_or(false);
        let _ = writeln!(
            out,
            "  {:<12} {:<20} {value} {}",
            level["level"].as_str().unwrap_or("?"),
            level["writer"].as_str().unwrap_or("?"),
            if active { "(active)" } else { "" },
        );
    }
    out
}

/// `<target>.<action>` (docs/07, ref/09 §5.10), split on the first `.`.
fn split_target_action(s: &str) -> Result<(&str, &str), Failure> {
    s.split_once('.')
        .filter(|(t, a)| !t.is_empty() && !a.is_empty())
        .ok_or_else(|| Failure::usage(format!("`{s}` must be `<target>.<action>`")))
}

/// `name=value` positional args into the `InvokeRequest.args` object
/// (ref/09 §5.10), with `jk write`'s own bool/number/string fallback.
fn parse_args(pairs: &[String]) -> Result<Value, Failure> {
    let mut map = serde_json::Map::new();
    for pair in pairs {
        let (name, value) = pair
            .split_once('=')
            .ok_or_else(|| Failure::usage(format!("`{pair}` must be `name=value`")))?;
        let value = match value {
            "true" => json!(true),
            "false" => json!(false),
            v => v.parse::<f64>().map_or_else(|_| json!(v), |n| json!(n)),
        };
        map.insert(name.to_owned(), value);
    }
    Ok(Value::Object(map))
}

/// How often `jk read --watch` asks the node for changes: a reading a
/// person can follow, not the 100 ms default of a program (ref/09 §6.5).
const WATCH_INTERVAL: &str = "1s";

fn lost(message: String) -> Failure {
    Failure { message, exit: 4 }
}

fn print_samples(items: &Value) {
    for s in items.as_array().cloned().unwrap_or_default() {
        let quality = match s["reason"].as_str() {
            Some("none") | None => s["quality"].as_str().unwrap_or("").to_owned(),
            Some(r) => format!("{}({r})", s["quality"].as_str().unwrap_or("")),
        };
        println!(
            "{:<32} {:>12} {:<6} {:<20} age {}",
            s["path"].as_str().unwrap_or(""),
            s["value"].to_string(),
            s["unit"].as_str().unwrap_or(""),
            quality,
            s["age"].as_str().unwrap_or("")
        );
    }
}

type UnixWs = tokio_tungstenite::WebSocketStream<tokio::net::UnixStream>;

/// Opens `GET /v1/ws` on `socket` and waits for its `welcome`.
async fn open_ws(socket: &Path) -> Result<UnixWs, Failure> {
    use futures_util::StreamExt;
    use tokio_tungstenite::tungstenite::Message;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::http::HeaderValue;

    let stream = tokio::net::UnixStream::connect(socket)
        .await
        .map_err(|e| lost(format!("{}: {e}", socket.display())))?;
    let mut request = "ws://joker/v1/ws"
        .into_client_request()
        .map_err(|e| Failure::usage(e.to_string()))?;
    request.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        HeaderValue::from_static("joker.v1"),
    );
    let (mut ws, _) = tokio_tungstenite::client_async(request, stream)
        .await
        .map_err(|e| lost(format!("connecting: {e}")))?;
    loop {
        match ws.next().await {
            Some(Ok(Message::Text(text))) => {
                let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                if v["op"] == "welcome" {
                    return Ok(ws);
                }
                return Err(lost(format!(
                    "the node did not welcome the connection: {text}"
                )));
            }
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
            Some(Ok(other)) => {
                return Err(lost(format!(
                    "the node did not welcome the connection: {other:?}"
                )));
            }
            Some(Err(e)) => return Err(lost(format!("connection lost: {e}"))),
            None => return Err(lost("the node closed the connection".into())),
        }
    }
}

/// One server message of a subscription, or why the subscription ended
/// (ref/09 §6.10): a close frame, a `closing` notice or a lost socket all
/// end with exit 4 and the node's own code and detail.
async fn next_message(ws: &mut UnixWs, closing: &mut Option<Value>) -> Result<Value, Failure> {
    use futures_util::StreamExt;
    use tokio_tungstenite::tungstenite::Message;

    loop {
        match ws.next().await {
            Some(Ok(Message::Text(text))) => {
                let Ok(v) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                match v["op"].as_str() {
                    Some("closing") => *closing = Some(v),
                    Some("error") => {
                        let p = &v["problem"];
                        return Err(Failure::usage(format!(
                            "{} {}",
                            p["code"].as_str().unwrap_or("error"),
                            p["detail"].as_str().unwrap_or("")
                        )));
                    }
                    Some("gap") => eprintln!(
                        "jk: {} update(s) lost ({})",
                        v["lost"],
                        v["reason"].as_str().unwrap_or("?")
                    ),
                    _ => return Ok(v),
                }
            }
            Some(Ok(Message::Close(frame))) => {
                let (code, reason) = frame
                    .map(|f| (u16::from(f.code), f.reason.to_string()))
                    .unwrap_or((1005, String::new()));
                let detail = closing
                    .as_ref()
                    .and_then(|c| c["problem"]["detail"].as_str())
                    .map(|d| format!(": {d}"))
                    .unwrap_or_default();
                return Err(lost(format!(
                    "the node closed the subscription ({code} {reason}){detail}"
                )));
            }
            Some(Ok(_)) => {}
            Some(Err(e)) => return Err(lost(format!("connection lost: {e}"))),
            None => return Err(lost("the node closed the connection".into())),
        }
    }
}

/// What `jk top` has seen of each topic.
#[derive(Default)]
struct TopState {
    metrics: Value,
    status: Value,
    outputs: std::collections::BTreeMap<String, Value>,
}

fn render_top(header: &str, t: &TopState) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    let _ = writeln!(out, "{header}");
    let _ = writeln!(out, "at {}", t.metrics["at"].as_str().unwrap_or("?"));
    let _ = writeln!(
        out,
        "{:<16} {:>10} {:>10} {:>14} {:>14}",
        "group", "ticks", "overruns", "jitter(max)", "duration(max)"
    );
    for g in t.metrics["groups"].as_array().cloned().unwrap_or_default() {
        let _ = writeln!(
            out,
            "{:<16} {:>10} {:>10} {:>14} {:>14}",
            g["group"].as_str().unwrap_or(""),
            g["ticks"],
            g["overruns"],
            g["jitter"]["max"].as_str().unwrap_or("-"),
            g["duration"]["max"].as_str().unwrap_or("-"),
        );
    }
    let drivers = t.metrics["drivers"].as_array().cloned().unwrap_or_default();
    if !drivers.is_empty() {
        let _ = writeln!(
            out,
            "\n{:<16} {:<11} {:>10} {:>10} {:>10}",
            "device", "link", "reconnects", "dropped", "invalid"
        );
        for d in drivers {
            let _ = writeln!(
                out,
                "{:<16} {:<11} {:>10} {:>10} {:>10}",
                d["device"].as_str().unwrap_or(""),
                d["link"].as_str().unwrap_or("?"),
                d["reconnects"],
                d["dropped_samples"],
                d["invalid_samples"],
            );
        }
    }
    if !t.outputs.is_empty() {
        let _ = writeln!(
            out,
            "\n{:<32} {:>12} {:<12} {:<8}",
            "output", "applied", "level", "tripped"
        );
        for o in t.outputs.values() {
            let _ = writeln!(
                out,
                "{:<32} {:>12} {:<12} {:<8}",
                o["path"].as_str().unwrap_or(""),
                o["applied"].to_string(),
                o["active_level"].as_str().unwrap_or("none"),
                o["tripped"],
            );
        }
    }
    let manual = t.status["manual"].as_array().map_or(0, Vec::len);
    let api = &t.metrics["api"];
    let _ = writeln!(
        out,
        "\nmanual levels {manual} · api connections {} subscriptions {} dropped {}",
        api["connections"], api["subscriptions"], api["dropped"]
    );
    out
}

async fn top_view(socket: &Path, header: &str, json_mode: bool) -> Result<u8, Failure> {
    use futures_util::SinkExt;
    use tokio_tungstenite::tungstenite::Message;

    let mut ws = open_ws(socket).await?;
    let topics: &[&str] = if json_mode {
        &["metrics"]
    } else {
        &["metrics", "status", "outputs"]
    };
    for topic in topics {
        let mut sub = json!({"op": "subscribe", "id": topic, "topic": topic});
        if *topic == "outputs" {
            sub["min_interval"] = json!("1s");
        }
        ws.send(Message::Text(sub.to_string().into()))
            .await
            .map_err(|e| lost(e.to_string()))?;
    }
    let mut subs: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    let mut state = TopState::default();
    let mut closing = None;
    loop {
        let v = next_message(&mut ws, &mut closing).await?;
        let sub = v["sub"].as_str().unwrap_or("").to_owned();
        match v["op"].as_str() {
            Some("subscribed") => {
                subs.insert(sub, v["topic"].as_str().unwrap_or("").to_owned());
                continue;
            }
            Some("data" | "resync") => {}
            _ => continue,
        }
        let items = v["items"].as_array().cloned().unwrap_or_default();
        match subs.get(&sub).map(String::as_str) {
            Some("metrics") => {
                state.metrics = items.into_iter().next().unwrap_or(Value::Null);
                if json_mode {
                    print_json(&state.metrics);
                    return Ok(0);
                }
            }
            Some("status") => state.status = items.into_iter().next().unwrap_or(Value::Null),
            Some("outputs") => {
                if v["op"] == "resync" {
                    state.outputs.clear();
                }
                for o in items {
                    let path = o["path"].as_str().unwrap_or("").to_owned();
                    state.outputs.insert(path, o);
                }
            }
            _ => continue,
        }
        if state.metrics.is_null() {
            continue;
        }
        print!("\x1B[2J\x1B[H{}", render_top(header, &state));
        std::io::stdout().flush().ok();
    }
}

async fn watch_signals(socket: &Path, sel: &[String], json_mode: bool) -> Result<u8, Failure> {
    use futures_util::SinkExt;
    use tokio_tungstenite::tungstenite::Message;

    let mut ws = open_ws(socket).await?;
    ws.send(Message::Text(
        json!({
            "op": "subscribe",
            "id": "watch",
            "topic": "signals",
            "sel": sel,
            "min_interval": WATCH_INTERVAL,
        })
        .to_string()
        .into(),
    ))
    .await
    .map_err(|e| lost(e.to_string()))?;

    let mut closing = None;
    loop {
        let v = next_message(&mut ws, &mut closing).await?;
        if !matches!(v["op"].as_str(), Some("data" | "resync")) {
            continue;
        }
        if json_mode {
            print_json(&v["items"]);
        } else {
            print_samples(&v["items"]);
        }
        std::io::stdout().flush().ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_and_action_split_on_the_first_dot() {
        assert_eq!(
            split_target_action("pcs.reset_fault").unwrap(),
            ("pcs", "reset_fault")
        );
    }

    #[test]
    fn a_missing_dot_is_a_usage_error() {
        assert!(split_target_action("pcs").is_err());
    }

    #[test]
    fn an_empty_target_or_action_is_a_usage_error() {
        assert!(split_target_action(".reset").is_err());
        assert!(split_target_action("pcs.").is_err());
    }

    #[test]
    fn args_parse_bools_numbers_and_strings() {
        let args = parse_args(&[
            "enabled=true".to_owned(),
            "retries=3".to_owned(),
            "label=hello".to_owned(),
        ])
        .unwrap();
        assert_eq!(args["enabled"], json!(true));
        assert_eq!(args["retries"], json!(3.0));
        assert_eq!(args["label"], json!("hello"));
    }

    #[test]
    fn an_arg_without_equals_is_a_usage_error() {
        assert!(parse_args(&["nope".to_owned()]).is_err());
    }

    #[test]
    fn render_output_explain_includes_group_levels_and_active_state() {
        let state = json!({
            "group": "main",
            "safe": false,
            "effective": true,
            "applied": true,
            "active_level": "app:light",
            "last_outcome": {"outcome": "accepted"},
            "tripped": false,
            "levels": [
                {
                    "level": "app:light",
                    "writer": "app:light",
                    "kind": "cycle",
                    "value": true,
                    "active": true,
                }
            ],
        });
        let rendered = render_output_explain("pi.act_led", &state);
        assert!(rendered.contains("pi.act_led"));
        assert!(rendered.contains("group main"));
        assert!(rendered.contains("active      app:light"));
        assert!(rendered.contains("accepted"));
        assert!(rendered.contains("(active)"));
    }

    #[test]
    fn render_output_explain_says_when_the_supervisor_forces_safe() {
        let state = json!({
            "group": "main",
            "safe": false,
            "effective": true,
            "applied": true,
            "active_level": "manual:4",
            "tripped": false,
            "force_safe": true,
            "levels": [],
        });
        let rendered = render_output_explain("esp.led", &state);
        assert!(rendered.contains("force_safe  true"), "{rendered}");
    }

    #[test]
    fn render_output_explain_omits_last_result_when_there_is_none() {
        let state = json!({
            "group": "main",
            "safe": false,
            "effective": false,
            "applied": false,
            "active_level": "none",
            "last_outcome": null,
            "tripped": false,
            "levels": [],
        });
        let rendered = render_output_explain("pi.act_led", &state);
        assert!(!rendered.contains("last result"));
        assert!(!rendered.contains("force_safe"));
    }
}

/// The arguments of `jk history`.
#[derive(Debug, Clone, Default)]
pub struct HistoryArgs {
    /// Selectors.
    pub selectors: Vec<String>,
    /// `--from`.
    pub from: Option<String>,
    /// `--to`.
    pub to: Option<String>,
    /// `--step`.
    pub step: Option<String>,
    /// `--agg`.
    pub agg: Option<String>,
    /// `--tier`.
    pub tier: Option<String>,
}

/// Percent-encodes a query value (RFC 3986 unreserved characters pass).
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~*,:".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
