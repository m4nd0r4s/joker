# ADR 0059 — `jkc` diagnostics and the canonical contract

**Status:** accepted (v1) · **Date:** 2026-10

## Context

Document 17 defines the `.jkc` language, the rules of `jkc check` (§7) and says that `jkc` writes a canonical `<name>@<version>.contract.json` with a contract hash (pinned by the lock, ref/02 §2.1.6) and a documentation hash (§6). Building the compiler (AUR-262) found what the text leaves open:

- §7 has no code for a syntax error, a duplicate name, an unknown name, an invalid annotation or a type error, although `jkc` must report all of them.
- The layout of the `.contract.json`, what "canonical" means byte by byte, and exactly what each hash covers are not written anywhere; ref/09 serves the file (`GET /v1/classes/{package}/{class}`) as an untyped object.
- `JC0107` ("physical quantity without a unit") contradicts document 14 §4.3, where `analog_in@1` declares `in reading: f64` without a unit because "the unit is given by the instance".
- `JC0310` ("queries in a cycle") is ambiguous (errata `translation-2026-10`, 17-3).
- `JC0205` says what a wrong `order` names, not what it must name; `JC0201` says writes expanded by calls count, not how; `JW0230` does not say what "conditioned on an `in`" is.
- §9 lists the changes and their bump, but not how `jkc` decides between them when a change is not in the table.

## Decision

### Codes

1. New codes in `codes.toml`, all errors: `JC0101` syntax error, or a name that breaks the naming rules (snake_case members, arguments, capabilities and variants; UpperCamelCase enums and records; at most 32 characters); `JC0102` a name declared twice (in the namespace of the class, among the methods, the capabilities, the fields of a record or event, the variants of an enum, or a class, enum or record declared twice in the compilation); `JC0103` an unknown name (a member, query, type, enum, variant or record); `JC0105` an unknown annotation without a namespace, an annotation on an item it does not apply to, or invalid arguments; `JC0106` a type error (a type an item does not accept, an invalid unit, a literal that does not fit, an expression of the wrong type, a collection read outside an aggregation, the wrong number of arguments in a call).
2. `jkc` also uses two manifest codes where they already mean the same thing: `J0804` (the class name and the file name, or the package and the directory under `classes/`, disagree; or a file has more than one class) and `J0847` (a `use` or a class reference names a class that is neither in the registry nor in the project, ref/03 §2.7).
3. A syntax error stops its file: the rest of the file is not checked. Every other finding is reported, and a class with an error has no contract.
4. Expression errors from the evaluator of document 02 §10 keep their meaning under the class codes: dimensions and quantity kinds are `JC0301`, a window above 15 minutes `JC0320`, an expression syntax error `JC0101`, an unknown path or variant `JC0103`, anything else `JC0106`.
5. Diagnostics render exactly as those of `jk check` (ref/01 §11.4): `error[JC0107]: …`, the file relative to the project root, line and column, the source line and the carets.

### Clarified rules

