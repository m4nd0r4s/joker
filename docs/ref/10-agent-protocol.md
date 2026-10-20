# ref/10 — Agent protocol (node ↔ Joker Deck), `agent/1`

Normative reference of the protocol between a node (`jokerd` on Linux, or a standalone Joker Micro) and a fleet control plane (the Joker Deck, or any other that complies with this document). It applies ref/00 without repeating it. The machine-readable schemas are in `spec/cddl/agent.cddl`.

Five rules that run through the whole document (document 20, §2):

1. **The node works without the Deck, forever.** Nothing here is needed for the node to control. A Deck that is down, an expired certificate or a revoked node never stop an output.
2. **The protocol has no message for writing to signals.** The Deck changes configuration (generations) and opens sessions; remote writes only exist inside a session, through the local API of the node, with the usual envelope, arbiter and TTL.
3. **The node decides.** Each document and each command is verified on the node against keys that the Deck cannot change.
4. **Complete and idempotent desired state.** The Deck says "where the node should be", never "what to do next". Receiving the same document twice does nothing.
5. **Git is the source of truth.** The protocol carries generations identified by hash and by commit; it does not carry edits.

---

## 1. Scope and sources

### 1.1 What it covers

| Surface | Section |
|---|---|
| node identity, certificates, trust bundle, security floors | 2.4, 3.1, 3.2 |
| enrollment (`jk node enroll`), renewal, revocation, cloned nodes | 2.5, 3.1, 3.2 |
| desired state (Deck → node), validation, apply by generations, MCUs behind the node | 2.6, 3.3–3.7 |
| commands (Deck → node): confirm, reject, request a full report, renew, upload logs, sessions | 2.7 |
| reported state (node → Deck) | 2.10, 3.8 |
| events (node → Deck) | 2.11, 3.9 |
| remote session (tunnel for `jk` and the Studio) | 2.12, 3.10 |
| transports: HTTPS with long-poll, MQTT, CoAP (micro profile) | 2.13–2.15, 3.11 |
| reduced profile for standalone MCUs | 2.15, 3.13 |
| `[agent]` keys of `node.toml` (hosted in ref/02) | 2.17 |
| versions and compatibility | 3.14, 6 |

It does not cover: telemetry (Sparkplug B, Zenoh, OTLP; document 20, §3.3), the Hub API, the internal format of the artifacts (ref/02, ref/08), Joker Link (ref/07) or the internal design of the Deck.

### 1.2 Sources

Document 20 (§2, §3.1, §4), document 21 (§1, §7, §8), ADR 0015, 0021 and 0022, document 01 (§12, §13, §15–§18), document 12 (§7, §11), document 19 (§3, §4.1, §6), document 09 (attacks 8, 9 and 11), document 08 (round 5), document 07 (§3), document 15 (§2.5) and ref/00.

### 1.3 Links to other references

| Reference | What it shares with this one |
|---|---|
| ref/00 | versions, time, identifiers, shared enums, encodings, errors, security |
| ref/01 | `[security]` of the manifest: `trust` (it only restricts, §2.4.3) and the `deck:` principals in `[security.roles]` (§3.10); the hash of the effective manifest (ref/01 §8.3), which enters the descriptor of the generation (§2.6.2) |
| ref/02 | `node.toml` (the trust anchors, `[policy] allow_dev` and `apply_from`, and the `[agent]` section of §2.17), the canonical digest of the lock (§2.6.2), per-recipient secrets and the `secret-bundle` (key 22 of the desired state), the KEM key of the node |
| ref/07 | `OTA` and `HELLO` messages of Joker Link, used in the `mcu` phase (§3.6) |
| ref/08 | format and signature of the `joker.generation.v1` blob and of the Joker Micro firmware |
| ref/09 | local endpoints `/v1/agent/*` and `/v1/node/*` (§2.18), the generation-transition mutex shared with the API (§3.4.2), pre-authenticated principals and audit of remote sessions (§3.10) |

### 1.4 Vocabulary

| Term | Meaning |
|---|---|
| **authority** | the Deck (`deck_id`) with which the node is enrolled. A node has at most one authority at a time |
| **DSD** | desired state document: `desired-state` signed in COSE_Sign1 |
| **bundle** | trust bundle: the public keys that the node accepts, per role, signed by the root of the organization |
| **floor** | *floor statement*: minimum version and revoked digests of an artifact type |
| **phase** | one of the three parts of an apply: `runtime`, `mcu`, `generation` (§3.4) |
| **minimal core** | the `hello`, `poll`/`desired`, `report`, `events` and `renew` messages in the `agent/1.0` form; they never change in the 1 series (§3.14) |

---

## 2. The reference

### 2.1 Encoding and dual form

| Where | Encoding |
|---|---|
| DSD, commands, bundles, floors, proofs | deterministic CBOR (RFC 8949, §4.2.1) inside COSE_Sign1 (RFC 9052). **Always**, also over HTTPS: a signed document is opaque to the transport |
| messages over HTTPS | JSON (RFC 8259), UTF-8, with text keys; CBOR with integer keys if the node announces `cbor_https` and the Deck accepts it (`Accept: application/cbor`) |
| messages over MQTT and CoAP | CBOR with integer keys; over MQTT, each message goes in COSE_Sign1 (§2.14, §3.11.2) |

**Dual form of the CDDL.** Each member is written `("name" / n)`. In CBOR only `n` and the binary forms are used; in JSON only `"name"` and the text forms. Mixing the two in one message is `bad_request` (`J0978`).

| CDDL type | CBOR | JSON |
|---|---|---|
| `time` | `int`, ns UTC since 1970 | RFC 3339 with `Z` (ref/00, §3) |
| `duration` | `uint`, ns | duration literal (`"10m"`) |
| `digest` | `bstr` of 32 B | `"sha256:<64 hex>"` |
| `ulid` | `bstr` of 16 B | 26 Crockford base32 characters |
| `kid` | `bstr` of 16 B: the first 16 bytes of the SHA-256 of the raw public key | 32 hex |
| `enum` | numeric value (§2.2) | `snake_case` name |
| `bytes`, `cose` | `bstr` | base64 (RFC 4648, §4, with `=`) |
| `seq` | `uint` < 2⁵³ | number |

**Integer keys.** 1–99: v1. 100–119: reserved for v2 (sites, §6). Negative ones in CBOR, and text ones starting with `x_` in JSON: private extensions, always ignored. A removed key is never reused (ref/00, §1).

**Tolerant reading.** A reader ignores unknown members and maps an unknown enum value to the category indicated in §2.2 (or `unknown`). The exception is the DSD: a key listed in `crit` (key 20) that the node does not know makes it refuse the document (`J0946`). It is the mechanism that prevents an old node from applying half of a new desired state.

**COSE.** Protected header: `alg` (1) = −8 (EdDSA) or −7 (ES256); `kid` (4); `content type` (3) with one of the values:

| Document | `content type` | Signed by (role) |
|---|---|---|
| `desired-state` | `application/joker-desired+cbor` | `deck` |
| `command` | `application/joker-command+cbor` | `deck` |
| `trust-bundle` | `application/joker-trust+cbor` | `root` |
| `floor-statement` | `application/joker-floor+cbor` | the role of the artifact type |
| `renew-proof` | `application/joker-renew+cbor` | identity key of the node (the old one) |
| messages over MQTT (node → Deck) | `application/joker-agent+cbor` | identity key of the node |
| messages over MQTT (Deck → node, neither DSD nor command) | `application/joker-agent+cbor` | `deck` |

No `external_aad`. There are no countersignatures or multiple signatures in v1.

### 2.2 Enumerations

The shared ones come from ref/00, §6: `clock_quality`, `alarm_state`, `severity`, `role`, `generation_state`, and the write results and reasons of ref/00, §4.4. Those specific to the protocol:

