# 20 — The product family and Joker Deck

Joker started as a runtime. With Joker Micro (document 19) and with the question "who orchestrates 3,000 of these?", it becomes a **family**. This document defines the pieces, the contracts between them and, in more detail, the missing piece: the **Deck**, the control plane of the fleet.

It is normative as to the principles and the contracts (ADR 0021). The internal design of the Deck is an **architecture proposal**: it will have its own set of documents when it starts.

---

## 1. The family

The theme writes itself: a joker is a card, and cards live in a deck.

| Piece | What it is | Where it runs | Status |
|---|---|---|---|
| **Joker** (`jokerd`, `jk`) | the runtime on Linux | Raspberry Pi, RevPi, industrial PCs, Jetson | v1 |
| **Joker Micro** | the runtime on microcontrollers | RP2350, ESP32-C6/S3, STM32 | v1 |
| **Joker Hub** | the artifact registry: profiles, boards, classes, apps, (models in 1.1) | cloud or on-premises | v1 (minimal), grows later |
| **Joker Deck** | the control plane of the fleet: inventory, desired state, rollouts, observability, remote access | cloud or on-premises | MVP in v1, its own product later |
| **Joker Studio** | the commissioning and development interface: dashboard, signal browser, trace and replay, manual writes | local (`jk ui`) and inside the Deck | v1.x |

```
                         ┌──────────────── Joker Deck ────────────────┐
  Git (sites, templates) │ inventory · desired state · rollouts ·     │ ◀── operators, integrators
  ──────────────────────▶│ observability · alarms · remote access     │
                         └───────┬───────────────────────▲────────────┘
                     desired state│ (signed)             │ reported state, telemetry
                                  ▼                      │
            ┌────── Linux node (jokerd) ────────┐        │
            │ apps · arbiter · envelope · ...   │────────┘
            └──────┬─────────────────▲──────────┘
             Joker Link│             │SAMPLES/OUTCOMES          ┌─── Joker Hub ───┐
                   ▼   │             │                          │ profiles, boards│
            ┌── Joker Micro ──┐    ┌─ Joker Micro (standalone) ─┐│ classes, apps   │
            │ coprocessor     │    │ Wi-Fi → Deck directly      ││ (OCI)           │
            └─────────────────┘    └────────────────────────────┘└─────────────────┘
```

---

## 2. The principles that are not negotiable

1. **Local-first.** A node works on its own, forever, without a Deck or a Hub. Everything the Deck does, `jk` also does, node by node. The Deck serves scale, not survival.
2. **The Deck is never in the control path.** There is no real-time write that depends on the Deck, and **the agent protocol has no message for writing to signals**: the Deck changes configuration (generations) and opens sessions. A remote write only exists inside a session (1.3), through the node's local API, like that of a person on site: a `manual` writer with the principal `deck:<deck_id>/user:<id>`, TTL, and the usual arbiter and envelope (ref/10, D9).
3. **Open contracts.** The agent protocol, the artifact format (OCI), the telemetry (Sparkplug B, Zenoh, OTLP) and the Hub API are publicly specified. Anyone can write another Deck, or connect Joker to Mender, balena or a SCADA.
4. **Git is the source of truth for configuration.** The Deck reads templates, overlays and sites from a repository (GitOps); edits in the interface generate commits. Nothing lives only in the Deck's database.
5. **Node security does not depend on the Deck.** Signatures and the trust policy are verified on the node; a compromised Deck cannot make a node accept an artifact that the node's keys do not accept.

---

## 3. The contracts

### 3.1 Agent protocol (node ↔ Deck)

The closed form is the `agent/1` protocol, in ref/10 (messages, enrollment, trust, confirmation, transports, micro profile), with the schemas in `spec/cddl/agent.cddl`. In summary:

| Message | Direction | Contents |
|---|---|---|
| **desired state** | Deck → node | signed document: runtime version, generation (hash of manifest + lock + contracts), artifacts (apps, profiles, MCU firmware, models in 1.1) by OCI digest, confirmation window, cohort |
| **reported state** | node → Deck | running versions, confirmed generation, lock drift, health (groups, devices, instances), active alarms, trips, resource usage, connected MCUs and their versions |
| **events** | node → Deck | alarm transitions, decisions marked for audit, results of generation applies |
| **remote session** (1.3) | Deck ↔ node | tunnel opened by the node, with TTL, for `jk` and for the Studio, with node roles and local consent |

- **Transport:** HTTPS with long-poll, or MQTT; tolerates months offline (the node stays in the last confirmed state).
- **Apply:** the node applies the desired state with the usual mechanism (document 01, §15): validate, plan, swap at a tick boundary, wait for confirmation, automatic rollback if health fails. The Deck has no special path. A generation only confirms itself after the Deck acknowledges a report (`report-ack`): a generation that cut the node's network does not confirm itself (ref/10 §3.5).
- **MCUs:** an MCU behind a Linux node receives firmware and generations over Joker Link, orchestrated by the node. A standalone MCU with a network speaks the same protocol, in a reduced version (CBOR over MQTT or CoAP).

### 3.2 Artifacts (Hub)

Everything is a signed OCI artifact, with its own media type and a signing role: the runtime, the generation of a Linux node, apps, drivers, profiles, boards, classes (with the contract and the conformance), the firmware and the generation blob of the MCUs, the sealed secrets and, in 1.1, the models. The single table of the types, with the short form (`joker.app.v1+wasm`) and the full registry form (`application/vnd.joker.app.v1+wasm`), is in ref/00, §7.1. A common OCI registry (Harbor, GHCR, ECR…) serves as a minimal Hub; the Hub adds search, compatibility (which profiles implement which classes) and the publication checks (class conformance, `jk class diff`).

### 3.3 Telemetry

The Deck consumes what the nodes already send: Sparkplug B or Zenoh for signals and alarms, OTLP for the metrics of the runtime itself (document 01, §13 and §19). There is no "Deck" format.

---

## 4. Joker Deck, in detail

### 4.1 What it does

| Area | Features |
|---|---|
| **Inventory** | nodes and MCUs, boards, versions, location, customer/site, enrollment status, certificates |
| **Configuration** | sites from templates + overlays (M13) in a Git repository; diff between desired and reported; drift detection |
| **Rollouts** | cohorts and waves (1% → 10% → 100%), automatic KPI stop (alarm rate, trips, overruns, rollback), maintenance windows per site |
| **Observability** | fleet health, aggregated alarms, runtime metrics, dashboards per site |
| **Remote operation** | `jk` and Studio through a tunnel, manual writes and approval of intents (M18) with audit |
| **Identity** | CA for enrolling nodes and MCUs, certificate rotation, revocation |
| **Audit** | who changed what, when, at which site, with which approval; the nodes' decision logs on request |
| **Multi-tenant** | an integrator manages the fleets of their customers; each customer sees only their own |

### 4.2 Proposed architecture

- **Server** in Rust (the same language, the same manifest, lock, `jkc` and verification crates that `jk` uses: the Deck validates a generation exactly like `jk check`).
- **State:** PostgreSQL for inventory, rollouts and audit; an OCI registry for artifacts; TimescaleDB or an equivalent, optional, for metrics; Git for configuration.
- **Web interface**, with the Studio embedded for each node.
- **Deployment:** SaaS or on-premises, in containers; a small Deck runs on a single server for fleets of a few hundred nodes.

### 4.3 Stress

**D1 — 10,000 nodes, 30% offline for weeks.** The desired state is idempotent and signed, and the node applies it when it comes back. The rollout counts an offline node as "pending", not as a failure. **Holds.**

