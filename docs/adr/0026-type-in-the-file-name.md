# ADR 0026 — The type of a file is in its name

**Status:** accepted (v1) · **Date:** 2026-09 · **Replaces** the ref/03 decision that "the type of a file comes from whoever refers to it"

## Context

A project has many TOML files of different types: manifests and overlays, equipment profiles, board profiles, simulation models, scenarios, conformance suites, class packages. The type of each one was only known from the directory it was in or from whoever referred to it. Whoever opens an `esp.toml` or an `rpi.toml` does not know whether they are reading a profile, a simulation or an overlay; in the track there was a `sim/pi.toml` (simulation) and an `rpi.toml` (overlay) side by side.

## Decision

1. **Each file states its type in its name**, in the form `<name>.<type>.toml`:

   | Type | Name | Example |
   |---|---|---|
   | manifest | `joker.toml` | `joker.toml`, `micro/esp/joker.toml` |
   | overlay | `joker.<variant>.toml` | `joker.sim.toml`, `joker.lab.toml`, `templates/joker.bess-v2.toml` |
   | lock | `joker.lock`, `joker.<variant>.lock` | `joker.lab.lock` |
   | equipment profile | `<model>@<major>.profile.toml` | `profiles/local/bench-esp32@1.profile.toml` |
   | board profile | `<board>.board.toml` | `boards/rpi-zero-w.board.toml` |
   | simulation model | `<name>.sim.toml` | `sim/esp.sim.toml`, `classes/std/valve.sim.toml` |
   | scenario | `<name>.scenario.toml` | `sim/scenarios/esp_fault.scenario.toml` |
   | conformance suite | `<name>.conformance.toml` | `classes/local/status_light.conformance.toml`, `conformance/envelope/rate.conformance.toml` |
   | class package | `<package>.package.toml` | `classes/acme/water/water.package.toml` |
   | driver fixture | `<driver>.fixture.toml` | `drivers/modbus/conformance/modbus-tcp.fixture.toml` |
   | driver descriptor (source of `jk driver meta`) | `<driver>.driver.toml` | `joker-drv-acme.driver.toml` |
   | class | `<name>.jkc` | (no change) |
   | node, client, Deck, fleet, secrets, code registry | fixed names | `node.toml`, `config.toml`, `deck.toml`, `fleet.toml`, `secrets.toml.enc`, `codes.toml` |

2. **The content must match the suffix.** Each type is recognized by its identification table: `[profile]` (profile), `[board]` (board), `[scenario]` (scenario), `[suite]` (conformance), `[package]` (package) and `[fixture]` (driver fixture), all required. Three types have no required table, and are recognized by the keys that characterize them: the **driver descriptor** has `name`, `tier` and `protocols` at the root (ref/06 §2.3); the **simulation model** (`[model]` is optional) has at least one of `[model]`, `[signals]` or `[plant]`; a **manifest layer** (`[project]` is only required in the effective manifest, ref/01 §2.3) only has manifest sections (ref/01 §4) and has no `schema` at the root. A file without the table of its suffix, or with the identification table of another type, is `J0803` (ref/03 §2.1.6).
3. **References do not change.** The manifest still writes `profile = "local/bench-esp32@1"`, `board = "rpi-zero-w"` and `model = "sim/esp.sim.toml"`: profile and board resolution adds the suffix; file paths are written in full.
4. **Overlays are chosen by variant.** `jk <command> --env lab` (or `JK_ENV=lab`) is the same as `--manifest joker.lab.toml`, and the corresponding lock is always `joker.lab.lock`. The `joker.<variant>.toml` rule applies in any directory (`templates/`, `sites/<site>/`), and an `extends` only accepts files with that name. `--manifest` still exists for arbitrary paths: an input manifest with a name outside the rule is accepted with the warning `J0169` (ref/01 §7.7).
5. **Editors know the schema from the name.** `jk` publishes a taplo configuration that associates `*.profile.toml`, `*.board.toml`, `*.sim.toml`, `*.scenario.toml`, `*.conformance.toml`, `*.fixture.toml`, `*.driver.toml`, `*.package.toml` and `joker*.toml` with the JSON schema of each type, with completion and errors in the editor. `jk init` and `jk new` write those associations to a `.taplo.toml` at the project root (ref/03 §2.1.6), and `jk migrate` adds it to an old project.

## Reasons

- A file can be understood without context: in a listing, in a PR review, in a search.
- It was already the form of `.conformance.toml` and of the class simulators (`.sim.toml`); it becomes the rule.
- The suffix matches the Hub's media types (`joker.profile.v1+toml` ↔ `.profile.toml`).
- The manifests of a site stay together in a listing (`joker.*`).

## Consequences

- Longer names.
- Renaming of all the examples, the track and the references (done in this revision).
- `jk migrate` renames the files of an old project and updates the `extends`, the `model =` and the `manifest =` of the scenarios (document 07, §2).
- The content ↔ suffix check becomes `J0803`, which used to cover "file referred to in the wrong place"; the old case is still there (a `model =` that points at a `.profile.toml` already fails by the suffix).