| Enum | Values | Unknown reads as |
|---|---|---|
| `agent-status` | `ok` 0, `retry` 1, `need_full` 2, `bad_request` 3, `unauthorized` 4, `forbidden` 5, `conflict` 6, `unsupported` 7, `revoked` 8, `too_large` 9, `rate_limited` 10, `quarantined` 11, `not_found` 12 | `retry` |
| `trust-role` | `root` 0 (anchor: signs bundles; the `trust.anchors` of `node.toml`, ref/02 §2.5.4), `deck` 1, `runtime` 2, `apps` 3, `config` 4, `profiles` 5, `boards` 6, `classes` 7, `firmware` 8, `models` 9 (reserved 1.1), `drivers` 10, `secrets` 11. The names are the roles of ref/02 §2.5.3 and of ref/00 §7.1 | ignored (the key is good for nothing) |
| `key_storage` | `tpm` 0, `secure_element` 1, `file` 2 | `file` |
| `node-kind` | `linux` 0, `micro` 1 | — (refuses) |
| `mode` | `dev` 0, `prod` 1 | `prod` |
| `capability` | `mqtt` 0, `long_poll` 1, `sessions` 2, `cbor_https` 3, `import` 4, `mcus` 5, `models` 6 (1.1), `attestation` 7 | ignored |
| `source-kind` | `oci` 0, `deck` 1, `mirror` 2 | source ignored |
| `source-auth` | `mtls` 0, `token` 1, `none` 2 | source ignored |
| `confirm-mode` | `auto` 0, `deck` 1, `manual` 2 | refuses (`J0979`) |
| `phase` | `runtime` 0, `mcu` 1, `generation` 2 | refuses |
| `apply-state` | `accepted` 0, `validating` 1, `fetching` 2, `staging` 3, `waiting` 4, `applying` 5, `confirming` 6, `confirmed` 7, `rolled_back` 8, `failed` 9, `refused` 10, `superseded` 11, `noop` 12 | — |
| `wait-reason` | `not_before` 0, `window` 1, `jitter` 2, `clock` 3, `paused` 4, `local_hold` 5, `confirmation` 6, `rate_limit` 7, `mcu_link` 8 | — |
| `command-kind` | `confirm` 0, `reject` 1, `report_full` 2, `renew` 3, `upload` 4, `session_open` 5 (1.3), `session_close` 6 (1.3) | `unsupported` |
| `upload-kind` | `decisions` 0, `logs` 1, `trace` 2, `events` 3 | `unsupported` |
| `health` | `ok` 0, `degraded` 1, `failed` 2, `unknown` 3 | `unknown` |
| `group-state` | `ok` 0, `degraded` 1, `overrun` 2, `tripped` 3, `stopped` 4 | `degraded` |
| `link-state` | `connecting` 0, `up` 1, `degraded` 2, `down` 3, `drift` 4, `stopped` 5 (the `link_state` of ref/00 §6) | `degraded` |
| `instance-health` | `ok` 0, `degraded` 1, `failed` 2 (document 14, A8) | `degraded` |
| `app-state` | `starting` 0, `running` 1, `crashloop` 2, `stopped` 3, `failed` 4, `shadow` 5, `updating` 6 (the `app_state` of ref/00 §6) | `failed` |
| `gen-origin` | `agent` 0, `local` 1, `import` 2, `session` 3, `shadow` 4, `rollback` 5, `boot` 6 (the origin of each **activation**, the same as the API's, ref/09 §5.13) | `local` |
| `transport` | `https` 0, `mqtt` 1, `coap` 2 | — |
| `slot` | `a` 0, `b` 1 | — |
| `event-type` | `alarm` 0, `decision` 1, `apply` 2, `trip` 3, `security` 4, `session` 5, `gap` 6, `clock` 7, `command_result` 8, `cert` 9, `agent` 10 | stored as is |
| `trip-action` | `tripped` 0, `reset` 1 | — |
| `security-kind` | `desired_refused` 0, `command_refused` 1, `trust_refused` 2, `floor_refused` 3, `artifact_refused` 4, `clone_suspected` 5, `session_refused` 6, `tls_pin` 7 | stored as is |
| `session-action` | `requested` 0, `opened` 1, `closed` 2, `refused` 3, `expired` 4 | — |
| `cert-action` | `renewed` 0, `expiring` 1, `expired` 2, `revoked` 3 | — |
| `agent-action` | `paused` 0, `resumed` 1, `hold` 2, `imported` 3, `offline` 4, `online` 5 | — |
| `tunnel-kind` | `open` 0, `data` 1, `close` 2, `reset` 3, `credit` 4, `ping` 5 | closes the tunnel |

### 2.3 Responses and errors

An error response is a `status-reply`:

| # | Name | Type | Req. | Meaning |
|---|---|---|---|---|
| 1 | `status` | `agent-status` | yes | category (decides what the client does) |
| 2 | `code` | J-code | no | the cause, from the registry (§2.20, ref/00, §8) |
| 3 | `detail` | tstr ≤ 1 KiB | no | text for people, in English |
| 4 | `retry_after` | duration | no | minimum wait before retrying |

Over HTTPS, the `status` accompanies the HTTP code:

| `agent-status` | HTTP | The client |
|---|---|---|
| `ok` | 200 | — |
| `retry` | 503 | retries with backoff (§3.11.4) |
| `need_full` | 409 | sends a full report |
| `bad_request` | 400 | does not retry the same message; logs `J0978` locally |
| `unauthorized` | 401 | renews the certificate (§3.2) |
| `forbidden` | 403 | does not retry; local alarm |
| `conflict` | 409 | rereads the state (hello) |
| `unsupported` | 501 | gives up on the feature |
| `revoked` | 410 | stops the agent; alarm `joker.agent.revoked` |
| `too_large` | 413 | splits or truncates (§3.8) |
| `rate_limited` | 429 | waits `retry_after` |
| `quarantined` | 403 | keeps reporting, does not expect desired state (§3.2.3) |
| `not_found` | 404 | — |

### 2.4 Identity and trust

#### 2.4.1 Keys

| Key | Algorithm | Where it lives | What for |
|---|---|---|---|
| **node identity** | ECDSA P-256 (ES256) | TPM 2.0 or secure element when there is one; otherwise a `0600` file in `/var/lib/joker/identity/` (warning `J0933`); on an MCU, secure element or protected flash | mTLS with the Deck and the broker; signature of the messages over MQTT; renewal proof |
| **organization root** (`root`) | Ed25519 | offline (HSM, card, ceremony) | signs trust bundles (the anchors, ref/02 §2.5.4) |
| **Deck** (`deck`) | Ed25519 | in the Deck, preferably in KMS/HSM | signs DSD, commands and Deck → node messages over MQTT |
| **artifacts** (`runtime`, `apps`, `config`, `profiles`, `boards`, `classes`, `drivers`, `firmware`, `secrets`, `models`) | Ed25519 | **outside the Deck**: release pipeline, KMS, offline signing | sign OCI artifacts (cosign/notation), floors and, `secrets`, the secret entries (ref/02 §2.4) |

D7 and ADR 0025: the identity of nodes is **P-256**, because that is what TPMs, secure elements (ATECC608 and the like) and MCU TLS stacks (mbedTLS) support; the organization keys are **Ed25519**. In COSE, `EdDSA` (−8) is the algorithm of the organization keys and `ES256` (−7) that of the signatures made by the node itself (MQTT messages, renewal proof). A `trust-key` with `alg` ≠ −8 is refused (`J0939`).

#### 2.4.2 Node certificate

| Field | Value |
|---|---|
| version | X.509 v3 |
| subject | `CN=<node>, O=<org>` |
| SAN | `URI:urn:joker:<org>:node:<node>` (Linux) or `URI:urn:joker:<org>:micro:<node>` (standalone MCU); `DNS:<node>`; and the X25519 KEM key for secrets in `URI:urn:joker:kem:x25519:<base64url>` (ref/02 §3.2.3, §3.4.2) |
| EKU | `clientAuth` |
| key | P-256 |
| validity | default 365 d; between 30 d and 5 years (policy of the Deck) |
| issuer | issuing CA of the organization (it may live in the Deck; its compromise does not give access to the nodes, §4, S1) |

#### 2.4.3 Trust bundle (`trust-bundle`)

Signed by a `root` key. It is what the node uses to verify everything else.

| # | Name | Type | Req. | Default | Validation | Error |
|---|---|---|---|---|---|---|
| 1 | `v` | `1` | yes | — | = 1 | `J0939` |
| 2 | `org` | name | yes | — | = org of the node | `J0939` |
| 3 | `version` | seq | yes | — | > installed version | `J0939` |
| 4 | `issued` | time | yes | — | — | — |
| 5 | `authority` | name | no | no Deck | a `deck` key with that `deck` | `J0939` |
| 6 | `keys` | `[+ trust-key]` | yes | — | ≤ 32; at least one `root` | `J0939` |
| 7 | `deck_urls` | `[+ tstr]` | no | — | `https://`, ≤ 4 | `J0939` |
| 8 | `deck_tls` | `[+ digest]` | no | WebPKI | SHA-256 of the SPKI accepted for the Deck and the broker | — |
| 9 | `mqtt` | `[+ tstr]` | no | — | `mqtts://`, ≤ 4 | `J0939` |
| 10 | `coap` | `[+ tstr]` | no | — | `coaps://`, ≤ 4 | `J0939` |
| 11 | `scope` | `[+ tstr]` | no | `["*"]` | node names or `site:<s>`; the node must be included | `J0939` |
| 12 | `min_mode` | `mode` | no | `dev` | the node refuses generations with a `mode` below this; it combines with `[policy] allow_dev` of `node.toml` (ref/02 §3.5.1): a `dev` generation needs both | `J0956`, `J0906` |

`trust-key`:

| # | Name | Type | Req. | Validation |
|---|---|---|---|---|
| 1 | `kid` | kid | yes | = SHA-256(`key`)[0..16]; unique in the bundle |
| 2 | `alg` | int | yes | −8 (Ed25519, D7) |
| 3 | `key` | bytes | yes | 32 B |
| 4 | `roles` | `[+ trust-role]` | yes | `root` is not combined with other roles |
| 5 | `not_after` | time | no | checked only with the clock `synced` |
| 6 | `deck` | name | if `deck` ∈ `roles` | the `deck_id` that the key represents |

**Update rule.** A new bundle is only accepted if it is signed by a `root` key **of the installed bundle** and has a higher `version`. A root rotation is a bundle signed by the old root that already includes the new one. A node that was away during several rotations receives the **chain** of bundles and verifies them one by one (`renew-reply.trust`, `hello-reply.trust`, `GET /trust`).

**Manifest.** The `[security] trust` of the manifest (document 03, §13) **only restricts**: each key referred to there must be in the bundle with the corresponding role. A key that is not is `J0292`, from manifest validation (ref/01), in `jk check` and in V20 of staging. Thus, whoever signs configuration cannot widen who signs runtime or apps.

#### 2.4.4 Security floors (`floor-statement`)

Signed by a key with the role of the artifact type (the floor of the runtime by the `runtime` key, and so on).

| # | Name | Type | Req. | Validation |
|---|---|---|---|---|
| 1 | `v` | `1` | yes | = 1 |
| 2 | `role` | `trust-role` | yes | ≠ `root`, ≠ `deck` |
| 3 | `seq` | seq | yes | > last floor accepted for (`role`, `scope`) |
| 4 | `issued` | time | yes | — |
| 5 | `min_version` | semver | no | minimum admissible version |
| 6 | `revoked` | `[* digest]` | no | ≤ 256 forbidden digests |
| 7 | `scope` | tstr | no, `"*"` | board, architecture (`arch:aarch64`) or `*` |
| 8 | `min_counter` | uint | no | minimum security counter (MCU firmware) |

The node keeps the floor with the highest `seq` per (`role`, `scope`), with fsync. An invalid floor makes the whole DSD be refused (`J0939`).

### 2.5 Enrollment and renewal

#### 2.5.1 Enrollment token

```
jkt1_<org>_<root-kid, 32 hex>_<secret, 32 base32 characters>
```

The `_` separates without ambiguity, because it does not exist in names (ref/00, §5). The token is single-use, expires (default 24 h, maximum 7 d) and may be bound to a node name or to a site. The `kid` of the root lets the node verify the bundle it receives without trusting the channel: it is the only *trust on first use* of the protocol, and it is explicit.

#### 2.5.2 `enroll-request` (node → Deck, `POST /enroll`, without mTLS)

| # | Name | Type | Req. | Validation | Error |
|---|---|---|---|---|---|
| 1 | `token` | tstr | yes | valid, unused, not expired | `J0930` |
| 2 | `node` | name | yes | grammar of ref/00, §5; free in the org or equal to that of the token | `J0932` |
| 3 | `csr` | bytes | yes | PKCS#10 DER, P-256, signed by its own key; `CN` = `node`; with the X25519 KEM key in the SAN URI `urn:joker:kem:x25519:<base64url>` (ref/02 §3.2.3) | `J0932` |
| 4 | `hw` | digest | yes | hardware fingerprint (§3.2.3) | — |
| 5 | `key_storage` | enum | yes | the Deck may require `tpm`/`secure_element` | `J0933` |
| 6 | `kind` | `node-kind` | yes | — | — |
| 7 | `board` | tstr | no | — | — |
| 8 | `runtime` | semver | no | — | — |
| 9 | `attestation` | `attestation` | no | reserved; the Deck may require it from 1.3 on | `J0933` |

`attestation` (TPM 2.0): `kind` (1, `tpm2` 0), `ek_cert` (2, certificate of the *endorsement key*, DER), `ak` (3, public key of the *attestation key*), `quote` (4, `TPM2_Quote` over PCRs 0–7 and the `nonce`), `nonce` (5, obtained from `GET /enroll/nonce`, valid for 5 min). The verification belongs to the Deck; the format is fixed now so that it does not change in 1.3.

#### 2.5.3 `enroll-reply`

| # | Name | Type | Req. | Meaning |
|---|---|---|---|---|
| 1 | `cert` | `[+ bytes]` | yes | DER chain, leaf first, up to the issuing CA |
| 2 | `trust` | cose | yes | signed bundle; the node verifies that the root has the `kid` of the token (`J0931`) |
| 3 | `deck` | name | no | `deck_id` of the authority (= `trust.authority`) |
| 4 | `settings` | `agent-settings` | no | initial values (§2.9) |
| 5 | `anchors` | `[+ trust-key]` | yes | the `root` keys of the organization: the node writes them to `trust.anchors` of `node.toml` (ref/02 §3.2.3, step 4); one of them has the `kid` of the token, and the bundle of key 2 must be signed by one of them |

#### 2.5.4 Renewal (`POST /renew`, mTLS)

`renew-request`: `csr` (1, a **new** identity key at each renewal, with the **same** KEM key in the SAN, which only changes at a new enrollment, ref/02 §3.2.3), `proof` (2, COSE_Sign1 of the old key over `renew-proof`), `hw` (3). `renew-proof`: `node` (1), `csr` (2, SHA-256 of the CSR), `ts` (3), `nonce` (4, ULID). `renew-reply`: `cert` (1) and, if the node is behind, the chain of bundles (2).

The Deck accepts mTLS with the old certificate **even if expired**, up to `renew_grace` after its end (policy of the Deck, default 730 d), provided it is not revoked and the proof checks out (`J0936` if not).

### 2.6 Desired state (`desired-state`)

| # | Name | Type | Req. | Default | Validation | Error |
|---|---|---|---|---|---|---|
| 1 | `v` | `1` | yes | — | = 1 | `J0946` |
| 2 | `node` | name | yes | — | = name of the node | `J0944` |
| 3 | `org` | name | yes | — | = org of the node | `J0944` |
| 4 | `deck` | name | yes | — | = `authority` of the bundle and = `deck` of the key | `J0941` |
| 5 | `seq` | seq | yes | — | > last accepted (§3.3) | `J0942`, `J0943` |
| 6 | `issued` | time | yes | — | ≤ now + 5 min (only with the clock `synced`) | `J0945` |
| 7 | `expires` | time | no | no expiration | > now (only with the clock `synced`) | `J0945` |
| 8 | `runtime` | `runtime-target` | no | keep the current one | §2.6.1 | `J0947`, `J0948` |
| 9 | `generation` | `generation-target` | no | keep the current one | §2.6.2 | `J0954`, `J0955` |
| 10 | `artifacts` | `[* artifact]` | yes | — | ≤ 256; all the digests referred to, and only those (§3.3, V12 and V18) | `J0949` |
| 11 | `sources` | `[+ source]` | no | `[{url = <deck>/agent/v1/blobs, kind = deck}]` | ≤ 8 | `J0946` |
| 12 | `confirm` | `confirm-policy` | no | §2.6.4 | §2.6.4 | `J0979` |
| 13 | `rollout` | `rollout-info` | no | — | informative; comes back in the events | — |
| 14 | `schedule` | `schedule` | no | apply now | §2.6.5 | `J0979` |
| 15 | `mcus` | `[* mcu-target]` | no | keep the current ones | ≤ 32; `device` exists in the target generation with the `joker-link` driver | `J0949` |
| 16 | `trust` | `[+ cose]` | no | — | new bundles, in version order | `J0939` |
| 17 | `floors` | `[+ cose]` | no | — | new floors | `J0939` |
| 18 | `settings` | `agent-settings` | no | — | bounded by the margins of the node (§2.9) | — |
| 19 | `override_local` | bool | no | `false` | if `true`, replaces a locally changed generation (§3.7) | — |
| 20 | `crit` | `[+ uint]` | no | — | keys that the node must know | `J0946` |
| 21 | `note` | tstr ≤ 1 KiB | no | — | text for people (appears in `jk agent desired`) | — |
| 22 | `secrets` | bytes | no | — | the `secret-bundle` of this node (media type `joker.secrets.v1`, `spec/cddl/secrets.cddl`, ref/02 §2.4): the entries encrypted to the KEM key of the node, each signed by the `secrets` role. The Deck delivers them without being able to read them (D6, ADR 0024). Verified entry by entry in V10b (§3.3) | `J0922`–`J0926` (per entry) |
| 23 | — | — | — | — | reserved: models (1.1) | — |
| 100–119 | — | — | — | — | reserved: v2 (sites) | — |

An absent `runtime`, `generation` or `mcus` means "no opinion": the node keeps what it has. When present, it is the complete target for that part.

#### 2.6.1 `runtime-target`

| # | Name | Type | Req. | Default | Validation | Error |
|---|---|---|---|---|---|---|
| 1 | `version` | semver | yes | — | = `org.joker.version` of the artifact | `J0948` |
| 2 | `artifact` | digest | yes | — | in `artifacts`, type `joker.runtime.v1` | `J0949` |
| 3 | `allow_downgrade` | bool | no | `false` | required if `version` < current | `J0947` |
| 4 | `health_window` | duration | no | `5m` | 1 min–1 h | `J0979` |

The OCI manifest of the runtime carries the annotations `org.joker.version`, `org.joker.arch`, `org.joker.boards` (list, or `*`), `org.joker.agent` (minor of the protocol), `org.joker.state_min` (minimum version of the `/var/lib/joker` format that it reads) and `org.joker.worlds` (accepted WIT worlds). The node reads them before downloading the binary.

#### 2.6.2 `generation-target`

| # | Name | Type | Req. | Validation | Error |
|---|---|---|---|---|---|
| 1 | `hash` | digest | yes | = hash computed in staging (§3.4.3) | `J0955` |
| 2 | `config` | digest | yes | in `artifacts`, type `joker.config.v1` | `J0949` |
| 3 | `label` | tstr ≤ 128 B | no | — | — |
| 4 | `source` | `git-ref` | no | `repo` (1), `commit` (2), `path` (3): where it came from, for audit | — |

**Descriptor and hash of the generation.** This section owns the definition (document 22, §5). A generation of a node is described by a **descriptor** in deterministic CBOR (`generation-descriptor` in the CDDL):

| # | Name | Type | Req. | Content |
|---|---|---|---|---|
| 1 | `v` | `1` | yes | version of the descriptor |
| 2 | `kind` | `node-kind` | yes | `linux` or `micro` |
| 3 | `manifest` | CBOR map | yes | the effective manifest **with the defaults materialized and without the `x_*` keys** (ref/01 §8.3), in deterministic CBOR with the TOML keys as `tstr` |
| 4 | `lock` | digest | yes | the **canonical digest** of `joker.lock` (ref/02 §2.1: SHA-256 of the canonical serialization, and not of the bytes of the file) |
| 5 | `contracts` | `[* digest]` | yes | the hashes of the contracts of the classes used (document 17, §6), in byte order |
| 6 | `artifacts` | `[* gen-artifact]` | yes | each file that the generation refers to and that changes behavior (apps, drivers, profiles, boards, simulation models, trust keys, EDS, …; ref/01 §8.3, point 4): `name` (1, as the manifest refers to it: `apps.peak_shaver`, `profiles/local/xy-md02@1.profile.toml`), `media_type` (2, short form of ref/00 §7.1), `digest` (3); in order of `name` |
| 7 | `mode` | `mode` | yes | the `mode` of the manifest (for V19 without opening the manifest) |

```
gen_hash = SHA-256( det-CBOR( generation-descriptor ) )
```

- **Provenance** (file and line of each key, ref/09 §5.13) does not enter: it does not change behavior.
- Reordering keys, changing comments or splitting a file into overlays does not change the effective manifest, and therefore does not change the hash; changing the content of a local profile or of a trust key changes it (point 6).
- The **local API** (ref/09 §5.13) and **Joker Micro** (the `source_hash` of the blob, ref/08 §2.13.2, with `kind = micro`) use this same computation. `jk`, `jokerd`, the Deck and the MCU all show the same hash.

**`config` and `hash`.** The `joker.config.v1+cbor` artifact (ref/00 §7.1) is a COSE_Sign1 signed by the `config` role whose payload is the descriptor. `generation-target.config` is the **digest of that artifact** (with the signature, to download and verify it); `generation-target.hash` is the **hash of the descriptor** (without the signature), which identifies the generation everywhere. In staging (V21), the node rebuilds the descriptor from the manifest, the lock and the downloaded artifacts, and compares its hash with `hash` and with the payload of the `config` artifact (`J0955`). The effective manifest goes inside the descriptor; the lock travels as an artifact of the DSD (`joker.lock.v1+toml`), and the node verifies its canonical digest against key 4; the other files, by the digests of key 6.

#### 2.6.3 `artifact` and `source`

`artifact`:

| # | Name | Type | Req. | Validation | Error |
|---|---|---|---|---|---|
| 1 | `digest` | digest | yes | unique in the list | `J0949` |
| 2 | `media_type` | tstr | yes | from the table of §2.16 | `J0957` |
| 3 | `size` | uint | yes | bytes; the node refuses the download if it exceeds it | `J0950` |
| 4 | `ref` | tstr | no | readable OCI reference, for people | — |
| 5 | `signatures` | `[+ digest]` | no | signature manifests, for registries without the *referrers* API | — |

`source`:

| # | Name | Type | Req. | Default | Meaning |
|---|---|---|---|---|---|
| 1 | `url` | tstr | yes | — | base of an OCI registry (`https://registry.example.com`) or of the proxy of the Deck |
| 2 | `kind` | `source-kind` | no | `oci` | `deck`: `GET /agent/v1/blobs/{digest}`; `mirror`: local registry of the site |
| 3 | `auth` | `source-auth` | no | `mtls` for `deck`, `token` for `oci` | `token`: the node requests a `registry-token` from the Deck (§2.13) |

The mirrors of the node configuration (`[agent] mirrors`) are tried **before** the sources of the DSD. The origin does not matter: the content is verified by digest and by signature.

#### 2.6.4 `confirm-policy`

| # | Name | Type | Default | Validation | Meaning |
|---|---|---|---|---|---|
| 1 | `mode` | `confirm-mode` | `auto` | — | `auto`: the node confirms itself by health (§3.5); `deck`: waits for a `confirm` command; `manual`: waits for a local `jk confirm` (or one through a session) |
| 2 | `window` | duration | `10m` | 1 min–24 h; ≥ `stable_for` + 1 min | confirmation window of ADR 0015 |
| 3 | `require_uplink` | bool | `true` | — | the confirmation requires an authenticated exchange with the Deck after the generation swap |
| 4 | `stable_for` | duration | `60s` | 10 s–1 h | continuous time of health required in `auto` |
| 5 | `alarm_severity` | `severity` | `critical` | — | a new alarm of this severity or higher fails the health |

An invalid policy is `J0979`. The `confirm` of the DSD applies to the `runtime` and `generation` phases; each `mcu-target` may have its own.

`rollout-info` is only information for the Deck, returned in the `apply` events and in `desired.rollout`: `id` (1, ULID of the rollout, required), `wave` (2, number of the wave), `cohort` (3, name of the cohort, ≤ 64 B). The node decides nothing with it.

#### 2.6.5 `schedule` and `window`

| # | Name | Type | Default | Validation | Meaning |
|---|---|---|---|---|---|
| 1 | `not_before` | time | — | — | do not swap before this instant (the download may start now) |
| 2 | `windows` | `[+ window]` | always | ≤ 16 | only swap within one of these windows, intersected with those of the node (§3.4.4) |
| 3 | `jitter` | duration | `0s` | ≤ 24 h | uniform random delay, drawn once per DSD and stored |
| 4 | `fetch_deadline` | duration | `72h` | 1 h–30 d | deadline to download everything, counted from acceptance |

`window`: `days` (1, `[+ 1..7]`, ISO 8601, 1 = Monday), `start` (2, `"HH:MM"`), `length` (3, duration ≤ 24 h), `tz` (4, IANA, default `"UTC"`). A window that crosses midnight belongs to the day on which it starts. On clock changes, the local wall-clock time counts; a time that does not exist starts at the next one.

#### 2.6.6 `mcu-target` (MCUs behind the node)

| # | Name | Type | Req. | Validation | Error |
|---|---|---|---|---|---|
| 1 | `device` | path | yes | device with the `joker-link` driver in the target generation | `J0949` |
| 2 | `firmware` | `mcu-firmware` | no | `version` (1), `artifact` (2, type `joker.micro-firmware.v1`), `allow_downgrade` (3, default `false`) | `J0963` |
| 3 | `generation` | `mcu-generation` | no | `hash` (1), `artifact` (2, type `joker.generation.v1`) | `J0964` |
| 4 | `confirm` | `confirm-policy` | no | default: `auto`, `window = "5m"`, `stable_for = "30s"`, `require_uplink = false` | `J0979` |
| 5 | `order` | uint | no, 0 | update order; ties by `device` | — |

#### 2.6.7 Example (JSON view, as `jk agent desired --json` shows it)

```json
{
  "v": 1,
  "node": "hb-01234",
  "org": "example",
  "deck": "deck-prod",
  "seq": 418,
  "issued": "2026-09-24T09:00:00Z",
  "expires": "2026-10-01T09:00:00Z",
  "generation": {
    "hash": "sha256:4c1d0e8f6a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5",
    "config": "sha256:9a0b1c2d3e4f5061728394a5b6c7d8e9f0a1b2c3d4e5f60718293a4b5c6d7e8f",
    "label": "hp-v3 epex tariff",
    "source": { "repo": "https://git.example.com/fleet/sites.git", "commit": "e3b1f07", "path": "sites/joker.hb-01234.toml" }
  },
  "artifacts": [
    { "digest": "sha256:9a0b1c2d3e4f5061728394a5b6c7d8e9f0a1b2c3d4e5f60718293a4b5c6d7e8f", "media_type": "joker.config.v1+cbor", "size": 18244 },
    { "digest": "sha256:1f2e3d4c5b6a79880716253443526170f1e2d3c4b5a69788f1e2d3c4b5a69788", "media_type": "joker.app.v1+wasm", "size": 2811904, "ref": "registry.example.com/apps/hp-optimizer:3.2.0" }
  ],
  "sources": [ { "url": "https://registry.example.com", "kind": "oci", "auth": "token" } ],
  "confirm": { "mode": "auto", "window": "15m", "stable_for": "5m" },
  "rollout": { "id": "01J8ZK3M4N5P6Q7R8S9T0V1W2X", "wave": 2, "cohort": "cm4-din" },
  "schedule": { "windows": [ { "days": [1, 2, 3, 4, 5], "start": "02:00", "length": "3h", "tz": "Europe/Berlin" } ], "jitter": "45m" }
}
```

### 2.7 Commands (`command`, Deck → node)

A command is a one-off request, signed by the `deck` key, **idempotent and bound to its target**. There are no commands for writing to signals, for restarting the node, or for changing trust.

| # | Name | Type | Req. | Validation | Error |
|---|---|---|---|---|---|
| 1 | `id` | ULID | yes | a repeated `id` returns the previous result (without error) | — |
| 2 | `node` | name | yes | = node | `J0944` |
| 3 | `deck` | name | yes | = authority | `J0941` |
| 4 | `ts` | time | yes | with the clock `synced`: \|ts − now\| ≤ 30 s (ref/00, §9) | `J0968` |
| 5 | `kind` | `command-kind` | yes | — | `unsupported` |
| 6 | `args` | per `kind` | depending | — | `J0967` |
| 7 | `expires` | time | no | with the clock `synced`: > now | `J0968` |

| `kind` | `args` | Does | Equivalent role | Version |
|---|---|---|---|---|
| `confirm` | `confirm-args`: `seq` (1), `phase` (2), `hash` (3), `device` (4, `mcu` phase) | confirms the pending phase, if it is exactly this one (seq, phase, hash) | `admin` | 1.0 |
| `reject` | `confirm-args` | immediate rollback of the phase being confirmed (`J0969`) | `admin` | 1.0 |
| `report_full` | — | sends a full report at the next send | `viewer` | 1.0 |
| `renew` | `renew-args`: `reason` (1) | renews the certificate now (§3.2) | — | 1.0 |
| `upload` | `upload-args` (§2.7.1) | uploads local records to the Deck | `viewer` | 1.0 |
| `session_open` | `session-open-args` (§2.12) | opens a remote session | according to the policy | 1.3 |
| `session_close` | `session-close-args`: `id` (1), `reason` (2) | closes a session | — | 1.3 |

A `confirm` or `reject` whose target is not in `confirming` responds `conflict` with `J0967` and does nothing. That is why a repeated, delayed or reordered command never breaks anything.

#### 2.7.1 `upload-args`

| # | Name | Type | Req. | Default | Validation |
|---|---|---|---|---|---|
| 1 | `upload` | ULID | yes | — | destination `PUT /agent/v1/uploads/{upload}` |
| 2 | `what` | `upload-kind` | yes | — | in `[agent.upload] allow` of the node, otherwise `forbidden` |
| 3 | `from` | time | no | start of the record | — |
| 4 | `to` | time | no | now | — |
| 5 | `filter` | tstr | no | everything | path selector (document 02, §9) |
| 6 | `max_bytes` | uint | no | `[agent.upload] max_bytes` | ≤ that value |

Format of the body: `decisions` and `events` in NDJSON (one decision or event per line, in the JSON form of this document), `logs` in NDJSON of the structured logs, `trace` in the `.jktrace` format (document 16, §7). The upload goes in the `bulk` class of the uplink budget (document 01, §13).

### 2.8 Hello and long-poll

`hello` (node → Deck), at the start of each connection:

| # | Name | Type | Req. | Meaning |
|---|---|---|---|---|
| 1 | `v` | `1` | yes | — |
| 2 | `node` | name | yes | — |
| 3 | `org` | name | yes | — |
| 4 | `proto` | `[major, minor]` | yes | `[1, N]`: the highest minor that the node speaks |
| 5 | `runtime` | semver | yes | running version |
| 6 | `boot` | ULID | yes | generated at each startup of `jokerd` |
| 7 | `uptime` | duration | yes | — |
| 8 | `hw` | digest | yes | hardware fingerprint |
| 9 | `key_storage` | enum | yes | — |
| 10 | `time` | time | yes | clock of the node |
| 11 | `clock` | `clock_quality` | yes | — |
| 12 | `desired_seq` | seq | yes | last DSD accepted (0 = none) |
| 13 | `report_seq` | seq | yes | last report sent |
| 14 | `event_seq` | seq | yes | last event emitted |
| 15 | `trust_version` | seq | yes | version of the installed bundle |
| 16 | `capabilities` | `[* capability]` | no | — |
| 17 | `kind` | `node-kind` | no, `linux` | — |

`hello-reply`: `proto_minor` (1, = min(node, Deck)), `deck` (2), `time` (3, clock of the Deck), `event_ack` (4, last event stored), `need_full` (5), `settings` (6), `trust` (7, chain of newer bundles), `status` (8, `quarantined` or `revoked`), `code` (9).

**`desired_seq` in the hello** lets the Deck never issue a `seq` that the node would reject: the next `seq` is always > max(that of the Deck, that of the node). It is what saves a Deck restored from an old backup (§4, S10).

`poll-reply` (response of the long-poll, §3.11.1): `time` (1), `desired` (2, the current DSD if `seq` > the requested one), `commands` (3, ≤ 16), `cursor` (4, opaque, for the next request), `event_ack` (5), `need_full` (6), `backoff` (7, wait imposed by the Deck).

### 2.9 `agent-settings` and node margins

The Deck suggests; the node applies the value bounded by the margins below (and by those of the local `[agent]`, which win).

| # | Name | Default | Margin | Meaning |
|---|---|---|---|---|
| 1 | `poll_wait` | `55s` | 10 s–300 s | maximum duration of a long-poll |
| 2 | `poll_interval` | `0s` (continuous long-poll) | 0 or 1 min–24 h | interval between short polls (LTE) |
| 3 | `report_min_interval` | `10s` | 1 s–1 h | minimum interval between deltas |
| 4 | `full_interval` | `6h` | 10 min–7 d | periodic full report |
| 5 | `heartbeat` | `5m` | 30 s–1 h | minimum delta with `resources` if nothing changed |
| 6 | `event_batch` | `100` | 1–1,000 | events per batch |
| 7 | `drain_rate` | `50` | 1–1,000 | events/s when draining an accumulated queue |

### 2.10 Reported state (`report`)

| # | Name | Type | Req. | Meaning |
|---|---|---|---|---|
| 1 | `v` | `1` | yes | — |
| 2 | `node` | name | yes | — |
| 3 | `seq` | seq | yes | increasing per node, persisted |
| 4 | `base` | seq | no | present: it is a **delta** (merge patch, RFC 7386) over the `base` report; absent: full |
| 5 | `time` | time | yes | — |
| 6 | `clock` | `clock_quality` | yes | — |
| 7 | `body` | `report-body` | yes | — |

`report-ack`: `ack` (1, the accepted `seq`) and `need_full` (2).

#### 2.10.1 `report-body`

The maps by name (`devices`, `alarms`…) are keyed by the path or name of document 02, §9, so that the merge patch works. In a delta, `null` deletes.

| # | Name | Content | Full includes | Micro profile |
|---|---|---|---|---|
| 1 | `node` | `board` (1), `arch` (2), `os` (3), `mode` (4), `site` (5), `tz` (6), `kind` (7), `boot` (8), `booted` (9) | everything | `board`, `kind`, `boot` |
| 2 | `runtime` | `version` (1), `slot` (2), `previous` (3), `state` (4, `generation_state` of the runtime phase), `proto_minor` (5), `worlds` (6) | everything | `version` (= firmware), `state` |
| 3 | `generation` | `confirmed` (1), `running` (2), `state` (3), `previous` (4), `origin` (5, `gen-origin`), `confirm_by` (6) | everything | `confirmed`, `running`, `state` |
| 4 | `desired` | `seq` (1), `state` (2, `apply-state`), `phase` (3), `wait` (4), `code` (5), `causes` (6, ≤ 16), `fetched` (7, `[done, total]` in bytes), `next` (8), `rollout` (9) | everything | `seq`, `state`, `code` |
| 5 | `drift` | name → `code` (1), `since` (2), `detail` (3): drift of the lock (`J0401`), generation changed locally (`J0966`), files changed in `/var/lib/joker` | everything | — |
| 6 | `health` | `overall` (1, `health`), `codes` (2) | everything | everything |
| 7 | `groups` | group → `state` (1), `period` (2), `jitter_p999` (3), `overruns` (4, since the start of the generation) | everything | `state` |
| 8 | `devices` | device → `link` (1, `link-state`, the six values of ref/00 §6), `since` (2), `driver` (3), `firmware` (4) | everything | `link` |
| 9 | `instances` | instance → `health` (1), `class` (2), `failed` (3, members) | everything | — |
| 10 | `apps` | app → `state` (1), `version` (2), `digest` (3), `restarts` (4) | everything | `state` |
| 11 | `alarms` | alarm → `state` (1), `severity` (2), `since` (3), `shelved_until` (4), `message` (5, ≤ 256 B); only alarms with a state ≠ `normal` | everything | ≤ 8, by severity |
| 12 | `trips` | output → `since` (1), `cause` (2) | everything | everything |
| 13 | `overrides` | output → `level` (1), `by` (2), `until` (3), `reason` (4): active manual writes | everything | — |
| 14 | `resources` | §2.10.2 | everything | micro |
| 15 | `mcus` | device → `link` (1), `board` (2), `firmware` (3), `firmware_digest` (4), `generation` (5, the `source_hash`), `state` (6, `generation_state`, with the confirmation state), `abi` (7), `link_version` (8, the negotiated one), `boot_count` (9), `reset_reason` (10), `counter` (11, `security_version`), `descriptor` (12, digest of the `firmware.jkfw`), `gen_seq` (13), `session_mode` (14, ref/00 §6), `confirm_left` (15, time left in the confirmation window), `params` (16, parameters changed locally: drift, ref/07 §6.4 key 16), `drivers` (17, the summary of the driver descriptors, ref/07 §6.4 key 15) | everything | — |
| 16 | `identity` | `serial` (1), `not_after` (2), `key_storage` (3), `trust_version` (4), `floors` (5, role → seq) | everything | `not_after`, `trust_version` |
| 17 | `agent` | `transport` (1), `queue` (2), `queue_bytes` (3), `dropped` (4), `oldest` (5), `paused_until` (6), `hold` (7) | everything | `queue`, `dropped` |
| 18 | `truncated` | keys of `report-body` that were cut (§3.8.3) | if any | if any |
| 19 | — | reserved: pending intents (M18) | — | — |
| 20 | — | reserved: models and shadow (1.1) | — | — |
| 21 | `secrets` | secret → `rev` (1), `ok` (2, decrypted and verified) — names and revisions, **never** values (ref/00 §9) | everything | everything |
| 22 | `drivers` | the driver manifest of the binary (ref/06 §3.8): driver → `version` (1), `tier` (2), `descriptor` (3, digest of the descriptor) | everything | — |

`gen-ref`, used in `generation` and in the `apply` events: `gen` (1, the integer of the node), `hash` (2), `at` (3, when it was swapped or confirmed), `by` (4, principal: `agent`, `user:<id>`, `deck:<deck_id>/user:<id>`), `label` (5).

**`health.overall`**, computed by the node:

| Value | When |
|---|---|
| `failed` | a group `stopped` or `tripped` by overrun; an app in `crashloop` or `failed`; the last apply ended in `failed`; the node did not start the confirmed generation |
| `degraded` | a device `down`, `degraded` or `drift`; an instance not `ok`; an active trip; an active `critical` alarm; clock `unsynced`; drift |
| `ok` | none of the above |
| `unknown` | still starting |

`health.codes` lists the J-codes that justify the value (for example `J0401`).

#### 2.10.2 `resources`

| # | Name | Type | Unit | Profile |
|---|---|---|---|---|
| 1 | `cpu` | pct | % 1 min average | Linux |
| 2 | `mem_used` | uint | B | Linux |
| 3 | `mem_total` | uint | B | Linux |
| 4 | `disk_free` | uint | B, in `/var/lib/joker` | Linux |
| 5 | `disk_total` | uint | B | Linux |
| 6 | `wear` | pct | estimated eMMC wear (document 08, round 5 (e)) | Linux |
| 7 | `temp` | float | °C | both |
| 8 | `throttled` | bool | — | Linux |
| 9 | `store_lag` | duration | lag of the slowest cursor of the historian | Linux |
| 10 | `heap_free` | uint | B | micro |
| 11 | `heap_min` | uint | B, minimum since boot | micro |
| 12 | `rssi` | int | dBm | micro |
| 13 | `boot_count` | uint | — | both |
| 14 | `reset_reason` | tstr | — | micro |
| 15 | `uplinks` | uplink → `lag` (1), `budget_used` (2, %) | — | Linux |

#### 2.10.3 Example of a full report (JSON)

```json
{
  "v": 1,
  "node": "hb-01234",
  "seq": 90211,
  "time": "2026-09-24T10:00:00.120Z",
  "clock": "synced",
  "body": {
    "node": { "board": "cm4-din", "arch": "aarch64", "mode": "prod", "site": "hb-01234", "kind": "linux" },
    "runtime": { "version": "1.0.3", "slot": "b", "previous": "1.0.2", "state": "confirmed", "proto_minor": 0 },
    "generation": {
      "confirmed": { "gen": 37, "hash": "sha256:4c1d0e8f6a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5", "at": "2026-09-24T02:41:10Z", "by": "agent" },
      "running": { "gen": 37, "hash": "sha256:4c1d0e8f6a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5" },
      "state": "confirmed",
      "origin": "agent"
    },
    "desired": { "seq": 418, "state": "confirmed" },
    "health": { "overall": "degraded", "codes": ["J0401"] },
    "groups": { "main": { "state": "ok", "period": "10s", "jitter_p999": "1.8ms", "overruns": 0 } },
    "devices": { "hp": { "link": "up" }, "heatmeter": { "link": "up" }, "pv": { "link": "drift" } },
    "apps": { "hp_opt": { "state": "running", "version": "3.2.0" }, "grid_limit": { "state": "running", "version": "1.1.0" } },
    "alarms": {},
    "trips": {},
    "overrides": {},
    "resources": { "cpu": 7.5, "mem_used": 402653184, "mem_total": 2147483648, "disk_free": 9663676416, "wear": 3, "temp": 51.0 },
    "identity": { "not_after": "2027-06-01T00:00:00Z", "key_storage": "file", "trust_version": 3 },
    "agent": { "transport": "https", "queue": 0, "dropped": 0 }
  }
}
```

### 2.11 Events

`event-batch`: `node` (1), `events` (2, ≤ `event_batch`). `event-ack`: `ack` (1).

`event`:

| # | Name | Type | Req. | Meaning |
|---|---|---|---|---|
| 1 | `seq` | seq | yes | increasing, contiguous, persisted; a lost event keeps its `seq` (§3.9.3) |
| 2 | `id` | ULID | yes | global identifier, stable across resends |
| 3 | `ts` | time | yes | UTC of the node at the moment of the event |
| 4 | `clock` | `clock_quality` | yes | quality of the clock at that moment |
| 5 | `boot` | ULID | yes | boot in which the event happened |
| 6 | `mono` | uint | yes | monotonic ns since that boot (pair of `ts`, ref/00, §3) |
| 7 | `type` | `event-type` | yes | — |
| 8 | `body` | per `type` | yes | — |

| `type` | Body | Class in the queue (§3.9.3) |
|---|---|---|
| `alarm` | `name` (1), `from` (2), `to` (3), `severity` (4), `message` (5), `by` (6), `shelved_until` (7) | A |
| `decision` | `output` (1), `writer` (2), `outcome` (3), `reason` (4), `level` (5), `value` (6), `applied` (7), `ttl` (8), `why` (9), `session` (10), `count` (11), `tick` (12) | B |
| `apply` | `seq` (1, of the DSD), `state` (2), `phase` (3), `gen` (4), `device` (5), `code` (6), `causes` (7), `rollout` (8) | A |
| `trip` | `output` (1), `action` (2, `trip-action`), `cause` (3), `by` (4) | A |
| `security` | `kind` (1), `code` (2), `detail` (3), `kid` (4), `seq` (5) | A |
| `session` | `id` (1), `action` (2), `user` (3), `role` (4), `code` (5), `requests` (6), `writes` (7) | A |
| `gap` | `ranges` (1, `[[from, to], …]`, ≤ 64), `counts` (2, `event-type` → n) | never lost |
| `clock` | `boot` (1), `mono` (2), `utc` (3), `quality` (4), `source` (5) | B |
| `command_result` | `id` (1), `status` (2), `code` (3) | B |
| `cert` | `action` (1), `not_after` (2), `serial` (3) | A |
| `agent` | `action` (1), `by` (2), `until` (3), `detail` (4) | C |

The values in `decision.value`/`applied` follow ref/00, §4.1, in the unit of the signal. `writer` uses the forms of ref/00, §5, plus `user:deck:<deck_id>/user:<id>` for writes in remote sessions (the writer `user:<principal>` of ref/00 §5, with the principal of the session).

**Which decisions go to the Deck** (`[agent.audit] decisions`, §2.17). The local decision log keeps **all** of them (M3); the Deck only receives the marked ones:

| Value | What it sends |
|---|---|
| `manual` | all writes, releases and method calls of `manual` writers |
| `session` | all writes made in remote sessions (**always on**, it cannot be turned off) |
| `trip`, `reset` | trips and resets (also as `trip` events) |
| `rejected` | transitions to `rejected` or `tripped` per (output, writer, reason): the first one, and then one with `count` when the result changes or every hour |
| `bridge` | writes from bridges, aggregated like `rejected` |
| `action` | actions of devices and instances (M19) |
| `intent` | reserved (M18, 1.1) |

Default: `["manual", "session", "trip", "reset", "rejected", "action"]`.

### 2.12 Remote session (agent/1.3)

`session-open-args`:

| # | Name | Type | Req. | Validation | Error |
|---|---|---|---|---|---|
| 1 | `id` | ULID | yes | new | — |
| 2 | `user` | `user-claim`: `id` (1), `name` (2), `groups` (3, ≤ 32), `mfa` (4) | yes | the Deck attests the identity; the node decides the role | — |
| 3 | `role` | `role` | yes | ≤ granted role (§3.10) | `J0981` |
| 4 | `reason` | tstr 1–256 B | yes | — | — |
| 5 | `ttl` | duration | yes | ≤ `[agent.remote] max_ttl` | `J0983` |
| 6 | `tunnel` | tstr | no | `wss://` of one of the `deck_urls`; default `<deck>/agent/v1/sessions/{id}/tunnel` | `J0985` |

**Tunnel.** A WebSocket connection (RFC 6455) **opened by the node**, with mTLS. Each binary message is a `tunnel-frame` in CBOR: `[stream, kind, payload?]`.

| `kind` | `payload` | Direction | Meaning |
|---|---|---|---|
| `open` | uint: destination (0 = local API; others reserved) | Deck → node | opens a byte stream to the local API |
| `data` | bstr ≤ 64 KiB | ↔ | bytes of the stream |
| `close` | — | ↔ | orderly close |
| `reset` | uint: `agent-status` | ↔ | abrupt close |
| `credit` | uint: bytes | ↔ | credit-based flow control (initial: 256 KiB per stream) |
| `ping` | — | ↔ | liveness; no response within 30 s closes the tunnel |

Each stream is **one connection to the local API** (ref/09), as if it came from the Unix socket, but with the pre-authenticated principal `deck:<deck_id>/user:<user.id>` (ref/00 §5) and the effective role of the session. HTTP/1.1, HTTP/2 and the WebSocket of the API pass through as they are. `jk --node deck:<node>` and the Studio are clients of this API.

### 2.13 HTTPS: Deck endpoints

Base: one of the `deck_urls` of the bundle, with the suffix `/agent/v1`. TLS 1.3 (1.2 accepted in the micro profile), HTTP/2 preferred, mTLS on everything except `/enroll`. `Content-Type: application/json` by default; `application/cbor` if negotiated; `application/cose` for a standalone signed document.

| Method and path | Authentication | Request | Response | Notes |
|---|---|---|---|---|
| `POST /enroll` | token in the body | `enroll-request` | `enroll-reply` | without mTLS |
| `GET /enroll/nonce` | none | — | `{"nonce": <bytes>}` | 32 B nonce for attestation (1.3); without mTLS |
| `POST /renew` | mTLS, accepts an expired certificate up to `renew_grace` | `renew-request` | `renew-reply` | — |
| `GET /trust?from=<version>` | mTLS | — | `[+ cose]` | chain of bundles from `from` on |
| `POST /hello` | mTLS | `hello` | `hello-reply` | at the start of each connection |
| `GET /poll?desired=<seq>&cursor=<c>&wait=<d>` | mTLS | — | `poll-reply` | long-poll (§3.11.1); responds at once if there is something new |
| `POST /report` | mTLS | `report` | `report-ack` | `409 need_full` if the base does not exist |
| `POST /events` | mTLS | `event-batch` | `event-ack` | — |
| `POST /registry-token` | mTLS | `{"scope": [...]}` | `registry-token`: `token` (1), `expires` (2), `scope` (3) | *bearer* token for the `auth = token` sources |
| `GET /blobs/{digest}` | mTLS | — | bytes | artifact proxy (`kind = deck`); accepts `Range` |
| `PUT /uploads/{id}` | mTLS | body of §2.7.1, `Content-Encoding: zstd` optional | `status-reply` | only for an `upload` requested and still open |
| `GET /sessions/{id}/tunnel` | mTLS + WebSocket | — | `tunnel-frame` | 1.3 |

Headers: the node sends `User-Agent: jokerd/<version> agent/1.<minor>`; the Deck responds with `Date` and, on errors, `Retry-After`.

### 2.14 MQTT: topics

MQTT 5 (3.1.1 accepted in the micro profile), over TLS with mTLS. Client identifier: `<org>.<node>`. Prefix: `jk/1/<org>/<node>/`.

| Suffix | Direction | QoS | Retain | Content |
|---|---|---|---|---|
| `hello` | node → Deck | 1 | no | COSE(`hello`) |
| `hello/reply` | Deck → node | 1 | no | COSE(`hello-reply`) |
| `desired` | Deck → node | 1 | **yes** | the DSD (already COSE) |
| `cmd` | Deck → node | 1 | no | command (already COSE); *message expiry* = `expires` |
| `report` | node → Deck | 1 | no | COSE(`report`) |
| `ack/report` | Deck → node | 1 | no | COSE(`report-ack`) |
| `events` | node → Deck | 1 | no | COSE(`event-batch`) |
| `ack/events` | Deck → node | 1 | no | COSE(`event-ack`) |
| `status` | node → Deck | 1 | **yes** | COSE(`{1: online bool, 2: boot, 3: time}`); also as the *will* with `online = false`, signed at connection time |
| `blob/req` | node → Deck | 1 | no | COSE(`blob-request`), micro profile only |
| `blob/data` | Deck → node | 1 | no | `blob-chunk` in CBOR, unsigned (integrity comes from the digest) |

| Parameter | Linux | Micro |
|---|---|---|
| `clean start` | `false` | `false` |
| *session expiry* | `7d` (`[agent.mqtt] session_expiry`) | `1d` |
| *keepalive* | `60s`; `5m`–`20m` on LTE | `120s` |
| maximum packet size | 256 KiB | 4 KiB |
| *receive maximum* | 16 | 2 |

**Broker ACL** (required, checked by `jk doctor` when possible): a node only publishes on `jk/1/<org>/<node>/{hello,report,events,status,blob/req}` and only subscribes to `jk/1/<org>/<node>/{hello/reply,desired,cmd,ack/#,blob/data}`, with `<node>` equal to the `CN` of the certificate. A publication refused by the broker appears locally as `J0977`.

Remote sessions do not go over MQTT (`J0985`).

### 2.15 Micro profile

For a standalone Joker Micro (M2, with network). The same CDDL, with these restrictions:

| Message | Allowed keys | Maximum size |
|---|---|---|
| `desired-state` | 1–10, 12, 14 (no `tz` ≠ `UTC`), 16, 17, 18, 20, 22 | 2 KiB |
| `runtime-target` | 1–4; `artifact` of type `joker.micro-firmware.v1` | — |
| `generation-target` | 1, 2; `config` of type `joker.generation.v1` | — |
| `command` | `confirm`, `reject`, `report_full`, `renew` | 512 B |
| `report` | those of the "micro profile" column of §2.10.1 | 1 KiB |
| `event-batch` | types `alarm`, `apply`, `trip`, `security`, `gap`, `clock`, `cert` | 1 KiB, ≤ 8 events |
| `hello`, `hello-reply` | all | 512 B |

A key outside the subset listed in `crit` is `J0990`; outside `crit`, it is ignored. A window with `tz` ≠ `UTC` is `J0991`.

**CoAP** (RFC 7252) over DTLS 1.2 with a certificate, base `coaps://<host>/a1/`:

| Resource | Method | Request | Response | Notes |
|---|---|---|---|---|
| `/a1/h` | POST | `hello` | `hello-reply` | — |
| `/a1/d?s=<seq>` | GET + Observe (RFC 7641) | — | DSD (Content-Format 18, COSE_Sign1) | notification at each new DSD |
| `/a1/c?k=<cursor>` | GET + Observe | — | `[* cose]` | commands |
| `/a1/r` | POST | `report` | `report-ack` | — |
| `/a1/e` | POST | `event-batch` | `event-ack` | — |
| `/a1/b/<digest in hex>` | GET + Block2 (RFC 7959), blocks of 512 B or 1 KiB | — | bytes | artifacts |
| `/a1/k` | POST | `renew-request` | `renew-reply` | — |

Content-Format 60 (`application/cbor`) for the unsigned messages. With DTLS terminated at the Deck, the messages of the node do not go in COSE (as over HTTPS).

**Artifacts in the micro profile**, in order of preference: HTTPS with `Range` (ESP-IDF HTTPS OTA or equivalent), CoAP Block2, or MQTT `blob/req` + `blob/data` in pieces of ≤ 2 KiB. The MCU requests one piece at a time; the download resumes by `offset`.

| Message | Keys |
|---|---|
| `blob-request` | `digest` (1), `offset` (2, bytes), `length` (3, ≤ 2,048) |
| `blob-chunk` | `digest` (1), `offset` (2), `data` (3, ≤ requested `length`), `total` (4, size of the artifact, in the first piece) |

A piece with a `digest` or `offset` that the MCU did not request is ignored. The digest of the whole artifact is verified at the end, and the signature before the image is recorded as bootable.

`status` (MQTT): `online` (1, bool), `boot` (2), `time` (3). `registry-token` is requested with `{"scope": ["repository:<name>:pull", …]}` (`registry-token-request`, `scope` (1)).

### 2.16 Artifact types and signing roles

| Media type (short form of ref/00) | Signing role | Phase | Version |
|---|---|---|---|
| `joker.runtime.v1` | `runtime` | `runtime` | 1.0 (new type, §5, D11) |
| `joker.config.v1+cbor` | `config` | `generation` | 1.0 (new type, §5, D11) |
| `joker.app.v1+wasm` | `apps` | `generation` | 1.0 |
| `joker.profile.v1+toml` | `profiles` | `generation` | 1.0 |
| `joker.board.v1+toml` | `boards` | `generation` | 1.0 |
| `joker.class.v1+jkc` | `classes` | `generation` | 1.0 |
| `joker.micro-firmware.v1` | `firmware` | `mcu` or `runtime` (micro) | 1.0 |
| `joker.generation.v1` | `config` | `mcu` or `generation` (micro) | 1.0 |
| `joker.driver.v1+wasm`, `joker.driver.v1+tar` | `drivers` | `generation` | 1.0 |
| `joker.lock.v1+toml` | (inside the generation: covered by `config`) | `generation` | 1.0 |
| `joker.secrets.v1` | `secrets` (per entry) | key 22; it is not a phase | 1.0 |
| `joker.model.v1+jkm` | `models` | `generation` | **reserved 1.1**: a 1.0 node rejects it with `J0957` |

An artifact is accepted if it has at least one valid signature from a key of the bundle with the role of the table, and if the `[security] trust` of the manifest does not exclude it. An unsigned artifact is only accepted if the target generation has `mode = "dev"`, the bundle has `min_mode = dev` **and** `node.toml` has `[policy] allow_dev = true` (§3.3, V19; ref/02 §3.5.1). The DSD, the commands, the bundles and the floors are always signed, in any mode.

The media types and the roles are those of the single table of ref/00 §7.1 (D13); this table only says in which phase they come in. In the OCI registry, the full `mediaType`/`artifactType` is `application/vnd.joker.<rest>` (for example `application/vnd.joker.app.v1+wasm`).

### 2.17 Agent configuration on the node

The startup configuration of the node belongs to ref/02 (`/etc/joker/node.toml`), which hosts this `[agent]` section as is; the meaning of the keys is defined here:

```toml
[agent]
enabled             = true                 # default: true if the bundle has an authority
transport           = "https"              # https | mqtt | coap (micro)
urls                = []                   # overrides deck_urls of the bundle (same org)
poll                = "long"               # long | duration (e.g. "15m" on LTE)
backoff_max         = "15m"
offline_warn        = "7d"
local_changes       = "hold"               # hold | overwrite
max_applies_per_day = 6
mirrors             = ["https://mirror.example.com"]
cbor                = false                # CBOR over HTTPS (savings on LTE)
maintenance         = [{ days = [1, 2, 3, 4, 5], start = "01:00", length = "4h", tz = "Europe/Lisbon" }]

[agent.queue]
max_disk = "64 MiB"

[agent.report]
min_interval  = "10s"
full_interval = "6h"
heartbeat     = "5m"

[agent.audit]
decisions = ["manual", "session", "trip", "reset", "rejected", "action"]

[agent.upload]
allow     = ["decisions", "logs", "events"]
max_bytes = "64 MiB"

[agent.mqtt]
url            = "mqtts://mqtt.example.com:8883"
keepalive      = "60s"
session_expiry = "7d"

[agent.remote]                              # agent/1.3
allow           = false
max_role        = "operator"                # viewer | operator | admin
default_role    = ""                        # role for Deck users without a mapping; empty = refuse
consent         = "none"                    # none | local
consent_signal  = ""                        # bool signal that authorizes (e.g. a "remote maintenance" switch)
consent_timeout = "5m"
max_sessions    = 2
max_ttl         = "8h"
idle            = "15m"
max_rate        = "1 MiB/s"
```

| Key | Type | Default | Validation | Meaning |
|---|---|---|---|---|
| `enabled` | bool | see above | — | turns the agent off without unenrolling |
| `transport` | enum | `https` | `coap` only on micro | — |
| `urls` | list of URLs | `[]` | same scheme as the bundle | — |
| `poll` | `long` or duration | `long` | 1 min–24 h | local `poll_interval` (wins over that of the Deck) |
| `backoff_max` | duration | `15m` | 1 min–6 h | ceiling of the backoff (§3.11.4) |
| `offline_warn` | duration | `7d` | ≥ 1 h | alarm `joker.agent.offline` |
| `local_changes` | enum | `hold` | — | §3.7 |
| `max_applies_per_day` | uint | 6 | 1–100 | swaps per sliding 24 h, of any phase |
| `mirrors` | list | `[]` | ≤ 4 | sources tried before those of the DSD |
| `cbor` | bool | `false` | — | — |
| `maintenance` | list of windows | no restriction | ≤ 16 | intersection with those of the DSD |
| `queue.max_disk` | size | `64 MiB` | 1 MiB–4 GiB | ceiling of the event queue |
| `report.*` | durations | §2.9 | margins of §2.9 | win over those of the Deck |
| `audit.decisions` | list | above | values of §2.11 | `session` is always included |
| `upload.allow` | list | `["decisions", "logs", "events"]` | `upload-kind` | `trace` must be requested explicitly |
| `upload.max_bytes` | size | `64 MiB` | ≤ 1 GiB | — |
| `mqtt.*` | — | above | — | §2.14 |
| `remote.*` | — | above | §3.10 | — |

The keys belong to a file written by people: an unknown key is `J0140` (ref/00, §1). No `[agent]` key arrives through the protocol: the Deck cannot change it.

### 2.18 Local signals, alarms and commands

Signals of the node (reserved prefix `joker.`, recorded and alarmable like the others):

| Signal | Type | Unit | Meaning |
|---|---|---|---|
| `joker.agent.connected` | bool | — | last successful exchange with the Deck less than 3 × `poll_wait` ago |
| `joker.agent.offline_for` | f64 | s | time since the last successful exchange |
| `joker.agent.queue` | u64 | — | events awaiting acknowledgment |
| `joker.agent.dropped` | u64 | — | events lost since enrollment |
| `joker.agent.desired_seq` | u64 | — | last DSD accepted |
| `joker.agent.cert_days` | f64 | d | days until the end of the certificate |

Alarms of the node:

| Alarm | Severity | When |
|---|---|---|
| `joker.agent.offline` | `medium` | `offline_for` > `offline_warn` (`J0974`) |
| `joker.agent.cert_expiring` | `medium` | less than 1/3 of the validity left and renewal failed (`J0935`) |
| `joker.agent.cert_expired` | `high` | certificate expired |
| `joker.agent.revoked` | `high` | the Deck answered `revoked` (`J0934`) |
| `joker.agent.quarantined` | `high` | the Deck answered `quarantined` (`J0937`, `J0938`) |
| `joker.agent.refused` | `high` | DSD, command, bundle or floor refused for security (`J0939`–`J0945`, `J0951`) |
| `joker.agent.revoked_artifact` | `high` | a running artifact is in a `revoked` floor |
| `joker.agent.queue_overflow` | `medium` | events lost (`J0970`) |
| `joker.agent.hold` | `low` | desired state on hold because of local changes or a pause |
| `joker.agent.session` | `low` | there is an open remote session |
| `joker.agent.session_request` | `medium` | session request awaiting local consent |

`jk` commands (document 07; all talk to the local API, ref/09 §5.21):

| Command | Role | Does | Local API (ref/09 §5.2, §5.21) |
|---|---|---|---|
| `jk node enroll <target> --ca <dir>` | `admin` | enrolls without a Deck, with a local CA (§3.1.1) | `createEnrollmentCsr`, `installEnrollmentCertificate` |
| `jk node enroll <target> --deck <url> --token <t>` | `admin` | enrolls in a Deck (§3.1.2) | `enrollWithDeck` |
| `jk node enroll <mcu> --link <device>` | `admin` | enrolls an MCU behind the node and provisions the `K_link` (ref/07 §4.7) | — (local, through a probe or USB; the node receives the secret through `pushSecrets`) |
| `jk node renew` | `admin` | renews the certificate now | `renewNode` |
| `jk node trust [show \| push <file.cose> \| set --anchors …]` | `viewer` / `admin` | shows the bundle and the anchors, installs a bundle, replaces the anchors (verified like the others) | `getNodeTrust`, `pushTrustBundle`, `setTrustAnchors` |
| `jk agent status` | `viewer` | connection, authority, seq, queue, pause, hold, sessions | `getAgent` |
| `jk agent desired [--json]` | `viewer` | the current DSD, decoded, and the state of the apply | `getAgentDesired` |
| `jk agent pause --ttl <d> --reason "…"` / `jk agent resume` | `admin` | suspends the swaps (download and reports continue) | `pauseAgent`, `resumeAgent` |
| `jk agent import <dir>` | `admin` | applies a DSD and artifacts brought by hand (§3.7.3) | blob uploads, `importAgentBundle` |
| `jk session list \| accept <id> \| close <id>` | `operator` | remote sessions (1.3) | `listAgentSessions`, `acceptAgentSession`, `closeAgentSession` |

None of these endpoints can be used **inside** a remote session (§3.10, `J0986`), except the reads `getAgent` and `getAgentDesired`.

### 2.19 Limits

| Thing | Linux | Micro |
|---|---|---|
| DSD | 64 KiB | 2 KiB |
| artifacts per DSD | 256 | 4 |
| MCUs per DSD | 32 | — |
| command | 4 KiB | 512 B |
| commands per `poll-reply` | 16 | 4 |
| trust bundle | 16 KiB, 32 keys | 2 KiB, 8 keys |
| full report | 256 KiB (JSON form) | 1 KiB |
| delta | 32 KiB | 512 B |
| event batch | 256 KiB, ≤ 1,000 events | 1 KiB, ≤ 8 |
| body of an event | 4 KiB | 256 B |
| alarm `message` | 256 B | 64 B |
| event queue | `queue.max_disk` (default 64 MiB) | 32 events in RAM (+ flash, if any) |
| simultaneous sessions | `max_sessions` (≤ 4) | — |
| streams per tunnel | 16 | — |
| upload | `upload.max_bytes` (≤ 1 GiB) | — |
| MQTT message | 256 KiB | 4 KiB |

A message above the limit is `too_large` with `J0976`.

### 2.20 Codes

Range `J0930`–`J0999` (ref/00, §8: `J09xx`, fleet). Severities of document 16, §9.

| Code | Severity | Message |
|---|---|---|
| `J0930` | error | enrollment token invalid, expired or already used |
| `J0931` | error | the bundle received at enrollment is not signed by the root given in the token |
| `J0932` | error | node name invalid, already used in the organization, or different from the CSR |
| `J0933` | warning | identity key in a file, without a TPM or secure element (or required attestation missing) |
| `J0934` | error | node certificate revoked |
| `J0935` | warning | node certificate near its end or expired; renewal pending |
| `J0936` | error | renewal refused: outside `renew_grace` or invalid proof |
| `J0937` | error | hardware fingerprint differs from the one at enrollment: node in quarantine |
| `J0938` | error | the same identity is connected from two nodes: suspected clone |
| `J0939` | error | invalid trust bundle or floor: signature, unknown root or non-increasing version |
| `J0940` | error | invalid COSE signature, algorithm not accepted or unknown `kid` |
| `J0941` | error | the document is not from the node's authority (`deck` role missing or another Deck) |
| `J0942` | error | desired state `seq` is not greater than the last accepted one |
| `J0943` | error | `seq` repeated with different content (two issuers) |
| `J0944` | error | document or command addressed to another node or organization |
| `J0945` | error | document expired or issued in the future (clock synchronized) |
| `J0946` | error | unsupported format: version, unknown critical key or invalid source |
| `J0947` | error | unauthorized downgrade, or downgrade below the security floor |
| `J0948` | error | runtime incompatible with the board, the architecture or the local state |
| `J0949` | error | artifact list inconsistent with the generation, the lock or the MCUs |
| `J0950` | error | downloaded artifact does not match the digest or the size |
| `J0951` | error | desired state or artifact without any accepted signature for its type (an invalid signature is `J0940`) |
| `J0952` | error | artifact not downloaded within `fetch_deadline` |
| `J0953` | error | insufficient disk space to prepare the apply |
| `J0954` | error | generation rejected by validation (causes in `causes`) |
| `J0955` | error | computed generation hash differs from the declared one |
| `J0956` | error | `mode = "dev"` generation rejected on a `prod` node |
| `J0957` | error | artifact type or feature not supported in this version |
| `J0958` | error | app compiled for a WIT world that the target runtime does not accept |
| `J0959` | warning | apply deferred (window, clock, pause, local changes) |
| `J0960` | error | confirmation window expired: rollback |
| `J0961` | error | health failed during confirmation: rollback |
| `J0962` | warning | local limit of applies per day reached |
| `J0963` | error | incompatible MCU firmware (board, ABI, security counter) |
| `J0964` | error | MCU generation blob rejected (cause in `causes`, e.g. `J0710`) |
| `J0965` | error | MCU update over Joker Link failed or did not confirm |
| `J0966` | warning | running generation changed locally: desired state on hold |
| `J0967` | error | command without a valid target (phase not pending or different hash) |
| `J0968` | error | command outside the time window or expired |
| `J0969` | error | generation rejected by the Deck during confirmation: rollback |
| `J0970` | warning | event queue full: events dropped |
| `J0971` | warning | report truncated for exceeding the limit |
| `J0972` | warning | node clock skewed against the Deck by more than 5 min |
| `J0973` | warning | node protocol minor outside the ones supported by the Deck (minimal core only) |
| `J0974` | warning | Deck unreachable for more than `offline_warn` |
| `J0975` | error | Deck or broker certificate does not match the bundle pins |
| `J0976` | error | message above the transport or profile limit |
| `J0977` | error | MQTT publish rejected by the broker (ACL) |
| `J0978` | error | malformed message, or message with mixed key forms |
| `J0979` | error | invalid confirmation policy, calendar or window |
| `J0980` | error | remote sessions disabled on this node |
| `J0981` | error | requested role above what the node grants to this Deck user |
| `J0982` | error | limit of concurrent sessions reached |
| `J0983` | error | session expired, closed, or `ttl` above `max_ttl` |
| `J0984` | error | local consent refused or not given in time |
| `J0985` | error | transport without support for remote sessions |
| `J0986` | error | tunnel request to an endpoint forbidden in remote sessions |
| `J0990` | error | micro profile: critical key outside the subset |
| `J0991` | error | micro profile: window with a time zone other than UTC |
| `J0992` | error | desired state with a generation rejected: the node's `[policy] apply_from` does not include `deck` |

`J0988`–`J0989` and `J0993`–`J0999` stay free (`J0987` was assigned by ref/02, §2.6). The codes of the secret entries (`J0922`–`J0926`) and of the mode policy (`J0906`) are those of ref/02.

---

## 3. Expected behavior

### 3.1 Enrollment

#### 3.1.1 Without a Deck: `jk node enroll <target> --ca <dir>`

For development, for the track and for those who do not want a Deck (document 20, D5).

```
jk (laptop)                                node (jokerd)
  │ ssh + local socket /run/joker/api.sock   │
  │── request CSR ──────────────────────────▶│ generates the P-256 key (TPM if present)
  │◀─ CSR, hw, key_storage ─────────────────│
  │ signs the CSR with the CA of <dir>        │
  │ signs a bundle without authority with the root of <dir>
  │── install cert + bundle ───────────────▶│ verifies: bundle signed by the root; cert = key
  │◀─ ok ───────────────────────────────────│ agent off (no authority)
```

The CA directory has `root.key` (Ed25519, the root), `ca.key`/`ca.crt` (P-256, the issuer), `org` (name), `dev` (empty file, only in a development CA) and, after `jk ca bundle`, `trust.cose` (the trust bundle, ref/02 §2.5.5). `jk` creates it with `jk ca init <dir> --org <org>`, or in `jk node enroll --ca <dir>` if it does not exist (with `--dev`). If the directory has `trust.cose` (the bundle from `jk ca bundle`, ref/02 §2.5.5), that is the one the enrollment installs, instead of the bundle without `authority` of the diagram. `jk ca init` also creates the user certificate `admin-<user>` (SAN `urn:joker:role:admin`) and registers it in `[user]` of the `config.toml` of the client (ref/02 §3.3.3). The bootstrap channel to the node is that of ref/02 §3.2.3: the local socket, with physical access or over SSH (`--node ssh://…`; `createEnrollmentCsr` and `installEnrollmentCertificate`, ref/09 §5.2), with the single-use code (`--code`) over the same socket when the node shows it. **There is never an enrollment port open to the network**; in enrollment through the Deck (§3.1.2) it is the node that connects outward, with the token. The CSR carries the KEM key in the SAN (§2.4.2), and the installation writes the anchors and the first bundle.

#### 3.1.2 With a Deck: `jk node enroll <target> --deck <url> --token <t>`, or `jokerd enroll`

```
node                                                   Deck
 │ generates the key; CSR; hw                            │
 │── POST /enroll {token, node, csr, hw, key_storage} ──▶│ validates the token (J0930), the name (J0932)
 │                                                       │ issues the certificate; marks the token used
 │◀─ {cert, trust, deck, settings} ─────────────────────│
 │ verifies: root of the bundle has the kid of the token (J0931)
 │           cert is of the generated key; scope includes the node
 │ installs; hello; full report                          │
```

- The key **never leaves the node**: it is the node that talks to the Deck, and `jk` only passes it the URL and the token.
- **Zero-touch**: the image of the node carries `jokerd enroll --deck <url> --token-file <f>` at first boot, with a token bound to the site. The token is a secret and is deleted after use.
- A node already enrolled refuses a new enrollment without `--force`; with `--force`, it generates a new key (the old identity is orphaned and the Deck revokes it).
- Changing Deck is a new bundle with another `authority`, signed by the root (§3.2.4), or a new enrollment.

#### 3.1.3 Standalone MCU

The MCU generates the identity key (secure element, if there is one) and the X25519 secrets key at first boot. Enrollment is done by the tool connected over USB or by Joker Link (`jk node enroll mcu:<port> --deck <url> --token <t>`): `jk` requests the CSR from the MCU (with the KEM key in the SAN), makes the `POST /enroll` with `kind = micro`, and returns to the MCU the certificate, the anchors and the bundle, which the MCU verifies as in §3.1.2 and stores in the `IDENTITY` area (registers 18 and 19, ref/08 §2.10.2; bundle ≤ 2 KiB). An MCU with HTTPS can enroll on its own.

An MCU **behind a node** has no certificate: it talks to the node over Joker Link, with the `K_link`, which `jk node enroll <mcu> --link <device>` generates and provisions (ref/07 §4.7): on the MCU, registers 16 and 17 of the `IDENTITY` area; on the node, the secret `secret:link.<device>`.

### 3.2 Renewal, revocation and cloned nodes

#### 3.2.1 Renewal

- From **2/3 of the validity** on, the node tries to renew once a day, with a new key (`renew-request`). Success: event `cert` `renewed`; the old certificate stops being used.
- A `renew` command brings the renewal forward.
- Renewal is atomic: the new key and certificate only replace the old ones after the `renew-reply` is stored with fsync. A failure halfway leaves the old pair.
- With the certificate **expired**, the node keeps trying `POST /renew` (the only endpoint that the Deck accepts under those conditions), up to `renew_grace`. After that, `J0936`: a new on-site enrollment is needed.
- None of this affects control. The node controls with the certificate expired, revoked or with none at all.

#### 3.2.2 Revocation

- It is a decision of the Deck (the CA is its own). The Deck refuses the TLS or responds `revoked` (`J0934`) on any endpoint. The MQTT broker must consult the revocation list of the Deck (CRL or OCSP of the issuing CA); `jk doctor` warns if the broker does not.
- The revoked node stops the agent, raises `joker.agent.revoked` and keeps working in the last confirmed state. A revoked node is never "switched off" by the Deck: principle 1 holds here too.
- Revoking a **key of the bundle** (for example, a compromised `deck` key) is a new bundle without that key, signed by the root. The Deck delivers it, but cannot forge it.

#### 3.2.3 Cloned node

| Case | Detection | Reaction |
|---|---|---|
| key in TPM or secure element | impossible to clone the key; the clone does not do TLS | — |
| key in a file, card copied to another board | `hello.hw` ≠ enrollment fingerprint | the Deck responds `quarantined` (`J0937`): it accepts reports and events, marked, and delivers neither DSD nor commands |
| two boards with the same identity connected at the same time | two different `boot`s with overlapping connections, or `event_seq` going backward | both in quarantine (`J0938`) |

The **hardware fingerprint** (`hw`) is the SHA-256 of `board serial ‖ SoC serial ‖ MAC of the primary interface ‖ EK public key of the TPM (if any)`, each field in UTF-8 and separated by `\n`. A legitimate hardware swap (repair) is resolved in the Deck (accepting the new fingerprint) or with a new enrollment. Quarantine never touches the control of the node.

#### 3.2.4 Bundle and floors

- The node accepts bundles from three places: `enroll-reply`, `hello-reply`/`renew-reply`/`GET /trust` and the `trust` field of the DSD. The rule is always that of §2.4.3.
- A bundle that changes the `authority` makes the node forget the `seq` of the previous Deck: the next DSD from the new authority may start at 1. It is the only way for the `seq` to restart.
- A new floor applies immediately to what has not been swapped yet. If what **is running** is revoked, the node raises `joker.agent.revoked_artifact` and **keeps running**: stopping would be worse than the risk, and the fix is a new DSD.

### 3.3 Validation of the desired state

The node validates in this order. The first step that fails decides the result.

| # | Step | Failure | Is the `seq` consumed? |
|---|---|---|---|
| V1 | well-formed COSE, `alg` ∈ {−8, −7}, correct `content type` | `J0940` | no |
| V2 | `kid` in the installed bundle; `not_after` (if the clock is `synced`) | `J0940` | no |
| V3 | the key has the `deck` role and its `deck` = `authority` | `J0941` | no |
| V4 | valid signature | `J0940` | no |
| V5 | deterministic CBOR, `v` = 1, all keys of `crit` known, limits of §2.19 | `J0946`, `J0976` | no |
| V6 | `node` and `org` are those of this node | `J0944` | no |
| V7 | `seq` > last accepted. Equal and with the same content: `noop`. Equal with different content: `J0943`. Lower: `J0942` | `J0942`, `J0943` | no |
| V8 | `issued` and `expires`, only with the clock `synced` | `J0945` | no |
| — | **accepted:** the DSD is stored with fsync, the `seq` becomes the last accepted, event `apply` `accepted`; a previous DSD not yet swapped becomes `superseded` | — | — |
| V9 | each bundle in `trust`, in order (§2.4.3) | `J0939` | yes |
| V10 | each floor in `floors` (§2.4.4) | `J0939` | yes |
| V10b | each entry of `secrets` (key 22), in the order of ref/02 §3.4.3: signature of the `secrets` role, `wrap` for this node, test decryption, `rev` ≥ the stored one; an entry that fails is rejected on its own (`security` event), the others are stored, and a rotated secret does not create a generation | `J0922`–`J0926` | yes |
| V11 | all `media_type`s known and supported in this version | `J0957` | yes |
| V12 | `artifacts` consistent: no duplicates; `runtime.artifact`, `generation.config` and the artifacts of the `mcus` present and with the right type | `J0949` | yes |
| V13 | `confirm`, `schedule` and `window` valid | `J0979` | yes |
| V14 | runtime: annotations of the OCI manifest (architecture, board, `state_min`) | `J0948` | yes |
| V15 | runtime and firmwares: downgrade only with `allow_downgrade`; never below `min_version` nor to a `revoked` digest | `J0947` | yes |
| V16 | space: sum of the `size` of the missing artifacts + reserve (max(10% of the disk, 256 MiB)) ≤ free | `J0953` | yes |
| V17 | **download** (state `fetching`): the `joker.config.v1` first; then the others, at most 2 in parallel; each with size and digest (`J0950`) and signature of the right role (`J0951`) | `J0950`, `J0951`, `J0952` | yes |
| V18 | **staging**: the digests of the lock, those of the `mcus` and those of `artifacts` match (except runtime and configuration); each `mcu-target.device` exists with the `joker-link` driver | `J0949` | yes |
| V19 | the `deck` origin is in `[policy] apply_from` (ref/02 §3.5.1); `mode` of the generation ≥ `min_mode` of the bundle and, if it is `dev`, `[policy] allow_dev = true`; in `prod`, all artifacts signed | `J0992`, `J0956`, `J0906`, `J0951` | yes |
| V20 | the validation of `jk check`, with the same crates, for the **target** runtime (the new one, if there is a `runtime` phase), including the `[security] trust` of the manifest (§2.4.3) | `J0954` + causes | yes |
| V21 | computed generation hash = `generation.hash` | `J0955` | yes |
| V22 | WIT world of each app accepted by the target runtime (`org.joker.worlds`; M21; the cause is the `J-E0001` of ref/04) and, on a `prod` node, no app with unstable features (key 10 of `joker:meta`; cause `J-E0003`) | `J0958` + cause | yes |
| V23 | MCUs: board, ABI and counter of the firmware; the blob fits the capabilities of the target firmware (`J0710` as cause) | `J0963`, `J0964` | yes |
| V24 | AOT precompilation of the apps for the target runtime (`.cwasm`) | `J0954` + cause | yes |

A DSD that fails at V1–V8 **does not exist** for the node: it changes nothing, generates a `security` event and the alarm `joker.agent.refused`. One that fails at V9–V24 becomes `refused` (V9–V16, V18–V24) or `failed` (V17), with the code and the causes in the report; the node stays exactly as it was. V10b is the exception: a rejected secret entry does not reject the DSD (the generation only fails, at V20, if it needs it).

The configuration artifact comes first because it is the one that says the `mode` (V19) and the `[security] trust` (V20), which decide the signatures of the others.

### 3.4 Apply

#### 3.4.1 State machine

```
             V1–V8                V9–V16          V17              V18–V24
 (received) ───────▶ accepted ──▶ validating ──▶ fetching ──▶ staging ──▶ waiting ──▶ applying ──▶ confirming ──▶ confirmed
     │                  │            │             │             │           │            ▲               │
     └─ refusal         └─ noop      └─ refused    └─ failed     └─ refused  │            └── next phase ────┤
        (does not exist)                                                    └─ superseded                  └─ rolled_back
```

`applying` and `confirming` repeat per phase, in the order **runtime → mcu → generation**. Each confirmed phase stays confirmed: if the `generation` phase rolls back, the new runtime and the MCUs already confirmed stay; the final state is `rolled_back`, with the phase in the event. A phase without changes (target equal to current) is skipped.

**Why this order.** The new runtime starts first with the **current** generation (the golden rule of 1.x guarantees that it reads it), and only then receives the new one, which may need it (V20 and V22 validate against the target runtime). The MCUs come before the generation of the node because the lock of the node pins what each MCU exports in the `HELLO` (document 19, §6): the new generation of the node may depend on the new firmware, and never the other way around.

#### 3.4.2 Concurrency between DSDs

- A new accepted DSD replaces (`superseded`) the previous one if the latter has not yet started swapping (`accepted` to `waiting`). The artifacts already downloaded stay in the cache by digest.
- If the previous one is in `applying` or `confirming`, the new one goes up to `staging` and waits (`wait = confirmation`) for the phase in progress to end. At that moment, what is left of the previous one is abandoned (`superseded`) and the plan of the new one is recomputed against the real state.
- **A rollback never waits:** it does not count toward `max_applies_per_day`, and it ignores windows, pauses and `hold`.
- **The same mutex as the API.** The agent and the local API (ref/09 §5.13) share the generation-transition mutex: at most one `pending` generation and one `confirming` per node, wherever it comes from. A swap of the agent waits (`wait = confirmation`) for a local transition in progress; a `POST /v1/generations` during a swap of the agent receives `J-E1608`.

#### 3.4.3 Phases

| Phase | Swap | Health window | Rollback |
|---|---|---|---|
| `runtime` | binary in the inactive slot; `jokerd` restarts in the new slot **with the current confirmed generation**; the equipment covers the interval with the fallback (M1) and the MCUs with their lease | `health_window` + `confirm` | restart in the previous slot; also automatic if the new `jokerd` does not start (systemd, document 01, §16) |
| `mcu` | §3.6 | `mcu-target.confirm` | the MCU reverts to the previous image |
| `generation` | the pipeline of document 01, §15: plan the diff, prepare, swap at a tick boundary of each affected group | `confirm` | swap back to the previous generation, kept warm (`keep_previous`) |

Numbering of generations: the integer `gen` is assigned by the node when the generation enters staging. A rollback **reactivates** the previous generation with the same hash and **the same number** (D9); the one that failed stays in the history with its own, in `rolled_back`. The history is of **activations**, each with its `origin` (`gen-origin`, §2.2: `agent` with the `seq` of the DSD, `rollback`, …), which the API shows in `GET /v1/generations` (ref/09 §5.13).

#### 3.4.4 Waiting before swapping

A phase only swaps when all the conditions hold; otherwise it stays in `waiting` with the reason (`J0959`):

| Reason | Condition to proceed |
|---|---|
| `not_before` | now ≥ `schedule.not_before` |
| `window` | now is within a window of the DSD **and** a local window (`[agent] maintenance`); with no windows on one side, that side does not restrict. An empty intersection in the next 7 days is reported at once |
| `jitter` | the drawn delay has passed (counted from the first time the other conditions held) |
| `clock` | there are `windows` or `not_before` and the clock is `unsynced` (with `holdover` it proceeds) |
| `paused` | `jk agent pause` active |
| `local_hold` | §3.7.1 |
| `confirmation` | another phase being confirmed |
| `rate_limit` | `max_applies_per_day` reached (`J0962`) |
| `mcu_link` | the MCU of the phase is not `up` |

Download and staging do not wait for any of this: they are done at once, so that the swap, when the window opens, takes seconds.

### 3.5 Confirmation and health

The confirmation window starts at the swap. The health checks:

| # | Check | If it fails |
|---|---|---|
| H1 | no cycle group in `overrun`, `tripped` or `stopped` | restarts `stable_for` |
| H2 | no app in `crashloop` or `failed` | **immediate rollback** (`J0961`) |
| H3 | no new trip since the swap | **immediate rollback** (`J0961`) |
| H4 | each device that was `up` before the swap is `up` or `degraded`; each new device reaches `up` or `degraded` | restarts `stable_for` |
| H5 | no new active alarm with severity ≥ `alarm_severity` | restarts `stable_for` |
| H6 | only in the `runtime` phase: the first tick of each group within `health_window` and the drivers connected (document 01, §16) | rollback at the end of `health_window` |
| H7 | `require_uplink`: after the swap, at least one complete authenticated exchange with the Deck (a `report-ack`) | prevents `auto` confirmation |

| `mode` | Confirms when | Rollback when |
|---|---|---|
| `auto` | H1–H6 continuously for `stable_for` **and** H7 | H2, H3 (immediate); end of the window without confirming (`J0960`, or `J0961` if health was failing) |
| `deck` | a `confirm` command arrives with the right (`seq`, phase, hash); the node requires H1–H6 at that moment, otherwise it responds `conflict` | H2, H3; `reject` (`J0969`); end of the window (`J0960`) |
| `manual` | `jk confirm` locally or in a remote session | H2, H3; `jk rollback`; end of the window (`J0960`) |

H7 is what makes `auto` an honest confirmation in the sense of ADR 0015: a generation that cut the network off from the node cannot confirm itself.

### 3.6 MCUs behind the node

For each `mcu-target`, by `order`, **one at a time**:

1. The MCU must be `up` on Joker Link (otherwise `wait = mcu_link`; at the end of `fetch_deadline`, `J0965`).
2. The node has already verified the signature (`firmware` or `config` role); the MCU verifies it **again** with its own keys (the bootloader and `joker-micro` for the firmware, `jk_generation_open` for the blob; ref/08 §2.12.4, §2.13.5). The MCU is the final authority over what it boots.
3. **Firmware:** the node sends the image through the `OTA` message (ref/07); the MCU writes it to the inactive slot and restarts in test mode (MCUboot *test swap* or equivalent). During the restart (milliseconds), the outputs of the MCU are in the `safe` of the MCU itself.
4. The node waits for the `HELLO` with the new digest (key 5.2; and 5.9, that of the descriptor) and the firmware on trial (12.4) within `confirm.window`, and then `stable_for` with the link `up`, the generation of the MCU running and no new trips on the MCU. Then it confirms through the `OTA` (the MCU marks the image as permanent). If not, the MCU reverts at the next restart, which the node triggers (`J0965`).
5. **Generation blob:** the same path, with the `joker.generation.v1` blob (ref/08 §2.13), hot-swapped (`jk_generation_swap`) with the window of the `OTA` `activate` (`confirm_ms`) and anti-rollback by `gen_seq` (ref/07 §9.4–§9.5). The next `HELLO` shows the `gen_seq`, the hash and the confirmation state (keys 6 and 12.7).
6. A failure stops the phase: the following MCUs are left un-updated and the previous ones stay confirmed.

The state of each MCU appears in `report-body.mcus`, and each step generates an `apply` event with `phase = mcu` and `device`.

### 3.7 Local changes, pause and import

#### 3.7.1 Local changes (local-first)

The local `jk apply`, `jk rollback` and `jk runtime upgrade` always work, with the `admin` role. When a person changes the generation or the runtime locally (`origin = local` or `session`):

| `[agent] local_changes` | Effect |
|---|---|
| `hold` (default) | the node keeps accepting, downloading and preparing DSDs, but **does not swap** (`wait = local_hold`, `J0966`, alarm `joker.agent.hold`). The wait ends with `jk agent resume`, with a DSD with `override_local = true`, or with a DSD whose generation and runtime are equal to the local ones (Git caught up with the change) |
| `overwrite` | the next DSD is applied normally |

The default protects the technician who fixed something on site at 3 in the morning from seeing the fix undone by the Deck at 3:05. The report shows the drift (`drift`), and the Deck decides.

#### 3.7.2 Pause

`jk agent pause --ttl <d> --reason "…"` suspends the swaps (not the downloads, the reports, the events or the rollbacks) until `ttl` (≤ 7 d) or `jk agent resume`. Event `agent` `paused`/`resumed`. It serves commissioning: the Deck swaps nothing while someone is working on the panel.

#### 3.7.3 Import (sites without network)

`jk agent import <dir>` applies a DSD brought by hand. The directory has `desired.cose` and an *OCI image layout* (`oci-layout`, `index.json`, `blobs/sha256/…`) with the artifacts. The processing is that of §3.3 and §3.4, with the directory as the first source and `origin = import`. The `seq` rules apply: a DSD imported and then received over the network is not applied twice. Since there is no Deck, H7 cannot be met: in an import, `mode = auto` with `require_uplink = true` becomes `manual`, and the person confirms with `jk confirm`.

### 3.8 Reported state

#### 3.8.1 Full and delta

| A full report is sent | A delta is sent |
|---|---|
| after the `hello` with `need_full = true` or with a new `boot` | at each change, aggregated during `report_min_interval` |
| after each phase transition of an apply | at each `heartbeat` if nothing changed (only `resources` and `agent`) |
| at each `full_interval`, with an offset of `hash(node) mod full_interval` to spread the fleet | — |
| with the `report_full` command | — |
| when a delta would exceed 32 KiB | — |

**Deltas.** The node keeps the last **acknowledged** report (`report-ack`). A delta is the merge patch (RFC 7386) from that report to the current state, with `base` = its `seq`. If the ack is lost, the next delta still has the same `base` and includes the previous changes: a repeated or lost delta never leaves the Deck in a wrong state. The Deck applies the delta if the `seq` it has stored is the `base`; otherwise it responds `need_full`.

#### 3.8.2 Frequency and cost

| Scenario | Full | Typical delta | Monthly volume (agent, without telemetry) |
|---|---|---|---|
| heat pump (document 08, round 5), JSON over HTTPS | ~3 KiB | ~200 B | ~2 MB (delta every 5 min, full every 6 h) |
| same, CBOR over MQTT, `heartbeat = 30m` | ~1.2 KiB | ~90 B | ~0.5 MB |
| BESS with 50 devices and 200 instances, JSON | ~25 KiB | ~1 KiB | ~15 MB |
| Joker Micro, CBOR | ≤ 1 KiB | ~60 B | ~0.2 MB |

The bytes of the agent count toward the uplink budget of the same connection (M12, document 01, §13): `alarm`, `security` and `apply` events in the `alarms` class; reports and other events in the `events` class; artifacts and uploads in the `bulk` class. Under budget pressure, the node widens `report_min_interval` up to 15 min and `full_interval` up to 24 h, and never delays the `alarms` classes.

#### 3.8.3 Truncation

A full report above 256 KiB is cut, in this order, until it fits: `devices` in `up`, `instances` in `ok`, `apps` in `running`, `groups` in `ok`, `alarms` of severity `low`. The keys cut go in `truncated`, and the node logs `J0971`. What is wrong is never cut before what is fine.

### 3.9 Events

#### 3.9.1 Production

Each event receives its `seq` when it is written to the local log (joker-store). Those of class A are written with `fsync = "always"` (like trips and the critical entries of the decision log, document 01, §12 and §19); those of classes B and C in a batch (1 s). In a power outage at most 1 s of B and C events is lost, which never got a persisted `seq` and therefore leave no hole.

#### 3.9.2 Delivery

- **At least once**, in `seq` order, in batches of up to `event_batch`.
- The Deck responds with the **cumulative ack**: the highest `seq` S such that all events ≤ S are durably stored in the Deck or declared lost by a `gap`. The node deletes from the queue everything that is ≤ S.
- Without an ack within 30 s, or after a reconnection, the node resends from S + 1. The Deck deduplicates by (node, `seq`); the `id` (ULID) deduplicates across systems (for example, after exporting to another system).
- Over MQTT, the PUBACK of the broker **is not** the ack: only the `ack/events` signed by the Deck deletes events. A repeated old ack only confirms what was already confirmed; an ack above the `event_seq` of the node is ignored.
- After a reconnection, the full report goes **before** the accumulated queue: the Deck knows the current state before digesting the past.

#### 3.9.3 Queue and what is lost

The queue is a cursor over the local log, with the ceiling `queue.max_disk`. When it is full, the node discards, in this order: the oldest C event; the oldest B; the oldest A. Each discard:

- keeps the `seq` of the event (the Deck sees the hole);
- goes into a `gap` event (never discarded; consecutive `gap`s merge, up to 64 ranges) with the ranges and the counts per type;
- increments `joker.agent.dropped` and raises `joker.agent.queue_overflow` (`J0970`).

**What is lost is the sending, not the record.** Decisions, alarms and trips stay in the local log, subject to local retention (document 01, §12), and the Deck can request them with the `upload` command after seeing the `gap`. With 64 MiB and ~200 B per event, the queue holds ~300,000 events: months of a normal site offline.

In the micro profile, the queue is a ring of 32 events in RAM (plus flash, if the firmware gives it room), with the same discard order and the same `gap`.

#### 3.9.4 Draining the queue

After a long time offline, the node sends the queue at `drain_rate` events/s (default 50/s), within the uplink budget. A Deck that needs to breathe responds `rate_limited`.

### 3.10 Remote session (agent/1.3)

**Opening.**

1. A person requests the session in the Deck. The authentication of the person (and the MFA) belongs to the Deck.
2. The Deck sends the `session_open` command, signed.
3. The node checks, in order: it supports 1.3; `[agent.remote] allow` (`J0980`); the transport has HTTPS (`J0985`); `max_sessions` (`J0982`); `ttl` ≤ `max_ttl` (`J0983`); the **role** (below, `J0981`).
4. If `consent = "local"`: it raises `joker.agent.session_request` and waits for `jk session accept <id>` (role `operator`, on the local socket) or for `consent_signal` to be true, up to `consent_timeout` (`J0984`).
5. The node opens the tunnel (`wss://`, mTLS) to the URL of the command, which must be in one of the `deck_urls`.
6. Event `session` `opened`; alarm `joker.agent.session`.

**The role belongs to the node.** The effective role is the lowest of three: the requested one, `max_role`, and what the `[security.roles]` of the manifest gives the principal. Session principals are written `deck:<deck_id>/user:<id>` and `deck:<deck_id>/group:<group>` (for example `operator = ["deck:deck-prod/group:ops-porto"]`). With no matching entry, `default_role` applies; empty, it refuses. A request above what is allowed is refused (`J0981`), and not silently reduced: whoever asked knows what they have.

**Where it ends.** Each stream of the tunnel ends at the local API, which applies the roles, the envelope, the arbiter and the TTL of manual writes (A10), as for any client. The tunnel leads to nothing else: no shell, no port forwarding to field equipment, no other node.

**Forbidden in sessions, with any role** (`J0986`): the identity and trust endpoints (`/v1/node/*`), changes to the agent itself (`/v1/agent/*` except the reads: `pause`, `resume`, `import`, accepting sessions) and secrets (`/v1/secrets`). The Deck cannot use a session to loosen the leash by which the node holds it. The list is that of ref/09 §2.3.

**End.** By `ttl`, by inactivity (`idle`), by `session_close`, by a local `jk session close`, by a restart of `jokerd` (sessions do not survive), or by revocation of the certificate. Event `session` `closed`/`expired` with the counts of requests and writes.

**Audit.** Each request in the tunnel enters the local audit log of the API (ref/09 §4) with the `id` of the session and the principal `deck:<deck_id>/user:<id>`; `GET /v1/audit?session=<id>` shows them. Each write enters the decision log with `writer = user:deck:<deck_id>/user:<id>` and `session`, and is always sent to the Deck (§2.11).

**Limits.** `max_rate` per session (token bucket), 16 streams, initial credit of 256 KiB per stream, `ping` every 30 s.

### 3.11 Transports

#### 3.11.1 HTTPS and long-poll

```
node                                         Deck
 │── TLS 1.3 (mTLS; deck_tls pins) ─────────▶│
 │── POST /hello ───────────────────────────▶│
 │◀─ hello-reply {need_full, event_ack} ─────│
 │── POST /report (full, if requested) ─────▶│
 │── POST /events (queue, in order) ────────▶│
 │── GET /poll?desired=418&cursor=c&wait=55s▶│  the Deck holds the request
 │◀─ poll-reply {desired?, commands?} ───────│  as soon as there is a DSD with seq > 418, commands, or at the end of wait
 │── GET /poll … (immediately) ─────────────▶│
```

- Reports and events go in separate requests on the **same** HTTP/2 connection.
- The poll is also the sign of life of the node to the Deck.
- With `poll_interval` > 0 (LTE), the node makes short polls (`wait = 0`) at that interval, and sends deltas and events when it has them. A `critical` alarm triggers an immediate poll.

#### 3.11.2 MQTT

1. `CONNECT` with `clean start = false`, the signed `status` *will* (`online = false`) and the certificate.
2. `SUBSCRIBE` to the Deck → node topics. The retained DSD arrives at once.
3. `PUBLISH` of the `hello`, after the `status` (`online = true`).
4. The Deck answers on `hello/reply`, and the rest follows as over HTTPS, with the acks on the `ack/*` topics.

**Everything is signed end to end** (§2.14): the broker is transport, not trust. A compromised broker can delay or throw away messages, and cannot forge a report, an event, an ack or a DSD. Replaying old messages is harmless: `seq` in reports and events, and acks that only confirm what exists.

#### 3.11.3 Artifacts

The node downloads by digest with the OCI distribution API (`GET /v2/<name>/blobs/<digest>`, with `Range` to resume), with the token from `POST /registry-token` when `auth = token`. Signatures are looked up through the *referrers* API or through the digests in `artifact.signatures`. A local cache by digest avoids repeated downloads; a partial blob is kept for 7 d to be resumed by a following DSD. Each source is tried in order (local mirrors, then those of the DSD), and each artifact is retried with a backoff of 30 s to 1 h until `fetch_deadline`.

#### 3.11.4 Reconnection and backoff

- Delay of attempt *n*: uniform random in [0, min(`backoff_max`, 1 s × 2ⁿ)] (*full jitter*). It goes back to *n* = 0 after a connection that lasted more than 60 s.
- `Retry-After`, `retry_after` and `poll-reply.backoff` are minimums: the node waits the larger of that and its own.
- Three consecutive failures on a URL move to the next one of `deck_urls` (or of `mqtt`); after the last, it goes back to the first.
- A TLS error due to a pin (`J0975`) is not retried more than once an hour: it is almost always configuration, not network.

#### 3.11.5 Months offline

| What happens | How it is resolved |
|---|---|
| the node controls in the last confirmed state | by design (principle 1) |
| a DSD in `waiting` keeps waiting | it is applied when the window opens, if it is still the latest |
| the certificate expires | `POST /renew` with the expired certificate, up to `renew_grace` (§3.2.1) |
| the bundle fell behind (`deck` keys rotated) | `hello-reply.trust` carries the chain; the node verifies version by version before verifying the DSD |
| the event queue fills | discard with `gap` (§3.9.3); the local log stays |
| the clock drifted | §3.12 |
| the runtime of the node has already left the support of the Deck | minimal core (§3.14) |

The Deck **does not keep a queue of DSDs**: it keeps the latest. The node that comes back receives today's desired state, and not the 26 weekly versions it went through. That is what makes the return after 6 months a single apply.

### 3.12 Clocks

1. **The node never refuses anything from the Deck on time grounds with the clock not `synced`.** Freshness comes from the `seq` (DSD) and from the binding to the target (commands). With the clock `synced`, `expires`, `issued` and the ±30 s window of commands apply (ref/00, §9).
2. **TLS in 1970.** With the clock `unsynced`, the node ignores the validity dates of the certificate of the Deck and accepts it only if the SPKI is in `deck_tls`. Without pins, the node waits for the clock before talking to the Deck (and that shows in `jk doctor`). That is why production bundles should have pins.
3. **Deck time.** Each `hello-reply` and `poll-reply` carries `time`. The node computes the offset (with half the RTT) and shows it in `jk agent status`. The node uses it as a coarse clock source (ref/02), with quality at most `holdover`, never `synced`.
4. **On the Deck side**, a report or event is never refused because of the offset. The Deck computes it, flags it above 5 min (`J0972`) and corrects the times of the events with the pair (`boot`, `mono`) and the `clock` events, which the node emits when the clock changes quality or jumps (it is the monotonic ↔ UTC correspondence of document 01, §18, sent outside).
5. Maintenance windows require `synced` or `holdover`; with `unsynced`, the phase waits (`wait = clock`).

### 3.13 Micro profile: behavior

- Same validation rules (V1–V24, with the subset of §2.15) and the same state machine, with the phases `runtime` (the firmware) and `generation` (the blob).
- **Firmware:** written to the inactive slot; restart in test mode; confirmed by the MCU itself after `stable_for` with H1–H5 and, if `require_uplink`, a `report-ack` from the Deck. Without confirmation, the bootloader reverts at the next restart (the watchdog guarantees that it happens).
- **Generation blob:** written to the inactive generation area and hot-swapped at a tick boundary (`jk_generation_swap`, ref/08 §2.12.4), with the same confirmation; without it, the MCU goes back to the previous generation on its own. Trips persist in flash across restarts.
- The MCU stores in flash: the bundle, the floors, the last `seq` accepted, the `seq` of the reports and of the events, and the state of the apply in progress.
- Reports: full at boot and at each `full_interval`; deltas aggregated for 30 s at least.
- No remote sessions, no `upload`, no MCUs behind.

### 3.14 Versions and compatibility

**Negotiation.** The node sends `proto = [1, N]` in the `hello`; the Deck answers with `proto_minor = min(N, M)`. Messages and keys of a minor above the negotiated one are not sent.

**Minimal core.** `hello`, `poll` and `desired`, `report`, `events`, `renew`, and the `confirm`, `reject` and `report_full` commands, in the `agent/1.0` form, **never change in the 1 series** (ADR 0022, decision 1: the agent protocol does not break within a 1.x). Every Deck of the 1 series accepts the minimal core of any 1.x node.

**Support.** A Deck fully supports nodes of the **last three minors and the LTS** (ADR 0022, decision 5): all features, tested. An older node only receives the minimal core (`hello-reply.code = J0973`, warning in the Deck), which is enough to send it a DSD with a new runtime. A node is never left without a path to be updated.

**What a minor may add** (ref/00, §1): optional keys, enum variants at the end, commands, event types, endpoints and topics. A new key in the DSD that an old node **must not** ignore goes in `crit`; the old node rejects the DSD (`J0946`) and the Deck knows, from the `proto_minor`, that it should not send it.

| Minor | Adds |
|---|---|
| `agent/1.0` | everything that this document does not mark with another version |
| `agent/1.1` | type `joker.model.v1+jkm` and role `models`; key 23 of the DSD; `report-body` 20 (models, shadow) |
| `agent/1.3` | remote sessions (commands 5 and 6, tunnel, `session` events); `report-body` 19 (pending intents); TPM attestation requirable at enrollment |
| `agent/1.5` | nothing mandatory; the Deck aggregates audit from `upload` and from the `decision` events |

**v2.** The v2 protocol describes **sites** (several nodes and MCUs as one system; document 21, §8). Reserved, without semantics:

- the identifier `agent/2`, the HTTPS prefix `/agent/v2`, the MQTT prefix `jk/2/` and the CoAP resource `/a2/`;
- keys 100–119 of `desired-state` and of `report-body` (in particular: 100 `site`, 101 `members`, 102 `placement`, 103 `site_generation`);
- the first segment `@` in paths (ref/00, §2), for paths qualified by node in reports and events.

A Deck of the 2 series speaks `agent/1` with 1.x nodes for the whole series (document 21, §1.1).

---

## 4. Stress

**S1 — The Deck is compromised.** The attacker has the database, the issuing CA and the `deck` key.

| Can | Cannot |
|---|---|
| stop delivering updates (freeze the fleet) | make a node run an artifact that is not signed by a pipeline key of the bundle (principle 5) |
| issue DSDs with **old but valid** artifacts | go below a floor, nor to an older runtime without `allow_downgrade`, nor to a revoked digest (V15) |
| make swaps at bad hours, within the **local** windows of the node | swap outside `[agent] maintenance`, nor more than `max_applies_per_day` times |
| confirm or reject generations being confirmed | prevent the automatic rollback of a generation that fails health |
| open sessions with invented users, with the role that the manifest gives the `deck:` principals | have more than that role; touch identity, trust or the agent; write without TTL or outside the envelope |
| read reports, events and uploads (loss of confidentiality) | read secrets (they never go through the protocol) |
| issue certificates for node names | impersonate the Deck to the nodes without the SPKI of the pins; change the `authority` |
| change the `[security] trust` of the manifest | — the manifest is signed by the `config` key, which is not in the Deck |

**Fixes incorporated:** the local maintenance window and the limit of swaps per day (without them, a compromised Deck could restart 3,000 `jokerd`s every minute, alternating between two valid runtimes, and put all the equipment in fallback); the prohibition of changing the agent and trust inside sessions; the `[security] trust` of the manifest that only restricts. **Residue:** the freeze and the choice of old versions above the floor. Operational mitigation: floors published by the pipeline at each security release, and an alarm in the pipeline if a node does not report the new version within N days. **Holds.**

**S2 — Rollout of 3,000 nodes** (document 08, round 5). The Deck issues DSDs in waves (1% → 10% → 100%), with `windows` from 2:00 to 5:00 and `jitter = "45m"`. The nodes download at once (2.8 MB of app × 3,000 = 8.4 GB, served by the registry or by a CDN, not by the Deck), and swap within the window, spread over 45 min. Load on the Deck: 3,000 open long-polls (~55 requests/s in steady state), full reports after each phase (~9,000 in 3 h, ~1/s). The first wave confirms in `auto` with `stable_for = "5m"`; the Deck sees the `apply` events and the KPIs (document 20, D2), and, if the stop rule fires, **simply does not issue** the DSDs of the following waves, and issues for the first wave a DSD with the previous generation (`seq` + 1). The nodes being confirmed receive `reject`. The 300 gateways on LTE (50 MB/month) receive the same DSD, but the 2.8 MB download goes in the `bulk` class with `fetch_deadline = "7d"`: it uses 6% of the monthly budget and never delays an alarm. **Finding:** without a long `fetch_deadline` and the `bulk` class, the monthly OTA ate the budget of the alarms. **Fixed** (§3.8.2). **Holds**, and the exit gate of 1.5 (99.9% at the desired state within 24 h) depends on the windows, and not on the protocol.

**S3 — Node offline for 6 months.** It comes back with runtime 1.0.3 and the Deck is already installing 1.2. The certificate (365 d) is still valid; if it were not, `POST /renew` with the expired one. The `hello` says `trust_version = 3`; the `hello-reply` carries versions 4 and 5 (a rotation of the `deck` key), which the node verifies in order. The Deck delivers **today's DSD** (runtime 1.2 + today's generation), and not the 26 intermediate ones. The node applies the `runtime` phase (1.2 reads the generation of 1.0, by the golden rule), confirms, and then the `generation`. The queue had 64 MiB of events; the full report goes first, the queue next at 50/s (~100 min for 300,000 events), with a `gap` if it filled up. The events with a `holdover` clock are corrected by the Deck with the `clock` events. **Holds.**

