//! `jk profile import csv` (ref/03 §2.2.19): turns a Modbus register-map
//! CSV into a profile **draft**.
//!
//! Scoped deliberately: only the `modbus` family (the only one ref/03
//! §2.2.19 specifies for 1.0). Label/description length limits (64 B /
//! 1 KiB), `readback`/`monotonic` suggestions beyond the simple rules
//! below, and the warning for base-1-looking addresses without
//! `--modicon` are not implemented here.

use std::collections::BTreeMap;

use joker_units::Unit;

use crate::diag::{Diagnostic, Diagnostics};
use crate::source::{FileId, Span};

/// The result of an import: the draft, or diagnostics (never both).
#[derive(Debug, Default)]
pub struct ImportResult {
    /// The generated profile draft, only when there were no errors.
    pub toml: Option<String>,
    /// Every finding, in file order.
    pub diagnostics: Diagnostics,
}

const REQUIRED: [&str; 3] = ["signal", "fc", "at"];
const KNOWN: [&str; 10] = [
    "signal",
    "fc",
    "at",
    "type",
    "direction",
    "unit",
    "scale",
    "offset",
    "label",
    "description",
];

struct Row {
    span: Span,
    fields: Vec<(String, Span)>,
}

fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

/// Splits `text` into non-comment, non-blank physical lines with their byte
/// offset, honoring RFC 4180 quoting (no embedded newlines in quoted
/// fields, out of scope here).
fn lines(file: FileId, text: &str, base: usize) -> Vec<(Span, &str)> {
    let mut out = Vec::new();
    let mut offset = base;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        let content = trimmed.trim_start();
        if !content.is_empty() && !content.starts_with('#') {
            let start = offset + (trimmed.len() - content.len());
            out.push((Span::new(file, start, start + content.len()), content));
        }
        offset += line.len();
    }
    out
}

fn separator(header: &str) -> u8 {
    let mut in_quotes = false;
    for b in header.bytes() {
        match b {
            b'"' => in_quotes = !in_quotes,
            b';' if !in_quotes => return b';',
            _ => {}
        }
    }
    b','
}

fn split_fields(file: FileId, span: Span, line: &str, sep: u8) -> Vec<(String, Span)> {
    let mut fields = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut start = 0;
    let mut in_quotes = false;
    let mut buf = String::new();
    while i < bytes.len() {
        let b = bytes[i];
        if in_quotes {
            if b == b'"' {
                if bytes.get(i + 1) == Some(&b'"') {
                    buf.push('"');
                    i += 2;
                    continue;
                }
                in_quotes = false;
                i += 1;
                continue;
            }
            buf.push(b as char);
            i += 1;
            continue;
        }
        if b == b'"' && buf.is_empty() {
            in_quotes = true;
            i += 1;
            continue;
        }
        if b == sep {
            fields.push((
                buf.trim().to_owned(),
                Span::new(file, span.start + start, span.start + i),
            ));
            buf.clear();
            start = i + 1;
            i += 1;
            continue;
        }
        buf.push(b as char);
        i += 1;
    }
    fields.push((
        buf.trim().to_owned(),
        Span::new(file, span.start + start, span.start + bytes.len()),
    ));
    fields
}

fn parse_rows(
    file: FileId,
    text: &str,
    diagnostics: &mut Diagnostics,
) -> Option<(Vec<String>, Vec<Row>)> {
    let stripped = strip_bom(text);
    let base = text.len() - stripped.len();
    let physical = lines(file, stripped, base);
    let Some((header_span, header_line)) = physical.first().copied() else {
        diagnostics.push(Diagnostic::new("J0803", "empty file: not a register map"));
        return None;
    };
    let sep = separator(header_line);
    let header_fields = split_fields(file, header_span, header_line, sep);
    let header: Vec<String> = header_fields
        .iter()
        .map(|(name, _)| name.to_lowercase())
        .collect();
    if REQUIRED.iter().any(|col| !header.iter().any(|h| h == col)) {
        diagnostics.push(Diagnostic::new(
            "J0803",
            "missing `signal`, `fc` or `at`: the file is not a register map",
        ));
        return None;
    }
    let mut ok = true;
    let mut seen = BTreeMap::new();
    for (name, (_, fspan)) in header.iter().zip(&header_fields) {
        if !KNOWN.contains(&name.as_str()) {
            diagnostics
                .push(Diagnostic::new("J0140", format!("unknown column `{name}`")).at(*fspan));
            ok = false;
        } else if seen.insert(name.clone(), *fspan).is_some() {
            diagnostics
                .push(Diagnostic::new("J0140", format!("repeated column `{name}`")).at(*fspan));
            ok = false;
        }
    }
    if !ok {
        return None;
    }
    let rows = physical[1..]
        .iter()
        .map(|&(span, line)| Row {
            span,
            fields: split_fields(file, span, line, sep),
        })
        .collect();
    Some((header, rows))
}

