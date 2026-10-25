//! `jkc check` (docs/17 §7, ADR 0059): package resolution, types and units,
//! the rules of the language, and the contract of each class.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, PathBuf};

use joker_expr::{CheckOptions, EnumId, SignalInfo, SignalType};
use joker_manifest::source::Span;
use joker_manifest::{Diagnostic, Diagnostics};
use joker_units::{Duration, Literal, Unit};

use crate::ast::{
    Action, Annotation, Class, ClassRef, Direction, EventBody, Field, File, Item, Member, Method,
    Nested, Query, Spanned, Stmt, TypeRef,
};
use crate::contract::{
    ActionDef, ClassMember, Contract, ContractFile, Docs, EnumDef, EventDef, ItemDocs, MethodDef,
    Param, QueryDef, RecordDef, Scalar, Statement, Ty,
};
use crate::expr::{ClassEnv, Read, canonical, reads};

/// Names the generated handles use in every language (docs/17 §3.2).
pub const RESERVED: [&str; 6] = ["name", "index", "health", "read", "raw", "has"];

/// The deepest chain of class members, counting the class (docs/17 §8).
pub const MAX_DEPTH: usize = 3;

/// The largest collection (docs/17 §3.2).
pub const MAX_COLLECTION: u32 = 1024;

/// The longest bounded list (docs/17 §3.3).
pub const MAX_LIST: u32 = 64;

/// The roles an `@role_hint` may name (ref/09 §3).
pub const ROLES: [&str; 4] = ["viewer", "operator", "engineer", "admin"];

/// A class, by package, name and major.
pub type ClassKey = (String, String, u32);

/// One parsed file.
#[derive(Debug, Clone)]
pub struct Parsed {
    /// The path, relative to the project root.
    pub path: PathBuf,
    /// The syntax tree.
    pub ast: File,
}

/// Every parsed file of one compilation, indexed by class, enum and record.
#[derive(Debug)]
pub struct Universe<'a> {
    files: &'a [Parsed],
    classes: BTreeMap<ClassKey, (usize, usize)>,
    enums: BTreeMap<(String, String), (usize, usize, u32)>,
    records: BTreeMap<(String, String), (usize, usize)>,
}

fn diag(code: &'static str, title: impl Into<String>, span: Span) -> Diagnostic {
    Diagnostic::new(code, title).at(span)
}

