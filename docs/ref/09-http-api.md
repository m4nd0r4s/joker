# ref/09 — Local HTTP and WebSocket API

Normative reference of the local API of `jokerd` (`joker-api`). The machine-readable file is `spec/openapi/joker-api.yaml` (OpenAPI 3.1); this document and that file say the same thing, and wherever they diverge it is a bug to fix in both (ADR 0023, point 3).

---

## 1. Scope and sources

**Covers:** transports, authentication and roles, JSON representation, errors, pagination, filters, idempotency, optimistic concurrency, anti-replay, limits, every `/v1/…` endpoint, the WebSocket subscription protocol and the Server-Sent Events, and the mapping of every `jk` command (document 07).

**The rule that defines the scope:** `jk` is only a client of this API (document 01, §14; document 07). Everything `jk` does against a node goes through here, and therefore an HMI, the Studio or a script can do the same. The commands that do not touch a node (`jk new`, `jk check`, `jk build`…) are local and are marked in the table of §9.

**Does not cover:** the node ↔ Deck protocol (ref/10), which reaches the same core by another path; Joker Link (ref/07); the client libraries (ref/05, which generate code from the OpenAPI).

| Source | What it gives this reference |
|---|---|
| 01 §6, §6.1, §7 | arbiter, intents, envelope: what manual writes, `reset` and intents do |
| 01 §14–§19 | the API itself, generations, OTA, security, time, observability |
| 02 §5–§8, §9, §11 | samples, coherent read, lifecycle of a write, actions, paths and selectors, alarms |
| 03 §13 | `[security] api` and `[security.roles]` |
| 07 | the commands and the exit codes of `jk` |
| 16 | A1–A11, E1–E9, §3 quality, §7 trace, §9 codes |
| 17 §4 | methods and actions of classes |
| 19 §6, 20 §3 | MCUs over Joker Link, remote sessions of the Deck |
| 09 attack 9, attack 11 | threat model of the API, ~1,000 WebSocket clients |
| 12 §5, §12 | fieldbus boundary; the Studio as a client of this API |
| ADR 0013, 0015, 0022 | the API never stalls the core; confirmation; `/v1` and compatibility |

**Links to other references:**

| Reference | Relation |
|---|---|
| ref/00 | representation (time, values, JSON sample, quality, results, identifiers, errors, security, limits). It applies here without repetition |
| ref/01 | `security.api.listen`, `security.roles.*`, `max_manual`, `[alarms.*]`; the API exposes the effective manifest and the provenance of each key (§5.13) |
| ref/02 | node configuration (socket group, client CA and CRL, local limits, trace quota, `[policy] apply_from`), enrollment and trust (anchors, bundle), per-recipient secrets (`secret-bundle`), configuration of the `jk` client (`--node`), fact capture for the lock, `[wcet]` |
| ref/04 | `propose` (intents, 1.1), `invoke` (actions: the app's codes and the API's, ref/04 §7.1), ids of app alarms (`app.<app>.<name>`) |
| ref/05 | Python and TypeScript clients generated from this OpenAPI |
| ref/06 | runtime driver manifest (`GET /v1/runtime/drivers`), link state per device (`link_state`, ref/00 §6) |
| ref/07, ref/08 | state of the MCUs (session mode, version, counters, `HELLO`, firmware descriptor, `gen_seq`, confirmation, changed parameters) and OTA over Joker Link |
| ref/10 | descriptor and hash of the generation (ref/10 §2.6.2), the generation-transition mutex shared with the agent, `/v1/agent/*` (§5.21), remote sessions over a tunnel (`agent/1.3`) |

---

## 2. Transport, identity and roles

### 2.1 Listeners

| Listener | Default | Rules |
|---|---|---|
| `unix:/run/joker/api.sock` | always on | owner `joker:joker`, mode `0660`; the socket group comes from the node configuration (ref/02). Whoever can open the file talks to the API |
| `tcp:<host>:<port>` | off; default port 7443 | **TLS 1.3 with mandatory mTLS**, always: there is no key to turn it off (the old `mtls` of the manifest is `J0140`, ref/01). No 0-RTT. No TLS without a client certificate |

The listeners come from `node.toml` (`[api] listen`, the minimum that always holds, ref/02) and from `[security.api] listen` of the generation, which **adds** listeners (ref/01). Changing those of the generation is changing generation, and the plan marks the change as `api`, which requires a confirmation window in `prod` (ADR 0015; §5.13).

**Enrollment bootstrap channel.** On a node not yet enrolled there is no client CA, and therefore no mTLS. **There is never an enrollment port open to the network.** The `/v1/node/enrollment/*` endpoints only answer on the **Unix socket**: with physical access to the node, or over SSH to it (`--node ssh://…`, ref/02 §3.2.3). Without a code, the principal of the socket must be `root` or in the `joker-admin` group (over SSH, the SSH user; ref/02 §3.2.3), otherwise `J-E1002`. With the single-use enrollment code that the node shows (`jokerd enroll-code`, only for `root` and `joker-admin`, or the console of a factory image; `jk node enroll --code`), any principal that reaches the socket suffices: `jk` sends the code in the `Joker-Enroll-Code` header on each enrollment request, and a wrong, expired or already used code is `J-E1913`. The code proves that whoever enrolls had access to the node; it is valid for 15 min, single use, 5 attempts (ref/02 §3.2.3). Enrollment in a Deck uses the enrollment token, and it is the node that connects outward (`POST /v1/node/enrollment/deck`, ref/10 §3.1.2).

**On the client side**, the `--node` of `jk` and of the libraries (ref/02 §3.3.1, ref/05 §22.1, document 07 §1) accepts only these forms: `local`, a context name from the client file (`[nodes]`), `unix:<path>`, `<host>:<port>` (TCP with mTLS; port 7443 by default) and `ssh://[<user>@]<host>[:<port>]` (SSH tunnel to the Unix socket). The `tcp:` prefix only exists in the listeners of the node (this table), and not in `--node`.

**HTTP:** HTTP/1.1 and HTTP/2. On the Unix socket, HTTP/2 by *prior knowledge* (h2c); over TCP, ALPN `h2` and `http/1.1`. No HTTP/3. The `Host` header is ignored on the Unix socket.

### 2.2 Identity (principal)

Each request has a **principal**, which is also the writer `user:<id>` of manual writes (ref/00, §5; document 16, A1).

| Transport | How it authenticates | `id` | Stored `subject` |
|---|---|---|---|
| `unix` | file permissions + `SO_PEERCRED` (uid, gid, groups) | Unix user name | `uid`, name |
| `tcp` | client certificate that chains to the client CA of the node (ref/02) | `CN` of the certificate | full DN and SHA-256 of the certificate |
| `tunnel` (1.3) | remote session opened by the Deck (`session-open-args`, ref/10 §2.12): the principal arrives **pre-authenticated** | `deck:<deck_id>/user:<id>` (ref/00 §5) | `user-claim` of the Deck (id, name, groups, MFA) and the `id` of the session |

- The `id` has the grammar `[A-Za-z0-9._@-]{1,64}` (in the tunnel, the `id` of the `user-claim`). A `CN` outside it is refused on the first request (`J-E1003`, 403).
- **The `id` is not enough to compare principals.** Holding a manual level, releasing it and the audit use the full principal (transport + `subject` + fingerprint). A certificate `CN=diogo` and the Unix user `diogo` are different principals.

### 2.3 Roles

Three roles, ordered: `viewer` < `operator` < `admin` (ref/00, §6). Each operation declares the minimum role (`x-joker-role` in the OpenAPI and the tables of §5).

**Assignment.** The role is the **highest** of all the `[security.roles]` rules that match the principal (document 03, §13; grammar in ref/01, §5.20):

```toml
[security.roles]
operator = ["cert:CN=ops-*", "local:group=joker-ops"]
admin    = ["cert:CN=admin-*", "local:user=deploy"]
```

| Rule | Matches |
|---|---|
| `local:user=<name>`, `local:group=<name>` | Unix user or group of the peer (socket) |
| `cert:<ATTR>=<pattern>` | attribute of the DN (`CN`, `OU`, `O`) or `SAN` (URI, DNS, email), with `*` as a glob |
| `deck:<deck_id>/user:<id>`, `deck:<deck_id>/group:<g>` | a user or a group of the `user-claim` of a remote session (ref/10 §3.10) |

- **With no matching rule, the role is `viewer`.** Reaching the socket (file permissions) or presenting a valid certificate from the client CA is already a trust decision; the role only raises it.
- **Implicit on the socket:** uid 0 and the `joker` user are `admin`. Whoever is root already has the node.
- **The role in the certificate** is done with a `cert:SAN=urn:joker:role:operator` rule issued by the CA of the organization: the node does not trust a field of the certificate without a rule that names it.
- **Tunnel (1.3):** the effective role is the **lowest** of three: the one requested in `session-open-args`, `[agent.remote] max_role` of the node, and what `[security.roles]` gives the `deck:` principals (ref/10 §3.10; with no rule, `default_role`, and empty refuses). **Forbidden in a session, with any role** (`J0986`, 403): `/v1/node/*` (identity, enrollment and trust), `/v1/agent/*` except `GET /v1/agent` and `GET /v1/agent/desired`, and `/v1/secrets`. In 1.0 the `tunnel` transport does not exist.
- The role is computed on each HTTP request and, on a WebSocket or SSE connection, at opening and at each generation swap (§6.9).

### 2.4 Session tokens, browsers and CORS

- **1.0 has no bearer tokens** (`Authorization: Bearer`). Identity always comes from the transport. The local Studio (`jk ui`, 1.3) is a `jk` process that serves the interface on `localhost` and talks to the node with the credentials of `jk` itself; in the Deck, it talks through the tunnel.
- **CORS off.** There is no `Access-Control-Allow-*` in any response. A request with an `Origin` header is refused (`J-E1004`, 403): a browser with the client certificate installed cannot be used by an arbitrary page against the node (CSRF, *DNS rebinding*). The list of allowed origins is reserved for 1.3.
- **Only `application/json`** in JSON bodies. `application/x-www-form-urlencoded`, `multipart/form-data` and `text/plain` are `J-E1008` (415), which closes off "simple" browser requests.

### 2.5 Anti-replay (ref/00, §9)

The operations marked `x-joker-mutating` (all those that change state; §5) require, **over TCP and over the tunnel**:

| Header | Format | Rule |
|---|---|---|
| `Joker-Nonce` | ULID | not repeated in the last 60 s (twice the window) |
| `Joker-Timestamp` | RFC 3339 UTC | within ±30 s of the UTC clock of the node, with the clock `synced` |

- Either one missing: `J-E1020` (400). Outside the window: `J-E1021` (400). Repeated nonce: `J-E1022` (409).
- **On the Unix socket they are optional**, and checked if present. The peer is authenticated by the kernel and there is no network on which to capture and replay a request.
- **Node clock `holdover` or `unsynced`** (D16, ref/00 §9): the `Joker-Timestamp` cannot be checked with confidence. The node accepts the request with only the check of the nonce and of the binding to the target, widens the nonce memory to 10 min, responds with `Joker-Clock: holdover` or `unsynced`, and the audit marks `replay_check = clock_holdover` or `clock_unsynced`. Refusing everything would leave an operator with their hands tied exactly when NTP is down.
- **The nonce memory is bounded** (100,000 entries). When it is full, new requests are refused (`J-E1029`, 503) instead of nonces being forgotten: anti-replay never fails open.
- **Nonce and idempotency are different things.** A legitimate retry (the network dropped before the response) carries a **new nonce** and the **same** `Idempotency-Key` (§3.8).

### 2.6 What never goes through the API

| Thing | Why | What exists instead |
|---|---|---|
| the value of a secret, in either direction | ref/00, §9 | `GET /v1/secrets` gives names and revisions; input accepts only entries encrypted with HPKE to the KEM key of the node (§5.3, D6) |
| the private key of the node | it stays in the TPM or in the protected file | CSR and certificate (§5.3) |
| a write that bypasses the envelope | invariant 3 of `AGENTS.md` | manual writes, methods and intents enter the arbiter like any writer |
| the `force_safe` bit | only the supervisor activates it (document 01, §3.4) | read in `OutputState.force_safe` |
| a write at a level outside `manual` | the bands belong to the manifest (ref/01, §3.11) | `supervisor` arrives through the Deck (ref/10) or through bridges |
| a write to the `kv` of an app | the state belongs to the app | read, `admin` only |
| editing files in `/var/lib/joker` | document 15, §2.5 | generations |
| a change of the trust policy, of the roles or of the listeners outside a generation | they would be a second truth | generations with confirmation |
| `SignalId` without the hash of the registry | ref/00, §5 | the `SignalId` only appears in `SignalMeta` next to `registry` |
| credentials in URLs of uplinks, bridges and devices | — | the endpoints appear without credentials |

---

## 3. API conventions

### 3.1 Versions

- Every path starts with **`/v1`**. `info.version` of the OpenAPI is the version of the API (`1.0.0`), and `GET /v1/version` says which versions of every surface the node supports.
- **A minor only adds** (ADR 0022; ref/00, §1): endpoints, optional fields, new values at the end of an enum, new topics. Nothing is removed in 1.x. An obsolete endpoint carries `Deprecation` (RFC 9745) and keeps working.
- **Responses: the client ignores the unknown** (fields, enum values). An unknown enum value counts as `unknown`, or by its category (an unknown quality reason counts by its `quality`).
- **Requests: the server is strict.** An unknown field in the body, or an unknown query parameter, is `J-E1011` (400), except fields that start with `x_`. A 1.3 client that uses a new option against a 1.0 node gets a clear error, instead of seeing the option silently ignored (§11, case S9). Clients consult `GET /v1/info` (`features`) and `GET /v1/version` before using what is new.

### 3.2 Headers

| Header | Direction | Where | Rule |
|---|---|---|---|
| `Joker-Request-Id` | response | all | ULID; repeated in the `instance` of errors and in the audit |
| `Joker-Generation` | response | all | `"<gen> sha256:<hex>"` of the generation running when the request was served |
| `Joker-Clock` | response | all | `synced`, `holdover`, `unsynced` |
| `Joker-Api-Version` | response | all | `1.0.0` |
| `Joker-Nonce`, `Joker-Timestamp` | request | mutating | §2.5 |
| `Idempotency-Key` | request | mutating (`POST`) | §3.8; required on `POST /v1/actions/…` |
| `Joker-Idempotent-Replay` | response | same | `true` when the response is the stored one |
| `If-Match` | request | generations, writes | §3.9 |
| `ETag` | response | outputs, generations | §3.9 |
| `traceparent` | request | all | W3C Trace Context; propagated to the logs and to OTLP |
| `Retry-After` | response | 429, 503, 409 for a job in progress | seconds |
| `Location` | response | 201, 202 | resource created or to follow |

### 3.3 JSON representation

ref/00, §2–§6 applies. What this API adds:

| Thing | Form | Example |
|---|---|---|
| instant | RFC 3339, `Z`, up to 9 digits | `"2026-09-24T10:00:00.123456789Z"` |
| instant in parameters (`from`, `to`) | RFC 3339 (with `Z` or offset), `now`, or relative `-<duration>` | `from=-1h`, `to=now` |
| duration | literal (ref/00, §3.1). **On output**, an integer in the largest exact unit of `d`, `h`, `m`, `s`, `ms`, `us`, `ns` | `"90m"`, `"1500ms"`, `"38us"` |
| monotonic | only with `mono=true`: `ts_acq_mono_ns`, `ts_hw_mono_ns` (decimal strings), plus `boot_id` in the response | `"81234567890"` |
| value | ref/00, §4.1, in the declared unit of the signal | `412.5`, `"run"`, `"18446744073709551615"` |
| input value | number in the unit of the signal, quantity literal, boolean, variant, decimal string, base64 | `"50 kW"`, `50`, `true` |
| level | `"<band>:<level>"`; `"none"` with no active level | `"manual:4"` |
| write result | ref/00, §4.4 | `{"outcome": "clamped_rate", "applied": 1.0}` |
| quality | `quality` + `reason`, names from ref/00, §4.3 | `"uncertain"`, `"stale"` |
| generation | `{ "gen": 17, "hash": "sha256:…", "state": "confirmed" }` | |
| ids | ULID for requests, sessions, writes, invocations, jobs, traces, intents, uploads | |
| quantities in metadata | literals as declared, and the evaluated value next to them when it is an expression | `"range": ["-bms.p_max_discharge", "bms.p_max_charge"], "range_now": [-100, 100]` |
| sizes | IEC prefixes | `"16 MiB"` |

**The sample** is the one of ref/00, §4.2, with these optional fields: `bits` (type `bits`), `age` (duration, computed at the moment of the read), `acq_id` (only in coherent reads), `ts_acq_mono_ns`, `ts_hw_mono_ns`. The quality returned already includes aging (`Uncertain(Stale)` and `Bad(Stale)`), computed at the instant of the read as in the latch (document 01, §3.3). `ts_hw` is translated to UTC like `ts_acq`.

**Input of values.** A number is understood in the unit of the signal. A quantity literal is converted; a wrong dimension is `J0107`, a wrong quantity is `J0108` (ref/00, §4.1). A value that does not fit the type (text in a `bool`, a nonexistent variant, an integer outside the range of the type) is `J-E1036`. `bool` only accepts `true` and `false`. A `string` output takes a JSON string, which is its text; a `bytes` output takes base64 with the standard alphabet and padding (RFC 4648 §4). More than 64 B (after decoding), base64 that does not decode, or another JSON type is `J-E1036` (ADR 0062).

### 3.4 Errors

`application/problem+json` (RFC 9457), with the members of ref/00, §8, and a few more:

