//! The canonical contract of a class (docs/17 §6, ADR 0059): what `jkc`
//! writes to `<name>@<version>.contract.json`, what the lock pins through
//! the contract hash, and what the node builds its instance registry from.
//!
//! The JSON form is canonical: object keys sorted by their bytes, no
//! whitespace, strings escaped only where JSON requires it, integers only,
//! every key always present. The contract hash is the SHA-256 of the
//! canonical `contract` object, the documentation hash that of the
//! canonical `docs` object, so the same class gives the same bytes and the
//! same hashes on every run and platform.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::ast::{Direction, Version};

/// The version of the `.jkc` language and of the contract layout.
pub const JKC: u64 = 1;

/// A resolved type (docs/17 §3.2).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Ty {
    /// `bool`.
    Bool,
    /// `i64`.
    I64,
    /// `u64`.
    U64,
    /// `f64`.
    F64,
    /// `f32`.
    F32,
    /// `string`.
    String,
    /// `bytes`.
    Bytes,
    /// `bits<N>`.
    Bits(u32),
    /// An enum, by full name (`std/PcsMode`).
    Enum(String),
    /// A record, by full name.
    Record(String),
    /// `list<T, N>`.
    List(Box<Ty>, u32),
    /// `()`.
    Unit,
}

impl Ty {
    /// The canonical spelling: `f64`, `bits<16>`, `enum std/PcsMode`,
    /// `record std/Session`, `list<u64, 4>`, `()`.
    pub fn render(&self) -> String {
        match self {
            Self::Bool => "bool".into(),
            Self::I64 => "i64".into(),
            Self::U64 => "u64".into(),
            Self::F64 => "f64".into(),
            Self::F32 => "f32".into(),
            Self::String => "string".into(),
            Self::Bytes => "bytes".into(),
            Self::Bits(n) => format!("bits<{n}>"),
            Self::Enum(name) => format!("enum {name}"),
            Self::Record(name) => format!("record {name}"),
            Self::List(inner, n) => format!("list<{}, {n}>", inner.render()),
            Self::Unit => "()".into(),
        }
    }

    /// Reads the canonical spelling back.
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "bool" => Self::Bool,
            "i64" => Self::I64,
            "u64" => Self::U64,
            "f64" => Self::F64,
            "f32" => Self::F32,
            "string" => Self::String,
            "bytes" => Self::Bytes,
            "()" => Self::Unit,
            _ => {
                if let Some(name) = text.strip_prefix("enum ") {
                    Self::Enum(name.to_owned())
                } else if let Some(name) = text.strip_prefix("record ") {
                    Self::Record(name.to_owned())
                } else if let Some(n) = text.strip_prefix("bits<").and_then(|t| t.strip_suffix('>'))
                {
                    Self::Bits(n.parse().ok()?)
                } else {
                    let body = text.strip_prefix("list<")?.strip_suffix('>')?;
                    let (inner, n) = body.rsplit_once(", ")?;
                    Self::List(Box::new(Self::parse(inner)?), n.parse().ok()?)
                }
            }
        })
    }

    /// Whether values are floating point.
    pub const fn is_float(&self) -> bool {
        matches!(self, Self::F64 | Self::F32)
    }

    /// Whether values are numbers.
    pub const fn is_numeric(&self) -> bool {
        matches!(self, Self::F64 | Self::F32 | Self::I64 | Self::U64)
    }

    /// Whether a member (`in`, `out`, `param`) may have the type.
    pub const fn is_scalar(&self) -> bool {
        !matches!(self, Self::Record(_) | Self::List(..) | Self::Unit)
    }
}

/// An `in`, `out` or `param` member.
#[derive(Debug, Clone, PartialEq)]
pub struct Scalar {
    /// The name.
    pub name: String,
    /// The direction.
    pub direction: Direction,
    /// The type.
    pub ty: Ty,
    /// The unit symbol; `None` for a type without one, or for an `in` or
    /// `param` whose unit the instance gives (ADR 0059).
    pub unit: Option<String>,
    /// The capability that brings it.
    pub capability: Option<String>,
    /// `@optional`.
    pub optional: bool,
    /// `= literal`, canonical.
    pub default: Option<String>,
    /// `@safe_hint`, canonical.
    pub safe_hint: Option<String>,
    /// `@range_hint`, canonical.
    pub range_hint: Option<(String, String)>,
    /// `@renamed_from`.
    pub renamed_from: Option<String>,
}