fn cell<'a>(header: &[String], row: &'a Row, col: &str) -> Option<(&'a str, Span)> {
    let i = header.iter().position(|h| h == col)?;
    row.fields
        .get(i)
        .map(|(v, span)| (v.as_str(), *span))
        .filter(|(v, _)| !v.is_empty())
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Space {
    Coil,
    Discrete,
    Input,
    Holding,
}

fn space(fc: u16) -> Option<Space> {
    match fc {
        1 | 5 | 15 => Some(Space::Coil),
        2 => Some(Space::Discrete),
        3 | 6 | 16 => Some(Space::Holding),
        4 => Some(Space::Input),
        _ => None,
    }
}

fn is_write_fc(fc: u16) -> bool {
    matches!(fc, 5 | 6 | 15 | 16)
}

fn is_bool_fc(fc: u16) -> bool {
    matches!(fc, 1 | 2 | 5 | 15)
}

fn parse_number(text: &str, span: Span, diagnostics: &mut Diagnostics) -> Option<f64> {
    match text.parse::<f64>() {
        Ok(n) => Some(n),
        Err(_) if text.contains(',') => {
            let fixed = text.replacen(',', ".", 1);
            diagnostics.push(
                Diagnostic::new("J0806", format!("invalid literal `{text}`"))
                    .at(span)
                    .help(format!("use {fixed}")),
            );
            None
        }
        Err(_) => {
            diagnostics
                .push(Diagnostic::new("J0806", format!("invalid literal `{text}`")).at(span));
            None
        }
    }
}

fn parse_at(text: &str, span: Span, diagnostics: &mut Diagnostics) -> Option<u32> {
    let n = if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).ok()
    } else {
        text.parse::<u32>().ok()
    };
    match n {
        Some(n) if n <= 65535 => Some(n),
        _ => {
            diagnostics
                .push(Diagnostic::new("J0810", format!("invalid address `{text}`")).at(span));
            None
        }
    }
}

/// Base-1 ("Modicon") notation: `0xxxx` coils, `1xxxx` discrete inputs,
/// `3xxxx` input registers, `4xxxx` holding registers.
fn modicon_at(text: &str, fc: u16, span: Span, diagnostics: &mut Diagnostics) -> Option<u32> {
    let n: u32 = text.parse().ok().filter(|n| *n >= 1).or_else(|| {
        diagnostics
            .push(Diagnostic::new("J0810", format!("invalid Modicon address `{text}`")).at(span));
        None
    })?;
    let (prefix, want) = (n / 10000, space(fc)?);
    let got = match prefix {
        0 => Space::Coil,
        1 => Space::Discrete,
        3 => Space::Input,
        4 => Space::Holding,
        _ => {
            diagnostics.push(
                Diagnostic::new("J0810", format!("invalid Modicon address `{text}`")).at(span),
            );
            return None;
        }
    };
    if got != want {
        diagnostics.push(
            Diagnostic::new(
                "J0810",
                format!("address `{text}` is not in the register space of fc {fc}"),
            )
            .at(span),
        );
        return None;
    }
    Some(n - prefix * 10000 - 1)
}

struct Signal {
    name: String,
    at: u32,
    fc: u16,
    type_name: String,
    direction: Option<&'static str>,
    unit: Option<String>,
    scale: Option<f64>,
    offset: Option<f64>,
    label: Option<String>,
    description: Option<String>,
}