| Member | Type | Required | Content |
|---|---|---|---|
| `type` | URI | yes | `urn:joker:code:<code>` |
| `title` | string | yes | short message from the `codes.toml` registry |
| `status` | integer | yes | the HTTP status |
| `code` | string | yes | `J-E1102`, `J0107`, … |
| `detail` | string | no | the concrete case |
| `instance` | string | no | `urn:joker:request:<ULID>` |
| `severity` | `error` \| `warning` | no | default `error` |
| `help` | string | no | how to resolve it, in the style of rustc |
| `pointer` | string | no | JSON Pointer (RFC 6901) to the field of the body |
| `param` | string | no | query parameter or header |
| `spans` | list | no | `{file, line, column, end_line?, end_column?, label?}` in the effective manifest |
| `errors` | list | no | diagnostics (`code`, `severity`, `message`, `help`, `key`, `spans`), for example all the J-codes of a generation validation |
| `suggestions` | list | no | nearby paths by edit distance (like `J0101`) |
| `retry_after` | duration | no | equal to `Retry-After` |
| `request_id` | ULID | no | equal to `Joker-Request-Id` |

**An envelope result is not an HTTP error.** A manual write rejected by the permissive responds **200** with `outcome: rejected, reason: interlock`: the request was well formed and the decision was taken. The same for an action that failed on the equipment (`state: failed`). `jk` translates these results into its exit codes (§9.2).

### 3.5 Pagination and order

- Paginated lists: `?limit=` (1–1,000, default 100) and `?cursor=` (opaque). Response `{ "items": […], "next_cursor": "…" | null }`.
- The cursor is stable against insertions (it is a position, not an offset) and is valid for 10 min.
- Order: signals, outputs, alarms, instances, devices by **path or name** (byte order); decisions, events, logs, audit by **time** (`order=desc` by default; `asc` available).
- An invalid or expired cursor is `J-E1032` (400).
- Bulky responses (history, decisions, logs, audit, events, alarm transitions) accept `Accept: application/x-ndjson`: one object per line, with no envelope, with the same filters; pagination still works through the `Link: <…>; rel="next"` header.

### 3.6 Selectors

The grammar of document 02, §9, with this precision:

```
selector := part ("." part)*                 (max. 8 parts, 288 characters)
part     := "**" | glob
glob     := [a-z*][a-z0-9_*]*                (max. 32 characters)
```

- `*` matches zero or more `[a-z0-9_]` characters **within a segment** (`inv*.ac.p`).
- `**` is a whole segment and matches **one or more** segments (`meter.**` matches `meter.p_total` and `meter.ac.p`, not `meter`).
- A selector matches canonical paths **and** aliases. The results are deduplicated by `SignalId` and presented by the path that matched first, in byte order; the canonical one comes in `SignalMeta.canonical`.
- Parameter `sel`, repeatable, up to 64 per request. A malformed selector is `J-E1030`. More than 10,000 resolved signals is `J-E1031` (422). A selector with no matches gives an empty list (200), except on the write endpoints, where a nonexistent path is `J-E1033` (404, with `suggestions`).
- In alarms, selectors apply to the id of the alarm.

### 3.7 Time ranges

- `from`, `to` (forms of §3.3) and `since` (duration; equivalent to `from=-<since>`). `since` with `from` is `J-E1034`. `from > to` is `J-E1034`.
- Defaults: `to = now`; `from` depends on the endpoint (history: `to − 1h`; decisions, logs, audit, events: unbounded, paginated).
- The local times of `jk` (`--since "2026-09-24 03:00"`) are converted by `jk`, with the time zone of the client, before they arrive here. The API only receives UTC or explicit offsets.

### 3.8 Idempotency

| Where | Key | Window | Scope | What is stored |
|---|---|---|---|---|
| `POST /v1/actions/{target}/{action}` | `Idempotency-Key` **required** (`J-E1023` without it) | `dedup_window` of the action (document 16, E9; default 60 s) | (target, action, key), as in the core (document 02, §8) | the invocation and its result |
| the other mutating `POST`s | `Idempotency-Key` optional | 10 min | (principal, operation, key) | the HTTP status and the body of the response |

- Key: printable ASCII, 1–64 B (ref/00, §5; `J-E1025` outside that).
- The same key with a different body (hash of the canonical body) is `J-E1024` (422).
- The same key with the original request still running is `J-E1028` (409, with `Retry-After`).
- The response memory is bounded to 10,000 entries per node; when it fills, the oldest goes (the 10 min window is a maximum, not a minimum, and this is said here so that nobody depends on it for more than immediate retries).
- Manual writes are already almost idempotent (repeating renews the lease with the same value); the key serves to not duplicate the entry in the decision log and to return the same ticket.

### 3.9 Optimistic concurrency (ETags)

- The `ETag` of `GET /v1/generations/current` and of `GET /v1/outputs/{path}` is the **generation hash** running, in quotes (`"sha256:…"`). The envelope and the registry only change with the generation.
- `If-Match` with that value:
  - **required** on `activate`, `rollback`, `shadows` (start, promotion, abort): `J-E1027` (428) without it, `J-E1026` (412) if the generation changed;
  - **optional** on `manual` and `methods`: `jk` sends what it saw in the preview, and a new generation between the preview and the confirmation gives 412 instead of applying a value against an envelope the person did not see.
- `GenerationProposal.base` is the same mechanism in the body: the plan is only valid against generation `base` (`J-E1609`).

### 3.10 Rate limits

Token bucket per principal. When exceeded: `J-E1014` (429) with `Retry-After`. The values are defaults; the node configuration (ref/02) may lower them, and never raise them above double.

| Class | Rate | Burst |
|---|---|---|
| reads (`GET`, `POST …/query`, previews) | 100/s | 200 |
| mutating | 10/s | 20 |
| manual writes and methods, per output | 2/s | 4 |
| `POST /v1/values/refresh`, per signal | 1/s | 1 |
| jobs (`doctor`, `discovery`, `profile-tests`, `bench`, `lock/probe`, `verify`) | 1 in progress per type and per node | — |
| WebSocket/SSE connections | 16 per principal, 1,000 per node (document 09, attack 11) | — |

### 3.11 Limits

| Thing | Limit | Code |
|---|---|---|
| body of a request | 16 MiB (ref/00, §10) | `J-E1009` (413) |
| upload chunk | 8 MiB | `J-E1606` (413) |
| URL | 8 KiB; headers 16 KiB | `J-E1009` |
| inline content (`ContentRef.content`) | 1 MiB | `J-E1009` |
| selectors per request | 64 | `J-E1030` |
| resolved signals per request | 10,000 | `J-E1031` |
| coherent read | 256 signals | `J-E1037` |
| history points per response | 100,000 | `J-E1038` (paginate or aggregate) |
| page | 1,000 items | `J-E1012` |
| `reason` | 1 KiB (ref/00, §10) | `J-E1012` |
| WebSocket message from the client | 64 KiB | close 1009 |
| WebSocket message from the server | 1 MiB (batches are split) | — |
| subscriptions per connection | 64 | `J-E1903` |
| sampling budget of the node (all subscriptions) | 1,000,000 samples/s | `J-E1903` |
| stored write tickets | 10 min | `J-E1110` (404) |
| generations kept for rollback | the last 10 | `J-E1613` |
| pending generation not activated | 1 h | `J-E1614` (410) |

### 3.12 Content types

| Type | Where |
|---|---|
| `application/json` | default |
| `application/problem+json` | errors |
| `application/x-ndjson` | bulky lists (§3.5) |
| `text/csv` | `GET /v1/history`, `GET /v1/decisions` (columns of §5.5 and §5.7) |
| `text/event-stream` | `GET /v1/stream` |
| `application/toml` | manifest and lock of a generation |
| `application/octet-stream` | upload chunks, blobs |
| `application/vnd.joker.trace.v1` | `.jktrace` (document 16, §7) |
| `application/yaml` | `GET /v1/openapi.yaml` |

Responses accept `Accept-Encoding: gzip, zstd`. Requests are not compressed.

### 3.13 Long operations

There are three forms, and each thing uses its own:

| Form | What for | Resource | Following |
|---|---|---|---|
| **write ticket** | manual writes, `release`, methods, approved intents: they wait for a tick of the owning group | `/v1/writes/{id}` | `wait=true` (default, up to `wait_timeout`, 5 s, max. 30 s); otherwise 202 + `Location` |
| **invocation** | actions (M19), short or long | `/v1/invocations/{id}` | short actions: `wait=true` up to the `timeout` of the action; long: 202, topic `invocations` |
| **job** | everything that takes time and is not of the core: `doctor`, discovery, profile tests, `bench`, `lock/probe`, `verify`, OTA of the runtime and of MCUs | `/v1/jobs/{id}` | 202 + `Location`; topic `jobs`; `POST /v1/jobs/{id}/cancel` |

Jobs survive a restart of `jokerd` (the OTA of the runtime restarts it by definition): the state is persisted and, after startup, the job appears `succeeded` or `failed`.

---

## 4. Audit

Each mutating request, each refusal by role, origin or anti-replay, and each opening and closing of a session generate an **audit** entry (`GET /v1/audit`, `admin`): full principal, role, session (the HTTP connection and, in the tunnel, the `id` of the remote session of the Deck), `request_id`, operation, status, code, summary of the request **without secrets**, `nonce`, `Idempotency-Key` and result of the anti-replay check. `session=<id>` filters the audit of a remote session (ref/10 §3.10). It is stored with `fsync = "always"`, like trips and the critical entries of the decision log (document 01, §12 and §19).

The audit says **who asked the API for what**. The **decision log** (M3, §5.7) says **what the arbiter and the envelope did** with each write, wherever it came from. A manual write appears in both, linked by the `request_id`.

---

## 5. Endpoint reference


Legend: minimum **Role**; **M** = mutating (anti-replay over TCP, §2.5). The complete bodies and responses are in the OpenAPI, with the same schema names in parentheses.

### 5.1 Meta

| Method | Path | Role | M | Does |
|---|---|---|---|---|
| GET | `/v1/info` | viewer | | node, site, runtime (version, `boot_id`, build features), API, mode, board, kernel, generation, clock, limits, API `features`, and who is calling (`Info`) |
| GET | `/v1/version` | viewer | | supported versions: API, current and previous WIT world, `joker:driver`, JDP, manifest schema, lock, `agent/1`, Joker Link, C ABI, trace (`Version`) |
| GET | `/v1/health` | viewer | | `ok` \| `degraded` \| `failed` with checks; `probe=live` (process responds), `ready` (first tick done), `full`; 503 on `failed` (`Health`) |
| GET | `/v1/whoami` | viewer | | principal, role, session, rules that matched (`WhoAmI`) |
| GET | `/v1/status` | viewer | | summary of `jk status`: generation (and confirmation deadline), clock, groups, devices, apps, alarms (counts and the first 20), trips, manual levels, uplinks, MCUs; `instances=true` adds the instances (`Status`) |
| GET | `/v1/openapi.json`, `/v1/openapi.yaml` | viewer | | this contract |

`features` in 1.0: `sse`, `intents` (the API; empty until 1.1), `micro` (if there are `joker-link` devices), `sim` (in `mode = "dev"` with `sim` devices), `agent` (an agent is configured), `enroll.code` (there is an active enrollment code, waiting to be used). 1.1 adds `shadow.apps` (ADR 0035), `models`, `blocks`, `streams`, `schedule`; 1.3, `tunnel`. It is the list that the client libraries read on `connect` (ref/05 §21).

### 5.2 Node, groups, metrics, logs

| Method | Path | Role | M | Does |
|---|---|---|---|---|
| GET | `/v1/node` | viewer | | identity (subject, issuer, fingerprint, validity, key storage: `tpm` \| `secure_element` \| `file`), the certificate chain in PEM (`chain_pem`, with the KEM key in the SAN URI) and the KEM key (`kem_key`), enrollment (`Node`). The chain lets `jk` attest the KEM key against the CA of the organization when it talks over the socket, without an mTLS connection (ref/02 §3.4.2) |
| POST | `/v1/node/enrollment/csr` | admin | M | generates (or, with `rotate`, rotates) the P-256 identity key and, at the first enrollment, the X25519 KEM key, and returns a PKCS#10 CSR in PEM, with the name of the node in the CN and in the SAN DNS and the KEM key in the SAN URI `urn:joker:kem:x25519:<base64url>` (ref/02 §3.2.3). **Only over the bootstrap channel** (§2.1: the Unix socket, local or over SSH; otherwise `J-E1912`); `root` or `joker-admin`, or any principal of the socket with `Joker-Enroll-Code` |
| PUT | `/v1/node/enrollment/certificate` | admin | M | installs the issued chain (leaf first), the trust anchors (`anchors`, `.pub` or `trust-key` keys), the first bundle (`trust`, COSE) and, optionally, the client CA of the API and the `[policy]` parameters; `jokerd` writes `node.toml` (ref/02 §3.2.3, step 4). The leaf must match the key (`J-E1911`) and have the KEM key in the SAN. Only over the bootstrap channel |
| POST | `/v1/node/enrollment/deck` | admin | M | enrolls in a Deck (ref/10 §3.1.2): `deck_url`, `token`, `force`; the node makes the `POST /enroll` and installs the response → job `enroll`. Only over the bootstrap channel |
| POST | `/v1/node/renew` | admin | M | renews the certificate now (through the Deck, or returns a CSR with the **same** KEM key, without a Deck) → job `renew` |
| GET | `/v1/node/hardware` | viewer | | inventory of the hardware of the node, **with reads only** of the file system (`/proc`, `/sys`, `/dev`): architecture, model, CPUs, memory, kernel, boot command line, GPIO chips, LEDs, thermal zones, watchdogs, serial ports, I²C, SPI, network, USB, TPM (`NodeHardware`). It is what `jk board probe` and `jk board check --node` use (ref/03 §2.3.7) |
| GET | `/v1/groups`, `/v1/groups/{group}` | viewer | | period, CPUs, priority, overrun policy, trip, order, tasks (WCET declared, from the lock, measured), jitter and duration (p50, p99, p99.9, max.), trace ring coverage (`Group`) |
| GET | `/v1/metrics` | viewer | | snapshot of the metrics of document 01, §19, with the histograms that do not fit in scalar signals; `ml` is `null` in 1.0 (`Metrics`) |
| GET | `/v1/logs` | viewer | | structured logs from the API ring (16 MiB by default), filters `source`, `app`, `level` (minimum), range; to follow, topic `logs` (§6) |

**`NodeHardware`** (`GET /v1/node/hardware`). Each field says where it comes from; a field that the node cannot read (permissions, a system without `device-tree`) comes as `null`, and the `unreadable` list says which and why. Nothing here opens a device for writing, requests a GPIO or touches a bus: `jokerd` reads directories and attributes, and the GPIO lines only through the information `ioctl` of the chardev (`GPIO_V2_GET_LINEINFO_IOCTL`), which does not reserve them.

| Field | Type | Source | Meaning |
|---|---|---|---|
| `collected_at` | `Timestamp` | — | when the inventory was read (it is read on each request; there is no cache) |
| `arch` | string | `uname -m`, normalized | `aarch64`, `x86_64`, `armv7`, `armv6`, `riscv64` (the values of `[board] arch`, ref/03 §2.3.1); `machine` carries the raw value (`armv6l`) |
| `model` | string \| `null` | `/proc/device-tree/model` (without the final `\0`); on x86, `/sys/class/dmi/id/product_name` | `"Raspberry Pi Zero W Rev 1.1"` |
| `compatible` | list of strings | `/proc/device-tree/compatible` | `["raspberrypi,model-zero-w", "brcm,bcm2835"]` |
| `cpus` | object | `/sys/devices/system/cpu` | `count` (online), `possible`, `model` (`/proc/cpuinfo`), `max_freq` (`Quantity`, from `cpufreq/cpuinfo_max_freq`), `isolated` (list, `/sys/devices/system/cpu/isolated`), `nohz_full` (list) |
| `memory` | object | `/proc/meminfo` | `total` (`MemTotal`, IEC `Size`), `available` |
| `kernel` | object | `uname -r`, `/sys/kernel/realtime`, `uname -v` | `release`, `version`, `preempt_rt` (bool: `/sys/kernel/realtime` = 1, or `PREEMPT_RT` in `uname -v`), `preempt` (`none` \| `voluntary` \| `full` \| `rt`, from `/sys/kernel/debug/sched/preempt` if readable, otherwise from `uname -v`) |
| `cmdline` | list of strings | `/proc/cmdline` | the boot parameters, split; `boot_params` highlights those that `jk doctor --rt` knows (`isolcpus`, `nohz_full`, `rcu_nocbs`, `irqaffinity`) |
| `gpio_chips` | list | `/dev/gpiochip*` (information ioctl) | `path`, `name` (`gpiochip0`), `label` (`pinctrl-bcm2835`), `lines` (number of lines), `used` (list of `{ offset, name, consumer }` of the lines already requested by the kernel or by another process, e.g. the `ACT` LED requested by `leds-gpio`), `named` (how many lines have a name) |
| `leds` | list | `/sys/class/leds/*` | `name` (`ACT`, `PWR`), `max_brightness`, `trigger` (the active one, between `[ ]`: `mmc0`, `heartbeat`, `none`), `gpio` (`{ chip, offset }` when the LED belongs to `leds-gpio`, through `device/of_node`) |
| `thermal_zones` | list | `/sys/class/thermal/thermal_zone*` | `name` (`thermal_zone0`), `type` (`cpu-thermal`), `temp` (`Quantity` in `°C`, read now) |
| `watchdogs` | list | `/sys/class/watchdog/watchdog*` | `device` (`/dev/watchdog0`), `identity` (`bcm2835-wdt`), `timeout`, `min_timeout`, `max_timeout` (`Duration`; `null` if the driver does not expose them), `nowayout` (bool), `active` (bool: `state` = `active`, someone already armed it) |
| `serial` | list | `/sys/class/tty/*` with `device`, `/dev/serial/by-id`, `/dev/serial/by-path`, links `/dev/serial0`, `/dev/serial1` | `path` (`/dev/ttyAMA0`, `/dev/ttyUSB0`), `aliases` (the stable links that point to it: `/dev/serial0`, `/dev/serial/by-id/usb-Silicon_Labs_CP2102_…-if00-port0`), `driver` (`uart-pl011`, `cp210x`), `usb` (`{ vendor_id, product_id, serial }` if it is USB), `soc` (bool: it is a UART of the board, and not an adapter) |
| `i2c` | list | `/dev/i2c-*`, `/sys/class/i2c-adapter` | `path`, `name` (that of the adapter: `bcm2835 (i2c@7e804000)`). **No** address scan is done |
| `spi` | list | `/dev/spidev*` | `path`, `bus`, `cs` |
| `net` | list | `/sys/class/net/*` | `name` (`eth0`, `wlan0`), `kind` (`ethernet` \| `wifi` \| `loopback` \| `virtual` \| `other`: `wireless/` or `phy80211` gives `wifi`; no `device` gives `virtual`), `mac`, `up` (bool), `usb` (bool: the interface is behind USB, like the Ethernet of the Pi 3), `addresses` (assigned IPv4 and IPv6) |
| `usb` | list | `/sys/bus/usb/devices/*` with `idVendor` | `bus`, `port`, `vendor_id`, `product_id`, `manufacturer`, `product`, `class`; never the serial number of devices that are not serial ports |
| `tpm` | object | `/sys/class/tpm/tpm0` | `present` (bool), `version` (`"2.0"`), `device` (`/dev/tpmrm0`) |
| `unreadable` | list of `{ field, path, reason }` | — | what could not be read, and why (`permission`, `absent`, `error`) |

