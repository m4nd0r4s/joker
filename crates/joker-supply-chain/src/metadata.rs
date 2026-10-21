//! The part of `cargo metadata --format-version 1` the checks read.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::Error;

/// The kind of a declared dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DepKind {
    /// `[dependencies]`: compiled into the crate.
    Normal,
    /// `[build-dependencies]`: compiled for the build script, on the host.
    Build,
    /// `[dev-dependencies]`: tests, examples and benches only.
    Dev,
}

/// A dependency as the manifest of a package declares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    /// The name of the package depended on.
    pub name: String,
    /// The name it is imported under, when renamed.
    pub rename: Option<String>,
    /// Its kind.
    pub kind: DepKind,
    /// Whether only a feature turns it on.
    pub optional: bool,
    /// The `target` it is restricted to (`cfg(…)` or a triple).
    pub target: Option<String>,
    /// The features of the dependency it turns on.
    pub features: Vec<String>,
    /// Whether it keeps the default features of the dependency.
    pub default_features: bool,
    /// Whether it is a path dependency (a workspace crate).
    pub is_path: bool,
}

impl Dependency {
    /// The name the features of the depending package use for it.
    pub fn key(&self) -> &str {
        self.rename.as_deref().unwrap_or(&self.name)
    }
}

/// A package of the graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// The package id of `cargo metadata`.
    pub id: String,
    /// The package name.
    pub name: String,
    /// The version.
    pub version: String,
    /// The SPDX licence expression, if the manifest has one.
    pub license: Option<String>,
    /// The `rust-version`, if any.
    pub rust_version: Option<String>,
    /// The features and what each turns on.
    pub features: BTreeMap<String, Vec<String>>,
    /// The declared dependencies.
    pub dependencies: Vec<Dependency>,
    /// Whether one of its targets is a procedural macro.
    pub proc_macro: bool,
}

/// One resolved edge: the package an edge leads to and the kinds it has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    /// The package id the edge leads to.
    pub to: String,
    /// The kinds and targets of the edge.
    pub kinds: Vec<(DepKind, Option<String>)>,
}

/// The workspace graph: its packages, its members and the resolved edges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    packages: BTreeMap<String, Package>,
    members: BTreeSet<String>,
    edges: BTreeMap<String, Vec<Edge>>,
}

impl Workspace {
    /// Reads the JSON output of `cargo metadata --format-version 1`.
    pub fn from_json(text: &str) -> Result<Self, Error> {
        let value: Value = serde_json::from_str(text)
            .map_err(|e| Error::new(format!("cargo metadata is not JSON: {e}")))?;
        Self::from_value(&value)
    }

    /// Reads the parsed output of `cargo metadata --format-version 1`.
    pub fn from_value(value: &Value) -> Result<Self, Error> {
        let mut packages = BTreeMap::new();
        for p in array(value, "packages")? {
            let package = package(p)?;
            packages.insert(package.id.clone(), package);
        }
        let members = array(value, "workspace_members")?
            .iter()
            .map(|m| m.as_str().map(str::to_owned))
            .collect::<Option<BTreeSet<_>>>()
            .ok_or_else(|| Error::new("workspace_members holds a non-string"))?;
        let mut edges = BTreeMap::new();
        if let Some(resolve) = value.get("resolve").filter(|r| !r.is_null()) {
            for node in array(resolve, "nodes")? {
                let id = string(node, "id")?;
                let mut out = Vec::new();
                for dep in array(node, "deps")? {
                    let mut kinds = Vec::new();
                    for k in array(dep, "dep_kinds")? {
                        kinds.push((kind(k.get("kind"))?, optional_string(k, "target")));
                    }
                    out.push(Edge {
                        to: string(dep, "pkg")?,
                        kinds,
                    });
                }
                edges.insert(id, out);
            }
        }
        Ok(Self {
            packages,
            members,
            edges,
        })
    }

    /// The members of the workspace, in name order.
    pub fn members(&self) -> Vec<&Package> {
        let mut members: Vec<&Package> = self
            .members
            .iter()
            .filter_map(|id| self.packages.get(id))
            .collect();
        members.sort_by(|a, b| a.name.cmp(&b.name));
        members
    }

    /// The member with this name.
    pub fn member(&self, name: &str) -> Option<&Package> {
        self.members().into_iter().find(|p| p.name == name)
    }

    /// The package with this id.
    pub fn package(&self, id: &str) -> Option<&Package> {
        self.packages.get(id)
    }

    /// Whether this id is a member of the workspace.
    pub fn is_member(&self, id: &str) -> bool {
        self.members.contains(id)
    }

    /// The resolved edges out of a package.
    pub fn edges(&self, id: &str) -> &[Edge] {
        self.edges.get(id).map(Vec::as_slice).unwrap_or(&[])
    }

    /// The package a declared dependency of `from` resolved to, if the
    /// resolved graph has it.
    pub fn resolved(&self, from: &Package, dep: &Dependency) -> Option<&Package> {
        self.edges(&from.id)
            .iter()
            .filter(|e| e.kinds.iter().any(|(k, _)| *k == dep.kind))
            .filter_map(|e| self.packages.get(&e.to))
            .find(|p| p.name == dep.name)
    }
}

fn package(p: &Value) -> Result<Package, Error> {
    let mut features = BTreeMap::new();
    if let Some(map) = p.get("features").and_then(Value::as_object) {
        for (name, entries) in map {
            let entries = entries
                .as_array()
                .ok_or_else(|| Error::new(format!("feature {name} is not a list")))?
                .iter()
                .filter_map(|e| e.as_str().map(str::to_owned))
                .collect();
            features.insert(name.clone(), entries);
        }
    }
    let mut dependencies = Vec::new();
    for d in array(p, "dependencies")? {
        dependencies.push(Dependency {
            name: string(d, "name")?,
            rename: optional_string(d, "rename"),
            kind: kind(d.get("kind"))?,
            optional: d.get("optional").and_then(Value::as_bool).unwrap_or(false),
            target: optional_string(d, "target"),
            features: d
                .get("features")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|f| f.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
            default_features: d
                .get("uses_default_features")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            is_path: d.get("path").is_some_and(|v| !v.is_null()),
        });
    }
    let proc_macro = p
        .get("targets")
        .and_then(Value::as_array)
        .is_some_and(|targets| {
            targets.iter().any(|t| {
                t.get("kind")
                    .and_then(Value::as_array)
                    .is_some_and(|k| k.iter().any(|k| k.as_str() == Some("proc-macro")))
            })
        });
    Ok(Package {
        id: string(p, "id")?,
        name: string(p, "name")?,
        version: string(p, "version")?,
        license: optional_string(p, "license"),
        rust_version: optional_string(p, "rust_version"),
        features,
        dependencies,
        proc_macro,
    })
}

fn kind(value: Option<&Value>) -> Result<DepKind, Error> {
    match value.and_then(Value::as_str) {
        None => Ok(DepKind::Normal),
        Some("build") => Ok(DepKind::Build),
        Some("dev") => Ok(DepKind::Dev),
        Some(other) => Err(Error::new(format!("unknown dependency kind {other}"))),
    }
}

fn array<'a>(value: &'a Value, key: &str) -> Result<&'a Vec<Value>, Error> {
    value
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| Error::new(format!("cargo metadata has no list {key}")))
}

fn string(value: &Value, key: &str) -> Result<String, Error> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| Error::new(format!("cargo metadata has no string {key}")))
}

fn optional_string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}
