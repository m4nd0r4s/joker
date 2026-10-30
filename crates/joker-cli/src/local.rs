//! The local commands of `jk` that need no node: `jk init`, `jk profile
//! check`, `jk explain <code>`, `jk config set|show` and the local half of
//! `jk diff` (docs/07 §2–§3, ref/02 §2.3).

use std::path::{Path, PathBuf};

use joker_manifest::codes::Registry;

use crate::Failure;

/// `jk init [--name <n>]` (docs/07 §2): turns the current, project-less
/// directory into a project with a minimal `joker.toml`, `sim/` and a
/// `.gitignore`. Nothing that exists is overwritten.
pub fn init(dir: &Path, name: Option<&str>) -> Result<u8, Failure> {
    let manifest = dir.join("joker.toml");
    if manifest.exists() {
        return Err(Failure::usage(format!(
            "{} already exists: this directory is already a project",
            manifest.display()
        )));
    }
    let name = match name {
        Some(n) => n.to_owned(),
        None => std::fs::canonicalize(dir)
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "project".to_owned()),
    };
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        return Err(Failure::usage(format!(
            "`{name}` is not a project name: use letters, digits, `-` and `_` (--name)"
        )));
    }
    let io = |e: std::io::Error| Failure::usage(format!("{}: {e}", dir.display()));
    std::fs::write(
        &manifest,
        format!("[project]\nname  = \"{name}\"\njoker = \"1.0\"\nmode  = \"dev\"\n"),
    )
    .map_err(io)?;
    println!("created joker.toml");
    std::fs::create_dir_all(dir.join("sim")).map_err(io)?;
    println!("created sim/");
    let gitignore = dir.join(".gitignore");
    if gitignore.exists() {
        println!("kept    .gitignore");
    } else {
        std::fs::write(&gitignore, "/target/\n/.joker/dev/\n/.joker/*.sock\n").map_err(io)?;
        println!("created .gitignore");
    }
    println!("next step: jk app new <name>");
    Ok(0)
}

/// `jk profile check <profile>` (docs/07 §2.3): validates a profile file
/// without equipment.
pub fn profile_check(file: &Path) -> Result<u8, Failure> {
    let text = std::fs::read_to_string(file)
        .map_err(|e| Failure::usage(format!("{}: {e}", file.display())))?;
    let report = joker_manifest::profile::parse(file, &text);
    let rendered = report.render();
    if !rendered.is_empty() {
        eprintln!("{rendered}");
    }
    let Some(profile) = report.profile else {
        return Ok(2);
    };
    let mut by_direction = std::collections::BTreeMap::new();
    for s in profile.signals.values() {
        *by_direction
            .entry(format!("{:?}", s.direction).to_lowercase())
            .or_insert(0) += 1;
    }
    let directions: Vec<String> = by_direction
        .iter()
        .map(|(d, n)| format!("{n} {d}"))
        .collect();
    println!(
        "✓ profile  {} · {} signal(s): {}{}",
        file.display(),
        profile.signals.len(),
        directions.join(", "),
        if profile.fallback { "; [fallback]" } else { "" }
    );
    Ok(0)
}

/// The registry page of `code` (`J0107`, `W0205`, `J-E1017`), as `jk
/// explain <code>` prints it, or `None` when `code` is not a code.
pub fn explain_code(code: &str) -> Option<String> {
    let registry = Registry::builtin().ok()?;
    let entry = registry.lookup(code)?;
    let mut out = format!("{} · ", entry.id());
    match entry.severity() {
        Some(s) => out.push_str(s.as_str()),
        None => out.push_str("retired"),
    }
    out.push_str(&format!(" · {}\n", entry.message()));
    if let joker_manifest::codes::Status::Retired { replaced_by } = entry.status() {
        out.push_str(&format!("retired: see {replaced_by}\n"));
    }
    if let Some(w) = entry.dev_warning() {
        out.push_str(&format!("in mode = \"dev\" it is the warning {w}\n"));
    }
    if let Some(text) = entry.explain() {
        out.push('\n');
        out.push_str(text.trim_end());
        out.push('\n');
    }
    out.push_str(&format!("\nsee {}", entry.doc()));
    for also in entry.also() {
        out.push_str(&format!(", {also}"));
    }
    out.push('\n');
    Some(out)
}