- **Reads only, and nothing sensitive.** The inventory is `viewer` because it touches nothing and reveals no secrets; it does not include serial numbers of equipment beyond USB serial ports (they are how the `by-id` path is written), nor the content of files of the node.
- **It is not the board.** The inventory says what exists **now** on this node; the board profile says what the **model** guarantees. A connected USB-serial adapter appears in `serial`, but does not belong to the board: `jk board probe` puts it in a comment, and not in `[serial]` (ref/03 §2.3.7).
- In `mode = "prod"` it responds the same way; the cost is that of reading a few hundred files in `/sys`, and it runs in `bg`.

`jk node enroll <target> --ca <…>` runs on the node or reaches the socket over SSH (`ssh -L`), with `--code` for whoever is neither `root` nor in the `joker-admin` group on the node; it generates the CSR, signs it with the local CA of the organization and installs the certificate and the anchors. With `--deck`, it uses `POST /v1/node/enrollment/deck` and the agent protocol (ref/10 §3.1.2). Trust after enrollment is in §5.21. Metrics are also `joker.*` signals (document 01, §19) and are read with `/v1/values?sel=joker.**`.

### 5.3 Secrets, sessions, audit

| Method | Path | Role | M | Does |
|---|---|---|---|---|
| GET | `/v1/secrets` | admin | | names, `rev`, type, `signer`, whether they are set, whether they are declared, who uses them, when they changed; **never the value** (`SecretEntry`). It is what `jk secret status` compares with the file (ref/02 §2.4.5) |
| POST | `/v1/secrets` | admin | M | receives a `secret-bundle` (type `joker.secrets.v1`, `application/vnd.joker.secrets.v1+cbor`, `spec/cddl/secrets.cddl`) with the entries encrypted for this node (`jk secret push`, and `jk apply` together with the generation). Each entry is verified on its own (ref/02 §3.4.3: signature of the `secrets` role, `wrap` for this node, test decryption, `rev`); the response says, per name, `accepted` or the code (`J0922`–`J0926`) (`SecretPushResult`) |
| PUT | `/v1/secrets/{secret}` | admin | M | a single entry, in JSON (`SealedSecret`: `scheme` = the HPKE suite of ref/02 §2.4.4, `key_id` = the id of the KEM key of the node, `sealed` = the CBOR of the `secret-entry` in base64); the same checks. A secret that no generation declares is `J-E1921`; an invalid entry is `J-E1920` with the code of ref/02 in `errors` |
| DELETE | `/v1/secrets/{secret}` | admin | M | deletes (only a secret that no stored generation refers to) |
| GET | `/v1/sessions` | admin | | active sessions (persistent HTTP, WebSocket, SSE) |
| DELETE | `/v1/sessions/{session}` | admin | M | closes a session (WebSocket with 4009) |
| GET | `/v1/audit` | admin | | audit (§4); filters `principal`, `operation`, `outcome`, range |

### 5.4 Signals and values

| Method | Path | Role | M | Does |
|---|---|---|---|---|
| GET | `/v1/signals` | viewer | | registry of the generation: `sel`, `direction`, `origin`, `device`, `instance`, `shape`, `values`; paginated; carries `generation` and `registry` (`SignalMeta`) |
| GET | `/v1/signals/{path}` | viewer | | metadata and sample of a signal, by canonical path or alias |
| GET | `/v1/signals/{path}/lineage` | viewer | | driver, device, bus, profile and register, acquisition (scale, units, deadband), readers (apps, `calc`, alarms, permissives, `requires_good`, ranges, queries, bridges, uplinks), writers, owning group, readback, atomic group (`Lineage`) |
| GET | `/v1/values` | viewer | | current samples (`sel` required); `coherent=true` does a coherent read (document 02, §6.4) and adds `acq_id`; `mono=true` (`ValuesResponse`) |
| POST | `/v1/values/query` | viewer | | the same with the body `{sel, coherent, mono}` |
| POST | `/v1/values/refresh` | operator | M | forces the read of the `param` signals with `poll = "on_demand"` that match `sel` (document 05, §3) and returns the new samples; the other signals of the selector are ignored; a timeout (default 5 s) is `J-E1016` |

`SignalMeta`: `path`, `canonical`, `aliases`, `signal_id` (valid only with `registry`), `type`, `enum` (variant → value), `bits` (name → position), `unit`, `quantity`, `shape` (`scalar`, `event`; `block` and `stream` reserved for 1.1), `direction` (`input`, `output`, `param`, `internal`), `origin` (`device`, `app`, `calc`, `alarm`, `bridge`, `pipeline`, `runtime`, `instance`, with the name), `max_age`, `poll`, `limits`, `stuck_detect`, `record`, `writable`, `group`, `members`, `sample`.

**`refresh` requires `operator`** because it generates traffic on the bus. `viewer` is a passive role: nothing it does reaches a piece of equipment.

### 5.5 Historian and events

| Method | Path | Role | M | Does |
|---|---|---|---|---|
| GET | `/v1/history` | viewer | | raw or aggregated query (`HistoryResponse`) |
| POST | `/v1/history/query` | viewer | | the same with the body `HistoryQuery` |
| GET | `/v1/events` | viewer | | recorded occurrences of signals with shape `event` (`EventRecord`: `path`, `ts`, `ts_src`, `seq`, `record`) |

History parameters:

| Parameter | Type | Default | Validation | Error |
|---|---|---|---|---|
| `sel` | selectors | — (required) | §3.6 | `J-E1030`, `J-E1031` |
| `from`, `to`, `since` | §3.7 | `to − 1h` .. `now` | §3.7 | `J-E1034` |
| `step` | duration | without `step`: raw points | ≥ 1 ms; with a fixed `tier`, ≥ resolution of the tier | `J-E1039` |
| `agg` | list of `avg`, `min`, `max`, `first`, `last`, `count`, `quality` | with `step`: `avg,min,max,count,quality` | `avg`, `min`, `max` only on numeric types | `J-E1039` |
| `tier` | `auto`, `raw`, `1s`, `1m`, `15m` | `auto` | — | — |
| `fill` | `none`, `previous`, `null` | `none` | — | — |
| `limit`, `cursor` | points per page | 10,000 | ≤ 100,000 | `J-E1038` |

- With `tier=auto`, the node chooses the coarsest tier with resolution ≤ `step` that covers `from`. If none covers the whole range, the response carries what there is, with `coverage` (tier, from, to) and a warning.
- `quality` in an aggregate is the **dominant quality** of the tier (document 01, §12).
- Raw points: `ts` (the `ts_acq` in UTC), `ts_src`, `value`, `quality`, `reason`, `seq`.
- The monotonic → UTC translation uses the correspondence table of the historian (document 01, §18). **A clock correction can change, retroactively, the `ts` returned** for old samples; the values and the `seq` do not change.
- `text/csv`: columns `path,ts,value,quality,reason,seq` (raw) or `path,ts,<aggregates>,quality` (aggregated).
- **Parquet is produced by `jk`** (`jk export --parquet`), from NDJSON. The node does not carry a Parquet writer (binary size, CPU in `bg`).

### 5.6 Outputs, arbiter and manual writes

| Method | Path | Role | M | Does |
|---|---|---|---|---|
| GET | `/v1/outputs` | viewer | | outputs with effective value, applied value, active level, last result, trip; filters `sel`, `tripped`, `manual` (`OutputState`) |
| GET | `/v1/outputs/{path}` | viewer | | complete state: envelope (with `range_now` and `actuations_used`), held levels, declared writers, `force_safe`, readback; `ETag` |
| POST | `/v1/outputs/{path}/preview` | viewer | | preview of a manual write; **does not write** (`WritePreview`) |
| POST | `/v1/outputs/{path}/manual` | operator | M | manual write (`ManualWriteRequest` → `WriteTicket`) |
| POST | `/v1/outputs/{path}/release` | operator | M | releases a manual level (`ReleaseRequest`) |
| GET | `/v1/manual` | viewer | | all active manual levels (`ManualOverride`) |
| GET | `/v1/writes/{write}` | viewer | | ticket of a write (10 min) |

**Body `ManualWriteRequest`** (also that of the preview):

| Field | Type | Req. | Default | Validation | Error |
|---|---|---|---|---|---|
| `value` | input value | yes | — | §3.3; `null` is not accepted | `J-E1036`, `J0107`, `J0108` |
| `ttl` | duration | **yes** (A10) | — | 1 s ≤ `ttl` ≤ `max_manual` of the output (default 8 h) | `J-E1101`, `J-E1103`, `J-E1102` |
| `level` | `manual:3`–`manual:5` | no | `manual:4` | only the `manual` band (ref/01, §3.11) | `J-E1104` |
| `reason` | string, 1–1,024 B | yes | — | — | `J-E1106` |
| `takeover` | bool | no | `false` | — | `J-E1105` without it, if the level belongs to another principal |
| `wait` | bool | no | `true` | — | — |
| `wait_timeout` | duration | no | `5s` | ≤ `30s` | `J-E1012` |
| `at` | instant | no | — | reserved for 1.1 (M6) | `J-E1017` |

**Semantics (document 16, A1–A11):**

1. The writer is `user:<id>` of the principal, `kind = manual`, at the requested level. The `lease_until` is `t_tick + ttl` (16 A2, with the tick at which the write enters); the TTL of a manual write **is** its lease.
2. The proposal goes to the inbox of the **owning group** of the output (A11) and is applied at the next tick of that group, like an async write (document 01, §5): step 2 of the tick, before the cycle tasks (A4).
3. **Renew:** the same principal at the same level replaces value and TTL (`renewed: true`).
4. **Holding:** another principal at the same level gives `J-E1105` (409) with who holds it, until when and why. With `takeover: true`, the API sends to the same inbox, in order, the *relinquish* of the holder and the new proposal; the decision log stores both (`kind = takeover`). There are no ties at a level (A5).
5. **Response:** with `wait`, 200 when the tick of the owning group has done the commit, with the `WriteOutcome`. A `rejected`, `clamped_*`, `suppressed` or `overridden` is a 200 response. Without `wait`, or if the tick does not arrive within `wait_timeout` (stuck group), 202 with the ticket in `queued`.
6. **`If-Match`** with the hash of the generation seen in the preview: 412 if the generation changed in the meantime.
7. **Expiry warning** (document 01, §6, rule 5): 15 min before the end of the TTL, the alarm `joker.output.<output>.manual_expiring` (severity `medium`) and an event on the `manual` topic. With `ttl ≤ 15 min`, only the event: an immediate alarm would be noise. When the TTL ends, the level releases itself and the decision log stores `kind = expiry`.
8. **Restarts** (document 16, A14): the manual level persists with the deadline in UTC and is restored before the first tick. If the clock is not `synced` at startup, the remaining TTL of the last record is restored, counted in monotonic time, and the alarm `joker.output.<output>.manual_restored` (severity `medium`) and a `restored` event on the `manual` topic are raised, so that an operator confirms (by renewing) or releases. The `ManualOverride` shows it with `restored: true` until the renewal or the release.

**Preview (`WritePreview`).** `jk write` shows it before asking for confirmation (document 07, §1, point 3, and §4):

| Field | Content |
|---|---|
| `current` | the `OutputState` now |
| `proposal` | level, value **converted to the unit of the output**, TTL, `lease_until`, `renews` |
| `conflict` | `null`, or who holds the requested level (writer, principal, until, reason) |
| `prediction.wins` | the requested level becomes the active one |
| `prediction.overrides` | active levels of lower priority that stop being in control |
| `prediction.outcome` | predicted result: `overridden(level)` if there is a higher active level; otherwise, the envelope evaluated over the current values (clock, permissive, `requires_good`, range, rate against the last applied, deadband, `min_interval`, actuation budget, atomic group) |
| `prediction.first_applied`, `time_to_target` | first applied value and estimate with the declared rate |
| `prediction.inputs` | the samples used in the expressions |
| `envelope` | the declared envelope |

The prediction uses the same functions of the core over the current values, outside a tick: it is **indicative**. The result that counts is that of the tick (§8.2).

**Release (`ReleaseRequest`).** `level` (default: the level that the principal holds on that output, if there is only one; `J-E1012` if there are several), `force` (releases a level of another principal; it stays in the audit; `J-E1108` without it), `reason`, `wait`. A level that is not held is `J-E1107` (404). Releasing is `relinquish()` (document 01, §6, rule 3).

### 5.7 Trips and decision log

| Method | Path | Role | M | Does |
|---|---|---|---|---|
| GET | `/v1/trips` | viewer | | tripped outputs: since when, `scope` (`output` or `group`), cause (`range`, `alarm`, `group_overrun`, `runtime`), `reset_when` and its current value (`Trip`) |
| POST | `/v1/outputs/{path}/reset` | operator | M | reset of a trip (`ResetRequest`: `reason` required, `wait`) |
| POST | `/v1/trips/reset` | operator | M | reset of several outputs by selector (`BulkResetRequest`), for example after the trip of a group |
| GET | `/v1/decisions` | viewer | | decision log (M3): one entry per change of (applied value, result, active level), with `repeats` and `last_ts` (01 §19) |

**Reset (document 16, E7).** The request is a `manual` action that goes to the inbox of the owning group. The response comes after the next tick: `cleared`, or `retripped` (the cause is still present and the output went back to trip at that tick, with the cause). An output that is not tripped is `J-E1120` (409). The trip is persistent (`state/trips`, document 01, §7), and the reset is also recorded with `fsync`, with its principal and reason (decision-log record `reset`, ADR 0061). In a bulk reset, an output that could not be reset (a full reset queue, for example) is `failed`, with its `error` (`code`, `detail`), and the others are still reset (ADR 0061).

**Decision log filters:** `output` (selectors), `cause` (only the decisions whose causal set contains that signal: `jk decisions --cause`), `writer`, `outcome`, `kind` (`proposal`, `expiry`, `release`, `takeover`, `safe`, `trip`, `reset`, `method`, `intent`), range, order, pagination.

**`Decision`:** `id`, `ts`, `tick` (group, seq), `generation`, `output`, `kind`, `writer` (id, kind, level, lease), `on_behalf_of`, `proposed`, `outcome`, `applied`, `active_level`, `inputs` (causal set: path, seq, value, quality), `app` (name, version, digest), `manual` (reason, TTL, `request_id`, principal), `method`, `intent`, `model_outputs` (reserved for 1.1, M22). On a `string` or `bytes` output, `proposed` and `applied` are the JSON form of the value (ref/00 §4.1), filled by the node from the staged proposal or from `safe` (ADR 0062).

`text/csv` (`jk decisions --report csv`): `ts,output,kind,writer,level,proposed,outcome,reason,applied,active_level,repeats,last_ts,principal,request_id`. The PDF report is generated by `jk` from the CSV; the node does not produce PDF.

### 5.8 Alarms

| Method | Path | Role | M | Does |
|---|---|---|---|---|
| GET | `/v1/alarms` | viewer | | alarms; `sel` (over the id), `state` (`active`, `unacked`, `shelved`, `suppressed`, `all`), minimum `severity` (`Alarm`) |
| GET | `/v1/alarms/{alarm}` | viewer | | one alarm |
| POST | `/v1/alarms/{alarm}/ack` | operator | M | acknowledges (`AckRequest`: `reason` optional) |
| POST | `/v1/alarms/{alarm}/shelve` | operator | M | shelves (`ShelveRequest`: `ttl` and `reason` required) |
| POST | `/v1/alarms/{alarm}/unshelve` | operator | M | unshelves |
| GET | `/v1/alarm-events` | viewer | | transitions, acknowledgments, shelvings, ends of shelving, suppressions (`AlarmEvent`) |