**D2 — A bad rollout.** Version 3.2 of the app increases trips on 1% of the nodes of the first wave. With a rule `halt_if = "trips_rate > baseline * 1.5"`, the rollout stops: the Deck stops issuing desired states for the following waves. The affected nodes go back through the confirmation window, through a `reject` command (those that are still confirming), or through a new desired state that points at the previous generation, which the node **reactivates** with the same hash and the same number (ref/10 §3.5, S2). **Holds**, and it is the reason waves exist.

**D3 — The Deck is compromised.** The attacker can publish a desired state, but cannot sign artifacts with the keys the nodes accept (principle 5), has no message for writing to signals (principle 2) and does not read secrets, which go encrypted to the nodes (ADR 0024). At most, they can roll back to already signed generations, limited by the maintenance window and by the node's `max_applies_per_day` (ref/10 §4). **Holds**, on the condition that the signing keys do **not** live in the Deck (HSM or offline signing in the release pipeline).

**D4 — An integrator manages 40 customers.** Multi-tenant with per-tenant isolation in the inventory, in the private artifacts and in the telemetry. The integrator's templates are shared; the sites belong to the customer. **Holds.**

**D5 — A customer does not want a cloud.** On-premises Deck, or no Deck: `jk` and Git are enough (principle 1). **Holds.**

**D6 — The Deck grows until it becomes a SCADA.** The risk of *scope creep* is real. Containment: the Deck manages **nodes and configuration**, and shows health and alarms; synoptics, operation HMI and plant historian remain the domain of dedicated products (document 12). If that changes, it is an explicit product decision, with an ADR.

### 4.4 The Deck MVP in practice

The minimum to run the Deck MVP (§5, v1) in a small installation: a server, a few dozen nodes, a Git repository. It is what the track uses in part 11, with the Deck on the laptop. The complete reference of the Deck (web interface, operator API, multi-tenancy, high availability) is **product**, and stays outside `docs/ref/`. The contract with the nodes is only the `agent/1` protocol of ref/10: everything the Deck does to a node goes through it.

#### 4.4.1 The binary and the commands

| Command | Does |
|---|---|
| `joker-deck serve --config deck.toml` | starts the Deck: the `/agent/v1` endpoints of ref/10 §2.13, the artifact store, reading from Git and the rollouts |
| `joker-deck token create --node <node> --ttl <d> [--config deck.toml]` | issues an enrollment token `jkt1_…` (ref/10 §2.5.1): single use, bound to the node name, `ttl` ≤ 7 d |

The equivalent container image is `registry.example.com/joker/deck`, with the same binary as the entry point:

```sh
docker run -d -p 8443:8443 -v /srv/deck:/srv/deck registry.example.com/joker/deck:1.0 serve --config /srv/deck/deck.toml
```

#### 4.4.2 `deck.toml`

```toml
# deck.toml — a small Deck, on a single server
[deck]
id     = "bancada-deck"
url    = "https://portatil.local:8443"
listen = "0.0.0.0:8443"
data   = "~/deck/data"

[database]
url = "sqlite:deck.db"                  # in production: "postgres://deck@db.example.com/deck"

[git]
repo   = "~/joker-bancada"
branch = "main"
poll   = "30s"

[ca]
dir = "~/org-ca"                        # the same CA as `jk ca init`

[artifacts]
store = "builtin"                       # or "oci://registry.example.com/joker/fleet"

[signing]
key = "~/.local/share/joker/keys/deck.key"
```