fn parse_signal(
    header: &[String],
    row: &Row,
    modicon: bool,
    diagnostics: &mut Diagnostics,
) -> Option<Signal> {
    let (name, _) = cell(header, row, "signal")?;
    if name.is_empty() {
        diagnostics.push(Diagnostic::new("J0805", "empty `signal`").at(row.span));
        return None;
    }
    let (fc_text, fc_span) = cell(header, row, "fc")?;
    let fc: u16 = fc_text
        .parse()
        .ok()
        .filter(|fc| space(*fc).is_some())
        .or_else(|| {
            diagnostics.push(
                Diagnostic::new("J0810", format!("invalid function code `{fc_text}`")).at(fc_span),
            );
            None
        })?;
    let (at_text, at_span) = cell(header, row, "at")?;
    let at = if modicon {
        modicon_at(at_text, fc, at_span, diagnostics)?
    } else {
        parse_at(at_text, at_span, diagnostics)?
    };
    let default_type = if is_bool_fc(fc) { "bool" } else { "u16" };
    let type_name = cell(header, row, "type")
        .map(|(v, _)| v.to_owned())
        .unwrap_or_else(|| default_type.to_owned());
    if let Some((_, span)) = cell(header, row, "type") {
        let bool_type = type_name == "bool";
        if bool_type != is_bool_fc(fc) {
            diagnostics.push(
                Diagnostic::new(
                    "J0809",
                    format!("type `{type_name}` incompatible with fc {fc}"),
                )
                .at(span),
            );
        }
    }
    let direction = match cell(header, row, "direction") {
        Some(("output", span)) if !is_write_fc(fc) => {
            diagnostics.push(
                Diagnostic::new("J0810", "`direction = \"output\"` needs a write fc").at(span),
            );
            Some("output")
        }
        Some(("input", span)) | Some(("param", span)) if is_write_fc(fc) && fc != 6 && fc != 16 => {
            diagnostics
                .push(Diagnostic::new("J0810", "`direction` needs a read fc or fc 6/16").at(span));
            None
        }
        Some(("output", _)) => Some("output"),
        Some(("param", _)) => Some("param"),
        Some((other, span)) => {
            diagnostics
                .push(Diagnostic::new("J0810", format!("invalid `direction` `{other}`")).at(span));
            None
        }
        None if is_write_fc(fc) => Some("output"),
        None => None,
    };
    let unit = match cell(header, row, "unit") {
        Some((u, span)) if is_bool_fc(fc) => {
            diagnostics.push(Diagnostic::new("J0815", "`unit` forbidden on `bool`").at(span));
            let _ = u;
            None
        }
        Some((u, span)) => match Unit::parse(u) {
            Ok(_) => Some(u.to_owned()),
            Err(_) => {
                diagnostics.push(Diagnostic::new("J0815", format!("invalid unit `{u}`")).at(span));
                None
            }
        },
        None => None,
    };
    let scale = match cell(header, row, "scale") {
        Some((s, span)) if is_bool_fc(fc) => {
            diagnostics.push(Diagnostic::new("J0815", "`scale` forbidden on `bool`").at(span));
            let _ = s;
            None
        }
        Some((s, span)) => {
            let n = parse_number(s, span, diagnostics);
            if n == Some(0.0) {
                diagnostics.push(Diagnostic::new("J0806", "`scale` must not be 0").at(span));
                None
            } else {
                n
            }
        }
        None => None,
    };
    let offset =
        cell(header, row, "offset").and_then(|(s, span)| parse_number(s, span, diagnostics));
    Some(Signal {
        name: name.to_owned(),
        at,
        fc,
        type_name,
        direction,
        unit,
        scale,
        offset,
        label: cell(header, row, "label").map(|(v, _)| v.to_owned()),
        description: cell(header, row, "description").map(|(v, _)| v.to_owned()),
    })
}

fn width(type_name: &str) -> u32 {
    match type_name {
        "bool" | "u16" | "i16" => 1,
        "u32" | "i32" | "f32" => 2,
        "u64" | "i64" | "f64" => 4,
        _ => 1,
    }
}