- **Alarm id:** the name from the manifest (`grid_overfreq`), that of the runtime (`joker.device.pcs.offline`, `joker.group.fast.overrun`, `joker.output.<output>.manual_expiring`, `joker.output.<output>.manual_restored`; the complete list of the `joker.*` is in ref/01 and in document 01, §19) or that of an app (`app.<app>.<name>`, ref/04 §4.2; the signals are `alarm.app.<app>.<name>.state` and `.active`).
- **Ack** follows document 02, §11: `active_unacked` → `active_acked`; `rtn_unacked` → `normal`. In `active_acked` it is idempotent (200, no change). In `normal` and `unknown` it is `J-E1204` (409): there is nothing to acknowledge, and an `unknown` is a quality problem of the inputs, with its own alarm.
- **Shelve:** `ttl` required (document 02, §11; `J-E1201`), at most the `max_shelve` of the alarm (default **24 h**, ref/01 §5.15; `J-E1202`). Shelving does not change the ISA-18.2 state of the alarm: it suppresses the notification (topics, uplinks with priority `alarms`) and marks `shelved`. At the end of the TTL, `shelve_expired`, and the alarm notifies again with whatever state it is in. Unshelving an alarm that is not shelved is `J-E1203`.
- `Alarm`: `id`, `source` (`manifest`, `app`, `runtime`, `instance`), `severity`, `state`, `active`, `uncertain`, `suppressed`, `shelved` (until, by whom, reason), `acked_by`, `acked_at`, `since`, `message` (already with the values interpolated), `condition`, `signals` (`state`, `active`), `trips`, `max_shelve`.

### 5.9 Intents (M18)

| Method | Path | Role | M | Does |
|---|---|---|---|---|
| GET | `/v1/intents` | viewer | | by default only the `pending` ones; `state`, `output` (`Intent`) |
| GET | `/v1/intents/{intent}` | viewer | | one intent; `preview=true` adds the preview of the write that the approval would make now |
| POST | `/v1/intents/{intent}/approve` | operator | M | approves and executes (`ApproveIntentRequest`) |
| POST | `/v1/intents/{intent}/reject` | operator | M | rejects (`reason` required) |

- An intent is born from `propose(output, value, justification, ttl)` of a writer `kind = "proposal"` (document 01, §6.1; ref/04). It stays `pending` until `expires_at = created_at + ttl`; if nobody decides by then, it becomes `expired`. An intent is valid for as long as the proposer said it was valid.
- **Approving** executes a manual write with the writer `intent:<id>` (ref/00, §5), `on_behalf_of` = whoever approved, at the level `level` (default `manual:4`), with TTL = that of the intent or less (`ttl` may shorten; lengthening is `J-E1302`), bounded by `max_manual`. The holding and `takeover` rules of §5.6 apply. The result comes in `Intent.write`.
- An intent that is no longer `pending` is `J-E1301` (409).
- **Version:** the intents API exists in 1.0. Proposal-only writers come in 1.1 (document 11), and until then the list is always empty (§12).

### 5.10 Actions (M19)

| Method | Path | Role | M | Does |
|---|---|---|---|---|
| GET | `/v1/actions` | viewer | | catalog: target and type (`device`, `instance`), action, arguments (type, unit, optional), permissive and its current value, role, timeout, long, `dedup_window` (`ActionDecl`) |
| POST | `/v1/actions/{target}/{action}` | operator (or the role of the action, if higher) | M | invokes; `Idempotency-Key` required (`InvokeRequest` → `Invocation`) |
| GET | `/v1/invocations`, `/v1/invocations/{invocation}` | viewer | | state, progress, result |
| POST | `/v1/invocations/{invocation}/cancel` | operator | M | cancels a long action |

- Devices and instances share the namespace of the first segment (document 16, P3), and therefore `target` is a single one.
- **Body:** `args` (object by name; quantities accept literals), `timeout` (may shorten the configured one, not lengthen it), `reason`, `wait`.
- **Order (document 17, §4.2):** role → permissive → deduplication by the key → serialized queue of the device → steps of the profile or `on-action` of the virtual implementation → result.
  - role below the one declared in `[devices.*.actions.*]` or `[instances.*.actions.*]`: `J-E1002` (403). **The floor is `operator`**: an action declared with `role = "viewer"` requires `operator` in the API (ref/01 gives the warning in `jk check`);
  - false permissive: `J-E1402` (409), with the expression and the values;
  - device queue full: `J-E1403` (503);
  - invalid arguments: `J-E1401` (400).
- **Result:** a short action responds 200 with `state` `succeeded` (and `result`) or `failed` (and `error`: `kind` `denied` \| `timeout` \| `device` \| `invalid` \| `cancelled`, `code` `J-E1406`–`J-E1409` or `J-E1415`, `message`). A long action responds 202 with the `ActionHandle` at `/v1/invocations/{id}`; progress arrives on the `invocations` topic. The codes are the counterpart of those an app receives in an `invoke` (table of ref/04 §7.1): the same fact, seen from each side.
- **Deduplication:** the same key within `dedup_window` returns the previous invocation (200, `replay: true`, `Joker-Idempotent-Replay: true`); with different arguments, `J-E1024`.
- Canceling a short or finished action is `J-E1405` (409).
- The actions of `joker-link` devices travel as `CALL`/`ACTION` (document 19, §6; ref/07).

### 5.11 Instances and classes (M24)

| Method | Path | Role | M | Does |
|---|---|---|---|---|
| GET | `/v1/instances` | viewer | | instances: class, resolved version, contract hash, origin (`implicit`, `composed`, `virtual`) and source, capabilities, health, implementation (`Instance`) |
| GET | `/v1/instances/{instance}` | viewer | | members (`in`, `out`, `param`, `event`, `action`, `method`, `query`, `member`) with path, mapping and sample; the **tree** of the class members, with the bound instance, the class, the origin and the health of each node (`InstanceNode`); the compositions of which the instance is a member (`member_of`); `explain=true` adds the origin of each mapping and the latency of each virtual layer (`jk explain <instance>`) |
| POST | `/v1/instances/{instance}/methods/{method}/preview` | viewer | | predicted expansion of the method and predicted result of each `out` (`MethodPreview`) |
| POST | `/v1/instances/{instance}/methods/{method}` | operator | M | calls the method as a `manual` writer (`MethodCallRequest` → `MethodCallResult`) |
| POST | `/v1/instances/{instance}/verify` | admin | M | runs the conformance of the class against the real instance (job `class_verify`): `allow_actuation` required on a non-simulated instance (`J0640`), refuses with a `safety` or `manual` level active on the outputs (`J0641`), writes at `manual:3` with TTL = duration of the suite (ref/03 §2.6.3) |
| GET | `/v1/classes` | viewer | | classes of the running generation, with version and contract |
| GET | `/v1/classes/{package}/{class}` | viewer | | the `contract.json` (document 17, §6); `std/pump@1` → `/v1/classes/std/pump@1` |

- **Method** (document 17, §4.1): `args` by name, `ttl`, `level`, `reason`, `takeover`, `wait`, `wait_timeout`, with the same rules as §5.6. The core stores a method record at the level of the caller and expands it in step 5a of the tick of the owning group. The response carries one ticket per expanded `out`. A method that writes `out`s of different groups produces one write per group, and the response waits for all of them. Nonexistent method: `J-E1411`; invalid arguments: `J-E1412`.
- **Health** (document 17, §4.5): `ok`, `degraded`, `failed`, with the responsible members in `health_detail`.
- **Tree** (document 17, §8): a class member appears in `members` with `kind = "member"`, the `class`, the `bounds` in a collection and `elements`, one `InstanceNode` per bound instance, in the order of the collection. Each node has the nested `path` (`esp_io.boot`, `station.pumps.p1`), the `instance` (null in an unnamed profile submember), the health and its own `members`, recursively, down to depth 3. The top-level health is already that of the tree. With `values=false`, the nodes come without samples. A virtual instance with the implementation stopped shows `implementation.state = stopped` (the `indicator` instance of the track, part 8, if `status_light_ctl` stops).
- **Verify:** `conformance` (blob or content), `cases`, `level` (default `manual:3`: the harness writes like an operator), `max_duration`, `allow_prod`, `reason`. In `mode = "prod"` it requires `allow_prod: true` (`J-E1414`), because the cases write to the equipment. In simulation, `jk class verify` is local.

### 5.12 Devices and lock facts

| Method | Path | Role | M | Does |
|---|---|---|---|---|
| GET | `/v1/devices`, `/v1/devices/{device}` | viewer | | driver (with version, tier and descriptor digest), bus, address (without credentials), profile, link state (`link_state`, the six values of ref/00 §6: `connecting`, `up`, `degraded`, `down`, `drift`, `stopped`), latency, timeouts, reconnections, drift, facts, actions (`Device`) |
| POST | `/v1/devices/{device}/stop` | operator | M | stops the driver of the device; `reason` required (`ReasonRequired` → `Device`) |
| POST | `/v1/devices/{device}/start` | operator | M | starts again a driver stopped with `stop` (`Device`) |
| GET | `/v1/lock/facts` | viewer | | facts of the running generation in the shape of the lock: `[discovered.*]`, `[fallback.*]`, measured restart time of `jokerd`, and the ready TOML excerpt (`LockFacts`) |
| GET | `/v1/lock/drift` | viewer | | drift entries (`J0401`), with the outputs blocked in `prod` |
| POST | `/v1/lock/probe` | admin | M | collects the same facts for devices that the node **does not yet have in a generation**, with reads only (job `lock_probe`): the request carries the effective manifest (`manifest`) or only the sections of the devices (`devices`: name → the `[devices.<d>]` section in JSON, with the buses they use), and what to collect (`what`: `discover`, `fallback`, `hello`) (`ProbeRequest`) |

**`stop` and `start` of a device** (`jk device stop|start`, document 07, §4). `stop` closes the driver of the device as in an orderly shutdown (the pending writes receive `not-connected` in the decision log; `sysfs` restores the `trigger` of the LEDs, ref/01 appendix A), puts the signals in `bad(not_connected)` and the `link_state` in `stopped` (ref/00 §6), and the outputs are covered by the device fallback (document 01, §8), as with a dead driver (ref/06 §5.7), but without a restart with backoff. On a `joker-link`, the link closes and the MCU stays with its local fallbacks. Like the `stop` of an app, it is **volatile**: it holds until the next startup of `jokerd` or the next generation that touches the device, and it appears in the `Device` as `stopped_by` (principal, when, reason). `start` does `connect`, the drift check and the `resync` of a reconnection (ref/02 §3.1.5). Both are audited (§4) and idempotent (200 on a device already stopped or already running); during a generation swap that touches the device, `J-E1501` (409). A device stopped by hand is not a fault: the event `joker.device.<d>.link` (ref/06 §2.12) carries `stopped_by` in the detail, and the `.offline` alarm is not raised.

`jk lock --node` uses `lock/probe` when the local manifest has devices that the node does not yet know (the flow of document 07, §9, does `jk lock` before the first `jk apply`), and `lock/facts` afterwards (ref/02 §3.1.3). **The probe never writes or switches on outputs**, nor the fallback registers: `fallback.configure = true` only acts at the activation of a generation. `discover` returns identity, effective map and digest; `fallback`, the current values of the fallback registers; `hello`, the `HELLO` of a `joker-link` MCU (ref/07 §6.4), without opening a `full` session.

### 5.13 Generations and blobs


**Blobs** (content-addressed store; semantics close to OCI Distribution):

| Method | Path | Role | M | Does |
|---|---|---|---|---|
| POST | `/v1/blobs/missing` | admin | | `{digests}` → `{missing}` |
| HEAD, GET | `/v1/blobs/{digest}` | admin | | exists; downloads (with `Range`) |
| POST | `/v1/blobs/uploads` | admin | M | with `?digest=` and an `octet-stream` body ≤ 8 MiB: monolithic upload (201). With `UploadStart` (`digest`, `size`, `kind`): upload session (202 + `Location`). Blob already existing: 201 with `existed: true` |
| GET | `/v1/blobs/uploads/{upload}` | admin | | `received` (next offset), to resume |
| PATCH | `/v1/blobs/uploads/{upload}` | admin | M | chunk with `Content-Range: bytes a-b/total`, ≤ 8 MiB, contiguous (`J-E1605`, 416) |
| PUT | `/v1/blobs/uploads/{upload}?digest=` | admin | M | closes (with an optional last chunk) and verifies the SHA-256 (`J-E1603`) |
| DELETE | `/v1/blobs/uploads/{upload}` | admin | M | cancels |

An upload session expires 1 h after the last chunk (`J-E1604`). The blobs that none of the 10 stored generations uses are deleted.

**Artifact types** (`kind`): those of the single table of ref/00 §7.1 (D13), in the short form: `joker.config.v1+cbor`, `joker.manifest.v1+toml`, `joker.lock.v1+toml`, `joker.provenance.v1+json`, `joker.app.v1+wasm`, `joker.app.v1+cwasm` (optional AOT; without it the node compiles), `joker.driver.v1+wasm`, `joker.driver.v1+tar`, `joker.profile.v1+toml`, `joker.board.v1+toml`, `joker.sim.v1+toml`, `joker.class.v1+jkc`, `joker.class.contract.v1+json`, `joker.class.conformance.v1+toml`, `joker.micro-firmware.v1`, `joker.generation.v1`, `joker.secrets.v1`, `joker.key.v1+pem`, `joker.runtime.v1`, `joker.trace.v1`. `joker.model.v1+jkm` is reserved for 1.1 (`J-E1017`). A `kind` outside the table is `J-E1012`.

**Generations:**

| Method | Path | Role | M | Does |
|---|---|---|---|---|
| GET | `/v1/generations` | viewer | | the stored generations (`Generation`: `gen`, `hash`, state, message, Git source, diff) and the **activation history** (`activations`: when, `origin`, principal, generation that was left, reason) |
| POST | `/v1/generations` | admin | M | validates and plans; `dry_run=true` stores nothing (200); otherwise it prepares a `pending` generation (201) |
| GET | `/v1/generations/{gen}` | viewer | | one generation, with plan and diagnostics; `ETag` |
| DELETE | `/v1/generations/{gen}` | admin | M | discards a `pending` one (it becomes `discarded`, `discard_reason = deleted`) |
| GET | `/v1/generations/{gen}/manifest` | viewer | | effective manifest (`application/toml`) |
| GET | `/v1/generations/{gen}/lock` | viewer | | lock |
| GET | `/v1/generations/{gen}/provenance` | viewer | | provenance of each key (file, line, operation `set`, `append`, `unset`, `widen`); `key=` filters |
| POST | `/v1/generations/{gen}/activate` | admin | M | swaps; `If-Match` required (`ActivateRequest`) |
| POST | `/v1/generations/{gen}/confirm` | admin | M | confirms the generation in `confirming` |
| POST | `/v1/generations/rollback` | admin | M | **reactivates** a stored generation (the previous one, by default); `If-Match` required (`RollbackRequest`) |

`{gen}` accepts the integer, the hash, or `current`, `previous` (the last one confirmed before the current one), `pending`, `confirming`.

**`GenerationProposal`:**

| Field | Type | Req. | Rule | Error |
|---|---|---|---|---|
| `manifest` | `ContentRef` (`digest` of a `joker.manifest.v1+toml` blob, or `content` ≤ 1 MiB) | yes | the **effective** manifest (overlays already merged by `jk`) | `J-E1602`, J-codes |
| `lock` | `ContentRef` | yes | | same |
| `provenance` | `ContentRef` | no | map key → origins, produced by `jk`; the node stores it and serves it | — |
| `artifacts` | list of `ArtifactRef` (`digest`, `kind`, `name`, `size`, `signature`) | yes | `name` identifies the artifact in the project: the manifest key for apps (`apps.peak_shaver`) and, for files, the path relative to the root with the suffix of the type (ADR 0026: `profiles/local/xy-md02@1.profile.toml`, `boards/rpi5.board.toml`, `keys/site-ops.pub`); a profile or a board with a `name` without the suffix of its `kind` is `J-E1012` | `J-E1602` (lists the missing ones) |
| `config` | `ContentRef` of the `joker.config.v1+cbor` artifact: the COSE_Sign1 whose payload is the descriptor of the generation (ref/10 §2.6.2) | in `prod` | signed by a key with the `config` role in the effective policy (ref/02 §2.5.4); the node rebuilds the descriptor from `manifest`, `lock` and `artifacts` and must obtain the same bytes | `J-E1607` |
| `base` | digest or `null` | yes (`null` only in `dev`) | generation on which the plan is made | `J-E1609` |
| `message` | string ≤ 1 KiB | no | | |
| `source` | `{repo, commit, path, dirty}` | no | | |

**Generation hash.** It is that of ref/10 §2.6.2: the SHA-256 of the **descriptor** in deterministic CBOR (effective manifest with the defaults materialized and without `x_*`, canonical digest of the lock, contracts of the classes, artifacts by `name`, `media_type` and `digest`, and the `mode`). It is the same hash that the agent uses in `generation-target.hash`, that Joker Micro stores in the `source_hash` of the blob, and that `Joker-Generation` and the `ETag` show. Provenance does not enter the hash: it does not change behavior.

**Origin.** Each generation created by the API has `origin = local` (socket or TCP) or `session` (tunnel of a remote session), and is refused if `[policy] apply_from` of the node (ref/02 §3.5.1) does not allow it: `local` needs `local`, `session` needs `deck` (`J-E1615`).

**Validation on the node.** The node runs the same rules as `jk check` (the `joker-manifest` crate) with the facts that only it has (board, kernel, drift, WCET of the lock, firmware of the MCUs). Failures: `J-E1601` (422) with all the diagnostics in `errors`, each with its J-code and `spans` in the effective manifest; `jk` translates the spans to the source file with the provenance.