**S4 — Two Decks fighting over the same node.** (a) A Deck of a former integrator keeps issuing DSDs for a node that changed Deck: its key is no longer the `authority` (V3, `J0941`), `security` event and alarm. (b) A high-availability Deck enters *split-brain* and the two halves issue `seq = 512` with different contents: the node accepts the first that arrives and rejects the second (`J0943`), and the event tells the Deck that it has two issuers. (c) The two halves issue 512 and 513: the node applies 513, because the `seq` is the order, and the `superseded` of 512 appears in the report. **Finding:** without `J0943`, the node silently applied the DSD of whoever won the race. **Fix:** rule V7 and the recommendation to the Deck to derive the `seq` from a single counter (a sequence in the database, never per instance). **Holds.**

**S5 — Desired state with an artifact that the node cannot download.** The registry deleted the tag, the CDN returns 403, or the artifact was never published. The node tries the mirrors and the sources in order, with backoff per artifact, and reports `desired.state = fetching` with `fetched = [1.2 MB, 4.0 MB]`. Nothing is swapped: staging only starts with everything downloaded and verified, and the apply is all or nothing per phase. At the end of `fetch_deadline`, `failed` with `J0952` and the digest in `causes`. The node stays on the confirmed generation; the Deck fixes the publication and issues `seq` + 1 (the blobs already downloaded are in the cache). Variant: the artifact downloads, but the digest does not match (`J0950`) or the signature is not from a key with the role (`J0951`): `refused` at once, without waiting for the deadline, and a security alarm. **Holds.**

