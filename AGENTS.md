# AGENTS.md

Instructions for any agent (or person) working in this repository. Short on purpose: the detail lives in `docs/`.

## 1. The specification rules

- `docs/` is normative. If the code and the documentation diverge, the code is wrong.
- If the documentation is wrong or incomplete, **do not work around it in code**. Write a new ADR in `docs/adr/` (next free number), update the affected documents in the same set of commits, and only then implement.
- Minimum reading before touching code: `docs/00`, `docs/12`, `docs/16`, `docs/ref/00`, the document for your area and the reference for your surface in `docs/ref/`.
- **The references in `docs/ref/` and the files in `wit/` and `spec/` change together** (ADR 0023): changing an interface means changing the reference and the machine-readable file in the same set of commits, with an ADR if it touches a public contract (ADR 0022). If a file and the text diverge, it is a bug in both; `joker-spec-tests` (document 13, task 0a) fails CI.
- Precedence between documents: the one in `docs/README.md` (the references in `docs/ref/` rank above the per-area documents). Examples and the learning track are illustrative; if they contradict the specification, the example is fixed.
- The 1.0 scope is the cut of `docs/26` §5 (ADR 0028, stable = implemented). Where an older document contradicts it, the cut wins and the surface is reserved.

## 2. Non-negotiable invariants

1. **`joker-core` and `joker-expr` do no I/O**, do not read the clock and do not generate randomness. The clock comes in through the `Clock` trait (ADR 0007).
2. **`joker-units`, `joker-expr`, `joker-core`, `joker-link-proto` and `joker-agent-proto` are `#![no_std]`** (the normative list is `docs/13` §1) and do not allocate on the tick path; every structure has a fixed capacity. `alloc` is an optional feature, only for building the registry on Linux (ADR 0020). A new dependency in these crates must be `no_std`. Synchronization in the core is only through `critical-section` and `portable-atomic`.
3. **No write path bypasses the envelope**: not apps, not the CLI, not the API, not bridges. Drivers only receive writes that were already applied (*committed*).
4. **Quality never disappears silently.** A `Bad` sample is never turned into a number. An API that returns only the value returns `Result`.
5. **Every output has a declared safe state.** There is no default value; a missing `safe` is a validation error.
6. **The cycle domain does not allocate, block or `await`.** Memory is pre-allocated at startup and locked (`mlockall`). Locks are forbidden; only lock-free structures with a known bound are used.
7. **Telemetry never blocks control.** Full queues drop samples according to the declared policy, and every drop is counted.
8. **Every write, accepted or not, enters the decision log**, with its result and cause (M3).
9. **Core determinism:** the same trace gives the same bytes. No `HashMap` with observable order in outputs; use `BTreeMap` or `IndexMap`.
10. No `unwrap()` outside tests; no `unsafe` outside the `*-sys` crates, `joker-host/ffi` and `joker-micro/ffi` (the Joker Micro C ABI, only with the `c-abi` feature; ref/08 §2.11).
11. **Everything is in English** (ADR 0029): identifiers, docstrings, error messages, commit messages, pull requests, issues, `docs/`, `spec/` and `wit/`.
12. **Code has no comments.** Items are documented with docstrings only. Configuration files carry no comments either.

## 3. Map and ownership boundaries

| Front | Owns | Consumes |
|---|---|---|
| Core | `crates/joker-core/`, `crates/joker-expr/`, `crates/joker-units/` | `docs/01`, `docs/02`, `docs/16`, `docs/ref/00` |
| Manifest | `crates/joker-manifest/` (with `codes.toml`) | `joker-core`, `docs/03`, `docs/ref/01`, `docs/ref/02` (lock) |
| HAL and drivers | `crates/joker-hal/`, `crates/joker-drivers/*`, `crates/joker-driver-sdk/`, `crates/joker-jdp/`, `crates/joker-drvtest/` | `joker-core`, `docs/05`, `docs/ref/03`, `docs/ref/06` |
| Wasm host and SDK | `crates/joker-host/`, `crates/joker-sdk/`, `wit/` | `joker-core`, `docs/04`, `docs/ref/04`, `docs/ref/05` |
| ML (1.1) | `crates/joker-ml*/` | `joker-core`, `docs/06` |
| Store and link | `crates/joker-store/`, `crates/joker-link/` | `joker-core` |
| Daemon, API and CLI | `crates/jokerd/`, `crates/joker-api/`, `crates/joker-client/`, `crates/joker-cli/`, `spec/openapi/` | everything; `docs/07`, `docs/ref/02`, `docs/ref/09` |
| Agent | `crates/joker-agent/`, `crates/joker-agent-proto/`, `spec/cddl/agent.cddl` | `docs/20`, `docs/ref/10` |
| Joker Micro and Link | `crates/joker-micro/`, `crates/joker-link-proto/`, `spec/c/`, `spec/cddl/joker-link.cddl`, `spec/cddl/joker-micro.cddl` | `joker-core`, `docs/19`, `docs/ref/07`, `docs/ref/08` |
| Classes (`jkc`) | `crates/joker-jkc/`, `classes/std/` | `docs/17` |
| Conformance | `conformance/`, `crates/joker-spec-tests/` | only the spec (mainly `docs/16` and `docs/ref/`) |

The full list of references each front owns is in document 13, §2.

`joker-core` and `wit/` are the contention points. Changes there are small, isolated in their own commit and made before the code that uses them. Changing `wit/` or `spec/` requires an ADR (M21, ADR 0023), and the matching reference changes in the same commit.

The conformance front writes its cases from the specification only and never reads the implementation: sharing code is sharing bugs.

## 4. Version control (jj) and tracking

- jj repository colocated with git; remote `origin` is `github.com/m4nd0r4s/joker`. One agent = one workspace: `jj workspace add ../joker-<front> -r main`.
- **Identity:** every commit is authored by the project owner's personal identity, which is set in the repository's jj config. Check `jj config get user.email` in a new workspace before the first commit. Commits and pull requests carry no AI attribution (no `Co-Authored-By` trailer for tools, no "generated with" lines).
- **Atomic commits:** one logical change per commit, and every commit builds and passes the tests.
- Message: `type(scope): summary` in English, with a body that explains why. Types: `feat`, `fix`, `docs`, `test`, `refactor`, `build`, `chore`, `perf`.
- Work is tracked in the Linear project **Joker** (team AUR) through the `linctl` CLI. A pull request names the issue it closes (`AUR-123`) in its title or body; move the issue to In Progress when starting and to In Review when the pull request is open.

## 5. Before calling a change done

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --target thumbv8m.main-none-eabihf -p joker-units -p joker-expr -p joker-core -p joker-hal
```

- If you touched the semantics of the envelope, the arbiter or the process image, a case in `conformance/` covers it.
- If you touched the cycle domain, the latency test (`cargo xtask rt-bench`) did not regress more than 5% at p99.9.
