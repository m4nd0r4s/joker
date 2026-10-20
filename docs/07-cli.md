# 07 — CLI (`jk`)

`jk` is the only point of contact for those who develop and operate, and it is also a client of the local API (document 01, §14): everything it does, an HMI or a script can also do. The library that `jk` uses is the `joker-client` crate (ref/05, part B).

**Where the detail is.** The endpoints each command uses are in the mapping table of ref/09, §9.1, and the exit codes in ref/09, §9.2. The client configuration, the node contexts, the lock, the secrets and trust are in ref/02. The agent commands and the enrollment through the Deck are in ref/10, §2.18. This document is the index of the commands; where a reference defines an option, the reference prevails.

## 1. Principles

1. **The same command on the laptop and on the device.** `--node <target>` (or `JK_NODE`) points at a `jokerd`. It accepts a context name from `[nodes]` in the client file, `local`, or a literal address: `unix:<path>`, `<host>:<port>` (TCP with mTLS; a name without a port, which is not a context, goes to 7443) or `ssh://[<user>@]<host>[:<port>]` (SSH tunnel to the node's Unix socket). The resolution is in ref/02, §3.3.1. Without a target, it uses the local runtime.
2. **Errors in the style of rustc:** code, location in the file, explanation, help. `jk explain J0107` shows the full page.
3. **Nothing dangerous without being explicit.** Manual writes require `--ttl` and `--reason` and show the envelope before applying; `apply` in `prod` requires `--confirm` when the generation touches network, security, uplinks or the API itself (ADR 0015). The commands that change the node show on the first line the node, the mode and where the choice came from (`→ lisboa-01 (prod) · from JK_NODE`). `--confirm`, `--yes`, `--ttl`, `--reason`, `--level`, `--reveal` and `--allow-dev` **have no environment variable**, on purpose (ref/02, §2.3.3).
4. **Human output by default and `--json` always available.** The human outputs that appear in the documents and in the track are **illustrative**: the text, the order and the alignment may change between versions without notice. The contract is `--json`, which is the API JSON (ref/09) of the endpoint the command uses (ref/09, §9.1), without a wrapper; for local commands, the one in the command's reference. Scripts and tests use `--json`.
5. **`--env <variant>`** (or `JK_ENV`) chooses the input overlay by variant: `--env lab` is `--manifest joker.lab.toml` (ADR 0026). This is how you switch between overlays (`--env sim`, `--env rpi`). **`--manifest <file>`** (or `JK_MANIFEST`) accepts any path (`--manifest sites/lisboa-01/joker.verao.toml`); a file whose name is outside the `joker.<variant>.toml` rule gives warning `J0169`. By default, `joker.toml`. Both options together are a usage error; the precedence between options, variables and `.joker/config.toml` (`defaults.env`, `defaults.manifest`) is in ref/01 §7.7. Each input manifest has its lock in the same directory: `joker.lock` for `joker.toml`, `joker.<variant>.lock` for each overlay (`--env rpi` → `joker.rpi.lock`); `--lock <file>` chooses another.
6. **Global options:** `--node`, `--env`, `--manifest`, `--lock`, `--locked` (no command writes the lock; any necessary change is `J0411`, it is the option for CI), `--offline` (local cache only), `--json`, `--color`, `--timeout`, `--cert`, `--key`, `--ca`, `--sign-key`. Precedence: option > `JK_*` variable > the project's `.joker/config.toml` > the user's `config.toml` > default. `jk config show [--origin]` shows the effective value and where it came from (ref/02, §2.3).
7. **Strict requests, mixed versions.** `jk` asks the node what version and what features it has and rejects, with a clear message, an option the node does not support (ref/09, §9.3). The 1.1 options exist in `jk` 1.0 and answer `J-E1017`.
8. **Nothing is ever compiled on the node.** `jokerd`, tier X drivers, apps and AOT (`.cwasm`) are compiled on the laptop or in CI, by cross build for the board's triple (`aarch64-unknown-linux-gnu`, `armv7-unknown-linux-gnueabihf`, `arm-unknown-linux-gnueabihf` for the ARMv6 of the Pi Zero W, `x86_64-unknown-linux-gnu`), and only then sent to the node (`jk apply`, `jk runtime upgrade`, or the Deck). `jk runtime build` and `jk dev --node` compile **on the laptop** and send the result; no command requires a compiler, `cargo` or the kernel headers on the node. A 512 MiB board does not compile `jokerd`, and a production node should not have a toolchain.

---

## 2. Project and development

| Command | Does |
|---|---|
| `jk init [--name <n>]` | turns the current (empty) directory into a project: minimal `joker.toml`, `sim/`, `.gitignore` and a `.taplo.toml` that associates each suffix (`*.profile.toml`, `*.board.toml`, `*.sim.toml`, `*.scenario.toml`, `joker*.toml`…) with the JSON schema of its type, for completion and errors in the editor (ADR 0026; ref/03 §2.1.6). `jk new` writes the same |
| `jk new <name> [--template sim-tank\|bess\|vision\|vibration\|empty]` | creates a project in a new directory (manifest, example app, `sim/` scenarios) |
| `jk app new <name> [--lang rust\|ts\|py]` | creates an app in `apps/` (TypeScript in 1.0, Python in 1.2; ref/05) |
| `jk check` | validates the manifest, the bindings, the units, the envelopes, RT admission (M23), the bus budget, the referenced secrets and the MCU capabilities (`J0710`); with `--node`, also on the node (`dry_run`) |
| `jk fmt [<file>…] [--check]` | canonical form of the TOML files and the scenarios: canonical duration literals, display forms (`Bad(CommFailure)`) rewritten in `snake_case` (ref/00, §2 and §3.1) |
| `jk build [--target <board\|triple>] [--world joker:runtime@<v>]` | compiles the apps to Wasm, precompiles the AOT for the target; `--world` generates for an older node (ref/05). The MCU targets are in §2.2 |
| `jk dev [--node <target>] [--panel]` | runs the runtime (local with `sim`, or remote) with hot-reload of the apps and the manifest on every save; with `--node`, compiles the apps and the AOT on the laptop and sends them to the node (§1, principle 8); `--panel` shows an interactive dashboard with the `sim` signals that declare `panel` (lamps, buttons by key, sliders) |
| `jk test [cenário…] [--fuzz N]` | unit tests of the apps and site scenarios (`sim/`) with a virtual clock |
| `jk bench [<app>] [--target mcu:<board>]` | measures the WCET of the cycle tasks on the target and records it in the lock's `[wcet]` (minimum of 10,000 runs; ref/02, §2.1.13). On an MCU, with the core's cycle counter: by probe, or, on a board without a probe (`[probe] tool = "espflash"`), over serial (ref/08, §2.12.5) |
| `jk diff [--against <generation\|file>]` | effective manifest after the overlays; differences from what is running |
| `jk explain <key\|code\|pipeline>` | origin of a key (overlay), explanation of an error, plan of a pipeline |
| `jk explain <output path>` | the arbiter state of an output, on the node: owning group, occupied levels (writer, value, lease), active level, envelope and last result (`GET /v1/outputs/{path}`, `OutputState`; ref/09, §5.6). An argument that is at the same time a manifest key and an output is read as an output; `jk explain 'outputs."<output>"'` asks for the key |
| `jk inspect <app.wasm\|driver.wasm\|firmware.jkfw\|model.jkm>` | metadata: ports, tasks, WIT world, signature, lineage; the descriptor of a driver; the descriptor of a firmware (ref/08, §2.14) |
| `jk lsp` | language server for the TOML files and the `.jkc` files |
| `jk sim <scenario>` | runs only the simulated plant, without apps (to see the dynamics) |

### 2.1 The lock

The detail (what each option writes and the canonical form) is in ref/02, §2.1.15 and §3.1.

| Command | Does |
|---|---|
| `jk lock` | resolves boards, profiles, classes, apps, drivers and MCUs; copies the approvals (`accepted`); prunes orphan entries. It never bumps a version that still satisfies the manifest |
| `jk lock --node <node>` | also captures, through the node and with reads only, the **missing** discovery and fallbacks, and writes `runtime` and `node` |
| `jk lock --update <selector> --node <node>` | recaptures the indicated devices (after a drift) |
| `jk lock --rebind --node <node>` | accepts a node different from the recorded one and recaptures everything; keeps the WCET if the board is the same (a template copied to another site) |
| `jk lock --upgrade [<reference>…]` | resolves again to the most recent version within the major |
| `jk lock --check` | does not write; in CI fails with code 7 if the lock is outdated or has drift |
| `jk lock --offline` | local cache only; what does not resolve is `J0411` |

In `dev`, `jk check`, `jk build`, `jk dev` and `jk test` update by themselves the sections that do not need the node; in `prod`, never. `jk apply` never writes the lock.

### 2.2 Joker Micro: build and flashing

| Command | Does |
|---|---|
| `jk build --target mcu:<board>` | firmware with the embedded generation (ELF, UF2 or bin) |
| `jk build --target mcu:<board> --generic` | generic firmware, which reads the generation from flash, and its descriptor `firmware.jkfw`; it carries all the transports of the board's `[link] transports` (ref/08, §2.12.1) |
| `jk build --target mcu:<board> --generation --firmware <firmware.jkfw> [--check-reproducible]` | the generation blob `<site>.jkgen`, signed (in `dev`, it may go unsigned, and the firmware accepts it; in `prod`, `J0779`), in the format and ISA that firmware understands (`J0789` if it needs more); `--check-reproducible` builds twice and compares |
| `jk build --target mcu:<board> --lib` | C library mode: `libjoker_node.a`, `joker_node.h` (fixed) and `joker_project.h` (generated) |
| `jk build --target mcu:<board> --crate` | crate mode: the generated crate `joker-project` in `target/joker/crate/` |
| `jk flash [--identity]` | flashes the firmware, the blob and, with `--identity`, the identity area: by probe (`probe-rs`) or over serial (`espflash`, on the ESP32), according to the board's `[probe]` (ref/03 §2.3.3) |
| `jk trace-dump --probe` | reads the trace rings from an MCU's RAM by probe, without symbols |

The modes and the artifacts are in document 19, §4, and in ref/08, §2.2, §2.12–§2.14.

## 3. Node and deploy

A **node** is a board with `jokerd` (document 15, §1). The `--node` option chooses the node; the manifest's `[devices]` are something else (the equipment the node talks to).

| Command | Does |
|---|---|
| `jk node enroll <target> …` | enrollment: P-256 identity (in the TPM if there is one) and X25519 KEM key, certificate, anchors; options in §3.1 |
| `jk node renew` | renews the certificate now (the KEM key is kept) |
| `jk node trust show` | active trust bundle, anchors, version, keys per role |
| `jk node trust push <bundle.cose>` | installs a new bundle (signed by an anchor, greater `version`) |
| `jk node trust set --anchors <keys/…pub>… [--revoked <id>…]` | change of anchors, signed by a current anchor; it is, with enrollment, the only write to `node.toml` |
| `jk doctor [--ml] [--rt] [--allow-live]` | checks the kernel (PREEMPT_RT, `isolcpus`), permissions, groups, watchdog, clock, accelerators (and whether they are real), disk, device fallbacks, node files changed by hand (`J0907`); `--rt` with live outputs in `prod` requires `--allow-live` (ref/09, §5.17) |
| `jk apply [--confirm 5m] [--dry-run]` | sends the configuration, the artifacts and the secrets that the generation refers to, shows the diff plan (what restarts, what reloads, why it asks for confirmation) and applies |
| `jk publish --manifest <m>… [--registry <oci>] [--sign-key <role>=<key>…]` | for a fleet with a Deck: for each input manifest, computes the node's generation (effective manifest, lock, contracts and artifacts by digest; the same descriptor and the same hash that `jk apply` and the Deck compute, ref/10 §2.6.2), **signs it** with the key of the `config` role, signs the apps (`apps`), the W and X drivers (`drivers`) and the MCU generation blobs (`config`), and publishes everything to the registry (default: `defaults.registry`), which is the Deck's `artifacts.store` (document 20, §4.4.2). `--sign-key` is repeated per role; without it, `user.signing_key` for all. It does not talk to any node. The Deck only distributes artifacts signed this way: without them, it logs `J0915` and emits nothing for that node. A role without a key, or a key that the bundle does not give to that role, makes `jk publish` fail before publishing (`J0915`, `J0292`) |
| `jk confirm` | confirms the pending generation and, with it, the OTA of each MCU that this activation put on trial (generation or firmware; ref/07 §9.4, ref/09 §5.13) |
| `jk rollback [<generation>]` | reactivates a previous generation (the same one, with the same number and the same hash; ref/00, §5) |
| `jk generations` | history of activations (who, when, origin, diff, status) |
| `jk runtime upgrade <version>` | OTA of the runtime (A/B slots) |
| `jk runtime build --features <list> [--target <board\|triple>]` | custom build of `jokerd` with tier N drivers and backends (document 09, attack 7), **on the laptop or in CI**, by cross build for the board's triple (§1, principle 8); the result reaches the node through `jk runtime upgrade` (upload `joker.runtime.v1`, ref/09) |
| `jk migrate [--dry-run]` | rewrites manifests and locks to the schema of the current runtime (optional in series 1; required in 2.0). It also **renames the files of a project older than ADR 0026** (type in the name): profiles (`profiles/x/y@1.toml` → `…@1.profile.toml`), boards (`.board.toml`), simulation models (`sim/esp.toml` → `sim/esp.sim.toml`), scenarios (`.scenario.toml`), suites (`.conformance.toml`), packages (`package.toml` → `<package>.package.toml`), driver fixtures (`.fixture.toml`) and overlays (`rpi.toml` → `joker.rpi.toml`, with the lock `rpi.lock` → `joker.rpi.lock`). The type of each file is decided by its content (the identification table, ref/03 §2.1.6), and not by the directory; the files it cannot classify stay as they are and are listed. It updates the references by path (`extends`, `model`, `manifest` of the scenarios) and adds the `.taplo.toml` if missing. It uses `git mv` when the project is in a Git repository, so the history follows the files. `--dry-run` shows the changes without making them |

### 3.1 Enrollment, keys and CA

```
jk node enroll <target> --ca <directory|url> [--name <node>] [--site <s>] [--org <o>]
               [--anchor <keys/x.pub>]… [--allow-dev] [--tpm required|preferred|off]
               [--code <code>] [--force]
jk node enroll <target> --deck <url> --token <t>
jk node enroll mcu:<port> --deck <url> --token <t>
jk node enroll <mcu> --link <device> [--probe] [--ca <directory>]
```

| Option | Does |
|---|---|
| `--ca <directory>` | signs the CSR with a local CA; if the directory does not exist, creates a development CA like `jk ca init --dev` (and the node gets `policy.allow_dev = true`). The `[nodes.<node>]` context gets the bootstrap address (`ssh://…`); it is changed to `<host>:7443` when the node listens on TCP (ref/02, §3.2.3) |
| `--ca <url>`, `--deck <url> --token <t>` | enrolls with a CA or a Deck; the response carries the anchors and the first bundle (ref/10, §3.1.2) |
| `--anchor <.pub>` | trust anchors of the node (repeatable) |
| `--allow-dev` | accept `mode = "dev"` generations; a bundle with `min_mode = prod` rejects them anyway |
| `--tpm required\|preferred\|off` | where the identity key lives; `required` fails without a TPM |
| `--code <code>` | the one-time code the node shows (`jokerd enroll-code`), sent through the local socket (physical access, or `ssh://…`): it proves that whoever enrolls had access to the node, and removes the need to be `root` or in the `joker-admin` group on the node (ref/02, §3.2.3). **There is never an enrollment port open to the network**; enrollment through the Deck uses the token, and it is the node that connects outward (ref/09, §2.1) |
| `--force` | enrolls an already enrolled node again, with a new key (the secrets must be encrypted again) |
| `--link <device>` | enrolls an MCU behind a node: generates the `K_link` of the `joker-link` device and writes it to the MCU and to the node (`secret:link.<device>`; ref/07, §4.7). With `--ca`, on an M2 board, it also generates the MCU's X25519 KEM key, writes to it a certificate from the CA with that key in the SAN and adds the MCU as recipient `node:<mcu>` of `secrets.toml.enc`: this is what allows secrets to be delivered to it, such as the Wi-Fi password (`[net.wifi]`, ref/01 §5.22; ref/02 §2.4.8) |
| `--probe` | writes the MCU's identity by probe, instead of through the link |

| Command | Does |
|---|---|
| `jk ca init <directory> --org <org> [--user <name>] [--dev]` | creates the local CA (Ed25519 root, P-256 issuer and the organization name; ref/10, §3.1.1) **and** the user certificate `admin-<user>` (role `admin` by the node's rule), which it records in `[user]` of the client's `config.toml` (ref/02, §3.3.3) |
| `jk ca bundle <directory> --key <role>=<pub>… [--deck <id>=<pub> --deck-url <url>] [--min-mode prod] --out <f>` | builds and signs with the CA root the trust bundle (keys per role, the Deck, the minimum mode), which `jk node trust push` installs and the Deck delivers at enrollment (ref/02, §2.5.5) |
| `jk key gen <name> --kind signing\|secrets` | Ed25519 signing key (artifacts, generations, secrets, bundles) or X25519 escrow key (secrets); the private key stays outside the project, the `.pub` in `keys/` (ref/02, §2.5) |
| `jk config show [--origin]` | effective client configuration and the origin of each value |
| `jk config set <key> <value> [--project]` | changes a key of the user's `config.toml` (or, with `--project`, of `.joker/config.toml`), validated by the schema of ref/02 §2.3.2: for example `jk config set nodes.pi.address pi.local:7443` after enrollment (ref/02, §3.2.3, step 5) |

### 3.2 Secrets

Secrets live in `secrets.toml.enc`, encrypted per recipient and versionable (ADR 0024; ref/02, §2.4).

| Command | Does |
|---|---|
| `jk secret set <name> [--nodes a,b] [--file <f>] [--description <d>]` | asks for the value without echo (or reads from stdin or from `--file`), bumps `rev`, encrypts for the recipients and signs |
| `jk secret get <name> --reveal` | decrypts with the escrow key; without `--reveal`, refuses |
| `jk secret list` | names, `rev`, type, nodes, missing recipients; never values |
| `jk secret rm <name>` | removes the entry |
| `jk secret import <file\|->` | imports a plaintext file (each entry is a `set`) |
| `jk secret recipient add --node <node>` \| `--user <name> --key <x25519.pub>` | adds a node (by the KEM key attested in the certificate) or an escrow person |
| `jk secret recipient rm <recipient>` | removes a recipient; **does not revoke**: the value is rotated afterwards |
| `jk secret push --node <node>` | sends the entries to the node without creating a generation |
| `jk secret status --node <node>` | compares names and `rev` of the file with those of the node |

### 3.3 Agent and sessions

They talk to the node's local API, which talks to the Deck (ref/10, §2.18).

| Command | Does |
|---|---|
| `jk agent status` | connection to the Deck, authority, `seq`, queue, pause, wait for local changes, sessions |
| `jk agent desired [--json]` | the current desired state, decoded, and the state of the apply |
| `jk agent pause --ttl <d> --reason "…"`, `jk agent resume` | suspends the generation swaps requested by the Deck (download and reports continue) |
| `jk agent import <directory>` | applies a desired state and artifacts brought by hand (no network) |
| `jk session list`, `jk session accept <id>`, `jk session close <id>` | remote sessions requested by the Deck (**1.3**) |

## 4. Operation

| Command | Does |
|---|---|
| `jk status [--instances]` | summary: generations, groups (jitter, overruns), devices (link), apps (state), active alarms, trips; `--instances` lists the class instances, the origin (profile, manifest, app) and the health |
| `jk top` | live TUI with cycle, drivers, envelope, store, link |
| `jk read <selector> [--watch]` | reads signals with quality and age (`jk read 'pcs.**' --watch`) |
| `jk read <selector> --refresh` | forces the read of a `param` with `poll = "on_demand"` |
| `jk write <output> <value> --ttl <t> --reason "…" [--level manual:<n>] [--takeover]` | manual write as writer `user:<principal>`: shows the envelope, the active level and the predicted result, and asks for confirmation. The default level is `manual:4`; `--ttl` and `--reason` are required; a level occupied by someone else is only taken with `--takeover`, which goes into the decision log |
| `jk release <output> [--level …]` | releases a manual level |
| `jk trace <signal>` | follows a signal: driver → acquisition → apps that read it → proposals → arbiter → envelope → commit → readback |
| `jk decisions [--output <s>] [--since 1h \| --from … --to …] [--cause <signal>] [--report pdf\|csv]` | decision log (M3): each write, with result, reason and the inputs it depended on; `--cause` filters the writes that read a given signal; `--report` generates a compliance report (the PDF is made by `jk`, not by the node) |
| `jk alarms [--active]`, `jk alarm ack <id>`, `jk alarm shelve <id> <ttl> "<reason>"`, `jk alarm unshelve <id>` | alarms; shelving has a required TTL, up to `max_shelve` |
| `jk trips`, `jk reset <output> --reason "…"` | trips and reset |
| `jk intents`, `jk intent approve <id>`, `jk intent reject <id>` | intents of proposal-only writers (M18, **1.1**; in 1.0 the list is always empty) |
| `jk action <device\|instance>.<action> [args] --key <k>` | invokes an action of a device or of an instance (M19) |
| `jk call <instance>.<method> [args] --ttl <t> --reason "…"` | calls a class method as a `manual` writer (document 17, §4.1) |
| `jk app <start\|stop\|restart\|logs> <app>` | app lifecycle |
| `jk device stop <device> --reason "…"`, `jk device start <device>` | stops and starts again the driver of a device (role `operator`, audited): while stopped, the link is `stopped`, the signals `bad(not_connected)` and the outputs on the device fallback. It is volatile, like an app's `stop`: it holds until the next startup of `jokerd` or the next generation (ref/09, §5.12) |
| `jk logs [--app <a>] [--follow]` | structured logs |

## 5. Data

| Command | Does |
|---|---|
| `jk history <selector> --from … --to … [--step 1m]` | queries the historian |
| `jk export <selector> --from … --to … --parquet out.parquet` | exports (the Parquet is written by `jk`) |
| `jk record --for 2h [--groups fast] --out x.jktrace` | records a replayable trace; at the start, saves the state of each app (`export-state`) in a state segment (document 16, §7) |
| `jk replay x.jktrace [--app a@versão] [--diff] [--rerun-models]` | replays (document 04, §7), from the state recorded by `jk record`; in a trace without state (`trace-dump`, `trace-from-history`), from `init`, and warns that the first ticks may diverge; `--rerun-models` is 1.1 |
| `jk trace-dump --group fast --last 30m --out x.jktrace` | extracts from the group's continuous trace ring (exact in the inputs, but without the app state: replay starts from `init`) |
| `jk trace-from-history <interval> --app <a> --out x.jktrace` | rebuilds an approximate trace from the historian |
| `jk link status` | uplinks: connection, cursor lag, budget (M12), drops |

## 6. Shadow and ML

| Command | Does | Version |
|---|---|---|
| `jk shadow start <app> <candidate>`, `jk shadow status`, `jk shadow diff`, `jk shadow promote`, `jk shadow abort` | shadow mode of apps (M16) | 1.0 |
| `jk shadow … <model>` | shadow mode of models | **1.1** |
| `jk model build <onnx\|dir> [--via burn\|ort] [--arch joker.tcn@2 --weights w.safetensors]` | creates the `.jkm`; with `--via burn` lists the missing operators | **1.1** |
| `jk model inspect <m.jkm>` | signature, guard, available backends, lineage | **1.1** |
| `jk model bench <m.jkm> [--node …] [--backend …]` | latency and memory per backend on the target | **1.1** |
| `jk model lineage <m>` | lineage tree (M10) | **1.1** |
| `jk model migrate <m.jkm> --burn <version>` | migrates the weights between Burn versions | **1.1** |
| `jk train <job> [--now]`, `jk train status` | on-device training | **1.1** |

## 7. Discovery, profiles, boards and drivers

| Command | Does |
|---|---|
| `jk discover <uri>` | read-only discovery (document 05, §7) |
| `jk profile import <sunspec\|nodeset\|eds\|esi> <file>` | generates a profile for review |
| `jk profile import csv <file> --name <vendor>/<model> --driver modbus [--out <file>] [--modicon]` | generates a profile **draft** from a register map in CSV (columns `signal,fc,at,type,direction,unit,scale,offset,label,description`); what the CSV does not say is marked `# REVIEW`; `--modicon` reads `at` in the old 1-based notation (`30002` → register 1). Format and rules in ref/03 §2.2.19 |
| `jk profile check <profile>` | validates a profile without equipment (schema, units, the driver's `plan`) |
| `jk profile diff <a> <b>` | compares two versions and says which part of the version must go up (ref/03, §2.2) |
| `jk profile test <profile> --uri <uri> [--node <target>]` | reads all the signals of the profile from a real piece of equipment and shows values, units, quality, measured latency and errors; **never writes**. The URI gives the protocol, the port or the address and the unit: `modbus-rtu:<port>?baud=<b>&unit=<u>`, `modbus-tcp://<host>:<port>?unit=<u>` (full grammar in ref/03 §2.2.20). Without `--node`, the driver runs on the laptop |
| `jk profile publish <profile>` | publishes to the registry (requires `jk profile diff` and conformance reports, ref/03 §2.8) |
| `jk board show <board>` | the profile of a board: fixed IO, ports, watchdog, capabilities, probe |
| `jk board probe --node <node> [--name <board>] [--out <file>]` | reads the node's hardware inventory (`GET /v1/node/hardware`, reads only) and writes a board profile **draft** (default: `boards/<board>.board.toml`); what the inventory cannot know (tier, target kernel, `fallback` of the fixed IO, `recommended`) is marked `# REVIEW` (ref/03 §2.3.7) |
| `jk board check <file> [--node <node>]` | validates a board profile (`J0800`–`J0849`); with `--node`, confirms on the node that the architecture, the CPUs, the peripherals, the lines and the declared paths exist (`J0828`, `J0829`) and lists the `# REVIEW` left to resolve |
| `jk board new <name> --from <board>` | copies an existing board profile (from the project or from the registry) to `boards/<name>.board.toml`, with a new `name`, `version = "1.0.0"` and `tier = "dev"` marked `# REVIEW` |
| `jk board publish <file>` | publishes a board to the registry (with no unresolved `# REVIEW`; ref/03 §2.3.7 and §2.8.4). A `tier = "dev"` board is never published (`J0848`) |
| `jk driver test <ref> [--fixture <f>] [--case <glob>] [--report <out.cbor>]` | driver conformance suite against an N, W (`w:./x.wasm`) or X (`x:./binário`) driver (ref/06, §6) |
| `jk driver meta <driver>.driver.toml` | generates the CBOR descriptor of a driver from TOML (for C drivers) |

## 7.1 Classes (M24)

| Command | Does |
|---|---|
| `jk class list [--std\|--project]` | available classes, with versions |
| `jk class show <class>` | members, capabilities, methods, queries, reference standards |
| `jk class new <name>@<major>` | class skeleton + conformance file |
| `jk class verify <class> --instance <i> [--allow-actuation] [--report <f>]` | runs the class conformance suite against an instance. On an instance that is not simulated it requires `--allow-actuation` (`J0640`) and `mode = "dev"`, refuses to run if there is an active `safety` or `manual` level on the instance's outputs (`J0641`), and writes at `manual:3` with a TTL equal to the duration of the suite: the apps keep running and are `overridden` during the test (ref/03, §2.6.3); `--report` writes the report that publication requires (ref/03, §2.6 and §2.8) |
| `jk class publish <file>` | publishes to the registry (requires conformance and runs `jk class diff`) |
| `jk class fmt\|check\|diff\|doc\|gen` | format, validate (JC-codes), compare versions (document 17, §9), generate documentation and types |
| `jk explain <instance>` | mapping of each member, origin (profile/manifest/app), latency of the virtual layers; on a composite instance, the tree of the class members (bound instance, class, origin and health of each node, indented), the depth (`JC0501`) and the layers (`J0620`), and the compositions the instance is a member of (`GET /v1/instances/{i}`, ref/09 §5.11; example in document 14, §9.3) |

Outside v1.0: `jk ui` (Joker Studio, 1.3) and `jk class extract` (after v1).

## 8. Exit codes

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | usage error |
| 2 | validation failed (J-codes) |
| 3 | the device refused (permissions, signature, envelope) |
| 4 | timeout or device unreachable |
| 5 | confirmation expired and there was a rollback |
| 6 | test or scenario failed |
| 7 | drift detected (`jk lock --check`) |

The correspondence with the API responses is in ref/09, §9.2.

## 9. Complete flows

**From the laptop to the field:**

```sh
jk new site-porto --template bess
cd site-porto
jk dev                                      # simulated plant; iterate on the app
jk test                                     # sim/ scenarios against the real manifest
jk node enroll revpi-porto.local --ca ~/org-ca
jk doctor --node revpi-porto.local --rt   # kernel, isolcpus, watchdog, fallbacks
jk discover sunspec://192.168.10.30 --node revpi-porto.local
jk lock --node revpi-porto.local          # pins discovered maps, firmware, fallbacks
jk bench --node revpi-porto.local         # real WCET → lock
jk check                                    # RT admission with measured WCET
jk secret recipient add --node revpi-porto # encrypts the secrets for this node too
jk apply --node revpi-porto.local --confirm 10m
jk status --node revpi-porto.local
jk confirm --node revpi-porto.local
```

**An incident:**

```sh
jk decisions --output pcs.p_set --since "2026-09-24 03:00" --to "03:30"
jk trace-from-history "03:00..03:30" --app peak_shaver --out incidente.jktrace
#   ↑ approximate trace: it is only exact if the app's inputs were recorded at the group's rate
#     (the group's `trace.continuous` keeps an exact trace ring of the last N hours)
jk replay incidente.jktrace --diff --app peak_shaver@1.4.3
```

**A new version of an app, in shadow (1.0):**

```sh
jk shadow start peak_shaver peak-shaver-1.5.0.wasm --node linha2
jk shadow status peak_shaver --node linha2   # 48 h later
jk shadow promote peak_shaver --node linha2
```

**A new model (1.1):**

```sh
jk model build yolo-parts-v8.onnx --via ort
jk model bench yolo-parts-v8.jkm --node linha2 --backend hailo,burn:cpu
jk shadow start detector yolo-parts-v8.jkm --node linha2
jk shadow status detector --node linha2   # 48 h later
jk shadow promote detector --node linha2
```

**A board that does not have a profile yet, and a piece of equipment with a register map** (ref/03 §2.3.7 and §2.2.19):

```sh
jk board probe --node zero --name rpi-zero-w           # draft in boards/rpi-zero-w.board.toml, with # REVIEW
jk board check boards/rpi-zero-w.board.toml --node zero      # after resolving the # REVIEW
jk profile import csv mapa.csv --name local/xy-md02 --driver modbus
jk profile test local/xy-md02@1 --node zero --uri "modbus-tcp://192.168.1.50:502?unit=1"
jk check --env zero                                      # joker.zero.toml, lock joker.zero.lock
```

**A coprocessor MCU, with generic firmware:**

```sh
jk build --target mcu:rp2350 --generic          # firmware.uf2 + firmware.jkfw
jk flash --identity                                      # by probe, once
jk node enroll pump-mcu --link pump_mcu --node revpi-porto.local
jk build --target mcu:rp2350 --generation --firmware firmware.jkfw
jk apply --node revpi-porto.local --confirm 10m          # the node takes the blob to the MCU through the link
```