/// A class member (`member m: pkg/class@major`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassMember {
    /// The name.
    pub name: String,
    /// The class reference, `pkg/name@major`.
    pub class: String,
    /// Whether it is a collection.
    pub collection: bool,
    /// The smallest collection; 1 for a fixed member.
    pub min: u32,
    /// The largest collection; 1 for a fixed member.
    pub max: u32,
    /// `@optional`.
    pub optional: bool,
    /// `@requires`, sorted.
    pub requires: Vec<String>,
    /// The capability that brings it.
    pub capability: Option<String>,
    /// `@renamed_from`.
    pub renamed_from: Option<String>,
}

/// A parameter, an argument or a field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    /// The name.
    pub name: String,
    /// The type.
    pub ty: Ty,
    /// The unit symbol.
    pub unit: Option<String>,
}

/// A method statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Statement {
    /// `target = expr`.
    Assign {
        /// The `out` member.
        target: String,
        /// The canonical expression.
        expr: String,
    },
    /// `member.method(args)`.
    Call {
        /// The fixed class member.
        member: String,
        /// The method of the member's class.
        method: String,
        /// The canonical argument expressions.
        args: Vec<String>,
    },
}

/// A method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodDef {
    /// The name.
    pub name: String,
    /// The capability that brings it.
    pub capability: Option<String>,
    /// `atomic`.
    pub atomic: bool,
    /// `order(…)`.
    pub order: Vec<String>,
    /// The parameters.
    pub params: Vec<Param>,
    /// The statements, in order.
    pub body: Vec<Statement>,
    /// `@renamed_from`.
    pub renamed_from: Option<String>,
}

/// A query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryDef {
    /// The name.
    pub name: String,
    /// The type.
    pub ty: Ty,
    /// The unit symbol.
    pub unit: Option<String>,
    /// The canonical expression.
    pub expr: String,
    /// The capability that brings it.
    pub capability: Option<String>,
    /// `@renamed_from`.
    pub renamed_from: Option<String>,
}

/// An event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventDef {
    /// The name.
    pub name: String,
    /// The capability that brings it.
    pub capability: Option<String>,
    /// The record, when declared as `event e: Record`.
    pub record: Option<String>,
    /// The fields, in position order.
    pub fields: Vec<Param>,
    /// `@renamed_from`.
    pub renamed_from: Option<String>,
}

/// An action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionDef {
    /// The name.
    pub name: String,
    /// The capability that brings it.
    pub capability: Option<String>,
    /// The arguments, in position order.
    pub params: Vec<Param>,
    /// The result type.
    pub result: Ty,
    /// `@timeout`, canonical.
    pub timeout: Option<String>,
    /// `@permit_hint`, canonical.
    pub permit_hint: Option<String>,
    /// `@role_hint`.
    pub role_hint: Option<String>,
    /// `@long`.
    pub long: bool,
    /// `@renamed_from`.
    pub renamed_from: Option<String>,
}

/// An enum the class uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumDef {
    /// The full name.
    pub name: String,
    /// The variants and their values, in declaration order.
    pub variants: Vec<(String, i64)>,
}

/// A record the class uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordDef {
    /// The full name.
    pub name: String,
    /// The fields, in position order.
    pub fields: Vec<Param>,
}

/// The contract part of a class: everything the contract hash covers.
#[derive(Debug, Clone, PartialEq)]
pub struct Contract {
    /// `pkg/name`.
    pub class: String,
    /// The full version.
    pub version: Version,
    /// The capabilities, sorted.
    pub capabilities: Vec<String>,
    /// The scalar members, sorted by name.
    pub members: Vec<Scalar>,
    /// The class members, sorted by name.
    pub class_members: Vec<ClassMember>,
    /// The queries, sorted by name.
    pub queries: Vec<QueryDef>,
    /// The methods, sorted by name.
    pub methods: Vec<MethodDef>,
    /// The events, sorted by name.
    pub events: Vec<EventDef>,
    /// The actions, sorted by name.
    pub actions: Vec<ActionDef>,
    /// `health required(…)`, sorted.
    pub health: Vec<String>,
    /// The enums the class uses, sorted by full name.
    pub enums: Vec<EnumDef>,
    /// The records the class uses, sorted by full name.
    pub records: Vec<RecordDef>,
}