/// The user's `config.toml` (ref/02 §2.3.1): `JK_CONFIG`, else
/// `~/.config/joker/config.toml`.
pub fn user_config() -> Result<PathBuf, Failure> {
    if let Ok(path) = std::env::var("JK_CONFIG") {
        return Ok(PathBuf::from(path));
    }
    std::env::var("HOME")
        .map(|home| PathBuf::from(home).join(".config/joker/config.toml"))
        .map_err(|_| Failure::usage("no HOME and no JK_CONFIG: where is config.toml?"))
}

/// A key of `config.toml` split into its table and its own name
/// (`nodes.lab.address` → `["nodes", "lab"]`, `address`); a quoted segment
/// keeps its dots (`registries."registry.example.com".auth`).
fn split_key(key: &str) -> Result<Vec<String>, String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in key.chars() {
        match c {
            '"' => quoted = !quoted,
            '.' if !quoted => parts.push(std::mem::take(&mut current)),
            c => current.push(c),
        }
    }
    parts.push(current);
    if quoted || parts.iter().any(String::is_empty) {
        return Err(format!("`{key}` is not a key"));
    }
    Ok(parts)
}

/// What a value of the key must be (ref/02 §2.3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    Path,
    Name,
    Choice(&'static [&'static str]),
    Duration,
    Address,
    Oci,
    Https,
    Search,
}

fn kind_of(parts: &[String], project: bool) -> Result<Kind, String> {
    let p: Vec<&str> = parts.iter().map(String::as_str).collect();
    let kind = match p.as_slice() {
        ["user", "name"] => Kind::Name,
        ["user", "cert" | "key" | "signing_key" | "secrets_key"] => Kind::Path,
        ["defaults", "node"] => Kind::Text,
        ["defaults", "manifest"] if project => Kind::Path,
        ["defaults", "env"] if project => Kind::Text,
        ["defaults", "manifest" | "env"] => {
            return Err(format!(
                "`{}` lives only in the project's .joker/config.toml: add --project",
                parts.join(".")
            ));
        }
        ["defaults", "ca"] => Kind::Path,
        ["defaults", "output"] => Kind::Choice(&["human", "json"]),
        ["defaults", "color"] => Kind::Choice(&["auto", "always", "never"]),
        ["defaults", "timeout"] => Kind::Duration,
        ["defaults", "registry"] => Kind::Oci,
        ["defaults", "search"] => Kind::Search,
        ["nodes", _, "address"] => Kind::Address,
        ["nodes", _, "ca" | "cert" | "key" | "signing_key"] => Kind::Path,
        ["nodes", _, "via"] => return Err("`nodes.<name>.via` is reserved for 1.3".to_owned()),
        ["decks", _, "url"] => Kind::Https,
        ["decks", _, "ca"] => Kind::Path,
        ["decks", _, "auth"] => Kind::Choice(&["cert"]),
        ["registries", _, "auth"] => Kind::Choice(&["docker", "none"]),
        _ => {
            return Err(format!(
                "`{}` is not a key of config.toml (ref/02 §2.3.2)",
                parts.join(".")
            ));
        }
    };
    Ok(kind)
}

fn toml_string(text: &str) -> String {
    toml::Value::String(text.to_owned()).to_string()
}