**`Plan`:** `base`, `target`, `requires_confirm` and `confirm_reasons` (`network`, `security`, `uplinks`, `api`), `disruption` (`none`, `hot`, `partial`, `full`), `changes` (each with `kind`, `target`, `effect` `none` \| `hot` \| `reload` \| `restart`, `detail` and from/to fields), `signal_ids` (kept, new, removed, renumbered), `admission` (utilization per `rt` CPU against the limit, M23), `micros` (OTA of firmware or of a generation that the swap includes; firmware carried by an activation is orchestrated MCU OTA, reserved for 1.1, and a 1.0 plan only lists generations; ADR 0035).

**States and transitions** (`generation_state`, ref/00, §6):

```
POST /generations ──▶ pending ──activate──▶ confirming ──confirm──▶ confirmed
                        │   (no window) ──────────────────────────▶ confirmed
                        │                       │
                        │                       ├─ window expired ──▶ rolled_back  (the base is reactivated)
                        │                       ├─ health failed ───▶ rolled_back
                        │                       └─ rollback ────────▶ rolled_back
                        ├─ activation failed ────────────────────────▶ failed (the base continues)
                        └─ discarded / 1 h without activation ───────▶ discarded (deleted / expired)

 rollback of a confirmed generation G: reactivates a stored F (same gen, same hash);
 G becomes rolled_back, F goes back to confirmed (or to confirming, if a window is requested)
```

- **One transition at a time.** At most one `pending` generation and one `confirming` per node. A `POST` with a `pending` one from another principal, or during a swap, is `J-E1608` (409). A `pending` one from the same principal is replaced (the previous one becomes `discarded`, `discard_reason = superseded`). The agent goes through the **same mutex** (ref/10 §3.4.2): an agent swap in progress gives `J-E1608` to a local `POST`, and a local transition in progress makes the agent wait.
- **`activate`:** `If-Match` = `base`. `confirm_window` between 30 s and 24 h (`J-E1623`); required in `prod` when `plan.requires_confirm` (ADR 0015; `J-E1610`). The swap happens at a tick boundary of each affected group (document 01, §15). With `wait` (default), the response arrives with the state `confirming`, `confirmed` or `failed`.
- **Health during the window:** the generation rolls back if, before the confirmation, a group trips on overrun, a new app enters `crashloop`, or a new driver does not reach `up` within the window. The reason stays in `failure`.
- **`confirm`** outside `confirming` is `J-E1611` (409); if the generation has already rolled back, the error says `rolled_back` and `jk` exits with code 5.
- **`confirm` and the MCUs.** When the activation took a generation blob (or a firmware) to an MCU behind a `joker-link` and put it on trial, the `confirm` of the generation of the node **also** confirms that pending OTA: the H sends `OTA confirm` to each MCU on trial from that activation (ref/07 §9.4), and the response only arrives `confirmed` after all of them answer `confirmed`. An MCU that has already rolled back on its own (`rolled_back`, `J-E2409`) makes the response fail with that code, and the generation of the node stays `confirming`: whoever confirms decides whether to roll back. Without a confirmation window on the node (an activation without `confirm_window`), the H confirms the MCU on its own by health, as in ref/07 §9.4.
- **`rollback`** (D9): `to` (default: `previous`). It **reactivates** the stored generation with the same `gen` and the same `hash`: it does not create a new generation. The history gains an **activation** with `origin = rollback`, the principal and the reason, and the generation that was left becomes `rolled_back`. A generation whose blobs have already been deleted is `J-E1613`. Going back to a generation that has already been confirmed does not require a window; one may be requested.
- **`dev`:** `jk dev --node` uses the same path, with `base: null`, without signature and without window. It is fast because the diff is minimal (document 01, §15), and not because there is a shortcut.
- **Origin of each activation** (`origin`, the `gen-origin` of ref/10 §2.2): `local`, `session`, `agent`, `import`, `shadow`, `rollback`, `boot`. The generations created by the node (shadow, automatic promotion) are `shadow`; the startup that goes back to the confirmed generation is `boot`.

### 5.14 Apps and shadow

App shadow (M16) is **1.1** (document 26, §5; ADR 0035): the `/v1/shadows/**` operations below are in the OpenAPI with `x-joker-since: '1.1'` and respond 501 with `J-E1017` on a 1.0 node, and the `shadow` state of an app is never reported. The apps operations are 1.0.

| Method | Path | Role | M | Does |
|---|---|---|---|---|
| GET | `/v1/apps`, `/v1/apps/{app}` | viewer | | version, digest, WIT world, SDK, state (the `app_state` of ref/00 §6: `starting`, `running`, `crashloop`, `stopped`, `failed`, `shadow`, `updating`), cycle and async instances (state, group, memory, tasks with WCET and overruns), writer, traps, restarts, backoff, shadow, implemented instances, `meta` (`joker:meta` in JSON) (`App`) |
| POST | `/v1/apps/{app}/start` | operator | M | starts a stopped app |
| POST | `/v1/apps/{app}/stop` | operator | M | stops; `reason` required |
| POST | `/v1/apps/{app}/restart` | operator | M | recreates the two instances; takes it out of `crashloop` |
| GET | `/v1/apps/{app}/kv` | admin | | lists keys (`prefix`) or reads one (`key`; value in base64); **there is no write** |
| GET | `/v1/shadows` | viewer | | active and recent shadows |
| POST | `/v1/shadows` | admin | M | `ShadowStartRequest`: `target` (`app:<name>`), `candidate` (artifact), `until`, `promote`, `reason`; `If-Match` |
| GET | `/v1/shadows/apps/{app}` | viewer | | state, divergence per output (p50, p95, max.), criteria and whether they pass (`Shadow`) |
| GET | `/v1/shadows/apps/{app}/diff` | viewer | | `shadowed` series against `applied` (`ShadowDiff`) |
| POST | `/v1/shadows/apps/{app}/promote` | admin | M | promotes everything or only `outputs` (partial promotion, document 08, round 6); `force` without the criteria; `confirm_window` |
| POST | `/v1/shadows/apps/{app}/abort` | admin | M | aborts |

- **`stop` is volatile:** it holds until the next startup of `jokerd` or the next generation, and appears in the state as `stopped_by` (principal, when, reason). Stopping an app durably is changing the generation. The outputs of the app follow the lease and go to `safe` on their own (document 01, §8).
- Redundant lifecycle operations (`start` on a running app) are idempotent (200). During a generation swap that touches the app, `J-E1501` (409).
- **The shadow goes through generations.** `POST /v1/shadows` creates a **derived generation** of the current one with the `[apps.<a>.shadow]` section (document 03, §8) and the candidate as an artifact, and activates it by the normal path (activation with `origin = shadow`, `derived_from`). Promoting and aborting are other derived generations. An automatic promotion (criteria met, `promote: true`) is also a new generation, created by the node. Thus the shadow appears in the generation history, survives restarts and rolls back like everything else; the cost is that the manifest of the node diverges from Git, and `jk diff` shows it.
- A shadow already active for the app is `J-E1511`; promoting without the criteria and without `force` is `J-E1512`.
- The shadow of **models** is at `/v1/shadows/models/{model}`, reserved for 1.1.

### 5.15 Runtime

| Method | Path | Role | M | Does |
|---|---|---|---|---|
| GET | `/v1/runtime` | viewer | | version, active slot, A/B slots (version, digest, installation, health), update in progress (`RuntimeState`) |
| GET | `/v1/runtime/drivers` | viewer | | the **driver manifest** embedded in the binary (ref/06 §3.8): per driver, name, version, tier, protocols, capabilities and descriptor digest; and the tier W and X drivers loaded by the generation (`RuntimeDriver`). It is what the report to the Deck carries in `drivers` (ref/10 §2.10.1) |
| POST | `/v1/runtime/upgrades` | admin | M | `RuntimeUpgradeRequest`: `artifact` (a `joker.runtime.v1` blob already uploaded), `version`, `signature`, `health_window`, `allow_downgrade`, `reason` → job `runtime_upgrade` |
| POST | `/v1/runtime/rollback` | admin | M | goes back to the other slot → job `runtime_rollback` |

The job installs in the inactive slot, swaps and restarts `jokerd`: the connection drops. `jk` recognizes the restart by the new `boot_id` in `/v1/info`, and follows the job until `succeeded` (health passed within the `health_window`) or `failed` (it went back to the previous slot; document 01, §16). An unsigned artifact in `prod`, from another architecture, or older without `allow_downgrade`: `J-E1620`. An update in progress: `J-E1621`. The node does not download the runtime from a registry: `jk` uploads it (local-first); the Deck uses its own path (ref/10).

### 5.16 Traces

| Method | Path | Role | M | Does |
|---|---|---|---|---|
| GET | `/v1/traces`, `/v1/traces/{trace}` | viewer | | traces on the node (`Trace`) |
| POST | `/v1/traces` | operator | M | creates (`TraceRequest`) |
| POST | `/v1/traces/{trace}/stop` | operator | M | ends a recording before its end; the file stays valid |
| GET | `/v1/traces/{trace}/file` | viewer | | downloads the `.jktrace` (`Range`); before `ready` or `stopped`, `J-E1706` (409) |
| DELETE | `/v1/traces/{trace}` | operator | M | deletes |

| `source` | Fields | Command | Rule |
|---|---|---|---|
| `record` | `duration`, `groups` (default: all), `async` | `jk record --for 2h --groups fast` | records from now on; `async = true` also records the async reads (document 09, attack 10). It starts with the state segment: the `export-state` of each app of the recorded groups, between two ticks (document 16, §7), from which `jk replay` starts |
| `ring` | one `groups`, `last` or `from`/`to` | `jk trace-dump --group fast --last 30m` | extracts from the `trace.continuous` ring of the group (exact); without a ring, `J-E1701`; outside the coverage, `J-E1702`. No state segment: the replay starts from `init` |
| `history` | `from`, `to`, `app` | `jk trace-from-history "03:00..03:30" --app peak_shaver` | rebuilds from the series of the historian; `approximate: true` and warnings about the inputs that were not recorded at the rate of the group |
| `micro` | `device` | — | requests the ring of the MCU through `TRACE` (document 19, U9); the node compresses the segments |

- Downloading a trace that is not yet `ready` or `stopped` is `J-E1706` (409).
- Traces count toward a disk quota (node configuration, ref/02); when it is full, `J-E1703` (507).
- **Replay** runs on the laptop (`jk replay`): there is no replay endpoint.

### 5.17 Jobs and diagnostics

| Method | Path | Role | M | Does |
|---|---|---|---|---|
| GET | `/v1/jobs`, `/v1/jobs/{job}` | viewer | | jobs (`Job`: `kind`, `state`, progress, `result`, `error`) |
| POST | `/v1/jobs/{job}/cancel` | the role that created it | M | cancels |
| GET | `/v1/doctor` | viewer | | last stored report |
| POST | `/v1/doctor` | admin | M | `DoctorRequest`: `checks` (`base` always, `rt`, `ml`, `network`), `rt_duration` (default 60 s), `allow_live` → job `doctor` |
| POST | `/v1/discovery` | admin | M | `DiscoveryRequest`: `uri`, `unit`, `rate` (default 10/s), `timeout` → job `discovery` |
| POST | `/v1/profile-tests` | admin | M | `ProfileTestRequest`: `profile`, `uri` (grammar of ref/03 §2.2.20), `unit` (equivalent to the `?unit=` of the URI; both, different, `J-E1708`), `timeout` → job `profile_test`: reads only; when the port or the address is already a bus of the active generation, the requests enter the scheduler of that bus, and do not compete with the driver |
| POST | `/v1/bench` | admin | M | `BenchRequest`: `manifest`, `apps`, `tasks`, `iterations`, `allow_live` → job `bench` |

- **`doctor`:** checks of document 07, §3, and of document 09, attack 5 (ids in `DoctorReport.checks`: `kernel.preempt_rt`, `cpu.isolcpus`, `cpu.governor`, `irq.affinity`, `watchdog`, `clock`, `disk`, `permissions`, `groups`, `accelerators`, `device_fallbacks`, `restart_time`, `rt.major_faults`, `rt.hwlat`, `rt.cyclictest`, `files.drift`, `fieldbus.concurrent_clients`). `rt` with active outputs in `prod` requires `allow_live` (`J-E1707`): `hwlatdetect` and `cyclictest` disturb the `rt` core. `network` belongs to 1.3 (gateway mode) and in 1.0 gives `skip`.
- **Discovery:** reads only, with a rate limit (document 05, §7). Unknown scheme: `J-E1708`. The result lists what it found (identity, SunSpec models, suggested profile, digest of the map); pinning it is `jk lock`.
- **`bench`:** measures the WCET of the cycle tasks on the target with synthesized input images; **the proposals are discarded**, nothing reaches the arbiter. With a generation in `prod` and active outputs, it requires `allow_live`, runs at the lowest `SCHED_FIFO` priority and marks the results with interference. The result comes with the `[wcet]` excerpt of the lock.
- A job of the same type in progress: `J-E1705` (409).

### 5.18 Uplinks, bridges, MCUs

| Method | Path | Role | M | Does |
|---|---|---|---|---|
| GET | `/v1/uplinks`, `/v1/uplinks/{uplink}` | viewer | | state, endpoint without credentials, cursor lag, backlog, budget (limit, used, projection, degraded classes; M12), drops (`Uplink`) |
| GET | `/v1/bridges` | viewer | | bridges: state, clients, writer, `writable`, writes accepted, rejected and rejected by anti-replay (`Bridge`). **Reserved 1.2** (bridges; ADR 0035): 501 with `J-E1017` in 1.0 |
| GET | `/v1/micros`, `/v1/micros/{device}` | viewer | | MCUs over Joker Link (`Micro`): transport, `link_state`, **session mode** (`session_mode`, ref/00 §6) and the reason when it is not `full`, **negotiated version** (`link_version`), authentication, board, firmware (version, slot, image digest, **firmware descriptor** and its digest, hash of the capabilities, `security_version`), ABI, generation (`gen_seq`, hash, `embedded` \| `flash`, **confirmation state** and time left in the window), registry, catalog, classes, drivers (the summary of key 15), clock, **parameters changed locally**, the **last `HELLO`** (decoded), **counters** (frames, CRC-32C, authentication, replay, retransmissions, resynchronizations; those of `jk_link_status`, ref/08 §2.6.p) and RTT, OTA in progress |
| POST | `/v1/micros/{device}/ota` | admin | M | `MicroOtaRequest`: `kind` (`firmware`, `generation`), `artifact`, `signature` (the COSE_Sign1 of the OTA manifest, ref/07 §9.1; in a generation it already comes in the *trailer* of the blob and may be omitted), `confirm_window` (the `confirm_ms` of `activate`), `reason` → job `micro_ota`: `begin`, transfer, test, confirmation or reversion (ref/07 §9) |

- OTA of a disconnected MCU: `J-E1802` (503). Incompatible artifact (board, ABI, security counter): `J-E1803`. OTA in progress: `J-E1804`. A device that is not `joker-link`: `J-E1801`.
- The OTA of MCUs that is part of a generation (the plan lists it in `micros`) is orchestrated by the activation; this endpoint is for the manual case. In 1.0 the activation only carries generation blobs, and a firmware update is always this manual operation; firmware carried by an activation is 1.1 (ADR 0035).
- The `budget` of an `Uplink` is 1.1 (M12) and is always `null` on a 1.0 node; the uplink kinds of 1.0 are `mqtt` and `mqtt-sparkplug` (ref/01 §5.17.3).

### 5.19 Simulation (`mode = "dev"`)

| Method | Path | Role | M | Does |
|---|---|---|---|---|
| GET | `/v1/sim` | viewer | | `sim` devices, panel descriptors (signal, `lamp`, `button`, `switch`, `slider`, `value`, key, range), forced inputs |
| PUT | `/v1/sim/inputs/{path}` | operator | M | forces the value of a signal of a `sim` device (quality `Good(overridden)`, document 02, §4); `ttl` optional |
| DELETE | `/v1/sim/inputs/{path}` | operator | M | releases; the simulation model is in control again |

In `mode = "prod"`, `J-E1018` (409). A signal that is not of a `sim` device: `J-E1930`. It is what `jk dev --panel` uses for the buttons and sliders when it runs against a node (`--node`).

### 5.20 Reserved (1.1+)

`/v1/models`, `/v1/models/{model}`, `/v1/models/{model}/bench`, `/v1/training`, `/v1/shadows/models/{model}` (and `promote`, `abort`), `/v1/signals/{path}/blocks`, `/v1/streams`. In 1.0 they respond 501 with `J-E1017`. They are in the OpenAPI with `x-joker-since: '1.1'`.

The 1.0 cut (document 26, §5; ADR 0035) reserves the same way the app shadow operations of §5.14 (`x-joker-since: '1.1'`) and `/v1/bridges` of §5.18 (`x-joker-since: '1.2'`). Unlike the paths above, they keep their full schemas in the OpenAPI, because the shape is already closed.

### 5.21 Agent and node trust

The agent (ref/10) and the trust of the node are also read and commanded through the local API: it is through here that `jk agent …`, `jk node …` and `jk session …` talk to the node (ref/10 §2.18).