fn render(name: &str, driver: &str, signals: &[Signal]) -> String {
    let mut out = String::new();
    out.push_str("schema = 1\n\n[profile]\n");
    out.push_str(&format!("name    = \"{name}\"\n"));
    out.push_str("version = \"1.0.0\"\n");
    out.push_str(&format!("driver  = \"{driver}\"\n"));
    out.push_str(
        "# REVIEW: vendor, model, docs, firmware (the register table does not state them)\n",
    );
    out.push_str(
        "# REVIEW: quirks: jk profile test measures the latency and suggests quirks.latency\n",
    );
    out.push_str("\n[defaults]\n");
    out.push_str("poll = \"1s\"   # REVIEW: read rate\n");
    out.push_str("\n[signals]\n");
    for s in signals {
        let mut line = format!(
            "\"{}\" = {{ at = {}, fc = {}, type = \"{}\"",
            s.name, s.at, s.fc, s.type_name
        );
        if let Some(d) = s.direction {
            line.push_str(&format!(", direction = \"{d}\""));
        }
        if let Some(u) = &s.unit {
            line.push_str(&format!(", unit = \"{u}\""));
        }
        if let Some(sc) = s.scale {
            line.push_str(&format!(", scale = {sc}"));
        }
        if let Some(o) = s.offset {
            line.push_str(&format!(", offset = {o}"));
        }
        if let Some(l) = &s.label {
            line.push_str(&format!(", label = \"{l}\""));
        }
        if let Some(d) = &s.description {
            line.push_str(&format!(", description = \"{d}\""));
        }
        line.push_str(" }");
        let readback = signals.iter().find(|r| {
            r.name != s.name
                && r.at == s.at
                && ((s.fc == 5 && r.fc == 1) || ((s.fc == 6 || s.fc == 16) && r.fc == 3))
        });
        if let Some(r) = readback {
            line.push_str(&format!(
                "   # REVIEW: readback = \"{}\"? safe_hint?",
                r.name
            ));
        }
        out.push_str(&line);
        out.push('\n');
    }
    out.push_str(
        "\n# REVIEW: [fallback]: does the equipment have a communication fallback? (ref/03 §2.2.11)\n",
    );
    out
}

/// Imports a Modbus register-map CSV (ref/03 §2.2.19) into a profile draft.
pub fn import_csv(
    file: FileId,
    text: &str,
    name: &str,
    driver: &str,
    modicon: bool,
) -> ImportResult {
    let mut diagnostics = Diagnostics::default();
    if driver != "modbus" {
        diagnostics.push(Diagnostic::new(
            "J0808",
            format!("driver `{driver}` not supported by the importer in 1.0"),
        ));
        return ImportResult {
            toml: None,
            diagnostics,
        };
    }
    let Some((header, rows)) = parse_rows(file, text, &mut diagnostics) else {
        return ImportResult {
            toml: None,
            diagnostics,
        };
    };
    let mut signals = Vec::new();
    let mut names: BTreeMap<String, Span> = BTreeMap::new();
    let mut addrs: Vec<(u16, u32, u32)> = Vec::new();
    for row in &rows {
        let Some(signal) = parse_signal(&header, row, modicon, &mut diagnostics) else {
            continue;
        };
        if let Some(prev) = names.insert(signal.name.clone(), row.span) {
            diagnostics.push(
                Diagnostic::new("J0811", format!("signal `{}` repeated", signal.name))
                    .at(row.span)
                    .label("also declared here")
                    .help(format!("first seen at byte {}", prev.start)),
            );
            continue;
        }
        let w = width(&signal.type_name);
        let overlap = addrs
            .iter()
            .any(|&(fc, at, ew)| fc == signal.fc && at < signal.at + w && signal.at < at + ew);
        if overlap {
            diagnostics.push(
                Diagnostic::new(
                    "J0811",
                    format!(
                        "address {} (fc {}) overlaps another signal",
                        signal.at, signal.fc
                    ),
                )
                .at(row.span),
            );
            continue;
        }
        addrs.push((signal.fc, signal.at, w));
        signals.push(signal);
    }
    if !diagnostics.is_empty() {
        return ImportResult {
            toml: None,
            diagnostics,
        };
    }
    ImportResult {
        toml: Some(render(name, driver, &signals)),
        diagnostics,
    }
}