**S6 — The new generation cuts the network off from the node.** A wrong route in the manifest. The swap happens, the local health checks H1–H5 pass, but H7 is never met: there is no `report-ack`. At the end of `window`, rollback (`J0960`), the network comes back, and the `apply` `rolled_back` event reaches the Deck. **Holds**, and it is exactly ADR 0015. **Finding:** the first version of `auto` only looked at local health, and confirmed a generation that had isolated the node. **Fixed** with H7 and `require_uplink = true` by default.

**S7 — Boot in 1970** (document 08, round 5 (d)). The armv7 gateway without RTC boots with the clock in 1970 and weak LTE. TLS with the Deck works through the pins (§3.12, 2); the DSD is accepted by its `seq`; `expires` is ignored; the phase with `windows` waits (`wait = clock`); the events go out with `clock = unsynced` and are corrected later by the `clock` event. **Finding:** without pins in the bundle, the node does not talk to the Deck until it has the time, and without the Deck it does not get the coarse time; it was stuck until NTP. **Fix:** the `deck_tls` pins and the time of the Deck as a coarse source (ref/02, §3.2.4). **Holds.**

**S8 — Cloned SD card.** A technician copies the card of a working RevPi to build a second one. The key was in a file. The clone connects with the same identity and another hardware fingerprint: quarantine (`J0937`). If both are connected at the same time, both are put in quarantine (`J0938`). Neither of them stops controlling. The solution is `jk node enroll --force` on the clone, which gives it an identity of its own. With a TPM, the clone cannot even do TLS. **Holds**, and the `J0933` on each node with a key in a file makes the risk visible.

