# joker-supply-chain

The checks of `docs/policies/dependencies.md` §1 that need code (ADR 0055): the dependency rules of document 13 §1, read from the graph of `cargo metadata`, and the toolchain rules of document 13 §5.1, read from the toolchain files.

- [`Workspace`]: the packages, the declared dependencies and the resolved graph of `cargo metadata --format-version 1`.
- [`check_graph`]: the dependency rules, as a list of [`Violation`]s (empty when the graph follows them).
- [`check_toolchains`]: the toolchain rules, on the root of a checkout.
- [`policy`]: the crate lists and allowlists of the policy, as constants.

The integration tests run both checks on this workspace; `cargo test -p joker-supply-chain` is the CI job.
