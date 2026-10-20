# Joker

The wildcard runtime for edge devices. The same core on Linux (Raspberry Pi, RevPi, industrial PC) and on microcontrollers (Joker Micro), and a single abstraction: **typed signals with unit, quality and time**. It controls I/O, speaks industrial protocols and collects telemetry, with a native safety envelope between every app and the hardware; ML arrives in 1.1. It is part of a family with the registry (Hub) and the fleet control plane (Deck).

**Status:** under implementation, starting with the first vertical slice M0 (`docs/26-final-review.md` §6). License: Apache-2.0 OR MIT. Figures marked as *target* are estimates to be validated in Phases 0 to 2 (`docs/11-roadmap.md`).

## Proposed layout

```
docs/            design specification — the source of truth (starts at docs/README.md)
  adr/           architecture decisions; changing the design = writing an ADR
  ref/           normative references for every interface (starts at ref/00-conventions.md)
  examples/      guided walkthrough and complete projects
  track/         learning track, 11 parts on the bench of document 23: A (signals and the Linux node: LED, temperature, ESP32 over Modbus, faults, Pi Zero W), B (classes: std/ and your own), C (Joker Micro on the ESP32), D (fleet: prod, secrets, Deck)
  GLOSSARY.md    English term for every domain word, translation style, file-name map
crates/          Rust workspace (built up from Phase 0 — see docs/13-implementation-plan.md)
  joker-units      units, dimensions, quantities
  joker-core       signals, process image, envelope, arbiter, expressions — no I/O
  joker-expr       the manifest expression language
  joker-manifest   parser and validation of joker.toml, joker.lock; code registry
  joker-hal        driver traits, read plans, transports (no_std)
  joker-drivers/*  modbus, opcua, gpio, i2c, spi, can, canopen, ethercat, revpi, mqtt, sim, v4l2…
  joker-driver-sdk SDK for tier W (Wasm) drivers in Rust
  joker-jdp        Joker Driver Protocol: tier X drivers (processes)
  joker-drvtest    driver conformance suite
  joker-host       wasmtime host, WIT world, capabilities
  joker-sdk        Rust app SDK (guest)
  joker-test       test harness with a virtual clock
  joker-micro      runtime for microcontrollers (Embassy); firmware, C library (ffi module), crate
  joker-link-proto Joker Link protocol (no_std)
  joker-jkc        compiler for .jkc classes
  joker-ml         backend trait, .jkm artifact, pipelines (v1.1)
  joker-ml-burn    Burn backend (the default)
  joker-ml-ort     ONNX Runtime backend
  joker-ml-hailo   HailoRT backend (plugin)
  joker-store      local historian and store-and-forward queue
  joker-link       uplinks: MQTT/Sparkplug B, Zenoh, OTLP
  joker-api        local API (HTTP + WebSocket)
  joker-client     Rust client library for the local API (the one jk uses)
  joker-agent      node ↔ Deck agent (agent/1)
  joker-agent-proto agent messages (no_std, shared with standalone MCUs)
  joker-spec-tests compiles and validates the references in docs/ref/ against the code (Phase 0, task 0a)
  jokerd           the daemon
  joker-cli        the CLI (the jk binary)
wit/             the WIT worlds: joker:runtime (apps), joker:driver (tier W drivers), joker:sim
spec/            machine-readable files of the references
  cddl/          CBOR schemas: joker-meta, joker-link, jdp, agent, joker-micro, secrets
  openapi/       joker-api.yaml (local HTTP API, OpenAPI 3.1)
  c/             joker_node.h (Joker Micro ABI), jdp.h (tier X drivers), example
conformance/     conformance cases for the envelope, the arbiter and the process image, one file per suite
tools/
  spec-lint/     checker for broken remissions and leftover Portuguese in the spec
```

## Develop

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python3 tools/spec-lint/spec_lint.py --links-only
```

Working rules, for people and agents: `AGENTS.md`.
