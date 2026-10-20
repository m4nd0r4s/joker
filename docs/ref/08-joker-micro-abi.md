# ref/08 — Joker Micro: C ABI, Rust crate and generation blob

Normative reference of Joker Micro as seen by whoever integrates it. It applies `ref/00` without repeating it and defers to document 16 in everything that is semantics. The machine-readable files are `spec/c/joker_node.h` (the fixed ABI), `spec/c/example/` (a generated header and an example program, compiled in CI) and `spec/cddl/joker-micro.cddl` (the CBOR structures). If this document and the header diverge, it is a bug to fix in both (ADR 0023, point 3).

---

## 1. Scope and sources

**Covers:**

1. the **C ABI** of library mode (`libjoker_node.a` + `joker_node.h` + `joker_project.h`): all functions, types, macros, call contexts, errors, memory, stack and what is generated per project;
2. the **Rust crate** `joker-micro` (crate mode, Embassy), the generated crate `joker-project` and the contract with the native backend of `joker-sdk`;
3. **firmware mode**: the generic firmware, the flash areas, boot, OTA and confirmation;
4. the **generation blob** (`*.jkgen`): header, sections, records, expression ISA, COSE signature, capabilities, `J0710` and determinism of `jk build`;
5. the **M1/M2 targets** and the RAM, flash and stack budgets;
6. the new codes `J0770`–`J0793` and `J-E2501`–`J-E2599`.

**Does not cover:** the Joker Link messages (`ref/07`: the ABI only exposes input and output bytes, §2.6.p), the manifest keys (`ref/01`, from which this document requests keys in §2.17), the board profiles (`ref/03`), the SDK API for app authors (`ref/04`) and the configuration of each driver (`ref/06`).

**Derives from:** 19 (all), 16 (all), 01 §3–4, §7–8, §15, §18, §20, 02 (all), 03 §6–§9, 04 §2–3, 17 §2, §4, §6, 20 §3, ADR 0007, 0015, 0020, 0022, and `ref/00`.

---

## 2. The reference

### 2.1 Concepts

| Concept | Definition |
|---|---|
| **node** (`jk_node_t`) | all the memory of one Joker Micro instance: registry, slots, images, arbiters, envelopes, expression state, app state, queues, trace. Static, of fixed size at build time. An MCU normally has one node; the ABI allows several. |
| **generation** (`jk_generation_t`) | the compiled blob of the project (§2.13), embedded in the library (`JK_GENERATION`) or read from flash. It is immutable and must stay mapped while the node uses it. |
| **external interface** | the set of identifiers that the integrator's code uses: signals, groups, writers, instances, members, alarms, parameters and links. It is **fixed at library build time** and bound **by name** to the registry of each generation in `jk_init`. A `JK_SIG_…` is an index of the interface, not a `SignalId`: that is why it stays valid when a new generation arrives through flash (§4, D2). |
| **external writer** | an app `kind = "extern"` of the manifest (§2.17): C code of the integrator that proposes writes with a level and a lease, like any app. |
| **external device** | a device `driver = "extern"`: its signals are fed (`jk_set_input`) and applied (`jk_get_output`) by the integrator. |
| **context** | where a function may be called (§2.4). |
| **delivery mode** | firmware, C library or Rust crate (19 §4). All three use the same core, the same blob and the same interface. |

### 2.2 Files: what is fixed and what is generated

| File | Fixed or generated | Content | Produced by |
|---|---|---|---|
| `joker_node.h` | **fixed** (changes only with the ABI version) | types, constants, functions, generic macros | `spec/c/joker_node.h`, copied by `jk build --lib` |
| `joker_project.h` | **generated** per project and per build | `JK_SIG_…`, `JK_GROUP_…`, `JK_WRITER_…`, `JK_INST_…`, `JK_C_…`, `JK_ALARM_…`, `JK_PARAM_…`, `JK_LINK_…`, the node size, the interface hash, the measured stacks, `JK_GENERATION` | `jk build --target mcu:<board> --lib` |
| `libjoker_node.a` | generated | core, statically linked Rust apps, requested drivers, interface table, embedded generation (if any) | likewise |
| `firmware.jkfw` | generated | build descriptor (§2.14), CBOR | likewise, and in all modes |
| `<site>.jkgen` | generated | signed generation blob | `jk build --target mcu:<board> --generation --firmware <firmware.jkfw>` |

The integrator's code includes **only** `joker_project.h`, which includes `joker_node.h`. The fixed header compiles on its own (C11 and C++17), but `jk_node_t` is only complete with the generated one, and the `jk_init` macro requires it.

### 2.3 ABI version

- `JK_ABI_VERSION_MAJOR` and `JK_ABI_VERSION_MINOR` (`ref/00` §1); `JK_ABI_VERSION` = `(major << 16) | minor` (the form used in document 19); `jk_abi_version()` returns that of the linked library.
- **v1.0 = ABI 1.0.** Builds before 1.0 use major 0 and have no guarantees.
- **Compatibility rule** (ADR 0022, `ref/00` §1): a minor only adds functions, constants, flag bits and fields **at the end** of structures; no existing structure changes size for code compiled with an earlier minor, because the library knows the header's minor from the `abi_version` argument of `jk_init_checked` and only writes the fields that minor knows. A function never changes signature in a minor.
- `jk_init_checked` refuses (`JK_E_ABI`) a different major, a header minor above the library's, a `node_size` smaller than needed and an `iface_hash` different from the library's (0 skips this check).
- The generated header has a `_Static_assert` with the version it was generated for.

### 2.4 General rules

1. **No allocation.** No function allocates. All memory is in the integrator's `jk_node_t` and in the buffers it passes.
2. **No threads, no hidden global state.** The library creates no tasks or timers and has no mutable global variables (the embedded `JK_GENERATION` is `const`). The integrator defines two symbols: `jk_now_us` and `jk_fatal` (§2.6.b).
3. **Reentrant per node.** Two nodes never share state. On the same node, calls follow the context table.
4. **Time comes from outside** (ADR 0007). All functions on the deterministic path receive `now_us`. The library only reads `jk_now_us()` in `jk_link_poll` (reception timestamps and link timers) and at the end of `jk_tick` (duration, for statistics and for the overrun policy of the **following** ticks, recorded in the trace).
5. **Units:** a value is always expressed in the declared unit of the signal (`ref/00` §4.1). The ABI does not convert. The generated header shows the unit in a comment.
6. **Explicit quality:** inputs always carry quality; reads always return it (invariant 4 of `AGENTS.md`).
7. **Errors:** they never abort; they return `jk_status_t`. Only a broken internal invariant calls `jk_fatal` (§2.6.b).
8. **Synchronization only through `critical-section` and `portable-atomic`** (core invariant, `AGENTS.md` §2.2). The 64-bit `seq`s and the slot seqlocks (document 01, §3.2) use `portable_atomic::AtomicU64`, which on 32-bit targets without 64-bit atomics (Cortex-M33, Xtensa LX6/LX7, RISC-V `imac`) rests on a critical section of a few instructions; the core never uses executor primitives or locks. This is what keeps `[any]` bounded (§2.4.1). **In library mode, the integrator links a `critical-section` implementation**: that of its RTOS (FreeRTOS, Zephyr, ESP-IDF) or that of its HAL, or enables the `cs-single-core` feature when the MCU has a single core and no interrupt calls the node (the critical section is disabling interrupts). With neither, the library does not link (the `critical-section` implementation symbol is missing). In firmware mode and in the crate, the implementation is that of the family's HAL (`chip-*`, §2.11.2).

#### 2.4.1 Call contexts

| Context | Who | Rule |
|---|---|---|
| `[any]` | any context, **including ISRs**, concurrently with everything | wait-free, bounded, stack ≤ `JK_PROJECT_STACK_ISR` |
| `[group g]` | the context that ticks group `g` | only one per group; different groups may preempt each other and run in parallel on another core (19 §3: one interrupt executor per group) |
| `[service]` | the node's service context | **only one** per node; it may be preempted by the groups and run in parallel with them |
| `[excl]` | exclusive | no `[group]` or `[service]` call on the same node at the same time; concurrent `[any]` calls (ISR) are allowed and return `JK_E_STATE` while the exclusive function lasts (except `jk_get_output*` during a `jk_generation_swap`, which return the previous state) |
| `[pure]` | no node state | any task context; not in an ISR (stack) |

In the single-loop pattern (19 §4.2), all calls are in the same context and there is nothing to coordinate.

**Misuse detection.** Each exclusive context (`[group g]`, `[service]`, `[excl]`) has an atomic busy flag. A call that finds its flag busy returns `JK_E_REENTRANT` and does nothing. Calling `jk_tick` of the same group from two tasks is therefore detected, and corrupts nothing.

**Single-writer rule per signal.** `jk_set_input*` is `[any]` **as long as** each signal is always written by the same context (document 01, §3.2). Two contexts writing the same signal is an integrator error that the library cannot detect without cost; the result is one of the two samples, never a torn sample.

**Wait-free readers.** A reader (latch, `jk_read`, `jk_get_output*`) never waits for a writer. If a read collides with a write in progress (an ISR that preempts the writer itself), it returns the **previous complete sample** and counts the collision (`latch_collisions`). This is what avoids the classic seqlock deadlock on a single-core MCU, in which the reader in a high-priority ISR would wait forever for a writer that cannot run (§4, D3).

#### 2.4.2 Table of functions by context

| Context | Functions |
|---|---|
| `[any]` | `jk_abi_version`, `jk_status_str`, `jk_set_input*`, `jk_read`, `jk_read_buf`, `jk_get_output*`, `jk_take_changed` (one consumer per group), `jk_group_output_count`, `jk_group_output_at`, `jk_group_period_us`, `jk_group_stats`, `jk_last_outcome`, `jk_alarm_info`, `jk_param_get`, `jk_time_utc`, `jk_link_status`, `jk_service_next_us`, `jk_supervise`, `jk_force_safe`, `jk_safe_get`, `jk_trace_freeze`, `jk_node_info` |
| `[group g]` | `jk_tick(node, g, …)` |
| `[service]` | `jk_service`, `jk_link_poll`, `jk_link_poll_at`, `jk_link_reset`, `jk_propose*`, `jk_relinquish`, `jk_manual_write`, `jk_manual_release`, `jk_call_method*`, `jk_action_*`, `jk_event_emit`, `jk_event_next`, `jk_alarm_ack`, `jk_alarm_shelve`, `jk_alarm_unshelve`, `jk_trip_reset`, `jk_param_set`, `jk_param_clear`, `jk_time_set_utc`, `jk_generation_confirm`, `jk_persist_flush`, `jk_trace_size`, `jk_trace_read`, `jk_ota_*` |
| `[excl]` | `jk_init` / `jk_init_checked`, `jk_restart`, `jk_shutdown`, `jk_generation_swap` |
| `[pure]` | `jk_generation_open`, `jk_generation_boot`, `jk_generation_info` |

The `jk_platform_t` callbacks are only called from `[excl]`, `[service]`, `jk_generation_open` (which reads the `IDENTITY` area) and `jk_generation_boot`, and they may block (ms). That is why these last two run in the service context or before `jk_init`. `jk_now_us` is called from `[group]` and `[service]`, and must be callable from any context.

### 2.5 Types and constants

#### 2.5.1 Identifiers

| Type | C | Width | Origin | Note |
|---|---|---|---|---|
| signal | `jk_sig_t` | `u32` | `JK_SIG_<PATH>` | interface index, not `SignalId` |
| group | `jk_group_t` | `u32` | `JK_GROUP_<NAME>` | ≤ 32 per node in the ABI; ≤ 8 in v1 (`J0776`) |
| external writer | `jk_writer_t` | `u32` | `JK_WRITER_<APP>` | |
| instance | `jk_inst_t` | `u32` | `JK_INST_<NAME>` | `ref/00` §5 |
| class member | `jk_member_t` | `u16` | `JK_C_<PACKAGE>_<CLASS>_<MAJOR>_<MEMBER>` | bound by (class, major, name) |
| alarm | `jk_alarm_t` | `u32` | `JK_ALARM_<NAME>` | |
| parameter | `jk_param_t` | `u32` | `JK_PARAM_<APP>_<NAME>` | app parameters; class `param`s are signals |
| link | `jk_link_id_t` | `u8` | `JK_LINK_<NAME>` | |
| action ticket | `jk_ticket_t` | `u32` | `jk_action_invoke` | |
| none | `JK_NONE` | `0xFFFFFFFF` | | |

#### 2.5.2 Enumerations

All with the values of `ref/00`: `jk_type_t` (`JK_TYPE_BOOL` 0 … `JK_TYPE_BYTES` 8; 16 and 17 reserved for block and stream, v1.1), quality (`JK_QC_*` 0–2, `JK_QR_*` 0–19, 240–255 private; `ref/00` §4.3), `jk_outcome_t` (`JK_OUTCOME_NONE` 0 … `JK_OUTCOME_TRIPPED` 9) and `jk_reject_t` (`JK_REJECT_WRONG_TYPE` 1 … `JK_REJECT_BUDGET` 8; `ref/00` §4.4), `jk_clock_q_t`, `jk_alarm_state_t`, `jk_severity_t`, `jk_role_t` (§6). Levels: `jk_level_t` `u8`, 1–16 absolute, `JK_LEVEL_NONE` = 0.

#### 2.5.3 Quality

`jk_quality_t` is the slot's `u32`: code in bits 0–7, reason in bits 8–15, flags in bits 16–31 (document 01, §3.2).

| Macro | Value |
|---|---|
| `JK_Q(code, reason)` | composes |
| `JK_GOOD` | `JK_Q(JK_QC_GOOD, JK_QR_NONE)` = 0 |
| `JK_UNCERTAIN(r)`, `JK_BAD(r)` | shortcuts |
| `JK_Q_CODE(q)`, `JK_Q_REASON(q)`, `JK_Q_FLAGS(q)`, `JK_Q_IS_GOOD(q)` | decompose |
| `JK_QF_TS_SRC_MONO` (bit 16) | the `ts_src` is local monotonic, not UTC |

On **input**, the code/reason combination must be one of those of document 02, §4 (for example, `Good` only with `none`, `overridden` or `local_override`; `stale` with `Uncertain` or `Bad`); reasons 240–255 are accepted with any code. Any other combination is `JK_E_QUALITY`. Undefined flags must be 0 on input and are ignored (cleared); on output they may come at 1 in future minors, and the integrator should use the macros.

#### 2.5.4 Value and samples

| Structure | Fields | Rules |
|---|---|---|
| `jk_value_t` | `type`, `as.b`, `as.i` (i64 and signed enum), `as.u` (u64, bits, unsigned enum), `as.f` (f64 and f32, promoted without loss), `as.buf.{ptr,len}` (UTF-8 string without NUL, bytes; `len` ≤ 64) | the `jk_reserved*` fields are 0; `buf` is borrowed only during the call |
| `jk_sample_in_t` | `value`, `quality`, `ts_acq_us` (required), `ts_hw_us` (or `JK_TS_HW_NONE`), `ts_src_us` (or `JK_TS_SRC_NONE`) | document 02, §5 |
| `jk_sample_t` | `value`, `quality` (already with `max_age` aging), `seq` (64 bits), `ts_acq_us`, `ts_hw_us`, `ts_src_us` | |
| `jk_output_t` | `value`, `quality`, `outcome`, `reject`, `level`, `flags` (`JK_OUTF_*`), `seq` | §2.6.h |
| `jk_outcome_info_t` | `outcome`, `reject`, `level`, `applied`, `tick_seq` | §2.6.i |

**Conversions accepted by the setters** (target signal → accepted setters). Everything else is `JK_E_TYPE`, without effect.

| Signal type | `_bool` | `_i64` | `_u64` | `_f64` | `_f32` | `_buf` |
|---|---|---|---|---|---|---|
| `bool` | yes | 0 or 1 (otherwise `JK_E_VALUE`) | 0 or 1 | — | — | — |
| `i64` | — | yes | if ≤ `INT64_MAX` | — | — | — |
| `u64` | — | if ≥ 0 | yes | — | — | — |
| `f64` | — | yes (rounded) | yes (rounded) | yes | yes | — |
| `f32` | — | yes (rounded) | yes (rounded) | yes (rounded to f32) | yes | — |
| `enum` | — | if it is a declared variant | likewise | — | — | — |
| `bits<N>` | — | if ≥ 0 and < 2ᴺ | if < 2ᴺ | — | — | — |
| `string` | — | — | — | — | — | valid UTF-8, ≤ 64 B |
| `bytes` | — | — | — | — | — | ≤ 64 B |

A value outside the domain (nonexistent variant, string with more than 64 B or invalid UTF-8) is `JK_E_VALUE`. A `NaN` or an infinity **is not an error**: the acquisition stage records it as `Bad(nan)` (document 16, §4). The output **getters** require the exact type, with two exceptions: `_f64` reads an `f32`, and `_i64`/`_u64` read an `enum` (numeric value) or a `bits` (`_u64`).

#### 2.5.5 Remaining types

All defined in the header; the `jk_reserved*` fields are 0 on input and ignored on output.

| Type | Where it is used | Section |
|---|---|---|
| `jk_generation_t`, `jk_generation_info_t`, `JK_GENF_*` | generation | §2.6.d |
| `jk_platform_t`, `jk_area_t` (`JK_AREA_*`), `jk_area_info_t`, `jk_notify_t` (`JK_NOTIFY_*`) | platform and flash | §2.6.c, §2.10 |
| `jk_group_stats_t` | group statistics | §2.6.f |
| `jk_action_err_t` (`JK_ACTION_*`), `jk_action_req_t` (`JK_ACTREQF_*`), `jk_ticket_t` | actions | §2.6.j |
| `jk_event_kind_t` (`JK_EV_*`), `jk_event_t` (`JK_EVF_*`) | events | §2.6.j |
| `jk_alarm_info_t` | alarms | §2.6.k |
| `jk_link_rx_fn`, `jk_link_tx_fn`, `jk_link_state_t` (`JK_LINK_*`), `jk_session_mode_t` (`JK_SESSION_*`, the `session_mode` of ref/00 §6), `jk_link_status_t` | link | §2.6.p |
| `jk_supervision_t` | supervision | §2.6.n |
| `jk_ota_kind_t` (`JK_OTA_*`) | local OTA | §2.6.r |
| `jk_node_state_t` (`JK_NODE_*`), `jk_node_info_t` | diagnostics | §2.6.s |

### 2.6 Functions

Each function: signature (that of the header), context, behavior, errors. The errors common to almost all of them are not repeated: `JK_E_ARG` (null pointer, argument out of range), `JK_E_STATE` (node not initialized or already stopped), `JK_E_HALTED` (node stopped by an internal failure), `JK_E_REENTRANT` (§2.4.1), `JK_E_UNSUPPORTED` (feature not in the build).

#### a. Utilities

| Function | Context | Behavior |
|---|---|---|
| `uint32_t jk_abi_version(void)` | `[any]` | version of the library, packed as `JK_ABI_VERSION` |
| `const char *jk_status_str(jk_status_t st)` | `[any]` | the registry code (`"J-E2512"`); `"ok"` for values ≥ 0; `"J-E2599"` for an unknown value. The string is static. The explanatory text is in `codes.toml` and appears with `jk explain` |

#### b. Symbols the integrator defines

| Symbol | Required | Rules |
|---|---|---|
| `uint64_t jk_now_us(void)` | yes (link error if missing) | monotonic µs since MCU boot (`ref/00` §3); never decreases; callable from any context, including ISRs; wait-free. A 32-bit timer must be extended by the integrator |
| `JK_NORETURN void jk_fatal(jk_status_t code, uint32_t detail)` | yes | called on a broken internal invariant or on a *panic* in the code of statically linked apps; **must not return** (MCU reset). There is no *sandbox* on the MCU: an app *panic* is fatal. The outputs stay in the hardware reset state, which the board declares safe (`J0786`) |

`JK_NORETURN` is `_Noreturn` in C and `[[noreturn]]` in C++.

#### c. Platform (`jk_platform_t`)

All fields are optional (`NULL`/0). The structure is read in `jk_init` and **must remain valid** while the node exists (the library keeps the pointer).

