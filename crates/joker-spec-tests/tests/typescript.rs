//! `tsc` on the TypeScript blocks (docs/13 §4, task 0a; ADR 0080).
//!
//! Every `ts` block of the documentation must parse as TypeScript, unless
//! `known.toml` lists it as an excerpt. The declaration blocks of ref/05
//! (those that only declare: no `import`) are then type-checked together
//! under `--strict`, as the `.d.ts` of the SDK they describe; their
//! diagnostics are divergences, recorded with a triage issue until the
//! TypeScript SDK exists (1.2, ADR 0035). The compiler is the
//! `typescript` package that `npm root -g` finds, or `JOKER_TYPESCRIPT`.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test helpers fail the test on a broken corpus"
)]

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::process::Command;

const CHECKER: &str = r#"
const ts = require(process.argv[2]);
const fs = require("fs");
const files = process.argv.slice(4);
const out = [];
const show = (d) => {
  const text = ts.flattenDiagnosticMessageText(d.messageText, " ");
  let at = "";
  if (d.file && d.start !== undefined) {
    const p = d.file.getLineAndCharacterOfPosition(d.start);
    at = `${d.file.fileName}:${p.line + 1}`;
  }
  return { at, code: `TS${d.code}`, text };
};
if (process.argv[3] === "syntax") {
  for (const f of files) {
    const sf = ts.createSourceFile(f, fs.readFileSync(f, "utf8"), ts.ScriptTarget.ES2022, true, ts.ScriptKind.TS);
    for (const d of sf.parseDiagnostics) out.push(show(d));
  }
} else {
  const program = ts.createProgram(files, {
    strict: true, noEmit: true, target: ts.ScriptTarget.ES2022,
    lib: ["lib.es2022.d.ts"], types: [], skipLibCheck: true,
  });
  for (const d of ts.getPreEmitDiagnostics(program)) out.push(show(d));
}
process.stdout.write(JSON.stringify(out));
"#;

fn typescript() -> Option<(PathBuf, PathBuf)> {
    let node = common::which("node");
    let module = std::env::var_os("JOKER_TYPESCRIPT")
        .map(PathBuf::from)
        .or_else(|| {
            let npm = common::which("npm")?;
            let (ok, out) = common::run(Command::new(npm).args(["root", "-g"]));
            ok.then(|| PathBuf::from(out.trim()).join("typescript"))
        })
        .filter(|p| p.join("package.json").is_file());
    let found = node.is_some() && module.is_some();
    common::tool(
        "node and the typescript package (npm install --global typescript)",
        found,
    )
    .then(|| (node.unwrap(), module.unwrap()))
}

fn check(
    node: &PathBuf,
    module: &PathBuf,
    mode: &str,
    files: &[PathBuf],
) -> Vec<(String, String, String)> {
    let dir = common::scratch(&format!("ts-{mode}-runner"));
    let script = dir.join("check.cjs");
    std::fs::write(&script, CHECKER).unwrap();
    let out = Command::new(node)
        .arg(&script)
        .arg(module)
        .arg(mode)
        .args(files)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "tsc runner failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let diags: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    diags
        .as_array()
        .unwrap()
        .iter()
        .map(|d| {
            (
                d["at"].as_str().unwrap_or("").to_owned(),
                d["code"].as_str().unwrap_or("").to_owned(),
                d["text"].as_str().unwrap_or("").to_owned(),
            )
        })
        .collect()
}

fn is_declaration(block: &common::Block) -> bool {
    !block.text.lines().any(|l| l.starts_with("import "))
        && block
            .text
            .lines()
            .find(|l| !l.trim().is_empty() && !l.trim_start().starts_with("//"))
            .is_some_and(|l| l.starts_with("export "))
}

#[test]
fn every_typescript_block_parses_and_the_sdk_declarations_type_check() {
    let blocks: Vec<common::Block> = common::markdown("docs")
        .iter()
        .filter(|d| !d.contains("/proposals/") && !d.contains("/superpowers/"))
        .flat_map(|d| common::blocks(d))
        .filter(|b| b.lang == "ts" || b.lang == "typescript")
        .collect();
    assert!(
        blocks.len() >= 25,
        "only {} TypeScript blocks",
        blocks.len()
    );
    let excerpts = common::Known::load().excerpts("typescript");
    let present: BTreeSet<String> = blocks.iter().map(common::Block::fingerprint).collect();
    let stale: Vec<&String> = excerpts.keys().filter(|f| !present.contains(*f)).collect();
    assert!(
        stale.is_empty(),
        "stale typescript excerpts in known.toml: {stale:?}"
    );
    let Some((node, module)) = typescript() else {
        return;
    };
    let dir = common::scratch("ts-blocks");
    let mut files = Vec::new();
    let mut by_file = BTreeMap::new();
    for block in blocks
        .iter()
        .filter(|b| !excerpts.contains_key(&b.fingerprint()))
    {
        let file = dir.join(format!("{}.ts", block.fingerprint()));
        std::fs::write(&file, &block.text).unwrap();
        by_file.insert(file.display().to_string(), block.clone());
        files.push(file);
    }
    let syntax: Vec<String> = check(&node, &module, "syntax", &files)
        .into_iter()
        .map(|(at, code, text)| {
            let file = at.rsplit_once(':').map_or(at.as_str(), |(f, _)| f);
            let block = by_file.get(file).map_or(at.clone(), common::Block::at);
            format!("{block}: {code} {text}")
        })
        .collect();
    assert!(
        syntax.is_empty(),
        "TypeScript blocks that do not parse (fix them, or list them as excerpts in known.toml):\n{}",
        syntax.join("\n")
    );
    let declarations: String = blocks
        .iter()
        .filter(|b| b.short == "ref/05" && is_declaration(b))
        .map(|b| b.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let sdk = common::scratch("ts-sdk").join("sdk.ts");
    std::fs::write(&sdk, declarations).unwrap();
    let mut found: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (_, code, text) in check(&node, &module, "types", &[sdk]) {
        let name = text
            .split('\'')
            .nth(1)
            .map_or_else(|| text.clone(), str::to_owned);
        found
            .entry("ref/05".to_owned())
            .or_default()
            .insert(format!("{code} {name}"));
    }
    common::ratchet("typescript", &found);
}
