# 15 — File map and vocabulary

An index of every file a Joker project has, who writes it, when, and where it is defined. The "Defined in" column points to the document that explains it and to the reference in `docs/ref/` that closes the format. And, before that, the vocabulary that tends to confuse: "device" means one precise thing, and it is neither the board nor the class.

---

## 1. Vocabulary: board, device, profile, instance, class

In the track there are four different "physical things", and each has its own name:

| Term | What it is | Example in the track | Where it is declared |
|---|---|---|---|
| **Board** | the computer model Joker runs on | the Raspberry Pi 3 B+ | `[target] board = "rpi3b-plus@1"` in the manifest; described by a **board profile** |
| **Node** | a concrete board with `jokerd`, identified and registered | `lab` (`lab.local`), the Pi 3 on the bench | `jk node enroll`; chosen with `--node` in the CLI |
| **Bus** | a shared communication medium | the serial port of the ESP32's USB bridge (`[buses.esp32]`) | `[buses.<name>]` in the manifest |
| **Device** | **a thing Joker talks to**, through a driver | the `pi` (the LEDs and the temperature, through `sysfs`), the `esp` (the ESP32 over Modbus RTU, unit 1) | `[devices.<name>]` in the manifest |
| **Profile** | the description of a device **model**: registers, scales, units, fallback | `local/bench-esp32@1` | one file per model, in `profiles/` or in the registry |
| **Signal** | a quantity of a device (or computed) | `pi.cpu_temp`, `esp.led` | in the profile, in the simulation model or in the manifest |
| **Class** | a **type** of equipment, for programs | `temp_sensor@1`, `led@1` | one file per class, in `classes/` or in the registry |
| **Instance** | a concrete piece of equipment that fulfills a class | `cpu` (a `temp_sensor@1`), `led_pi`, `sinal` (a virtual `status_light@1`) | implicit (profile), in the manifest (`[instances.*]`) or virtual (app) |

Two relations that need to be clear:

- **Device ≠ instance.** The device is *who you talk to* (address, driver, protocol). The instance is *what that thing is for the program*. Sometimes they coincide: the track's ESP32 is the device `esp` **and**, through the profile, the implicit instance `esp` (a `std/led@1`, a `std/button@1` and a `std/analog_in@1`). Other times they do not: the `ACT` LED is a signal of the device `pi` (`sysfs`), and the instance `led_pi` was composed in the manifest (track, part 7). A real pump is an instance made of signals from three devices.
- **There is no file per device.** Devices are sections of the manifest. Files are per **model**: one profile serves every device of that model (ten identical meters = one profile), and one board profile serves every board of that model.

---

## 2. The files of a project

**The type is in the name** (ADR 0026). Every TOML file is written `<name>.<type>.toml` (`.profile.toml`, `.board.toml`, `.sim.toml`, `.scenario.toml`, `.conformance.toml`, `.fixture.toml`, `.driver.toml`, `.package.toml`); the manifest layers are called `joker.toml` or `joker.<variant>.toml`, in any directory, and the locks `joker.lock` or `joker.<variant>.lock`. The content must match the suffix (`J0803`, ref/03 §2.1.6). The configuration files of `jk`, of the node and of the Deck have fixed names (`config.toml`, `node.toml`, `deck.toml`, `fleet.toml`). The complete table is in ref/00 §2.

### 2.1 Written by people