| Field | Signature | What for | Without it |
|---|---|---|---|
| `user` | `void *` | context passed to all callbacks and to link 0 | `NULL` |
| `area_info` | `(user, area, *info) → status` | size, erase and write granularity, erased value of each area (§2.10) | no area: no persistence, no OTA, embedded generation only |
| `area_read` / `area_write` / `area_erase` | `(user, area, offset, buf/data, len) → status` | blocking access to flash; `offset` and `len` respect `write_size`/`erase_size` | likewise |
| `area_map` | `(user, area) → const void *` | XIP address of an area: flash blobs are used in place | no flash generations managed by the library (`jk_generation_boot`, `jk_generation_swap(…, NULL, …)` give `JK_E_UNSUPPORTED`); the integrator can still read a blob into aligned RAM and use `jk_generation_open`. All v1 targets have mappable flash |
| `link_key` | `(user, link, key_out[32]) → status` | link key from a secure element | the key comes from the `JK_AREA_IDENTITY` area (record 17) |
| `kem_key` | `(user, key_out[32]) → status` | X25519 private secrets key from a secure element (standalone MCU, ref/02 §2.4) | the key comes from the `JK_AREA_IDENTITY` area (record 19) |
| `notify` | `(user, what, arg)` | notices from the service context (`JK_NOTIFY_*`, table below) | nothing |
| `trust_keys`, `trust_count` | `const uint8_t (*)[32]`, `u32` | the **anchors** (root Ed25519 keys, ref/02 §2.5.4): they sign trust bundles. Without a bundle in the `IDENTITY` area (record 18), they are valid for all roles | only embedded or `dev` generations |

| `JK_NOTIFY_*` | `arg` | When |
|---|---|---|
| `GEN_READY` (1) | low `gen_seq` | a generation received by OTA was verified and waits for `jk_generation_swap(node, NULL, …)` |
| `GEN_ROLLBACK` (2) | low `gen_seq` | the confirmation window expired; the node went back to the previous generation |
| `GEN_CONFIRMED` (3) | low `gen_seq` | confirmed |
| `FW_READY` (4) | length | firmware verified in `JK_AREA_FW_STAGING`; the integrator marks the bootloader and restarts when it wants |
| `TIME` (5) | `jk_clock_q_t` | the clock quality changed |
| `LINK` (6) | link | the state of a link changed |
| `TRACE_FROZEN` (7) | 0 | the trace ring froze (trip or request) |
| `RESTART` (8) | 0 | the peer requested a restart of the node (ref/07) |

#### d. Generation

| Function | Context | Behavior |
|---|---|---|
| `jk_generation_open(gen, blob, len, platform, min_seq)` | `[pure]` | validates in order: alignment of 8 (`JK_E_ALIGN`); header and section table (`JK_E_GEN_FORMAT`); format, ISA and known required sections (`JK_E_GEN_VERSION`); CRC-32C and SHA-256 of the image (`JK_E_GEN_INTEGRITY`); the bundle chain of the `TRUST` section, if any (signed by the anchors or by the installed bundle, with increasing `version`; `JK_E_GEN_TRUST`); if `SIGNED`, the COSE signature of the *trailer* (§2.13.5, `JK_E_GEN_SIGNATURE`) by a key with the `config` role in the effective bundle (`JK_E_GEN_TRUST`); `gen_seq ≥ min_seq` (`JK_E_GEN_ROLLBACK`); the binding to the node, if the claim has it (`JK_E_GEN_NODE`, comparing with the name in the `IDENTITY` area); `board_id`, apps, drivers and `min_runtime` against the build (`JK_E_GEN_FIRMWARE`); capabilities (`JK_E_GEN_CAPACITY`); the bytecode verifier (§2.13.4, `JK_E_GEN_FORMAT`). A blob without a signature is only accepted if it has the `DEV` flag and the build is not `prod`. It does not change any node. |
| `jk_generation_boot(gen, platform)` | `[pure]` | chooses and opens the boot generation from the `GEN_A`/`GEN_B` areas and from the generation state in `PERSIST` (§2.12.3, steps 4–5): the active area; the other one if the active one was `confirming` (a restart during the window counts as a failure) or does not validate; `min_seq` = `confirmed_seq`. `JK_E_NOT_FOUND` if none validates (the integrator then uses the embedded one, if it has one, or stays in maintenance mode); `JK_E_UNSUPPORTED` without `area_map` |
| `jk_generation_info(gen, info)` | `[pure]` | `gen_seq`, `source_hash` (the generation hash), `registry_hash`, `image_hash`, size, format, `min_runtime`, `JK_GENF_*` flags |

`JK_GENERATION` (embedded) is built by the build with `EMBEDDED`: it does not go through signature verification (its integrity is that of the firmware), but it goes through all the others in `jk_init`.

#### e. Lifecycle

| Function | Context | Behavior |
|---|---|---|
| `jk_init(node, gen, platform, now_us)` (macro) → `jk_init_checked(node, sizeof *node, JK_ABI_VERSION, JK_PROJECT_IFACE_HASH, gen, platform, now_us)` | `[excl]` | checks the ABI (§2.3) and the node alignment (`JK_E_ALIGN`); validates the generation (the `jk_generation_open` checks not yet done); **binds the interface** by name to the registry (a port without a signal compatible in type, unit and direction is `JK_E_GEN_FIRMWARE`); builds the node; reads the persistence (trips, alarms, parameters, `kv`, generation state); puts **all outputs in `safe`** (A8) and marks them `CHANGED`; starts the clock in `Unsynced` (the monotonic epoch changes at every boot, and a previous UTC mapping is not valid); runs the apps' `init`. It may call the flash callbacks. If it fails, the node stays `JK_NODE_UNINIT` and it may be tried again. |
| `jk_restart(node, now_us)` | `[excl]` | warm restart with the same generation: same as `jk_init`, without validating the blob again |
| `jk_shutdown(node)` | `[excl]` | forces all outputs to `safe`, writes the persistence (blocking), moves to `JK_NODE_SHUTDOWN`. Afterwards only `jk_get_output*`, `jk_safe_get`, `jk_node_info` and `jk_init` work |
| `jk_generation_swap(node, next, confirm_ms, now_us)` | `[excl]` | generation swap at a tick boundary of **all** groups (the integrator guarantees it by its being exclusive). `next = NULL` uses the generation received by OTA (`JK_NOTIFY_GEN_READY`), otherwise `JK_E_NOT_FOUND`. It keeps, for each output with the same path, type and unit: the last applied value (A8), the trip state, and the leases of writers with the same identity and the same level; rate ramps continue (E2). It keeps the state of alarms with the same name and the same compiled condition. App state: `migrate` if the version changed (document 01, §9), otherwise kept. With `confirm_ms > 0`, the generation becomes `confirming` (ADR 0015) and, if `jk_generation_confirm` does not arrive in time, the service context goes back to the previous one through the same mechanism and notifies `GEN_ROLLBACK`. The previous generation must remain mapped until confirmation. |
| `jk_generation_confirm(node)` | `[service]` | confirms; writes `confirmed_seq` (anti-rollback); `JK_E_STATE` if nothing is being confirmed |

#### f. Tick and groups

| Function | Context | Behavior |
|---|---|---|
| `jk_tick(node, group, now_us)` | `[group g]` | one complete tick, by the steps of document 01, §4, with the rules of document 16: 1 latch (external inputs, slots of other groups, aging by `now_us`); 2 drains the group's proposal box (external writers, link, async tasks of the apps); 3 the group's expressions (derived, alarms, permissives, queries); 4 cycle tasks of the native apps, in the group's order, with 4b for virtual instances; 5a method expansion; 5 arbiter; 6 envelope; 7 commit (slots, `CHANGED`, trip persistence marked for the service); 8 trace; 9 statistics. `now_us` is the latch instant: it is the apps' `cx.now()` and the value recorded in the trace. A `now_us` smaller than that of the previous tick of the same group is `JK_E_TIME`, without a tick. A late tick is not an error: it counts in `max_lateness_us`, and the group's `overrun` policy (document 01, §4) applies with the duration measured by `jk_now_us()` at the end. An `Err` from an app task is not returned here (it stays in the trace and in the statistics). |
| `jk_group_period_us(node, group)` | `[any]` | period of the current generation; 0 if the group does not exist. The integrator schedules the tick with this value, and not with a constant, to follow new generations |
| `jk_group_stats(node, group, out)` | `[any]` | `ticks`, `overruns`, `skipped`, `last_duration_us`, `max_duration_us`, `max_lateness_us`, `latch_collisions`, `inbox_dropped`; coherent (versioned) read |

#### g. Inputs

| Function | Context | Behavior |
|---|---|---|
| `jk_set_input_bool/_i64/_u64/_f64/_f32(node, sig, v, q, ts_acq_us)` | `[any]`, single writer per signal | ingests a sample into an input signal of an `extern` device, through the core's acquisition stage (document 01, §3.3): conversions of §2.5.4, `NaN` → `Bad(nan)`, physical limits → `Bad(out_of_range)`, `debounce`, acquisition deadband (if the sample falls in the deadband, it does not count as new and the `seq` does not go up), `stuck_detect`. Sets the "changed" bit of the groups that read it. It becomes visible at the **next latch** of each group. |
| `jk_set_input_buf(node, sig, data, len, q, ts)` | likewise | string or bytes |
| `jk_set_input_ex(node, sig, s)` | likewise | with `ts_hw_us` and `ts_src_us` |
| `jk_set_input(node, sig, v, q, ts)` | likewise | `_Generic` macro (C11): `_Bool` → `_bool`; `float` → `_f32`; `double` → `_f64`; signed integers → `_i64`; unsigned → `_u64`. `char` and `long double` are not in the list (compile error: use a cast). It does not exist in C++. |

Errors: `JK_E_SIGNAL` (index outside the interface), `JK_E_DIRECTION` (the signal is not an input of an `extern` device), `JK_E_TYPE`, `JK_E_VALUE`, `JK_E_QUALITY`.

#### h. Reading signals and outputs

| Function | Context | Behavior |
|---|---|---|
| `jk_read(node, sig, now_us, out)` | `[any]` | last value of any signal of the interface (inputs, applied outputs, derived, alarms, exposed signals), with the aging computed at `now_us` (document 01, §3.3). Strings and bytes come with `len = 0`: use `jk_read_buf` |
| `jk_read_buf(node, sig, now_us, out, buf, cap)` | `[any]` | likewise, with a copy of the string or bytes into `buf` (`cap ≥ 64` is always enough); `JK_E_FULL` if `cap` is short |
| `jk_get_output_bool/_i64/_u64/_f64/_f32(node, sig, v, q)` | `[any]` | the **last applied value** (commit) of an output of an `extern` device: it is what the integrator applies to the hardware. `JK_OK` and `*v` written, or `JK_RELINQUISHED` (the output is in `safe = "relinquish"`: `*v` is left untouched and the integrator hands control back to the previous logic, document 03, §6.1). `*q` is `Good`, `Uncertain(not_confirmed)` after a failed verification, or `Bad(tripped)` when tripped. **The value is always applied**, whatever `q` is: it is always admissible (the safe one, if needed). Before the first tick, it is the `safe`. |
| `jk_get_output(node, sig, &v, &q)` | `[any]` | `_Generic` macro over the type of `&v`: `_Bool *`, `int64_t *`, `uint64_t *`, `double *`, `float *` |
| `jk_get_output_ex(node, sig, out)` | `[any]` | complete state: value, quality, tick result, reason, active level, `JK_OUTF_*` flags and `seq` of the commit |
| `jk_take_changed(node, group, &sig)` | `[any]`, one consumer per group | the next `extern` output of the group whose applied state changed since the last call (value, `safe`, trip, relinquish, forced), or `JK_EMPTY`. It is for drivers where writing is costly (Modbus, SPI): a `Suppressed` output never appears here (E8). An integrator who writes a GPIO on every tick does not need it |
| `jk_group_output_count(node, group)`, `jk_group_output_at(node, group, i)` | `[any]` | the `extern` outputs of a group, to bring them to safe after a `jk_supervise` |

| `JK_OUTF_*` | Bit | Meaning |
|---|---|---|
| `CHANGED` | 0 | changed since the last `jk_take_changed` of this group |
| `SAFE` | 1 | no active level: the value is the `safe` (A6) |
| `RELINQUISH` | 2 | `safe = "relinquish"` in effect |
| `TRIPPED` | 3 | tripped (E7) |
| `FORCED` | 4 | `force_safe` active (supervisor or `jk_force_safe`) |
| `UNCONFIRMED` | 5 | `verify` failed |
| `PULSE` | 6 | pulse in progress (`pulse`) |
| `MANUAL` | 7 | the active level is a manual writer |

Errors: `JK_E_SIGNAL`, `JK_E_DIRECTION` (not an `extern` output, for the getters), `JK_E_TYPE`, `JK_E_GROUP`.

#### i. Firmware writes: external and manual writers

| Function | Context | Behavior |
|---|---|---|
| `jk_propose(node, writer, sig, v, now_us)` and `jk_propose_bool/_i64/_u64/_f64` | `[service]` | proposal from an external writer (app `kind = "extern"`) on an output it declares in `writes`. It enters the box of the **owning group** (A11) and is handled at step 2 of that group's next tick, before the cycle tasks (A4). The lease is renewed in that tick (A2, A3). On an output the writer did not declare, the proposal reaches the arbiter and gives `rejected(not_owner)`, without renewing the lease (A3): it is the result the semantics define, not an error of the call. A signal that is not an output is `JK_E_DIRECTION`. Full box: `JK_E_FULL`, counted in `inbox_dropped`. |
| `jk_relinquish(node, writer, sig, now_us)` | `[service]` | releases the writer's level on that output at the next tick (document 01, §6) |
| `jk_last_outcome(node, writer, sig, out)` | `[any]` | the result of that writer's last proposal on that output: `JK_PENDING` until the owning group has ticked; then `JK_OK` with `outcome`, `reject`, `level` (the level that won, for `overridden`), `applied`, `tick_seq` |
| `jk_manual_write(node, sig, v, level, ttl_ms, user_id, now_us)` | `[service]` | manual write from a local HMI (document 16, A10): `level` in the `manual` band (3–5) and allowed on the output (otherwise `JK_E_LEVEL`); `ttl_ms` > 0 and ≤ `max_manual` (otherwise `JK_E_TTL`). Identity `user:<user_id>`, role `operator`. Recorded in the decision log and announced over the link |
| `jk_manual_release(node, sig, level, now_us)` | `[service]` | releases a manual level |

#### j. Classes: methods, actions and events

| Function | Context | Behavior |
|---|---|---|
| `jk_call_method(node, writer, inst, method, args, nargs, now_us)` | `[service]` | adds a method record to the box of the owning group of each assigned `out`; it is expanded at step 5a at the writer's level (17 §4.1). `args` are `jk_value_t` in declaration order, up to `JK_METHOD_MAX_ARGS` (8), converted as in §2.5.4. `JK_E_NOT_FOUND` (instance or member), `JK_E_ARGS` (number, type or unit), `JK_E_FULL` |
| `jk_call_method_cbor(…, cbor, len, now_us)` | `[service]` | likewise, with the arguments in deterministic CBOR (`method-args` in the CDDL), for records and lists |
| `jk_action_invoke(node, caller, inst, action, args, len, key, key_len, now_us, &ticket)` | `[service]` | invokes an action (17 §4.2): role of the `caller` writer and permissive evaluated on reception; deduplication by `key` within `dedup_window` (E9; 1–64 printable ASCII, otherwise `JK_E_ARGS`); serialized queue per device; timeout from the manifest. Returns a ticket. `JK_E_FULL` if there are `action_pending` open tickets |
| `jk_action_result(node, ticket, &err, buf, cap, &len)` | `[service]` | `JK_PENDING`, or `JK_OK` with `err` (`JK_ACTION_OK`, `DENIED`, `TIMEOUT`, `DEVICE`, `INVALID`, `CANCELLED`) and in `buf` the CBOR result (success) or a UTF-8 message (error). The ticket is released on `JK_OK`; `JK_E_FULL` if `cap` is not enough (the ticket stays) |
| `jk_action_cancel(node, ticket)` | `[service]` | requests cancellation (only `@long` actions; the others give `JK_E_STATE`) |
| `jk_action_next(node, &req, args, cap)` | `[service]` | the **implementer's** side: the next request for an action implemented by the firmware (`implementer = extern`), already authorized and deduplicated; `JK_EMPTY` if there is none |
| `jk_action_progress(node, id, percent)` | `[service]` | progress of an `@long` action |
| `jk_action_is_cancelled(node, id)` | `[service]` | `JK_OK` if the caller canceled, `JK_EMPTY` if not |
| `jk_action_complete(node, id, err, result, len)` | `[service]` | responds; after `deadline_us` the library has already responded `TIMEOUT` and the late response gives `JK_E_NOT_FOUND` |
| `jk_event_emit(node, inst, event, cbor, len, now_us)` | `[service]` | emits an `event` of an instance implemented by the firmware (CBOR record of the `.jkc`); delivered to each consumer (apps' `on_event`, link, firmware queue); `JK_E_ARGS` if the CBOR is not a valid record; full queue: it is dropped and counted (`dropped`) |
| `jk_event_next(node, &ev, buf, cap)` | `[service]` | the next event of the firmware queue: class events, alarm transitions (`alarm_from`, `alarm_to`, `severity`, `UNCERTAIN` flag) and events published by apps; `JK_EMPTY` if there is none; `JK_EVF_TRUNCATED` and the real `len` if `cap` is not enough |

#### k. Alarms and trips

| Function | Context | Behavior |
|---|---|---|
| `jk_alarm_info(node, alarm, out)` | `[any]` | state (`ref/00` §6), severity, `shelved`, `suppressed`, `since_us`, `shelved_until_us` |
| `jk_alarm_ack(node, alarm, user_id, now_us)` | `[service]` | acknowledgment (document 02, §11); `JK_E_STATE` if there is nothing to acknowledge |
| `jk_alarm_shelve(node, alarm, ttl_ms, user_id, reason, len, now_us)` | `[service]` | shelving with a required TTL (`JK_E_TTL` if 0); UTF-8 reason ≤ 1 KiB |
| `jk_alarm_unshelve(node, alarm, user_id, now_us)` | `[service]` | |
| `jk_trip_reset(node, sig, user_id, now_us)` | `[service]` | the local `jk reset` (E7); takes effect at the next tick of the owning group; `JK_E_STATE` if the output is not tripped |

#### l. Parameters

| Function | Context | Behavior |
|---|---|---|
| `jk_param_get(node, param, out)` | `[any]` | value in effect of an app parameter (generation or override) |
| `jk_param_set(node, param, v, flags, now_us)` | `[service]` | runtime override, only for parameters declared changeable (`runtime = true`, §2.17); validates the app's type, unit and range (`JK_E_PARAM`); the app receives it at the next tick (`on-params`). With `JK_PARAMF_PERSIST`, it survives restarts as long as the parameter's declaration does not change. Every override is **drift** relative to Git (document 20, §2): it is announced in `HELLO` and appears in `jk status` |
| `jk_param_clear(node, param, now_us)` | `[service]` | goes back to the generation's value and erases the persisted one |

#### m. Time

| Function | Context | Behavior |
|---|---|---|
| `jk_time_set_utc(node, mono_us, utc_us, q)` | `[service]` | sets the monotonic → UTC mapping, when the generation's clock source is `extern` (RTC, an M2's own SNTP). With the `link` source, it is the link's `TIME` that does it, and this function gives `JK_E_STATE` |
| `jk_time_utc(node, mono_us, &utc_us, &q)` | `[any]` | converts; `q` becomes `Holdover` without updates and `Unsynced` after `max_holdover` (document 03, §3) |

#### n. Safe state and watchdog

| Function | Context | Behavior |
|---|---|---|
| `jk_supervise(node, now_us, out)` | `[any]` (made for a timer ISR) | for each group, compares `now_us` with the last tick: > 1 period sets `late_groups`; > 3 periods (document 01, §20) activates `force_safe` on the group's outputs and sets `forced_groups`. With `force_safe`, `jk_get_output*` returns the `safe` with `JK_OUTF_FORCED`, and an integrator whose hardware is stopped in that group should write the safe value directly (`jk_group_output_at` + `jk_safe_get`). `force_safe` clears at the group's next complete tick. `feed_watchdog = 1` only if no group is forced, the node is not stopped and the service has run within `service_deadline` (`service_late`). The integrator feeds the hardware watchdog **only** with `feed_watchdog = 1` |
| `jk_force_safe(node, on)` | `[any]` | forces all outputs to `safe` (emergency stop, *brown-out*); volatile; the result of the commits is `tripped` with `Bad(tripped)` while it lasts; `on = false` goes back to normal without a `reset` |
| `jk_safe_get(node, sig, out)` | `[any]` | the output's `safe` (immutable data of the generation): `JK_OK` or `JK_RELINQUISHED` |

#### o. Trace