| Method | Path | Role | M | Does |
|---|---|---|---|---|
| GET | `/v1/agent` | viewer | | state of the agent (`jk agent status`): connected, authority (`deck_id`), transport, last contact, `desired_seq`, `report_seq`, `event_seq`, queue (events, bytes, dropped, the oldest), pause, `hold`, open remote sessions (`AgentState`) |
| GET | `/v1/agent/desired` | viewer | | the current DSD decoded, in JSON (the view of ref/10 §2.6.7), with the state of the apply (`apply-state`, phase, wait, code, causes, bytes downloaded) (`AgentDesired`) |
| POST | `/v1/agent/pause` | admin | M | suspends the swaps (`ttl` ≤ 7 d, `reason`); downloads, reports and rollbacks continue (ref/10 §3.7.2) |
| POST | `/v1/agent/resume` | admin | M | resumes; also ends a `hold` of local changes (ref/10 §3.7.1) |
| POST | `/v1/agent/import` | admin | M | applies a DSD brought by hand (`jk agent import`, ref/10 §3.7.3): `desired` (the COSE in base64) and the digests of the artifacts already uploaded through `/v1/blobs/uploads` → job `agent_import` |
| GET | `/v1/agent/sessions` | operator | | remote sessions of the Deck: open and awaiting consent (**1.3**) |
| POST | `/v1/agent/sessions/{session}/accept` | operator | M | gives local consent to a session (`consent = "local"`, ref/10 §3.10) (**1.3**) |
| DELETE | `/v1/agent/sessions/{session}` | operator | M | closes a remote session (**1.3**) |
| GET | `/v1/node/trust` | viewer | | anchors, active bundle (version, authority, keys and roles, `deck_urls`, pins, `min_mode`), floors per role, revoked (`TrustState`) |
| PUT | `/v1/node/trust/bundle` | admin | M | installs a trust bundle (`application/cose`): signed by an anchor and with a higher `version` (ref/02 §2.5.4; `J0918`, `J0939`) (`jk node trust push`) |
| PUT | `/v1/node/trust/anchors` | admin | M | replaces the anchors (`jk node trust set`): a COSE document signed by a current anchor, with the new anchors and the revoked ones (ref/02 §3.2.3); `jokerd` rewrites `node.toml` |

- None of these endpoints works in a remote session, except `GET /v1/agent` and `GET /v1/agent/desired` (§2.3, `J0986`).
- `pause`, `resume` and `import` with the agent off (no authority) are `J-E1940`. The remote session endpoints respond `J-E1017` before 1.3.
- Remote sessions (`/v1/agent/sessions`) are a different thing from the connections of the API itself (`/v1/sessions`, §5.3).

---

## 6. WebSocket and Server-Sent Events

### 6.1 Connection

- `GET /v1/ws` with `Upgrade: websocket` and `Sec-WebSocket-Protocol: joker.v1`. Another subprotocol, or none: 400 `J-E1900`.
- Authentication and role are those of the HTTP connection (§2). An `Origin` present is refused (§2.4).
- Text messages, one JSON object per message, with the `op` field. Binary messages: close 4000.
- **There are no writes over WebSocket in 1.0.** Writes go over HTTP, where they have anti-replay, idempotency, audit and uniform error codes. WebSocket is only for viewing.

### 6.2 Client messages

| `op` | Fields | Response |
|---|---|---|
| `subscribe` | `id`, `topic`, filters (§6.4), `min_interval`, `mode`, `initial`, `mono`, `fields`, `buffer` | `subscribed` or `error` |
| `unsubscribe` | `id`, `sub` | `unsubscribed` |
| `resume` | `id`, `session`, `subs: [{sub, last_seq}]` | `resumed` (§6.7) |
| `ping` | `id` | `pong` |
| `list` | `id` | `subscriptions` |

An unknown `op` is `J-E1901`; an unknown topic, `J-E1902`; a malformed message, `J-E1900`. These errors arrive as `error` and **do not close the connection**, unless they repeat more than 10 times in a minute (close 4000).

### 6.3 Server messages

| `op` | When | Fields |
|---|---|---|
| `welcome` | right after the upgrade | `session`, `boot_id`, `api`, `role`, `generation`, `heartbeat` (`15s`), `resume_window` (`60s`), `limits` |
| `subscribed` | subscription accepted | `id`, `sub`, `topic`, `resolved` (resolved signals), effective `min_interval` |
| `data` | data | `sub`, `seq`, `items` |
| `resync` | complete state (initial, after a `gap`, after a `resume` without replay) | `sub`, `seq`, `items` |
| `gap` | lost items | `sub`, `lost`, `reason` (`overflow`, `intake_overflow`, `resume_unavailable`, `generation`, `role_changed`), `ts` |
| `resolved` | the generation changed and the selector resolves to another list | `sub`, `generation`, `added`, `removed` |
| `heartbeat` | 15 s without another message | `ts` |
| `pong`, `unsubscribed`, `resumed`, `subscriptions` | responses | |
| `error` | error of a request or of a subscription | `id`, `sub`, `problem` (RFC 9457) |
| `closing` | before the server closes | `code`, `problem` |

**`seq`** counts the `data` and `resync` messages of each subscription, starting at 1. It is what the client keeps in order to resume.

### 6.4 Topics

| Topic | Type | Items | Filters | Role |
|---|---|---|---|---|
| `signals` | state | `Sample` | `sel`, `mode`, `fields`, `mono` | viewer |
| `outputs` | state | summarized `OutputState` (effective, applied, active level, last result, trip) | `sel` | viewer |
| `status` | state | `Status` | — | viewer |
| `metrics` | state | `Metrics` | — | viewer |
| `signal_trace` | state | one record per tick with changes in the signal: acquired sample, readers, proposals, arbiter, envelope, commit, readback; `skipped_ticks` | `path` | viewer |
| `alarms` | events | `AlarmEvent` | `sel`, `severity`, `states` | viewer |
| `events` | events | `EventRecord` | `sel` | viewer |
| `decisions` | events | `Decision` | `sel` (outputs), `outcome`, `writer` | viewer |
| `logs` | events | `LogEntry` | `source`, `app`, `level` | viewer |
| `manual` | events | `ManualOverride` with `event` (`set`, `renewed`, `expiring`, `expired`, `released`, `taken_over`, `restored`) | `sel` | viewer |
| `intents` | events | `Intent` | `states` | viewer |
| `jobs` | events | `Job` | `job` | viewer |
| `invocations` | events | `Invocation` | `invocation` | viewer |
| `generations` | events | `Generation` | — | viewer |
| `audit` | events | `AuditEntry` | — | admin |
| `frames`, `blocks` | — | reserved for 1.1 | — | — |

### 6.5 Rate and coalescing

- **State topics** (`signals`, `outputs`, `status`, `metrics`, `signal_trace`) are **sampled**, not queued. At each `min_interval`, the server reads the current state and sends what changed (`mode = change`, default: only signals with a new `seq`) or everything (`mode = interval`). Between two sends, the intermediate samples are dropped on purpose: it is the same *latest-wins* policy of the output boxes (document 01, §3.4). The jump in the `seq` of each signal tells the client how many samples were coalesced.
- `min_interval`: default `100ms` in `signals`, `outputs` and `signal_trace`; `1s` in `status` and `metrics`. Minimum `10ms` (`J-E1905`); in `outputs`, the minimum is the period of the owning group; in `status` and `metrics`, `1s`.
- **Event topics** are not coalesced: each item is delivered, or its drop is declared with a `gap`.
- `initial: true` (default): the first message is a `resync` with the current state (state topics) or the active alarms (`alarms`).

### 6.6 Backpressure: a slow client never stalls the core

```
 core (rt, io)                       svc (tokio, bg)                                 network
 ─────────────                       ───────────────                                 ───────
 slots (seqlock) ◀── wait-free read ── sampler per subscription ──▶ output queue of the connection ──▶ socket
 bounded SPSC queues ──▶ event intake ──▶ ring per subscription (buffer) ──┘      (1 MiB)
 (alarms, decisions, events, logs)        │
                                          └─ intake full → counted drop (joker.api.dropped) → gap intake_overflow
```

1. **The core does not know clients.** The state topics read the slots with the seqlock protocol (document 01, §3.2), without waiting and without writing anything that the core reads. The event topics consume the bounded lock-free queues that already exist for the historian and the uplinks (ADR 0013). No action of a client reaches an `rt` thread.
2. **State:** a slow client only receives less often. The sampler does not generate the next message while the previous one has not left the queue of the connection, and the effective rate drops on its own.
3. **Events:** each subscription has a ring of `buffer` items (64–4,096, default 1,024). When it is full, the oldest go and the client receives a `gap` with `lost`. In `alarms`, the `gap` is followed by a `resync` with the current table of alarms, so that the state is never wrong.
4. **Connection:** if the output queue (1 MiB) stays full for more than **30 s**, the server sends `closing` and closes with **4008** (`J-E1906`). A client that does not read does not hold memory forever.
5. **Node budget:** the sum of the sampling rates of all subscriptions is limited to 1,000,000 samples/s. A subscription that exceeds it is refused (`J-E1903`), with the rate that still fits in the `detail`.

### 6.7 Resumption after a reconnection

- The server keeps the state of a session (subscriptions, event rings and the last 256 messages sent of each subscription, up to 1 MiB) for `resume_window` (60 s) after the connection drops.
- The client connects again and sends `resume` with the old `session` and the last `seq` of each subscription. For each one, the server answers in `resumed` with `replayed` (it resends the messages after `last_seq`) or `resync` (it sends the current state and, on event topics, a `gap` with `resume_unavailable`).
- Outside the window, or after a restart of `jokerd` (different `boot_id`), the session no longer exists: `J-E1907`, and the client subscribes again. For old events, the path is REST with a range (`/v1/alarm-events`, `/v1/decisions`, `/v1/events`), which reads from the historian.
- Resumption requires the same principal. Another principal with the same `session` is `J-E1907`.

### 6.8 Heartbeats

- The server sends a WebSocket `ping` frame every 15 s and a `heartbeat` message when 15 s have passed without another message (for JavaScript clients, which do not see control frames).
- The client must show a sign of life (a `pong` frame, which browsers send on their own, or any message) every 45 s. Otherwise, close **4002** (`J-E1908`).

### 6.9 Generation swap and role

- On a generation swap, each subscription with selectors is resolved again. If the list changes, a `resolved` arrives with `added` and `removed`; the paths that survive continue without interruption. A `SignalId` is never sent.
- The role is recomputed. If it drops, the subscriptions that required the old role end with `gap` (`role_changed`) and `error`; the connection continues.

### 6.10 Close codes

| Code | Meaning |
|---|---|
| 1000 | normal |
| 1001 | `jokerd` is going to stop or restart |
| 1008 | policy (role revoked, origin) |
| 1009 | client message above 64 KiB |
| 4000 | protocol error |
| 4002 | missing heartbeat (`J-E1908`) |
| 4008 | slow client (`J-E1906`) |
| 4009 | session closed by an `admin` |
| 4029 | too many connections (`J-E1014`) |

### 6.11 Server-Sent Events

**Decision: there is SSE**, at `GET /v1/stream`, as a read-only mirror of **one** subscription per request: `topic`, `sel`, `min_interval`, `initial`, `mono`, and the remaining filters in `filter` (JSON with the fields of `subscribe`). It serves scripts and `curl`, which do not speak WebSocket easily, and it uses the same subscription engine, with the same coalescing, the same rings and the same cutoff for slowness.

```
event: subscribed
data: {"op":"subscribed","sub":"s1","topic":"alarms","resolved":14}

event: data
id: 01J8Z3K6QW4R5T6Y7U8I9O0PAS.42
data: {"op":"data","sub":"s1","seq":42,"items":[…]}

: hb
```

- `id` is `<session>.<seq>`. The browser resends it in `Last-Event-ID` when it reconnects, and that is an implicit `resume` with the rules of §6.7.
- Heartbeat: the comment line `: hb` every 15 s.
- SSE counts toward the connection limit and the sampling budget like a WebSocket connection.

---

## 7. New codes (J-E1000–J-E1999)

All with severity `error`, except where stated. They go into `codes.toml` (document 16, §9).

| Code | HTTP | Message |
|---|---|---|
| `J-E1000` | 500 | internal API error |
| `J-E1001` | 401 | no identity (tunnel without a valid session) |
| `J-E1002` | 403 | insufficient role for this operation |
| `J-E1003` | 403 | identity refused (CN outside the grammar, certificate without a rule applicable to the transport) |
| `J-E1004` | 403 | request with `Origin`: CORS disabled |
| `J-E1005` | 404 | endpoint does not exist |
| `J-E1006` | 405 | method not allowed |
| `J-E1007` | 406 | response type not available |
| `J-E1008` | 415 | request content type not accepted |
| `J-E1009` | 413 | request above the limit (body, URL, headers, inline content) |
| `J-E1010` | 400 | malformed JSON |
| `J-E1011` | 400 | unknown field or parameter |
| `J-E1012` | 400 | invalid field or parameter value |
| `J-E1013` | 400 | required field missing |
| `J-E1014` | 429 | rate or connection limit exceeded |
| `J-E1015` | 503 | API unavailable (startup, shutdown) |
| `J-E1016` | 504 | timed out waiting for the core or the equipment |
| `J-E1017` | 501 | feature reserved for a later version or not in this build |
| `J-E1018` | 409 | operation not allowed in this mode (`dev`/`prod`) |
| `J-E1020` | 400 | `Joker-Nonce` or `Joker-Timestamp` missing in a mutating request over TCP |
| `J-E1021` | 400 | `Joker-Timestamp` outside the ±30 s window |
| `J-E1022` | 409 | repeated nonce (replay) |
| `J-E1023` | 400 | required `Idempotency-Key` missing |
| `J-E1024` | 422 | `Idempotency-Key` reused with different content |
| `J-E1025` | 400 | invalid `Idempotency-Key` |
| `J-E1026` | 412 | the generation changed (`If-Match`) |
| `J-E1027` | 428 | required `If-Match` missing |
| `J-E1028` | 409 | request with the same key still in progress |
| `J-E1029` | 503 | nonce memory full; try again later |
| `J-E1030` | 400 | malformed selector or too many selectors |
| `J-E1031` | 422 | selector resolves to too many signals |
| `J-E1032` | 400 | invalid or expired cursor |
| `J-E1033` | 404 | unknown signal (with suggestions) |
| `J-E1034` | 400 | invalid time interval |
| `J-E1035` | 400 | invalid duration literal |
| `J-E1036` | 400 | value incompatible with the signal type |
| `J-E1037` | 422 | coherent read with too many signals |
| `J-E1038` | 422 | history result too large: aggregate or paginate |
| `J-E1039` | 400 | step, tier or aggregate not applicable |
| `J-E1100` | 409 | the signal is not an output |
| `J-E1101` | 400 | manual write without TTL (A10) |
| `J-E1102` | 422 | TTL above `max_manual` |
| `J-E1103` | 400 | TTL below 1 s |
| `J-E1104` | 422 | level outside the `manual` band |
| `J-E1105` | 409 | manual level held by another principal (use `takeover`) |
| `J-E1106` | 400 | write without a reason |
| `J-E1107` | 404 | manual level not held |
| `J-E1108` | 409 | releasing another principal's level requires `force` |
| `J-E1110` | 404 | write ticket does not exist or expired |
| `J-E1120` | 409 | the output is not in trip |
| `J-E1200` | 404 | unknown alarm |
| `J-E1201` | 400 | shelving without TTL |
| `J-E1202` | 422 | shelving TTL above `max_shelve` |
| `J-E1203` | 409 | alarm not shelved |
| `J-E1204` | 409 | nothing to acknowledge (`normal` or `unknown`) |
| `J-E1300` | 404 | unknown intent |
| `J-E1301` | 409 | intent already decided or expired |
| `J-E1302` | 422 | the approval cannot extend the intent TTL |
| `J-E1400` | 404 | unknown action |
| `J-E1401` | 400 | invalid action arguments |
| `J-E1402` | 409 | action permissive false |
| `J-E1403` | 503 | device action queue full |
| `J-E1404` | 404 | unknown invocation |
| `J-E1405` | 409 | the invocation cannot be canceled |
| `J-E1406` | — | (result) the action exceeded the timeout |
| `J-E1407` | — | (result) the equipment returned an error |
| `J-E1408` | — | (result) the implementation refused the action |
| `J-E1409` | — | (result) action canceled |
| `J-E1410` | 404 | unknown instance |
| `J-E1411` | 404 | unknown method |
| `J-E1412` | 400 | invalid method arguments |
| `J-E1413` | 404 | unknown class in this generation |
| `J-E1414` | 409 | conformance against equipment in `prod` requires `allow_prod` |
| `J-E1415` | — | (result) the action's device is not connected |
| `J-E1500` | 404 | unknown app |
| `J-E1501` | 409 | app or device lifecycle not allowed now (generation swap in progress) |
| `J-E1502` | 404 | `kv` key does not exist |
| `J-E1510` | 404 | shadow does not exist (reserved 1.1) |
| `J-E1511` | 409 | there is already an active shadow for the app (reserved 1.1) |
| `J-E1512` | 409 | promotion criteria not met (use `force`) (reserved 1.1) |
| `J-E1600` | 404 | unknown generation |
| `J-E1601` | 422 | the generation did not pass validation (diagnostics in `errors`) |
| `J-E1602` | 422 | missing blobs (list in `detail`) |
| `J-E1603` | 400 | blob digest does not match the content |
| `J-E1604` | 404 | upload does not exist or expired |
| `J-E1605` | 416 | invalid or out-of-order `Content-Range` |
| `J-E1606` | 413 | chunk or blob above the limit |
| `J-E1607` | 422 | signature missing or rejected by the trust policy |
| `J-E1608` | 409 | another generation transition in progress |
| `J-E1609` | 409 | `base` is not the current generation |
| `J-E1610` | 422 | the generation requires a confirmation window (ADR 0015) |
| `J-E1611` | 409 | nothing to confirm |
| `J-E1612` | 409 | invalid state transition for this generation |
| `J-E1613` | 409 | target generation is no longer stored |
| `J-E1614` | 410 | pending generation discarded or expired |
| `J-E1615` | 403 | the generation source is not allowed by the node's `[policy] apply_from` |
| `J-E1620` | 422 | invalid runtime artifact (signature, architecture, version) |
| `J-E1621` | 409 | runtime update in progress |
| `J-E1622` | 409 | the other slot has no usable runtime |
| `J-E1623` | 400 | confirmation window outside 30 s–24 h |
| `J-E1700` | 404 | unknown trace |
| `J-E1701` | 409 | the group has no continuous trace ring |
| `J-E1702` | 422 | interval outside the ring or the historian retention |
| `J-E1703` | 507 | trace quota exhausted |
| `J-E1704` | 404 | unknown job |
| `J-E1705` | 409 | job of the same type in progress |
| `J-E1706` | 409 | trace not ready yet |
| `J-E1707` | 409 | disruptive operation with active outputs requires `allow_live` |
| `J-E1708` | 400 | invalid discovery or equipment URI |
| `J-E1800` | 404 | unknown device |
| `J-E1801` | 409 | the device is not `joker-link` |
| `J-E1802` | 503 | MCU disconnected |
| `J-E1803` | 422 | artifact incompatible with the MCU |
| `J-E1804` | 409 | MCU OTA in progress |
| `J-E1900` | — | malformed WebSocket message or wrong subprotocol |
| `J-E1901` | — | unknown `op` |
| `J-E1902` | — | unknown topic or no role for it |
| `J-E1903` | — | too many subscriptions, or sampling budget exhausted |
| `J-E1904` | — | subscription does not exist |
| `J-E1905` | — | `min_interval` below the minimum |
| `J-E1906` | — | slow client: connection closed (4008) |
| `J-E1907` | — | session not resumable |
| `J-E1908` | — | missing heartbeat (4002) |
| `J-E1909` | 404 | unknown session |
| `J-E1910` | 409 | enrollment state does not allow the operation |
| `J-E1911` | 422 | certificate does not match the key or invalid chain |
| `J-E1912` | 403 | enrollment only over the bootstrap channel (the Unix socket, local or over SSH) |
| `J-E1913` | 401 | enrollment code (`Joker-Enroll-Code`) wrong, expired or already used |
| `J-E1920` | 422 | invalid sealed envelope |
| `J-E1921` | 404 | undeclared secret |
| `J-E1930` | 409 | the signal does not belong to a `sim` device |
| `J-E1940` | 409 | the agent is disabled on this node (no authority) |