| File | Format | What it describes | Who writes it | When it changes | Defined in |
|---|---|---|---|---|---|
| `joker.toml` | TOML | **the site**: board, buses, devices, outputs and envelopes, cycles, apps and bindings, instances, models, pipelines, `calc`, alarms, telemetry, bridges, security | integrator / site engineer | always; it is the main file | 03, ref/01 |
| overlays `joker.<variant>.toml` (`joker.sim.toml`, `joker.rpi.toml`, `templates/joker.bess-v2.toml`, `sites/lisboa-01/joker.verao.toml`) | TOML, with `extends` | variations of the manifest: simulator vs hardware, shared template vs concrete site; chosen with `--env <variant>` | integrator | when the same system has more than one environment or site | 03 §2, ref/01 §7 |
| `profiles/<vendor>/<model>@<major>.profile.toml` | TOML | a **device model**: registers, types, scales, units, limits, enums, coherence, fallback, actions, `implements` | hardware integrator | when a piece of equipment appears that is not yet in the registry | 05 §6, ref/03 §2.2 |
| `boards/<board>.board.toml` | TOML | a **board model**, with `version`: recommended cores, watchdog, fixed IO (with `fallback`), serial ports, accelerators; on an MCU, flash, FPU, capabilities and probe | whoever supports a new board (rare) | almost never: they come from the registry | 05 §11, ref/03 §2.3 |
| `classes/<package>/<name>.jkc` | `.jkc` (document 17) | a **class**: members, capabilities, methods, actions, events, queries, health | abstraction author | when a new equipment type is created | 17, ref/03 §2.7 |
| `classes/<package>/<package>.package.toml` | TOML | a **class package**: name, license, authors, dependencies (the `local` package does not need it) | abstraction author | when a package is published | 17 §3.2, ref/03 §2.7 |
| `classes/<package>/<name>.conformance.toml` | TOML | the tests that any implementation of the class must pass (the single conformance schema) | abstraction author | with the class | 14 §7.3, ref/03 §2.6 |
| `classes/<package>/<name>.sim.toml` | TOML | the **simulator** of a class, to run conformance and apps without hardware | abstraction author | optional, with the class | ref/03 §2.4.11 |
| `sim/<model>.sim.toml` | TOML | a **simulated device**: signals, generators, dynamics, faults, panel, `implements` | the developer | while there is no hardware, and for tests | 04 §6, ref/03 §2.4 |
| `sim/scenarios/<name>.scenario.toml` | TOML | a **site test scenario**: actions over time and expectations | the developer | with each new behavior | 04 §6, ref/03 §2.5 |
| `apps/<app>/` (`Cargo.toml` + `src/`, or `package.json`, or `pyproject.toml`) | code | an **app**; ports, tasks and parameters are declared in the code, without `app.toml` | programmer | always | 04, ref/04, ref/05 |
| `keys/*.pub` | public keys | keys the project uses: anchors and per-role keys (`runtime`, `config`, `apps`, `drivers`, `secrets`…); the manifest only **restricts** the node's trust bundle | security officer | at the start, and on rotations | 03 §13, ref/02 §2.5 |
| `.joker/config.toml` | TOML | local client configuration for the project: default node and manifest (or variant) (not version-controlled; the user's one is in `~/.config/joker/config.toml`) | the developer | rarely | ref/02 §2.3 |
| `.taplo.toml` | TOML | associates each suffix (`*.profile.toml`, `joker*.toml`…) with the JSON schema of its type, for completion and errors in the editor | `jk init`, `jk new`, `jk migrate` (version-controlled) | almost never | ref/03 §2.1.6 |

### 2.2 Generated, but version-controlled

| File | Generated by | What it pins | Version-control it? |
|---|---|---|---|
| `joker.lock` and `joker.<variant>.lock` | `jk lock`, `jk bench`, `jk build` (digests of local apps) | exact versions of boards, profiles, classes, apps and drivers; discovered maps (SunSpec), firmware, verified fallbacks, measured WCET, approved exceptions (`widen`, `accept_missing_fallback`) | **yes**: it is what makes the site reproducible and detects drift (ref/02 §2.1) |
| `secrets.toml.enc` | `jk secret` | the project's secrets, each encrypted for the recipient nodes and for the escrow people, signed and with `rev` | **yes** (ADR 0024): it only contains encrypted values, and that is how the Deck distributes them without reading them (ref/02 §2.4) |

**One lock per input manifest**, in the same directory: `joker.lock` for `joker.toml` and `joker.<variant>.lock` for each input overlay (`joker.rpi.toml` → `joker.rpi.lock`, which is the lock of `jk <command> --env rpi`; `sites/lisboa-01/joker.toml` → `sites/lisboa-01/joker.lock`). An overlay used only through `extends` (a template) has no lock (ref/02 §2.1.1).

### 2.3 Artifacts (produced, distributed, not edited)

Every distributed artifact has a media type, and the types are in a single table, with the signing role of each one: ref/00 §7.1.

| Artifact | Produced by | Content | Media type | Defined in |
|---|---|---|---|---|
| `*.wasm` | `jk build` | the component of an app, with the `joker:meta` section | `joker.app.v1+wasm` | 04 §2, ref/04 §5 |
| `*.cwasm` | `jk build --target` or installation | the AOT for an architecture | `joker.app.v1+cwasm` | 01 §9 |
| tier W driver (`*.wasm`) or tier X driver (multi-platform index) | driver build | a driver, with its descriptor | `joker.driver.v1+wasm`, `joker.driver.v1+tar` | ref/06 §4.7, §5.9 |
| `*.jktrace` | `jk record`, `jk trace-dump` | a reproducible trace | `joker.trace.v1` | 16 §7 |
| Joker Micro firmware (ELF/UF2/bin) | `jk build --target mcu:…` (`--generic` for the generic one) | runtime + apps + drivers for an MCU | `joker.micro-firmware.v1` | 19 §4, ref/08 §2.12 |
| `firmware.jkfw` | `jk build --target mcu:… --generic` | descriptor of the generic firmware: board, capabilities, format and ISA it accepts | (with the firmware) | ref/08 §2.14 |
| `<site>.jkgen` | `jk build --target mcu:… --generation --firmware <f.jkfw>` | the generation blob of an MCU, compiled and signed (`config` role) | `joker.generation.v1` | 19 §4.1, ref/08 §2.13 |
| `libjoker_node.a`, `joker_node.h`, `joker_project.h` | `jk build --target mcu:… --lib` | Joker as a C library: the fixed ABI header and the generated project header | — | 19 §4.2, ref/08 §2.2, §2.8 |
| crate `joker-project` | `jk build --target mcu:… --crate` | the typed interface of the project for Rust firmware | — | ref/08 §2.11.4 |
| `*.contract.json` | `jkc` | the canonical form of a class; it goes with each generation to the node | `joker.class.contract.v1+json` | 17 §6 |
| conformance report | `jk class verify --report`, `jk driver test --report` | the signed result of a suite, published next to the artifact | `joker.conformance-report.v1+json` | ref/03 §2.8, ref/06 §6.5 |
| generation of a Linux node | `jk apply` (sent to the node) or `jk publish` (published to the registry, for the Deck) | effective manifest, lock, contracts and artifacts by digest, signed by `jk` with the `config` role key. **The Deck neither produces nor signs generations**: it does not have the organization keys; it computes the descriptor from Git and looks up in the registry the generation signed with that descriptor (`J0915` if it is missing; document 20, §4.4.2) | `joker.config.v1+cbor` | ref/10 §2.6, 07 §3 |
| `*.jkm` | `jk model build` (1.1) | a model: `model.toml` (signature and guard), `lineage.toml`, `signature.sig` (COSE_Sign1, ADR 0025), `payload/` | `joker.model.v1+jkm` (reserved) | 06 §2 |

### 2.4 Secrets

| File | What it is | Rule |
|---|---|---|
| `secrets.toml.enc` | secrets (broker credentials, APIs), each encrypted with HPKE for the KEM key of each recipient node and signed | **can be version-controlled** (ADR 0024); the manifest refers to them by name (`secret:<name>`), and apps by a local name in `[apps.<app>.secrets]` |
| plaintext file for `jk secret import` | the input of an import | **never** in git; deleted after importing |
| `.joker/secrets.dev.toml` | plaintext secrets only for the local runtime of `jk dev` | never in git (`.joker/` is in `.gitignore`) |
| private signing and escrow keys | in `~/.local/share/joker/keys/`, in an HSM or `pkcs11:` | never in the project (ref/02 §2.5.1) |

### 2.5 On the device

**Files are not edited on the device.** Everything arrives through `jk apply` (configuration, artifacts and secrets), through `jk secret push`, through the Deck (which distributes what `jk publish` signed), or through `jk node enroll` (identity). The runtime state lives in `/var/lib/joker/` and is managed by `jokerd`. Editing there by hand is equivalent to a non-version-controlled patch, and `jk doctor` gives `J0907` if it finds differences.

**The exception is `/etc/joker/node.toml`**, the node's startup configuration: only what is needed before the first generation, what must survive a bad generation (the minimal API listener, the minimal administrators, the trust anchors, the mode policy) and what belongs to the physical installation. Even this file **is not edited by hand**: only `jokerd` writes it, at enrollment and on anchor replacement (`jk node trust set`). The generation only adds to it, never removes from it (ref/02 §2.2).

| Where | What |
|---|---|
| `/etc/joker/` | `node.toml`, `keys/*.pub` (anchors), optional `enroll-token` |
| `/run/joker/` | `api.sock`; `enroll-code` while the node has no identity |
| `/var/lib/joker/identity/`, `trust/` | node certificate and key, KEM key, active trust bundle |
| `/var/lib/joker/generations/<gen>/` | one directory per generation (signed document, effective manifest, lock, contracts); `current` and `confirmed` are links |
| `/var/lib/joker/artifacts/`, `aot/` | content-addressed store and AOT |
| `/var/lib/joker/secrets/` | received secrets, still encrypted |
| `/var/lib/joker/state/`, `kv/`, `store/`, `trace/`, `link/` | trips, alarms, intents, the apps' `kv`, historian and decision log, trace rings, uplink cursors |
| `/var/lib/joker/mcu/`, `runtime/a\|b/` | firmware and blobs waiting for the MCUs; A/B slots of `jokerd` |

The complete tree, with the owner and permissions of each entry, is in ref/02 §2.2.4.

### 2.6 In the Joker repository: `wit/` and `spec/`

They are not files of a project, but they are the ones that define the interfaces a project uses. They are part of the references (ADR 0023): if one of them and the text of the reference diverge, it is a bug in both.

| File | What it is | Reference |
|---|---|---|
| `wit/joker-runtime.wit` | the apps' world (`joker:runtime`) | ref/04 |
| `wit/joker-driver.wit` | the world of tier W drivers (`joker:driver`) | ref/06 §4 |
| `wit/joker-sim.wit` | the Wasm simulated plants of the `sim` driver | ref/03 §2.4.10 |
| `spec/cddl/joker-meta.cddl` | the `joker:meta` section of apps | ref/04 §5 |
| `spec/cddl/joker-link.cddl` | the Joker Link messages | ref/07 |
| `spec/cddl/joker-micro.cddl` | the `.jkgen` blob and the `.jkfw` descriptor | ref/08 |
| `spec/cddl/jdp.cddl`, `spec/c/jdp.h` | the protocol of tier X drivers | ref/06 §5 |
| `spec/cddl/agent.cddl` | the protocol of the `agent/1` agent | ref/10 |
| `spec/cddl/secrets.cddl` | the sealed set of secrets for a node | ref/02 §2.4 |
| `spec/openapi/joker-api.yaml` | the local HTTP API (OpenAPI 3.1) | ref/09 |
| `spec/c/joker_node.h`, `spec/c/example/` | the C ABI of Joker Micro and an example with `joker_project.h` | ref/08 |

---

## 3. Who touches what

Most people only change three or four of these files:

| Person | Files |
|---|---|
| **Site integrator** | `joker.toml` and overlays, the locks (via `jk lock`), `secrets.toml.enc` (via `jk secret`), sometimes a profile |
| **Hardware integrator** | `profiles/**`, rarely `boards/**` |
| **Abstraction author** | `classes/**/*.jkc` (definition + conformance), `apps/<implementation>/` |
| **Business programmer** | `apps/<app>/`, `sim/scenarios/**`, sometimes `sim/*.sim.toml` |
| **Operator** | none: uses `jk` (write with TTL, alarms, intents) |

---

## 4. From the empty directory to the complete system

The same map applied to the learning track (project `joker-bancada/`), part by part. The complete list, with what each file is at the end, is in `track/README.md`, "The directory, at the end of the track".

| Part | New or changed files |
|---|---|
| 1 | `joker.toml` (common) and `joker.sim.toml` (overlay) from the start, `joker.sim.lock`, `sim/pi.sim.toml`, `sim/scenarios/pisca.scenario.toml`, `apps/luz/` (0.1); outside the project, `.joker/config.toml` |
| 2 | `joker.lab.toml` (board `rpi3b-plus@1`, `[resources]`, the `pi` through `sysfs`), `joker.lab.lock`; outside the project, `~/org-ca/` and the `lab` context in `~/.config/joker/config.toml` |
| 3 | `sim/pi.sim.toml` (`carga`, `cpu_temp`), `joker.toml` (`[calc]`, `[alarms.cpu_quente]`, `[telemetry.record]`), `joker.lab.toml` (`pi.cpu_temp`), `sim/scenarios/aquece.scenario.toml`, `apps/luz/` (0.2) |
| 4 | `mapa-esp32.csv`, `profiles/local/bench-esp32@1.profile.toml`, `sim/esp.sim.toml`, `joker.sim.toml` and `joker.lab.toml` (`[buses.esp32]` with `dtr`/`rts`, `[devices.esp]`), `joker.toml` (`[outputs."esp.led"]`, debounce), `joker.lab.lock` (profile, `[fallback]`, `[discovered]`), `sim/scenarios/botao.scenario.toml`, `apps/luz/` (0.3) |
| 5 | `joker.toml` (`[signals."esp.touch"]`, `calc.toque_nivel`, `[alarms.esp_offline]`), `sim/esp.sim.toml` (fault key), `sim/scenarios/falha_esp.scenario.toml`, `apps/luz/` (0.4 and 0.4.1), `joker.lab.lock` |
| 6 | `joker.rpi.toml` (the `[devices.pi]` leaves `joker.lab.toml`), `joker.zero.toml`, `joker.zero.lock`, `boards/rpi-zero-w.board.toml`; `joker.lab.toml` now extends `joker.rpi.toml` |
| 7 | `joker.toml` (five composite `[instances.*]`, `bind` of `luz`), `profiles/local/bench-esp32@1.profile.toml` and `sim/esp.sim.toml` (`[implements]`), `apps/luz/` (0.5), the three locks |
| 8 | `classes/local/status_light.jkc` (+ `status_light.conformance.toml`), `apps/status_light_ctl/`, `joker.toml` (virtual instance `sinal`, its outputs, `order` of `main`), `apps/luz/` (0.6), `sim/scenarios/pisca.scenario.toml`, the three locks |
| 9 | `micro/esp/joker.toml`, `joker.lab-link.toml`, `joker.lab-link.lock`, `secrets.toml.enc` (recipient `node:esp`) |
| 10 | `micro/esp/apps/reflexo/`, `micro/esp/joker.lock` (`[wcet]` of the chip), `micro/esp/joker.toml` (`[net.wifi]`, `[link]` over UDP), `joker.lab-link.toml`, `secrets.toml.enc` (`wifi_bancada`) |
| 11 | `keys/*.pub`, `fleet.toml`, `secrets.toml.enc` (`mqtt_bancada`), `joker.toml` (`mode = "prod"`, `[telemetry.uplink]`, `[security]`, `accept_missing_fallback`), `[accepted]` in the locks, `apps/luz/` (0.7 and 0.7.1); outside the project, `~/deck/deck.toml` |

---

## 5. Why there are so many, and why not fewer

Each file exists because it has **a different owner** or **a different life**:

- the **profile** belongs to an equipment model and is reused across all sites; if it lived in the manifest, each site would have its own copy of the registers of the same meter;
- the **class** is a contract between many apps and many profiles; it does not belong to any site;
- the **lock** changes when the world changes (firmware, discovery), and not when someone edits;
- the **scenarios** are tests and change with behavior, and not with hardware;
- the **overlays** separate what the system does (common) from where it is (sim or hardware, site A or B).

Merging them would reduce the number of files and increase the number of copies. It is the same separation Cargo makes between `Cargo.toml`, `Cargo.lock` and the crate registry.
