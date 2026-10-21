//! Reading the manifest layers, resolving `extends` and merging the layers
//! into one tree (ref/01 §7.1–§7.3, §7.7).

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use toml::de::DeTable;

use crate::diag::{Diagnostic, Diagnostics};
use crate::source::{FileId, Sources, Span, normalize};
use crate::value::{self, Entry, Node, Table, Value};

/// The largest manifest file accepted (ref/01 §11.3).
pub const MAX_FILE_SIZE: usize = 4 * 1024 * 1024;
/// The deepest `extends` chain (ref/01 §5.1).
pub const MAX_EXTENDS_DEPTH: usize = 8;
/// The most files in one `extends` graph (ref/01 §5.1).
pub const MAX_EXTENDS_FILES: usize = 32;

/// Keys whose lists are sets: `append` skips elements already present
/// (ref/01 §7.3). Every other list is ordered, and a repeated element is
/// `J0164`.
const SET_KEYS: [&str; 8] = [
    "requires_good",
    "alias",
    "capabilities",
    "allow",
    "trip",
    "writes",
    "reads",
    "remote_levels",
];

/// Identification tables of the other file types, which a manifest layer
/// never has (ref/01 §7.7).
const FOREIGN_TABLES: [&str; 4] = ["profile", "board", "scenario", "suite"];

/// Where the manifest files are read from.
pub trait FileSystem {
    /// Reads a UTF-8 file.
    fn read(&self, path: &Path) -> io::Result<String>;
    /// Whether a file or folder exists.
    fn exists(&self, path: &Path) -> bool;
}

/// The real file system.
#[derive(Debug, Clone, Copy, Default)]
pub struct OsFileSystem;

impl FileSystem for OsFileSystem {
    fn read(&self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }
}

/// An in-memory file system, for tests and for tools that hold the files.
#[derive(Debug, Clone, Default)]
pub struct MemoryFileSystem {
    files: BTreeMap<PathBuf, String>,
    dirs: BTreeSet<PathBuf>,
    permissive: bool,
}

impl MemoryFileSystem {
    /// Adds a file.
    pub fn file(&mut self, path: impl Into<PathBuf>, text: impl Into<String>) -> &mut Self {
        self.files.insert(normalize(&path.into()), text.into());
        self
    }

    /// Adds an empty folder.
    pub fn dir(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.dirs.insert(normalize(&path.into()));
        self
    }

    /// Makes every path that is not a file count as an existing folder, for
    /// checking examples whose app folders and models are not at hand.
    pub fn permissive(&mut self) -> &mut Self {
        self.permissive = true;
        self
    }
}

impl FileSystem for MemoryFileSystem {
    fn read(&self, path: &Path) -> io::Result<String> {
        self.files
            .get(&normalize(path))
            .cloned()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn exists(&self, path: &Path) -> bool {
        let path = normalize(path);
        self.permissive
            || self.files.contains_key(&path)
            || self.dirs.contains(&path)
            || self.files.keys().any(|file| file.starts_with(&path))
    }
}

/// One manifest layer, as read.
#[derive(Debug, Clone)]
pub struct Layer {
    /// The file.
    pub file: FileId,
    /// The file path, normalised.
    pub path: PathBuf,
    /// The contents, without `extends`.
    pub table: Table,
}

/// How a file name fits the layer naming rule of ref/01 §7.7.
enum LayerName {
    Valid,
    BadVariant,
    Other,
}

fn layer_name(path: &Path) -> LayerName {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return LayerName::Other;
    };
    if name == "joker.toml" {
        return LayerName::Valid;
    }
    let Some(variant) = name
        .strip_prefix("joker.")
        .and_then(|rest| rest.strip_suffix(".toml"))
    else {
        return LayerName::Other;
    };
    let valid = variant.len() <= 32
        && variant.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        });
    if valid {
        LayerName::Valid
    } else {
        LayerName::BadVariant
    }
}

struct Loader<'a> {
    fs: &'a dyn FileSystem,
    root: PathBuf,
    sources: &'a mut Sources,
    diags: &'a mut Diagnostics,
    visited: BTreeSet<PathBuf>,
    layers: Vec<Layer>,
    too_many: bool,
}