The validation J-codes (J01xx–J07xx) appear inside `J-E1601` and, on their own, in the conversion of values (`J0107`, `J0108`). Those of trust and secrets (`J0906`, `J0915`–`J0926`, ref/02) and that of sessions (`J0986`, ref/10) appear as they are. There are no new codes outside this range.

---

## 8. Expected behavior

### 8.1 Relation to the core

- **The API is an `svc` service (document 01, §2) and only talks to the core through bounded queues and seqlock reads** (document 01, §1; ADR 0013). A manual write, a method, a reset or an approved intent enter through the inbox of the owning group as an async proposal; nothing of the API runs on an `rt` thread.
- **No write path of the API bypasses the arbiter and the envelope** (invariant 3). All requests that change an output enter the decision log (invariant 8).
- The rules of arbitration, envelope, quality and trace are those of document 16. This reference does not repeat them: it only says where they come in.

### 8.2 Order and consistency

- Requests from the same principal to the same output are serialized by the API in order of arrival. Requests from different principals to the same output too, and the arbiter resolves the priority.
- The result of a write is that of the tick at which it was applied. The preview is a prediction; between it and the tick, inputs, levels and leases may change.
- `GET /v1/values` without `coherent` reads each slot atomically, but not the several slots at once. With `coherent`, the samples come from the same seqlock epoch (document 02, §6.4), and those that share `acq_id` come from the same request to the driver (§6.2).
- A response always carries `Joker-Generation`; a client that joins data from two responses knows whether it crossed a generation swap.

### 8.3 Restarts and failures

- `jokerd` restarts (runtime OTA, watchdog): connections and sessions are lost, and the `boot_id` changes. Write tickets are lost; manual levels are **not**: they persist across restarts with the deadline in UTC (D4, 16 A14), and one that expired during the restart does not come back. Trips, generations, jobs, traces and the audit survive.
- A stuck group: the writes stay `queued` and respond 202 at the end of `wait_timeout`; the supervisor handles the group (document 01, §20).
- Disk full: the audit and the decision log are the last to lose writes (document 01, §20); the creation of traces is refused (`J-E1703`).

### 8.4 Security

- TLS 1.3, without 0-RTT, without renegotiation. Client certificates are verified against the client CA of the node and against the revocation list that the node configuration provides (ref/02).
- All refusals (role, origin, anti-replay, identity) stay in the audit with the full principal.
- The API does not return credentials, private keys or secret values in any response, including errors and logs.

---

## 9. `jk` as a client

### 9.1 Mapping of every command of document 07

**Local** = does not touch a node. **Local + node** = runs on the laptop and uses the API when there is `--node`.

| Command | Where | Endpoints |
|---|---|---|
| `jk init`, `jk new`, `jk app new` | local | — |
| `jk fmt`, `jk lsp`, `jk config show`, `jk config set`, `jk ca init`, `jk ca bundle`, `jk key gen` | local | — |
| `jk check` | local; with `--node`, also on the node | `POST /v1/generations?dry_run=true` |
| `jk build [--target]`, `jk build --target mcu:<board> [--generic \| --generation \| --lib \| --crate]` | local | — |
| `jk flash [--identity]`, `jk trace-dump --probe` | local (probe) | — |
| `jk dev [--node] [--panel]` | local (sim) or node | `POST /v1/blobs/…`, `POST /v1/generations` (`base: null`), `…/activate`, `GET /v1/ws` (`signals`, `logs`), `GET /v1/sim`, `PUT`/`DELETE /v1/sim/inputs/{path}` |
| `jk test [--fuzz]` | local | — |
| `jk bench [<app>]` | node | `POST /v1/blobs/…`, `POST /v1/bench`, `GET /v1/jobs/{id}` |
| `jk bench --target mcu:<board>` | local (probe or serial, ref/08 §2.12.5) | — |
| `jk diff [--against]` | local + node | `GET /v1/generations/{gen}/manifest`, `…/lock`, `…/provenance` |
| `jk explain <key\|code\|pipeline>` | local; `<key>` with `--node` uses the node | `GET /v1/generations/current/provenance?key=` |
| `jk explain <instance>` | node | `GET /v1/instances/{i}?explain=true` |
| `jk explain <output>` | node | `GET /v1/outputs/{path}` (`OutputState`: `group`, `levels`, `active_level`, `envelope`, `last_outcome`) |
| `jk inspect <app.wasm\|driver.wasm\|firmware.jkfw\|model.jkm>` | local; with `--node`, the installed app | `GET /v1/apps/{app}` (`meta`) |
| `jk lock [--update] [--check]` | local + node | `POST /v1/lock/probe`, `GET /v1/lock/facts`, `GET /v1/lock/drift`, `GET /v1/generations/current/lock`, `GET /v1/runtime/drivers` (the drivers of the binary for the lock, ref/06 §3.8) |
| `jk sim <scenario>` | local | — |
| `jk node enroll <target> --ca [--code]` | node (bootstrap channel: the Unix socket, local or over SSH) | `POST /v1/node/enrollment/csr`, `PUT /v1/node/enrollment/certificate`, `GET /v1/node` |
| `jk node enroll <target> --deck <url> --token <t>` | node | `POST /v1/node/enrollment/deck`, `GET /v1/jobs/{id}` |
| `jk node enroll <mcu> --link <device> [--probe]` | local (probe, USB) + node | `POST /v1/secrets` (the `K_link` as `secret:link.<device>`) |
| `jk node enroll mcu:<port> --deck <url> --token <t>` | local (USB) + Deck | — (the standalone MCU talks to the Deck through the micro profile, ref/10 §2.14) |
| `jk node renew` | node | `POST /v1/node/renew` |
| `jk node trust show \| push \| set` | node | `GET /v1/node/trust`, `PUT /v1/node/trust/bundle`, `PUT /v1/node/trust/anchors` |
| `jk secret set \| get \| list \| rm \| import`, `jk secret recipient add --user \| rm` | local | — |
| `jk secret recipient add --node` | node | `GET /v1/node` (the certificate chain with the KEM key in the SAN, verified by `jk` against the CA of the organization; ref/02 §3.4.2) |
| `jk secret push --node`, `jk secret status --node` | node | `POST /v1/secrets`, `GET /v1/secrets` |
| `jk agent status \| desired \| pause \| resume \| import` | node | `GET /v1/agent`, `GET /v1/agent/desired`, `POST /v1/agent/pause`, `…/resume`, uploads + `POST /v1/agent/import` |
| `jk session list \| accept \| close` (1.3) | node | `GET /v1/agent/sessions`, `POST …/{s}/accept`, `DELETE …/{s}` |
| `jk doctor [--ml] [--rt] [--allow-live]` | node | `POST /v1/doctor` (`allow_live`), `GET /v1/jobs/{id}`, `GET /v1/doctor` |
| `jk apply [--confirm] [--dry-run]` | node | `POST /v1/blobs/missing`, uploads, `POST /v1/secrets` (the secret entries that the generation refers to), `POST /v1/generations[?dry_run=true]`, `POST /v1/generations/{gen}/activate` |
| `jk confirm` | node | `POST /v1/generations/confirming/confirm` |
| `jk rollback [<generation>]` | node | `POST /v1/generations/rollback` |
| `jk generations` | node | `GET /v1/generations` |
| `jk runtime upgrade <version>` | node | uploads (`joker.runtime.v1`), `POST /v1/runtime/upgrades`, `GET /v1/jobs/{id}`, `GET /v1/runtime`, `GET /v1/info` (`boot_id`) |
| `jk runtime build` | local | — |
| `jk migrate` | local | — |
| `jk status [--instances]` | node | `GET /v1/status[?instances=true]` |
| `jk top` | node | `GET /v1/ws` (`status`, `metrics`, `outputs`) |
| `jk read <selector> [--watch]` | node | `GET /v1/values`; `--watch`: `GET /v1/ws` (`signals`) |
| `jk read <selector> --refresh` | node | `POST /v1/values/refresh` |
| `jk write <output> <value> --ttl [--level] --reason [--takeover]` | node | `POST /v1/outputs/{path}/preview` → confirmation → `POST /v1/outputs/{path}/manual` (`If-Match`; `--takeover` is the `takeover` field) |
| `jk release <output> [--level]` | node | `POST /v1/outputs/{path}/release` |
| `jk trace <signal>` | node | `GET /v1/signals/{path}/lineage`, `GET /v1/ws` (`signal_trace`) |
| `jk decisions […] [--report pdf\|csv]` | node (+ local for PDF) | `GET /v1/decisions` (`text/csv` for reports) |
| `jk alarms [--active]` | node | `GET /v1/alarms[?state=active]`, `GET /v1/alarm-events` |
| `jk alarm ack <id>` | node | `POST /v1/alarms/{id}/ack` |
| `jk alarm shelve <id> <ttl> "<reason>"` | node | `POST /v1/alarms/{id}/shelve` |
| `jk alarm unshelve <id>` | node | `POST /v1/alarms/{id}/unshelve` |
| `jk trips` | node | `GET /v1/trips` |
| `jk reset <output> --reason` | node | `POST /v1/outputs/{path}/reset` |
| `jk intents` | node | `GET /v1/intents` |
| `jk intent approve <id>` | node | `GET /v1/intents/{id}?preview=true` → `POST /v1/intents/{id}/approve` |
| `jk intent reject <id>` | node | `POST /v1/intents/{id}/reject` |
| `jk action <target>.<action> [args] --key` | node | `GET /v1/actions?target=`, `POST /v1/actions/{target}/{action}`, `GET /v1/ws` (`invocations`), `POST /v1/invocations/{id}/cancel` |
| `jk call <instance>.<method> [args] --ttl --reason` | node | `GET /v1/classes/…` (positional arguments → names), `POST …/methods/{m}/preview` → `POST …/methods/{m}` |
| `jk app start\|stop\|restart <app>` | node | `POST /v1/apps/{app}/start\|stop\|restart` |
| `jk device stop\|start <device> [--reason]` | node | `POST /v1/devices/{device}/stop\|start` |
| `jk app logs <app>`, `jk logs [--app] [--follow]` | node | `GET /v1/logs`; `--follow`: `GET /v1/ws` (`logs`) |
| `jk history <selector> --from --to [--step]` | node | `GET /v1/history` |
| `jk export … --parquet` | node + local | `GET /v1/history` (`application/x-ndjson`); the Parquet is written by `jk` |
| `jk record --for [--groups] --out` | node | `POST /v1/traces` (`record`), `GET /v1/traces/{id}`, `GET /v1/traces/{id}/file` |
| `jk replay <trace> […]` | local | — (the trace comes from one of the three trace commands) |
| `jk trace-dump --group --last --out` | node | `POST /v1/traces` (`ring`), `…/file` |
| `jk trace-from-history <range> --app --out` | node | `POST /v1/traces` (`history`), `…/file` |
| `jk link status` | node | `GET /v1/uplinks` |
| `jk model build\|inspect\|migrate` | local | — |
| `jk model bench <m> --node` | node (1.1) | `POST /v1/models/{model}/bench` (reserved) |
| `jk model lineage <m>` | local; with `--node`, 1.1 | `GET /v1/models/{model}` (reserved) |
| `jk train <job> [--now]`, `jk train status` | node (1.1) | `POST /v1/training`, `GET /v1/training` (reserved) |
| `jk shadow start <app> <candidate>` | node (1.1) | uploads, `POST /v1/shadows` |
| `jk shadow status\|diff\|promote\|abort` | node (1.1) | `GET /v1/shadows/apps/{app}`, `…/diff`, `POST …/promote`, `POST …/abort` |
| `jk shadow … <model>` | node (1.1) | `/v1/shadows/models/{model}` (reserved) |
| `jk discover <uri>` | node | `POST /v1/discovery`, `GET /v1/jobs/{id}` |
| `jk profile import <format> <file>` (including `csv`), `jk profile check`, `jk profile diff` | local | — |
| `jk board show <board>`, `jk board new <name> --from <board>`, `jk board check <file>` | local | — |
| `jk board probe --node <node> [--name] [--out]` | node (reads only) | `GET /v1/node/hardware`, `GET /v1/info` (declared board, to compare) |
| `jk board check <file> --node <node>` | local + node (reads only) | `GET /v1/node/hardware` |
| `jk driver test`, `jk driver meta` | local | — |
| `jk profile test <profile> --uri <uri> [--node]` | local (driver on the laptop) or node | `POST /v1/profile-tests` (the URI of ref/03 §2.2.20 goes as is in `uri`), `GET /v1/jobs/{id}` |
| `jk profile publish` | Hub | — (it is not the node) |
| `jk publish --manifest … --registry …` | local + OCI registry (the store of the Deck) | — (it is not the node; the Deck reads from the registry, document 20 §4.4.2) |
| `jk class list [--std\|--project]`, `jk class show` | local; with `--node`, those of the generation | `GET /v1/classes`, `GET /v1/classes/{package}/{class}` |
| `jk class new`, `jk class fmt\|check\|diff\|doc\|gen` | local | — |
| `jk class verify <class> --instance <i>` | local (sim) or node (equipment) | `POST /v1/instances/{i}/verify` |
| `jk class publish`, `jk board publish` | Hub | — |

Mentioned outside document 07: `jk ui` (1.3; local server of `jk`, client of this API), `jk class extract` (local, after v1). `jk device configure` only exists in the proposal `proposals/class-idl.md` (§11).

### 9.2 Exit codes (document 07, §8)

| Situation in the API | Exit |
|---|---|
| 2xx; write with `accepted`, `clamped_*`, `suppressed`; action `succeeded` | 0 |
| 2xx with `overridden` | 0, with a warning ("level X is still in control") |
| 400 (J-E101x, J-E103x, J-E1101, J-E1106…) | 1 |
| 422 from validation (`J-E1601`, `J0107`, `J0108`) | 2 |
| 401, 403, 409 from permissive, holding or signature; write `rejected` or `tripped`; action `failed` | 3 |
| no connection, 503, 504, 429 after 3 attempts | 4 |
| `confirm` refused because the generation is `rolled_back`; `jk apply --confirm` that waits and sees the rollback | 5 |
| `verify` or `profile_test` with failed cases | 6 |
| drift (`jk lock --check`, local, or `GET /v1/lock/drift` with entries) | 7 |

### 9.3 Obligations of `jk` as a client

- Always send `Joker-Nonce` and `Joker-Timestamp` on mutating requests, even over the socket (it costs nothing and simplifies the code).
- Retry a mutating request after a network failure with a **new nonce** and the **same** `Idempotency-Key`.
- Consult `GET /v1/version` and `features` when opening the session and refuse with a clear message an option that the node does not support, before receiving a `J-E1011`.
- Always show the `code` of problems, so that `jk explain` works (ref/00, §8).

---


## 10. Examples with `curl`

The examples use the Unix socket. From the laptop, the socket is reached over SSH, and there anti-replay is optional (§2.5):

```sh
ssh -N -L /tmp/porto.sock:/run/joker/api.sock joker-admin@revpi-porto.local &
J='curl -sS --unix-socket /tmp/porto.sock'
H='-H Content-Type:application/json'
```

Over TCP with mTLS, each mutating request carries the two headers:

```sh
T='curl -sS --cert ops.crt --key ops.key --cacert org-ca.crt'
$T -X POST https://revpi-porto.local:7443/v1/alarms/grid_overfreq/ack \
   -H "Joker-Nonce: $(ulid)" -H "Joker-Timestamp: $(date -u +%FT%TZ)" \
   -H Content-Type:application/json -d '{"reason":"seen"}'
```