/// The TOML text of `value` for a key of `kind`, or why it is refused.
fn value_of(kind: Kind, value: &str) -> Result<String, String> {
    match kind {
        Kind::Text | Kind::Path => Ok(toml_string(value)),
        Kind::Name => {
            let ok = (1..=32).contains(&value.len())
                && value
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-".contains(c));
            if ok {
                Ok(toml_string(value))
            } else {
                Err(format!(
                    "`{value}` is not a user name ([a-z0-9._-], 1 to 32)"
                ))
            }
        }
        Kind::Choice(options) => {
            if options.contains(&value) {
                Ok(toml_string(value))
            } else {
                Err(format!("`{value}`: one of {}", options.join(", ")))
            }
        }
        Kind::Duration => jokerd::wire::parse_duration(value)
            .map(|_| toml_string(value))
            .map_err(|e| format!("not a duration: {e}")),
        Kind::Address => {
            let tcp = value
                .rsplit_once(':')
                .is_some_and(|(host, port)| !host.is_empty() && port.parse::<u16>().is_ok());
            let ok = value.starts_with("unix:") || value.starts_with("ssh://") || tcp;
            if ok {
                Ok(toml_string(value))
            } else {
                Err(format!(
                    "`{value}`: an address is unix:<path>, <host>:<port> or ssh://[<user>@]<host>[:<port>]"
                ))
            }
        }
        Kind::Oci => {
            if value.starts_with("oci://") {
                Ok(toml_string(value))
            } else {
                Err(format!("`{value}`: a registry is an oci:// URL"))
            }
        }
        Kind::Https => {
            if value.starts_with("https://") {
                Ok(toml_string(value))
            } else {
                Err(format!("`{value}`: a Deck is an https:// URL"))
            }
        }
        Kind::Search => {
            let entries: Vec<&str> = value
                .split(',')
                .map(str::trim)
                .filter(|e| !e.is_empty())
                .collect();
            if let Some(bad) = entries
                .iter()
                .find(|e| **e != "builtin:" && !e.starts_with("oci://"))
            {
                return Err(format!(
                    "`{bad}`: the search path holds oci:// URLs and \"builtin:\""
                ));
            }
            let items: Vec<String> = entries.iter().map(|e| toml_string(e)).collect();
            Ok(format!("[{}]", items.join(", ")))
        }
    }
}

fn header_of(table: &[String]) -> String {
    let parts: Vec<String> = table
        .iter()
        .map(|p| {
            if p.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                p.clone()
            } else {
                toml_string(p)
            }
        })
        .collect();
    format!("[{}]", parts.join("."))
}

fn is_key_line(line: &str, key: &str) -> bool {
    let t = line.trim_start();
    let rest = t
        .strip_prefix(key)
        .or_else(|| t.strip_prefix(&toml_string(key)));
    rest.is_some_and(|r| r.trim_start().starts_with('='))
}

/// `text` with `key = value` set in `[table]`, every other line, comment
/// included, kept as it is: the key's line is replaced, or the key goes at
/// the end of its table, or a new table goes at the end of the file.
pub fn set_line(text: &str, table: &[String], key: &str, value: &str) -> String {
    let header = header_of(table);
    let lines: Vec<&str> = text.lines().collect();
    let pair = format!("{key} = {value}");
    let start = lines.iter().position(|l| {
        let t = l.trim();
        t == header
            || t.strip_prefix('[')
                .and_then(|r| r.strip_suffix(']'))
                .is_some_and(|inner| {
                    split_key(&inner.replace(' ', "")).ok().as_deref() == Some(table)
                })
    });
    let mut out: Vec<String> = lines.iter().map(|l| (*l).to_owned()).collect();
    match start {
        Some(start) => {
            let end = lines[start + 1..]
                .iter()
                .position(|l| l.trim_start().starts_with('['))
                .map_or(lines.len(), |i| start + 1 + i);
            if let Some(i) = (start + 1..end).find(|&i| is_key_line(lines[i], key)) {
                out[i] = pair;
            } else {
                let mut at = end;
                while at > start + 1 && lines[at - 1].trim().is_empty() {
                    at -= 1;
                }
                out.insert(at, pair);
            }
        }
        None => {
            if out.last().is_some_and(|l| !l.trim().is_empty()) {
                out.push(String::new());
            }
            out.push(header);
            out.push(pair);
        }
    }
    let mut text = out.join("\n");
    text.push('\n');
    text
}