/// Reads the entry manifest and every layer it extends, linearised in
/// post-order with each file once (ref/01 §7.1). The entry layer comes last.
pub fn load(
    fs: &dyn FileSystem,
    entry: &Path,
    sources: &mut Sources,
    diags: &mut Diagnostics,
) -> Vec<Layer> {
    let entry = normalize(entry);
    let root = entry.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut loader = Loader {
        fs,
        root,
        sources,
        diags,
        visited: BTreeSet::new(),
        layers: Vec::new(),
        too_many: false,
    };
    match layer_name(&entry) {
        LayerName::Valid => {}
        LayerName::BadVariant => loader.diags.push(Diagnostic::new(
            "J0150",
            format!("invalid layer variant in `{}`", entry.display()),
        )),
        LayerName::Other => loader.diags.push(
            Diagnostic::new(
                "J0169",
                format!(
                    "entry manifest `{}` is not named `joker.toml` or `joker.<variant>.toml`",
                    entry.display()
                ),
            )
            .help("`jk migrate` proposes the new name"),
        ),
    }
    loader.visit(&entry, None, &mut Vec::new());
    loader.layers
}

impl Loader<'_> {
    fn display(&self, path: &Path) -> PathBuf {
        path.strip_prefix(&self.root)
            .map_or_else(|_| path.to_path_buf(), Path::to_path_buf)
    }

    fn visit(&mut self, path: &Path, from: Option<Span>, chain: &mut Vec<PathBuf>) {
        if self.visited.contains(path) {
            return;
        }
        let place = |d: Diagnostic| match from {
            Some(span) => d.at(span),
            None => d,
        };
        if chain.iter().any(|p| p == path) {
            self.diags.push(place(Diagnostic::new(
                "J0161",
                format!("`extends` cycle through `{}`", self.display(path).display()),
            )));
            return;
        }
        if chain.len() >= MAX_EXTENDS_DEPTH {
            self.diags.push(place(Diagnostic::new(
                "J0162",
                format!("`extends` deeper than {MAX_EXTENDS_DEPTH} levels"),
            )));
            return;
        }
        let text = match self.fs.read(path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                self.diags.push(place(Diagnostic::new(
                    "J0146",
                    format!("`{}` is not UTF-8", self.display(path).display()),
                )));
                return;
            }
            Err(_) => {
                self.diags.push(place(Diagnostic::new(
                    "J0160",
                    format!("file not found: `{}`", self.display(path).display()),
                )));
                return;
            }
        };
        let size = text.len();
        let file = self.sources.add(self.display(path), text);
        if size > MAX_FILE_SIZE {
            self.diags.push(Diagnostic::new(
                "J0149",
                format!(
                    "`{}` is larger than 4 MiB",
                    self.sources.path(file).display()
                ),
            ));
            return;
        }
        let Some(mut table) = self.parse(file) else {
            return;
        };
        if let Some(foreign) = FOREIGN_TABLES.iter().find(|t| table.contains_key(**t)) {
            let key = table.get(*foreign).map(|e| e.key);
            let mut d = Diagnostic::new(
                "J0803",
                format!("`[{foreign}]` in a manifest layer: this file is not a manifest"),
            );
            if let Some(span) = key {
                d = d.at(span);
            }
            self.diags.push(d);
            return;
        }
        chain.push(path.to_path_buf());
        if let Some(extends) = table.remove("extends") {
            for (target, span) in self.extends_targets(&extends, path) {
                self.visit(&target, Some(span), chain);
            }
        }
        chain.pop();
        if self.visited.insert(path.to_path_buf()) {
            if self.visited.len() > MAX_EXTENDS_FILES && !self.too_many {
                self.too_many = true;
                self.diags.push(place(Diagnostic::new(
                    "J0162",
                    format!("`extends` graph with more than {MAX_EXTENDS_FILES} files"),
                )));
            }
            self.layers.push(Layer {
                file,
                path: path.to_path_buf(),
                table,
            });
        }
    }

    fn parse(&mut self, file: FileId) -> Option<Table> {
        let text = self.sources.get(file).map(|f| f.text().to_owned())?;
        match DeTable::parse(&text) {
            Ok(parsed) => match value::from_toml(file, parsed.get_ref()) {
                Ok(table) => Some(table),
                Err(value::InvalidNumber(span)) => {
                    self.diags.push(
                        Diagnostic::new("J0146", "number out of range")
                            .at(span)
                            .label("does not fit a 64-bit integer or float"),
                    );
                    None
                }
            },
            Err(error) => {
                let range = error.span().unwrap_or(0..0);
                self.diags.push(
                    Diagnostic::new("J0146", "file is not valid TOML")
                        .at(Span::new(file, range.start, range.end))
                        .label(error.message().trim().to_owned()),
                );
                None
            }
        }
    }

    fn extends_targets(&mut self, extends: &Entry, declaring: &Path) -> Vec<(PathBuf, Span)> {
        let Value::Array(items) = &extends.node.value else {
            self.diags.push(
                Diagnostic::new("J0143", "`extends` must be a list of files")
                    .at(extends.node.span)
                    .label(format!("found {}", extends.node.value.type_name())),
            );
            return Vec::new();
        };
        let base = declaring.parent().unwrap_or(Path::new(""));
        let mut targets = Vec::new();
        let mut seen = BTreeSet::new();
        for item in items {
            let Some(text) = item.as_str() else {
                self.diags.push(
                    Diagnostic::new("J0143", "`extends` entries must be strings")
                        .at(item.span)
                        .label(format!("found {}", item.value.type_name())),
                );
                continue;
            };
            if text.contains("://") || text.starts_with("oci:") {
                self.diags.push(
                    Diagnostic::new("J0160", "`extends` only accepts local files")
                        .at(item.span)
                        .label("not a local file"),
                );
                continue;
            }
            let target = normalize(&base.join(text));
            match layer_name(&target) {
                LayerName::Valid => {}
                LayerName::BadVariant => {
                    self.diags.push(
                        Diagnostic::new("J0150", "invalid layer variant")
                            .at(item.span)
                            .label(
                                "the variant follows `[a-z0-9]+(-[a-z0-9]+)*`, up to 32 characters",
                            ),
                    );
                    continue;
                }
                LayerName::Other => {
                    self.diags.push(
                        Diagnostic::new(
                            "J0803",
                            "`extends` names a file that is not a manifest layer",
                        )
                        .at(item.span)
                        .label("a layer is named `joker.toml` or `joker.<variant>.toml`"),
                    );
                    continue;
                }
            }
            if !seen.insert(target.clone()) {
                self.diags.push(
                    Diagnostic::new("J0164", "repeated element in an ordered list")
                        .at(item.span)
                        .label("already in `extends`"),
                );
                continue;
            }
            targets.push((target, item.span));
        }
        targets
    }
}