**S9 — 50 MB per month on LTE.** Continuous long-poll over HTTPS: a poll every 55 s with resumed TLS costs ~1–2 KB (LTE NATs close idle connections), 47,000 per month, up to ~90 MB. **Broke.** **Fix:** `poll = "15m"` (2,900 polls, ~5 MB) with an immediate poll on critical alarms, or MQTT with `keepalive = "20m"` and CBOR (~1 MB per month including reports). `jk check` warns if a node with `[uplink] budget` uses continuous long-poll. **Holds with the fix.**

**S10 — The Deck is restored from yesterday's backup.** The `seq` counter of the Deck for the node is at 510, and the node has already accepted 514. With nothing else, all new DSDs would be `J0942` forever. **Fix incorporated:** the `hello` carries `desired_seq`, and the Deck starts issuing from max(its own, that of the node) + 1. The same for events: the `event_ack` of the Deck is below what the node has already deleted; the node resends what it still has, and the rest appears as a hole without a `gap`, which the Deck recovers with `upload` (`what = events`). **Holds.**

**S11 — 1.0 node and 1.5 Deck.** 1.0 has already left support (12 months). The Deck talks to it through the minimal core (`J0973` as a warning), does not send it a `crit` it does not know, and delivers it a DSD with runtime 1.5. The node would reject a DSD with a model (`J0957`), and the Deck knows it from `proto_minor = 0`. **Holds.**

