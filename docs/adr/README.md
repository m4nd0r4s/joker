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
| [0059](0059-jkc-diagnostics-and-canonical-contract.md) | `jkc` diagnostics, the canonical contract and its hashes, and version compatibility |
| [0060](0060-fail-closed-permits-and-step-quality.md) | Action permits fail closed; step conditions honour quality |
| [0061](0061-runtime-state-files-supervisor-records-and-bulk-reset-errors.md) | Runtime state files, supervisor and restore records, and per-output errors of a bulk reset |
| [0062](0062-wide-signal-values.md) | Wide signal values: the wide slot, decoding, the envelope, the API, the decision log and the trace |
| [0063](0063-socket-roles-subscription-limits-and-device-counters.md) | Socket roles, subscription limits and device counters in status and metrics |
| [0064](0064-profile-acquisition-subset-and-refused-keys.md) | The profile acquisition stage of 1.0, and profile keys this runtime refuses |
| [0065](0065-device-write-acks-in-the-decision-api.md) | Device write acks in the decision API, and what confirms a rejected output |
| [0066](0066-device-and-instance-actions-invocations-and-their-conformance-area.md) | Device and instance actions: the invocation lifecycle, and the `action` conformance area |
| [0067](0067-apps-reach-the-node-as-artifacts.md) | How Wasm apps reach a node, run from its AOT cache, and report traps |
| [0068](0068-expression-evaluation-groups-and-window-periods.md) | Evaluation groups and window periods of manifest expressions |
| [0069](0069-clock-quality-re-evaluation.md) | How `jokerd` derives and re-evaluates the clock quality |
| [0070](0070-state-write-failures-orphaned-trips-boot-count-and-jokerd-flags.md) | State write failures, trips of outputs a generation removed, the startup counter in the API, and `jokerd`'s bench flags |
| [0071](0071-prometheus-metrics-exposition.md) | The Prometheus `/metrics` exposition: endpoint, role, names, labels and summaries |
| [0072](0072-output-envelope-keys-in-jokerd.md) | Every output envelope key takes effect in `jokerd`: step 6, `verify`, `keepalive` and `pulse` |
| [0073](0073-raw-access-in-the-catalog-drivers-and-the-wire-double.md) | Access by address in the catalog drivers, and the wire double of the action suites |
| [0074](0074-subscription-resume-re-resolution-and-sampling-budget.md) | Subscription resumption, re-resolution on a generation swap, and the sampling budget |
| [0075](0075-manifest-signal-adjustments-and-the-output-stage.md) | Manifest signal adjustments take effect, and the output stage converts values to raw |
| [0076](0076-modbus-register-outputs-and-bit-read-modify-write.md) | Modbus register outputs, and a `bool` output in a register bit by read-modify-write |
| [0077](0077-sunspec-discovery-over-modbus.md) | SunSpec discovery in the Modbus drivers, its effective map, and the profile generated from it |
| [0078](0078-revpi-driver-over-picontrol.md) | The `revpi` driver over piControl |
| [0079](0079-data-classes-and-egress.md) | Data classes and egress protection (D1–D4) |
| [0080](0080-how-the-references-are-compiled.md) | How `joker-spec-tests` compiles the references (task 0a) |
| [0081](0081-historian-storage-wal-segments-and-retention.md) | Historian storage: WAL, segments, checkpoints and retention |
| [0082](0082-alarm-journal-persistence-and-historian-wiring.md) | The alarm journal, alarms across restarts, and the historian in jokerd |
| [0083](0083-the-native-mode-of-joker-test.md) | The native mode of `joker-test` |
| [0084](0084-class-instances-on-the-node-and-manual-method-calls.md) | Class instances on the node, class queries as derived signals, and manual method calls |
| [0085](0085-runtime-alarms-in-jokerd.md) | The runtime alarms (`joker.*`) in jokerd |
| [0086](0086-the-hung-group-alarm.md) | The hung-group alarm (`joker.group.<g>.stalled`) |
| [0087](0087-figures-per-hardware-class-and-locked-memory.md) | The figures of document 01 §21 per hardware class, and memory as `jokerd` locks it |
| [0088](0088-the-scalar-slot-of-a-wide-signal-carries-its-quality.md) | The scalar slot of a wide signal carries its quality (amends ADR 0062 decision 2) |
| [0089](0089-one-thread-per-cycle-group.md) | One thread per cycle group: per-group state, cross-group alarm trips, priorities and the supervisor's narrowed response (amends ADR 0061 decision 6) |
| [0090](0090-the-runtime-signals-on-linux.md) | The runtime's own signals on Linux (`joker.*`), `leases_expiring`, `Metrics.runtime.dropped` and the measured WCET of `GET /v1/groups` |
| [0091](0091-jk-why-and-the-app-lifecycle-on-the-node.md) | `jk why <output>`, and the app lifecycle on the node: stop, start and restart in `jokerd` 1.0 |
| [0092](0092-node-hardware-storage-and-board-probe.md) | The root medium in the hardware inventory, `jk board probe` and `jk board check --node`, and what a reference board promises |
| [0093](0093-the-linux-gpio-driver-on-chardev-v1.md) | The Linux `gpio` driver on chardev v1: `edges` and `timestamp` reserved (`J0807`), `debounce` by the acquisition stage |
| [0094](0094-seeded-cddl-generation-and-signed-64-bit-integers.md) | Seeded CDDL generation in `joker-spec-tests`, and signed 64-bit integers in the CDDL files (amends ADR 0080 decision 3) |
| [0096](0096-json-timestamps-in-agent-cddl.md) | The JSON form of `timestamp` in `agent.cddl`, and the `ota-begin` example of ref/07 §9.3 |
| [0097](0097-the-manifest-model-of-joker-manifest.md) | The manifest model of `joker-manifest`: schema as data and a typed model (ref/01 §12) |

Changing a decision = writing a new ADR that replaces it, and updating the affected documents in the same set of commits.