enum Directive<'a> {
    Unset,
    Append(&'a [Node]),
    Invalid(&'static str),
}

/// Whether a node is `{ unset = true }`.
pub fn is_unset(node: &Node) -> bool {
    matches!(directive(node), Some(Directive::Unset))
}

fn directive(node: &Node) -> Option<Directive<'_>> {
    let table = node.as_table()?;
    if let Some(unset) = table.get("unset") {
        return Some(match (&unset.node.value, table.len()) {
            (Value::Bool(true), 1) => Directive::Unset,
            (_, 1) => Directive::Invalid("`unset` only takes `true`"),
            _ => Directive::Invalid("`unset` cannot be mixed with other keys"),
        });
    }
    if let Some(append) = table.get("append") {
        return Some(match (&append.node.value, table.len()) {
            (Value::Array(items), 1) => Directive::Append(items),
            (_, 1) => Directive::Invalid("`append` takes a list"),
            _ => Directive::Invalid("`append` cannot be mixed with other keys"),
        });
    }
    None
}

/// Whether two nodes hold the same value, whatever their spans.
pub fn same(a: &Node, b: &Node) -> bool {
    match (&a.value, &b.value) {
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| same(a, b))
        }
        (Value::Table(x), Value::Table(y)) => {
            x.len() == y.len()
                && x.iter()
                    .zip(y)
                    .all(|((ka, a), (kb, b))| ka == kb && same(&a.node, &b.node))
        }
        (x, y) => x == y,
    }
}

/// Merges the layers in order, the entry layer last (ref/01 §7.2).
pub fn merge(layers: &[Layer], diags: &mut Diagnostics) -> Table {
    let mut merged = Table::new();
    for layer in layers {
        merge_table(&mut merged, &layer.table, diags);
    }
    merged
}