6. **`JC0107`.** An `f64` or `f32` `out`, query, method or action argument, or record or event field without a unit is `JC0107`; `1` declares a dimensionless number. An `in` or `param` without a unit takes its unit from the binding of each instance (`analog_in@1`'s `reading`), and an expression of the class that reads it is `JC0107`, because its dimension cannot be checked.
7. **`JC0310`** is a dependency cycle between queries: `a` reads `b`, which reads `a`, directly or through other queries.
8. **`JC0205`.** `atomic` without `order`, `order` without `atomic`, and an `order` that names something the method neither assigns nor calls, names it twice, or leaves out an assigned `out` or a called member, are `JC0205`.
9. **`JC0201`** counts, for each method, the `out` members it assigns and those the methods it calls assign, transitively, each under the path of the member (`drive.run_cmd`). A path written twice is `JC0201`.
10. **`JW0230`.** A method is conditioned on an `in` when an assignment or a call argument reads an `in` member, a query, or an `in` or query of a fixed class member, other than through `own()`. One warning per method.
11. `own(x)` takes an `out` member of the class itself (`JC0106` otherwise).

### The canonical contract

12. `jkc` writes, for each class without errors, `<name>@<version>.contract.json`. Its content is the canonical JSON of an object with four keys: `contract`, `contract_hash`, `docs` and `doc_hash`.
13. **Canonical JSON:** UTF-8; no whitespace outside strings; object keys sorted by their bytes; strings escape only `"`, `\` and the control characters (`\b`, `\f`, `\n`, `\r`, `\t`, otherwise `\u00xx` in lowercase hexadecimal); numbers only as integers; every key of the layout below is always present (`null`, `false` or `[]` when empty). The same class gives the same bytes on every run and platform.
14. **`contract`** holds: `jkc` (1); `class` (`<package>/<name>`); `version`; `capabilities`; `members` (`name`, `kind` `in`|`out`|`param`, `type`, `unit`, `capability`, `optional`, `default`, `safe_hint`, `range_hint`, `renamed_from`); `class_members` (`name`, `class` `<package>/<name>@<major>`, `collection`, `min`, `max`, `optional`, `requires`, `capability`, `renamed_from`); `queries` (`name`, `type`, `unit`, `expr`, `capability`, `renamed_from`); `methods` (`name`, `capability`, `atomic`, `order`, `params`, `body`, `renamed_from`, where a statement is `{assign, expr}` or `{call, method, args}`); `events` (`name`, `capability`, `record`, `fields`, `renamed_from`); `actions` (`name`, `capability`, `params`, `result`, `timeout`, `permit_hint`, `role_hint`, `long`, `renamed_from`); `health`; and the `enums` and `records` the class uses, by full name (`std/PcsMode`), so a contract is self-contained.
   - Items keyed by name are sorted by name, and `capabilities`, `health` and `requires` are sorted; parameters, fields, statements, `order` and enum variants keep their declaration order, because position is meaning (record keys are field positions, docs/17 §9).
   - Types are written `bool`, `i64`, `u64`, `f64`, `f32`, `string`, `bytes`, `bits<N>`, `enum <package>/<Name>`, `record <package>/<Name>`, `list<T, N>` and `()`; units as written in the class.
   - Expressions and literals are canonical: their tokens, one space around binary operators and keywords, none after `(` or a unary operator, none before `)` or `,`, a literal as `<number> <unit>`. Reformatting a class never changes its contract.
15. **`docs`** holds `items`: for the class (key `class`) and each documented item (key `<kind>:<name>`, kinds `in`, `out`, `param`, `member`, `query`, `method`, `event`, `action`), its `doc` (the `///` lines, joined by `\n`) and its documentation `annotations` (`@aligns`, `@deprecated` and every namespaced one, such as `@hmi.label`), each with its arguments as written. Comments (`//`) are in neither part.
16. **`contract_hash`** is the SHA-256 of the canonical bytes of `contract`, and **`doc_hash`** that of `docs`, both written `sha256:<hex>` (ref/00 §6). Hints (`@safe_hint`, `@range_hint`, `@permit_hint`, `@role_hint`, `@timeout`) are in the contract: the conformance runner and the manifest read them.
17. A reader of a `.contract.json` (the node, the API) verifies both hashes and refuses the file if either does not match.

### Versions (`JC0601`)

18. `jkc` compares a class with the previous contract of the same reference and computes the largest bump its changes require, with the table of docs/17 §9 and these rules for what it leaves out: removing a capability, member, query, method, event or action is major unless a new item names it with `@renamed_from` and keeps its kind and type (minor); adding a member is minor only if it is `@optional` or belongs to a capability the previous version did not have, and major otherwise; making a member optional, changing a default or a hint, and adding variants to an enum are minor; removing or renumbering a variant, changing `health required`, moving an item between capabilities, and changing a class member other than by widening its bounds or making it optional, are major; parameters of actions follow the rule of record fields; documentation only is a patch.
19. A new version that is older than the previous, or that bumps less than its changes require (the same version with another contract hash included), is `JC0601`.

## Consequences

- Every rule of docs/17 §7 and every code of this ADR has a failing fixture in `crates/joker-jkc/tests/fixtures`, and the contract of `pump@1` is pinned byte for byte in a golden file.
- The lock, the API and the node read the same bytes, and a contract tampered with after `jkc` wrote it is refused.
- `analog_in@1`'s `reading` keeps the unit of each instance, and a class that wants to compute with a value declares its unit.