| Key | Type | Req. | Default | Meaning |
|---|---|---|---|---|
| `deck.id` | name (ref/00 §5) | yes | — | the `deck_id`: the `authority` of the trust bundle, the `deck` of each desired state and of the `deck` key (ref/10 §2.4.3, §2.6) |
| `deck.url` | `https://` URL | yes | — | the address the nodes use; it must be in the bundle's `deck_urls`. The server's TLS certificate is issued by the `[ca]` issuer for the name of this URL |
| `deck.listen` | `"<host>:<port>"` | no | `"0.0.0.0:8443"` | where the server listens |
| `deck.data` | path | yes | — | local state: the SQLite database, the built-in store, the copy of the repository |
| `database.url` | `"sqlite:<path>"` \| `"postgres://…"` | no | `"sqlite:deck.db"` | inventory, rollouts, reports and events. SQLite (relative to `data`) for a small installation; PostgreSQL in production. The password never goes in the file: it comes from the libpq variables (`PGPASSWORD`) or from `~/.pgpass` |
| `git.repo` | URL or path | yes | — | the project repository; the credentials are those of the system's Git (SSH agent, *credential helper*) |
| `git.branch` | string | no | `"main"` | the branch that is the desired state of the fleet |
| `git.poll` | duration | no | `"30s"` | interval for reading the branch; ≥ `"5s"` |
| `ca.dir` | path | yes | — | the `jk ca init` directory: the Deck reads the issuer (`ca.crt`, `ca.key`), the `org` and the trust bundle (`trust.cose`), which it delivers at enrollment. **It never reads `root.key`**: the root signs bundles outside the Deck (§2, principle 5) |
| `artifacts.store` | `"builtin"` \| `oci://` URL | no | `"builtin"` | where the signed artifacts live. `builtin`: an OCI distribution API at `<url>/v2/`, stored in `data`, and the nodes download from `/agent/v1/blobs` (source `kind = deck`); `oci://`: an external registry, and the nodes download from there (source `kind = oci`, with `registry-token`, ref/10 §2.13) |
| `signing.key` | path or `pkcs11:` | yes | — | the Ed25519 key **with the `deck` role**, and only that one: it signs desired states and commands. The Deck refuses to start if this key has any role other than `deck` in the bundle |

`~` at the start of a path expands to `$HOME`; relative paths resolve from the directory of `deck.toml`. An unknown key refuses the startup.

**The Deck does not have the organization keys.** It does not have the root, nor the `config`, `apps` or `secrets` keys. That is why it does not fabricate generations: it reads from Git what the generation of each node should be, computes the descriptor and the hash with the same crates as `jk` (ref/10 §2.6.2), and looks in the store for the `joker.config.v1+cbor` artifact signed by the `config` role with that descriptor, and for the artifacts it refers to, signed by their roles. Whoever signs and sends them is `jk`, on the machine of whoever has the keys: `jk publish --manifest <m>… --registry <oci> --sign-key <role>=<key>…` (document 07, §3) signs the generation of each node and the apps, and publishes them to the `artifacts.store` registry (with `builtin`, the OCI registry that the Deck itself serves at `<deck.url>/v2/`). The Deck only looks for signed artifacts: without them, it issues nothing for that node, and logs `J0915`.

#### 4.4.3 `fleet.toml`, in the repository

The Deck reads from the branch, at the root of the repository, a `fleet.toml`: which node runs which input manifest, in which cohort, and how a rollout is done.

```toml
# fleet.toml
version = 1

[nodes.zero]
manifest = "joker.zero.toml"
cohort   = "canario"

[nodes.lab]
manifest = "joker.lab-link.toml"
cohort   = "estavel"

[rollout]
wait    = "10m"
confirm = { mode = "auto", window = "15m", stable_for = "5m" }
halt_if = ["overruns_rate > 0.001", "trips_rate > 0", "rollbacks > 0"]

[[rollout.wave]]
cohorts = ["canario"]

[[rollout.wave]]
cohorts = ["estavel"]
```