**S12 — A technician runs `jk apply` on site in the middle of a rollout.** At 3:00 they fix a Modbus address; at 3:05 the Deck delivers wave 2. With `local_changes = "hold"`, the DSD is prepared, but not swapped (`J0966`); the report shows `origin = local` and the drift. When the fixed address reaches Git and the Deck issues a DSD with that generation, the hash matches and the wait ends on its own. **Finding:** without `hold`, the fix was undone within 5 minutes, and the technician fixed it again, and the Deck undid it again. **Holds.**

---

## 5. Decisions and rejected alternatives

**D1 — Complete and idempotent desired state, with `seq`.** Rejected: a queue of orders ("install X", "restart Y"), as in several device managers. A queue offline for months has to be replayed in order, and a lost or repeated order leaves the node in a state nobody chose. The complete state gives the return after 6 months in a single apply (S3).

**D2 — Two layers of signature.** The DSD is signed by the `deck` key (which lives in the Deck), and the artifacts by the pipeline keys (which do not). Rejected: signing the DSD in the pipeline (waves, windows and rollbacks would then need a signing ceremony); and not signing it at all (TLS does not protect when there is an MQTT broker in the middle, nor in an import on a USB stick).

**D3 — Signed documents always in CBOR/COSE, also over HTTPS.** Rejected: JWS in JSON. Two signature families and the JSON canonicalization problem; ref/00 already fixes COSE with deterministic CBOR.