| Function | Context | Behavior |
|---|---|---|
| `jk_trace_freeze(node, freeze)` | `[any]` | freezes (or unfreezes) the rings at the next tick boundary; with `freeze_on_trip`, a trip freezes them by itself and notifies `TRACE_FROZEN` |
| `jk_trace_size(node, &bytes)` | `[service]` | size of the `.jktrace` that `jk_trace_read` produces |
| `jk_trace_read(node, offset, buf, cap, &len)` | `[service]` | reads the frozen ring as a `.jktrace` stream (document 16, §7), without compression (bit 0 of the `flags`), with times in **ns** (µs × 1000), one segment per group, and `n_ml_outputs = 0`. `JK_E_STATE` if it is not frozen. The same stream goes through the link's `TRACE` |

#### p. Joker Link

The ABI only exposes the integration point: input and output bytes. The messages, framing, authentication and negotiation belong to `ref/07`.

| Element | Definition |
|---|---|
| `int32_t (*jk_link_rx_fn)(void *ctx, uint8_t *buf, uint32_t cap)` | non-blocking. **Stream** link (UART, RS-485, USB CDC, TCP): returns as many bytes as it wants; the framing (COBS + CRC-32C, D12, ref/07 §3.2) belongs to the library. **Datagram** link (UDP; CAN with ISO-TP done by the integrator): exactly one message per call. 0 = nothing now; < 0 = transport down |
| `int32_t (*jk_link_tx_fn)(void *ctx, const uint8_t *buf, uint32_t len)` | non-blocking. Stream: bytes accepted (partial allowed; the library keeps the rest). Datagram: `len` or 0 (try later). < 0 = down |
| `jk_link_poll(node, rx, tx)` | `[service]`: link 0, with `ctx = platform->user`, `now = jk_now_us()`; runs **one step of `jk_service`** at the end |
| `jk_link_poll_at(node, link, rx, tx, ctx)` | `[service]`: any link, explicit `ctx`; likewise |
| `jk_link_status(node, link, out)` | `[any]`: state (`DOWN`, `HANDSHAKE`, `UP`, `AUTH_FAILED`, `VERSION_MISMATCH`), session mode (`JK_SESSION_FULL`, `_OBSERVE`, `_MAINTENANCE`, `_REFUSED`: the `session_mode` of ref/00 §6, decided by the peer, ref/07 §7.3), negotiated version, authenticated, last error, and **separate** counters: frames received and sent, invalid CRC-32C (`crc_errors`, `J-E2001`), invalid tag (`auth_errors`, `J-E2101`), replay (`replay_drops`, `J-E2102`), other framing and message errors (`rx_errors`), retransmissions and frames discarded on send; `last_rx_us`. They are the same counters that `jk doctor` shows on the Linux node side (ref/07 §3.1) |
| `jk_link_reset(node, link)` | `[service]`: closes the session and restarts the `HELLO` |

The states of `jk_link_state_t` are those of the device machine of ref/07 §7.1: `IDLE` → `DOWN`; `OFFERED` and `CONFIRMING` → `HANDSHAKE`; `UP` → `UP` (with the session mode in `session_mode`); `AUTH_FAILED` and `VERSION_MISMATCH` say why the last handshake did not reach `UP` (answer `refused` with status 27, or three offers without a valid answer; `refused` with status 3, ref/07 §7.3), and go back to `HANDSHAKE` on the next attempt.

The type of each link (stream or datagram), the transport, the MTU, the authentication and the allowed levels come from the generation (`LINKS` section, §2.13.3). The size of the buffers comes from the capabilities. `jk_link_poll` returns `JK_OK`, `JK_E_LINK_DOWN` (callback < 0), `JK_E_LINK_AUTH`, `JK_E_LINK_VERSION` or `JK_E_LINK` (framing); none of these is fatal, and the link restarts by itself.

**Who is in charge of the outputs:** the proposals that arrive over the link are writers of the MCU (`WRITERS` table, one per allowed level of the link: each pair (link, level) is a writer, ref/07 §8.1, 16 A12), with a lease. If the link drops, the lease expires at the latch of the owning group and the envelope applies the `safe` (19 §6). Nothing in the ABI can bypass this.

#### q. Service

| Function | Context | Behavior |
|---|---|---|
| `jk_service(node, now_us)` | `[service]` | one step of the MCU's async domain: async tasks of the apps (`every`, `on`, `on_event`, `startup`; 19 §3), expiry of manual TTLs and confirmation windows, action timeouts, writing of dirty persistent records (trips, alarms, parameters, `kv`), OTA steps (verification in pieces), `notify` notices. It never does network I/O: that is `jk_link_poll` |
| `jk_service_next_us(node)` | `[any]` | the monotonic instant by which the service should run again (`UINT64_MAX`: nothing pending), for whoever wants to sleep between ticks |
| `jk_persist_flush(node)` | `[service]` | writes everything that is dirty right away (blocking) |

**Rule:** with persistence, async tasks, actions, manual TTLs or a confirmation window, the integrator calls `jk_service` (or `jk_link_poll`) at least every `service_deadline` (default 100 ms). `jk_supervise` counts the delays.

#### r. Local OTA