| Key | Type | Req. | Default | Meaning |
|---|---|---|---|---|
| `version` | integer | yes | — | `1` |
| `nodes.<node>.manifest` | file | yes | — | input manifest of the node, relative to the repository root; the lock is the one with the same name (ref/02 §2.1.1), and it must have been captured on this node (`J0480`) |
| `nodes.<node>.cohort` | name | yes | — | the node's cohort; it appears in `rollout.cohort` of the desired state (ref/10 §2.6.4) |
| `rollout.wave` | list of tables | yes | — | the waves, in order; each `cohorts` lists cohorts, and each cohort is in exactly one wave |
| `rollout.wait` | duration | no | `"30m"` | wait between waves, counted from the moment the wave passed (below) |
| `rollout.confirm` | table | no | the one in ref/10 §2.6.4 | the `confirm-policy` that goes in the desired state: `mode`, `window`, `stable_for`, `require_uplink`, `alarm_severity` |
| `rollout.halt_if` | expression or list of expressions | no | `[]` | conditions on the KPIs of the wave; a single true one is enough to stop the rollout |

**KPIs**, computed by the Deck from the reports and events of the nodes of the wave (ref/10 §2.10, §2.11), since each node's swap:

| KPI | Meaning |
|---|---|
| `overruns_rate` | overruns ÷ ticks, summed over all the cycle groups of the nodes of the wave (`report-body.groups`) |
| `trips_rate` | new trips per node and per hour (`trip` events) |
| `rollbacks` | nodes of the wave that ended this rollout in `rolled_back` |
| `baseline` | in a comparison, the value of the same KPI on the same nodes in the hour before the swap (`"trips_rate > baseline * 1.5"`, D2) |

**From a commit to a rollout.**

1. At each `poll`, the Deck reads the branch. A new commit that changes the generation of some node (or its secrets) opens a **rollout**, and the previous rollout still in progress becomes `superseded`: it stops issuing, and the nodes that already received it stay where they are until the new one reaches them.
2. For each node, the Deck validates the generation like `jk check` (V19–V22 of ref/10 §3.3) and looks for the signed artifacts (§4.4.2). The secrets come from the commit's `secrets.toml.enc`: each node gets its own `secret-bundle`, with only its own `wrap` (key 22 of the desired state); a `wrap` made for a certificate that is no longer the node's is `J0922`, and that node is put on hold. A node that does not pass receives nothing; the others proceed.
3. **Wave by wave.** The Deck issues the desired state for the nodes of the wave, with `rollout = { id, wave, cohort }` and the `confirm` of `fleet.toml`. The wave **passes** when all the online nodes have confirmed, and at least one has confirmed, and after `wait` without any `halt_if` condition being true. An offline node counts as **pending**, never as a failure (D1): it receives today's desired state when it comes back.
4. **Stop.** A true `halt_if` condition, at any moment of the wave, puts the rollout in `halted`: the Deck issues nothing for the following waves, sends `reject` to the nodes of the wave that are still confirming (ref/10 §2.7) and issues to those that have already confirmed a desired state with the previous generation (`seq` + 1), which the node reactivates with the same hash and the same number (D2). It restarts with a new commit.
5. The Deck keeps **the last** desired state of each node, and never a queue (ref/10 §3.11.5).

In the MVP the Deck only **reads** Git: it writes no commits, no locks, no approvals. An invalid `fleet.toml` opens no rollout: the Deck logs the error and carries on with the last valid one.

---

## 5. Sequence

| When | What |
|---|---|
| v1 (Phase 3) | agent protocol specified; **Deck MVP**: inventory, desired state from Git, rollouts with waves and KPI stop, fleet health; Hub = OCI registry + publication checks |
| v1.x | Studio (local and in the Deck); multi-tenancy; approval of intents in the Deck; remote sessions |
| v1.5 (LTS; document 21, §7; ref/10 §6) | complete observability, aggregated audit, standalone MCUs managed at scale |

The Deck is **another product**, with another team when it grows, but it shares the validation crates with `jk`. That is what prevents the Deck and the node from having different ideas about what a valid generation is.

## 6. Note on the business model

Principles 1 and 3 (local-first and open contracts) are compatible with several models. The most natural one is **runtime, Joker Micro, protocol and formats open**, and **Deck and private Hub as a product** (SaaS or on-premises license). It is a decision for the project owner, tied to the choice of license (document 18, §6), and it should be taken before the first public commit.