**D4 — Long-poll and MQTT; no permanent control channel.** Rejected: gRPC (HTTP/2 with Protobuf, without a useful `no_std` and without a counterpart over MQTT), and a permanent WebSocket for control (industrial proxies cut it, and it would become the "control path" that principle 2 forbids). The WebSocket only exists for sessions, opened by the node, with a TTL.

**D5 — Deltas by merge patch (RFC 7386) over maps keyed by name.** Rejected: JSON Patch (RFC 6902), with paths by index and dependent on order. Merge patch works the same in CBOR and in JSON, and a lost delta breaks nothing (the base is always the last acknowledged one).

**D6 — Cumulative, application-level ack.** Rejected: ack per event (too chatty on LTE) and MQTT QoS 2 (it only guarantees delivery to the broker, and not to the Deck).

**D7 — Trust bundle with an offline root, floors per role: a short version of TUF.** Rejected: full TUF and Uptane. They are the right direction and the design is compatible with them (root, delegated roles, chained rotation, downgrade protection), but the volume of metadata and signatures does not fit an MCU with a 2 KiB DSD in v1.

**D8 — A single authority per node.** Rejected: several Decks with equal rights (resolving conflicts between them on the node is inventing a consensus protocol). The change of authority is explicit and signed by the root.

**D9 — No writes to signals in the protocol.** Remote writes only exist in sessions, through the local API. Rejected: a `write` command in the agent, which would be a second write path, with another authentication, another audit and another way to go wrong. A remote write is the same as a local one (AGENTS.md, invariant 3).

