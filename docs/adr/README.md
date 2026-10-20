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
| [0036](0036-english-identifiers-in-the-specification.md) | English identifiers in the specification: illustrative names renamed, `"$value"` and `n_wide` |

Changing a decision = writing a new ADR that replaces it, and updating the affected documents in the same set of commits.