/// The documentation of one item: its `///` text and the annotations that
/// do not change the contract.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemDocs {
    /// The `///` text.
    pub doc: Option<String>,
    /// `(name, canonical arguments)`, in source order.
    pub annotations: Vec<(String, Vec<String>)>,
}

/// The documentation part of a class: what the documentation hash covers.
/// Items are keyed `class`, `<kind>:<name>` and, for fields,
/// `<kind>:<name>.<field>`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Docs {
    /// The documented items.
    pub items: BTreeMap<String, ItemDocs>,
}

/// A compiled class: contract, documentation and their hashes.
#[derive(Debug, Clone, PartialEq)]
pub struct ContractFile {
    /// The contract.
    pub contract: Contract,
    /// The documentation.
    pub docs: Docs,
}

impl Contract {
    /// The package.
    pub fn package(&self) -> &str {
        self.class.rsplit_once('/').map_or("", |(p, _)| p)
    }

    /// The class name without the package.
    pub fn name(&self) -> &str {
        self.class.rsplit_once('/').map_or(&self.class, |(_, n)| n)
    }

    /// The reference apps and manifests use: `pkg/name@major`.
    pub fn reference(&self) -> String {
        format!("{}@{}", self.class, self.version.major)
    }

    /// A scalar member by name.
    pub fn member(&self, name: &str) -> Option<&Scalar> {
        self.members.iter().find(|m| m.name == name)
    }

    /// A class member by name.
    pub fn class_member(&self, name: &str) -> Option<&ClassMember> {
        self.class_members.iter().find(|m| m.name == name)
    }

    /// A method by name.
    pub fn method(&self, name: &str) -> Option<&MethodDef> {
        self.methods.iter().find(|m| m.name == name)
    }

    /// A query by name.
    pub fn query(&self, name: &str) -> Option<&QueryDef> {
        self.queries.iter().find(|q| q.name == name)
    }

    /// An action by name.
    pub fn action(&self, name: &str) -> Option<&ActionDef> {
        self.actions.iter().find(|a| a.name == name)
    }

    /// An event by name.
    pub fn event(&self, name: &str) -> Option<&EventDef> {
        self.events.iter().find(|e| e.name == name)
    }