fn is_snake(name: &str) -> bool {
    name.len() <= 32
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn is_camel(name: &str) -> bool {
    name.len() <= 32
        && name.starts_with(|c: char| c.is_ascii_uppercase())
        && name.chars().all(|c| c.is_ascii_alphanumeric())
}

fn is_class_name(name: &str) -> bool {
    name.split('.').all(is_snake)
}

impl<'a> Universe<'a> {
    /// Indexes the files, reporting duplicate classes, enums and records.
    pub fn new(files: &'a [Parsed], diags: &mut Diagnostics) -> Self {
        let mut universe = Self {
            files,
            classes: BTreeMap::new(),
            enums: BTreeMap::new(),
            records: BTreeMap::new(),
        };
        let mut next_enum = 0u32;
        for (fi, file) in files.iter().enumerate() {
            let package = file.ast.package.value.clone();
            for (ci, class) in file.ast.classes.iter().enumerate() {
                let key = (
                    package.clone(),
                    class.name.value.clone(),
                    class.version.value.major,
                );
                if universe.classes.insert(key, (fi, ci)).is_some() {
                    diags.push(diag(
                        "JC0102",
                        format!(
                            "class `{package}/{}@{}` is declared twice",
                            class.name.value, class.version.value.major
                        ),
                        class.name.span,
                    ));
                }
            }
            for (ei, e) in file.ast.enums.iter().enumerate() {
                let key = (package.clone(), e.name.value.clone());
                if universe.enums.contains_key(&key) || universe.records.contains_key(&key) {
                    diags.push(diag(
                        "JC0102",
                        format!("`{package}/{}` is declared twice", e.name.value),
                        e.name.span,
                    ));
                    continue;
                }
                universe.enums.insert(key, (fi, ei, next_enum));
                next_enum += 1;
            }
            for (ri, r) in file.ast.records.iter().enumerate() {
                let key = (package.clone(), r.name.value.clone());
                if universe.enums.contains_key(&key) || universe.records.contains_key(&key) {
                    diags.push(diag(
                        "JC0102",
                        format!("`{package}/{}` is declared twice", r.name.value),
                        r.name.span,
                    ));
                    continue;
                }
                universe.records.insert(key, (fi, ri));
            }
        }
        universe
    }

    /// The class keys, sorted.
    pub fn class_keys(&self) -> impl Iterator<Item = &ClassKey> {
        self.classes.keys()
    }

    fn class(&self, key: &ClassKey) -> Option<(&'a Parsed, &'a Class)> {
        let (fi, ci) = self.classes.get(key)?;
        let file = self.files.get(*fi)?;
        Some((file, file.ast.classes.get(*ci)?))
    }

    fn resolve_class(&self, file: &File, r: &ClassRef) -> ClassKey {
        if let Some(package) = &r.package {
            return (package.clone(), r.name.clone(), r.major);
        }
        file.uses
            .iter()
            .find(|u| u.local_name() == r.name && u.major == Some(r.major))
            .map_or_else(
                || ("std".to_owned(), r.name.clone(), r.major),
                |u| (u.package.clone(), u.name.clone(), r.major),
            )
    }

    fn resolve_named(&self, file: &File, name: &str) -> Option<Ty> {
        let local = (file.package.value.clone(), name.to_owned());
        let key = if self.enums.contains_key(&local) || self.records.contains_key(&local) {
            local
        } else {
            let u = file
                .uses
                .iter()
                .find(|u| u.local_name() == name && u.major.is_none())?;
            (u.package.clone(), u.name.clone())
        };
        let full = format!("{}/{}", key.0, key.1);
        if self.enums.contains_key(&key) {
            Some(Ty::Enum(full))
        } else if self.records.contains_key(&key) {
            Some(Ty::Record(full))
        } else {
            None
        }
    }

    fn split(full: &str) -> (String, String) {
        full.rsplit_once('/')
            .map(|(p, n)| (p.to_owned(), n.to_owned()))
            .unwrap_or_default()
    }

    fn enum_def(&self, full: &str) -> Option<(EnumDef, u32)> {
        let (fi, ei, id) = self.enums.get(&Self::split(full))?;
        let e = self.files.get(*fi)?.ast.enums.get(*ei)?;
        Some((
            EnumDef {
                name: full.to_owned(),
                variants: e
                    .variants
                    .iter()
                    .map(|(n, v)| (n.value.clone(), *v))
                    .collect(),
            },
            *id,
        ))
    }

    fn record_fields(&self, full: &str) -> Option<(&'a Parsed, &'a [Field])> {
        let (fi, ri) = self.records.get(&Self::split(full))?;
        let file = self.files.get(*fi)?;
        Some((file, &file.ast.records.get(*ri)?.fields))
    }

    fn resolve_type(&self, file: &File, t: &TypeRef) -> Result<Ty, String> {
        Ok(match t {
            TypeRef::Bool => Ty::Bool,
            TypeRef::I64 => Ty::I64,
            TypeRef::U64 => Ty::U64,
            TypeRef::F64 => Ty::F64,
            TypeRef::F32 => Ty::F32,
            TypeRef::String => Ty::String,
            TypeRef::Bytes => Ty::Bytes,
            TypeRef::Unit => Ty::Unit,
            TypeRef::Bits(n) if (1..=64).contains(n) => Ty::Bits(*n),
            TypeRef::Bits(n) => return Err(format!("`bits<{n}>`: N is between 1 and 64")),
            TypeRef::Named(name) => self
                .resolve_named(file, name)
                .ok_or_else(|| format!("unknown type `{name}`"))?,
            TypeRef::List(inner, n) => {
                if *n == 0 || *n > MAX_LIST {
                    return Err(format!("`list<_, {n}>`: N is between 1 and {MAX_LIST}"));
                }
                let inner = self.resolve_type(file, inner)?;
                if matches!(inner, Ty::List(..) | Ty::Unit) {
                    return Err("a list holds scalars, enums or records".into());
                }
                Ty::List(Box::new(inner), *n)
            }
        })
    }

    /// The signature of a class, read from its declaration.
    pub fn signature(&self, key: &ClassKey) -> Option<Signature> {
        let (file, class) = self.class(key)?;
        let mut sig = Signature::default();
        self.collect_signature(&file.ast, &class.items, None, &mut sig);
        Some(sig)
    }

    fn collect_signature(
        &self,
        file: &File,
        items: &[Item],
        cap: Option<&str>,
        sig: &mut Signature,
    ) {
        let cap_owned = cap.map(str::to_owned);
        for item in items {
            match item {
                Item::Member(m) => {
                    let ty = self.resolve_type(file, &m.ty.value).ok();
                    let unit = m.unit.as_ref().and_then(|u| Unit::parse(&u.value).ok());
                    sig.scalars.insert(
                        m.name.value.clone(),
                        SigScalar {
                            direction: m.direction,
                            ty,
                            unit,
                            capability: cap_owned.clone(),
                        },
                    );
                }
                Item::Query(q) => {
                    let ty = self.resolve_type(file, &q.ty.value).ok();
                    let unit = q.unit.as_ref().and_then(|u| Unit::parse(&u.value).ok());
                    sig.queries.insert(
                        q.name.value.clone(),
                        SigScalar {
                            direction: Direction::In,
                            ty,
                            unit,
                            capability: cap_owned.clone(),
                        },
                    );
                }
                Item::Nested(n) => {
                    sig.nested.insert(
                        n.name.value.clone(),
                        SigNested {
                            key: self.resolve_class(file, &n.class.value),
                            collection: n.range.is_some(),
                            requires: requires_of(&n.annotations),
                            capability: cap_owned.clone(),
                        },
                    );
                }
                Item::Method(m) => {
                    let params = m
                        .params
                        .iter()
                        .map(|p| {
                            (
                                self.resolve_type(file, &p.ty.value).ok(),
                                p.unit.as_ref().and_then(|u| Unit::parse(&u.value).ok()),
                            )
                        })
                        .collect();
                    let mut writes = Vec::new();
                    let mut calls = Vec::new();
                    for s in &m.body {
                        match s {
                            Stmt::Assign { target, .. } => writes.push(target.value.clone()),
                            Stmt::Call {
                                member: Some(member),
                                method,
                                ..
                            } => calls.push((member.value.clone(), method.value.clone())),
                            Stmt::Call { member: None, .. } => {}
                        }
                    }
                    sig.methods.insert(
                        m.name.value.clone(),
                        SigMethod {
                            params,
                            capability: cap_owned.clone(),
                            writes,
                            calls,
                        },
                    );
                }
                Item::Action(a) => {
                    sig.actions.insert(a.name.value.clone(), cap_owned.clone());
                }
                Item::Event(e) => {
                    sig.events.insert(e.name.value.clone(), cap_owned.clone());
                }
                Item::Capability(c) => {
                    sig.capabilities.insert(c.name.value.clone());
                    self.collect_signature(file, &c.items, Some(&c.name.value), sig);
                }
                Item::Health(_) => {}
            }
        }
    }

    /// The composition depth of a class (docs/17 §8), or `None` when the
    /// chain does not end (the class contains itself).
    pub fn depth(&self, key: &ClassKey) -> Option<usize> {
        self.depth_from(key, &mut Vec::new())
    }

    fn depth_from(&self, key: &ClassKey, stack: &mut Vec<ClassKey>) -> Option<usize> {
        if stack.contains(key) {
            return None;
        }
        let Some(sig) = self.signature(key) else {
            return Some(1);
        };
        stack.push(key.clone());
        let mut deepest = 0;
        for n in sig.nested.values() {
            let d = self.depth_from(&n.key, stack)?;
            deepest = deepest.max(d);
        }
        stack.pop();
        Some(deepest + 1)
    }

    fn expanded_writes(
        &self,
        key: &ClassKey,
        method: &str,
        prefix: &str,
        depth: usize,
        out: &mut Vec<String>,
    ) {
        if depth > MAX_DEPTH {
            return;
        }
        let Some(sig) = self.signature(key) else {
            return;
        };
        let Some(m) = sig.methods.get(method) else {
            return;
        };
        for w in &m.writes {
            out.push(format!("{prefix}{w}"));
        }
        for (member, called) in &m.calls {
            if let Some(n) = sig.nested.get(member) {
                self.expanded_writes(
                    &n.key,
                    called,
                    &format!("{prefix}{member}."),
                    depth + 1,
                    out,
                );
            }
        }
    }
}

fn requires_of(annotations: &[Annotation]) -> Vec<String> {
    let mut out: Vec<String> = annotations
        .iter()
        .filter(|a| a.name.value == "requires")
        .flat_map(|a| a.args.iter().map(|x| x.value.clone()))
        .collect();
    out.sort();
    out.dedup();
    out
}

/// A scalar member or query, as other classes see it.
#[derive(Debug, Clone)]
pub struct SigScalar {
    /// The direction (`in` for queries).
    pub direction: Direction,
    /// The type, when it resolves.
    pub ty: Option<Ty>,
    /// The unit, when declared and valid.
    pub unit: Option<Unit>,
    /// The capability that brings it.
    pub capability: Option<String>,
}

/// A class member, as other classes see it.
#[derive(Debug, Clone)]
pub struct SigNested {
    /// The member's class.
    pub key: ClassKey,
    /// Whether it is a collection.
    pub collection: bool,
    /// `@requires`.
    pub requires: Vec<String>,
    /// The capability that brings it.
    pub capability: Option<String>,
}

/// A method, as other classes see it.
#[derive(Debug, Clone)]
pub struct SigMethod {
    /// The parameter types and units.
    pub params: Vec<(Option<Ty>, Option<Unit>)>,
    /// The capability that brings it.
    pub capability: Option<String>,
    /// The `out` members it assigns.
    pub writes: Vec<String>,
    /// The `(member, method)` calls it makes.
    pub calls: Vec<(String, String)>,
}

/// What a class offers to the classes that contain it.
#[derive(Debug, Clone, Default)]
pub struct Signature {
    /// `in`, `out` and `param`.
    pub scalars: BTreeMap<String, SigScalar>,
    /// Queries.
    pub queries: BTreeMap<String, SigScalar>,
    /// Class members.
    pub nested: BTreeMap<String, SigNested>,
    /// Methods.
    pub methods: BTreeMap<String, SigMethod>,
    /// Actions and their capability.
    pub actions: BTreeMap<String, Option<String>>,
    /// Events and their capability.
    pub events: BTreeMap<String, Option<String>>,
    /// Capabilities.
    pub capabilities: BTreeSet<String>,
}

/// Checks the file-level declarations of one file: the package against the
/// path, one class per file, the class name against the file name, `use`,
/// enums and records.
pub fn check_file(universe: &Universe<'_>, parsed: &Parsed, diags: &mut Diagnostics) {
    let file = &parsed.ast;
    if !file
        .package
        .value
        .split('/')
        .all(|s| is_snake(s) && !s.contains("__"))
    {
        diags.push(diag(
            "JC0101",
            "a package is `ident (\"/\" ident)*`",
            file.package.span,
        ));
    }
    let components: Vec<String> = parsed
        .path
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => s.to_str().map(str::to_owned),
            _ => None,
        })
        .collect();
    if let Some(pos) = components.iter().rposition(|c| c == "classes") {
        let dir = components
            .get(pos + 1..components.len().saturating_sub(1))
            .map(|d| d.join("/"))
            .unwrap_or_default();
        if dir != file.package.value {
            diags.push(diag(
                "J0804",
                format!(
                    "package `{}` does not match the directory `classes/{dir}`",
                    file.package.value
                ),
                file.package.span,
            ));
        }
    }
    if let Some(second) = file.classes.get(1) {
        diags.push(diag(
            "J0804",
            "one class per file (ref/03 §2.7)",
            second.name.span,
        ));
    }
    let stem = parsed
        .path
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_suffix(".jkc"))
        .unwrap_or("");
    for class in &file.classes {
        let name = &class.name.value;
        let with_major = format!("{name}@{}", class.version.value.major);
        if stem != name.as_str() && stem != with_major {
            diags.push(diag(
                "J0804",
                format!("class `{name}` is in `{stem}.jkc`: the file is `{name}.jkc` or `{with_major}.jkc`"),
                class.name.span,
            ));
        }
    }
    for u in &file.uses {
        let found = match u.major {
            Some(major) => {
                universe
                    .classes
                    .contains_key(&(u.package.clone(), u.name.clone(), major))
            }
            None => {
                let key = (u.package.clone(), u.name.clone());
                universe.enums.contains_key(&key) || universe.records.contains_key(&key)
            }
        };
        if !found {
            diags.push(diag(
                "J0847",
                format!(
                    "`use {}/{}` names a class, enum or record that is neither in the registry nor in the project",
                    u.package, u.name
                ),
                u.span,
            ));
        }
    }
    for e in &file.enums {
        if !is_camel(&e.name.value) {
            diags.push(diag(
                "JC0101",
                "an enum name is `[A-Z][A-Za-z0-9]*`, at most 32 characters",
                e.name.span,
            ));
        }
        let mut names = BTreeSet::new();
        let mut values = BTreeSet::new();
        for (v, value) in &e.variants {
            if !is_snake(&v.value) {
                diags.push(diag(
                    "JC0101",
                    "a variant is `[a-z][a-z0-9_]*`, at most 32 characters",
                    v.span,
                ));
            }
            if !names.insert(v.value.clone()) {
                diags.push(diag(
                    "JC0102",
                    format!("variant `{}` is declared twice", v.value),
                    v.span,
                ));
            }
            if *value < 0 || !values.insert(*value) {
                diags.push(diag(
                    "JC0102",
                    format!(
                        "variant `{}`: values are distinct and not negative",
                        v.value
                    ),
                    v.span,
                ));
            }
        }
    }
    for r in &file.records {
        if !is_camel(&r.name.value) {
            diags.push(diag(
                "JC0101",
                "a record name is `[A-Z][A-Za-z0-9]*`, at most 32 characters",
                r.name.span,
            ));
        }
        let mut seen = BTreeSet::new();
        for f in &r.fields {
            if !seen.insert(f.name.value.clone()) {
                diags.push(diag(
                    "JC0102",
                    format!("field `{}` is declared twice", f.name.value),
                    f.name.span,
                ));
            }
            check_field(universe, file, f, false, diags);
        }
    }
}

