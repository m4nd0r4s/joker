//! The dependency rules of document 13 §1 and of the policy §1, on the graph.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::metadata::{DepKind, Dependency, Package, Workspace};
use crate::policy::{
    CORE, CORE_ALLOWLIST, CORE_EDGES, DAEMON, MCU_LICENCES, NO_STD, NO_STD_EXTERNAL, STD_FEATURE,
    TEST_CFGS,
};
use crate::{Rule, Violation, offers_one_of};

/// Checks every dependency rule on the graph and returns what breaks them,
/// sorted; an empty list means the graph follows the policy.
pub fn check_graph(ws: &Workspace) -> Vec<Violation> {
    let mut out = Vec::new();
    core_edges(ws, &mut out);
    daemon(ws, &mut out);
    for name in NO_STD {
        if let Some(package) = ws.member(name) {
            no_std(ws, package, &mut out);
        }
    }
    mcu_licences(ws, &mut out);
    out.sort();
    out.dedup();
    out
}

fn test_only(dep: &Dependency) -> bool {
    dep.target
        .as_deref()
        .is_some_and(|t| TEST_CFGS.contains(&t))
}

fn ships(dep: &Dependency) -> bool {
    dep.kind != DepKind::Dev && !test_only(dep)
}

fn core_edges(ws: &Workspace, out: &mut Vec<Violation>) {
    for (name, allowed) in CORE_EDGES {
        let Some(package) = ws.member(name) else {
            continue;
        };
        for dep in package
            .dependencies
            .iter()
            .filter(|d| ships(d) && d.is_path)
        {
            if dep.name != package.name && !allowed.contains(&dep.name.as_str()) {
                out.push(Violation::new(
                    Rule::WorkspaceEdge,
                    &package.name,
                    format!(
                        "depends on {}; document 13 §1 allows only {:?}",
                        dep.name, allowed
                    ),
                ));
            }
        }
    }
}

fn daemon(ws: &Workspace, out: &mut Vec<Violation>) {
    for package in ws.members() {
        if package.name == DAEMON {
            continue;
        }
        for dep in package.dependencies.iter().filter(|d| d.name == DAEMON) {
            out.push(Violation::new(
                Rule::Daemon,
                &package.name,
                format!("has a {:?} dependency on {DAEMON}", dep.kind),
            ));
        }
    }
}

/// The features of a package that turn on its `std` feature, directly or
/// through other features of the same package.
fn std_features(package: &Package) -> BTreeSet<&str> {
    package
        .features
        .keys()
        .map(String::as_str)
        .filter(|f| feature_closure(package, f).contains(STD_FEATURE))
        .collect()
}

fn feature_closure<'a>(package: &'a Package, start: &'a str) -> BTreeSet<&'a str> {
    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::from([start]);
    while let Some(feature) = queue.pop_front() {
        if !seen.insert(feature) {
            continue;
        }
        for entry in package.features.get(feature).into_iter().flatten() {
            if !entry.starts_with("dep:") && !entry.contains('/') {
                queue.push_back(entry);
            }
        }
    }
    seen
}

/// Whether only the package's `std` feature, or features that imply it,
/// turn this optional dependency on.
fn std_gated(package: &Package, dep: &Dependency, std: &BTreeSet<&str>) -> bool {
    if !dep.optional {
        return false;
    }
    let key = dep.key();
    let activators: Vec<&str> = package
        .features
        .iter()
        .filter(|(_, entries)| {
            entries.iter().any(|e| {
                e.strip_prefix("dep:") == Some(key)
                    || e == key
                    || e.split_once('/').is_some_and(|(d, _)| d == key)
            })
        })
        .map(|(f, _)| f.as_str())
        .collect();
    !activators.is_empty() && activators.iter().all(|f| std.contains(f))
}