    /// The JSON value of the contract.
    pub fn to_value(&self) -> Value {
        json!({
            "jkc": JKC,
            "class": self.class,
            "version": self.version.to_string(),
            "capabilities": self.capabilities,
            "members": self.members.iter().map(scalar_value).collect::<Vec<_>>(),
            "class_members": self.class_members.iter().map(class_member_value).collect::<Vec<_>>(),
            "queries": self.queries.iter().map(|q| json!({
                "name": q.name,
                "type": q.ty.render(),
                "unit": q.unit,
                "expr": q.expr,
                "capability": q.capability,
                "renamed_from": q.renamed_from,
            })).collect::<Vec<_>>(),
            "methods": self.methods.iter().map(method_value).collect::<Vec<_>>(),
            "events": self.events.iter().map(|e| json!({
                "name": e.name,
                "capability": e.capability,
                "record": e.record,
                "fields": params_value(&e.fields),
                "renamed_from": e.renamed_from,
            })).collect::<Vec<_>>(),
            "actions": self.actions.iter().map(|a| json!({
                "name": a.name,
                "capability": a.capability,
                "params": params_value(&a.params),
                "result": a.result.render(),
                "timeout": a.timeout,
                "permit_hint": a.permit_hint,
                "role_hint": a.role_hint,
                "long": a.long,
                "renamed_from": a.renamed_from,
            })).collect::<Vec<_>>(),
            "health": self.health,
            "enums": self.enums.iter().map(|e| json!({
                "name": e.name,
                "variants": e.variants.iter().map(|(n, v)| json!({"name": n, "value": v})).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "records": self.records.iter().map(|r| json!({
                "name": r.name,
                "fields": params_value(&r.fields),
            })).collect::<Vec<_>>(),
        })
    }

    /// Reads a contract back from its JSON value.
    pub fn from_value(value: &Value) -> Result<Self, String> {
        let o = obj(value, "contract")?;
        if o.get("jkc").and_then(Value::as_u64) != Some(JKC) {
            return Err(format!("contract: `jkc` is not {JKC}"));
        }
        let class = string(o, "class")?;
        let version = Version::parse(&string(o, "version")?)
            .ok_or_else(|| "contract: invalid `version`".to_owned())?;
        let members = array(o, "members")?
            .iter()
            .map(scalar_from)
            .collect::<Result<_, _>>()?;
        let class_members = array(o, "class_members")?
            .iter()
            .map(class_member_from)
            .collect::<Result<_, _>>()?;
        let queries = array(o, "queries")?
            .iter()
            .map(|q| {
                let q = obj(q, "query")?;
                Ok(QueryDef {
                    name: string(q, "name")?,
                    ty: ty(q, "type")?,
                    unit: opt_string(q, "unit")?,
                    expr: string(q, "expr")?,
                    capability: opt_string(q, "capability")?,
                    renamed_from: opt_string(q, "renamed_from")?,
                })
            })
            .collect::<Result<_, String>>()?;
        let methods = array(o, "methods")?
            .iter()
            .map(method_from)
            .collect::<Result<_, _>>()?;
        let events = array(o, "events")?
            .iter()
            .map(|e| {
                let e = obj(e, "event")?;
                Ok(EventDef {
                    name: string(e, "name")?,
                    capability: opt_string(e, "capability")?,
                    record: opt_string(e, "record")?,
                    fields: params_from(e, "fields")?,
                    renamed_from: opt_string(e, "renamed_from")?,
                })
            })
            .collect::<Result<_, String>>()?;
        let actions = array(o, "actions")?
            .iter()
            .map(|a| {
                let a = obj(a, "action")?;
                Ok(ActionDef {
                    name: string(a, "name")?,
                    capability: opt_string(a, "capability")?,
                    params: params_from(a, "params")?,
                    result: ty(a, "result")?,
                    timeout: opt_string(a, "timeout")?,
                    permit_hint: opt_string(a, "permit_hint")?,
                    role_hint: opt_string(a, "role_hint")?,
                    long: boolean(a, "long")?,
                    renamed_from: opt_string(a, "renamed_from")?,
                })
            })
            .collect::<Result<_, String>>()?;
        let enums = array(o, "enums")?
            .iter()
            .map(|e| {
                let e = obj(e, "enum")?;
                let variants = array(e, "variants")?
                    .iter()
                    .map(|v| {
                        let v = obj(v, "variant")?;
                        let value = v
                            .get("value")
                            .and_then(Value::as_i64)
                            .ok_or_else(|| "variant: invalid `value`".to_owned())?;
                        Ok((string(v, "name")?, value))
                    })
                    .collect::<Result<_, String>>()?;
                Ok(EnumDef {
                    name: string(e, "name")?,
                    variants,
                })
            })
            .collect::<Result<_, String>>()?;
        let records = array(o, "records")?
            .iter()
            .map(|r| {
                let r = obj(r, "record")?;
                Ok(RecordDef {
                    name: string(r, "name")?,
                    fields: params_from(r, "fields")?,
                })
            })
            .collect::<Result<_, String>>()?;
        Ok(Self {
            class,
            version,
            capabilities: strings(o, "capabilities")?,
            members,
            class_members,
            queries,
            methods,
            events,
            actions,
            health: strings(o, "health")?,
            enums,
            records,
        })
    }
}

impl Docs {
    /// The JSON value of the documentation.
    pub fn to_value(&self) -> Value {
        let items: Map<String, Value> = self
            .items
            .iter()
            .map(|(k, d)| {
                let annotations: Vec<Value> = d
                    .annotations
                    .iter()
                    .map(|(n, a)| json!({"name": n, "args": a}))
                    .collect();
                (k.clone(), json!({"doc": d.doc, "annotations": annotations}))
            })
            .collect();
        json!({ "items": items })
    }

    /// Reads the documentation back from its JSON value.
    pub fn from_value(value: &Value) -> Result<Self, String> {
        let o = obj(value, "docs")?;
        let items = obj(
            o.get("items")
                .ok_or_else(|| "docs: missing `items`".to_owned())?,
            "items",
        )?;
        let mut out = BTreeMap::new();
        for (key, item) in items {
            let item = obj(item, "item")?;
            let annotations = array(item, "annotations")?
                .iter()
                .map(|a| {
                    let a = obj(a, "annotation")?;
                    Ok((string(a, "name")?, strings(a, "args")?))
                })
                .collect::<Result<_, String>>()?;
            out.insert(
                key.clone(),
                ItemDocs {
                    doc: opt_string(item, "doc")?,
                    annotations,
                },
            );
        }
        Ok(Self { items: out })
    }
}

impl ContractFile {
    /// The contract hash: SHA-256 of the canonical `contract` object.
    pub fn contract_hash(&self) -> [u8; 32] {
        sha256(&canonical_json(&self.contract.to_value()))
    }

    /// The documentation hash: SHA-256 of the canonical `docs` object.
    pub fn doc_hash(&self) -> [u8; 32] {
        sha256(&canonical_json(&self.docs.to_value()))
    }

    /// The file name: `<name>@<version>.contract.json` (docs/15).
    pub fn file_name(&self) -> String {
        format!(
            "{}@{}.contract.json",
            self.contract.name(),
            self.contract.version
        )
    }

    /// The canonical bytes of the `.contract.json` file.
    pub fn to_json(&self) -> String {
        canonical_json(&json!({
            "contract": self.contract.to_value(),
            "contract_hash": digest(&self.contract_hash()),
            "docs": self.docs.to_value(),
            "doc_hash": digest(&self.doc_hash()),
        }))
    }

    /// Reads a `.contract.json` file, verifying both hashes.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let o = obj(&value, "contract file")?;
        let file = Self {
            contract: Contract::from_value(
                o.get("contract")
                    .ok_or_else(|| "missing `contract`".to_owned())?,
            )?,
            docs: Docs::from_value(o.get("docs").ok_or_else(|| "missing `docs`".to_owned())?)?,
        };
        if string(o, "contract_hash")? != digest(&file.contract_hash()) {
            return Err("`contract_hash` does not match the contract".into());
        }
        if string(o, "doc_hash")? != digest(&file.doc_hash()) {
            return Err("`doc_hash` does not match the documentation".into());
        }
        Ok(file)
    }
}

/// `sha256:<hex>` (ref/00 §6).
pub fn digest(hash: &[u8; 32]) -> String {
    let mut out = String::from("sha256:");
    for b in hash {
        let _ = write!(out, "{b:02x}");
    }
    out
}

fn sha256(text: &str) -> [u8; 32] {
    Sha256::digest(text.as_bytes()).into()
}

/// The canonical text of a JSON value: keys sorted by their bytes, no
/// whitespace, minimal escapes (ADR 0059).
pub fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_value(value, &mut out);
    out
}

fn write_value(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            let _ = write!(out, "{n}");
        }
        Value::String(s) => write_string(s, out),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, key) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(key, out);
                out.push(':');
                if let Some(v) = map.get(key) {
                    write_value(v, out);
                }
            }
            out.push('}');
        }
    }
}

fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn scalar_value(m: &Scalar) -> Value {
    json!({
        "name": m.name,
        "kind": m.direction.keyword(),
        "type": m.ty.render(),
        "unit": m.unit,
        "capability": m.capability,
        "optional": m.optional,
        "default": m.default,
        "safe_hint": m.safe_hint,
        "range_hint": m.range_hint.as_ref().map(|(a, b)| vec![a, b]),
        "renamed_from": m.renamed_from,
    })
}

fn class_member_value(m: &ClassMember) -> Value {
    json!({
        "name": m.name,
        "class": m.class,
        "collection": m.collection,
        "min": m.min,
        "max": m.max,
        "optional": m.optional,
        "requires": m.requires,
        "capability": m.capability,
        "renamed_from": m.renamed_from,
    })
}

fn params_value(params: &[Param]) -> Vec<Value> {
    params
        .iter()
        .map(|p| json!({"name": p.name, "type": p.ty.render(), "unit": p.unit}))
        .collect()
}

fn method_value(m: &MethodDef) -> Value {
    let body: Vec<Value> = m
        .body
        .iter()
        .map(|s| match s {
            Statement::Assign { target, expr } => json!({"assign": target, "expr": expr}),
            Statement::Call {
                member,
                method,
                args,
            } => json!({"call": member, "method": method, "args": args}),
        })
        .collect();
    json!({
        "name": m.name,
        "capability": m.capability,
        "atomic": m.atomic,
        "order": m.order,
        "params": params_value(&m.params),
        "body": body,
        "renamed_from": m.renamed_from,
    })
}

type Object = Map<String, Value>;

fn obj<'v>(value: &'v Value, what: &str) -> Result<&'v Object, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{what}: expected an object"))
}

fn array<'v>(o: &'v Object, key: &str) -> Result<&'v Vec<Value>, String> {
    o.get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("expected an array in `{key}`"))
}

fn string(o: &Object, key: &str) -> Result<String, String> {
    o.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("expected a string in `{key}`"))
}