fn check_field(
    universe: &Universe<'_>,
    file: &File,
    f: &Field,
    scalar_only: bool,
    diags: &mut Diagnostics,
) -> Option<Param> {
    if !is_snake(&f.name.value) {
        diags.push(diag(
            "JC0101",
            "a name is `[a-z][a-z0-9_]*`, at most 32 characters",
            f.name.span,
        ));
    }
    let ty = match universe.resolve_type(file, &f.ty.value) {
        Ok(ty) => ty,
        Err(e) => {
            let code = if e.starts_with("unknown") {
                "JC0103"
            } else {
                "JC0106"
            };
            diags.push(diag(code, e, f.ty.span));
            return None;
        }
    };
    if scalar_only && !ty.is_scalar() {
        diags.push(diag(
            "JC0106",
            format!(
                "`{}`: records and lists appear only in events and actions (docs/17 §3.3)",
                ty.render()
            ),
            f.ty.span,
        ));
        return None;
    }
    if ty == Ty::Unit {
        diags.push(diag("JC0106", "`()` is only a result type", f.ty.span));
        return None;
    }
    let unit = unit_of(f.unit.as_ref(), diags);
    if ty.is_float() && f.unit.is_none() {
        diags.push(diag(
            "JC0107",
            format!(
                "`{}` is a physical quantity without a unit; write `1` if it is dimensionless",
                f.name.value
            ),
            f.ty.span,
        ));
    }
    Some(Param {
        name: f.name.value.clone(),
        ty,
        unit: unit.map(|_| f.unit.as_ref().map(|u| u.value.clone()).unwrap_or_default()),
    })
}

fn unit_of(unit: Option<&Spanned<String>>, diags: &mut Diagnostics) -> Option<Unit> {
    let u = unit?;
    match Unit::parse(&u.value) {
        Ok(unit) => Some(unit),
        Err(e) => {
            diags.push(diag(
                "JC0106",
                format!("invalid unit `{}`: {}", u.value, e.kind.message()),
                u.span,
            ));
            None
        }
    }
}