/// `jk config set <key> <value> [--project]` (ref/02 §2.3.2): validates
/// the key and the value, and sets it in the user's `config.toml` or, with
/// `--project`, in `.joker/config.toml`, keeping the rest of the file.
pub fn config_set(key: &str, value: &str, project: bool) -> Result<u8, Failure> {
    let parts = split_key(key).map_err(Failure::usage)?;
    let kind = kind_of(&parts, project).map_err(Failure::usage)?;
    let value = value_of(kind, value).map_err(Failure::usage)?;
    let file = if project {
        PathBuf::from(".joker/config.toml")
    } else {
        user_config()?
    };
    let before = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "version = 1\n".to_owned(),
        Err(e) => return Err(Failure::usage(format!("{}: {e}", file.display()))),
    };
    before
        .parse::<toml::Table>()
        .map_err(|e| Failure::usage(format!("{}: {e}", file.display())))?;
    let (table, name) = parts.split_at(parts.len() - 1);
    let after = set_line(&before, table, &name[0], &value);
    after.parse::<toml::Table>().map_err(|e| {
        Failure::usage(format!(
            "{}: the result is not valid TOML: {e}",
            file.display()
        ))
    })?;
    if let Some(parent) = file.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| Failure::usage(format!("{e}")))?;
    }
    std::fs::write(&file, after)
        .map_err(|e| Failure::usage(format!("writing {}: {e}", file.display())))?;
    println!("{key} = {value}  ({})", file.display());
    Ok(0)
}

fn flatten(prefix: &str, value: &toml::Value, out: &mut Vec<(String, String)>) {
    match value {
        toml::Value::Table(t) => {
            for (k, v) in t {
                let key = if k
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                {
                    k.clone()
                } else {
                    toml_string(k)
                };
                let path = if prefix.is_empty() {
                    key
                } else {
                    format!("{prefix}.{key}")
                };
                flatten(&path, v, out);
            }
        }
        other => out.push((prefix.to_owned(), other.to_string())),
    }
}

/// `jk config show [--origin]` (docs/07 §3.1): the effective client
/// configuration, the project's `.joker/config.toml` over the user's; with
/// `--origin`, the file each value comes from. A project `[nodes.<n>]`
/// replaces the user's whole (ref/02 §2.3.2).
pub fn config_show(origin: bool) -> Result<u8, Failure> {
    let mut files = vec![(user_config()?, "user")];
    files.push((PathBuf::from(".joker/config.toml"), "project"));
    let mut values: Vec<(String, String, String)> = Vec::new();
    for (file, _) in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let table: toml::Table = text
            .parse()
            .map_err(|e| Failure::usage(format!("{}: {e}", file.display())))?;
        if let Some(nodes) = table.get("nodes").and_then(toml::Value::as_table) {
            for node in nodes.keys() {
                let prefix = format!("nodes.{node}.");
                values.retain(|(k, _, _)| !k.starts_with(&prefix));
            }
        }
        let mut flat = Vec::new();
        flatten("", &toml::Value::Table(table), &mut flat);
        for (k, v) in flat {
            values.retain(|(key, _, _)| *key != k);
            values.push((k, v, file.display().to_string()));
        }
    }
    values.sort();
    for (k, v, from) in values {
        if origin {
            println!("{k} = {v}    # {from}");
        } else {
            println!("{k} = {v}");
        }
    }
    Ok(0)
}

/// One line of a diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line<'a> {
    /// In both.
    Same(&'a str),
    /// Only in the first.
    Removed(&'a str),
    /// Only in the second.
    Added(&'a str),
}

/// The line diff of `a` and `b` (longest common subsequence).
pub fn diff<'a>(a: &'a str, b: &'a str) -> Vec<Line<'a>> {
    let a: Vec<&str> = a.lines().collect();
    let b: Vec<&str> = b.lines().collect();
    let mut lcs = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::new();
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            out.push(Line::Same(a[i]));
            i += 1;
            j += 1;
        } else if i < a.len() && (j == b.len() || lcs[i + 1][j] >= lcs[i][j + 1]) {
            out.push(Line::Removed(a[i]));
            i += 1;
        } else {
            out.push(Line::Added(b[j]));
            j += 1;
        }
    }
    out
}