fn opt_string(o: &Object, key: &str) -> Result<Option<String>, String> {
    match o.get(key) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        _ => Err(format!("expected a string or null in `{key}`")),
    }
}

fn boolean(o: &Object, key: &str) -> Result<bool, String> {
    o.get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("expected a boolean in `{key}`"))
}

fn number(o: &Object, key: &str) -> Result<u32, String> {
    o.get(key)
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| format!("expected an integer in `{key}`"))
}

fn strings(o: &Object, key: &str) -> Result<Vec<String>, String> {
    array(o, key)?
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("expected strings in `{key}`"))
        })
        .collect()
}

fn ty(o: &Object, key: &str) -> Result<Ty, String> {
    Ty::parse(&string(o, key)?).ok_or_else(|| format!("invalid type in `{key}`"))
}

fn params_from(o: &Object, key: &str) -> Result<Vec<Param>, String> {
    array(o, key)?
        .iter()
        .map(|p| {
            let p = obj(p, key)?;
            Ok(Param {
                name: string(p, "name")?,
                ty: ty(p, "type")?,
                unit: opt_string(p, "unit")?,
            })
        })
        .collect()
}

fn scalar_from(value: &Value) -> Result<Scalar, String> {
    let m = obj(value, "member")?;
    let direction = match string(m, "kind")?.as_str() {
        "in" => Direction::In,
        "out" => Direction::Out,
        "param" => Direction::Param,
        other => return Err(format!("member: unknown kind `{other}`")),
    };
    let range_hint = match m.get("range_hint") {
        Some(Value::Null) => None,
        Some(Value::Array(pair)) => match pair.as_slice() {
            [Value::String(a), Value::String(b)] => Some((a.clone(), b.clone())),
            _ => return Err("member: invalid `range_hint`".into()),
        },
        _ => return Err("member: invalid `range_hint`".into()),
    };
    Ok(Scalar {
        name: string(m, "name")?,
        direction,
        ty: ty(m, "type")?,
        unit: opt_string(m, "unit")?,
        capability: opt_string(m, "capability")?,
        optional: boolean(m, "optional")?,
        default: opt_string(m, "default")?,
        safe_hint: opt_string(m, "safe_hint")?,
        range_hint,
        renamed_from: opt_string(m, "renamed_from")?,
    })
}

fn class_member_from(value: &Value) -> Result<ClassMember, String> {
    let m = obj(value, "class member")?;
    Ok(ClassMember {
        name: string(m, "name")?,
        class: string(m, "class")?,
        collection: boolean(m, "collection")?,
        min: number(m, "min")?,
        max: number(m, "max")?,
        optional: boolean(m, "optional")?,
        requires: strings(m, "requires")?,
        capability: opt_string(m, "capability")?,
        renamed_from: opt_string(m, "renamed_from")?,
    })
}

fn method_from(value: &Value) -> Result<MethodDef, String> {
    let m = obj(value, "method")?;
    let body = array(m, "body")?
        .iter()
        .map(|s| {
            let s = obj(s, "statement")?;
            if s.contains_key("assign") {
                Ok(Statement::Assign {
                    target: string(s, "assign")?,
                    expr: string(s, "expr")?,
                })
            } else {
                Ok(Statement::Call {
                    member: string(s, "call")?,
                    method: string(s, "method")?,
                    args: strings(s, "args")?,
                })
            }
        })
        .collect::<Result<_, String>>()?;
    Ok(MethodDef {
        name: string(m, "name")?,
        capability: opt_string(m, "capability")?,
        atomic: boolean(m, "atomic")?,
        order: strings(m, "order")?,
        params: params_from(m, "params")?,
        body,
        renamed_from: opt_string(m, "renamed_from")?,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn types_round_trip() {
        for t in [
            "bool",
            "bits<16>",
            "enum std/PcsMode",
            "record std/S",
            "list<record std/S, 8>",
            "list<u64, 4>",
            "()",
        ] {
            assert_eq!(Ty::parse(t).unwrap().render(), t);
        }
    }

    #[test]
    fn canonical_json_sorts_keys_and_escapes_minimally() {
        let v = json!({"b": [1, "x\"y\n"], "a": {"d": null, "c": true}, "é": "µ"});
        assert_eq!(
            canonical_json(&v),
            r#"{"a":{"c":true,"d":null},"b":[1,"x\"y\n"],"é":"µ"}"#
        );
    }
}