For integrators that receive blobs or firmware over a transport of their own (BLE, the firmware's own MQTT). Joker Link has its own (`ref/07`), which uses the same mechanism.

| Function | Context | Behavior |
|---|---|---|
| `jk_ota_begin(node, kind, total_len)` | `[service]` | `JK_OTA_GENERATION` writes to the inactive generation area; `JK_OTA_FIRMWARE` to `JK_AREA_FW_STAGING`. Erases the area (blocking). `JK_E_BUSY` if an OTA is already in progress; `JK_E_GEN_CAPACITY` if `total_len` does not fit |
| `jk_ota_write(node, offset, data, len)` | `[service]` | writes; the pieces arrive in order (`offset` equal to the total already written, otherwise `JK_E_OTA`) |
| `jk_ota_finish(node)` | `[service]` | verifies: for a generation (the whole `.jkgen` file, image and *trailer*), like `jk_generation_open` (with `min_seq = confirmed_seq + 1`); for firmware (image followed by the COSE manifest), the signature with the `firmware` role, the `board_id` and the `security_version`. Returns `JK_PENDING` while the verification runs (in pieces, so as not to block the service), `JK_OK` and the `GEN_READY` or `FW_READY` notice at the end, or `JK_E_OTA_VERIFY` and the original error in `jk_link_status`/`jk_node_info` |
| `jk_ota_abort(node)` | `[service]` | |

#### s. Diagnostics

`jk_node_info(node, out)` `[any]`: state (`UNINIT`, `RUNNING`, `HALTED`, `SHUTDOWN`), clock quality, `confirming`, `gen_pending`, node bytes in use, `gen_seq`, `iface_hash`, confirmation deadline, `source_hash`, `registry_hash`, version of the installed trust bundle (`trust_version`, 0 without a bundle) and the firmware's `security_version`.

### 2.7 Status codes

`jk_status_t` is `int32_t`. `JK_OK` = 0; positive values are **informative**; negative ones are errors, each with a `J-E25xx` code of the registry (`jk_status_str`). The same codes appear in `jk` and in the API when `jokerd` receives them from an MCU over the link.

| Value | Macro | Code | Severity | Short message |
|---|---|---|---|---|
| 0 | `JK_OK` | — | — | success |
| 1 | `JK_EMPTY` | — | — | nothing (more) to return |
| 2 | `JK_PENDING` | — | — | still to be decided |
| 3 | `JK_RELINQUISHED` | — | — | output in relinquish: no value to apply |
| −1 | `JK_E_ARG` | `J-E2501` | error | invalid argument |
| −2 | `JK_E_STATE` | `J-E2502` | error | call outside the allowed state |
| −3 | `JK_E_ABI` | `J-E2503` | error | ABI, node size or interface incompatible with the library |
| −4 | `JK_E_ALIGN` | `J-E2504` | error | node or blob not 8-byte aligned |
| −5 | `JK_E_UNSUPPORTED` | `J-E2505` | error | function not included in this build |
| −6 | `JK_E_REENTRANT` | `J-E2506` | error | concurrent call in an exclusive context |
| −7 | `JK_E_HALTED` | `J-E2507` | error | node stopped by an internal failure |
| −10 | `JK_E_GEN_FORMAT` | `J-E2510` | error | malformed blob |
| −11 | `JK_E_GEN_VERSION` | `J-E2511` | error | unknown format, ISA or required section |
| −12 | `JK_E_GEN_CAPACITY` | `J-E2512` | error | the generation does not fit the build's capabilities (counterpart of `J0710`) |
| −13 | `JK_E_GEN_SIGNATURE` | `J-E2513` | error | invalid or missing signature |
| −14 | `JK_E_GEN_TRUST` | `J-E2514` | error | untrusted key |
| −15 | `JK_E_GEN_ROLLBACK` | `J-E2515` | error | `gen_seq` below the minimum (anti-rollback) |
| −16 | `JK_E_GEN_FIRMWARE` | `J-E2516` | error | incompatible board, apps, drivers, runtime or interface |
| −17 | `JK_E_GEN_NODE` | `J-E2517` | error | blob bound to another node |
| −18 | `JK_E_GEN_INTEGRITY` | `J-E2518` | error | image CRC or SHA-256 does not match |
| −20 | `JK_E_SIGNAL` | `J-E2520` | error | signal outside the interface |
| −21 | `JK_E_DIRECTION` | `J-E2521` | error | wrong direction for this function |
| −22 | `JK_E_TYPE` | `J-E2522` | error | incompatible type |
| −23 | `JK_E_GROUP` | `J-E2523` | error | unknown group |
| −24 | `JK_E_TIME` | `J-E2524` | error | time going backward |
| −25 | `JK_E_FULL` | `J-E2525` | error | queue or buffer full |
| −26 | `JK_E_NOT_FOUND` | `J-E2526` | error | unknown instance, member, alarm, parameter, writer or ticket |
| −27 | `JK_E_ARGS` | `J-E2527` | error | invalid method or action arguments |
| −28 | `JK_E_VALUE` | `J-E2528` | error | value outside the type's domain |
| −29 | `JK_E_QUALITY` | `J-E2529` | error | invalid combination of code and quality reason |
| −30 | `JK_E_PARAM` | `J-E2530` | error | parameter out of range or not changeable |
| −31 | `JK_E_TTL` | `J-E2531` | error | TTL missing or above the maximum |
| −32 | `JK_E_LEVEL` | `J-E2532` | error | level not allowed |
| −33 | `JK_E_BUSY` | `J-E2533` | error | operation already in progress |
| −40 | `JK_E_PERSIST` | `J-E2540` | error | flash callback failed |
| −41 | `JK_E_PERSIST_CORRUPT` | `J-E2541` | warning | corrupted persistent record (ignored) |
| −42 | `JK_E_PERSIST_FULL` | `J-E2542` | error | persistence area full |
| −50 | `JK_E_LINK` | `J-E2550` | warning | framing or protocol error on the link |
| −51 | `JK_E_LINK_AUTH` | `J-E2551` | error | link authentication failed |
| −52 | `JK_E_LINK_VERSION` | `J-E2552` | error | incompatible link version |
| −53 | `JK_E_LINK_DOWN` | `J-E2553` | warning | transport down |
| −60 | `JK_E_OTA` | `J-E2560` | error | invalid OTA sequence |
| −61 | `JK_E_OTA_VERIFY` | `J-E2561` | error | received image did not verify |
| −99 | `JK_E_INTERNAL` | `J-E2599` | error | internal invariant (code passed to `jk_fatal`) |

Values from −100 to −127 are kept for future use by the ABI; `J-E2600`–`J-E2999` remain free for Joker Micro (MCU drivers, firmware OTA, v1.x).

### 2.8 The generated header (`joker_project.h`)

Generated by `jk build --target mcu:<board> --lib` (and, with the same names in Rust, by `--crate`, §2.11.4). A complete example is in `spec/c/example/joker_project.h`.

#### 2.8.1 Content

| Element | Form | Note |
|---|---|---|
| ABI check | `_Static_assert(JK_ABI_VERSION_MAJOR == M && JK_ABI_VERSION_MINOR >= m, …)` | the version it was generated for |
| identity | `JK_PROJECT_NAME`, `JK_PROJECT_BOARD`, `JK_PROJECT_RUNTIME` (strings) | |
| interface hash | `JK_PROJECT_IFACE_HASH` = `UINT64_C(0x…)` | the first 8 B (little-endian) of the SHA-256 of the canonical interface table; the same value is in the library and in the descriptor (§2.14) |
| node size | `JK_PROJECT_NODE_WORDS` and `struct jk_node { _Alignas(8) uint64_t jk_opaque[JK_PROJECT_NODE_WORDS]; };` | completes the incomplete type of the fixed header |
| measured stacks | `JK_PROJECT_STACK_TICK`, `_SERVICE`, `_LINK_POLL`, `_ISR`, `_GEN_OPEN` (bytes) | §2.9.3 |
| embedded generation | `JK_PROJECT_HAS_EMBEDDED_GENERATION` (0 or 1) and, with 1, `extern const jk_generation_t JK_GENERATION;` | with `--generic`, 0 |
| counts | `JK_GROUP_COUNT`, `JK_SIG_COUNT`, `JK_LINK_COUNT` | |
| identifiers | one macro per interface entry, with a cast (`((jk_sig_t)4u)`), usable in `switch` and not in `#if` | next table |

| Entry | Name | Example |
|---|---|---|
| group | `JK_GROUP_<GROUP>` | `JK_GROUP_MAIN` |
| signal | `JK_SIG_<PATH>` with `.` → `_` | `pump.run` → `JK_SIG_PUMP_RUN` |
| external writer | `JK_WRITER_<APP>` | `JK_WRITER_PANEL` |
| instance | `JK_INST_<INSTANCE>` | `JK_INST_P1` |
| class member | `JK_C_<PACKAGE>_<CLASS>_<MAJOR>_<MEMBER>`, with `/` and `.` → `_` | `std/hvac.vav@1`, `damper` → `JK_C_STD_HVAC_VAV_1_DAMPER` |
| alarm | `JK_ALARM_<NAME>` | `JK_ALARM_TANK_HIGH` |
| app parameter | `JK_PARAM_<APP>_<PARAMETER>` | `JK_PARAM_LEVEL_TARGET` |
| link | `JK_LINK_<NAME>` | `JK_LINK_UPLINK` |
| enum variant | `JK_ENUM_<ENUM>_<VARIANT>` (the enum name goes from `CamelCase` to `SNAKE`) | `PcsMode.charge` → `JK_ENUM_PCS_MODE_CHARGE` (numeric value) |
| named bit | `JK_BIT_<PATH>_<BIT>` (bit index) | `JK_BIT_PCS_STATUS_ALARM_OVERTEMP` |

- In each entry type, the indices are **dense from 0, in name order** (the canonical path, for signals). That is what makes the header deterministic.
- Each macro has a comment with the path, the type, the unit, the direction and, for outputs, the owning group and the `safe` of the generation it was generated with.
- A collision of two names in the same macro (`a.b_c` and `a_b.c`) is `J0775`; `jk build` suggests an alias. An identifier with more than 63 characters is the warning `J0787` (it is the limit guaranteed by C11 for internal names and macros).

#### 2.8.2 What goes into the interface

| Entry | When | Required in a new generation |
|---|---|---|
| signals of `extern` devices | always | **yes** |
| outputs declared in `writes` of `extern` apps | always | **yes** |
| signals in `reads` of `extern` apps and in `[target.extern] expose` | if declared | no |
| groups | all those of the build's generation | **yes** |
| external writers | apps `kind = "extern"` | **yes** |
| links | all | **yes** |
| instances and the members of their classes | all instances of the build's generation | no |
| alarms | all | no |
| app parameters | all (the changeable ones marked in the comment) | no |

When a new generation (through flash) lacks a **required** entry with the same name, type, unit and direction, it is rejected (`JK_E_GEN_FIRMWARE` on the MCU, `J0790` in `jk build --generation`). A missing **optional** entry becomes "absent": the functions that receive it return `JK_E_NOT_FOUND`, and `jk build --generation` shows it as a warning in the same `J0790` message.

### 2.9 Memory, alignment and stack

#### 2.9.1 Node memory

- `jk_node_t` has `JK_PROJECT_NODE_WORDS × 8` bytes, alignment of 8, and is allocated statically (`static jk_node_t node;`). It may be in any RAM that all the contexts that use it see coherently. It need not be zeroed: `jk_init` writes all of it.
- The size is the sum of the build's capabilities (§2.13.3) times the costs of the following table, plus the app state (`size_of` of each app and of the states of its tasks), rounded up to 8. `jk check` uses the same formulas for `J0710` and shows the detail per component.

| Component | RAM |
|---|---|
| slot per signal | 32 B (+ 2 × 64 B of arena per `string`/`bytes` signal) |
| process image | 16 B per latched port per group |
| output (envelope, arbiter, double-buffered box) | 96 B + 24 B per declared writer |
| expression state | the `state_len` of each program; windows: 16 B per sample |
| proposal box | 24 B per entry, per group |
| method queue | 16 B + arguments per record, per group |
| pending actions | 48 B + arguments (≤ 1 KiB) |
| events | 32 B + payload (≤ 1 KiB) per entry |
| link | 2 × MTU + 256 B of state (+ 64 B with encryption) per link |
| trace | `trace_bytes` (default 16 KiB, U9 of document 19) |

#### 2.9.2 Node header (for probes)

The first 64 bytes of `jk_node_t` have a fixed format, so that `jk trace-dump --probe` finds the node and the trace rings in RAM without symbols. The rest of the node is opaque.

| Offset | Type | Field |
|---|---|---|
| 0 | `u32` | magic `"JKND"` (0x444E4B4A) |
| 4 | `u16`, `u16` | major and minor of the library's ABI |
| 8 | `u32` | state (`jk_node_state_t`) |
| 12 | `u32` | `JK_PROJECT_NODE_WORDS` |
| 16 | `u64` | interface hash |
| 24 | `u64` | `gen_seq` |
| 32 | `u32` | offset (from the start of the node) of the trace table: per group, `u32` ring offset, `u32` size, `u32` head, `u32` flags (bit 0 frozen) |
| 36 | `u32` | number of groups |
| 40 | `[8]u8` | first 8 B of the `registry_hash` |
| 48 | `[16]u8` | reserved, 0 |

#### 2.9.3 Alignment and stack

| Rule | Value |
|---|---|
| node alignment | 8 (`JK_E_ALIGN`) |
| blob alignment | 8, in mapped flash or in RAM (`JK_E_ALIGN`) |
| buffers passed to the ABI | any alignment |
| flash writes | multiples of `write_size`, erases of `erase_size`, library data with any alignment |
| FPU in an ISR | the `[any]` functions with `float`/`double` use the FPU; the ISR must save the FPU context (Cortex-M *lazy stacking* is enough) |
| multicore | supported (RP2350) if the node memory is coherent; on an MCU with a data cache and two cores (STM32H745), the node goes in non-cacheable memory or on a single core |

| Entry point | Target stack (to be measured) | Macro measured by the build |
|---|---|---|
| `jk_set_input*`, `jk_get_output*`, `jk_read*`, `jk_supervise` and the remaining `[any]` | ≤ 256 B | `JK_PROJECT_STACK_ISR` |
| `jk_tick` | ≤ 1.5 KiB + the largest stack of the group's app cycle tasks | `JK_PROJECT_STACK_TICK` (the largest of all groups) |
| `jk_service` | ≤ 2 KiB + the largest stack of the async tasks | `JK_PROJECT_STACK_SERVICE` |
| `jk_link_poll` (includes a service step) | ≤ 3.5 KiB + async tasks | `JK_PROJECT_STACK_LINK_POLL` |
| `jk_generation_open`, `jk_ota_finish` (Ed25519) | ≤ 3 KiB | `JK_PROJECT_STACK_GEN_OPEN` |
| `jk_init`, `jk_generation_swap` | ≤ 2 KiB + the apps' `init`/`migrate` | included in `_SERVICE` |

The stacks are measured at build time by static analysis of the call graph (no recursion in the core; apps with recursion or with indirect calls that cannot be analyzed give the warning `J0791` ("not measured"). With groups in nested interrupt executors, `jk check` adds up the stacks of all priority levels that may be active at the same time and compares with the board's stack (`J0774`).

### 2.10 Persistence

#### 2.10.1 Areas

| Area | Content | Minimum size | Who writes |
|---|---|---|---|
| `JK_AREA_PERSIST` | generation state, trips, alarms (acknowledgment, shelving), persistent parameters, app `kv`, actuation budgets | 2 erase sectors and ≥ 4 × the live content | the library, in the service |
| `JK_AREA_GEN_A`, `JK_AREA_GEN_B` | one generation blob each (A/B) | the largest blob (`J0781`) | the library (OTA) or `jk flash` |
| `JK_AREA_FW_STAGING` | firmware image received by OTA, for the bootloader | the firmware image | the library (OTA) |
| `JK_AREA_IDENTITY` | node name, link keys, trust bundle (≤ 2 KiB), X25519 secrets key (standalone MCU) | 2 sectors | enrollment (`jk node enroll <mcu> --link <d>`, ref/07 §4.7; ref/10 §3.1.3), `jk flash --identity` and the installation of bundles (`TRUST` section of the blob, or the agent on a standalone MCU) |

In library mode, the integrator chooses where the areas go (`area_info`); an area with `size = 0` does not exist.

**Remote levels are not persisted.** The levels of remote writers (link), including the manual writes that arrive over the link, live only in RAM: after an MCU reset they disappear until the Linux node renews them. The persistence of remote manual levels (16 A14) belongs to the Linux node, which keeps the TTL and sends them with `lease = min(remaining TTL, max_lease of the link)` (16 A12, ref/07 §8.1). In firmware mode, the board declares them (§2.12.2).

#### 2.10.2 Format

The area is a circular journal of sectors. It is the library that defines it, so that a new firmware reads the records of an old one.

```
sector   := header(16 B) record* erased-space
header   := "JKPS" u16:format(=1) u16:flags u32:erase-counter u32:crc32c(of the previous 12 B)
record   := u8:type u8:flags(bit 0 = tombstone) u16:key-length u16:value-length u16:0
            u32:seq(global, increasing) u32:crc32c(header without the crc + key + value)
            key value padding(up to a multiple of max(8, write_size), with the erased value)
```

- Integers in little-endian; **CRC-32C** (Castagnoli, D12), the same as Joker Link's (ref/07 §3.2) and the blob's.
- **Keys:** never `SignalId` (it is only valid within a generation; `ref/00` §5). A signal, alarm or parameter is identified by the **path-hash**: the first 8 bytes of the SHA-256 of the canonical path in UTF-8 (`alarm.<name>`, `<app>.<parameter>`).
- **Values:** deterministic CBOR, with the schemas of `joker-micro.cddl` (§4), except `kv`, which stores the app's bytes.

| Type | Key | Value (CDDL) |
|---|---|---|
| 1 generation state | — (single) | `gen-state` |
| 2 trip | path-hash of the output | `trip-state` |
| 3 alarm | path-hash of `alarm.<name>` | `alarm-persist` |
| 4 parameter | path-hash of `<app>.<parameter>` | `param-persist` |
| 5 `kv` | path-hash of the app ‖ key (≤ 64 B) | bytes (≤ 1 KiB per value, ≤ 16 KiB per app; `J0784`) |
| 6 actuation budget | path-hash of the output | `budget-persist` |
| 16 node name | — | `identity-node` (`IDENTITY` area) |
| 17 link key | path-hash of the link name | `identity-link-key` (`IDENTITY` area) |
| 18 trust bundle | — (single) | `identity-trust`: the COSE `trust-bundle` of ref/10 §2.4.3, micro profile (≤ 2 KiB, ≤ 8 keys; `IDENTITY` area) |
| 19 secrets key | — (single) | `identity-kem`: X25519 of the standalone MCU, attested in its certificate (ref/02 §2.4; `IDENTITY` area) |
| 7–15, 20–127 | reserved | |
| 128–255 | private to the integrator (the library ignores them and preserves them in compaction) | |

- **Reading at boot:** the area is scanned; for each (type, key) the record with the largest `seq` and a valid CRC prevails. A record with a wrong CRC is ignored and counted (`JK_E_PERSIST_CORRUPT` in the statistics, alarm `joker.persist.corrupt`), and does not prevent boot.
- **Compaction:** when the active sector fills up, the live records are copied to the next erased sector, and the old one is erased only after the new one is complete. A power outage midway always leaves a valid set. Without space after compacting: `JK_E_PERSIST_FULL` and an alarm.
- **Wear:** the per-sector erase counter spreads the writing. `jk check` estimates the erases per day from the declared rates (trips, `kv`) and warns if the flash life falls below 10 years (`J0792`).

#### 2.10.3 When writing happens

Writing to flash takes milliseconds and **never** happens in a group context. A trip, an acknowledgment or a changed parameter marks the record as dirty, and the next `jk_service` writes it. The window between the occurrence and the write is, at most, the interval between calls to the service (bounded by `service_deadline`). `jk_shutdown` and `jk_persist_flush` write everything right away. This difference from Linux (document 01, §7: trip with fsync) is declared in §4, D5.

### 2.11 The Rust crate (`joker-micro`)

#### 2.11.1 Crates

| Crate | Role | `no_std` |
|---|---|---|
| `joker-core`, `joker-expr`, `joker-units`, `joker-link-proto` | the core shared with Linux (ADR 0020) | yes |
| `joker-micro` | host of the core on MCUs: node, contexts, persistence, link, OTA, blob, trace; `embassy` module; MCU drivers | yes |
| `joker-micro/ffi` (module) | the C ABI (`jk_*`), only with the `c-abi` feature | yes |
| `joker-sdk` with the `backend-native` feature | the app SDK, compiled natively (§2.11.5) | yes |
| `joker-project` | **generated** by `jk build --crate`: typed interface, generation, apps, storage and Embassy tasks of the project | yes |

**Core dependencies** (`joker-core`, `joker-expr`, `joker-units`, `joker-link-proto`): only stable (1.x) or trivial crates, without `alloc` on the tick path: `heapless`, `critical-section`, `portable-atomic` (with `critical-section` underneath where there are no 64-bit atomics) and `minicbor`. Synchronization is only through these two (§2.4, rule 8). Everything that is 0.x (Embassy, `embedded-io`, `esp-hal` `unstable`, `esp-radio`) stays in `joker-micro` and in the `chip-*` modules, behind interfaces that are updated without touching the core or the C ABI (document 25, §5; document 13, §5.1).

#### 2.11.2 Features of `joker-micro`

| Feature | Default | What it enables |
|---|---|---|
| `embassy` | yes | `embassy` module (tasks, timers, `embassy-time`) |
| `link` | yes | Joker Link (`joker-link-proto`), stream and datagram |
| `link-crypto` | yes | ChaCha20-Poly1305 (required on IP; `ref/07`) |
| `gen-flash` | yes | opening flash blobs: Ed25519, SHA-256, COSE |
| `persist` | yes | persistence (§2.10) over `embedded-storage` |
| `trace` | yes | trace rings |
| `ota` | yes, **required** | OTA of generations and firmware. In firmware mode and in crate mode, the OTA receiver is always part of the image (§2.12.6): without it, `joker-micro` does not compile (`compile_error!` with `J0795`), except with `c-abi`, in which it is the integrator's firmware that carries the image |
| `defmt` | no | logs with `defmt` |
| `c-abi` | no | exports the `jk_*` (library mode) |
| `cs-single-core` | no | `critical-section` implementation by disabling interrupts, for a single-core MCU in which no interrupt calls the node (§2.4, rule 8); without it, in library mode, the integrator provides the implementation |
| `drv-gpio`, `drv-adc`, `drv-pwm`, `drv-i2c`, `drv-spi`, `drv-uart`, `drv-modbus-rtu`, `drv-modbus-rtu-slave`, `drv-can`, `drv-onewire` | no | MCU drivers, each over the trait of the table of ref/06 §3.7 (`embedded-hal` 1.0 where it exists; `AdcRead` and `CanFrames` from `joker-hal` where it does not); `extern` is always present |
| `chip-rp`, `chip-stm32`, `chip-esp` | no | binding to the HAL of **one family** (firmware mode only): `embassy-rp`, `embassy-stm32` or `esp-hal`. **Exactly one** per build: zero or more than one is `compile_error!`. The exact chip reference is not a feature of `joker-micro`: `jk build` passes it to the HAL (the HAL's feature, such as `esp-hal/esp32` or `embassy-stm32/stm32g474re`) from `[board] chip` (ref/03 §2.3.1). The bench's original ESP32 is `chip-esp` with `chip = "esp32"`. With Wi-Fi, `chip-esp` brings the radio's fixed heap (§2.15.2). The `chip-*` modules have no `unsafe` (§2.11.3, pin ownership) |

`jk build` chooses the features from the manifest and the board. Without a feature, the corresponding functions exist and return `Error::Unsupported` (`JK_E_UNSUPPORTED`), so that the same code compiles with any set.

#### 2.11.3 Public API

```rust
#![no_std]
// Shared types, re-exported from the core (the same as in the SDK and on Linux):
pub use joker_core::{Quality, QualityCode, QualityReason, WriteOutcome, RejectReason,
                     ClockQuality, AlarmState, Severity, Role, Level, ValueType, Value};

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Micros(pub u64);          // monotonic, µs since MCU boot
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct UtcMicros(pub i64);

/// One error per `JK_E_*`; `code()` returns "J-E25xx".
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Error { Arg, State, Abi, Align, Unsupported, Reentrant, Halted,
                 GenFormat, GenVersion, GenCapacity, GenSignature, GenTrust, GenRollback,
                 GenFirmware, GenNode, GenIntegrity, Signal, Direction, Type, Group, Time,
                 Full, NotFound, Args, Value, Quality, Param, Ttl, Level, Busy,
                 Persist, PersistCorrupt, PersistFull, Link, LinkAuth, LinkVersion, LinkDown,
                 Ota, OtaVerify, Internal }
impl Error { pub const fn code(self) -> &'static str; pub const fn status(self) -> i32; }

// Typed handles (generated in joker_project; the type catches at compile time what C sees as JK_E_TYPE)
pub struct In<T: SignalType>(/* interface index */);       // input of an extern device
pub struct Out<T: SignalType>(/* … */);                    // output
pub struct Sig<T: SignalType>(/* … */);                    // read-only signal
pub struct GroupId(/* … */); pub struct WriterId(/* … */); pub struct LinkId(/* … */);
pub struct InstId(/* … */); pub struct Member(/* … */); pub struct AlarmId(/* … */);
pub struct ParamId<T: SignalType>(/* … */);
pub trait SignalType: Copy + sealed::Sealed { const TYPE: ValueType; }
// bool, i64, u64, f64, f32, enums #[derive(joker::Enum)], Bits<N>, Text (heapless::String<64>), Bytes (heapless::Vec<u8, 64>)

pub struct Sample<T> { pub value: T, pub quality: Quality, pub seq: u64,
                       pub ts_acq: Micros, pub ts_hw: Option<Micros>, pub ts_src: Option<UtcMicros> }
pub struct OutputState<T> { pub value: Option<T> /* None = relinquish */, pub quality: Quality,
                            pub outcome: WriteOutcome, pub level: Option<Level>, pub flags: OutFlags, pub seq: u64 }

// Generation
pub struct Generation<'a> { /* … */ }
impl<'a> Generation<'a> {
    pub fn open(blob: &'a [u8], trust: &[[u8; 32]], identity: Option<&str>, min_seq: u64)
        -> Result<Self, Error>;
    pub fn boot(storage: &'a dyn Storage, trust: &[[u8; 32]]) -> Result<Self, Error>;   // jk_generation_boot
    pub const fn embedded(blob: &'static AlignedBlob) -> Generation<'static>;
    pub fn info(&self) -> GenerationInfo;
}
#[repr(C, align(8))] pub struct AlignedBlob(pub [u8]);
#[macro_export] macro_rules! include_generation { ($path:literal) => { /* &'static AlignedBlob */ } }

// Platform (the jk_platform_t in Rust)
pub trait Platform: Sync {
    fn now(&self) -> Micros;                                  // = jk_now_us
    fn storage(&self) -> Option<&dyn Storage> { None }
    fn trust_keys(&self) -> &[[u8; 32]] { &[] }                     // anchors (§2.13.5)
    fn link_key(&self, _link: LinkId) -> Option<[u8; 32]> { None }
    fn kem_key(&self) -> Option<[u8; 32]> { None }                  // X25519 secrets key (standalone MCU)
    fn notify(&self, _n: Notify) {}
}
pub trait Storage: Sync {                                     // blocking; only [excl] and [service]
    fn info(&self, area: Area) -> Option<AreaInfo>;
    fn read(&self, area: Area, offset: u32, buf: &mut [u8]) -> Result<(), Error>;
    fn write(&self, area: Area, offset: u32, data: &[u8]) -> Result<(), Error>;
    fn erase(&self, area: Area, offset: u32, len: u32) -> Result<(), Error>;
    fn map(&self, _area: Area) -> Option<&'static [u8]> { None }
}
/// Adapter over embedded-storage, with the board's area table.
pub struct NorStorage<F: embedded_storage::nor_flash::NorFlash> { /* … */ }
impl<F: embedded_storage::nor_flash::NorFlash> NorStorage<F> {
    pub fn new(flash: F, areas: &'static [(Area, u32 /*offset*/, u32 /*size*/)]) -> Self;
}

// The project (implemented by the generated crate)
pub trait Project: 'static {
    type Storage: NodeStorage;       // node memory: concrete fixed-capacity types, no arena and no unsafe
    type Apps: AppSet;               // the project's native apps (§2.11.5)
    const IFACE: &'static Iface;
    const IFACE_HASH: u64;
}

pub fn init<'n, P: Project>(storage: &'n mut P::Storage, apps: &'n mut P::Apps,
                            gen: &'n Generation<'n>, platform: &'n dyn Platform, now: Micros)
    -> Result<Parts<'n, P>, Error>;                         // [excl]

/// Capabilities split by context: the type system does what C does with JK_E_REENTRANT.
pub struct Parts<'n, P: Project> {
    pub io: Io<'n, P>,              // Copy + Send + Sync: [any] functions
    pub service: Service<'n, P>,    // Send, !Sync: the service context (one)
    pub groups: Groups<'n, P>,      // where the GroupRunners are taken from (one per group)
    pub changes: Changes<'n, P>,    // where the ChangeFeeds are taken from (one per group)
}
impl<'n, P: Project> Groups<'n, P> {
    pub fn take(&mut self, g: GroupId) -> Option<GroupRunner<'n, P>>;
    pub fn give_back(&mut self, r: GroupRunner<'n, P>);
    pub fn is_complete(&self) -> bool;                      // all given back
}
impl<'n, P: Project> GroupRunner<'n, P> {                   // Send, !Sync: [group g]
    pub fn tick(&mut self, now: Micros) -> Result<(), Error>;
    pub fn id(&self) -> GroupId;
    pub fn period(&self) -> Micros;
}
impl<'n, P: Project> Changes<'n, P> { pub fn take(&mut self, g: GroupId) -> Option<ChangeFeed<'n, P>>; }
impl<'n, P: Project> ChangeFeed<'n, P> { pub fn next(&mut self) -> Option<OutAny>; }

impl<'n, P: Project> Io<'n, P> {                            // [any]
    pub fn set_input<T: SignalType>(&self, s: In<T>, v: T, q: Quality, ts_acq: Micros) -> Result<(), Error>;
    pub fn set_input_sample<T: SignalType>(&self, s: In<T>, v: T, q: Quality, ts_acq: Micros,
                                           ts_hw: Option<Micros>, ts_src: Option<UtcMicros>) -> Result<(), Error>;
    pub fn read<T: SignalType, S: Readable<T>>(&self, s: S, now: Micros) -> Result<Sample<T>, Error>;
    pub fn output<T: SignalType>(&self, o: Out<T>) -> Result<OutputState<T>, Error>;
    pub fn safe<T: SignalType>(&self, o: Out<T>) -> Option<T>;             // None = relinquish
    pub fn group_outputs(&self, g: GroupId) -> impl Iterator<Item = OutAny> + 'n;
    pub fn last_outcome<T: SignalType>(&self, w: WriterId, o: Out<T>) -> Option<OutcomeInfo<T>>;
    pub fn supervise(&self, now: Micros) -> Supervision;
    pub fn force_safe(&self, on: bool);
    pub fn trace_freeze(&self, on: bool);
    pub fn group_stats(&self, g: GroupId) -> GroupStats;
    pub fn alarm(&self, a: AlarmId) -> Result<AlarmInfo, Error>;
    pub fn param<T: SignalType>(&self, p: ParamId<T>) -> Result<T, Error>;
    pub fn utc(&self, at: Micros) -> (Option<UtcMicros>, ClockQuality);
    pub fn link_status(&self, l: LinkId) -> LinkStatus;
    pub fn node_info(&self) -> NodeInfo;
    pub fn service_next(&self) -> Option<Micros>;
}

impl<'n, P: Project> Service<'n, P> {                       // [service]
    pub fn poll(&mut self, now: Micros) -> Result<(), Error>;                            // jk_service
    pub fn link_stream<T>(&mut self, l: LinkId, io: &mut T, now: Micros) -> Result<(), Error>
        where T: embedded_io::Read + embedded_io::ReadReady + embedded_io::Write + embedded_io::WriteReady;
    pub fn link_datagram<D: Datagram>(&mut self, l: LinkId, io: &mut D, now: Micros) -> Result<(), Error>;
    pub fn link_reset(&mut self, l: LinkId);
    pub fn propose<T: SignalType>(&mut self, w: WriterId, o: Out<T>, v: T, now: Micros) -> Result<(), Error>;
    pub fn relinquish<T: SignalType>(&mut self, w: WriterId, o: Out<T>, now: Micros) -> Result<(), Error>;
    pub fn manual_write<T: SignalType>(&mut self, o: Out<T>, v: T, level: Level, ttl_ms: u32,
                                       user: u32, now: Micros) -> Result<(), Error>;
    pub fn manual_release<T: SignalType>(&mut self, o: Out<T>, level: Level, now: Micros) -> Result<(), Error>;
    pub fn call_method(&mut self, w: WriterId, i: InstId, m: Member, args: &[Value], now: Micros) -> Result<(), Error>;
    pub fn call_method_cbor(&mut self, w: WriterId, i: InstId, m: Member, cbor: &[u8], now: Micros) -> Result<(), Error>;
    pub fn invoke_action(&mut self, caller: WriterId, i: InstId, a: Member, args_cbor: &[u8],
                         key: &str, now: Micros) -> Result<Ticket, Error>;
    pub fn action_result(&mut self, t: Ticket, buf: &mut [u8]) -> core::task::Poll<Result<ActionReply, Error>>;
    pub fn cancel_action(&mut self, t: Ticket) -> Result<(), Error>;
    pub fn next_action_request(&mut self, args: &mut [u8]) -> Option<ActionRequest>;
    pub fn action_progress(&mut self, id: u32, percent: u8) -> Result<(), Error>;
    pub fn action_cancelled(&self, id: u32) -> bool;
    pub fn complete_action(&mut self, id: u32, r: Result<&[u8], (ActionError, &str)>) -> Result<(), Error>;
    pub fn emit_event(&mut self, i: InstId, e: Member, cbor: &[u8], now: Micros) -> Result<(), Error>;
    pub fn next_event(&mut self, buf: &mut [u8]) -> Option<Event>;
    pub fn alarm_ack(&mut self, a: AlarmId, user: u32, now: Micros) -> Result<(), Error>;
    pub fn alarm_shelve(&mut self, a: AlarmId, ttl_ms: u32, user: u32, reason: &str, now: Micros) -> Result<(), Error>;
    pub fn alarm_unshelve(&mut self, a: AlarmId, user: u32, now: Micros) -> Result<(), Error>;
    pub fn trip_reset<T: SignalType>(&mut self, o: Out<T>, user: u32, now: Micros) -> Result<(), Error>;
    pub fn param_set<T: SignalType>(&mut self, p: ParamId<T>, v: T, persist: bool, now: Micros) -> Result<(), Error>;
    pub fn param_clear<T: SignalType>(&mut self, p: ParamId<T>, now: Micros) -> Result<(), Error>;
    pub fn time_set_utc(&mut self, mono: Micros, utc: UtcMicros, q: ClockQuality) -> Result<(), Error>;
    pub fn confirm_generation(&mut self) -> Result<(), Error>;
    /// [excl] in Rust: requires all GroupRunners given back (Groups::is_complete).
    pub fn swap_generation(&mut self, groups: &mut Groups<'n, P>, next: Option<&'n Generation<'n>>,
                           confirm_ms: u32, now: Micros) -> Result<(), Error>;
    pub fn persist_flush(&mut self) -> Result<(), Error>;
    pub fn trace_size(&self) -> Result<u32, Error>;
    pub fn trace_read(&self, offset: u32, buf: &mut [u8]) -> Result<usize, Error>;
    pub fn ota_begin(&mut self, kind: OtaKind, total_len: u32) -> Result<(), Error>;
    pub fn ota_write(&mut self, offset: u32, data: &[u8]) -> Result<(), Error>;
    pub fn ota_finish(&mut self) -> core::task::Poll<Result<(), Error>>;
    pub fn ota_abort(&mut self);
}

pub trait Datagram { fn recv(&mut self, buf: &mut [u8]) -> Result<Option<usize>, Error>;
                     fn send(&mut self, buf: &[u8]) -> Result<bool, Error>; }
```

- The rules are those of the C ABI, function by function (the C ABI is a thin layer on top of this crate).
- **No `unsafe` outside the `ffi` module**, and this also holds for the `chip-*` modules. The node memory is a generated concrete type (`P::Storage`), with fixed-capacity arrays, and not an arena; the apps live in `P::Apps`, also concrete.
- **Pin ownership in the generic firmware.** The generic firmware only knows which pin is the LED when it reads the blob, at runtime, and the HALs hand out the peripherals once, as *singletons*, with the pins chosen at compile time. `joker-micro` **never** uses `steal` (`AnyPin::steal(n)` and similar are `unsafe` and bypass ownership). Instead, at step 6 of boot (§2.12.3), the `chip-*` module receives the peripherals from the HAL's `init`, converts **all** the pins declared in the board's `[pins]` into an erased type (`degrade()` to `AnyPin` in `esp-hal`, `Peri<AnyPin>` in `embassy-rp`, `AnyPin` in `embassy-stm32`) and keeps them in a **pin table** indexed by the board's name. Each driver takes its pin from the table **once**; a second request for the same pin is `JK_E_GEN_FIRMWARE` (`jk check` already rejects it before, with `J0829`, ref/03 §2.3.3). A pin that is not in `[pins]` does not exist for the firmware.
- A *panic* in an app or in the core is handled by the firmware's *panic handler* (recommended: `panic-reset`); the library does not catch it. In library mode, the *panic handler* of `libjoker_node.a` calls `jk_fatal(JK_E_INTERNAL, …)`.

#### 2.11.4 The generated crate (`joker-project`)

`jk build --target mcu:<board> --crate` generates, in `target/joker/crate/`, a crate called `joker-project` that the integrator's firmware uses by path (`joker-project = { path = "…" }`):

| Item | Content |
|---|---|
| `pub struct Project;` and `impl joker_micro::Project for Project` | storage and app types, interface, hash |
| `pub type Storage`, `pub type Apps` | the concrete types |
| `pub mod sig { pub const TANK_LEVEL: In<f32> = …; pub const PUMP_RUN: Out<bool> = …; … }` | the same names as in C, without the `JK_SIG_` prefix |
| `pub mod group`, `writer`, `inst`, `member`, `alarm`, `param`, `link`, `enums` | likewise |
| `pub static GENERATION: Generation<'static>` | only with an embedded generation |
| `pub const NODE_BYTES: usize`, `pub const STACK_*: usize` | as in C |
| `pub mod tasks` (`embassy` feature) | `#[embassy_executor::task] group_task(GroupRunner<'static, Project>)`, `service_task(…)`, `supervisor_task(…)`: Embassy does not accept generic tasks, so they are generated here |
| `pub fn init(storage, apps, platform, now) -> Result<Parts<'static, Project>, Error>` | shortcut for `joker_micro::init::<Project>` with the embedded generation |

#### 2.11.5 Contract with `joker-sdk` (native backend)

The same app (19 §5) compiles with `joker-sdk` and the `backend-native` feature (instead of `backend-wasm`, the default one). The `#[joker::app]` macro then generates, instead of the WIT exports:

```rust
pub trait NativeApp: Sized + 'static {                // implemented only by the macro
    const META: &'static [u8];                        // the same CBOR as the joker:meta section (ref/04 §5)
    type Cycle: Default;                              // state of each cycle task (generated tuple)
    type Tasks;                                       // futures of the async tasks, of fixed size
    fn init(binder: &mut Binder<'_>, params: &ParamSet<'_>) -> Result<Self, InitError>;
    fn on_params(&mut self, params: &ParamSet<'_>) -> Result<(), InitError>;
    fn on_cycle(&mut self, st: &mut Self::Cycle, task: u16, cx: &mut CycleCx<'_>) -> TaskResult;
    fn poll_task(&mut self, tasks: &mut Self::Tasks, task: u16, cx: &mut TaskCx<'_>,
                 waker: &mut core::task::Context<'_>) -> core::task::Poll<TaskResult>;
    fn on_action(&mut self, instance: u32, action: u16, args: &[u8], out: &mut [u8],
                 cx: &mut TaskCx<'_>) -> core::task::Poll<Result<usize, ActionFail>>;
    fn export_state(&self, out: &mut [u8]) -> usize;
    fn migrate(&mut self, previous: &[u8], from_version: &str) -> Result<(), InitError>;
}
pub trait AppSet { /* generated: dispatch by app index, without dyn */ }
```

- `CycleCx` gives the app the group's latch image and the proposal buffer, with the same records as the packed image of ref/04 §3 (write: 14 B; method: 10 B + arguments). The SDK's `io.pump.set(v)` writes there directly: it is the same path as on Linux without the Wasm boundary.
- The async tasks (`every`, `on`, `on_event`, `startup`) are futures kept in `Tasks` (size known at build time) and advanced by `jk_service`.
- **API unavailable on the MCU** (`history`, `infer`, `frames`, `subscribe` of arbitrary lists, network, `kv` above 16 KiB): with `backend-native`, these functions require a *trait* that the target does not implement, annotated with `#[diagnostic::on_unimplemented(message = "J0720: … does not exist on Joker Micro", note = "<suggestion>")]`. The error comes out of `rustc`, and `jk build` translates the message into `J0720` with the suggestion (19 §5, U6).
- **`J0721`** (f64 without a double-precision FPU) and **`J0780`** (floating point without an FPU): besides the ports and the `f64` fields of `joker:meta` (ref/04 §4.14), `jk build` looks, in the object of each app, for calls to the software floating-point routines (`__aeabi_d*`, `__adddf3`… for f64; `__aeabi_f*`, `__addsf3`… for f32) and gives the warning with the names of the app functions that call them.
- A native app has no *sandbox*, *fuel* or *epoch*: an infinite loop is an overrun of the group, which `jk_supervise` catches, and a *panic* is `jk_fatal`. It is the declared cost of v1 (`wasmi` arrives in 1.4).

#### 2.11.6 Embassy

```rust
// Minimal example: one group in an interrupt executor, the service and the supervisor in the thread executor.
static STORAGE: StaticCell<joker_project::Storage> = StaticCell::new();
static APPS: StaticCell<joker_project::Apps> = StaticCell::new();
static EXEC_FAST: InterruptExecutor = InterruptExecutor::new();

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    let platform = PLATFORM.init(Board::new(/* timer, flash */));
    let mut parts = joker_project::init(STORAGE.init(Default::default()), APPS.init(Default::default()),
                                        platform, Micros(Instant::now().as_micros())).unwrap();
    interrupt::SWI_IRQ_1.set_priority(Priority::P1);
    let fast = EXEC_FAST.start(interrupt::SWI_IRQ_1);
    fast.spawn(joker_project::tasks::group_task(parts.groups.take(joker_project::group::MAIN).unwrap())).unwrap();
    spawner.spawn(joker_project::tasks::service_task(parts.service /*, link */)).unwrap();
    spawner.spawn(joker_project::tasks::supervisor_task(parts.io, Watchdog::new(p.WATCHDOG))).unwrap();
}
```

| Function of `joker_micro::embassy` | Does |
|---|---|
| `async fn run_group<P>(r: GroupRunner<'static, P>) -> !` | `Ticker` with the group's period, absolute deadlines; each round calls `tick(now)`; rereads the period after a generation swap |
| `async fn run_service<P, L: LinkIo>(s: Service<'static, P>, links: L) -> !` | waits for `service_next`, for the next `service_deadline` deadline or for data on a link, and calls `link_*` + `poll` |
| `async fn run_supervisor<P, W: Watchdog>(io: Io<'static, P>, wd: W, every: Duration) -> !` | periodic `supervise`; feeds the watchdog only with `feed_watchdog` |
| `trait Watchdog { fn feed(&mut self); }` | implemented for the watchdogs of the supported HALs |
| `trait LinkIo` | gathers the transports of the links (`embedded-io-async` for streams, `Datagram` for UDP/CAN) |

### 2.12 Firmware mode

#### 2.12.1 What it is

`joker-micro` compiled as a complete firmware for a board: HAL, drivers, apps, link, persistence, OTA and the loop of §2.11.6, without integrator code. Two variants:

| Variant | Command | Generation |
|---|---|---|
| **firmware with an embedded generation** | `jk build --target mcu:<board>` | `const`, inside the image |
| **generic firmware** | `jk build --target mcu:<board> --generic` | read from the `GEN_A`/`GEN_B` area; the blob comes from `jk build --generation --firmware <firmware.jkfw>` |

The generic firmware is per (board, set of apps, set of drivers, capabilities): a single one for the 3,000 sites of round 5 of document 08. **It carries all the Joker Link transports** that the board declares in `[link] transports` (ref/03 §2.3.3), so that the same image serves a generation over serial, over USB CDC or over UDP/TCP: it is the generation that chooses the transport (`[link]`, ref/01 §5.22). A firmware with an embedded generation only carries the transports that generation uses, plus the board's `default` (maintenance mode). In `dev`, the firmware accepts an **unsigned** generation blob (`J0779` is `error-in-prod`); in `prod`, it refuses it at step 5 of §2.12.3. `jk flash` (probe-rs) writes the firmware, the blob and, with `--identity`, the identity area; OTA does the same over the link.

#### 2.12.2 Flash partitions

The board (MCU board profile, `ref/03`) declares the areas; the typical sizes are:

| Partition | Typical size (RP2350, 4 MiB) | Content |
|---|---|---|
| bootloader | 32 KiB | `embassy-boot` (or MCUboot on other boards), with the firmware public key |
| bootloader state | 8 KiB | the bootloader's A/B swap marks |
| active firmware | 1 MiB | the running image |
| `FW_STAGING` | 1 MiB | the image received by OTA |
| `GEN_A`, `GEN_B` | 64 KiB each | generation blobs |
| `PERSIST` | 64 KiB | persistent journal (§2.10) |
| `IDENTITY` | 8 KiB | node name, keys |

On an STM32G4 (512 KiB of internal flash) the same partitions shrink (firmware 192 KiB × 2, generations 16 KiB × 2, persistence 16 KiB) and `jk check` verifies that they fit (`J0781`).

#### 2.12.3 Boot

| # | Step | Failure → |
|---|---|---|
| 0 | the bootloader verifies the signature of the active firmware (and swaps A/B if one is pending) | previous firmware |
| 1 | clocks, monotonic timer, hardware watchdog (the board's timeout) | reset |
| 2 | the outputs stay in the **hardware reset state**, which the board declares safe (`reset_safe = true`; otherwise `J0786`, acceptable with `accept_unsafe_reset`, §2.16); no output pin is configured before step 6 | — |
| 3 | reads `IDENTITY` (name, link keys, trust bundle) and `PERSIST` (generation state, trips, alarms, parameters) | empty persistence: factory boot |
| 4 | chooses the generation: that of the active area; if it was `confirming` and the previous boot did not confirm, goes back to the other area (`rolled_back`) | the other area; with no valid one: **maintenance mode** |
| 5 | `jk_generation_open` (format, integrity, signature, anti-rollback, node, firmware, capabilities) | the other area; otherwise maintenance mode |
| 6 | builds the **pin table** from the board's `[pins]` (§2.11.3, pin ownership, without `steal`); node `init`: outputs in `safe` (A8), trips restored, apps initialized; only then do the drivers take their pins from the table and configure them, already with the safe value | maintenance mode (pin requested twice: `JK_E_GEN_FIRMWARE`) |
| 7 | starts the groups (interrupt executors, rate-monotonic priority), the service, the links and the supervisor | — |
| 8 | first tick | target ≤ 100 ms from reset on M1 (to be measured; `firmware.jkfw` key 16) |

**Maintenance mode:** without a valid generation, there are no signals or outputs. The firmware keeps the pins in the reset state, brings up the link with the board's default configuration (`ref/03`) and only accepts `HELLO`, generation or firmware `OTA` and state requests; it feeds the watchdog; `jokerd` sees it as a `Bad(config_error)` device.

#### 2.12.4 OTA and confirmation

- **Generation:** the blob arrives (link or `jk_ota_*`) at the inactive area, is verified with `min_seq = confirmed_seq + 1` and, in the firmware, swapped at the next tick boundary with the confirmation window of the message (`confirm_ms` = key 11 of the `OTA` `activate`, ref/07 §9.4), by `jk_generation_swap`. Without confirmation within the window, it goes back to the previous one hot. A restart during the window counts as a failure (step 4).
- **Firmware:** the image arrives at `FW_STAGING`, is verified (the COSE manifest of ref/07 §9.1 with a key of the `firmware` role; `board_id`; `security_version` ≥ the confirmed one, otherwise `JK_E_GEN_ROLLBACK`; and the firmware descriptor compatible with the active generation, otherwise `JK_E_GEN_FIRMWARE`), and the firmware marks the swap in the bootloader and restarts at a moment with no pending writes. The bootloader verifies the signature again with its own keys (ref/10 §3.6). The new firmware only marks itself as good after N clean ticks and the `HELLO` (the health test of document 01, §16); otherwise the bootloader reverts at the next restart.
- **Confirmation by `jk confirm`:** when the OTA is part of an activation of the Linux node with a confirmation window, the `confirm` reaches the MCU only after the node's `jk confirm` (ref/07 §9.4; ref/09 §5.13); until then, the generation or the firmware is on trial, and the window counts on the MCU.
- **Anti-rollback:** `confirmed_seq` only goes up; a blob with a `gen_seq` less than or equal to the confirmed one is refused (`JK_E_GEN_ROLLBACK`), except the active blob itself after a restart.

#### 2.12.5 WCET measurement: `jk bench --target mcu:<board>`

`jk bench --target mcu:<board>` flashes a measurement firmware onto the MCU (the board's generic one, with the manifest's apps and a bench executor), runs each cycle task at least 10,000 times with the inputs of the bench generator, and writes the measured maximum in `[wcet]` of the lock (ref/02 §2.1.13). Time is always measured with the core's **cycle counter** (`DWT_CYCCNT` on Cortex-M, `mcycle` on RISC-V, `CCOUNT` on Xtensa), converted by the board's `clock`; never by the host's clock. The result leaves the MCU according to the board's `[probe]` (ref/03 §2.3.3):

| `[probe] tool` | How it flashes and reads |
|---|---|
| `probe-rs` (default) | flashes and reads the counters through the probe (SWD/JTAG), without touching the link |
| `espflash` (board **without a probe**, like the bench's ESP32) | flashes over serial (`espflash`) and reads the results **through the same serial port**, in one CBOR record per task (`{task, n, min, max, p99}` in cycles) that the executor writes at the end; for the reading, the port is opened with DTR and RTS disabled, like the default of `[buses.*]` (ref/01 §5.6), so as not to restart the board |

Both paths give the same `[wcet]`: the lock does not distinguish the origin. At the end, `jk bench` flashes back the firmware that was there, if it managed to read it (otherwise, it warns: the MCU keeps the measurement firmware until the next `jk flash`).

#### 2.12.6 Required OTA receiver

**Rule (1.0):** every Joker Micro firmware, in firmware mode (with an embedded generation or generic) and in crate mode, carries the **OTA receiver** for generations and firmware: an MCU in the field must be able to receive new firmware without a visit. Concretely:

| Requirement | Check |
|---|---|
| the `ota` feature enabled (§2.11.2) | `compile_error!` in `joker-micro` and `J0795` in `jk build` |
| the board declares `FW_STAGING`, `GEN_A` and `GEN_B` with size for the image and for the largest blob, and a bootloader with A/B swap (§2.12.2) | `J0795` (missing partition), `J0781` (does not fit) |
| maintenance mode (§2.12.3) accepts `OTA` through the board's `[link] default` | it is part of the firmware; it is not configurable |
| in crate mode, the generated `joker-project` binds the OTA service to the loop of §2.11.6; the integrator cannot remove it without disabling the feature | `J0795` |

In **library mode** (C ABI), the firmware belongs to the integrator: the `jk_ota_*` are always in the ABI, but carrying them through to the bootloader is the integrator's work (document 19, §4.2). A board without flash for two images (A/B) is not a target of firmware mode in 1.0.

### 2.13 The generation blob (`*.jkgen`)

OCI media type `joker.generation.v1` (document 20, §3.2). It is the same in-memory structure that the core uses, serialized: the MCU uses it **in place** (XIP), without copying it or interpreting it into another form. The embedded generation is exactly the same format, with the `EMBEDDED` flag.

#### 2.13.1 General rules

| Rule | Value |
|---|---|
| endianness | **little-endian** in all integers and `f64` (IEEE 754). All v1 targets are little-endian; a big-endian target is `J0770` |
| alignment | the blob starts at an address that is a multiple of 8; each section starts at an offset that is a multiple of 8; the records of each section have a size that is a multiple of 8 (except `STRINGS`, `CODE` and `BLOBS`, which are bytes); the padding is 0 |
| offsets | relative to the start of the blob, `u32` |
| `strref` | 8 B: `u32` offset in `STRINGS`, `u16` length, `u16` 0. UTF-8 without NUL |
| `poolref` | 8 B: `u32` index in `U32POOL` (in elements), `u32` number of elements |
| indices | `u32`, `0xFFFFFFFF` = none |
| durations | `u32` µs, except the `_ms` fields (`u32` ms) and `stuck_detect_us` (`u64`) (`ref/00` §3) |
| values | `u64` with the bits of the type (`ref/00` §4.1) |
| path-hash | the first 8 B of the SHA-256 of the canonical path in UTF-8, read as a `u64` LE |

#### 2.13.2 Header (128 B)

| Offset | Type | Field | Rule |
|---|---|---|---|
| 0 | `[4]u8` | magic `"JKGN"` | |
| 4 | `u16` | `format` | 1 in v1 |
| 6 | `u16` | `header_size` | 128 |
| 8 | `u32` | `image_len` | bytes of `[0, image_len)`, a multiple of 8; it is what the SHA-256 covers |
| 12 | `u32` | `flags` | bit 0 `SIGNED` (there is a *trailer*), bit 1 `DEV` (`mode = "dev"`; a node in `prod` never sends a blob with this bit, `J0794`, §2.16), bit 2 `EMBEDDED`, bit 3 `NODE_BOUND`; others 0 |
| 16 | `u64` | `gen_seq` | increasing integer per node (`ref/00` §5) |
| 24 | `[32]u8` | `source_hash` | **the generation hash** of the MCU: the SHA-256 of the generation descriptor of ref/10 §2.6.2, computed over the MCU's effective manifest, the lock and the contracts. It is what `jk`, `jokerd` and the Deck show, what `HELLO` announces (key 6.2) and the `hash` of `mcu-generation` in the desired state |
| 56 | `[32]u8` | `registry_hash` | that of document 16, §7 (ordered list of `SignalId`s, types, groups) |
| 88 | `[8]u8` | `board_id` | first 8 B of the SHA-256 of `"<board>@<major>"` |
| 96 | `u16` | `expr_isa` | 1 in v1 (§2.13.4) |
| 98 | `u16` | `section_count` | |
| 100 | `u32` | `section_table_off` | 128 |
| 104 | `u32` | `trailer_off` | `image_len` if `SIGNED`, otherwise 0 |
| 108 | `u32` | `trailer_len` | |
| 112 | `u32` | `min_runtime` | `major << 16 \| minor` of the minimum `joker-micro` |
| 116 | `u32` | `image_crc32c` | CRC-32C of the image with this field at 0 (quick detection of corrupted flash, before the SHA-256) |
| 120 | `[8]u8` | reserved | 0 |

**Section table:** `section_count` entries of 16 B, sorted by `kind`: `u16 kind`, `u16 flags` (bit 0 `REQUIRED`), `u32 offset`, `u32 length` (bytes), `u32 count` (records; 0 for byte sections). A reader that finds an unknown `kind` with `REQUIRED` rejects it (`JK_E_GEN_VERSION`); without `REQUIRED`, it skips it (rule 2 of `ref/00` §1). All v1 sections are `REQUIRED`.

#### 2.13.3 Sections

The index of a record in a section **is** its identifier in the generation: record *n* of `SIGNALS` is `SignalId` *n*, and so on. Empty sections are omitted.

| `kind` | Section | Record | Content |
|---|---|---|---|
| 0x0001 | `STRINGS` | bytes | all strings, without repetition, in order of first reference |
| 0x0002 | `CAPS` | 8 B | needs of the generation: `u16 cap_id`, `u16 0`, `u32 value`, by increasing `cap_id` (next table) |
| 0x0003 | `REQUIRES` | 32 B | `u8 kind` (1 app, 2 driver), `u8 0`, `u16 major`, `u16 minor`, `u16 patch`, `strref name`, `[8]u8 hash` (apps: the `meta_hash`, the first 8 B of the SHA-256 of the `joker:meta` CBOR; drivers: the first 8 B of the digest of the driver descriptor that `jk build` planned with, ref/06 §2.3, §3.7; `extern`: 0), `[8]u8 0` |
| 0x0004 | `SIGNALS` | 40 B | `u64 path_hash`, `strref path` (canonical), `u8 type` (`ref/00` §6), `u8 direction` (0 input, 1 output, 2 derived, 3 class parameter), `u8 quantity` (0 none, 1 `active_power` … 7 `temperature_difference`, in the order of 16 §5), `u8 flags` (bit 0 has `AUX`, bit 1 exported in `HELLO`, bit 2 sent in `SAMPLES`, bit 3 text), `u32 max_age_us` (`0xFFFFFFFF` = none), `u32 aux` (index in `SIGNAL_AUX`), `strref unit`, `u32 0`. Sorted by canonical path (document 01, §3.1) |
| 0x0005 | `PATHS` | 24 B | `u64 path_hash`, `u32 SignalId`, `strref path` (only `u32` offset + `u16` length), `u16 flags` (bit 0 canonical, bit 1 instance member, bit 2 alias), `u32 0`; **all** paths (canonical and aliases, P1), sorted by `path_hash` for binary search |
| 0x0006 | `SIGNAL_AUX` | 48 B | `f64 limit_lo`, `f64 limit_hi` (NaN = none), `f64 acquisition_deadband` (0 = none), `u64 stuck_detect_us` (0 = none), `u32 debounce_us`, `u32 enum` (index in `ENUMS`), `u32 0`, `u32 0` |
| 0x0007 | `ENUMS` | 16 B | `strref name`, `u32 first variant`, `u16 n`, `u8 kind` (0 enum, 1 bits), `u8 0` |
| 0x0008 | `VARIANTS` | 16 B | `strref name`, `u64 value` (enum) or bit index (bits) |
| 0x0009 | `GROUPS` | 48 B | `strref name`, `u32 period_us`, `u8 overrun` (0 `skip`, 1 `catch_up`, 2 `degrade`, 3 `trip`), `u8 flags`, `u16 trip_after`, `u32 first task`, `u16 n_tasks`, `u16 degrade_ticks`, `poolref latch` (latched SignalIds, sorted), `poolref outputs` (outputs it owns), `u32 trace_bytes`, `u32 0` |
| 0x000A | `TASKS` | 8 B | `u16 app`, `u16 task` (index in the app's metadata), `u16 flags` (bit 0 `optional`), `u16 0`; in tick order (`order`) |
| 0x000B | `WRITERS` | 24 B | `strref identity` (`app:<name>`, `extern:<app>`, `bridge:<link>:<level>`, `impl:<instance>`), `u8 writer_kind` (`ref/00` §6), `u8 level` (1–16), `u16 flags`, `u32 lease_us` (0 = A9), `u32 owner` (app, link or `NONE`), `u8 role`, `[3]u8 0` |
| 0x000C | `OUTPUTS` | 152 B | see the table below |
| 0x000D | `OUTPUT_WRITERS` | 8 B | `u32 writer`, `u8 level`, `u8 flags`, `u16 0`: the arbiter levels of each output (A5; uniqueness was already verified by `J0210`) |
| 0x000E | `ATOMIC` | 32 B | `strref name`, `poolref members` (in `order`), `u8 on_rate` (0 `independent`, 1 `proportional`), `u8 has_commit`, `u16 0`, `u32 commit signal`, `u64 commit value` |
| 0x000F | `U32POOL` | 4 B | lists of `u32` referred to by `poolref` |
| 0x0010 | `PROGRAMS` | 32 B | `u32 code_off`, `u32 code_len`, `u32 state_off`, `u16 state_len`, `u8 max_stack` (≤ 32), `u8 result type`, `u8 kind` (1 `calc`, 2 alarm `when`, 3 alarm `suppress_when`, 4 `interlock`, 5 `range` limit, 6 `reset_when`, 7 query, 8 method assignment, 9 action permissive, 10 argument of a call to a member method), `u8 flags`, `u16 n_args`, `u32 evaluation group`, `u32 target` (signal, alarm, output or member; in `kind` 10, the class member), `u32 0` (in `kind` 10, the index in `METHODS` of the called method) |
| 0x0011 | `CODE` | bytes | bytecode (§2.13.4) |
| 0x0012 | `CONSTS` | 16 B | `u8 type`, `[7]u8 0`, `u64 bits` |
| 0x0013 | `ALARMS` | 56 B | `strref name`, `u32 when`, `u32 suppress_when` (programs), `u32 delay_on_us`, `u32 delay_off_us`, `u8 severity`, `u8 flags` (bit 0 from the app), `u16 0`, `u32 state signal`, `u32 active signal`, `strref message` (template with `{path}`), `poolref trips` (outputs of `actions.trip`), `u32 0` |
| 0x0014 | `CLASSES` | 56 B | `strref "package/name"`, `u16 major`, `u16 minor`, `u16 patch`, `u16 n_capabilities`, `[32]u8 contract hash` (17 §6), `u32 first member`, `u16 n_members`, `u16 0` |
| 0x0015 | `MEMBERS` | 24 B | `strref name`, `u8 kind` (1 `in`, 2 `out`, 3 `param`, 4 `event`, 5 `action`, 6 `method`, 7 `query`, 8 class member), `u8 type`, `u8 capability` (`0xFF` = none), `u8 flags` (bit 0 `@optional`, bit 1 required for `health`, bit 2 `atomic`, bit 3 `@long`), `u32 aux` (method: index in `METHODS`; action: `@timeout` in ms; event: offset of the schema in `BLOBS`; class member: index in `CLASSES`), `u32 aux2` (class member: `u16 min`, `u16 max` of the collection, or `1`, `1` in a fixed member), `u32 0` |
| 0x0016 | `METHODS` | 16 B | `u32 class`, `u16 member`, `u8 n_assignments` (assignments and call arguments), `u8 flags` (bit 0 `atomic`), `u32 first program` (consecutive: `kind` 8 per assignment, `kind` 10 per argument of a call to a member method, 17 §4.1; a call without arguments takes one `kind` 10 with `code_len` 0), `u32 order` (index in `U32POOL` of `n_assignments` members) |
| 0x0017 | `INSTANCES` | 32 B | `strref name`, `u32 class`, `u32 map` (index in `U32POOL` of `n_members` SignalIds; `NONE` for members without a signal; in a class member, the index in `U32POOL` of the list of bound instances, preceded by its length), `u32 capabilities` (bits), `u8 origin` (0 implicit, 1 composite, 2 virtual), `u8 flags` (bit 0: member only, no top-level name on the link, ref/07 §6.3), `[2]u8 0`, `u32 implemented by` (app or `NONE`), `u32 0` |
| 0x0018 | `ACTIONS` | 32 B | `u32 instance`, `u16 member`, `u8 implementer` (1 driver, 2 `extern`, 3 app, 4 link), `u8 minimum role`, `u32 permit` (program), `u32 timeout_ms`, `u32 dedup_window_ms`, `u32 implementer index`, `u32 queue` (device that serializes), `u32 0` |
| 0x0019 | `EVENTS` | 24 B | `u32 instance` (or `NONE` for an app event), `u16 member`, `u16 flags` (bit 0 for the link, bit 1 for the firmware queue), `u32 origin` (app port), `u32 schema` (offset in `BLOBS`), `u32 schema length`, `u32 queue depth` |
| 0x001A | `APPS` | 48 B | `strref name`, `u16 major`, `u16 minor`, `u16 patch`, `u16 flags` (bit 0 `extern`), `u32 writer`, `u32 first port`, `u16 n_ports`, `u16 n_parameters`, `u32 first parameter`, `u32 kv_bytes`, `[8]u8 meta_hash`, `u32 0` |
| 0x001B | `APP_PORTS` | 24 B | `u32 target` (signal, instance, alarm or `NONE` if optional and unbound), `u8 kind` (1 input, 2 output, 3 internal, 4 published, 5 input event, 6 output event, 7 equipment, 8 alarm, 9 element of `Many`), `u8 conversion` (0 none, 1 affine), `u16 index in Many`, `f64 factor`, `f64 offset` — the unit conversion of the binding (document 02, §3) already resolved |
| 0x001C | `PARAMS` | 40 B | `strref name`, `u32 app`, `u8 type`, `u8 flags` (bit 0 changeable at runtime, bit 1 persistable), `u16 0`, `u64 value`, `f64 minimum`, `f64 maximum` (NaN = none) |
| 0x001D | `DEVICES` | 32 B | `strref name`, `strref driver`, `u16 driver major`, `u16 flags`, `u32 config` (offset in `BLOBS`), `u32 length`, `u32 descriptor` (offset in `BLOBS` of the 32 B of the SHA-256 of the driver descriptor that `jk build` planned with, ref/06 §2.3; `NONE` for `extern`; the first 8 B are repeated in the driver's `REQUIRES` record) — the configuration is the `micro-device-config` of `spec/cddl/jdp.cddl` (ref/06 §3.7): the descriptor digest (key 0, the same as this field), the `device-spec` with the descriptor's `config` schema, the `point-spec`s with the MCU's `SignalId`s, the read plan and the bus. `HELLO` summarizes the descriptors in key 15 (ref/07 §6.4) |
| 0x001E | `BLOBS` | bytes | deterministic CBOR: driver and link configurations, event schemas, each item aligned to 8 |
| 0x001F | `LINKS` | 32 B | `strref name`, `u8 transport` (1 `uart`, 2 `rs485`, 3 `can`, 4 `usb_cdc`, 5 `udp`, 6 `tcp`), `u8 framing` (1 stream, 2 datagram), `u8 authentication` (0 none, 1 optional, 2 required), `u8 peer` (0 Linux node, 1 Deck), `u16 allowed levels` (bit *n*−1 = level *n*), `u16 mtu`, `u32 first writer` (one per allowed level, consecutive), `u32 config` (offset in `BLOBS`), `u32 length`, `u32 0` |
| 0x0020 | `NODE` | 48 B | `strref node name` (if `NODE_BOUND`), `u32 service_deadline_ms`, `u8 clock source` (0 none, 1 `link`, 2 `extern`), `u8 flags` (bit 0 `freeze_on_trip`), `u16 0`, `u32 max_holdover_ms` (`0xFFFFFFFF` = no limit), `u32 trace_bytes` total, `u32 SAMPLES ring` (entries), `u32 firmware event queue`, `u32 default confirmation (ms)`, `[12]u8 0` |
| 0x0021 | `CATALOG` | bytes | the Joker Link catalog (ref/07 §6.3), **as is**: what `CATALOG` transfers; its SHA-256 is the catalog hash of `HELLO`. Present if the generation has links |
| 0x0022 | `SECRETS` | bytes | a `secret-bundle` (ref/02 §2.4, `spec/cddl/secrets.cddl`) with the entries encrypted for the MCU's X25519 key (record 19), only with the MCU's `wrap`; present on standalone MCUs with secrets (ref/04 §4.14) and on any M2 MCU whose `[net.wifi]` has `password` (ref/01 §5.22) |
| 0x0023 | `TRUST` | bytes | chain of new trust bundles (`trust-chain` in the CDDL), in version order; the MCU installs them (record 18) before verifying the blob's signature (§2.13.5) |
| 0x0024 | `NET` | bytes | `micro-net` in deterministic CBOR (`spec/cddl/joker-micro.cddl`): the Wi-Fi configuration of `[net.wifi]` (ref/01 §5.22): `ssid`, `country`, `hostname` and the **name** of the password secret, which is looked up in `SECRETS`; never the value. Only on M2 MCUs with `[net.wifi]` |
| 0x0040–0x0043 | reserved | — | `MODELS`, `BLOCKS`, `STREAMS` (v1.1, "Ready for ML"), `SCHEDULES` (scheduled writes, M6, v1.1) |
| 0x0044 | reserved | — | `WASM_APPS` (apps in `wasmi`, v1.4) |
| 0x8000–0xFFFF | private | — | never `REQUIRED`; ignored by `joker-micro` |

**`OUTPUTS` (152 B):**

| Offset | Type | Field |
|---|---|---|
| 0 | `u32` | signal |
| 4 | `u32` | owning group (A11) |
| 8 | `u8` × 4 | `on_range` (0 `clamp`, 1 `reject`, 2 `trip`), `on_interlock` (0 `safe`, 1 `hold`), `safe_kind` (0 value, 1 `relinquish`), `safer` (0 `equal`, 1 `lower`, 2 `higher`) |
| 12 | `u32` | flags: bit 0 `permit_uncertain`, bit 1 has `rate`, bit 2 has `safe_ramp`, bit 3 has `deadband`, bit 4 has `verify`, bit 5 `pulse`, bit 6 persistent trip (default 1) |
| 16 | `u64` | `safe` value |
| 24 | 16 B | lower limit: `u8 kind` (0 none, 1 constant, 2 program), `[3]u8 0`, `u32 program`, `f64 constant` (E6) |
| 40 | 16 B | upper limit, likewise |
| 56 | `f64` | `rate` (unit/s) |
| 64 | `f64` | `safe_ramp` |
| 72 | `f64` | `deadband` |
| 80 | `u32` | `min_interval_us` |
| 84 | `u32` | `grace_us` (E4) |
| 88 | `u32` | `interlock` (program) |
| 92 | `u32` | `reset_when` (program) |
| 96 | `poolref` | `requires_good` |
| 104 | `u32` | `max_lease_ms` (A9; default 15 min) |
| 108 | `u32` | `max_manual_ms` (A10; default 8 h) |
| 112 | `u16`, `u16`, `u32` | `max_actuations`: count, 0, window in s |
| 120 | `u32` | `verify.readback` (signal) |
| 124 | `u32` | `verify.within_us` |
| 128 | `f64` | `verify.tolerance` |
| 136 | `u32` | `pulse_us` |
| 140 | `u32` | atomic group (index in `ATOMIC`) |
| 144 | `u32` | first in `OUTPUT_WRITERS` |
| 148 | `u16`, `u16` | number of writers; allowed manual levels (bits 3–5) |

**Capabilities (`cap_id`).** The same numbering in `CAPS` (what the generation needs), in the firmware descriptor (the build's maximum, §2.14) and in the keys of `[target.capacity]` (§2.17), with the names in `snake_case`:

| id | name | id | name | id | name |
|---|---|---|---|---|---|
| 1 | `signals` | 15 | `members` | 29 | `u32pool` |
| 2 | `paths` | 16 | `instances` | 30 | `image_entries` |
| 3 | `outputs` | 17 | `methods` | 31 | `text_signals` |
| 4 | `groups` | 18 | `actions` | 32 | `inbox` (per group) |
| 5 | `tasks` | 19 | `events` | 33 | `method_queue` (per group) |
| 6 | `writers` | 20 | `apps` | 34 | `action_pending` |
| 7 | `output_writers` | 21 | `app_ports` | 35 | `event_queue` |
| 8 | `atomic` | 22 | `params` | 36 | `trace_bytes` |
| 9 | `programs` | 23 | `devices` | 37 | `kv_bytes` |
| 10 | `code_bytes` | 24 | `links` | 38 | `window_samples` |
| 11 | `consts` | 25 | `strings_bytes` | 39 | `manual_writes` |
| 12 | `expr_state_bytes` | 26 | `blobs_bytes` | 40 | `app_state_bytes` |
| 13 | `alarms` | 27 | `enums` | | |
| 14 | `classes` | 28 | `variants` | 41 | `radio_heap_bytes` (only ESP32 with Wi-Fi, §2.15.2) |

**`J0710` and `JK_E_GEN_CAPACITY`.** A generation fits in a build if, for each `cap_id`, the need (`CAPS`) is ≤ the descriptor's maximum. `jk check` and `jk build --generation` make this comparison before writing (`J0710`, with the list of exceeded capabilities and the RAM and flash estimate per component of §2.9.1); `joker-micro` repeats it in `jk_generation_open` (`JK_E_GEN_CAPACITY`) and never trusts `jk`. With an embedded generation, the build's capabilities are those of the generation plus the headroom of `[target.capacity]`.

#### 2.13.4 Expression bytecode (ISA 1)

The same bytecode is evaluated by `joker-expr` on Linux (in memory) and on the MCU (in the blob). The semantics are those of document 02, §10 and of document 16, §3–4; the ISA only fixes the encoding.

**Machine:** a stack of 16 B cells (`u64` bits, `u8` type, `u8` quality code, `u8` reason, `u8` flags, `u32` 0), depth ≤ `max_stack` ≤ 32. Each program has its state (`state_off`, `state_len`) in the node: windows, `held` timers, `rose`/`fell`/`hyst` bits. The evaluation context gives the group's snapshot, the latch instant and, in method assignments, the arguments and the caller.

**Encoding:** a `u8` *opcode*, followed by the operands in little-endian, without alignment. Jumps are relative to the end of the instruction and **forward only**: there are no loops (expressions are total), and the time of a program is bounded by its size.

| Op | Mnemonic | Operands | Stack | Semantics |
|---|---|---|---|---|
| 0x01 | `CONST` | `u16 k` | → v | `CONSTS[k]`, `Good` |
| 0x02 | `TRUE` / 0x03 `FALSE` | — | → b | |
| 0x04 | `LOAD` | `u32 signal` | → s | sample from the snapshot, with aging; an `out` gives the **applied** value (17 §11) |
| 0x05 | `OWN` | `u32 signal` | → v | `own(x)` (only in method assignments; 16 §4) |
| 0x06 | `ARG` | `u8 i` | → v | argument *i* of the method |
| 0x08 | `AGE` | `u32 signal` | → f64 s | `age(x)` |
| 0x10 | `AFFINE` | `u16 ka`, `u16 kb` | x → x·a+b | unit conversion resolved in `jk check` |
| 0x11 | `TOF` | — | i → f64 | integer to f64 |
| 0x20–0x24 | `ADD`, `SUB`, `MUL`, `DIV`, `NEG` | — | a b → r | in f64; division by zero, root of a negative, `NaN` → `Bad(nan)` (16 §4); quality: the worst (16 §3.2) |
| 0x28–0x2D | `EQ`, `NE`, `LT`, `LE`, `GT`, `GE` | — | a b → bool | integers with the same signedness compare exactly; the rest in f64 |
| 0x30 | `NOT` | — | b → b | |
| 0x31 | `JFG` | `i16` | b → b | if the top is `false` with `Good`, jumps (keeps it): the short-circuit of `&&` (02 §4) |
| 0x32 | `JTG` | `i16` | b → b | if the top is `true` with `Good`, jumps: that of `\|\|` |
| 0x33 | `AND` / 0x34 `OR` | — | a b → b | logical, with the symmetric short-circuit of 16 §3.2: a `false (Good)` on either side decides the `AND`, a `true (Good)` decides the `OR`; otherwise, the worst quality (`JFG`/`JTG` only spares the evaluation of the right side) |
| 0x38 | `JMP` | `i16` | — | forward only |
| 0x39 | `IF` | `i16 else`, `i16 end` | c → | `c` `Bad` → pushes `Bad` of the result type and jumps to the end; otherwise keeps the quality of `c` and continues (`true`) or jumps to `else` (16 §3.2) |
| 0x3A | `ENDIF` | — | r → r | if `c` was `Uncertain`, the result is, at most, `Uncertain` |
| 0x40–0x45 | `ABS`, `MIN`, `MAX`, `CLAMP`, `SQRT`, `POW` | — | … → r | |
| 0x48 | `GOOD` | — | s → bool | `good(x)`, always `Good` |
| 0x49 | `QUALITY` | — | s → u64 | `quality(x)` (code), always `Good` |
| 0x50–0x53 | `WAVG`, `WMIN`, `WMAX`, `WRATE` | `u16 state`, `u32 window_us` | x → r | windows (≤ 15 min, `J0305`); quality by the 10% rule (16 §3.2) |
| 0x58 | `HELD` | `u16 state`, `u32 dur_us` | b → b | 16 §3.2 |
| 0x59 | `ROSE` / 0x5A `FELL` | `u16 state` | b → b | |
| 0x5B | `HYST` | `u16 state` | x lo hi → b | |
| 0x5C | `BIT` | — | x n → b | |
| 0x60 | `AGG` | `u8 f`, `u32 list`, `u16 n` | → r | `any` 0, `all` 1, `count` 2, `sum` 3, `maxs` 4, `mins` 5 over `n` signals of `U32POOL` |
| 0x70 | `RET` | — | r → | result of the program |

**Verifier** (in `jk_generation_open`; failure → `JK_E_GEN_FORMAT`): *opcodes* known in the blob's ISA; jumps inside the program and forward; stack depth ≤ `max_stack` on all paths; signals, constants, lists and state within the sections and within `state_len`; `OWN` and `ARG` only in method assignments; the program ends in `RET` with the declared type.

A new ISA (2, …) only adds *opcodes*; a firmware that only knows 1 refuses a blob with `expr_isa = 2` (`JK_E_GEN_VERSION`), and `jk build --generation` does not produce it for that firmware (`J0789`).

#### 2.13.5 Signature

A signed blob has, after the image (`trailer_off = image_len`), a **COSE_Sign1** (RFC 9052, `ref/00` §7) whose payload is the **Joker Link OTA manifest** (`ota-manifest`, ref/07 §9.1; `generation-trailer` in the CDDL), which identifies the image by its SHA-256. It is **the only form** of signature of an MCU generation: OTA over the link sends this manifest in `begin`, and the MCU writes it as the *trailer*; `jk_ota_*` receives the whole `.jkgen` file (image and *trailer*); boot verifies it again. The image stays in place, aligned and mappable, and the Ed25519 verification runs over a few dozen bytes.

| Part | Content |
|---|---|
| protected header | `{1: -8 (EdDSA), 4: kid}`; `kid` = the first 16 bytes of the SHA-256 of the public key (ref/02 §2.5) |
| unprotected | `{}` |
| payload (`ota-manifest`) | `1` = 1 (`generation`), `2` target, `3` the `gen_seq` in decimal, `4` = `image_len`, `5` SHA-256 of `[0, image_len)`, `6` = 0, `7` = `gen_seq`, `8` = `source_hash`, `9` capabilities hash of the target firmware (§2.14), `12` = `board_id`, `13` = `format`, `? 14` node name, `? 15` = `registry_hash` |
| signature | Ed25519 over the `Sig_structure` `["Signature1", protected, h'', payload]` |

Verification requires that `4`, `5`, `7`, `8`, `12`, `13` (and `15`, if present) match the image and the header, that `9` be the capabilities hash of this firmware (otherwise `JK_E_GEN_CAPACITY`) and, if there is a `14`, that it be the node name in the `IDENTITY` area (`JK_E_GEN_NODE`).

**Who signs** (D7, ref/00 §7.1): the generation, a key with the `config` role; the firmware, a key with the `firmware` role (the same manifest, with `1` = 2 and the firmware's `security_version` in `6`). The keys and roles come from the MCU's **trust bundle** (record 18 of the `IDENTITY` area, ≤ 2 KiB, the micro profile of the `trust-bundle` of ref/10 §2.4.3); without a bundle, the anchors of `platform->trust_keys` are valid for all roles (ref/02 §2.5.4). A new bundle arrives in the `TRUST` section of a blob (MCUs behind a node) or through the agent (standalone MCUs, ref/10 §2.15), and is only accepted if it is signed by an anchor or by a `root` key of the installed bundle and has a greater `version`; the MCU installs the chain of the `TRUST` section **before** verifying the signature of the blob that carries it.

#### 2.13.6 Determinism of `jk build --generation`

The same set (effective manifest, `joker.lock`, contracts, firmware descriptor, key) **always gives the same bytes**:

- records in canonical order: signals by path; groups, writers, apps, classes, instances, alarms, devices and links by name; members in the declaration order of the canonical contract (17 §6); programs in the order of the sections that refer to them;
- strings deduplicated in order of first reference, in section order;
- zero padding, deterministic CBOR, no dates, file paths or machine names;
- the format and the ISA are the **highest that the firmware supports** (descriptor, keys 5 and 6) and that the generation needs; never higher;
- Ed25519 is deterministic (RFC 8032).

`jk build --generation --check-reproducible` builds twice and compares. Conformance has reference blobs: the same project must give the same bytes on any machine.

### 2.14 Firmware descriptor (`firmware.jkfw`)

Each MCU build (firmware, library or crate) produces a descriptor in deterministic CBOR (`firmware-descriptor` in the CDDL), which also stays inside the image (link section `.joker.fw`) and is announced in `HELLO`. It is the contract between a generic firmware and the blobs it accepts.

| Key | Content |
|---|---|
| 1 | `joker-micro` version (SemVer) |
| 2 | ABI version, packed |
| 3, 4 | board and `board_id` |
| 5, 6 | highest blob format and ISA it understands |
| 7 | maximum capabilities (`cap_id` → value) |
| 8 | linked apps: `[name, version, meta_hash]` |
| 9 | drivers: `[name, major, descriptor digest]` (the digest is missing only for `extern`) |
| 10 | link transports |
| 11 | features |
| 12 | interface hash (library and crate; zeros in firmware mode) |
| 13 | stack per context |
| 14 | flash areas (firmware mode) |
| 15 | FPU, the same enum as the boards' `fpu` field (ref/03 §2.3): `"none"` (no FPU: `f32` and `f64` in software, `J0780` and `J0721`), `"sp"` (single precision in hardware; `f64` in software, `J0721`), `"sp+dcp"` (single precision plus a double-precision coprocessor, like the RP2350's DCP: `f64` is accelerated and does not give `J0721`), `"dp"` (single and double in hardware) |
| 16 | measured time between reset and the first tick, ms |
| 17 | `security_version` of the firmware (the security counter, anti-rollback; ref/07 §9.5) |
| 18 | version of the compiled **core** (`"<maj>.<min>"` of `joker-core`); it is what the lock pins in `[mcu.<d>] core` and what `jk check` uses for `J0796` (§3.11). Without it (firmware older than the rule), it counts as the major.minor of key 1 |

- The **capabilities hash** is the SHA-256 of the deterministic CBOR of key 7. It goes in `HELLO` (key 5.5) and in the manifest of a generation (key 9): a blob made for another set of capabilities is refused before spending flash.
- The **descriptor digest** is the SHA-256 of the whole descriptor; it goes in `HELLO` (key 5.9) and in the report to the Deck (ref/10 §2.10.1, `mcus`).
- The drivers (key 9) carry the descriptor digest of each driver (ref/06 §2.3), so that `jk build --generation` rejects a plan made with another descriptor (`J0773`).

`jk` shows it in JSON with the same fields (`jk inspect firmware.jkfw`).

### 2.15 Targets and budgets

#### 2.15.1 Target classes (19 §3.1)

| Target | Class | Core | FPU | RAM | Flash | Default capabilities (signals / outputs / groups / instances / apps) | HAL (example `[board] chip`), feature |
|---|---|---|---|---|---|---|---|
| RP2350 | **M1** (reference) | 2× Cortex-M33, 150 MHz | `sp+dcp` | 520 KiB | 4 MiB (external, XIP) | 256 / 64 / 8 / 32 / 4 | `embassy-rp` (`rp235xa`), `chip-rp` |
| STM32G4 (G474) | M1 | Cortex-M4F, 170 MHz | `sp` | 128 KiB | 512 KiB | 128 / 32 / 4 / 16 / 2 | `embassy-stm32` (`stm32g474re`), `chip-stm32` |
| ESP32-C6 | **M2** | RISC-V, 160 MHz | **`none`** | 512 KiB | 4 MiB (external) | 256 / 64 / 8 / 32 / 4 | `esp-hal` (`esp32c6`), `chip-esp` |
| ESP32-S3 | M2 | 2× Xtensa LX7, 240 MHz | `sp` | 512 KiB (+ PSRAM) | 8 MiB (external) | 512 / 128 / 8 / 64 / 8 | `esp-hal` (`esp32s3`), `chip-esp` |
| ESP32 (original) | M2, **development only** (document 23) | 2× Xtensa LX6, 240 MHz | `sp` | 520 KiB | 4 MiB (external) | 256 / 64 / 4 / 32 / 4 | `esp-hal` (`esp32`), `chip-esp` |
| STM32H5 (H563) | M2 | Cortex-M33, 250 MHz | `sp` | 640 KiB | 2 MiB | 512 / 128 / 8 / 64 / 8 | `embassy-stm32` (`stm32h563zi`), `chip-stm32` |
| STM32H7 (H743) | M2 | Cortex-M7, 480 MHz | `dp` | 1 MiB | 2 MiB | 1024 / 256 / 8 / 128 / 8 | `embassy-stm32` (`stm32h743zi`), `chip-stm32` |

The default capabilities come from the board profile (`ref/03`) and are overridden with `[target.capacity]`. The version of each HAL is **exact** and pinned in the workspace's `Cargo.lock`, and changes once per phase (document 13, §5.1); `esp-hal` uses `unstable` for the ADC, touch and the watchdog, isolated in `chip-esp`. The ESP32-C6 has no FPU: all floating point is in software, and apps with `f32` get the warning `J0780`.

#### 2.15.2 Reference budget (to be measured in Phase 2)

Configuration of document 19, §3.1: core + Joker Link (UART, with encryption) + Modbus RTU + 1 app + 64 signals (16 outputs, 2 groups), on an RP2350. Target: **≤ 160 KiB of flash and ≤ 48 KiB of RAM** (exit gate 4 of Phase 2, document 11).

| Component | Flash (target) | RAM (target) |
|---|---|---|
| core (registry, arbiter, envelope, alarms, classes, methods) | 44 KiB | — |
| `joker-expr` (VM, windows) | 12 KiB | — |
| Joker Link (CBOR, COBS, CRC) | 14 KiB | 3 KiB (1 KiB buffers, state) |
| encryption and signature (ChaCha20-Poly1305, Ed25519 verification only, SHA-256/512) | 24 KiB | — |
| Embassy + HAL + timers | 18 KiB | 2 KiB |
| Modbus RTU driver | 8 KiB | 0.5 KiB |
| persistence + OTA | 7 KiB | 0.5 KiB (page buffer) |
| app (code) | 4 KiB | 1 KiB (state) |
| `libm` f32, minimal formatting | 5 KiB | — |
| generation (64 signals, embedded) | 6 KiB | — |
| slots (64 × 32 B) and images (2 groups × 32 ports × 16 B) | — | 3 KiB |
| outputs (16 × (96 + 3 × 24) B) | — | 2.7 KiB |
| expressions, queues (boxes, methods, events, actions) | — | 4 KiB |
| trace (U9) | — | 16 KiB |
| stacks (service 4 KiB, 2 groups 2 × 1.5 KiB, ISR 1 KiB) | — | 8 KiB |
| **Total** | **142 KiB** | **40.7 KiB** |

If the budget fails, the first cut is the trace (8 KiB) and the second the Ed25519 verification in firmware without a generation in flash (`gen-flash` feature).

**The declared exception: the ESP32 radio.** The core does not allocate, but the ESP32's Wi-Fi (`esp-radio`) needs a heap (`esp-alloc`) and a preemptive scheduler (`esp-rtos`). The rule:

| Rule | Why |
|---|---|
| the heap and `esp-rtos` only exist in the firmware of an ESP32 board with `[net] wifi = true` and a `[net.wifi]` in the MCU manifest (ref/01 §5.22); without Wi-Fi, they do not go into the image | the other targets do not pay the cost |
| the heap has a **fixed size**, declared in the board (`[capacities] radio_heap_bytes`, default 72 KiB), belongs only to the radio and stays outside the core: no core crate, driver or app accesses it (the `#[global_allocator]` is only visible in the `chip-esp` module) | the invariant "the core does not allocate" remains true |
| the heap and the `esp-rtos` stacks **count in `J0710`**, added to the generation's RAM | the generation must not fit "on paper" and fail with the radio on |
| the radio tasks run at a priority below the group executors (the board's `[irq] reserved`, ref/03 §2.3.3) | the radio does not preempt the cycle |
| the **group jitter is measured with the radio active** (UDP traffic on the link), as well as with it off, at gate 5 of Phase 2 and in `jk bench --target mcu` | interference from the radio's scheduler is the risk (document 25, §5) |

The `esp-radio` version (beta in 2026) is pinned exactly in the workspace (document 13, §5.1).

#### 2.15.3 Stack and time

| Target | Metric | Target (to be measured) |
|---|---|---|
| RP2350 | tick jitter at 1 kHz, p99.9 | ≤ 10 µs (gate 5 of Phase 2) |
| RP2350 | `jk_tick` of 64 signals, 16 outputs, 1 simple app | ≤ 50 µs |
| RP2350 | envelope + arbiter per output | ≤ 1 µs |
| RP2350 | `jk_set_input_f32` | ≤ 0.5 µs |
| RP2350 | `jk_generation_open` of a 16 KiB blob (SHA-256 + Ed25519) | ≤ 30 ms |
| M1 | reset → first tick (without link) | ≤ 100 ms |
| all | maximum cycle | ~10 kHz with simple logic (19 U4) |

### 2.16 New `J07xx` codes

Errors of `jk check` and `jk build` for MCU targets. `J0710`, `J0720` and `J0721` come from document 19 and are defined here (`J0720` also in ref/04 §4.16).

| Code | Severity | Short message |
|---|---|---|
| `J0710` | error | the generation does not fit in the capabilities of the firmware or the target (with the exceeded capabilities and the RAM and flash estimate, §2.13.3; on an ESP32 with Wi-Fi, it includes the radio heap, §2.15.2) |
| `J0720` | error | API or manifest section unavailable on Joker Micro (§2.11.5, ref/04 §4.14) |
| `J0721` | warning | `f64` on an MCU without a double-precision FPU (`fpu` = `none` or `sp`, §2.14) |
| `J0770` | error | unknown or unsupported MCU target (board without an MCU profile, big-endian architecture) |
| `J0771` | error | `--generation` without a valid firmware descriptor (`--firmware`) |
| `J0772` | error | the generation uses an app that the firmware does not contain (different name, major or `meta_hash`) |
| `J0773` | error | the generation uses a driver that the firmware does not contain (different name, major or descriptor digest) |
| `J0774` | error | estimated stack above the budget of a context |
| `J0775` | error | collision of generated identifiers (C or Rust) |
| `J0776` | error | more cycle groups than the target supports (8 in v1) |
| `J0777` | error | group period below the target minimum (100 µs) or not representable in the timer |
| `J0778` | error | persistence requested (trips, `kv`, persistent parameters) without a persistence area |
| `J0779` | error-in-prod | unsigned generation blob |
| `J0780` | warning | software floating point (`f32`) on a target without an FPU |
| `J0781` | error | blob or firmware larger than its flash area |
| `J0782` | error | signal type unavailable in Joker Micro v1 (block, stream) |
| `J0783` | error | scheduled writes (`schedule_depth > 0`, `timed`) on an MCU: they arrive in 1.1 |
| `J0784` | error | app `kv` above 16 KiB |
| `J0785` | error | `extern` device signal without `type`, `direction` or (if numeric) `unit` |
| `J0786` | error-in-prod | board without safe outputs at reset (`reset_safe = false`); acceptable with a justification through `[target] accept_unsafe_reset` (see after the table) |
| `J0787` | warning | generated identifier with more than 63 characters |
| `J0788` | error-in-prod | generation in flash not bound to a node (`node`) |
| `J0789` | error | the generation needs a blob format, ISA or `min_runtime` above what the firmware supports |
| `J0790` | error | required entry of the external interface with no match in the generation |
| `J0791` | warning | stack of a context not measurable (recursion or indirect calls); the budget is not verified |
| `J0792` | warning | estimated flash wear: life below 10 years with the declared rates |
| `J0793` | warning | optional entry of the external interface with no match in the generation |
| `J0794` | error | node in `prod` with an MCU whose generation is `dev` (the MCU manifest has `mode = "dev"`, or the blob has the `DEV` bit) |
| `J0795` | error | Joker Micro firmware without an OTA receiver: `ota` feature disabled, or board without `FW_STAGING`, `GEN_A`/`GEN_B` or an A/B bootloader (§2.12.6) |
| `J0796` | error | the generation uses features that the MCU's core (`[mcu.<d>] core` in the lock) does not know (ref/02 §2.1; §3.11) |

`J0797`–`J0799` remain free.

**The MCU in a node in `prod`.** A node in `prod` only sends `prod` generation blobs to an MCU (bit `DEV` of the header at 0, §2.13.2). The `jk check` of the node manifest verifies this from the MCU manifest (the `micro` key of the `joker-link` device, ref/01 appendix A) and gives `J0794`; `jokerd` verifies it again before carrying a blob over the link (generation `OTA`, ref/07 §9) and rejects the node's generation with the same code, and so does the Deck, at validation. A node in `dev` carries both. The MCU does not decide this by itself: what it accepts is its own policy (the `min_mode` of the trust bundle, ref/02 §2.5.4).

**`J0786` with a justification.** With the MCU manifest in `prod`, `J0786` is an error. A board without safe outputs at reset may be accepted with a justification, like the missing fallback (`accept_missing_fallback`, ref/01 §5.10): the key `accept_unsafe_reset = "<justification>"` in the `[target]` of the MCU manifest (ref/01 §5.22) turns `J0786` into a warning, and `jk lock` copies it to `[accepted] accept_unsafe_reset."<board>"` of that manifest's lock (ref/02 §2.1.14). The acceptance does not change the firmware: the pins stay in the hardware reset state until step 6 of boot (§2.12.3); it only records, signed with the generation, who accepted it and why.

### 2.17 What this asks of the manifest and the boards

This document defines no manifest or board keys: they are in `ref/01` (`[target]`, `[target.capacity]`, `[target.extern]`, `[target.micro]`, `[link]` in §5.22, `extern` apps, `[clock] sync`) and in `ref/03` (MCU boards). The table summarizes what `jk build` reads from them.

| Where | Key | Type | What for |
|---|---|---|---|
| `[target]` | `board = "<MCU board>"` | string | `jk build --target mcu:<board>` and the `board_id` |
| `[target]` | `accept_unsafe_reset = "<justification>"` (ref/01 §5.22) | string | accepts `J0786` in `prod`; it goes to the `[accepted]` of the MCU's lock and does not change the firmware's behavior (§2.16) |
| `[project]` | `mode` | `"dev"` \| `"prod"` | `DEV` bit of the header; a node in `prod` only sends `prod` blobs (`J0794`, §2.16) |
| `[target.capacity]` | the names of §2.13.3 (`signals = 128`, `trace_bytes = "16 KiB"`, …) | integer or size | capabilities of the build |
| `[target.extern]` | `expose = ["alarm.**", "calc.*"]` | selectors | signals the firmware may read (§2.8.2) |
| `[target.micro]` | `service_deadline = "100ms"`, `freeze_on_trip = true`, `samples_ring = 256`, `event_queue = 16`, `confirm = "5m"` | | `NODE` section |
| `[clock]` | `sync = "link" \| "extern" \| "none"` (new values for MCU) | string | clock source |
| `[devices.<d>]` | `driver = "extern"` | string | device fed by the integrator |
| `[devices.<d>.signals]` | `<name> = { type, unit, direction, … }` | table | signals of a device without a profile (`J0785`) |
| `[apps.<a>]` | `kind = "extern"`, `writer = { … }`, `writes = [outputs]`, `reads = [selectors]` | | external writer |
| `[apps.<a>.params]` | `<p> = { value = …, runtime = true, persist = true }` | | changeable parameters (§2.6.l) |
| `[link]` (ref/01 §5.22) and `[outputs.<s>] remote_levels` | transport (`bus` or `address`), `addr`, `auth`, `peer`, delegated levels | | `LINKS` section |
| `[net.wifi]` (ref/01 §5.22) | `ssid`, `password = "secret:<name>"`, `country`, `hostname` | strings | `NET` and `SECRETS` sections; requires `[net] wifi = true` on the board |
| `[devices.<d>]` with `driver = "esp32"` (ref/01 appendix A) | signals with `touch = <0–9>`, `rssi = true`, `uptime = true` | | `DEVICES` section; `touch` is resolved through the `touch_channel` of the board's `[pins]` |
| MCU board profile (`ref/03` §2.3.1, §2.3.3) | `[board]` with `os = "none"`, `mcu_class`, `chip` (the HAL name, which chooses the `chip-*` family), `arch` (Rust triple), `fpu` (the enum of §2.14, key 15), `memory`, `flash`, `reset_safe`, `stack`; `[areas]` (offsets and sizes), `[irq] priorities`, `[watchdog]`, default `[capacities]`, `[link] default` (maintenance mode) | | §2.12, §2.15 |

**How the Wi-Fi password reaches the MCU.** The MCU needs it before it has a network, and nobody on the way should be able to read it:

1. **MCU key.** An M2 MCU with `[net.wifi] password` has an X25519 secrets key in record 19 of the `IDENTITY` area, like a standalone MCU. It is generated at enrollment: `jk node enroll <mcu> --link <d> --ca <directory>` (coprocessor, document 07 §3.1) or the enrollment of a standalone MCU; the public key goes in the MCU's certificate, and the MCU becomes the recipient `node:<mcu>` of `secrets.toml.enc` (ref/02 §2.4.8, with the attestation of §3.4.2).
2. **Encryption.** `jk secret set wifi_bancada` encrypts the value for the recipients, including `node:<mcu>`, like any secret. A missing name is `J0920`; without the MCU's `wrap`, `J0922`.
3. **Build.** `jk build --generation` copies the entry, **still encrypted**, into the `SECRETS` section of the blob, only with the `wrap` of `node:<mcu>`, and writes the name of the secret in `NET`. `jk` decrypts nothing: it does not need the escrow key. The blob's signature (§2.13.5) covers both sections.
4. **Transport.** The blob reaches the MCU by `jk flash` or over the link (generation OTA, ref/07 §9). The Linux node and the Deck carry it without being able to read it, just like the other secrets (ref/02 §3.4.3).
5. **Boot.** The firmware verifies the blob, decrypts the entry with the key of record 19 (HPKE, ref/02 §2.4.4), connects to the Wi-Fi and erases the plaintext copy from RAM after passing it to the radio. The value never goes to the trace, to `HELLO` or to persistence.

Since the first blob must arrive before there is a network, the first flashing is always by `jk flash` or over the serial link; an IP-only link on an MCU without a valid generation falls into maintenance mode, which uses the board's `[link] default` (§2.12.3). **Rotating a secret on an MCU: the exception to ADR 0024 §3.** On a Linux node, rotating a secret does not create a new generation (ref/02 §3.4.4). On an MCU, the secret goes **inside the blob** (`SECRETS` section), and so rotating it produces a **new blob with the same manifest**: `jk secret set` followed by `jk build --generation` (or by `jk apply`, which does it when the `rev` of an MCU secret goes up) gives a blob with a **new `gen_seq`** (the anti-rollback of §2.12 still applies) and a new `image_hash`, but the same `source_hash` and the same `registry_hash`: the Linux generation and its digest do not change, and the Deck sees nothing that depends on the value. The new blob follows the normal path (confirmation, rollback, §3.7), and the MCU must reach the new network with the previous blob or over the serial link.

---

## 3. Expected behavior

### 3.1 The tick

- The order of the steps is that of document 01, §4, and the rules are those of document 16 (arbiter A1–A14, envelope E1–E10, quality §3, expressions §4). The MCU has no rules of its own: it is the same `joker-core`.
- **Step 2:** each group's box receives, in order of arrival, the proposals and method records of the external writers (`jk_propose`, `jk_call_method`), of the link (`PROPOSE`, `CALL`) and of the apps' async tasks. They all count as async for rule A4: the last one from the same writer wins, and a cycle one from the same writer in the same tick beats them.
- **Step 4:** the cycle tasks are native calls. There is no *epoch*: a task that does not finish jams the group, and `jk_supervise` forces the `safe` of its outputs at 3 periods (§3.4). An `Err` ends the task without discarding the proposals already made; a *panic* is `jk_fatal` (§2.6.b).
- **Step 7:** the commit writes the applied value into each output's double-buffered box (the reader never waits), marks `CHANGED`, marks for the service the persistent records to write and queues on the link the changes to announce (`OUTCOMES`, `SAMPLES`).
- The `now_us` of `jk_tick` is the latch instant: the apps' `cx.now()`, the trace's `t_mono` and the base of the leases (A2). The `overrun` policy uses the duration measured with `jk_now_us()` and only affects the following ticks, with the flag recorded in the trace, so that replay is exact.

### 3.2 Determinism and conformance

- The conformance cases (document 16, §8) run on the MCU with a conformance firmware (`joker-micro-conformance`, document 13, task 22), which receives the case's ticks by probe (RTT) or over the link, calls `jk_set_input_ex`, `jk_propose`, `jk_tick` with the case's times, and returns the results. **They must give the same bytes as on Linux** (ADR 0020).
- Nothing nondeterministic enters the decisions of a tick: not `jk_now_us`, not the preemption order between groups (a group only sees from another the values applied as of its latch, document 02, §6.3), not the moment at which an ISR wrote an input (the latch fixes it, and the trace records it).
- The MCU trace is that of document 16, §7, with times in ns (µs × 1000, exact) and without compression; `jk replay` runs the same core on the laptop (19 U9).

### 3.3 Inputs and reads

- An input written before a group's `jk_tick` is seen in that tick; written during the tick, in the next one. Never midway.
- Aging (`max_age`, document 01, §3.3) is computed at the latch with the tick's `now_us` and, in `jk_read`, with the `now_us` passed.
- A collision of the reader with the writer gives the previous complete sample, and is counted. With the writer in an ISR and the group in a task, it only happens if the ISR preempts the latch between the two halves of a 32 B copy: it is rare and harmless.

### 3.4 Outputs and safe state

| Situation | What the integrator reads in `jk_get_output` | Guaranteed by |
|---|---|---|
| before `jk_init` | nothing (`JK_E_STATE`): the pins are in the hardware reset state | the board (`reset_safe`, `J0786`) |
| after `jk_init`, before the first tick | the `safe` | A8 |
| no active writer (leases expired, link down) | the `safe` (`JK_OUTF_SAFE`) | A6 |
| group jammed > 3 periods | the `safe` (`JK_OUTF_FORCED`), if the integrator is still reading; otherwise, it writes it from the supervisor ISR | `jk_supervise` |
| trip | the `safe` (`JK_OUTF_TRIPPED`, `Bad(tripped)`), persistent | E7, §2.10 |
| `jk_force_safe(true)` | the `safe` (`JK_OUTF_FORCED`) | |
| node stopped (`HALTED`) | the `safe`; `feed_watchdog = 0`, and the watchdog restarts the MCU | §3.9 |
| MCU frozen | nothing: the hardware watchdog restarts it; until then, the last value stays on the pins | watchdog + equipment fallback (document 01, §8) |
| `safe = "relinquish"` in effect | `JK_RELINQUISHED` | document 03, §6.1 |

The returned value is **always admissible**: the integrator applies it without thinking. That is what makes the coprocessor MCU safe by design (19 §6, U1).

### 3.5 Concurrency

- The structures shared between contexts are only these, all lock-free: slots (single writer), output boxes (double buffer, writer = owning group), proposal boxes per group (SPSC: producer = service, consumer = group), event queues, `OUTCOMES` and `SAMPLES` per group (SPSC: producer = group, consumer = service), `force_safe` and trace-freeze bits (atomic). The memory order is *acquire/release*; on multicore, the node memory must be coherent (§2.9.3).
- Different groups never share mutable state: each one has its image, its outputs, its trace ring and its statistics (document 01, §3.4).

### 3.6 Link and leases

- The peer's proposals (Linux node or Deck) enter with the level they carry, as long as it is among the link's allowed levels (otherwise `rejected(not_owner)`), and with a lease ≤ the link's `max_lease` (A9, 16 A12; the MCU does not persist them, §2.10.1). The writer is the pair (link, level) of the `WRITERS` table.
- If the link drops, nothing happens immediately: the leases expire at the latch of the owning group (A2), and the envelope applies the `safe`. The time until safe is, at most, the lease plus one period of the group (19 U1: ≤ 40 ms with a lease of 3 × 10 ms).
- `HELLO` announces the `source_hash` (the generation hash), the `registry_hash`, the `gen_seq`, the firmware descriptor (version, image and descriptor digests, capabilities hash, `security_version`), the summary of the descriptors of the planned drivers, the locally changed parameters and the generation's exports (signals, instances, class contracts, actions), from the blob. The format is that of `ref/07` §6.4.

### 3.7 Generations

- Swap at a tick boundary, inheriting the applied value, the leases and the trips of the outputs that remain (A8, document 18, P6). A new output starts in `safe`; a removed output ceases to exist (the firmware stops applying it: the integrator sees `JK_E_SIGNAL` if it still has it in the interface, or the interface prevented the generation, `J0790`).
- Confirmation and rollback as in document 01, §15, hot, without restarting the MCU.
- The active generation and the previous one must remain mapped until confirmation. In firmware mode, they are the two A/B areas.

### 3.8 Time

- The clock starts `Unsynced`. With the `link` source, each `TIME` sets the mapping and makes it `Synced`; without `TIME` it moves to `Holdover` and, after `max_holdover`, to `Unsynced` (document 03, §3). With the `extern` source, the same with `jk_time_set_utc`.
- Apps with `requires_clock = "synced"` have their outputs blocked (`rejected(clock_unsynced)` at step 0 of the envelope, which leads to `safe`) while the clock is not `Synced` (document 01, §7 and §18; 16, E1).
- The MCU's monotonic instants never leave it without the UTC pair (`ref/00` §3); `jokerd` converts µs ↔ ns at the boundary.

### 3.9 Errors and internal failures

- No call error changes state: a function that returns an error has done nothing (except those that say so: `jk_action_result` with `JK_E_FULL` keeps the ticket).
- A broken internal invariant (an impossible state, an index outside the blob that the verifier should have caught) puts the node in `HALTED`: all outputs in `safe`, all stateful functions return `JK_E_HALTED`, `jk_supervise` stops asking for the watchdog, and `jk_fatal(JK_E_INTERNAL, detail)` is called in the next service or group context. The detail identifies the location and goes to the log of the next boot, if there is persistence.

### 3.10 Limits

| Thing | v1 limit |
|---|---|
| groups per node | 8 (ABI: 32) |
| minimum group period | 100 µs (~10 kHz, 19 U4) |
| arguments of a method through the ABI (`jk_value_t`) | 8; more, through CBOR |
| idempotency key | 1–64 B printable ASCII (`ref/00` §10) |
| `string`/`bytes` in a signal | 64 B |
| event or action argument | 1 KiB, lists of 64 |
| `kv` per app | 16 KiB (19 §3) |
| arbiter levels | 16 |
| expression stack | 32 cells |
| blob | 16 MiB (`u32` offsets; in practice, the generation area) |
| link MTU | that of `ref/07`, per transport |

### 3.11 Compatibility

| Between | Rule |
|---|---|
| C code and library | ABI: same major, header minor ≤ library minor, same interface hash (§2.3) |
| firmware and blob | the firmware reads the formats and ISAs up to those of its descriptor; `jk` writes the highest one the firmware reads (§2.13.6). A 1.3 `jk` produces blobs for a 1.0 generic firmware; a 1.3 firmware reads the blobs of a 1.0 `jk` |
| new firmware and old persistence | the journal format (§2.10.2) only grows: new types, new CBOR fields (ignored by the old ones) |
| firmware and `jokerd` | through the `link_version` of `HELLO` (`ref/07`) |
| generation and firmware core | the lock pins the core version of each MCU (`[mcu.<d>] core = "1.0"`, ref/02 §2.1.10). `jk check` only lets into an MCU generation what that core knows: a feature from a later minor (core rule, expression instruction, blob section, manifest key) is `J0796`, with the version that introduced it. To use it, a firmware with a newer core is flashed first (firmware OTA, §2.12.6) and the lock is updated |
| apps and firmware | a native app is firmware code; changing an app is a new firmware (or, in 1.4, `wasmi`). The generation refers to the app by name, major and `meta_hash` (`J0772`) |
| 1.x | golden rule of ADR 0022: no blob, persistence, header or call that works in a 1.y stops working in a 1.z > y |

---

## 4. Stress

**D1 — The header was regenerated, but the old library stayed in the project.** The integrator added a signal to the manifest, ran `jk build --lib` and copied only the header. The indices changed, and `JK_SIG_TANK_LEVEL` now points to another signal. **Broke** silently. **Fix incorporated:** `jk_init` is a macro that passes `sizeof(node)`, `JK_ABI_VERSION` and `JK_PROJECT_IFACE_HASH`; the library refuses with `JK_E_ABI` (`J-E2503`) before reading any input.

**D2 — Generic firmware and a new generation with more signals.** In library mode with a generation in flash, the generation of site B has a signal that site A's did not have, and all the `SignalId`s after it shift. A header with `SignalId`s would be wrong at half the sites. **Broke. Fix incorporated:** the header's identifiers are indices of the **external interface**, fixed at library build time and bound by name to each generation in `jk_init` (§2.1, §2.8.2). A generation that lacks a required entry is rejected (`J0790` at build time, `JK_E_GEN_FIRMWARE` on the MCU).

**D3 — Seqlock on a single-core MCU.** The integrator writes an input in a low-priority task and the group runs in a high-priority interrupt. With a classic seqlock, the latch that interrupts the writer midway waits for the version to become even, and the task that would make it even never runs again. **Broke** (total deadlock). **Fix incorporated:** wait-free readers: a collision returns the previous complete sample and is counted (§2.4.1). Proposed note to document 01, §3.2 (report).

**D4 — Two tasks call `jk_tick` of the same group.** A FreeRTOS integrator starts the loop twice by mistake. **Holds:** the busy flag of the group context returns `JK_E_REENTRANT` to the second call, without effect. **Different** groups in different tasks are the intended use.

**D5 — Power outage 1 ms after a trip.** The trip is in the node, but `jk_service` has not yet written it. **Does not fully hold:** the trip is lost, and the output boots in `safe` but without the trip. **Declared mitigation:** the trip record is written at the first `jk_service` (≤ `service_deadline`); `jk_shutdown` writes on *brown-out* if the board has a warning; and if the condition persists (the usual case, an `actions.trip` alarm), the trip happens again at the first tick. The difference from Linux's `fsync` is written in §2.10.3 and in the report for document 19.

**D6 — `jokerd` dies (U1).** The leases of the link's proposals expire at the latch of the MCU's owning group; in ≤ lease + 1 period the outputs are in `safe`. If it is the **MCU's loop** that jams, the integrator's ISR with `jk_supervise` forces the `safe` at 3 periods and stops feeding the watchdog, which restarts the MCU. **Holds.**

**D7 — A valid blob from site A loaded at site B.** Same generic firmware, same signing key: the signature verifies. **Broke** (site B would get site A's envelopes). **Fix incorporated:** the COSE manifest may bind the blob to a node (key 14), compared with the `IDENTITY` area (`JK_E_GEN_NODE`); in `prod` the binding is required (`J0788`). Anti-rollback (`gen_seq > confirmed_seq`) also prevents restoring an old blob of the site itself.

**D8 — Generic 1.0 firmware in the field, 1.3 `jk` on the laptop.** The 1.3 `jk` has a new expression function. **Holds:** `jk build --generation` reads the firmware descriptor, writes the format and the ISA the firmware understands, and gives `J0789` if the generation needs something that only exists in a newer firmware, with the suggestion to update the firmware first.

**D9 — An app with `f32` on an ESP32-C6.** The C6 has no FPU; `J0721` only talked about `f64`. A 1 kHz PID loop in `f32` costs 10× more than expected. **Broke** (the warning did not come out). **Fix incorporated:** the descriptor declares the FPU, and `jk build` looks for the software floating-point routines in the app's object: `J0721` for `f64` without a double-precision FPU and the new `J0780` for `f32` without an FPU.

**D10 — An `int` in a `bool` signal, a `double` in an `f32`, an 80 B string.** Common C errors. **Holds:** the `_Generic` macro chooses the setter by the type of the value; the table of §2.5.4 accepts `0`/`1` in a `bool` and rounds a `double` to `f32`; a `double` in a `u64` is `JK_E_TYPE` and the long string `JK_E_VALUE`, both without effect and with a code. Compile-time typed handles exist in the Rust crate (`In<T>`, `Out<T>`), and not in C (§5).

**D11 — A new generation in the middle of a ramp (document 18, P6).** The output `p_set` is rising at 10 kW/s. **Holds:** `jk_generation_swap` keeps the last applied value, the leases with the same identity and the trip state; the rate continues from the last applied value (E2). With the confirmation window expired, the return to the previous generation is the same, in the opposite direction.

**D12 — Stack exhausted with four nested groups.** Each group is an interrupt executor with its own priority; all four may be active at the same time on the same main stack. **Broke** (silent memory corruption). **Fix incorporated:** `jk check` adds up the stacks of all priority levels and compares with the board's (`J0774`); in firmware mode, `joker-micro` links with `flip-link` (Cortex-M), and an overflow becomes a hardware fault, hence a reset, and not corruption.

---

## 5. Decisions and rejected alternatives

| Decision | Rejected alternative | Why |
|---|---|---|
| two headers: fixed `joker_node.h` and generated `joker_project.h` | a single generated header (the literal reading of 19 §4.2) | the fixed one can be reviewed, versioned and compiled in CI as a contract; the generated one changes with every build |
| identifiers = indices of the external interface, bound by name in `jk_init` | the generation's `SignalId`s in the header | they would break with every generation in flash (D2) |
| `jk_now_us` defined by the integrator, but read only in the link and in the statistics | the library reads the clock in the tick | ADR 0007: the tick's time must be the argument's, for the trace and the replay |
| `jk_platform_t` with callbacks and `user` | weak symbols (`__attribute__((weak))`) | they are not C11, and they do not allow two nodes |
| informative positive values (`JK_EMPTY`, `JK_PENDING`, `JK_RELINQUISHED`) | only `JK_OK` and errors, with output parameters | "there are no more events" is not an error; `ref/00` §8 only fixes 0 and the negatives |
| generic setters with `_Generic` + typed functions | typed handles in C (one `struct` per type) | they do not work in `switch`, weigh on DX and C++ has no `_Generic`; the check stays at runtime (`JK_E_TYPE`), and at compile time in Rust |
| external writers with "async" proposals before the tick | C *callbacks* called inside the tick (step 4) | the WCET of the integrator's C does not enter admission, and reentrancy from the tick would be a trap; drained at step 2 they give the same effect in the single loop |
| `jk_link_poll` includes a `jk_service` step | explicit service only | the example of document 19 would not call the service, and persistence would never run |
| one service context per node | multithreaded service | the queues stay SPSC and lock-free; the service's work is small |
| binary blob of fixed tables, used in place (XIP) | CBOR, FlatBuffers | CBOR would force copying to RAM; FlatBuffers brings a dependency and a verifier that does not know our semantics; the tables are the core's format |
| COSE_Sign1 over the OTA manifest of ref/07, with the SHA-256 of the image | COSE with the image in the payload; a blob-specific *claim* beside the OTA manifest | the image would no longer be aligned and mappable, and Ed25519 would have to go over all of it; two ways of signing the same thing would be two verifiers |
| forward-only jumps in the bytecode | free jumps | expressions are total; the verifier guarantees time bounded by size |
| persistence journal defined by the library | an integrator KV | atomicity, wear and compatibility across firmware versions are our guarantees |
| node memory as a generated concrete type | generic arena | no `unsafe` outside `ffi` (invariant 10 of `AGENTS.md`) |
| app *panic* = `jk_fatal` | catching the *panic* | there is no `catch_unwind` in `no_std`; a half-updated state is worse than a reset to the safe state |

---

## 6. Version markers

| Element | v1.0 | Reserved |
|---|---|---|
| C ABI | 1.0, all the functions of this document | a 1.x only adds (§2.3) |
| signal types | 0–8 | 16 block, 17 stream (v1.1, "Ready for ML") |
| blob sections | 0x0001–0x0024 | 0x0040 `MODELS`, 0x0041 `BLOCKS`, 0x0042 `STREAMS` (v1.1); 0x0043 `SCHEDULES` (scheduled writes, M6, v1.1); 0x0044 `WASM_APPS` (`wasmi`, v1.4) |
| trace | `n_ml_outputs = 0` always | model outputs on the MCU (v1.4, Burn Flex `no_std`) |
| scheduled writes (`write_at`, `timed`) | not on the MCU (`J0783`) | v1.1 |
| ML | no | v1.1 trial on M2, v1.4 production (document 21, §6) |
| apps | native in Rust | `wasmi` on M2 (v1.4); native Structured Text (v1.2), through the same `NativeApp` |
| link transports | `uart`, `rs485`, `can`, `usb_cdc`, `udp`, `tcp` | 7 `lorawan`, 8 `nbiot` (v1.4, only on demand) |
| low power | `jk_service_next_us` already exists | event-woken groups and energy budget (v1.4) |
| capabilities | `cap_id` 1–41 | 42+ are added at the end |
| codes | `J0770`–`J0794`, `J-E2501`–`J-E2599` | `J0795`–`J0799`, `J-E2600`–`J-E2999` |
| node-qualified paths (`@node.…`) | no | v2 (document 21, §8) |