(`ulid`: any ULID generator.)

### 10.1 From the laptop to the field (document 07, §9)

```sh
# jk new, jk dev, jk test: local.

# jk node enroll revpi-porto.local --ca ~/org-ca
$J -X POST http://joker/v1/node/enrollment/csr $H -d '{"node":"revpi-porto"}' | jq -r .csr_pem > porto.csr
#   (jk signs porto.csr with the local CA and obtains porto.crt)
$J -X PUT http://joker/v1/node/enrollment/certificate $H \
   -d "$(jq -n --rawfile c porto.crt --rawfile ca clients-ca.crt '{chain_pem:$c, client_ca_pem:$ca}')"

# jk doctor --rt
$J -X POST http://joker/v1/doctor $H -d '{"checks":["base","rt"],"rt_duration":"60s"}'
#   → 202, Location: /v1/jobs/01J8Z4…
$J http://joker/v1/jobs/01J8Z4Q0M6K2V7X9B1C3D5F7GH | jq '.state, .result.summary'

# jk discover sunspec://192.168.10.30
$J -X POST http://joker/v1/discovery $H -d '{"uri":"sunspec://192.168.10.30"}'

# jk lock --node: the manifest is not on the node yet
$J -X POST http://joker/v1/lock/probe $H \
   -d "$(jq -n --rawfile m /tmp/effective.toml '{manifest:{content:$m}}')"
#   → job lock_probe; result.lock_toml is the [discovered.*]/[fallback.*] excerpt

# jk bench: upload the app and measure
D=sha256:$(sha256sum target/peak_shaver.wasm | cut -d' ' -f1)
$J -X POST http://joker/v1/blobs/missing $H -d "{\"digests\":[\"$D\"]}"
$J -X POST "http://joker/v1/blobs/uploads?digest=$D&kind=joker.app.v1%2Bwasm" \
   -H Content-Type:application/octet-stream --data-binary @target/peak_shaver.wasm
$J -X POST http://joker/v1/bench $H -d "$(jq -n --rawfile m /tmp/effective.toml --arg d "$D" \
   '{manifest:{content:$m}, apps:[{digest:$d, kind:"joker.app.v1+wasm", name:"apps.peak_shaver"}]}')"

# jk check: local (and, if desired, on the node)
$J -X POST 'http://joker/v1/generations?dry_run=true' $H -d @proposal.json | jq .plan

# jk apply --confirm 10m
BASE=$($J -D - -o /dev/null http://joker/v1/generations/current | awk -F'"' '/^etag/I{print $2}')
$J -X POST http://joker/v1/generations $H -d @proposal.json      # base = $BASE; → 201, gen 18
$J -X POST http://joker/v1/generations/18/activate $H -H "If-Match: \"$BASE\"" \
   -d '{"confirm_window":"10m","reason":"first deploy"}' | jq '.state, .confirm_deadline'

# jk status
$J http://joker/v1/status | jq '.generation, .devices, .alarms.active'

# jk confirm
$J -X POST http://joker/v1/generations/confirming/confirm $H -d '{}'
```

`proposal.json`:

```json
{
  "manifest": { "digest": "sha256:1f0c…" },
  "lock": { "digest": "sha256:77ab…" },
  "provenance": { "digest": "sha256:c3d2…" },
  "artifacts": [
    { "digest": "sha256:9f2c…", "kind": "joker.app.v1+wasm", "name": "apps.peak_shaver" },
    { "digest": "sha256:0b1e…", "kind": "joker.profile.v1+toml", "name": "profiles/janitza/umg604@2.profile.toml" },
    { "digest": "sha256:5a6b…", "kind": "joker.key.v1+pem", "name": "keys/site-ops.pub" }
  ],
  "config": { "digest": "sha256:d41f…" },
  "base": "sha256:8e7d…",
  "message": "site-porto: first deploy",
  "source": { "repo": "git@git.example.com:sites/porto.git", "commit": "4c1d2e9", "dirty": false }
}
```

### 10.2 An incident

```sh
# jk decisions --output pcs.p_set --since "2026-09-24 03:00" --to "03:30"
$J 'http://joker/v1/decisions?output=pcs.p_set&from=2026-09-24T03:00:00Z&to=2026-09-24T03:30:00Z&order=asc' \
   -H Accept:text/csv > decisions.csv

# only the writes that read meter.p_total
$J 'http://joker/v1/decisions?output=pcs.p_set&cause=meter.p_total&since=1h' | jq '.items[] | {ts, outcome, applied}'

# jk trace-from-history "03:00..03:30" --app peak_shaver --out incident.jktrace
$J -X POST http://joker/v1/traces $H \
   -d '{"source":"history","from":"2026-09-24T03:00:00Z","to":"2026-09-24T03:30:00Z","app":"peak_shaver"}'
#   → 201, id 01J8Z5…, approximate: true, warnings
$J -o incident.jktrace http://joker/v1/traces/01J8Z5B2N4P6R8T0V2X4Z6A8CD/file

# or, exact, from the continuous ring of the group
$J -X POST http://joker/v1/traces $H -d '{"source":"ring","groups":["fast"],"from":"2026-09-24T03:00:00Z","to":"2026-09-24T03:30:00Z"}'

# jk replay incident.jktrace --diff --app peak_shaver@1.4.3: local
```

### 10.3 Day-to-day operation

```sh
# jk read 'pcs.**'
$J 'http://joker/v1/values?sel=pcs.**' | jq '.items[] | [.path, .value, .unit, .quality, .age]'

# jk read 'pcs.**' --watch (SSE, without WebSocket)
$J -N 'http://joker/v1/stream?topic=signals&sel=pcs.**&min_interval=500ms'

# jk write pcs.p_set "50 kW" --ttl 30m --reason "test" : preview, then write
$J -X POST http://joker/v1/outputs/pcs.p_set/preview $H \
   -d '{"value":"50 kW","ttl":"30m","reason":"test"}' | jq '.prediction, .conflict'
$J -X POST http://joker/v1/outputs/pcs.p_set/manual $H -H 'If-Match: "sha256:8e7d…"' \
   -H "Idempotency-Key: $(uuidgen)" -d '{"value":"50 kW","ttl":"30m","reason":"test"}'
#   → {"id":"01J8…","state":"committed","level":"manual:4","outcome":{"outcome":"clamped_rate","applied":12.5},…}

# jk release pcs.p_set
$J -X POST http://joker/v1/outputs/pcs.p_set/release $H -d '{"reason":"end of test"}'

# jk alarm shelve grid_overfreq 2h "maintenance"
$J -X POST http://joker/v1/alarms/grid_overfreq/shelve $H -d '{"ttl":"2h","reason":"maintenance"}'

# jk reset pcs.p_set --reason "BMS fault resolved"
$J -X POST http://joker/v1/outputs/pcs.p_set/reset $H -d '{"reason":"BMS fault resolved"}' | jq .result

# jk action pcs.reset_fault --key k1
$J -X POST http://joker/v1/actions/pcs/reset_fault $H -H 'Idempotency-Key: k1' -d '{"args":{}}'

# jk call p1.run_at 60% --ttl 10m --reason "manual start"
$J -X POST http://joker/v1/instances/p1/methods/run_at $H \
   -d '{"args":{"speed":"60 %"},"ttl":"10m","reason":"manual start"}'

# jk intent approve 01J8…
$J -X POST http://joker/v1/intents/01J8Z6C3P5R7T9V1X3Z5B7D9FG/approve $H -d '{"reason":"ok, floor 3"}'
```

### 10.4 Shadow of an app (the "a new model" flow of §9, with an app)

The flow of document 07 uses a model, which belongs to 1.1. App shadow is also 1.1 (§5.14; ADR 0035); with an app, from 1.1:

```sh
# jk shadow start peak_shaver peak-shaver-1.5.0.wasm
$J -X POST http://joker/v1/shadows $H -H 'If-Match: "sha256:8e7d…"' -d '{
  "target": "app:peak_shaver",
  "candidate": { "digest": "sha256:aa01…", "kind": "joker.app.v1+wasm", "name": "apps.peak_shaver" },
  "until": { "duration": "48h", "max_divergence": "5 kW p95" },
  "reason": "version 1.5.0 in shadow"
}'

# jk shadow status peak_shaver (48 h later)
$J http://joker/v1/shadows/apps/peak_shaver | jq '.state, .divergence, .criteria'

# jk shadow promote peak_shaver
$J -X POST http://joker/v1/shadows/apps/peak_shaver/promote $H -H 'If-Match: "sha256:31c9…"' \
   -d '{"reason":"p95 divergence of 1.2 kW"}'
```

---

## 11. Stress

**S1 — A dashboard subscribes to `**` on a node with a 1 ms group and 5,000 signals, over Wi-Fi.** The sampler reads the slots without waiting every 100 ms and sends only the new `seq`s. The Wi-Fi cannot keep up: the queue of the connection fills, the sampler stops generating messages while it does not drain, and the effective rate falls to what the network can take. After 30 s of a full queue, 4008. The core sees none of this. **Finding:** 1,000 clients on `**` at 10 Hz were 50 M reads/s in `bg`, more than the CPU. **Fix incorporated:** sampling budget of the node (1 M/s, `J-E1903`) and the limit of 10,000 signals per subscription.

**S2 — The network drops in the middle of a `jk write`.** `jk` does not know whether the write arrived. It retries with a new nonce and the same `Idempotency-Key`: the API returns the stored ticket (`Joker-Idempotent-Replay: true`), and the decision log has one entry, not two. Without the key, the retry would only renew the lease (the same value, the same level): harmless, but with two decisions. **Holds.**

**S3 — Two operators write to the same output at the same time, both at `manual:4`.** The first wins; the second receives `J-E1105` with the name, the deadline and the reason of the first. **Finding:** document 16, A5, forbids ties at a level, but only as a validation error; a manual level is held at runtime by people. **Fix incorporated:** holding per principal, explicit `takeover`, and both decisions in the log.

**S4 — The generation changes between the preview and the confirmation.** The person saw `range = ±125 kW`; in the meantime the Deck applied `±100 kW`. `jk` sends `If-Match` with the hash it saw: 412 `J-E1026`, and `jk` shows the new preview. **Holds.**

**S5 — Forgetting the TTL, or setting 30 days.** `J-E1101` with the help "manual writes always take `--ttl` (A10); the maximum for this output is 8h". `ttl: "30d"` is `J-E1102`, with the `max_manual`. The API does not round a TTL down: an operator who asked for 30 days must know they did not get them. **Holds.**

**S6 — `jk apply` and the Deck at the same time.** `jk` prepares generation 18 on top of 17; the agent receives a desired state and starts swapping to another. The transition mutex gives `J-E1608` to the second, and the `If-Match` of `jk` would fail anyway. **Holds.**

**S7 — The new generation changes the TCP listener and cuts off the API itself.** The plan marks `api` in `confirm_reasons`; in `prod`, `activate` without a window is `J-E1610`. With `--confirm 10m`, `jk confirm` no longer reaches the node, the window expires and the previous generation comes back on its own. The `jk` that was waiting sees `rolled_back` and exits with 5. **Finding:** ADR 0015 speaks of network, security and uplinks, and not of the API itself. **Fix incorporated:** `api` is a separate confirmation reason.

**S8 — A captured request is resent by a compromised relay.** Over TCP, the repeated nonce gives `J-E1022`; a new request with an old `Joker-Timestamp` gives `J-E1021`. **Finding:** with the clock `unsynced` (startup without RTC and without NTP), the window cannot be checked, and refusing everything would take control away from the operator exactly when it is most needed. **Fix incorporated:** only the nonce, memory widened to 10 min, `Joker-Clock: unsynced` and the audit marked. **Finding 2:** a full nonce memory that forgot the oldest ones would open a replay window. **Fix:** refuse (`J-E1029`), never forget.

**S9 — A browser with the client certificate installed visits a malicious page.** The page tries a `POST` to `https://node.example.com:7443/v1/outputs/…/manual`. The browser attaches the certificate; the API refuses by the `Origin` (`J-E1004`) and by the non-JSON content type of a form (`J-E1008`), and without CORS the page cannot even read the response. **Holds**, and it is the reason for not having CORS in 1.0.

**S10 — Mixed versions.** A 1.3 `jk` sends `{"at": …}` (scheduled write, 1.1) to a 1.0 node: `J-E1017`, and not a silent immediate write. A 1.0 `jk` against a 1.3 node receives new fields and ignores them. **Finding:** the rule of ref/00 ("readers tolerate the unknown"), applied to requests, would make an old node ignore a new safety option. **Fix incorporated:** the server is strict with requests (`J-E1011`), clients tolerate responses.

**S11 — A history query over 5,000 signals, 7 days raw.** It resolves to 5,000 signals (within the limit), but it is millions of points: `J-E1038` with the suggestion of `step` or of paginated NDJSON. In NDJSON, the response streams out, page by page, with the read of the historian in `bg`. **Holds.**

**S12 — Common unit and type errors.** `jk write pcs.p_set 50` writes 50 **kW** (the unit of the signal); `"50 W"` is converted to 0.05 kW; `"50 Hz"` is `J0107`; `"50 var"` is `J0108`; `true` in an `f64` and `"1"` in a `bool` are `J-E1036`; a `u64` above 2⁵³ goes as a string. The preview always shows the value already converted, in the unit of the output, before the person confirms. **Holds.**

**S13 — Resumption after a short cut in an alarm subscription.** A 20 s cut: `resume` resends the transitions from the replay buffer. A 5 min cut: `resume_unavailable`, a `resync` with the current table, and the transitions of the interval come from `/v1/alarm-events`. No transition is lost without warning. **Holds.**

---

## 12. Decisions and rejected alternatives

| Decision | Why | Rejected |
|---|---|---|
| No writes over WebSocket | anti-replay, idempotency, audit and errors in a single place | writes over WS: two paths for the same semantics |
| SSE as a mirror of one subscription | `curl` and scripts without a WS library; the engine is the same | SSE with several topics per connection (that is WebSocket again) |
| State topics sampled, events queued | a slow client receives less, never stalls anything, and events are not lost silently | one queue per client for everything (memory grows with slowness) |
| The envelope result is 200 | the request was well formed; the decision is data, not an error | 409 on `rejected` (mixes usage errors with decisions of the core) |
| Manual level held per principal, with `takeover` | an operator does not erase the override of another without knowing | "the last one wins" (an operator erases, without knowing, the override of another) |
| `manual:4` by default | it is what the track shows (part 2) and it leaves `manual:3` for escalations | `manual:5` |
| Shadow as derived generations; rollback as **reactivation** of the stored generation (D9) | a single configuration mechanism, with activation history and confirmation; the same hash and the same number everywhere | shadow as runtime state outside generations; rollback as a new generation (two numbers for the same content) |
| Chunked upload in the OCI style, content-addressed store | resumable, deduplicated, familiar | multipart with everything in one request (blows the 16 MiB) |
| The node does not generate Parquet, Arrow or PDF | binary and CPU of the node; `jk` and the client libraries convert the NDJSON (ref/05 §23) | export on the node; Arrow in `queryHistory` (request from ref/05) |
| Preview as an operation of its own (`previewManualWrite`, `previewMethodCall`) | the preview is a read (`viewer`), without anti-replay or write audit | `dry_run` in the body of the write (request from ref/05): the same operation with two roles |
| No tokens in 1.0; identity from the transport | one less credential to store and revoke | JWT for the Studio (it comes through the tunnel in 1.3) |
| CORS off and `Origin` refused | a browser with a certificate is a real vector | list of origins in 1.0 |
| Strict requests, tolerant responses | a new option is never silently ignored | tolerance in both directions |
| `viewer` by default for whoever arrives | the socket and the CA are already the trust decision | "no rule, no access" (forces listing every reader) |
| Pending intents expire at their TTL | whoever proposes says how long the proposal is valid | fixed approval deadline |
| `refresh` requires `operator` | it generates traffic on the bus | `viewer` |
| Discovery, `bench`, `doctor` `admin` only | they touch the fieldbus or the `rt` core | `operator` |

---

## 13. Version markers

| Version | What |
|---|---|
| **1.0** | everything in §2–§11, except what follows |
| 1.0, empty until 1.1 | `/v1/intents` (proposal-only writers, M18, arrive in 1.1) |
| 1.0, `skip` until 1.3 | `doctor` with `network` |
| **1.1** (document 26, §5; ADR 0035) | app shadow: `/v1/shadows`, `/v1/shadows/apps/**` and the `shadow` state of an app (§5.14); uplink budgets (`Uplink.budget`) and the `otlp` and `http` uplinks; firmware in `Plan.micros` (orchestrated MCU OTA). They respond `J-E1017` or are absent in 1.0 |
| **1.1** (document 11, §"Ready for ML") | `/v1/models/**`, `/v1/training`, `/v1/shadows/models/**`, `/v1/signals/{path}/blocks`, `/v1/streams`; topics `frames` and `blocks`; `shape` `block` and `stream`; `kind` `joker.model.v1+jkm`; `ManualWriteRequest.at` (M6); `Metrics.ml`; `Decision.model_outputs` (M22); `PlanChange.kind = model`. Everything is already in the representation and responds `J-E1017` in 1.0 |
| **1.2** (document 26, §5; ADR 0035) | `/v1/bridges` (bridges, M11); the `zenoh` uplink |
| **1.3** | `tunnel` transport and remote sessions of the Deck (`/v1/agent/sessions`); list of origins (CORS) for the Studio; session tokens, if they are needed; `doctor` `network` (gateway mode) |
| 2.0 | paths qualified by node (`@<node>.…`, document 21, §8): `/v2` prefix |