/// A diff as `jk diff` prints it: the changed lines with `-`/`+`, each
/// under the last table header above it.
pub fn render_diff(a_name: &str, a: &str, b_name: &str, b: &str) -> (String, usize) {
    let lines = diff(a, b);
    let mut out = format!("--- {a_name}\n+++ {b_name}\n");
    let mut header: Option<&str> = None;
    let mut printed: Option<&str> = None;
    let mut changes = 0;
    for line in &lines {
        let (mark, text) = match line {
            Line::Same(t) => {
                if t.trim_start().starts_with('[') {
                    header = Some(t);
                }
                continue;
            }
            Line::Removed(t) => ('-', *t),
            Line::Added(t) => ('+', *t),
        };
        changes += 1;
        if let Some(h) = header
            && printed != Some(h)
            && !text.trim_start().starts_with('[')
        {
            out.push_str(&format!("  {h}\n"));
            printed = Some(h);
        }
        out.push_str(&format!("{mark} {text}\n"));
    }
    (out, changes)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn parts(key: &str) -> Vec<String> {
        split_key(key).unwrap()
    }

    #[test]
    fn keys_split_on_unquoted_dots() {
        assert_eq!(parts("nodes.lab.address"), ["nodes", "lab", "address"]);
        assert_eq!(
            parts("registries.\"registry.example.com\".auth"),
            ["registries", "registry.example.com", "auth"]
        );
        assert!(split_key("nodes..address").is_err());
    }

    #[test]
    fn only_the_keys_of_ref_02_are_accepted() {
        assert_eq!(
            kind_of(&parts("nodes.lab.address"), false).unwrap(),
            Kind::Address
        );
        assert!(kind_of(&parts("defaults.manifest"), false).is_err());
        assert!(kind_of(&parts("defaults.manifest"), true).is_ok());
        assert!(kind_of(&parts("nodes.lab.via"), false).is_err());
        assert!(kind_of(&parts("defaults.colour"), false).is_err());
        assert!(value_of(Kind::Address, "lab.local:7443").is_ok());
        assert!(value_of(Kind::Address, "lab.local").is_err());
        assert!(value_of(Kind::Duration, "30s").is_ok());
        assert!(value_of(Kind::Duration, "soon").is_err());
        assert_eq!(
            value_of(Kind::Search, "builtin:, oci://r.example.com/joker").unwrap(),
            "[\"builtin:\", \"oci://r.example.com/joker\"]"
        );
    }

    #[test]
    fn a_set_keeps_the_rest_of_the_file() {
        let text = "version = 1\n\n[user]\nname = \"dl\"\n\n[nodes.lab]\naddress = \"ssh://pi@lab.local\"\nca = \"ca.crt\"\n";
        let set = set_line(
            text,
            &["nodes".into(), "lab".into()],
            "address",
            "\"lab.local:7443\"",
        );
        assert_eq!(
            set,
            "version = 1\n\n[user]\nname = \"dl\"\n\n[nodes.lab]\naddress = \"lab.local:7443\"\nca = \"ca.crt\"\n"
        );
        let added = set_line(text, &["user".into()], "cert", "\"u.crt\"");
        assert!(added.contains("name = \"dl\"\ncert = \"u.crt\"\n\n[nodes.lab]"));
        let new = set_line(text, &["nodes".into(), "zero".into()], "address", "\"z:1\"");
        assert!(new.ends_with("ca = \"ca.crt\"\n\n[nodes.zero]\naddress = \"z:1\"\n"));
    }

    #[test]
    fn the_diff_shows_the_changed_lines_under_their_table() {
        let a =
            "[devices.esp]\ndriver = \"modbus\"\npoll = \"100ms\"\n\n[outputs.x]\nsafe = false\n";
        let b =
            "[devices.esp]\ndriver = \"modbus\"\npoll = \"200ms\"\n\n[outputs.x]\nsafe = false\n";
        let (text, changes) = render_diff("a", a, "b", b);
        assert_eq!(changes, 2);
        assert_eq!(
            text,
            "--- a\n+++ b\n  [devices.esp]\n- poll = \"100ms\"\n+ poll = \"200ms\"\n"
        );
        assert_eq!(render_diff("a", a, "b", a).1, 0);
    }

    #[test]
    fn a_code_has_a_page() {
        let page = explain_code("J0828").unwrap();
        assert!(page.starts_with("J0828 · error · "), "{page}");
        assert!(page.contains("see ref/03"), "{page}");
        assert!(explain_code("pi.act_led").is_none());
        assert!(explain_code("W0205").unwrap().starts_with("J0205"));
    }
}
