//! Rust generation (docs/17 §6, ref/04 §4.8.1): from a contract, a
//! `no_std` module with the class's identity (reference, version,
//! contract hash, capabilities), its member names, its methods by method
//! number, and its enums and records, under the names of ref/04 §4.8.1.
//!
//! The handles of ref/04 §4.8.2 (`<C>Cycle`, `<C>Task`) build on these
//! items and belong to the SDK; this module generates only what follows
//! from the contract alone.

use std::fmt::Write as _;

use crate::contract::{ContractFile, Param, Ty};

/// `pump` → `Pump`, `temp_sensor` → `TempSensor`, `meter.ac3` → `MeterAc3`,
/// `PcsMode` → `PcsMode` (ref/04 §4.8.1).
pub fn camel(name: &str) -> String {
    let mut out = String::new();
    for part in name.split(['_', '.', '/']) {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    out
}

/// `run_at` → `RUN_AT`, for member-name constants.
fn shout(name: &str) -> String {
    name.to_uppercase()
}

fn short(full: &str) -> &str {
    full.rsplit('/').next().unwrap_or(full)
}

fn rust_ty(ty: &Ty, borrowed: &mut bool) -> String {
    match ty {
        Ty::Bool => "bool".into(),
        Ty::I64 => "i64".into(),
        Ty::U64 | Ty::Bits(_) => "u64".into(),
        Ty::F64 => "f64".into(),
        Ty::F32 => "f32".into(),
        Ty::String => {
            *borrowed = true;
            "&'a str".into()
        }
        Ty::Bytes => {
            *borrowed = true;
            "&'a [u8]".into()
        }
        Ty::Enum(name) => camel(short(name)),
        Ty::Record(name) => {
            *borrowed = true;
            format!("{}<'a>", camel(short(name)))
        }
        Ty::List(inner, _) => {
            *borrowed = true;
            let mut b = false;
            format!("&'a [{}]", rust_ty(inner, &mut b))
        }
        Ty::Unit => "()".into(),
    }
}

fn doc_line(out: &mut String, indent: &str, text: &str) {
    for line in text.lines() {
        let _ = writeln!(out, "{indent}/// {line}");
    }
}

fn unit_note(p: &Param) -> String {
    match &p.unit {
        Some(u) => format!(" in `{u}`"),
        None => String::new(),
    }
}

/// The Rust items of one class with the enums and records it uses.
pub fn rust(file: &ContractFile) -> String {
    let mut out = rust_class(file);
    out.push_str(&rust_types(
        file.contract.enums.iter(),
        file.contract.records.iter(),
    ));
    out
}

/// The Rust items of one class: its marker type with identity and
/// member names, and its methods.
pub fn rust_class(file: &ContractFile) -> String {
    let c = &file.contract;
    let name = camel(c.name());
    let mut out = String::new();
    let summary = file
        .docs
        .items
        .get("class")
        .and_then(|d| d.doc.clone())
        .unwrap_or_else(|| format!("The class `{}`.", c.reference()));
    doc_line(&mut out, "", &summary);
    let _ = writeln!(
        out,
        "#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]\npub struct {name};\n"
    );
    let hash = file
        .contract_hash()
        .iter()
        .map(|b| format!("0x{b:02x}"))
        .collect::<Vec<_>>()
        .join(", ");
    let caps = c
        .capabilities
        .iter()
        .map(|k| format!("{k:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    let _ = writeln!(out, "impl {name} {{");
    let _ = writeln!(
        out,
        "    /// The reference apps and manifests use.\n    pub const REFERENCE: &'static str = {:?};",
        c.reference()
    );
    let _ = writeln!(
        out,
        "    /// The full version of the contract.\n    pub const VERSION: &'static str = {:?};",
        c.version.to_string()
    );
    let _ = writeln!(
        out,
        "    /// The contract hash the lock pins (docs/17 §6.1).\n    pub const CONTRACT_HASH: [u8; 32] = [{hash}];"
    );
    let _ = writeln!(
        out,
        "    /// The optional capabilities.\n    pub const CAPABILITIES: &'static [&'static str] = &[{caps}];"
    );
    for m in &c.members {
        let cap = m
            .capability
            .as_ref()
            .map(|k| format!(", capability `{k}`"))
            .unwrap_or_default();
        let unit = m
            .unit
            .as_ref()
            .map(|u| format!(" in `{u}`"))
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "    /// `{} {}: {}`{unit}{cap}.\n    pub const {}: &'static str = {:?};",
            m.direction.keyword(),
            m.name,
            m.ty.render(),
            shout(&m.name),
            m.name
        );
    }
    for q in &c.queries {
        if c.members.iter().any(|m| m.name == q.name) {
            continue;
        }
        let _ = writeln!(
            out,
            "    /// `query {}: {}` = `{}`.\n    pub const {}: &'static str = {:?};",
            q.name,
            q.ty.render(),
            q.expr,
            shout(&q.name),
            q.name
        );
    }
    let _ = writeln!(out, "}}\n");
    if !c.methods.is_empty() {
        let _ = writeln!(
            out,
            "/// The methods of `{}`, by method number (the position in the\n/// contract, ADR 0084).",
            c.reference()
        );
        let _ = writeln!(
            out,
            "#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]\n#[repr(u16)]\npub enum {name}Method {{"
        );
        for (i, m) in c.methods.iter().enumerate() {
            let args = m
                .params
                .iter()
                .map(|p| match &p.unit {
                    Some(u) => format!("{}: {} {u}", p.name, p.ty.render()),
                    None => format!("{}: {}", p.name, p.ty.render()),
                })
                .collect::<Vec<_>>()
                .join(", ");
            let _ = writeln!(
                out,
                "    /// `{}({args})`.\n    {} = {i},",
                m.name,
                camel(&m.name)
            );
        }
        let _ = writeln!(out, "}}\n");
        let _ = writeln!(out, "impl {name}Method {{");
        let _ = writeln!(
            out,
            "    /// Every method, by number.\n    pub const ALL: [Self; {}] = [{}];",
            c.methods.len(),
            c.methods
                .iter()
                .map(|m| format!("Self::{}", camel(&m.name)))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let _ = writeln!(
            out,
            "    /// The method number of a method record (ref/04 §3.3).\n    pub const fn number(self) -> u16 {{\n        self as u16\n    }}"
        );
        let _ = writeln!(
            out,
            "    /// The name in the contract.\n    pub const fn name(self) -> &'static str {{\n        match self {{"
        );
        for m in &c.methods {
            let _ = writeln!(out, "            Self::{} => {:?},", camel(&m.name), m.name);
        }
        let _ = writeln!(out, "        }}\n    }}");
        let _ = writeln!(
            out,
            "    /// How many arguments the method takes.\n    pub const fn arity(self) -> u8 {{\n        match self {{"
        );
        for m in &c.methods {
            let _ = writeln!(
                out,
                "            Self::{} => {},",
                camel(&m.name),
                m.params.len()
            );
        }
        let _ = writeln!(out, "        }}\n    }}\n}}\n");
    }
    out
}

/// The Rust items of enums and records.
pub fn rust_types<'a>(
    enums: impl IntoIterator<Item = &'a crate::contract::EnumDef>,
    records: impl IntoIterator<Item = &'a crate::contract::RecordDef>,
) -> String {
    let mut out = String::new();
    for e in enums {
        let ename = camel(short(&e.name));
        let _ = writeln!(
            out,
            "/// The enum `{}`.\n#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]\n#[repr(i64)]\npub enum {ename} {{",
            e.name
        );
        for (v, value) in &e.variants {
            let _ = writeln!(out, "    /// `{v}`.\n    {} = {value},", camel(v));
        }
        let _ = writeln!(out, "}}\n");
        let _ = writeln!(
            out,
            "impl {ename} {{\n    /// The value on the wire.\n    pub const fn value(self) -> i64 {{\n        self as i64\n    }}\n    /// The variant of a value.\n    pub const fn from_value(value: i64) -> Option<Self> {{\n        match value {{"
        );
        for (v, value) in &e.variants {
            let _ = writeln!(out, "            {value} => Some(Self::{}),", camel(v));
        }
        let _ = writeln!(out, "            _ => None,\n        }}\n    }}\n}}\n");
    }
    for r in records {
        let rname = camel(short(&r.name));
        let mut borrowed = false;
        let fields: Vec<String> = r
            .fields
            .iter()
            .map(|f| {
                format!(
                    "    /// Field {}{}.\n    pub {}: {},",
                    f.name,
                    unit_note(f),
                    f.name,
                    rust_ty(&f.ty, &mut borrowed)
                )
            })
            .collect();
        let lifetime = if borrowed { "<'a>" } else { "" };
        let _ = writeln!(
            out,
            "/// The record `{}`; its CBOR keys are the field positions (docs/17 §9).\n#[derive(Debug, Clone, Copy, PartialEq)]\npub struct {rname}{lifetime} {{\n{}\n}}\n",
            r.name,
            fields.join("\n")
        );
    }
    out
}

/// A crate's worth of classes: one `pub mod v<major>` per major with
/// every class of `files`, and the names without a module pointing at
/// each class's own major (ref/04 §4.8.1).
pub fn rust_package<'a>(files: impl IntoIterator<Item = &'a ContractFile>) -> String {
    let mut by_major: std::collections::BTreeMap<u32, Vec<&ContractFile>> =
        std::collections::BTreeMap::new();
    for f in files {
        by_major
            .entry(f.contract.version.major)
            .or_default()
            .push(f);
    }
    let mut out = String::new();
    let mut exported = std::collections::BTreeSet::new();
    for (major, files) in &by_major {
        let _ = writeln!(
            out,
            "/// The classes of major {major}.\npub mod v{major} {{"
        );
        let mut body = String::new();
        let mut enums = std::collections::BTreeMap::new();
        let mut records = std::collections::BTreeMap::new();
        for f in files {
            body.push_str(&rust_class(f));
            for e in &f.contract.enums {
                enums.insert(e.name.clone(), e);
            }
            for r in &f.contract.records {
                records.insert(r.name.clone(), r);
            }
        }
        body.push_str(&rust_types(enums.into_values(), records.into_values()));
        for line in body.lines() {
            if line.is_empty() {
                out.push('\n');
            } else {
                let _ = writeln!(out, "    {line}");
            }
        }
        let _ = writeln!(out, "}}\n");
        for f in files {
            let name = camel(f.contract.name());
            if !exported.insert(name.clone()) {
                continue;
            }
            let method = if f.contract.methods.is_empty() {
                String::new()
            } else {
                format!(", {name}Method")
            };
            let _ = writeln!(out, "pub use v{major}::{{{name}{method}}};");
        }
    }
    out
}