fn signal_type(universe: &Universe<'_>, ty: &Ty) -> Option<SignalType> {
    Some(match ty {
        Ty::Bool => SignalType::Bool,
        Ty::I64 => SignalType::I64,
        Ty::U64 => SignalType::U64,
        Ty::F64 => SignalType::F64,
        Ty::F32 => SignalType::F32,
        Ty::Bits(_) => SignalType::Bits,
        Ty::String => SignalType::String,
        Ty::Bytes => SignalType::Bytes,
        Ty::Enum(name) => SignalType::Enum(EnumId(universe.enum_def(name)?.1)),
        Ty::Record(_) | Ty::List(..) | Ty::Unit => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Scalar(Direction),
    Query,
    Event,
    Action,
    Nested { collection: bool },
}

#[derive(Debug, Clone)]
struct Sym {
    kind: Kind,
    capability: Option<String>,
    span: Span,
    ty: Option<Ty>,
    unit: Option<Unit>,
    unit_free: bool,
    nested: Option<ClassKey>,
}

/// Checks one class and builds its contract; `None` when it has errors.
pub fn check_class(
    universe: &Universe<'_>,
    parsed: &Parsed,
    class: &Class,
    diags: &mut Diagnostics,
) -> Option<ContractFile> {
    let before = diags.iter().filter(|d| !d.code.starts_with("JW")).count();
    let mut checker = Checker {
        universe,
        file: &parsed.ast,
        class,
        diags,
        symbols: BTreeMap::new(),
        methods: BTreeMap::new(),
        capabilities: BTreeMap::new(),
        health: None,
        env: ClassEnv::default(),
        docs: Docs::default(),
        used: BTreeSet::new(),
    };
    let contract = checker.run();
    let errors = checker
        .diags
        .iter()
        .filter(|d| !d.code.starts_with("JW"))
        .count();
    (errors == before).then_some(contract).flatten()
}

struct Checker<'u, 'a, 'd> {
    universe: &'u Universe<'a>,
    file: &'a File,
    class: &'a Class,
    diags: &'d mut Diagnostics,
    symbols: BTreeMap<String, Sym>,
    methods: BTreeMap<String, (&'a Method, Option<String>)>,
    capabilities: BTreeMap<String, Span>,
    health: Option<&'a crate::ast::Health>,
    env: ClassEnv,
    docs: Docs,
    used: BTreeSet<Ty>,
}

enum Owned<'a> {
    Member(&'a Member),
    Nested(&'a Nested),
    Query(&'a Query),
    Event(&'a crate::ast::Event),
    Action(&'a Action),
}

impl<'a> Checker<'_, 'a, '_> {
    fn push(&mut self, code: &'static str, title: impl Into<String>, span: Span) {
        self.diags.push(diag(code, title, span));
    }

    fn run(&mut self) -> Option<ContractFile> {
        let class = self.class;
        if !is_class_name(&class.name.value) {
            self.push(
                "JC0101",
                "a class name is snake_case segments separated by `.`",
                class.name.span,
            );
        }
        let mut owned: Vec<(Owned<'a>, Option<String>)> = Vec::new();
        self.collect(&class.items, None, &mut owned);
        self.class_docs();
        self.names();
        let mut members = Vec::new();
        let mut class_members = Vec::new();
        let mut queries_ast = Vec::new();
        let mut events = Vec::new();
        let mut actions_ast = Vec::new();
        for (item, cap) in &owned {
            match item {
                Owned::Member(m) => {
                    if let Some(s) = self.member(m, cap.clone()) {
                        members.push(s);
                    }
                }
                Owned::Nested(n) => {
                    if let Some(c) = self.nested(n, cap.clone()) {
                        class_members.push(c);
                    }
                }
                Owned::Query(q) => queries_ast.push((*q, cap.clone())),
                Owned::Event(e) => {
                    if let Some(e) = self.event(e, cap.clone()) {
                        events.push(e);
                    }
                }
                Owned::Action(a) => actions_ast.push((*a, cap.clone())),
            }
        }
        let queries = self.queries(&queries_ast);
        let actions: Vec<ActionDef> = actions_ast
            .iter()
            .filter_map(|(a, cap)| self.action(a, cap.clone()))
            .collect();
        let methods: Vec<(&'a Method, Option<String>)> = self
            .methods
            .values()
            .map(|(m, c)| (*m, c.clone()))
            .collect();
        let methods: Vec<MethodDef> = methods
            .into_iter()
            .filter_map(|(m, cap)| self.method(m, cap))
            .collect();
        let health = self.health_names();
        self.depth();
        let mut contract = Contract {
            class: format!("{}/{}", self.file.package.value, class.name.value),
            version: class.version.value,
            capabilities: self.capabilities.keys().cloned().collect(),
            members,
            class_members,
            queries,
            methods,
            events,
            actions,
            health,
            enums: Vec::new(),
            records: Vec::new(),
        };
        contract.members.sort_by(|a, b| a.name.cmp(&b.name));
        contract.class_members.sort_by(|a, b| a.name.cmp(&b.name));
        contract.queries.sort_by(|a, b| a.name.cmp(&b.name));
        contract.methods.sort_by(|a, b| a.name.cmp(&b.name));
        contract.events.sort_by(|a, b| a.name.cmp(&b.name));
        contract.actions.sort_by(|a, b| a.name.cmp(&b.name));
        self.used_types(&mut contract);
        Some(ContractFile {
            contract,
            docs: std::mem::take(&mut self.docs),
        })
    }

    fn collect(
        &mut self,
        items: &'a [Item],
        cap: Option<&str>,
        owned: &mut Vec<(Owned<'a>, Option<String>)>,
    ) {
        let cap_owned = cap.map(str::to_owned);
        for item in items {
            let (name, kind, span, own) = match item {
                Item::Member(m) => (
                    &m.name,
                    Kind::Scalar(m.direction),
                    m.name.span,
                    Owned::Member(m),
                ),
                Item::Nested(n) => (
                    &n.name,
                    Kind::Nested {
                        collection: n.range.is_some(),
                    },
                    n.name.span,
                    Owned::Nested(n),
                ),
                Item::Query(q) => (&q.name, Kind::Query, q.name.span, Owned::Query(q)),
                Item::Event(e) => (&e.name, Kind::Event, e.name.span, Owned::Event(e)),
                Item::Action(a) => (&a.name, Kind::Action, a.name.span, Owned::Action(a)),
                Item::Method(m) => {
                    if self.methods.contains_key(&m.name.value) {
                        self.push(
                            "JC0102",
                            format!("method `{}` is declared twice", m.name.value),
                            m.name.span,
                        );
                    } else {
                        self.methods
                            .insert(m.name.value.clone(), (m, cap_owned.clone()));
                    }
                    continue;
                }
                Item::Capability(c) => {
                    if self.capabilities.contains_key(&c.name.value) {
                        self.push(
                            "JC0102",
                            format!("capability `{}` is declared twice", c.name.value),
                            c.name.span,
                        );
                    } else {
                        self.capabilities.insert(c.name.value.clone(), c.name.span);
                    }
                    if !is_snake(&c.name.value) {
                        self.push(
                            "JC0101",
                            "a capability name is `[a-z][a-z0-9_]*`",
                            c.name.span,
                        );
                    }
                    self.collect(&c.items, Some(&c.name.value), owned);
                    continue;
                }
                Item::Health(h) => {
                    if self.health.is_some() {
                        self.push("JC0102", "`health` is declared twice", h.span);
                    }
                    self.health = Some(h);
                    continue;
                }
            };
            if self.symbols.contains_key(&name.value) {
                self.push(
                    "JC0102",
                    format!("`{}` is declared twice", name.value),
                    span,
                );
                continue;
            }
            self.symbols.insert(
                name.value.clone(),
                Sym {
                    kind,
                    capability: cap_owned.clone(),
                    span,
                    ty: None,
                    unit: None,
                    unit_free: false,
                    nested: None,
                },
            );
            owned.push((own, cap_owned.clone()));
        }
    }

    fn names(&mut self) {
        let mut found = Vec::new();
        for (name, sym) in &self.symbols {
            found.push((name.clone(), sym.span));
        }
        for (name, (m, _)) in &self.methods {
            found.push((name.clone(), m.name.span));
            if let Some(sym) = self.symbols.get(name)
                && matches!(sym.kind, Kind::Query | Kind::Event | Kind::Action)
            {
                self.diags.push(diag(
                    "JC0102",
                    format!("method `{name}` has the name of a query, event or action"),
                    m.name.span,
                ));
            }
        }
        for (name, span) in found {
            if !is_snake(&name) {
                self.push(
                    "JC0101",
                    "a name is `[a-z][a-z0-9_]*`, at most 32 characters",
                    span,
                );
            }
            if RESERVED.contains(&name.as_str()) {
                self.push(
                    "JC0207",
                    format!("`{name}` is reserved for the generated handles"),
                    span,
                );
            }
        }
    }

    fn doc_item(&mut self, key: String, doc: Option<&String>, annotations: &[Annotation]) {
        let annotations: Vec<(String, Vec<String>)> = annotations
            .iter()
            .filter(|a| is_doc_annotation(&a.name.value))
            .map(|a| {
                (
                    a.name.value.clone(),
                    a.args.iter().map(|x| x.value.clone()).collect(),
                )
            })
            .collect();
        if doc.is_none() && annotations.is_empty() {
            return;
        }
        self.docs.items.insert(
            key,
            ItemDocs {
                doc: doc.cloned(),
                annotations,
            },
        );
    }

    fn class_docs(&mut self) {
        let class = self.class;
        for a in &class.annotations {
            let ok = match a.name.value.as_str() {
                "aligns" => one_string(a),
                "deprecated" => a.args.len() <= 1,
                name => name.contains('.') || self.unknown(a),
            };
            if !ok {
                self.bad_annotation(a);
            }
        }
        self.doc_item("class".into(), class.doc.as_ref(), &class.annotations);
    }

    fn unknown(&mut self, a: &Annotation) -> bool {
        self.push(
            "JC0105",
            format!("unknown annotation `@{}` here", a.name.value),
            a.name.span,
        );
        true
    }

    fn bad_annotation(&mut self, a: &Annotation) {
        self.push(
            "JC0105",
            format!("invalid arguments for `@{}`", a.name.value),
            a.span,
        );
    }

    fn common_annotation(&mut self, a: &Annotation) -> Option<bool> {
        match a.name.value.as_str() {
            "renamed_from" => Some(one_string(a)),
            "deprecated" => Some(a.args.len() <= 1),
            name if name.contains('.') => Some(true),
            _ => None,
        }
    }

    fn renamed_from(annotations: &[Annotation]) -> Option<String> {
        annotations
            .iter()
            .find(|a| a.name.value == "renamed_from")
            .and_then(|a| a.args.first())
            .map(|s| s.value.trim_matches('"').to_owned())
    }

    fn member(&mut self, m: &'a Member, cap: Option<String>) -> Option<Scalar> {
        let ty = match self.universe.resolve_type(self.file, &m.ty.value) {
            Ok(ty) => ty,
            Err(e) => {
                let code = if e.starts_with("unknown") {
                    "JC0103"
                } else {
                    "JC0106"
                };
                self.push(code, e, m.ty.span);
                return None;
            }
        };
        if !ty.is_scalar() {
            self.push(
                "JC0106",
                format!(
                    "`{}` members are scalars or enums (docs/17 §3.3)",
                    m.direction.keyword()
                ),
                m.ty.span,
            );
            return None;
        }
        let unit = unit_of(m.unit.as_ref(), self.diags);
        let unit_free = ty.is_float() && m.unit.is_none();
        if unit_free && m.direction == Direction::Out {
            self.push(
                "JC0107",
                format!(
                    "`out {}` is a physical quantity without a unit; write `1` if it is dimensionless",
                    m.name.value
                ),
                m.ty.span,
            );
        }
        let mut optional = false;
        let mut safe_hint = None;
        let mut range_hint = None;
        for a in &m.annotations {
            let ok = match a.name.value.as_str() {
                "optional" => {
                    optional = true;
                    a.args.is_empty()
                }
                "safe_hint" if m.direction == Direction::Out => match a.args.as_slice() {
                    [v] => {
                        safe_hint = self.literal(v, &ty, unit.as_ref());
                        true
                    }
                    _ => false,
                },
                "range_hint" if ty.is_numeric() => match a.args.as_slice() {
                    [lo, hi] => {
                        let lo = self.literal(lo, &ty, unit.as_ref());
                        let hi = self.literal(hi, &ty, unit.as_ref());
                        range_hint = lo.zip(hi);
                        true
                    }
                    _ => false,
                },
                _ => match self.common_annotation(a) {
                    Some(ok) => ok,
                    None => self.unknown(a),
                },
            };
            if !ok {
                self.bad_annotation(a);
            }
        }
        let default = m
            .default
            .as_ref()
            .and_then(|d| self.literal(d, &ty, unit.as_ref()));
        if let Some(sym) = self.symbols.get_mut(&m.name.value) {
            sym.ty = Some(ty.clone());
            sym.unit = unit;
            sym.unit_free = unit_free;
        }
        self.used.insert(ty.clone());
        self.doc_item(
            format!("{}:{}", m.direction.keyword(), m.name.value),
            m.doc.as_ref(),
            &m.annotations,
        );
        Some(Scalar {
            name: m.name.value.clone(),
            direction: m.direction,
            ty,
            unit: m.unit.as_ref().map(|u| u.value.clone()),
            capability: cap,
            optional,
            default,
            safe_hint,
            range_hint,
            renamed_from: Self::renamed_from(&m.annotations),
        })
    }

    fn literal(&mut self, v: &Spanned<String>, ty: &Ty, unit: Option<&Unit>) -> Option<String> {
        let text = v.value.trim();
        let bad = |this: &mut Self, why: &str| {
            this.push(
                "JC0106",
                format!("`{text}` is not a `{}` literal: {why}", ty.render()),
                v.span,
            );
            None
        };
        match ty {
            Ty::Bool => match text {
                "true" | "false" => Some(text.to_owned()),
                _ => bad(self, "expected `true` or `false`"),
            },
            Ty::String => {
                if text.len() >= 2 && text.starts_with('"') && text.ends_with('"') {
                    Some(text.to_owned())
                } else {
                    bad(self, "expected a quoted string")
                }
            }
            Ty::Enum(name) => {
                let variant = text.rsplit('.').next().unwrap_or(text);
                let known = self
                    .universe
                    .enum_def(name)
                    .is_some_and(|(e, _)| e.variants.iter().any(|(n, _)| n == variant));
                if known {
                    Some(variant.to_owned())
                } else {
                    bad(self, "unknown variant")
                }
            }
            Ty::F64 | Ty::F32 | Ty::I64 | Ty::U64 | Ty::Bits(_) => {
                let (value, lit_unit) = match Literal::parse(text) {
                    Ok(Literal::Number(n)) => (n, None),
                    Ok(Literal::Quantity(q)) => (q.value, Some(q.unit)),
                    Err(e) => return bad(self, e.kind.message()),
                };
                match (unit, lit_unit) {
                    (Some(u), Some(l)) => {
                        if l.check_compatible(u).is_err() {
                            self.push(
                                "JC0301",
                                format!("`{text}` has another dimension than `{}`", u.symbol()),
                                v.span,
                            );
                            return None;
                        }
                    }
                    (Some(u), None) if !u.is_dimensionless() => {
                        return bad(self, "the member has a unit");
                    }
                    (None, Some(l)) if !l.is_dimensionless() => {
                        return bad(self, "the member has no unit");
                    }
                    _ => {}
                }
                let integral = matches!(ty, Ty::I64 | Ty::U64 | Ty::Bits(_));
                if integral && (value.fract() != 0.0 || (value < 0.0 && *ty != Ty::I64)) {
                    return bad(self, "expected an integer in range");
                }
                canonical(text).ok().or_else(|| bad(self, "malformed"))
            }
            _ => bad(self, "this type has no literals"),
        }
    }

    fn nested(&mut self, n: &'a Nested, cap: Option<String>) -> Option<ClassMember> {
        let key = self.universe.resolve_class(self.file, &n.class.value);
        let reference = format!("{}/{}@{}", key.0, key.1, key.2);
        let Some(sig) = self.universe.signature(&key) else {
            self.push(
                "J0847",
                format!("class `{reference}` is neither in the registry nor in the project"),
                n.class.span,
            );
            return None;
        };
        let (min, max) = match &n.range {
            Some(r) => {
                let (min, max) = r.value;
                if min > max || max > MAX_COLLECTION || max == 0 {
                    self.push(
                        "JC0106",
                        format!(
                            "collection bounds are `min..max` with 0 ≤ min ≤ max ≤ {MAX_COLLECTION} and max ≥ 1"
                        ),
                        r.span,
                    );
                    return None;
                }
                (min, max)
            }
            None => (1, 1),
        };
        let mut optional = false;
        for a in &n.annotations {
            let ok = match a.name.value.as_str() {
                "optional" => {
                    optional = true;
                    a.args.is_empty()
                }
                "requires" => {
                    for c in &a.args {
                        if !sig.capabilities.contains(&c.value) {
                            self.push(
                                "JC0504",
                                format!(
                                    "`@requires({})`: `{reference}` has no capability `{}`",
                                    c.value, c.value
                                ),
                                c.span,
                            );
                        }
                    }
                    !a.args.is_empty()
                }
                _ => match self.common_annotation(a) {
                    Some(ok) => ok,
                    None => self.unknown(a),
                },
            };
            if !ok {
                self.bad_annotation(a);
            }
        }
        if let Some(sym) = self.symbols.get_mut(&n.name.value) {
            sym.nested = Some(key.clone());
        }
        self.doc_item(
            format!("member:{}", n.name.value),
            n.doc.as_ref(),
            &n.annotations,
        );
        Some(ClassMember {
            name: n.name.value.clone(),
            class: reference,
            collection: n.range.is_some(),
            min,
            max,
            optional,
            requires: requires_of(&n.annotations),
            capability: cap,
            renamed_from: Self::renamed_from(&n.annotations),
        })
    }

    fn event(&mut self, e: &'a crate::ast::Event, cap: Option<String>) -> Option<EventDef> {
        for a in &e.annotations {
            match self.common_annotation(a) {
                Some(true) => {}
                Some(false) => self.bad_annotation(a),
                None => {
                    self.unknown(a);
                }
            }
        }
        self.doc_item(
            format!("event:{}", e.name.value),
            e.doc.as_ref(),
            &e.annotations,
        );
        let (record, fields) = match &e.body {
            EventBody::Fields(fields) => {
                let mut out = Vec::new();
                let mut seen = BTreeSet::new();
                for f in fields {
                    if !seen.insert(f.name.value.clone()) {
                        self.push(
                            "JC0102",
                            format!("field `{}` is declared twice", f.name.value),
                            f.name.span,
                        );
                    }
                    out.push(check_field(self.universe, self.file, f, false, self.diags)?);
                }
                (None, out)
            }
            EventBody::Record(r) => match self.universe.resolve_named(self.file, &r.value) {
                Some(Ty::Record(full)) => {
                    let fields = self.record_params(&full)?;
                    self.used.insert(Ty::Record(full.clone()));
                    (Some(full), fields)
                }
                _ => {
                    self.push("JC0103", format!("unknown record `{}`", r.value), r.span);
                    return None;
                }
            },
        };
        for f in &fields {
            self.used.insert(f.ty.clone());
        }
        Some(EventDef {
            name: e.name.value.clone(),
            capability: cap,
            record,
            fields,
            renamed_from: Self::renamed_from(&e.annotations),
        })
    }

    fn record_params(&mut self, full: &str) -> Option<Vec<Param>> {
        let (file, fields) = self.universe.record_fields(full)?;
        let mut sink = Diagnostics::default();
        fields
            .iter()
            .map(|f| check_field(self.universe, &file.ast, f, false, &mut sink))
            .collect()
    }

    fn build_env(&mut self) {
        let mut id = 0u32;
        let mut entries = Vec::new();
        for (name, sym) in &self.symbols {
            if let (Kind::Scalar(_) | Kind::Query, Some(ty)) = (sym.kind, &sym.ty)
                && let Some(st) = signal_type(self.universe, ty)
            {
                entries.push((name.clone(), st, sym.unit));
            }
            if let (Kind::Nested { .. }, Some(key)) = (sym.kind, &sym.nested) {
                self.nested_paths(name, key, 2, &mut entries);
            }
        }
        for (name, st, unit) in entries {
            self.env.signal(&name, SignalInfo { id, ty: st, unit });
            id += 1;
        }
        let package = self.file.package.value.clone();
        let mut visible: Vec<(String, String)> = self
            .universe
            .enums
            .keys()
            .filter(|(p, _)| *p == package)
            .map(|(p, n)| (n.clone(), format!("{p}/{n}")))
            .collect();
        for u in &self.file.uses {
            if u.major.is_none() {
                visible.push((
                    u.local_name().to_owned(),
                    format!("{}/{}", u.package, u.name),
                ));
            }
        }
        for (local, full) in visible {
            if let Some((def, eid)) = self.universe.enum_def(&full) {
                let variants = def.variants.into_iter().collect();
                self.env.enumeration(&local, EnumId(eid), variants);
            }
        }
    }

    fn nested_paths(
        &self,
        prefix: &str,
        key: &ClassKey,
        depth: usize,
        out: &mut Vec<(String, SignalType, Option<Unit>)>,
    ) {
        if depth > MAX_DEPTH {
            return;
        }
        let Some(sig) = self.universe.signature(key) else {
            return;
        };
        for (name, s) in sig.scalars.iter().chain(sig.queries.iter()) {
            if let Some(st) = s.ty.as_ref().and_then(|t| signal_type(self.universe, t)) {
                out.push((format!("{prefix}.{name}"), st, s.unit));
            }
        }
        for (name, n) in &sig.nested {
            self.nested_paths(&format!("{prefix}.{name}"), &n.key, depth + 1, out);
        }
    }

    fn expression(
        &mut self,
        expr: &Spanned<String>,
        method: Option<&ClassEnv>,
    ) -> Option<(joker_expr::Checked, Vec<Read>)> {
        let env = method.unwrap_or(&self.env);
        let options = CheckOptions {
            in_class_method: method.is_some(),
        };
        match joker_expr::check(&expr.value, env, options) {
            Ok(checked) => {
                let r = reads(checked.ast(), &expr.value);
                Some((checked, r))
            }
            Err(e) => {
                self.expr_error(expr, &e);
                None
            }
        }
    }

    fn expr_error(&mut self, expr: &Spanned<String>, e: &joker_expr::Diagnostic) {
        let code = match e.code() {
            "J0301" | "J0108" => "JC0301",
            "J0305" => "JC0320",
            "J0158" | "J0153" | "J0154" | "J0149" => "JC0101",
            "J0302" | "J0303" => "JC0103",
            _ => "JC0106",
        };
        let start = expr.span.start + e.span.start as usize;
        let end = (expr.span.start + e.span.end as usize).clamp(start, expr.span.end.max(start));
        self.push(
            code,
            e.kind.to_string(),
            Span::new(expr.span.file, start, end),
        );
    }

    fn sub_span(expr: &Spanned<String>, range: &std::ops::Range<usize>) -> Span {
        Span::new(
            expr.span.file,
            expr.span.start + range.start,
            expr.span.start + range.end,
        )
    }

    fn scope(&mut self, expr: &Spanned<String>, reads: &[Read], cap: Option<&str>) {
        for r in reads {
            let head = r.path.split('.').next().unwrap_or("");
            let Some(sym) = self.symbols.get(head).cloned() else {
                continue;
            };
            let span = Self::sub_span(expr, &r.range);
            if let Some(c) = &sym.capability
                && Some(c.as_str()) != cap
            {
                let title = format!("`{head}` belongs to capability `{c}`: use it inside it");
                self.push("JC0210", title, span);
            }
            if sym.unit_free && r.path == head {
                let title = format!(
                    "`{head}` has no unit in the class (the instance gives it): an expression cannot read it"
                );
                self.push("JC0107", title, span);
            }
            if let Kind::Nested { collection: true } = sym.kind
                && !r.aggregated
            {
                let title = format!(
                    "`{head}` is a collection: read it only through an aggregation (any, all, count, sum, maxs, mins)"
                );
                self.push("JC0106", title, span);
            }
        }
    }

    fn expect_type(
        &mut self,
        expr: &Spanned<String>,
        checked: &joker_expr::Checked,
        ty: &Ty,
        unit: Option<&Unit>,
    ) {
        let result = match ty {
            Ty::Bool => checked.expect_bool().map(|_| ()),
            Ty::F64 | Ty::F32 | Ty::I64 | Ty::U64 | Ty::Bits(_) => {
                checked.expect_unit(unit.unwrap_or(&Unit::ONE)).map(|_| ())
            }
            Ty::Enum(name) => {
                let id = self.universe.enum_def(name).map(|(_, id)| id);
                match checked.result_type() {
                    joker_expr::Ty::Enum(found) if Some(found.0) == id => Ok(()),
                    found => {
                        let title = format!("expected `{}`, found {found}", ty.render());
                        self.push("JC0106", title, expr.span);
                        return;
                    }
                }
            }
            _ => {
                let title = format!("an expression cannot produce `{}`", ty.render());
                self.push("JC0106", title, expr.span);
                return;
            }
        };
        if let Err(e) = result {
            self.expr_error(expr, &e);
        }
    }

    fn queries(&mut self, queries: &[(&'a Query, Option<String>)]) -> Vec<QueryDef> {
        let mut out = Vec::new();
        let mut deps: BTreeMap<String, (Vec<String>, Span)> = BTreeMap::new();
        let mut typed = Vec::new();
        for (q, cap) in queries {
            for a in &q.annotations {
                match self.common_annotation(a) {
                    Some(true) => {}
                    Some(false) => self.bad_annotation(a),
                    None => {
                        self.unknown(a);
                    }
                }
            }
            self.doc_item(
                format!("query:{}", q.name.value),
                q.doc.as_ref(),
                &q.annotations,
            );
            let ty = match self.universe.resolve_type(self.file, &q.ty.value) {
                Ok(ty) if ty.is_scalar() => ty,
                Ok(ty) => {
                    let title = format!("a query is a scalar or an enum, not `{}`", ty.render());
                    self.push("JC0106", title, q.ty.span);
                    continue;
                }
                Err(e) => {
                    self.push("JC0103", e, q.ty.span);
                    continue;
                }
            };
            let unit = unit_of(q.unit.as_ref(), self.diags);
            if ty.is_float() && q.unit.is_none() {
                let title = format!(
                    "query `{}` is a physical quantity without a unit; write `1` if it is dimensionless",
                    q.name.value
                );
                self.push("JC0107", title, q.ty.span);
            }
            if let Some(sym) = self.symbols.get_mut(&q.name.value) {
                sym.ty = Some(ty.clone());
                sym.unit = unit;
            }
            self.used.insert(ty.clone());
            typed.push((*q, cap.clone(), ty, unit));
        }
        self.build_env();
        for (q, cap, ty, unit) in typed {
            let Some((checked, reads)) = self.expression(&q.expr, None) else {
                continue;
            };
            self.expect_type(&q.expr, &checked, &ty, unit.as_ref());
            self.scope(&q.expr, &reads, cap.as_deref());
            let used: Vec<String> = reads
                .iter()
                .map(|r| r.path.clone())
                .filter(|p| self.symbols.get(p).is_some_and(|s| s.kind == Kind::Query))
                .collect();
            deps.insert(q.name.value.clone(), (used, q.name.span));
            out.push(QueryDef {
                name: q.name.value.clone(),
                ty,
                unit: q.unit.as_ref().map(|u| u.value.clone()),
                expr: canonical(&q.expr.value).unwrap_or_else(|_| q.expr.value.clone()),
                capability: cap.clone(),
                renamed_from: Self::renamed_from(&q.annotations),
            });
        }
        for (name, (_, span)) in &deps {
            if reaches(&deps, name, name, &mut BTreeSet::new()) {
                self.diags.push(diag(
                    "JC0310",
                    format!("query `{name}` depends on itself"),
                    *span,
                ));
            }
        }
        out
    }

    fn params(&mut self, params: &'a [Field], scalar_only: bool) -> Option<Vec<Param>> {
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        let mut ok = true;
        for p in params {
            if !seen.insert(p.name.value.clone()) {
                let title = format!("argument `{}` is declared twice", p.name.value);
                self.push("JC0102", title, p.name.span);
                ok = false;
            }
            if self.symbols.contains_key(&p.name.value) {
                let title = format!(
                    "argument `{}` has the name of a member: rename it",
                    p.name.value
                );
                self.push("JC0206", title, p.name.span);
                ok = false;
            }
            match check_field(self.universe, self.file, p, scalar_only, self.diags) {
                Some(param) => {
                    self.used.insert(param.ty.clone());
                    out.push(param);
                }
                None => ok = false,
            }
        }
        ok.then_some(out)
    }

    fn action(&mut self, a: &'a Action, cap: Option<String>) -> Option<ActionDef> {
        let params = self.params(&a.params, false);
        let result = match self.universe.resolve_type(self.file, &a.result.value) {
            Ok(ty) => ty,
            Err(e) => {
                self.push("JC0103", e, a.result.span);
                return None;
            }
        };
        self.used.insert(result.clone());
        let mut timeout = None;
        let mut permit_hint = None;
        let mut role_hint = None;
        let mut long = false;
        for ann in &a.annotations {
            let ok = match ann.name.value.as_str() {
                "timeout" => match ann.args.as_slice() {
                    [d] => match Duration::parse(&d.value) {
                        Ok(dur) if dur > Duration::ZERO => {
                            timeout = canonical(&d.value).ok();
                            true
                        }
                        _ => false,
                    },
                    _ => false,
                },
                "permit_hint" => match ann.args.as_slice() {
                    [e] => {
                        if let Some((checked, reads)) = self.expression(e, None) {
                            self.expect_type(e, &checked, &Ty::Bool, None);
                            self.scope(e, &reads, cap.as_deref());
                            permit_hint = canonical(&e.value).ok();
                        }
                        true
                    }
                    _ => false,
                },
                "role_hint" => match ann.args.as_slice() {
                    [r] => {
                        role_hint = Some(r.value.trim_matches('"').to_owned());
                        ROLES.contains(&r.value.trim_matches('"'))
                    }
                    _ => false,
                },
                "long" => {
                    long = true;
                    ann.args.is_empty()
                }
                _ => match self.common_annotation(ann) {
                    Some(ok) => ok,
                    None => self.unknown(ann),
                },
            };
            if !ok {
                self.bad_annotation(ann);
            }
        }
        if timeout.is_none() {
            let title = format!(
                "action `{}` has no `@timeout`: say how long it may take",
                a.name.value
            );
            self.push("JC0402", title, a.name.span);
        }
        self.doc_item(
            format!("action:{}", a.name.value),
            a.doc.as_ref(),
            &a.annotations,
        );
        Some(ActionDef {
            name: a.name.value.clone(),
            capability: cap,
            params: params?,
            result,
            timeout,
            permit_hint,
            role_hint,
            long,
            renamed_from: Self::renamed_from(&a.annotations),
        })
    }

    fn method_env(&self, params: &[Param]) -> ClassEnv {
        let mut env = ClassEnv::default();
        let mut id = 0u32;
        for (name, sym) in &self.symbols {
            if let (Kind::Scalar(_) | Kind::Query, Some(ty)) = (sym.kind, &sym.ty)
                && let Some(st) = signal_type(self.universe, ty)
            {
                env.signal(
                    name,
                    SignalInfo {
                        id,
                        ty: st,
                        unit: sym.unit,
                    },
                );
                id += 1;
            }
            if let (Kind::Nested { .. }, Some(key)) = (sym.kind, &sym.nested) {
                let mut entries = Vec::new();
                self.nested_paths(name, key, 2, &mut entries);
                for (path, st, unit) in entries {
                    env.signal(&path, SignalInfo { id, ty: st, unit });
                    id += 1;
                }
            }
        }
        for p in params {
            if let Some(st) = signal_type(self.universe, &p.ty) {
                let unit = p.unit.as_ref().and_then(|u| Unit::parse(u).ok());
                env.signal(&p.name, SignalInfo { id, ty: st, unit });
                id += 1;
            }
        }
        let package = self.file.package.value.clone();
        for (p, n) in self.universe.enums.keys() {
            if *p == package
                && let Some((def, eid)) = self.universe.enum_def(&format!("{p}/{n}"))
            {
                env.enumeration(n, EnumId(eid), def.variants.into_iter().collect());
            }
        }
        for u in &self.file.uses {
            if u.major.is_none()
                && let Some((def, eid)) =
                    self.universe.enum_def(&format!("{}/{}", u.package, u.name))
            {
                env.enumeration(
                    u.local_name(),
                    EnumId(eid),
                    def.variants.into_iter().collect(),
                );
            }
        }
        env
    }

    fn method(&mut self, m: &'a Method, cap: Option<String>) -> Option<MethodDef> {
        for a in &m.annotations {
            match self.common_annotation(a) {
                Some(true) => {}
                Some(false) => self.bad_annotation(a),
                None => {
                    self.unknown(a);
                }
            }
        }
        self.doc_item(
            format!("method:{}", m.name.value),
            m.doc.as_ref(),
            &m.annotations,
        );
        let params = self.params(&m.params, true)?;
        let env = self.method_env(&params);
        let mut body = Vec::new();
        let mut writes: Vec<(String, Span)> = Vec::new();
        let mut assigned: Vec<String> = Vec::new();
        let mut called: Vec<String> = Vec::new();
        let mut warned = false;
        let mut ok = true;
        for stmt in &m.body {
            match stmt {
                Stmt::Assign { target, expr } => {
                    let Some(ty) = self.assign_target(target, cap.as_deref()) else {
                        ok = false;
                        continue;
                    };
                    let unit = self.symbols.get(&target.value).and_then(|s| s.unit);
                    let Some((checked, reads)) = self.expression(expr, Some(&env)) else {
                        ok = false;
                        continue;
                    };
                    self.expect_type(expr, &checked, &ty, unit.as_ref());
                    self.scope(expr, &reads, cap.as_deref());
                    self.method_reads(expr, &reads, &mut warned, m);
                    writes.push((target.value.clone(), target.span));
                    assigned.push(target.value.clone());
                    body.push(Statement::Assign {
                        target: target.value.clone(),
                        expr: canonical(&expr.value).unwrap_or_else(|_| expr.value.clone()),
                    });
                }
                Stmt::Call {
                    member,
                    method,
                    args,
                    span,
                } => {
                    let Some(member) = member else {
                        let code = if self
                            .symbols
                            .get(&method.value)
                            .is_some_and(|s| s.kind == Kind::Action)
                        {
                            "JC0401"
                        } else {
                            "JC0503"
                        };
                        let title = format!(
                            "a method calls only methods of fixed class members (`member.method()`), not `{}()`",
                            method.value
                        );
                        self.push(code, title, *span);
                        ok = false;
                        continue;
                    };
                    match self.call(member, method, args, &env, cap.as_deref(), m, &mut warned) {
                        Some((key, statement)) => {
                            let mut expanded = Vec::new();
                            self.universe.expanded_writes(
                                &key,
                                &method.value,
                                &format!("{}.", member.value),
                                2,
                                &mut expanded,
                            );
                            writes.extend(expanded.into_iter().map(|w| (w, *span)));
                            called.push(member.value.clone());
                            body.push(statement);
                        }
                        None => ok = false,
                    }
                }
            }
        }
        let mut seen = BTreeSet::new();
        for (w, span) in &writes {
            if !seen.insert(w.clone()) {
                let title = format!(
                    "method `{}` assigns `{w}` more than once (counting the calls it makes)",
                    m.name.value
                );
                self.push("JC0201", title, *span);
                ok = false;
            }
        }
        let order = self.order(m, &assigned, &called);
        ok.then(|| MethodDef {
            name: m.name.value.clone(),
            capability: cap,
            atomic: m.atomic,
            order,
            params,
            body,
            renamed_from: Self::renamed_from(&m.annotations),
        })
    }

    fn assign_target(&mut self, target: &Spanned<String>, cap: Option<&str>) -> Option<Ty> {
        if target.value.contains('.') {
            let title = format!(
                "`{}` is a member of a class member: change it through its methods",
                target.value
            );
            self.push("JC0204", title, target.span);
            return None;
        }
        let Some(sym) = self.symbols.get(&target.value).cloned() else {
            let title = format!("unknown member `{}`", target.value);
            self.push("JC0103", title, target.span);
            return None;
        };
        if sym.kind != Kind::Scalar(Direction::Out) {
            let title = format!(
                "a method assigns only `out` members; `{}` is not one",
                target.value
            );
            self.push("JC0204", title, target.span);
            return None;
        }
        if let Some(c) = &sym.capability
            && Some(c.as_str()) != cap
        {
            let title = format!(
                "`{}` belongs to capability `{c}`: assign it from a method of that capability",
                target.value
            );
            self.push("JC0210", title, target.span);
            return None;
        }
        sym.ty
    }

    fn method_reads(
        &mut self,
        expr: &Spanned<String>,
        reads: &[Read],
        warned: &mut bool,
        m: &Method,
    ) {
        for r in reads {
            let head = r.path.split('.').next().unwrap_or("");
            if r.own {
                let out = r.path == head
                    && self
                        .symbols
                        .get(head)
                        .is_some_and(|s| s.kind == Kind::Scalar(Direction::Out));
                if !out {
                    let title = format!(
                        "`own({})`: `own` takes an `out` member of the class",
                        r.path
                    );
                    self.push("JC0106", title, Self::sub_span(expr, &r.range));
                }
                continue;
            }
            let conditioned = match self.symbols.get(head).map(|s| s.kind) {
                Some(Kind::Scalar(Direction::In) | Kind::Query) => r.path == head,
                Some(Kind::Nested { collection: false }) => self.nested_member_is_input(&r.path),
                _ => false,
            };
            if conditioned && !*warned {
                *warned = true;
                let title = format!(
                    "method `{}` is conditioned on `{}`: it looks like an interlock; check that the manifest also has it",
                    m.name.value, r.path
                );
                self.push("JW0230", title, Self::sub_span(expr, &r.range));
            }
        }
    }

    fn nested_member_is_input(&self, path: &str) -> bool {
        let mut parts = path.split('.');
        let Some(head) = parts.next() else {
            return false;
        };
        let Some(mut key) = self.symbols.get(head).and_then(|s| s.nested.clone()) else {
            return false;
        };
        let rest: Vec<&str> = parts.collect();
        for (i, part) in rest.iter().enumerate() {
            let Some(sig) = self.universe.signature(&key) else {
                return false;
            };
            if i + 1 == rest.len() {
                return sig
                    .scalars
                    .get(*part)
                    .is_some_and(|s| s.direction == Direction::In)
                    || sig.queries.contains_key(*part);
            }
            match sig.nested.get(*part) {
                Some(n) => key = n.key.clone(),
                None => return false,
            }
        }
        false
    }

    #[allow(clippy::too_many_arguments)]
    fn call(
        &mut self,
        member: &Spanned<String>,
        method: &Spanned<String>,
        args: &[Spanned<String>],
        env: &ClassEnv,
        cap: Option<&str>,
        m: &Method,
        warned: &mut bool,
    ) -> Option<(ClassKey, Statement)> {
        let Some(sym) = self.symbols.get(&member.value).cloned() else {
            let title = format!("unknown class member `{}`", member.value);
            self.push("JC0103", title, member.span);
            return None;
        };
        let (Kind::Nested { collection }, Some(key)) = (sym.kind, sym.nested.clone()) else {
            let title = format!("`{}` is not a class member", member.value);
            self.push("JC0503", title, member.span);
            return None;
        };
        if collection {
            let title = format!(
                "`{}` is a collection: a method does not call its methods (behavior over collections belongs in an app)",
                member.value
            );
            self.push("JC0502", title, member.span);
            return None;
        }
        if let Some(c) = &sym.capability
            && Some(c.as_str()) != cap
        {
            let title = format!(
                "`{}` belongs to capability `{c}`: use it inside it",
                member.value
            );
            self.push("JC0210", title, member.span);
            return None;
        }
        let sig = self.universe.signature(&key)?;
        if sig.actions.contains_key(&method.value) {
            let title = format!(
                "`{}.{}` is an action: methods do not call actions",
                member.value, method.value
            );
            self.push("JC0401", title, method.span);
            return None;
        }
        let Some(target) = sig.methods.get(&method.value) else {
            let title = format!(
                "`{}/{}@{}` has no method `{}`",
                key.0, key.1, key.2, method.value
            );
            self.push("JC0503", title, method.span);
            return None;
        };
        if let Some(c) = &target.capability {
            let requires = self
                .class
                .items
                .iter()
                .chain(self.class.items.iter().flat_map(|i| match i {
                    Item::Capability(c) => c.items.as_slice(),
                    _ => &[],
                }))
                .find_map(|i| match i {
                    Item::Nested(n) if n.name.value == member.value => {
                        Some(requires_of(&n.annotations))
                    }
                    _ => None,
                })
                .unwrap_or_default();
            if !requires.contains(c) {
                let title = format!(
                    "`{}.{}` is of capability `{c}`: add `@requires({c})` to `member {}`",
                    member.value, method.value, member.value
                );
                self.push("JC0503", title, method.span);
                return None;
            }
        }
        if target.params.len() != args.len() {
            let title = format!(
                "`{}.{}` takes {} argument(s), not {}",
                member.value,
                method.value,
                target.params.len(),
                args.len()
            );
            self.push("JC0106", title, method.span);
            return None;
        }
        let mut canonical_args = Vec::new();
        for (arg, (ty, unit)) in args.iter().zip(target.params.clone()) {
            let (checked, reads) = self.expression(arg, Some(env))?;
            if let Some(ty) = ty {
                self.expect_type(arg, &checked, &ty, unit.as_ref());
            }
            self.scope(arg, &reads, cap);
            self.method_reads(arg, &reads, warned, m);
            canonical_args.push(canonical(&arg.value).unwrap_or_else(|_| arg.value.clone()));
        }
        Some((
            key,
            Statement::Call {
                member: member.value.clone(),
                method: method.value.clone(),
                args: canonical_args,
            },
        ))
    }

    fn order(&mut self, m: &Method, assigned: &[String], called: &[String]) -> Vec<String> {
        let Some(order) = &m.order else {
            if m.atomic {
                let title = format!(
                    "atomic method `{}` without `order(…)`: say in which order the writes apply",
                    m.name.value
                );
                self.push("JC0205", title, m.name.span);
            }
            return Vec::new();
        };
        if !m.atomic {
            self.push("JC0205", "`order` is only for `atomic` methods", order.span);
        }
        let mut names = Vec::new();
        for n in &order.value {
            let known = assigned.contains(&n.value) || called.contains(&n.value);
            if !known {
                let title = format!(
                    "`order` names `{}`, which the method neither assigns nor calls",
                    n.value
                );
                self.push("JC0205", title, n.span);
            }
            if names.contains(&n.value) {
                let title = format!("`order` names `{}` twice", n.value);
                self.push("JC0205", title, n.span);
            }
            names.push(n.value.clone());
        }
        for w in assigned.iter().chain(called.iter()) {
            if !names.contains(w) {
                let title = format!("`order` does not name `{w}`, which the method writes");
                self.push("JC0205", title, order.span);
            }
        }
        names
    }

    fn health_names(&mut self) -> Vec<String> {
        let Some(h) = self.health else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for n in &h.names {
            match self.symbols.get(&n.value).cloned() {
                None => {
                    let title = format!("unknown member `{}`", n.value);
                    self.push("JC0103", title, n.span);
                }
                Some(sym) if !matches!(sym.kind, Kind::Scalar(_)) => {
                    let title = format!(
                        "`health required` names scalar members; `{}` counts by the rules of docs/17 §4.5",
                        n.value
                    );
                    self.push("JC0106", title, n.span);
                }
                Some(sym) if sym.capability.is_some() => {
                    let title = format!(
                        "`{}` belongs to a capability: it lowers health to `Degraded` by itself",
                        n.value
                    );
                    self.push("JC0210", title, n.span);
                }
                Some(_) => out.push(n.value.clone()),
            }
        }
        out.sort();
        out.dedup();
        out
    }

    fn depth(&mut self) {
        let key = (
            self.file.package.value.clone(),
            self.class.name.value.clone(),
            self.class.version.value.major,
        );
        match self.universe.depth(&key) {
            Some(d) if d <= MAX_DEPTH => {}
            Some(d) => {
                let title = format!(
                    "composition depth {d}: at most {MAX_DEPTH} classes in a chain of class members"
                );
                self.push("JC0501", title, self.class.name.span);
            }
            None => self.push(
                "JC0501",
                "the class contains itself through its class members",
                self.class.name.span,
            ),
        }
    }

    fn used_types(&mut self, contract: &mut Contract) {
        let mut enums = BTreeSet::new();
        let mut records = BTreeSet::new();
        let mut stack: Vec<Ty> = self.used.iter().cloned().collect();
        while let Some(t) = stack.pop() {
            match t {
                Ty::Enum(name) => {
                    enums.insert(name);
                }
                Ty::Record(name) => {
                    if records.insert(name.clone())
                        && let Some(fields) = self.record_params(&name)
                    {
                        stack.extend(fields.into_iter().map(|f| f.ty));
                    }
                }
                Ty::List(inner, _) => stack.push(*inner),
                _ => {}
            }
        }
        contract.enums = enums
            .iter()
            .filter_map(|e| self.universe.enum_def(e).map(|(d, _)| d))
            .collect();
        contract.records = records
            .iter()
            .filter_map(|r| {
                Some(RecordDef {
                    name: r.clone(),
                    fields: self.record_params(r)?,
                })
            })
            .collect();
    }
}

fn reaches(
    deps: &BTreeMap<String, (Vec<String>, Span)>,
    from: &str,
    target: &str,
    seen: &mut BTreeSet<String>,
) -> bool {
    let Some((next, _)) = deps.get(from) else {
        return false;
    };
    for n in next {
        if n == target {
            return true;
        }
        if seen.insert(n.clone()) && reaches(deps, n, target, seen) {
            return true;
        }
    }
    false
}

fn one_string(a: &Annotation) -> bool {
    matches!(a.args.as_slice(), [s] if s.value.len() >= 2 && s.value.starts_with('"') && s.value.ends_with('"'))
}

fn is_doc_annotation(name: &str) -> bool {
    matches!(name, "aligns" | "deprecated") || name.contains('.') || !is_joker_annotation(name)
}

fn is_joker_annotation(name: &str) -> bool {
    matches!(
        name,
        "safe_hint"
            | "range_hint"
            | "permit_hint"
            | "role_hint"
            | "timeout"
            | "optional"
            | "requires"
            | "long"
            | "aligns"
            | "renamed_from"
            | "deprecated"
    )
}