fn no_std(ws: &Workspace, package: &Package, out: &mut Vec<Violation>) {
    let std = std_features(package);
    let core = CORE.contains(&package.name.as_str());
    for dep in &package.dependencies {
        if test_only(dep) || std_gated(package, dep, &std) {
            continue;
        }
        match dep.kind {
            DepKind::Dev => {}
            DepKind::Build => {
                if core && !dep.is_path && !CORE_ALLOWLIST.contains(&dep.name.as_str()) {
                    out.push(Violation::new(
                        Rule::CoreAllowlist,
                        &package.name,
                        format!("build-depends on {}, outside the core allowlist", dep.name),
                    ));
                }
            }
            DepKind::Normal => no_std_dependency(ws, package, dep, core, out),
        }
    }
    for (feature, entries) in &package.features {
        if std.contains(feature.as_str()) {
            continue;
        }
        for entry in entries {
            if let Some((dep, enabled)) = entry.split_once('/')
                && enabled == STD_FEATURE
            {
                out.push(Violation::new(
                    Rule::NoStd,
                    &package.name,
                    format!(
                        "feature {feature} turns on {}/std without turning on std",
                        dep.trim_end_matches('?')
                    ),
                ));
            }
        }
    }
}

fn no_std_dependency(
    ws: &Workspace,
    package: &Package,
    dep: &Dependency,
    core: bool,
    out: &mut Vec<Violation>,
) {
    let name = dep.name.as_str();
    if dep.is_path {
        if name != package.name && !NO_STD.contains(&name) {
            out.push(Violation::new(
                Rule::NoStd,
                &package.name,
                format!("depends on {name}, which is not a no_std crate of document 13 §1"),
            ));
        }
        return;
    }
    if !NO_STD_EXTERNAL.contains(&name) {
        out.push(Violation::new(
            Rule::NoStd,
            &package.name,
            format!("depends on {name}, which is not in the no_std list of the policy §3"),
        ));
    }
    if core && !CORE_ALLOWLIST.contains(&name) {
        out.push(Violation::new(
            Rule::CoreAllowlist,
            &package.name,
            format!("depends on {name}, outside the core allowlist of the policy §3"),
        ));
    }
    if dep.features.iter().any(|f| f == STD_FEATURE) {
        out.push(Violation::new(
            Rule::NoStd,
            &package.name,
            format!("turns on {name}/std"),
        ));
    }
    if dep.default_features
        && let Some(target) = ws.resolved(package, dep)
        && feature_closure(target, "default").contains(STD_FEATURE)
    {
        out.push(Violation::new(
            Rule::NoStd,
            &package.name,
            format!("keeps the default features of {name}, which turn on std"),
        ));
    }
}

