//! The commands that talk to a node (docs/07 §3–§4, ref/09 §9.1).

use std::io::{BufRead, Write};
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
        let mut proposal = json!({
            "manifest": {"content": manifest},
            "lock": {"content": project::lock_text(entry)},
            "artifacts": [],
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
        for s in v["items"].as_array().cloned().unwrap_or_default() {
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
        Ok(0)
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
                e["outcome"]["outcome"].as_str().unwrap_or(""),
                e["applied"].to_string(),
                e["active_level"].as_str().unwrap_or(""),
                e["writer"]["writer"].as_str().unwrap_or("")
            );
        }
        Ok(0)
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
    }
}