/// Merges one layer's table into an accumulated one, with the rules of
/// ref/01 §7.2.
pub fn merge_table(acc: &mut Table, layer: &Table, diags: &mut Diagnostics) {
    for (key, entry) in layer {
        match directive(&entry.node) {
            Some(Directive::Unset) => {
                if acc.remove(key).is_none() {
                    diags.push(
                        Diagnostic::new(
                            "J0163",
                            format!("`unset` of `{key}`, which no earlier layer sets"),
                        )
                        .at(entry.key),
                    );
                }
            }
            Some(Directive::Invalid(why)) => {
                diags.push(
                    Diagnostic::new("J0164", "invalid merge directive")
                        .at(entry.node.span)
                        .label(why),
                );
            }
            Some(Directive::Append(items)) => append(acc, key, entry, items, diags),
            None => match (acc.get_mut(key), &entry.node.value) {
                (Some(previous), Value::Table(table)) => match &mut previous.node.value {
                    Value::Table(previous_table) => merge_table(previous_table, table, diags),
                    _ => diags.push(
                        Diagnostic::new(
                            "J0165",
                            format!("overlay turns `{key}` from a value into a table"),
                        )
                        .at(entry.key),
                    ),
                },
                (Some(previous), _) if matches!(previous.node.value, Value::Table(_)) => {
                    diags.push(
                        Diagnostic::new(
                            "J0165",
                            format!("overlay turns `{key}` from a table into a value"),
                        )
                        .at(entry.key),
                    );
                }
                (None, Value::Table(table)) => {
                    let mut fresh = Table::new();
                    merge_table(&mut fresh, table, diags);
                    acc.insert(
                        key.clone(),
                        Entry {
                            key: entry.key,
                            node: Node {
                                value: Value::Table(fresh),
                                span: entry.node.span,
                            },
                        },
                    );
                }
                _ => {
                    acc.insert(key.clone(), entry.clone());
                }
            },
        }
    }
}