fn mcu_licences(ws: &Workspace, out: &mut Vec<Violation>) {
    let mut reached: BTreeMap<String, String> = BTreeMap::new();
    for root in NO_STD {
        let Some(root) = ws.member(root) else {
            continue;
        };
        let mut seen = BTreeSet::new();
        let mut queue = VecDeque::from([root.id.clone()]);
        while let Some(id) = queue.pop_front() {
            if !seen.insert(id.clone()) {
                continue;
            }
            let Some(package) = ws.package(&id) else {
                continue;
            };
            if package.proc_macro {
                continue;
            }
            if ws.is_member(&id) {
                let std = std_features(package);
                for dep in &package.dependencies {
                    if dep.kind == DepKind::Normal
                        && !test_only(dep)
                        && !std_gated(package, dep, &std)
                        && let Some(next) = ws.resolved(package, dep)
                    {
                        queue.push_back(next.id.clone());
                    }
                }
            } else {
                reached
                    .entry(id.clone())
                    .or_insert_with(|| root.name.clone());
                for edge in ws.edges(&id) {
                    let ships = edge.kinds.iter().any(|(kind, target)| {
                        *kind == DepKind::Normal
                            && !target.as_deref().is_some_and(|t| TEST_CFGS.contains(&t))
                    });
                    if ships {
                        queue.push_back(edge.to.clone());
                    }
                }
            }
        }
    }
    for (id, root) in reached {
        let Some(package) = ws.package(&id) else {
            continue;
        };
        let subject = format!("{}@{}", package.name, package.version);
        match &package.license {
            None => out.push(Violation::new(
                Rule::McuLicence,
                subject,
                format!("reached from {root}, has no SPDX licence expression"),
            )),
            Some(licence) if !offers_one_of(licence, MCU_LICENCES) => {
                out.push(Violation::new(
                    Rule::McuLicence,
                    subject,
                    format!("reached from {root}, is {licence}, not MIT or Apache-2.0"),
                ));
            }
            Some(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::check_graph;
    use crate::{Rule, Workspace};

    fn id(name: &str) -> String {
        format!("path+file:///w/crates/{name}#0.0.0")
    }

    fn ext(name: &str) -> String {
        format!("registry+https://github.com/rust-lang/crates.io-index#{name}@1.0.0")
    }

    fn dep(name: &str, kind: Option<&str>, path: bool) -> Value {
        json!({
            "name": name, "rename": null, "kind": kind, "optional": false,
            "target": null, "features": [], "uses_default_features": false,
            "path": if path { json!(format!("/w/crates/{name}")) } else { Value::Null },
        })
    }

    struct Graph {
        packages: Vec<Value>,
        members: Vec<String>,
        nodes: Vec<Value>,
    }

    impl Graph {
        fn new() -> Self {
            Self {
                packages: Vec::new(),
                members: Vec::new(),
                nodes: Vec::new(),
            }
        }

        fn member(mut self, name: &str, deps: Vec<Value>, features: Value) -> Self {
            self.add(id(name), name, "Apache-2.0 OR MIT", deps, features);
            self.members.push(id(name));
            self
        }

        fn external(mut self, name: &str, licence: &str, features: Value) -> Self {
            self.add(ext(name), name, licence, Vec::new(), features);
            self
        }

        fn add(
            &mut self,
            pid: String,
            name: &str,
            licence: &str,
            deps: Vec<Value>,
            features: Value,
        ) {
            let edges: Vec<Value> = deps
                .iter()
                .map(|d| {
                    let n = d["name"].as_str().unwrap_or_default();
                    let to = if d["path"].is_null() { ext(n) } else { id(n) };
                    json!({ "name": n, "pkg": to, "dep_kinds": [{ "kind": d["kind"], "target": d["target"] }] })
                })
                .collect();
            self.nodes.push(json!({ "id": pid, "deps": edges }));
            self.packages.push(json!({
                "id": pid, "name": name, "version": "1.0.0", "license": licence,
                "rust_version": "1.99", "features": features, "dependencies": deps,
                "targets": [{ "kind": ["lib"] }],
            }));
        }

        fn build(self) -> Workspace {
            let value = json!({
                "packages": self.packages,
                "workspace_members": self.members,
                "resolve": { "nodes": self.nodes },
            });
            Workspace::from_value(&value).unwrap()
        }
    }

    fn rules(ws: &Workspace) -> Vec<(Rule, String)> {
        check_graph(ws)
            .into_iter()
            .map(|v| (v.rule, v.subject))
            .collect()
    }

    fn base() -> Graph {
        Graph::new()
            .external(
                "portable-atomic",
                "Apache-2.0 OR MIT",
                json!({ "default": ["fallback"], "fallback": [], "std": [] }),
            )
            .member("joker-units", vec![], json!({}))
            .member(
                "joker-expr",
                vec![dep("joker-units", None, true)],
                json!({}),
            )
            .member(
                "joker-core",
                vec![
                    dep("joker-units", None, true),
                    dep("joker-expr", None, true),
                    dep("portable-atomic", None, false),
                ],
                json!({}),
            )
    }

    #[test]
    fn a_graph_that_follows_the_rules_passes() {
        assert_eq!(rules(&base().build()), vec![]);
    }

    #[test]
    fn the_core_may_not_depend_on_other_workspace_crates() {
        let ws = base()
            .member("joker-hal", vec![dep("joker-core", None, true)], json!({}))
            .member("joker-units", vec![dep("joker-hal", None, true)], json!({}))
            .build();
        assert!(rules(&ws).contains(&(Rule::WorkspaceEdge, "joker-units".into())));
    }

    #[test]
    fn nothing_depends_on_the_daemon_with_any_kind() {
        let ws = base()
            .member("jokerd", vec![dep("joker-core", None, true)], json!({}))
            .member(
                "joker-api",
                vec![dep("jokerd", Some("dev"), true)],
                json!({}),
            )
            .build();
        assert_eq!(rules(&ws), vec![(Rule::Daemon, "joker-api".into())]);
    }

    #[test]
    fn a_no_std_crate_may_not_reach_a_std_crate() {
        let ws = base()
            .external("tokio", "MIT", json!({}))
            .member(
                "joker-hal",
                vec![dep("joker-core", None, true), dep("tokio", None, false)],
                json!({}),
            )
            .build();
        assert_eq!(rules(&ws), vec![(Rule::NoStd, "joker-hal".into())]);
    }

    #[test]
    fn a_dependency_behind_the_std_feature_is_allowed() {
        let mut tokio = dep("tokio", None, false);
        tokio["optional"] = json!(true);
        let ws = base()
            .external("tokio", "MIT", json!({}))
            .member(
                "joker-hal",
                vec![dep("joker-core", None, true), tokio],
                json!({ "std": ["dep:tokio"], "testing": ["std"] }),
            )
            .build();
        assert_eq!(rules(&ws), vec![]);
    }

    #[test]
    fn a_dependency_behind_another_feature_is_checked() {
        let mut tokio = dep("tokio", None, false);
        tokio["optional"] = json!(true);
        let ws = base()
            .external("tokio", "MIT", json!({}))
            .member(
                "joker-hal",
                vec![dep("joker-core", None, true), tokio],
                json!({ "std": ["dep:tokio"], "alloc": ["dep:tokio"] }),
            )
            .build();
        assert_eq!(rules(&ws), vec![(Rule::NoStd, "joker-hal".into())]);
    }

    #[test]
    fn std_of_a_dependency_may_not_be_turned_on() {
        let mut atomic = dep("portable-atomic", None, false);
        atomic["features"] = json!(["std"]);
        let ws = base()
            .member(
                "joker-hal",
                vec![dep("joker-core", None, true), atomic],
                json!({}),
            )
            .member(
                "joker-driver-sdk",
                vec![dep("portable-atomic", None, false)],
                json!({ "alloc": ["portable-atomic/std"] }),
            )
            .build();
        assert_eq!(
            rules(&ws),
            vec![
                (Rule::NoStd, "joker-driver-sdk".into()),
                (Rule::NoStd, "joker-hal".into()),
            ]
        );
    }

    #[test]
    fn default_features_that_turn_on_std_are_refused() {
        let mut heapless = dep("heapless", None, false);
        heapless["uses_default_features"] = json!(true);
        let ws = base()
            .external(
                "heapless",
                "MIT OR Apache-2.0",
                json!({ "default": ["std"], "std": [] }),
            )
            .member("joker-link-proto", vec![heapless], json!({}))
            .build();
        assert_eq!(rules(&ws), vec![(Rule::NoStd, "joker-link-proto".into())]);
    }

    #[test]
    fn the_core_only_takes_the_core_allowlist() {
        let ws = base()
            .external("embedded-io", "MIT OR Apache-2.0", json!({}))
            .member(
                "joker-agent-proto",
                vec![dep("embedded-io", None, false)],
                json!({}),
            )
            .member(
                "joker-hal",
                vec![dep("embedded-io", None, false)],
                json!({}),
            )
            .build();
        assert_eq!(
            rules(&ws),
            vec![(Rule::CoreAllowlist, "joker-agent-proto".into())]
        );
    }

    #[test]
    fn loom_and_dev_dependencies_do_not_count() {
        let mut loom = dep("loom", None, false);
        loom["target"] = json!("cfg(loom)");
        let ws = base()
            .external("loom", "MIT", json!({}))
            .external("proptest", "MIT OR Apache-2.0", json!({}))
            .member(
                "joker-core",
                vec![
                    dep("joker-units", None, true),
                    dep("proptest", Some("dev"), false),
                    loom,
                ],
                json!({}),
            )
            .build();
        assert_eq!(rules(&ws), vec![]);
    }

    #[test]
    fn licences_under_a_no_std_crate_are_mit_or_apache() {
        let ws = base()
            .external("libm", "BSD-3-Clause", json!({}))
            .member(
                "joker-expr",
                vec![dep("joker-units", None, true), dep("libm", None, false)],
                json!({}),
            )
            .build();
        assert_eq!(rules(&ws), vec![(Rule::McuLicence, "libm@1.0.0".into())]);
    }
}