**D10 — Identity in P-256, renewal with a new key, and renewal with an expired certificate.** Rejected: Ed25519 for identity (TPMs, secure elements and mbedTLS do not do it in TLS), and EST (RFC 7030), which forces PKCS#7 and HTTP Basic authentication at enrollment; the semantics of `/enroll` and `/renew` are those of EST, without the formats.

**D11 — Two new artifact types: `joker.runtime.v1` and `joker.config.v1+cbor`.** Document 20, §3.2, had no way to distribute the Linux runtime or the configuration of a Linux node as an artifact; without them, the DSD had nowhere to point. The alternative (the runtime as a `.deb` package and the configuration as a Git commit to be downloaded by the node) put Git and APT in the path of each node.

**D12 — Phases in a fixed order (runtime → mcu → generation).** Rejected: an order declared in the DSD. The right order follows from the dependencies (§3.4.1), and a configurable order would only serve to choose it wrongly.

**D13 — CDDL with keys in the dual form `("name" / n)`.** Rejected: two separate schemas (JSON Schema and CDDL), which diverge at the first commit; and text-only keys in CBOR too (double the size on an MCU).

**D14 — LwM2M for the MCUs, rejected.** It would be a second object model next to the classes and the signals. The micro profile is the same protocol, with fewer keys.

---

## 6. Version markers

| Item | Version |
|---|---|
| enrollment, renewal, revocation, quarantine, bundle, floors | 1.0 |
| desired state, commands `confirm`, `reject`, `report_full`, `renew`, `upload` | 1.0 |
| reported state, events, queue with `gap` | 1.0 |
| HTTPS with long-poll, MQTT, CoAP (micro profile) | 1.0 |
| micro profile (standalone MCUs); management at scale by the Deck | 1.0; at scale, 1.5 |
| MCUs behind the node (`mcu` phase) | 1.0 |
| offline import (`jk agent import`) | 1.0 |
| **ML-ready:** type `joker.model.v1+jkm`, role `models`, key 23 of the DSD, `report-body` 20, capability `models` | reserved in 1.0; semantics in **1.1** |
| pending intents in the report (`report-body` 19) | reserved; 1.1 (M18) and approval in the Deck in 1.3 |
| remote sessions (commands 5 and 6, tunnel, `session` events, `[agent.remote]`) | **1.3** (defined now, refused with `unsupported` before) |
| TPM attestation at enrollment | optional in 1.0; requirable by the Deck from 1.3 on |
| per-recipient secrets (`joker.secrets.v1`, key 22 of the DSD, `report-body` 21) | 1.0 (D6, ADR 0024) |
| sites (keys 100–119, `agent/2`, `jk/2/`, `/agent/v2`, `/a2/`) | reserved for **v2** |