fn append(acc: &mut Table, key: &str, entry: &Entry, items: &[Node], diags: &mut Diagnostics) {
    let is_set = SET_KEYS.contains(&key);
    let target = acc.entry(key.to_owned()).or_insert_with(|| Entry {
        key: entry.key,
        node: Node {
            value: Value::Array(Vec::new()),
            span: entry.node.span,
        },
    });
    let Value::Array(list) = &mut target.node.value else {
        diags.push(
            Diagnostic::new("J0164", "invalid merge directive")
                .at(entry.node.span)
                .label(format!("`append` on `{key}`, which is not a list")),
        );
        return;
    };
    for item in items {
        if list.iter().any(|present| same(present, item)) {
            if !is_set {
                diags.push(
                    Diagnostic::new("J0164", "repeated element in an ordered list")
                        .at(item.span)
                        .label(format!("already in `{key}`")),
                );
            }
            continue;
        }
        list.push(item.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(files: &[(&str, &str)], entry: &str) -> (Table, Diagnostics, Sources, Vec<Layer>) {
        let mut fs = MemoryFileSystem::default();
        for (path, text) in files {
            fs.file(*path, *text);
        }
        let mut sources = Sources::default();
        let mut diags = Diagnostics::default();
        let layers = load(&fs, Path::new(entry), &mut sources, &mut diags);
        let merged = merge(&layers, &mut diags);
        (merged, diags, sources, layers)
    }

    fn codes(diags: &Diagnostics) -> Vec<&'static str> {
        diags.iter().map(|d| d.code).collect()
    }

    #[test]
    fn linearisation_is_post_order_without_repeats() {
        let (_, diags, sources, layers) = run(
            &[
                (
                    "site/joker.toml",
                    "extends = [\"joker.a.toml\", \"joker.b.toml\"]\n",
                ),
                ("site/joker.a.toml", "extends = [\"joker.t.toml\"]\n"),
                (
                    "site/joker.b.toml",
                    "extends = [\"joker.t.toml\", \"joker.p.toml\"]\n",
                ),
                ("site/joker.t.toml", ""),
                ("site/joker.p.toml", ""),
            ],
            "site/joker.toml",
        );
        assert!(diags.is_empty(), "{diags:?}");
        let order: Vec<String> = layers
            .iter()
            .map(|l| sources.path(l.file).display().to_string())
            .collect();
        assert_eq!(
            order,
            [
                "joker.t.toml",
                "joker.a.toml",
                "joker.p.toml",
                "joker.b.toml",
                "joker.toml"
            ]
        );
    }

    #[test]
    fn extends_paths_are_relative_to_the_declaring_file() {
        let (merged, diags, _, _) = run(
            &[
                (
                    "sites/x/joker.toml",
                    "extends = [\"../../templates/joker.base.toml\"]\n",
                ),
                (
                    "templates/joker.base.toml",
                    "extends = [\"joker.common.toml\"]\n[a]\nb = 1\n",
                ),
                ("templates/joker.common.toml", "[a]\nc = 2\n"),
            ],
            "sites/x/joker.toml",
        );
        assert!(diags.is_empty(), "{diags:?}");
        let a = merged["a"].node.as_table().unwrap();
        assert!(a.contains_key("b") && a.contains_key("c"));
    }

    #[test]
    fn extends_errors() {
        let (_, diags, _, _) = run(
            &[
                (
                    "joker.toml",
                    "extends = [\"joker.a.toml\", \"missing/joker.toml\", \"oci://r/x:1\", \"base.toml\"]\n",
                ),
                ("joker.a.toml", "extends = [\"joker.toml\"]\n"),
            ],
            "joker.toml",
        );
        assert_eq!(codes(&diags), ["J0160", "J0803", "J0161", "J0160"]);
    }

    #[test]
    fn merge_rules_and_directives() {
        let (merged, diags, _, _) = run(
            &[
                (
                    "joker.base.toml",
                    "[o]\nlist = [\"a\"]\nset = [\"a\"]\nscalar = 1\ngone = 1\n[t]\nx = 1\n",
                ),
                (
                    "joker.toml",
                    "extends = [\"joker.base.toml\"]\n[o]\nlist = { append = [\"b\"] }\nrequires_good = { append = [\"x\", \"x\"] }\nscalar = 2\ngone = { unset = true }\nnever = { unset = true }\nt = 1\n",
                ),
            ],
            "joker.toml",
        );
        assert_eq!(codes(&diags), ["J0163"]);
        let o = merged["o"].node.as_table().unwrap();
        let strings = |key: &str| -> Vec<String> {
            match &o[key].node.value {
                Value::Array(items) => items
                    .iter()
                    .filter_map(|i| i.as_str().map(str::to_owned))
                    .collect(),
                _ => Vec::new(),
            }
        };
        assert_eq!(strings("list"), ["a", "b"]);
        assert_eq!(strings("requires_good"), ["x"]);
        assert_eq!(o["scalar"].node.value, Value::Integer(2));
        assert!(!o.contains_key("gone"));
        assert_eq!(o["t"].node.value, Value::Integer(1));
    }

    #[test]
    fn merge_errors() {
        let (_, diags, _, _) = run(
            &[
                (
                    "joker.base.toml",
                    "[a]\nt = { x = 1 }\nv = 1\nord = [\"a\"]\n",
                ),
                (
                    "joker.toml",
                    "extends = [\"joker.base.toml\"]\n[a]\nt = 1\nv = { y = 1 }\nord = { append = [\"a\"] }\nbad = { unset = false }\nmixed = { append = [1], x = 1 }\n",
                ),
            ],
            "joker.toml",
        );
        let mut found = codes(&diags);
        found.sort_unstable();
        assert_eq!(found, ["J0164", "J0164", "J0164", "J0165", "J0165"]);
    }

    #[test]
    fn invalid_toml_and_foreign_files() {
        let (_, diags, _, _) = run(&[("joker.toml", "a = \n")], "joker.toml");
        assert_eq!(codes(&diags), ["J0146"]);
        let (_, diags, _, _) = run(&[("joker.toml", "[profile]\nname = \"x\"\n")], "joker.toml");
        assert_eq!(codes(&diags), ["J0803"]);
        let (_, diags, _, _) = run(&[("site.toml", "")], "site.toml");
        assert_eq!(codes(&diags), ["J0169"]);
    }
}
