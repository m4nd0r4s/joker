//! The human views of a class: the line of `jk class list` and the page of
//! `jk class show` (docs/07 §7, track part 7 §2).

use std::fmt::Write as _;

use crate::contract::{ContractFile, Param, Ty};

fn summary(file: &ContractFile) -> String {
    file.docs
        .items
        .get("class")
        .and_then(|d| d.doc.as_deref())
        .and_then(|d| d.lines().next())
        .unwrap_or("")
        .trim_end_matches('.')
        .to_owned()
}

fn doc(file: &ContractFile, key: &str) -> String {
    file.docs
        .items
        .get(key)
        .and_then(|d| d.doc.as_deref())
        .and_then(|d| d.lines().next())
        .unwrap_or("")
        .to_owned()
}

fn ty(t: &Ty, unit: Option<&String>) -> String {
    match unit {
        Some(u) => format!("{} {u}", t.render()),
        None => t.render(),
    }
}

fn params(ps: &[Param]) -> String {
    ps.iter()
        .map(|p| format!("{}: {}", p.name, ty(&p.ty, p.unit.as_ref())))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The line of `jk class list`: reference, version and summary.
pub fn list_line(file: &ContractFile) -> String {
    format!(
        "{:<26} {:<8} {}",
        file.contract.reference(),
        file.contract.version.to_string(),
        summary(file)
    )
}

/// The page of `jk class show`.
pub fn show(file: &ContractFile) -> String {
    let c = &file.contract;
    let mut out = format!("{} ({}) — {}\n", c.reference(), c.version, summary(file));
    let section = |title: &str, rows: Vec<String>, out: &mut String| {
        for (i, row) in rows.iter().enumerate() {
            let head = if i == 0 { title } else { "" };
            let _ = writeln!(out, "  {head:<13} {row}");
        }
    };
    let cap = |c: &Option<String>| c.as_ref().map(|c| format!("[{c}]")).unwrap_or_default();
    let members = c
        .members
        .iter()
        .map(|m| {
            let mut extra = Vec::new();
            if m.optional {
                extra.push("optional".to_owned());
            }
            if let Some(h) = &m.safe_hint {
                extra.push(format!("safe_hint {h}"));
            }
            if m.unit.is_none() && m.ty.is_float() {
                extra.push("unit of the instance".to_owned());
            }
            format!(
                "{:<16} {:<6} {:<12} {:<16} {}",
                m.name,
                m.direction.keyword(),
                ty(&m.ty, m.unit.as_ref()),
                cap(&m.capability),
                extra.join(", ")
            )
            .trim_end()
            .to_owned()
        })
        .collect();
    section("members", members, &mut out);
    let nested = c
        .class_members
        .iter()
        .map(|m| {
            let bounds = if m.collection {
                format!("[{}..{}]", m.min, m.max)
            } else {
                String::new()
            };
            format!("{:<16} {}{bounds} {}", m.name, m.class, cap(&m.capability))
                .trim_end()
                .to_owned()
        })
        .collect();
    section("class members", nested, &mut out);
    let methods = c
        .methods
        .iter()
        .map(|m| {
            let atomic = if m.atomic { "atomic " } else { "" };
            format!(
                "{atomic}{}({}) {}",
                m.name,
                params(&m.params),
                cap(&m.capability)
            )
            .trim_end()
            .to_owned()
        })
        .collect();
    section("methods", methods, &mut out);
    let queries = c
        .queries
        .iter()
        .map(|q| {
            format!(
                "{:<16} {:<12} {:<28} {} {}",
                q.name,
                ty(&q.ty, q.unit.as_ref()),
                q.expr,
                cap(&q.capability),
                doc(file, &format!("query:{}", q.name))
            )
            .trim_end()
            .to_owned()
        })
        .collect();
    section("queries", queries, &mut out);
    let events = c
        .events
        .iter()
        .map(|e| {
            format!(
                "{} {{ {} }} {}",
                e.name,
                params(&e.fields),
                cap(&e.capability)
            )
        })
        .collect();
    section("events", events, &mut out);
    let actions = c
        .actions
        .iter()
        .map(|a| {
            let timeout = a
                .timeout
                .as_ref()
                .map(|t| format!("timeout {t}"))
                .unwrap_or_default();
            format!(
                "{}({}) -> {} {timeout} {}",
                a.name,
                params(&a.params),
                a.result.render(),
                cap(&a.capability)
            )
            .trim_end()
            .to_owned()
        })
        .collect();
    section("actions", actions, &mut out);
    section(
        "capabilities",
        vec![c.capabilities.join(", ")]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect(),
        &mut out,
    );
    section(
        "health",
        vec![c.health.join(", ")]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect(),
        &mut out,
    );
    let aligns: Vec<String> = file
        .docs
        .items
        .get("class")
        .map(|d| {
            d.annotations
                .iter()
                .filter(|(n, _)| n == "aligns")
                .flat_map(|(_, a)| a.iter().map(|x| x.trim_matches('"').to_owned()))
                .collect()
        })
        .unwrap_or_default();
    section("aligned with", aligns, &mut out);
    out
}
