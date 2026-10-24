# ADRs

| # | Decision |
|---|---|
| [0001](0001-rust-for-the-runtime.md) | Rust for the runtime |
| [0002](0002-apps-as-wasm-components.md) | Apps are WebAssembly components |
| [0003](0003-the-signal-as-the-single-abstraction.md) | The signal is the single abstraction |
| [0004](0004-process-image-and-two-domains.md) | Process image, two execution domains, two instances per app |
| [0005](0005-no-write-bypasses-the-envelope.md) | No write path bypasses the envelope |
| [0006](0006-mandatory-safe-state.md) | Mandatory safe state, with no default |
| [0007](0007-clock-as-input.md) | The clock is an input of the core |
| [0008](0008-drivers-in-three-tiers.md) | Drivers in three tiers |
| [0009](0009-burn-as-default-backend.md) | Burn as the default ML backend, not the exclusive one |
| [0010](0010-toml-manifest-and-lock.md) | Declarative TOML manifest and `joker.lock` |
| [0011](0011-arbiter-with-priorities-and-leases.md) | Arbiter with priorities and leases |
| [0012](0012-shadow-before-actuating.md) | Shadow mode as the only promotion path for what actuates |
| [0013](0013-telemetry-never-blocks.md) | Telemetry never blocks control |
| [0014](0014-native-pipelines.md) | Native pipelines for bulk data |
| [0015](0015-apply-with-confirmation.md) | Apply configuration with confirmation and automatic rollback |
| [0016](0016-one-process.md) | One process, not microservices |
| [0017](0017-equipment-classes.md) | Equipment classes and instances, on top of signals |
| [0018](0018-one-format-per-type.md) | One format per kind of thing: TOML, `.jkc` and code |
| [0019](0019-ml-in-1-1.md) | ML in 1.1, with v1 prepared for it |
| [0020](0020-no-std-core-and-joker-micro.md) | A `no_std` core from day one, and Joker Micro in v1 |
| [0021](0021-local-first-family.md) | A product family, local-first, with open contracts |
| [0022](0022-versioning-policy.md) | Versioning policy: themed minors, full compatibility in 1.x, LTS at the end of the series |
| [0023](0023-normative-references.md) | Interfaces are closed in normative references (`docs/ref/`) |
| [0024](0024-per-recipient-secrets.md) | Secrets encrypted per recipient, versionable in Git |
| [0025](0025-identity-and-signatures.md) | P-256 identity on nodes, Ed25519 in the organization, COSE for everything that is signed |
| [0026](0026-type-in-the-file-name.md) | The type of a file is in its name (`<name>.<type>.toml`, `joker.<variant>.toml`) |
| [0027](0027-composite-classes.md) | Composite classes end to end: binding, nested paths, methods, health, profiles and Joker Micro |
| [0028](0028-stable-equals-implemented.md) | Stable = implemented; what does not ship in 1.0 stays reserved |
| [0029](0029-english-project-language.md) | English is the project language; code has docstrings only, no comments |
| [0030](0030-unit-syntax-and-kinds.md) | Unit syntax, quantity kinds in compounds, and literal error codes |
| [0031](0031-registry-hash-encoding.md) | Signal ordering and the registry hash encoding |
| [0032](0032-dt-after-overrun-skip.md) | `dt()` counts whole periods, also after `overrun = "skip"` |
| [0033](0033-write-outcomes-and-decision-records.md) | Write outcomes per proposal and per tick, and decision records |
| [0034](0034-expression-typing-and-evaluation.md) | Expression typing, `J0304` and `J0307`, and the semantics of windows and stateful functions |
| [0035](0035-reconcile-the-references-with-the-1-0-cut.md) | Reconcile the references with the 1.0 cut: what the cut does not keep is reserved, and the planning decisions it left open |
| [0036](0036-english-identifiers-in-the-specification.md) | English identifiers in the specification: illustrative names renamed, `"$value"` and `n_wide` |
| [0037](0037-trace-v1-encoding-details.md) | Encoding details of the `.jktrace` v1 format |
| [0038](0038-envelope-steps-2-to-6.md) | Details of envelope steps 2–6 |
| [0039](0039-atomic-groups-in-the-envelope.md) | Atomic groups in the envelope |
| [0040](0040-alarm-lifecycle-and-unknown.md) | Alarm lifecycle: delays, `Unknown`, suppression and shelving |
| [0041](0041-method-expansion-and-queries-in-the-core.md) | Method expansion, `own()` and queries in the core |
| [0042](0042-core-case-initial-state-and-staging.md) | Initial state and staging of a core conformance case |
| [0043](0043-causal-sets-and-method-records-in-the-trace.md) | Causal sets, and method records in the trace |
| [0047](0047-m0-driver-contract-subset.md) | The M0 subset of the driver contract (`joker-hal`) |
| [0048](0048-rt-admission-and-memory-estimate.md) | Real-time admission: the formula, what the WCET contains, and the memory estimate |
| [0049](0049-bus-budget-beyond-modbus-rtu.md) | The bus budget beyond Modbus RTU |
| [0051](0051-m0-sim-driver.md) | The M0 `sim` driver: how the model reaches it, the format subset and its time |
| [0052](0052-aot-cache-identity-and-cycle-subset-host.md) | AOT cache identity, load errors, WASI terminals and the seed expansion |
| [0053](0053-sdk-error-size-and-the-sys-crate.md) | The size of the SDK error, and the `joker-sdk-sys` crate |
| [0054](0054-complete-sim-driver.md) | The complete `sim` driver: what reaches it, the dynamics language, actions and the plant surface |
| [0055](0055-supply-chain-and-dependency-policies.md) | Supply chain and dependency policies: CI checks, core allowlist, MCU licences, `cargo vet`, toolchains and the wasmtime LTS line |
| [0056](0056-allocation-free-cycle-calls.md) | Allocation-free cycle calls: reserved buffers of `on-cycle`, the trapping allocator and `J-E0903` |
| [0057](0057-the-cli-may-embed-the-daemon.md) | `jk` may embed the daemon; no library depends on it |
| [0058](0058-action-step-interpreter-and-decision-log-entries.md) | The action step interpreter lives in `joker-hal`; a new decision-log record kind for actions |
| [0060](0060-fail-closed-permits-and-step-quality.md) | Action permits fail closed; step conditions honour quality |

Changing a decision = writing a new ADR that replaces it, and updating the affected documents in the same set of commits.
