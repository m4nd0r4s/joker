//! The Rust blocks of the references name items that exist in the crates
//! that implement them (docs/13 §4, task 0a; ADR 0080).
//!
//! Most Rust blocks of the references are signature listings (`fn` without
//! a body, `{ /* … */ }`), which no compiler accepts. Each block is read
//! for what it declares: every public type, trait, function, constant and
//! module, and every public method or associated constant of an `impl` or
//! a `trait`, except those gated on an `unstable-*` feature (reserved,
//! ADR 0035). An example block declares nothing public; what it uses from
//! the SDK (`joker::<item>`) is checked instead. The crates are parsed
//! with `syn`, so an item counts when its name is declared or re-exported
//! anywhere in them.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test helpers fail the test on a broken corpus"
)]

mod common;

use std::collections::{BTreeMap, BTreeSet};

use regex::Regex;

/// The crates that implement each reference with Rust blocks.
const SURFACES: [(&str, &[&str]); 5] = [
    ("ref/01", &["crates/joker-manifest"]),
    (
        "ref/04",
        &[
            "crates/joker-sdk",
            "crates/joker-sdk-macros",
            "crates/joker-sdk-sys",
            "crates/joker-test",
        ],
    ),
    (
        "ref/05",
        &[
            "crates/joker-sdk",
            "crates/joker-sdk-macros",
            "crates/joker-sdk-sys",
            "crates/joker-client",
        ],
    ),
    (
        "ref/06",
        &[
            "crates/joker-hal",
            "crates/joker-drivers/sim",
            "crates/joker-drivers/modbus",
            "crates/joker-drivers/sysfs",
            "crates/joker-drivers/gpio",
            "crates/joker-drivers/i2c",
        ],
    ),
    ("ref/08", &["crates/joker-micro"]),
];

/// What a set of crates declares.
#[derive(Debug, Default)]
struct Index {
    names: BTreeSet<String>,
    members: BTreeSet<(String, String)>,
}

impl Index {
    fn has(&self, item: &str) -> bool {
        match item.split_once("::") {
            Some((ty, member)) => self.members.contains(&(ty.to_owned(), member.to_owned())),
            None => self.names.contains(item),
        }
    }

    fn add_use(&mut self, tree: &syn::UseTree) {
        match tree {
            syn::UseTree::Path(p) => self.add_use(&p.tree),
            syn::UseTree::Name(n) => {
                self.names.insert(n.ident.to_string());
            }
            syn::UseTree::Rename(r) => {
                self.names.insert(r.rename.to_string());
            }
            syn::UseTree::Group(g) => g.items.iter().for_each(|t| self.add_use(t)),
            syn::UseTree::Glob(_) => {}
        }
    }

    fn add_items(&mut self, items: &[syn::Item]) {
        for item in items {
            self.add_item(item);
        }
    }

    fn add_item(&mut self, item: &syn::Item) {
        use syn::Item;
        match item {
            Item::Struct(i) => {
                self.names.insert(i.ident.to_string());
            }
            Item::Enum(i) => {
                self.names.insert(i.ident.to_string());
            }
            Item::Union(i) => {
                self.names.insert(i.ident.to_string());
            }
            Item::Type(i) => {
                self.names.insert(i.ident.to_string());
            }
            Item::Const(i) => {
                self.names.insert(i.ident.to_string());
            }
            Item::Static(i) => {
                self.names.insert(i.ident.to_string());
            }
            Item::Fn(i) => {
                self.names.insert(i.sig.ident.to_string());
            }
            Item::Mod(i) => {
                self.names.insert(i.ident.to_string());
                if let Some((_, items)) = &i.content {
                    self.add_items(items);
                }
            }
            Item::Macro(i) => {
                if let Some(ident) = &i.ident {
                    self.names.insert(ident.to_string());
                }
            }
            Item::Use(i) => self.add_use(&i.tree),
            Item::Trait(i) => {
                let name = i.ident.to_string();
                self.names.insert(name.clone());
                for t in &i.items {
                    let member = match t {
                        syn::TraitItem::Fn(f) => f.sig.ident.to_string(),
                        syn::TraitItem::Const(c) => c.ident.to_string(),
                        syn::TraitItem::Type(t) => t.ident.to_string(),
                        _ => continue,
                    };
                    self.members.insert((name.clone(), member));
                }
            }
            Item::Impl(i) => {
                let Some(ty) = type_name(&i.self_ty) else {
                    return;
                };
                for t in &i.items {
                    let member = match t {
                        syn::ImplItem::Fn(f) => f.sig.ident.to_string(),
                        syn::ImplItem::Const(c) => c.ident.to_string(),
                        syn::ImplItem::Type(t) => t.ident.to_string(),
                        _ => continue,
                    };
                    self.members.insert((ty.clone(), member));
                }
            }
            _ => {}
        }
    }
}

fn type_name(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        syn::Type::Reference(r) => type_name(&r.elem),
        _ => None,
    }
}

fn index(crates: &[&str]) -> Option<Index> {
    let mut index = Index::default();
    let mut any = false;
    for krate in crates {
        let src = common::root().join(krate).join("src");
        if !src.is_dir() {
            continue;
        }
        any = true;
        let mut stack = vec![src];
        while let Some(path) = stack.pop() {
            if path.is_dir() {
                for entry in std::fs::read_dir(&path).unwrap() {
                    stack.push(entry.unwrap().path());
                }
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                let file =
                    syn::parse_file(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                index.add_items(&file.items);
            }
        }
    }
    any.then_some(index)
}

#[derive(Debug, Clone, PartialEq)]
enum Context {
    Top,
    Impl(String),
    Trait(String),
    Other,
    Reserved,
}

fn strip_comments(text: &str) -> String {
    let block = Regex::new(r"(?s)/\*.*?\*/").unwrap();
    let line = Regex::new(r"(?m)//.*$").unwrap();
    let text = block.replace_all(text, " ");
    line.replace_all(&text, "").into_owned()
}

/// The text without the items gated on an `unstable-*` feature: an
/// attribute and the item after it, up to its `;` or its closing brace.
/// What a reserved item uses is reserved with it.
fn without_unstable(text: &str) -> String {
    let gate = Regex::new(r#"\#\[cfg\(feature\s*=\s*"unstable-[^"]*"\)\]"#).unwrap();
    let mut out = String::new();
    let mut rest = text;
    while let Some(m) = gate.find(rest) {
        out.push_str(&rest[..m.start()]);
        let item = &rest[m.end()..];
        let mut depth = 0usize;
        let mut end = item.len();
        for (i, c) in item.char_indices() {
            match c {
                '{' => depth += 1,
                '}' if depth > 0 => {
                    depth -= 1;
                    if depth == 0 {
                        end = i + 1;
                        break;
                    }
                }
                ';' if depth == 0 => {
                    end = i + 1;
                    break;
                }
                _ => {}
            }
        }
        rest = &item[end..];
    }
    out.push_str(rest);
    out
}

/// What a block declares (`Type`, `Type::member`) and what it uses from the
/// SDK (`joker::item`).
fn declared(text: &str) -> (BTreeSet<String>, BTreeSet<String>) {
    let text = strip_comments(text);
    let token = Regex::new(
        r#"(?x)
        (?P<unstable>\#\[cfg\(feature\s*=\s*"unstable-[^"]*"\)\])
        | (?P<impl>\bimpl\b(?:\s*<[^{;]*?>)?\s+(?P<ihead>[^{;]*?)\{)
        | (?P<trait>\bpub\s+trait\s+(?P<tname>\w+)[^{;]*\{)
        | (?P<fn>(?P<fpub>\bpub(?:\([^)]*\))?\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?(?:extern\s+"C"\s+)?fn\s+(?P<fname>\w+))
        | (?P<item>\bpub\s+(?:struct|enum|union|type|const|static|mod|trait)\s+(?P<iname>\w+))
        | (?P<open>\{)
        | (?P<close>\})
        "#,
    )
    .unwrap();
    let head = Regex::new(r"^(?:.*?\bfor\s+)?(?:&(?:'\w+\s+)?)?(?:[\w:]+::)?(\w+)").unwrap();
    let mut stack = vec![Context::Top];
    let mut skip = false;
    let mut out = BTreeSet::new();
    for caps in token.captures_iter(&text) {
        let top = stack.last().cloned().unwrap_or(Context::Top);
        if caps.name("unstable").is_some() {
            skip = true;
        } else if let Some(h) = caps.name("ihead") {
            let ty = head
                .captures(h.as_str().trim())
                .map(|c| c[1].to_owned())
                .unwrap_or_default();
            stack.push(if skip || top == Context::Reserved {
                Context::Reserved
            } else {
                Context::Impl(ty)
            });
            skip = false;
        } else if let Some(name) = caps.name("tname") {
            if !skip && top == Context::Top {
                out.insert(name.as_str().to_owned());
            }
            stack.push(if skip || top == Context::Reserved {
                Context::Reserved
            } else {
                Context::Trait(name.as_str().to_owned())
            });
            skip = false;
        } else if let Some(name) = caps.name("fname") {
            let public = caps.name("fpub").is_some();
            if !skip {
                match &top {
                    Context::Top if public => {
                        out.insert(name.as_str().to_owned());
                    }
                    Context::Impl(ty) if public && !ty.is_empty() => {
                        out.insert(format!("{ty}::{}", name.as_str()));
                    }
                    Context::Trait(t) => {
                        out.insert(format!("{t}::{}", name.as_str()));
                    }
                    _ => {}
                }
            }
            skip = false;
        } else if let Some(name) = caps.name("iname") {
            if !skip {
                match &top {
                    Context::Top => {
                        out.insert(name.as_str().to_owned());
                    }
                    Context::Impl(ty) if !ty.is_empty() => {
                        out.insert(format!("{ty}::{}", name.as_str()));
                    }
                    _ => {}
                }
            }
            skip = false;
        } else if caps.name("open").is_some() {
            stack.push(if top == Context::Reserved {
                Context::Reserved
            } else {
                Context::Other
            });
        } else if caps.name("close").is_some() && stack.len() > 1 {
            stack.pop();
        }
    }
    let stable = without_unstable(&text);
    let used = Regex::new(r"\b(joker|joker_client|joker_hal|joker_test)::(\w+)")
        .unwrap()
        .captures_iter(&stable)
        .map(|c| format!("{}::{}", &c[1], &c[2]))
        .collect();
    if text.contains("#[joker::") || text.contains("fn main(") {
        return (BTreeSet::new(), used);
    }
    (out, used)
}

#[test]
fn the_reader_sees_declarations_and_uses() {
    let (decl, used) = declared(
        "pub struct A { pub x: u8 }\nimpl<'a, T: X> A<T> { pub fn get(&self) -> T; fn private(); }\n\
         pub trait B { fn run(&self); }\nimpl Secret { pub fn expose(&self) -> &[u8]; pub fn name(&self) -> &str; }\n\
         #[cfg(feature = \"unstable-x\")]\npub fn later();\n#[cfg(feature = \"unstable-y\")]\nimpl C { pub fn x(); }\n",
    );
    let expected: BTreeSet<String> = [
        "A",
        "A::get",
        "B",
        "B::run",
        "Secret::expose",
        "Secret::name",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    assert_eq!(decl, expected);
    assert!(used.is_empty());
    let (decl, used) = declared(
        "#[cfg(feature = \"unstable-z\")]\npub const E: joker_hal::Entry = joker_hal::entry! { a: B };\n\
         pub const F: joker_hal::Stable = 1;\n",
    );
    assert_eq!(decl, ["F".to_owned()].into_iter().collect());
    assert_eq!(used, ["joker_hal::Stable".to_owned()].into_iter().collect());
    let (decl, used) = declared("#[joker::cycle(period = \"1s\")]\npub fn control() {}\n");
    assert!(decl.is_empty());
    assert_eq!(used, ["joker::cycle".to_owned()].into_iter().collect());
}

#[test]
fn every_item_of_the_rust_blocks_exists_in_its_crates() {
    let blocks = common::reference_blocks("rust");
    assert!(blocks.len() >= 30, "only {} Rust blocks", blocks.len());
    let excerpts = common::Known::load().excerpts("rust");
    let mut found: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut indexes: BTreeMap<&str, Option<Index>> = BTreeMap::new();
    for block in &blocks {
        if excerpts.contains_key(&block.fingerprint()) {
            continue;
        }
        let Some((_, crates)) = SURFACES.iter().find(|(r, _)| *r == block.short) else {
            panic!(
                "{}: Rust block in a reference without a crate in SURFACES",
                block.at()
            );
        };
        let index = indexes
            .entry(block.short.as_str())
            .or_insert_with(|| index(crates));
        let (decl, used) = declared(&block.text);
        let chapter = block.section.split('.').next().unwrap_or("");
        let entry = found
            .entry(format!("{} §{chapter}", block.short))
            .or_default();
        match index {
            None => {
                entry.insert(format!("crate {} does not exist", crates.join(", ")));
            }
            Some(index) => {
                for item in decl.iter().filter(|i| !index.has(i)) {
                    entry.insert(item.clone());
                }
                for item in used.iter() {
                    let name = item.split_once("::").map_or(item.as_str(), |(_, n)| n);
                    if !index.has(name) {
                        entry.insert(item.clone());
                    }
                }
            }
        }
    }
    found.retain(|_, v| !v.is_empty());
    common::ratchet("rust", &found);
}
