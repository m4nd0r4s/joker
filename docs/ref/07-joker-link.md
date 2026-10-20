# ref/07 — Joker Link: the protocol on the wire

Normative reference of Joker Link v1.0, the protocol between a Joker Micro and the node that uses it (a `jokerd`, or the Deck for an M2 MCU). It closes document 19, §6, at the byte level: two teams that read only this document and `spec/cddl/joker-link.cddl`, one in `no_std` Rust and the other in C, must produce implementations that interoperate.

It applies ref/00 without repeating it. The semantics (arbiter, envelope, quality, trace) are those of document 16: here we only say how they cross the wire.

Normative words: **must** / **must not** (required), **should** (recommended; deviating requires a reason), **may** (optional).

---

## 1. Scope and sources

| Covers | Does not cover |
|---|---|
| framing on UART/RS-485, USB CDC, CAN/CAN-FD (ISO-TP), UDP, TCP | the C ABI and the format of the generation blob (ref/08) |
| security: keys, handshake, encryption, anti-replay, rotation | the enrollment of the Linux node's identity (ref/02) |
| session: header, acknowledgment, retransmission, fragmentation, timers | the messages of the agent protocol (ref/10) |
| the 25 messages, with CDDL, direction, reliability and examples in bytes | the TOML keys of the `joker-link` device and of the MCU's `[link]` (ref/01; §15 lists what this reference needs) |
| the session state machine and degradation | the HTTP API that shows the link state (ref/09) |
| the binding to the core: arbiter, results, quality, time, trace | |
| firmware and generation OTA over the link | |
| size and time budgets on the wire | |
| versions and negotiation; framing of the micro profile (ref/10) | |

**Sources.** 19 §6 (the protocol), §7 (U1, U7, U9, U10); 16 §1 (A1–A11), §2 (E1–E9), §3, §7 (trace), §8; 01 §3 (slot and seq), §6–§8 (arbiter, envelope, safe state), §15–§18 (generations, OTA, security, time); 02 §4–§8 (quality, time, writes, actions); 05 §12 (link states); 17 §2, §4, §9 (methods, actions, class versions); 20 §3.1; ADR 0015, 0020, 0022; ref/00 (all of it).

**Connects to:**

| Reference | What |
|---|---|
| ref/00 | time (§3), values and quality (§4), identifiers (§5), enums (§6), CBOR and COSE (§7), errors (§8) |
| ref/01 | `driver = "joker-link"` device of the Linux node; `[link]` of the MCU manifest (`bus`, `address`, `addr`, `peer`, `auth`); `J0179`, `J0234` |
| ref/02 | `[mcu.<d>]` and `[discovered.<d>]` of the lock; drift `J0460`, `J0461`; the link key secret |
| ref/08 | `jk_link_poll`, `jk_link_status` (CRC, authentication and replay counters, session mode), flash areas and identity records, `jk_generation_swap`, `jk_restart`, `jk_trace_read`, blob format (the catalog and the OTA manifest go inside it); `J-E25xx` codes. The link codes are `J-E2000`–`J-E2499`; the ABI's, `J-E2500` onward |
| ref/09 | link state (the six values of `link_state`, ref/00 §6), session mode, negotiated version, counters, last `HELLO`, firmware descriptor, `gen_seq` and confirmation state, in the local API (`GET /v1/micros/{device}`) |
| ref/10 | `mcu` phase of the desired state, which uses `OTA` and `HELLO` from here; micro profile over MQTT/CoAP (§14) |

**Files:** this document and `spec/cddl/joker-link.cddl` (the message bodies, the catalog and the OTA manifest). The binary headers are only here (§3–§5).

**Roles.** In each session there is a **device** (D, the Joker Micro) and a **host** (H, the `jokerd`; or the Deck, for an M2 MCU whose `[link] peer = "deck"`). D is always the one that starts the handshake. In v1, a D has at most **one** session at a time.

---

## 2. The layers

```
 ┌──────────────────────────────────────────────────────────────────────────────┐
 │ L4  messages      CBOR, integer keys (§6)          HELLO SAMPLES PROPOSE …    │
 ├──────────────────────────────────────────────────────────────────────────────┤
 │ L3  session       binary header 10–12 B (§5)       type seq ack ack_bits rseq │
 ├──────────────────────────────────────────────────────────────────────────────┤
 │ L2  security      1 B | +24 B (session) | +36 B (handshake) (§4)              │
 │                   in plaintext, or ChaCha20-Poly1305                           │
 ├──────────────────────────────────────────────────────────────────────────────┤
 │ L1  framing       CRC-32C always; COBS + 0x00 on streams; 1 datagram = 1 frame│
 ├──────────────────────────────────────────────────────────────────────────────┤
 │ L0  transport     UART · RS-485 · USB CDC · TCP  │  CAN/CAN-FD (ISO-TP) · UDP │
 └──────────────────────────────────────────────────────────────────────────────┘
```

| Term | Meaning |
|---|---|
| **L2 frame** | the bytes between L1 and L3: security header, message (encrypted or not) and tag |
| **message** | L3 header + CBOR body |
| **packet** | an L2 frame with its message; it is the unit the session numbers (`seq`) and acknowledges |
| `max_frame` | the maximum size of an L2 frame, negotiated in `HELLO` (§3.6) |
| `max_msg` | the maximum size of a message after reassembly (§5.5) |

All binary integers (L1, L2, L3) are **little-endian**, as in the trace (16 §7).

---

## 3. Framing (L1)

### 3.1 Transports

| Transport | Type | Framing | CRC-32C | Security (§4.6) | Address |
|---|---|---|---|---|---|
| UART | stream | COBS + `0x00` (§3.2) | yes | optional | `addr` byte (0) |
| RS-485 full-duplex | stream | COBS + `0x00` | yes | optional | `addr` (0 or 1–247) |
| RS-485 half-duplex | stream, with turn (§3.3) | COBS + `0x00` | yes | optional | `addr` 1–247 |
| USB CDC-ACM | stream | COBS + `0x00` | yes | optional | `addr` (0) |
| TCP | stream | COBS + `0x00` | yes | **required** | `addr` (0) |
| CAN / CAN-FD | datagram (ISO-TP) | one ISO-TP message = one frame (§3.4) | yes | optional | CAN identifier |
| UDP | datagram | one datagram = one frame (§3.5) | yes | **required** | L2 `sid` |

CRC-32C is on **all** transports, even where there is another check underneath (TCP, UDP) or the Poly1305 tag on top. It costs 4 B and gives a single implementation rule, and it makes it possible to tell noise (wrong CRC) from an attack or a wrong key (right CRC, wrong tag), which `jk doctor` shows in separate counters.

### 3.2 Streams: COBS + CRC-32C

```
on the wire: COBS( addr | L2 frame | crc32c ) | 0x00
                   └ 1 B ┘           └ 4 B LE ┘   └ delimiter
```

| Element | Rule |
|---|---|
| `addr` | u8. 0 = point-to-point; 1–247 = address of D on a shared bus; 248–254 reserved; 255 = broadcast from H (only class E messages, §5.4). In a frame from H it is the destination address; in one from D, the address of D itself |
| `crc32c` | CRC-32C over `addr` and the L2 frame, written in little-endian |
| COBS | *Consistent Overhead Byte Stuffing* (Cheshire and Baker, 1999), in the original form: blocks of up to 254 bytes, code `0xFF` without an implicit zero. The output has no `0x00` |
| delimiter | one `0x00` after each frame |

**CRC-32C** (*reveng* catalog: CRC-32/ISCSI):

| Parameter | Value |
|---|---|
| width | 32 |
| polynomial | `0x1EDC6F41` (reflected: `0x82F63B78`) |
| initial value | `0xFFFFFFFF` |
| reflected input and output | yes, yes |
| final XOR | `0xFFFFFFFF` |
| check (`"123456789"`) | `0xE3069283` |
| residue | `0xB798B438` |

**Sender.** Encodes, sends and ends with `0x00`. It may send extra `0x00` bytes before a frame (empty frames are ignored); it should do so before the first frame after the transport opens, to discard garbage on the line.

**Receiver.** Accumulates bytes up to a `0x00`, decodes the COBS and validates in the following order. A failure discards the frame silently (no `ERROR`) and counts in the indicated counter:

| # | Check | Counter (code) |
|---|---|---|
| 1 | accumulated ≤ `max_frame` + 5 + ⌈(`max_frame` + 5) / 254⌉; otherwise discard up to the next `0x00` | `oversize` (`J-E2002`) |
| 2 | valid COBS and decoded length ≥ 6 | `framing` (`J-E2003`) |
| 3 | CRC-32C of the decoded bytes minus the last 4 = those 4 (LE) | `crc` (`J-E2001`) |
| 4 | `addr` is 0, its own address (D), or 255 (D, class E only) | — (not for me) |

**Example.** The `SAMPLES` message of §6.9, in plaintext, on UART (`addr` 0):

```
L2 frame  00 12 10 2c 03 92 01 ff ff ff ff a2 01 1a 00 12 eb c0 02 83 83 00 19 04 b1 f5
          84 01 19 03 6d f4 18 fa 85 18 28 19 14 00 f9 5e 72 19 07 08 19 03 01          (49 B)
crc32c    over 00 | L2 frame   = 0x3C5F95E8  → e8 95 5f 3c
on wire   01 01 0e 12 10 2c 03 92 01 ff ff ff ff a2 01 1a 07 12 eb c0 02 83 83 12 19 04
          b1 f5 84 01 19 03 6d f4 18 fa 85 18 28 19 14 0e f9 5e 72 19 07 08 19 03 01 e8
          95 5f 3c 00                                                                   (56 B)
```

### 3.3 RS-485 half-duplex: the turn

On a two-wire RS-485 pair only one side talks at a time. H is the controller:

1. H sends one or more frames to `addr` A; the last one carries the `EOT` flag (§5.2). That gives the turn to A.
2. A must start replying within `turn_timeout` (default 5 ms, counted from H's final `0x00`, after `turnaround`) and sends from 1 to `turn_frames` frames (default 4), the last one with `EOT`. With nothing to send, it replies with an `ACK` with `EOT`.
3. With no reply within `turn_timeout`, H counts a lost turn and moves on to the next D. Three consecutive lost turns count as one lost frame for §5.3.
4. H gives the turn to each D at least once every `report` (§5.6). Frames with `addr` 255 do not give the turn.
5. `turnaround` (minimum silence before transmitting, for the transceiver to switch) comes from the bus (`[buses.<b>] turnaround`, ref/01); default 100 µs.

In full-duplex, `EOT` is ignored.

### 3.4 CAN and CAN-FD: ISO-TP

Each L2 frame, followed by its CRC-32C (4 B, LE), is **one ISO-TP message** (ISO 15765-2:2016), normal addressing, with 29-bit identifiers:

```
bits 28–24  priority       0x06 = control, 0x07 = bulk
bits 23–16  protocol       0x4A (configurable: can_id_base)
bits 15–8   destination    0x00 = H; 0x01–0xFD = D
bits 7–0    source
```

| Rule | Value |
|---|---|
| channels | each D has two pairs of identifiers: **control** and **bulk**. `CATALOG`, `TRACE` and `OTA` go on bulk; everything else on control. An ISO-TP transfer in progress on bulk does not block control, because CAN arbitration interleaves the frames by identifier |
| classic CAN | 8 B frames; the last one may be shorter (no padding); receivers accept both |
| CAN-FD | `BRS` on; DLC up to 64 B; first frame with 62 B of data |
| size | ≤ 4095 B per ISO-TP message (without the 32-bit escape), therefore `max_frame` ≤ 4091 |
| flow control | `BS = 0`, `STmin = 0` by default (configurable); `N_Bs` and `N_Cr` = 100 ms |
| ISO-TP error | wrong sequence, timeout or `FC(Overflow)`: the message is lost and counts in `isotp` (`J-E2004`); the session recovers through §5.3 |

In the C ABI, ISO-TP belongs to the integrator: `jk_link_rx_fn` delivers a whole ISO-TP message (L2 frame + CRC) per call (ref/08).

### 3.5 UDP

One datagram = one L2 frame followed by the CRC-32C (4 B, LE). H port by default **7471** (configurable). D sends to H; H replies to the source address and port of the last authenticated packet of that session (D may change address: DHCP, NAT). The session is identified by the L2 `sid` (§4.1), because security is required. Datagram payload ≤ 1232 B (IPv6 1280 − 40 − 8), therefore `max_frame` ≤ 1228.

### 3.6 TCP and USB CDC

TCP and USB CDC are streams: they use §3.2 as is, with `addr` 0. On TCP, D connects to H (port 7471 by default), with `TCP_NODELAY`; closing the connection is link loss (§7). One connection = one session.

### 3.7 Frame sizes

| Transport | Minimum `max_frame` accepted by any implementation | Default | Maximum |
|---|---|---|---|
| UART, RS-485, USB CDC, TCP | 512 | 1024 | 4096 |
| CAN, CAN-FD | 512 | 1024 | 4091 |
| UDP | 512 | 1024 | 1228 |

`max_frame` counts only the L2 frame (without `addr`, CRC, COBS, delimiter or ISO-TP PCI). The session value is the smaller of what D declares in `caps` and what is configured in H. A configured value outside these limits is `J0734`. **The `HELLO` must fit in 512 B** (§6.4), so that the handshake works before any negotiation.

---

## 4. Security (L2)

### 4.1 The first byte and the three frame forms

```
bit   7 6 | 5 4  | 3     | 2 1 0
      fmt | kind | phase | reserved (0; the receiver ignores them)
fmt   = 0 in v1 (other value: discard, counter "format", J-E2005)
kind  = 0 plaintext · 1 handshake · 2 session · 3 reserved (discard)
phase = session key phase (kind 2 only; §4.4)
```

| `kind` | Form | Overhead | Messages |
|---|---|---|---|
| 0, plaintext | `hdr(1) | message` | 1 B | all, only with `auth = "none"` |
| 1, handshake | `hdr(1) | kid(8) | nonce(12) | encrypt(message) | tag(16)` | 37 B | only `HELLO`, with `auth = "required"` |
| 2, session | `hdr(1) | sid(4, LE) | ctr(4, LE) | encrypt(message) | tag(16)` | 25 B | all except `HELLO`, with `auth = "required"` |

- The cipher is **ChaCha20-Poly1305** (RFC 8439): 32 B key, 12 B nonce, 16 B tag.
- **AAD** = the cleartext bytes before the ciphertext: `hdr | kid | nonce` (handshake) or `hdr | sid | ctr` (session).
- **Session nonce** = `00 00 00 00 | ctr as u64 LE` (12 B). **Handshake nonce** = the 12 random bytes of the frame.
- A frame with a `kind` that does not match the configured mode is discarded (`J-E2104` for a plaintext frame with `auth = "required"`).

### 4.2 Keys

| Key | Derivation | Use |
|---|---|---|
| `K_link` | 32 random bytes, generated at MCU enrollment (§4.7) | shared secret D ↔ H; never goes on the wire, except encrypted in `KEYROT` |
| `kid` | `SHA-256("joker-link/1 kid" ‖ K_link)[0..8]` | tells H which `K_link` to use, before decrypting |
| `K_hs` | `HKDF-SHA256(salt = empty, IKM = K_link, info = "joker-link/1 hs", L = 32)` | encrypts the `HELLO`s |
| `PRK` | `HKDF-Extract(salt = nonce_d ‖ nonce_h, IKM = K_link)` | per session |
| `k_d2h` | `HKDF-Expand(PRK, "joker-link/1 d2h" ‖ sid(4, LE), 32)` | D → H frames |
| `k_h2d` | `HKDF-Expand(PRK, "joker-link/1 h2d" ‖ sid(4, LE), 32)` | H → D frames |
| `k'` (next phase) | `HKDF-Expand(k, "joker-link/1 ku", 32)` | key update (§4.4), per direction |

The `info` strings are ASCII, without a terminator. HKDF: RFC 5869, with SHA-256; "empty salt" is the salt of 32 zeros of RFC 5869, §2.2.

### 4.3 Handshake

```
  D                                                                   H
  │ HELLO offer  [kind 1, K_hs, random nonce]  nonce_d (16 B)         │
  │──────────────────────────────────────────────────────────────────▶│ kid → K_link; decrypts;
  │                                                                   │ checks versions, hashes (§7.3)
  │        HELLO answer [kind 1, K_hs]  nonce_h, echo nonce_d, sid,   │
  │◀──────────────────────────────────────────────────── mode, params │
  │ confirms the echo; derives k_d2h, k_h2d                           │ derives k_d2h, k_h2d
  │ READY  [kind 2, k_d2h, ctr 0]  (class F)                          │
  │──────────────────────────────────────────────────────────────────▶│ session UP: the READY proves
  │                                      ACK  [kind 2, k_h2d, ctr 0]  │ that D has K_link
  │◀──────────────────────────────────────────────────────────────────│
  │ session UP                                                        │
```

- `nonce_d` and `nonce_h` are 16 B and are **new in every handshake**. H must take them from a cryptographic generator. D should use the chip's TRNG; without a TRNG, it must use `nonce_d = HMAC-SHA256(K_link, "joker-link/1 nd" ‖ c)[0..16]`, with `c` a 64-bit counter (LE) incremented and persisted in flash **before** sending the offer, so that it never repeats. The 12 B nonce of the handshake frame is derived the same way, with `"joker-link/1 hn"`.
- The freshness of `nonce_d` protects D: without it, an attacker could replay an old answer, lead D to derive old keys and replay recorded session frames. That of `nonce_h` protects H.
- The 12 B nonces of the handshake frames are random (or derived from the same counter). With a fixed `K_hs`, the probability of a collision is negligible at the rate of handshakes (limited by §7.1).
- H replies to at most one `HELLO` per `kid` every 100 ms, to limit the cost of offers repeated by an attacker.
- Without `auth` (plaintext), the handshake is the same with `kind 0`: the nonces are sent anyway, but protect nothing.
- **No forward secrecy in v1**: whoever obtains `K_link` and has recorded the traffic decrypts it. Key `14` of `HELLO` is reserved for an ephemeral X25519 exchange in a 1.x (§18).

### 4.4 Counter, anti-replay and key update

**Counter.** Each direction has its own `ctr`, which starts at 0 in each key phase and goes up by 1 with each frame. A `ctr` is never reused with the same key.

**Anti-replay window** (per direction and per phase), in the style of RFC 6479:

1. Verify the tag. Failure: discard, counter `auth` (`J-E2101`).
2. With `max` = the largest accepted `ctr`: `ctr > max` accepts and slides the window; `max − 64 < ctr ≤ max` accepts only if the bit of that `ctr` is zero, and sets it; `ctr ≤ max − 64` discards, counter `replay` (`J-E2102`).

On ordered transports (serial, CAN, TCP), an out-of-order frame is always a sign of a problem; the rule is the same.

**Session key update** (per direction, without a message of its own):

- The sender flips the `phase` bit, starts encrypting with `k' = HKDF-Expand(k, "joker-link/1 ku", 32)` and restarts `ctr` at 0.
- The receiver, on seeing the flipped phase, derives `k'` and tries to decrypt. If the tag matches, it installs `k'` and keeps the previous key for 3 × `rto_max` (6 s) for delayed frames of the previous phase.
- A sender may only update again after receiving the acknowledgment (§5.3) of a frame already in the new phase.
- **When:** it must update before `ctr` reaches 2³² − 2¹⁶; it should update every 2³¹ frames or 24 h, whichever comes first. An exhausted `ctr` without an update ends the session (`J-E2105`).

### 4.5 Rotation of `K_link`: `KEYROT`

`K_link` changes at a new enrollment or by policy. H generates the new key and sends it in a `KEYROT` (§6.8), which only exists in session frames (encrypted with `k_h2d`). D stores it as **pending** in persistent memory and replies `KEYROT` stored. H sends `BYE(rehandshake)`. D does the next handshake with the pending key; if it ends in an acknowledged `READY`, D promotes it and erases the old one. If three consecutive handshakes with the pending key fail, D goes back to the old one (so as not to be isolated) and H logs `J-E2106`.

### 4.6 When it is required

| Transport | `auth` |
|---|---|
| UDP, TCP | **must** be `"required"`. `"none"` on an IP transport is `J0179` (ref/01) in `jk check`, and plaintext frames received are discarded (`J-E2104`) |
| UART, RS-485, USB CDC, CAN | `"required"` (default) or `"none"`. `"none"` is only acceptable on a closed bus inside a cabinet; it is the warning `J0738` in all modes, and it shows up in `jk doctor` (19 U10) |

There is no security negotiation: D (`[link] auth`, compiled into the generation) and H (the device's `auth` key) are configured; if they do not match, there is no session and both sides count refused frames. So there is no *downgrade* attack.

### 4.7 Enrollment and key storage

- `K_link` is born at MCU enrollment: `jk node enroll <mcu> --link <device>` (document 07; ref/10 §3.1.3), with the MCU connected by probe, by USB or already in `maintenance` over the link itself. `jk` generates the key, writes to the MCU records **16** (node name) and **17** (link key, keyed by the *path-hash* of the link name) of the `JK_AREA_IDENTITY` area (ref/08 §2.10.2), or hands it to a secure element (`link_key` callback, ref/08), and stores it on the Linux node as the secret `secret:link.<device>` (ref/00 §9, ref/02), encrypted for the node. A standalone MCU receives in the same enrollment its X25519 secrets key (record 19, ref/08 §2.10.2).
- Rotation of `K_link` after enrollment is `KEYROT` (§4.5).
- `K_link` never appears in logs, in the API, in `jk doctor` or in error messages. The `kid` may appear.

### 4.8 Test vectors

`K_link = 00 01 02 … 1f`, `nonce_d = 10 11 … 1f`, `nonce_h = 20 21 … 2f`, `sid = 1`:

```
kid     47d78759c31e7116
K_hs    773e29c63397f8122ebf0daba10addbe8bb96ca12fee10f037b64b34c476c278
k_d2h   34e79c45b75e7288b5e7c76e92fca2f304f6d063eb6d8b4a5eb02fb2d789738d
k_h2d   79631917c20c7f57af300949270905396b76bbc760d5d2513725029ff751f213
k_d2h'  e6fea0e448d8bd138bdc0ad699ced2ccac9295bdd179224804982065cf3d3948   (phase 1)
```

`PING` from D (message `04 10 0c 00 09 00 ff 00 00 00 a1 01 1a 00 12 ec 2e`, 17 B) encrypted with `k_d2h`, `ctr = 5`:

```
L2 frame  20 01 00 00 00 05 00 00 00 cb 54 0e 42 30 6a 12 41 1a 73 f3 9b 4e f2 79 b4 62
          15 78 81 d6 41 4e d6 f7 0d 1d 3e 25 e1 be b1 60                               (42 B)
UART      01 03 20 01 01 01 02 05 01 01 26 cb 54 0e 42 30 6a 12 41 1a 73 f3 9b 4e f2 79
          b4 62 15 78 81 d6 41 4e d6 f7 0d 1d 3e 25 e1 be b1 60 69 4f b2 dc 00          (49 B)
```

`HELLO` request from H (message `01 00 00 00 00 00 00 00 00 00 a1 01 02`), handshake, nonce `40 41 … 4b`:

```
L2 frame  10 47 d7 87 59 c3 1e 71 16 40 41 42 43 44 45 46 47 48 49 4a 4b 7f 91 30 3c bd
          d9 f5 49 98 3e 0c 16 b4 3e 55 36 a6 db 43 22 04 46 be f4 7b c8 e6 0c a9       (50 B)
```

### 4.9 Cost

ChaCha20-Poly1305 in software on a Cortex-M33 at 150 MHz: on the order of 15–25 cycles/B (to be measured in the MCU phase), that is ≈ 50 µs for a 350 B frame. HKDF and SHA-256 only in the handshake. There is no AES on the RP2350, and ChaCha20 is constant-time without hardware: that is why it is the choice of document 19.

---

## 5. Session and common header (L3)

### 5.1 Header

Each message starts with a binary header of 10 B, or 12 B with `REL`:

| Offset | Field | Type | Meaning |
|---|---|---|---|
| 0 | `type` | u8 | message type (§6.1) |
| 1 | `flags` | u8 | §5.2 |
| 2 | `seq` | u16 LE | packet number, per direction |
| 4 | `ack` | u16 LE | largest `seq` received from the other side (valid only with `AV`) |
| 6 | `ack_bits` | u32 LE | bit *i* = 1: packet `ack − 1 − i` was received |
| 10 | `rseq` | u16 LE | only with `REL`: number of the reliable message (§5.4) |

The body follows: **one** CBOR item (always a map with integer keys), or nothing (`ACK`). Extra bytes after the item are a message error (`J-E2006`).

The header is binary, and not CBOR, because the receiver must acknowledge, order and route packets without decoding the body, and because a fixed 10 B costs less to parse on an MCU than a map.

### 5.2 Flags

| Bit | Name | Meaning |
|---|---|---|
| 0 | `REL` | reliable message; the `rseq` field is present |
| 1 | `MORE` | fragment with continuation (§5.5) |
| 2 | `EOT` | end of turn in half-duplex (§3.3) |
| 3 | `ACKNOW` | requests an immediate `ACK`, without waiting `ack_delay` |
| 4 | `AV` | `ack` and `ack_bits` are valid |
| 5–7 | — | reserved: the sender sets 0, the receiver ignores |

`HELLO`s carry `seq = ack = ack_bits = 0` and `flags = 0`, and are not part of the numbering. The first packet in each direction after the handshake has `seq = 0` (the `READY` on D). Comparison of `seq` and `rseq` is 16-bit serial number arithmetic (RFC 1982).

### 5.3 Acknowledgment

- **Accepting.** A packet counts as received only after it passes L1, L2 (tag and window), has a valid header and, if it is `REL`, after the message has been delivered or stored in the window. A packet that the receiver did not store (full window, no memory) is **not** acknowledged.
- **Acknowledging.** After the handshake, all packets carry `AV` and the current receive state (`ack`, `ack_bits`). Packets of classes F and S (§5.4) request acknowledgment: if the receiver has nothing to send within `ack_delay`, it sends an `ACK`. Packets of class E and `ACK`s do not request it, but are acknowledged by piggybacking. `ACKNOW` forces an immediate `ACK`.
- **Declaring lost.** The sender declares a packet lost when it has not been acknowledged and (a) three packets sent after it have already been acknowledged, or (b) `RTO` has passed since it was sent.
- **RTT and RTO.** Each acknowledgment of a packet gives an RTT sample (a retransmission is always a new packet, with a new `seq`, so there is no Karn ambiguity). `RTO = max(rto_min, SRTT + 4·RTTVAR)`, limited to `rto_max` = 2 s, with the SRTT and RTTVAR of RFC 6298. Initial RTO = 10 × `rto_min`.

### 5.4 Delivery classes

| Class | Messages | If the packet is lost |
|---|---|---|
| **E**, ephemeral | `BYE`, `PING`, `PONG`, `ERROR`, `PROPOSE`, `TIME`, `TRACE` data, `OTA` data and `ack` | nothing; the next message replaces it, or the request is repeated |
| **S**, state | `SAMPLES` | the sender marks the signals of that packet as dirty again and sends the **current values** (never the old ones) |
| **F**, reliable | all the others | the message is resent in a new packet, with the same `rseq`, up to `retransmit` times |

**Class F.**

- `rseq` starts at 0 in each direction of each session and goes up by 1 per message (or fragment).
- At most `window` unacknowledged F messages in each direction. The receiver delivers them in `rseq` order; stores those that arrive out of order, within the window; discards without acknowledging those outside it; and discards (acknowledging) repeated ones.
- Once the `retransmit` retransmissions of a message are exhausted, the session ends: `BYE(error)`, `J-E2009`, state `LOST` (§7).

**Class S.** D keeps, for the last 8 unacknowledged `SAMPLES` packets, the set of `sid`s they carried. On declaring a packet lost, it marks those signals as dirty, unless they have already gone in a later packet. If the table fills up, the oldest packet counts as lost.

**Old messages.** On UDP, packets may arrive out of order. D discards a `PROPOSE` with a `seq` earlier than that of the last `PROPOSE` it has already consumed. H discards a sample whose 16-bit `seq` is earlier than the last accepted one for that signal.

### 5.5 Fragmentation

Only F messages are fragmented. A message that does not fit in a frame is split into fragments with consecutive `rseq`s; all but the last carry `MORE`, and all have the same `type`. The body of each fragment is a slice of the CBOR bytes of the whole message (it is not a CBOR item); the receiver concatenates them in `rseq` order. A reassembled message above `max_msg` is discarded (`J-E2008`, `ERROR` with status 10). E and S messages must fit in a frame: the sender splits them into several messages (several `SAMPLES`), and never splits a `PROPOSE` writer block (§6.12).

`max_msg`: minimum 2048, default 4096, maximum 65,535.

### 5.6 Session parameters and timers

H chooses them in the answer `HELLO`, within D's capabilities. Defaults:

| Parameter | Serial, USB, CAN | UDP, TCP | Meaning |
|---|---|---|---|
| `max_frame` | 1024 | 1024 | §3.7 |
| `max_msg` | 4096 | 4096 | §5.5 |
| `window` | 4 | 8 | unacknowledged F messages (1–16) |
| `keepalive` | 50 ms | 1 s | with nothing sent for this time, a `PING` is sent |
| `link_timeout` | 200 ms | 5 s | with no valid packet received for this time, the link is lost (§7) |
| `report` | 10 ms | 10 ms | minimum interval between `SAMPLES` with changes |
| `refresh` | 1 s | 5 s | period of the full resend of the signals (§6.9); 0 turns it off |
| `max_lease` | 5 s | 5 s | ceiling of a remote writer's lease (§8.1) |
| `rto_min` | 10 ms | 100 ms | §5.3 |
| `retransmit` | 8 | 8 | retransmissions of an F message before giving up (it is not the common `retries` key of devices, which counts repetitions of core requests, ref/01 §5.8) |
| `ack_delay` | 2 ms | 20 ms | §5.3 |
| `time_interval` | 10 s | 10 s | period of the `TIME` probes (§8.4); 0 turns it off |
| `bulk_max` | 256 | 1024 | maximum payload of an `OTA`, `TRACE` or `CATALOG` chunk (§5.7) |
| `turn_frames` | 4 | — | half-duplex only (§3.3) |

`link_timeout` must be ≥ 3 × `keepalive`.

### 5.7 Send priority

Each side sends in the following order, and a class only advances when those above it have nothing ready:

1. `HELLO`, `READY`, `BYE`, `ERROR`, `PONG`, due `ACK`;
2. `PROPOSE` (H), `ALARMS` (D);
3. `RESULT`, `OUTCOMES`, `CALL`, `ACTION`, `CANCEL`, `ALARM_CMD`, `RESET`, `RESTART`, `TIME`, `KEYROT`, `PING`;
4. `SAMPLES`, `READ`;
5. `EVENTS`;
6. `CATALOG`, `TRACE`, `OTA` (bulk), in chunks of at most `bulk_max` B.

`bulk_max` limits the time that a chunk already on the wire can delay a `PROPOSE` or a `SAMPLES` (it is the `t_bulk` term of the liveness condition, §11.6). On CAN, bulk goes on its own identifier and does not block control (§3.4).

---

## 6. Messages

### 6.1 General table

| Type | Name | Direction | Class | Modes (§7.3) | Response | Minimum role at the source | Version |
|---|---|---|---|---|---|---|---|
| `0x00` | — | — | — | — | — | — | never used |
| `0x01` | `HELLO` | ↔ | handshake | — | `HELLO` | — | 1.0 |
| `0x02` | `READY` | D → H | F | all | — | — | 1.0 |
| `0x03` | `BYE` | ↔ | E | all | — | — | 1.0 |
| `0x04` | `PING` | ↔ | E | all | `PONG` | — | 1.0 |
| `0x05` | `PONG` | ↔ | E | all | — | — | 1.0 |
| `0x06` | `ACK` | ↔ | — | all | — | — | 1.0 |
| `0x07` | `ERROR` | ↔ | E | all | — | — | 1.0 |
| `0x08` | `KEYROT` | ↔ | F | all, `kind 2` only | `KEYROT` | `admin` | 1.0 |
| `0x10` | `CATALOG` | ↔ | F | all | `CATALOG` | `viewer` | 1.0 |
| `0x11` | `READ` | H → D | F | full, observe | `SAMPLES` | `viewer` | 1.0 |
| `0x12` | `SAMPLES` | D → H | S | full, observe | — | — | 1.0 |
| `0x13` | `PROPOSE` | H → D | E | full | `OUTCOMES` | the writer's (node manifest) | 1.0 |
| `0x14` | `OUTCOMES` | D → H | F | full | — | — | 1.0 |
| `0x15` | `CALL` | H → D | F | full | `RESULT` | the writer's; `operator` in manual | 1.0 |
| `0x16` | `ACTION` | H → D | F | full | `RESULT` | the action's `role` (MCU manifest) | 1.0 |
| `0x17` | `RESULT` | D → H | F | all | — | — | 1.0 |
| `0x18` | `CANCEL` | H → D | F | full | `RESULT` | the action's | 1.0 |
| `0x19` | `EVENTS` | D → H | F | full, observe | — | — | 1.0 |
| `0x1A` | `ALARMS` | D → H | F | full, observe | — | — | 1.0 |
| `0x1B` | `ALARM_CMD` | H → D | F | full | `RESULT` | `operator` | 1.0 |
| `0x1C` | `RESET` | H → D | F | full | `RESULT` | `operator` | 1.0 |
| `0x1D` | `RESTART` | H → D | F | all | `RESULT`, then `BYE` | `admin` | 1.0 |
| `0x20` | `TIME` | ↔ | E | all | `TIME` | — | 1.0 |
| `0x21` | `TRACE` | ↔ | F (data: E) | all | `TRACE` | `viewer`; `operator` to freeze and release | 1.0 |
| `0x22` | `OTA` | ↔ | F (data and `ack`: E) | all | `OTA` | `admin` | 1.0 |
| `0x30` | `BLOCKS` | D → H | — | — | — | — | **reserved 1.1** (block signals, M7) |
| `0x31`–`0x3F` | — | — | — | — | — | — | reserved 1.x |
| `0x40`–`0xDF` | — | — | — | — | — | — | reserved |
| `0xE0`–`0xFF` | private | — | — | — | — | — | never standardized; a normal peer ignores them |

- **Unknown type:** the receiver ignores the message; if it has `REL`, it acknowledges it (so as not to block the window) and replies `ERROR` with status 2 (`J-E2007`), once per type and per session.
- **Message outside the mode** ("Modes" column): `ERROR` with status 5 (`J-E2208`).
- **Role.** The role is checked on the Linux node, which knows the user (ref/09, ref/00 §9). Messages with `role` carry the checked role, and D compares it again with what its generation requires (defense in depth): an insufficient role gives `RESULT` with status 12 (`J-E2313`).

### 6.2 Common encoding

**Emission:** deterministic CBOR (RFC 8949, §4.2.1): maps with keys in increasing byte order, integers and lengths in the shortest form, `float` in the shortest form that represents the value **exactly** (half, single or double precision; `NaN` = `f9 7e 00`), no indefinite lengths, no tags beyond those indicated. **Reading:** any well-formed CBOR; unknown keys ignored; `float` of any width; an unknown enum value counts as unknown (ref/00 §1). Absent keys have the default of the message's table.

**Values** (the signal type comes from the catalog, §6.3):

| Signal type (ref/00 §6) | CBOR | Note |
|---|---|---|
| `bool` | `true` / `false` | |
| `i64` | integer (major type 0 or 1) | |
| `u64` | unsigned integer | |
| `f64` | `float` | the shortest exact form: `412.5` → `f9 5e 72` (3 B) |
| `f32` | `float` | likewise; never needs double precision |
| `enum<N>` | unsigned integer | the numeric value of the variant |
| `bits<N>` | unsigned integer | the word |
| `string` | `tstr` ≤ 64 B | |
| `bytes` | `bstr` ≤ 64 B | |

A value with the wrong CBOR type for the signal is `Rejected(WrongType)` in a proposal, and is ignored with the `malformed` counter in a sample.

**Time** (ref/00 §3): MCU instants in monotonic µs since MCU boot (`mono-us`, u64); UTC in µs (`utc-us`, s64), only after a `TIME` (§8.4); durations in ms (`dur-ms`, u32) or in µs when the name says so. Within a batch, instants are `t0` (absolute) plus a `dt` in µs (u32).

**Quality** (ref/00 §4.3): u32 with the code in bits 0–7, the reason in bits 8–15 and the flags in bits 16–31, as in the slot (01 §3.2). `Uncertain(Stale)` = `1 | 3 << 8` = 769. The private reasons 240–255 leave the MCU as 0.

**Records** (arguments, results, event fields): a deterministic CBOR map with **integer keys**, the position of the field in the schema (the class's `.jkc` contract, or the app's `joker:meta`), counting from 0 (D2, ref/00 §4.1, ref/04 §4.10). It is the same form that crosses WIT, JDP and the C ABI; the link carries it without rewriting it, and only the text interfaces show the names. Limits of ref/00 §10: strings ≤ 1 KiB, lists ≤ 64 elements.

**Identifiers:** `sid` (u32) is the `SignalId` in the **MCU's** generation; `iid`, `mid`, `aid`, `eid`, `alid`, `gid` are catalog indices (§6.3). All are valid only with the session's catalog hash (ref/00 §5). The `origin` is the writer's identifier in the **Linux node's** generation (u32), and is only informative for the MCU (§8.1).

### 6.3 The catalog

The catalog is everything the Linux node needs to generate the MCU profile (19 §6): signals, instances, methods, actions, events, alarms and groups. It is a deterministic CBOR item, `catalog` in the CDDL, generated by `jk build --target mcu:…` and stored **as is** in the MCU's generation (`CATALOG` section of the blob, ref/08 §2.13.3). The **catalog hash** is the SHA-256 of those bytes; it goes in `HELLO` (key 8) and is the `digest` of `[discovered.<d>]` for a `joker-link` device (ref/02).

| Key | Content | Fields (key: meaning) |
|---|---|---|
| 1 | signals | 1 `sid`; 2 path on the MCU; 3 type (ref/00 §6); 4 unit (`"1"` = dimensionless); 5 direction (`input` 0, `output` 1, `internal` 2, `param` 3, `derived` 4); 6 `gid` of the owning group; 7 enum variants or bit names `[[value, name]]`; 8 levels delegated to the link (outputs: bit *L−1* = level *L*); 9 `range` and 10 `safe` (informative, for `jk write` to show the envelope); 11 `max_age` in ms; 12 quantity; 13 width *N* of `enum<N>`/`bits<N>` |
| 2 | instances | 1 `iid`; 2 name; 3 class (`"std/pump@1"`); 4 full version; 5 contract hash; 6 capabilities; 7 members `[[name, sid]]`; 8 class members `[[name, [iid…]]]` (one `iid` in a fixed member, the list in the collection's order); 9 member only (`true`: the instance has no top-level name on the node) |
| 3 | methods | 1 `mid`; 2 path (`"p1.run_at"`); 3 `iid`; 4 `atomic` |
| 4 | actions | 1 `aid`; 2 path (`"p1.reset"`, `"pcs.reset_fault"`); 3 `iid` (absent in a device action); 4 required role; 5 timeout in ms; 6 `@long` |
| 5 | events | 1 `eid`; 2 path; 3 `iid`; 4 fields `[[name, type, unit]]` |
| 6 | alarms | 1 `alid`; 2 name; 3 severity (ref/00 §6); 4 message; 5 `sid` of the `alarm.<name>.state` signal |
| 7 | groups | 1 `gid`; 2 name; 3 period in µs |
| 8 | MCU name | the same as the identity in `HELLO` |

- The indices (`sid`, `iid`, `mid`, `aid`, `eid`, `alid`, `gid`) are dense, start at 0 and follow the order of the MCU registry (canonical path, 01 §3.1). Only the signals the MCU exports appear; the apps' internal ones (`#[internal]`) do not leave.
- **Composite instances** (document 17, §8). An instance with class members carries the tree in key 2.8, with the `iid`s of the bound instances; each of them is also in list 2, with its signals. An instance that on the MCU is a member of another carries 2.9 = `true`. An unnamed submember (ref/03 §2.2.14) has the nested path as its name (`hp.compressor`). The lists of methods (3), actions (4) and events (5) have the top-level paths of each one.
- The MCU's `joker.*` signals (metrics, 01 §19) are in the catalog like the others. The minimum set is `joker.link.rx_frames`, `joker.link.crc_errors`, `joker.link.auth_errors`, `joker.link.retransmits`, `joker.micro.boot_count` and `joker.group.<g>.overruns`.
- The catalog is transferred by `CATALOG` (§6.11), in pieces, and H caches it by hash. An H that already has it (cache, or because the node's generation brings it from `jk build`) does not request it.

### 6.4 `HELLO` (`0x01`)

Three phases in the same type (key 1). Always in handshake frames (`kind 1`) or in plaintext (`kind 0`); never fragmented; it must fit in 512 B of L2 frame.

**Offer** (phase 0, D → H):

| Key | Field | Type | Req. | Meaning |
|---|---|---|---|---|
| 1 | phase | 0 | yes | |
| 2 | versions | `[+ [major, max_minor]]` | yes | the majors D speaks and the highest minor of each (§13) |
| 3 | `nonce_d` | `bstr` 16 | yes | §4.3 |
| 4 | identity | map | yes | 1 MCU name (DNS label); 2 hardware id (4–32 B); 3 board |
| 5 | firmware | map | yes | 1 version; 2 image digest (SHA-256); 3 target; 4 `JK_ABI_VERSION` `[maj, min]`; 5 capabilities hash (ref/08 §2.14); 6 Joker Micro version; 7 compiled features; 8 capabilities: map `cap_id` → value with the numbering and names of ref/08 §2.13.3, with at least `signals` (1), `outputs` (3), `groups` (4), `instances` (16), `trace_bytes` (36) and `kv_bytes` (37); 9 firmware descriptor digest (SHA-256 of `firmware.jkfw`, ref/08 §2.14); 10 confirmed `security_version` (anti-rollback, §9.5) |
| 6 | generation | map | no | 1 `gen_seq` of the blob; 2 generation hash (the blob's `source_hash`, ref/08 §2.13.2); 3 state (ref/00 §6 `generation_state`); 4 origin (`embedded` 0, `flash` 1). Absent: D has no valid generation |
| 7 | registry hash | `bstr` 32 | with 6 | the blob's `registry_hash`, the one of the trace (16 §7) |
| 8 | catalog hash | `bstr` 32 | with 6 | §6.3 |
| 9 | contracts | `[* [class, version, hash]]` | no | the classes the MCU exports (19 §6, U7) |
| 10 | truncated | bool | no, `false` | `true`: list 9 did not fit; H reads it from the catalog |
| 11 | link capabilities | map | yes | 1 `max_frame`; 2 `max_msg`; 3 `window`; 4 features (§6.4.1); 5 deduplication capacity; 6 remote writer capacity; 7 trace ring bytes |
| 12 | boot | map | yes | 1 boot counter; 2 reason for the last reset; 3 active firmware slot; 4 firmware on trial; 5 result of the last firmware update; 6 generation slot; 7 generation on trial; 8 ms left in the confirmation window |
| 13 | now | `mono-us` | yes | MCU clock at send time |
| 14 | — | `bstr` 32 | no | **reserved 1.x** (ephemeral X25519) |
| 15 | drivers | `bstr` 32 | with planned drivers | SHA-256 of the deterministic CBOR of `[* [name, major, descriptor digest]]`, by name: the MCU drivers that `jk build` planned, with the descriptor digest it planned with (ref/06, ref/08 §2.13.3 `DEVICES`) |
| 16 | changed parameters | `[* [path-hash, v]]` | no | up to 8 app parameters overridden locally (`jk_param_set`, ref/08 §2.6.l): drift relative to Git |
| 17 | truncated parameters | bool | no, `false` | `true`: there are more than 8; H reads the rest by `READ` of the `param` signals |

Reset reasons: `unknown` 0, `power_on` 1, `pin` 2, `watchdog` 3, `software` 4, `brownout` 5, `ota` 6, `fault` 7.

#### 6.4.1 Features (key 11.4)

| Bit | Feature |
|---|---|
| 0 | encrypted frames (`kind 1` and `2`) |
| 1 | plaintext frames |
| 2 | half-duplex |
| 3 | hot generation swap (`jk_generation_swap`) |
| 4 | `TRACE` |
| 5 | generation `OTA` |
| 6 | firmware `OTA` |
| 7 | `ACTION` |
| 8 | long actions (`@long`) |
| 9 | `TIME` |
| 10 | `KEYROT` |
| 11–31 | reserved (0) |

An H does not send a message whose feature D does not declare (`ERROR` status 5 if it does).

**Answer** (phase 1, H → D):

| Key | Field | Type | Req. | Meaning |
|---|---|---|---|---|
| 1 | phase | 1 | yes | |
| 2 | version | `[major, minor]` | yes | the chosen one (§13) |
| 3 | `nonce_h` | `bstr` 16 | yes | |
| 4 | echo | `bstr` 16 | yes | the offer's `nonce_d`; D discards the answer if it does not match |
| 5 | mode | uint | yes | `full` 0, `observe` 1, `maintenance` 2, `refused` 3 (§7.3) |
| 6 | `sid` | u32 ≠ 0 | if not `refused` | chosen by H, random, unique among its sessions |
| 7 | parameters | map | if not `refused` | 1 `max_frame`; 2 `max_msg`; 3 `window`; 4 `keepalive`; 5 `link_timeout`; 6 `report`; 7 `refresh`; 8 `max_lease`; 9 `rto_min`; 10 `retransmit`; 11 `ack_delay`; 12 `time_interval`; 13 `bulk_max`; 14 `turn_frames` (§5.6; ms) |
| 8 | status | uint | no | why it is not `full` (§10.1) |
| 9 | code | `tstr` | no | `J-E22xx` |
| 10 | detail | `tstr` | no | |

**Request** (phase 2, H → D): `{1: 2}`. Asks D to restart the handshake (for example, `jokerd` restarted and lost the session). D only obeys it if it has not received any valid session packet in the last 2 × `keepalive`; that way, a request repeated by an attacker does not bring down a live session.

Example offer (MCU `io-rp2350-01`, generation 7, one class):

```cbor-diag
{
  1: 0,
  2: [[1, 0]],
  3: h'101112131415161718191a1b1c1d1e1f',
  4: {1: "io-rp2350-01", 2: h'e66038b7c3a2f12d', 3: "acme/io16-rp2350"},
  5: {1: "1.0.3", 2: h'e5c2ed6fe0a694040f93b1e0bfaa1b140df0e51de29f6903f76ac093089ca470', 3: "acme/io16-rp2350", 6: "1.0.3", 7: ["gpio", "modbus-rtu"]},
  6: {1: 7, 2: h'69e2071fe2dbff9a0f8a270b5d88c4b886a9b048d61f21ec6793faef6adbbd85', 3: 2, 4: 1},
  7: h'9be86532d0f649500bb9e54961364246fc50c2ab410232ef8a8b5c2dd52b5976',
  8: h'25c7c73ece719aafc154ea525c9d0616cf4499a4297d1f703cfa06e10bbb3d63',
  9: [["std/relay@1", "1.0.0", h'36df9772a6e35a5decb09ff8bbc1a1aab2b9266c65c2bebb638de85d2ef6514a']],
  11: {1: 1024, 2: 4096, 3: 4, 4: 1023, 5: 16, 6: 8, 7: 16384},
  12: {1: 42, 2: 1, 3: 0, 4: false, 5: 2},
  13: 1234567
}
```

```
header     01 00 00 00 00 00 00 00 00 00
body       ac 01 00 02 81 82 01 00 03 50 10 11 12 13 14 15 16 17 18 19 1a 1b 1c 1d
           1e 1f 04 a3 01 6c 69 6f 2d 72 70 32 33 35 30 2d 30 31 02 48 e6 60 38 b7
           c3 a2 f1 2d 03 70 61 63 6d 65 2f 69 6f 31 36 2d 72 70 32 33 35 30 05 a5
           01 65 31 2e 30 2e 33 02 58 20 e5 c2 ed 6f e0 a6 94 04 0f 93 b1 e0 bf aa
           1b 14 0d f0 e5 1d e2 9f 69 03 f7 6a c0 93 08 9c a4 70 03 70 61 63 6d 65
           2f 69 6f 31 36 2d 72 70 32 33 35 30 06 65 31 2e 30 2e 33 07 82 64 67 70
           69 6f 6a 6d 6f 64 62 75 73 2d 72 74 75 06 a4 01 07 02 58 20 69 e2 07 1f
           e2 db ff 9a 0f 8a 27 0b 5d 88 c4 b8 86 a9 b0 48 d6 1f 21 ec 67 93 fa ef
           6a db bd 85 03 02 04 01 07 58 20 9b e8 65 32 d0 f6 49 50 0b b9 e5 49 61
           36 42 46 fc 50 c2 ab 41 02 32 ef 8a 8b 5c 2d d5 2b 59 76 08 58 20 25 c7
           c7 3e ce 71 9a af c1 54 ea 52 5c 9d 06 16 cf 44 99 a4 29 7d 1f 70 3c fa
           06 e1 0b bb 3d 63 09 81 83 6b 73 74 64 2f 72 65 6c 61 79 40 31 65 31 2e
           30 2e 30 58 20 36 df 97 72 a6 e3 5a 5d ec b0 9f f8 bb c1 a1 aa b2 b9 26
           6c 65 c2 be bb 63 8d e8 5d 2e f6 51 4a 0b a7 01 19 04 00 02 19 10 00 03
           04 04 19 03 ff 05 10 06 08 07 19 40 00 0c a5 01 18 2a 02 01 03 00 04 f4
           05 02 0d 1a 00 12 d6 87
```

(10 B of header + 368 B of body = 378 B of message.)

Answer, `full` mode, `sid = 1`:

```cbor-diag
{
  1: 1,
  2: [1, 0],
  3: h'202122232425262728292a2b2c2d2e2f',
  4: h'101112131415161718191a1b1c1d1e1f',
  5: 0,
  6: 1,
  7: {1: 1024, 2: 4096, 3: 4, 4: 50, 5: 200, 6: 10, 7: 1000, 8: 5000, 9: 10, 10: 8, 11: 2, 12: 10000, 13: 256}
}
```

```
header     01 00 00 00 00 00 00 00 00 00
body       a7 01 01 02 82 01 00 03 50 20 21 22 23 24 25 26 27 28 29 2a 2b 2c 2d 2e
           2f 04 50 10 11 12 13 14 15 16 17 18 19 1a 1b 1c 1d 1e 1f 05 00 06 01 07
           ad 01 19 04 00 02 19 10 00 03 04 04 18 32 05 18 c8 06 0a 07 19 03 e8 08
           19 13 88 09 0a 0a 08 0b 02 0c 19 27 10 0d 19 01 00
```

(10 B of header + 89 B of body = 99 B of message.)

Request:

```cbor-diag
{1: 2}
```

```
header     01 00 00 00 00 00 00 00 00 00
body       a1 01 02
```

(10 B of header + 3 B of body = 13 B of message.)

### 6.5 `READY` (`0x02`)

D → H, class F, `rseq = 0`, the first session frame. Confirms the mode and proves possession of the keys. `{1: mode (echo), 2: now (mono-us)}`. After H's `ACK`, D sends a full `SAMPLES` (`refresh`) and an `ALARMS` with `snapshot`.

```cbor-diag
{1: 0, 2: 1240110}
```

```
header     02 01 00 00 00 00 00 00 00 00 00 00
body       a2 01 00 02 1a 00 12 ec 2e
```

(12 B of header + 9 B of body = 21 B of message.)

### 6.6 `BYE` (`0x03`)

↔, class E. Closes the session. `{1: reason, ? 2: code, ? 3: detail}`. Reasons: `shutdown` 0, `rehandshake` 1, `generation_change` 2, `reboot` 3, `ota_activate` 4, `error` 5. The receiver erases the session keys; D goes back to `IDLE` (§7.1). Remote leases are **not** dropped by a `BYE` (§7.4).

```cbor-diag
{1: 2}
```

```
header     03 10 8f 03 4d 00 ff ff ff ff
body       a1 01 02
```

(10 B of header + 3 B of body = 13 B of message.)

### 6.7 `PING` (`0x04`), `PONG` (`0x05`), `ACK` (`0x06`)

- `PING` {1: t (µs, monotonic clock of the sender)}: sent when nothing has been sent for `keepalive`. Class E.
- `PONG` {1: echo of t, 2: t of the responder}: immediate response to a `PING`. Gives an RTT to whoever pinged.
- `ACK`: empty body (0 B); only the header counts. Never requests acknowledgment.

```cbor-diag
{1: 1240110}
```

```
header     04 10 0c 00 09 00 ff 00 00 00
body       a1 01 1a 00 12 ec 2e
```

(10 B of header + 7 B of body = 17 B of message.)

```cbor-diag
{1: 1240110, 2: 88012345}
```

```
header     05 10 0a 00 0c 00 ff ff 00 00
body       a2 01 1a 00 12 ec 2e 02 1a 05 3e f6 39
```

(10 B of header + 13 B of body = 23 B of message.)

```cbor-diag
(empty body)
```

```
header     06 10 0d 00 0b 00 ff ff ff ff
body       (empty)
```

(10 B of header + 0 B of body = 10 B of message.)

### 6.8 `KEYROT` (`0x08`)

Only in session frames (§4.5). Class F.

| Phase (key 1) | Direction | Fields |
|---|---|---|
| 0, offer | H → D | 2 new `kid` (8 B); 3 new `K_link` (32 B) |
| 1, stored | D → H | 2 stored `kid` |

```cbor-diag
{
  1: 0,
  2: h'e1092152bcfa5913',
  3: h'202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f'
}
```

```
header     08 11 32 00 3c 00 00 00 00 00 05 00
body       a3 01 00 02 48 e1 09 21 52 bc fa 59 13 03 58 20 20 21 22 23 24 25 26 27
           28 29 2a 2b 2c 2d 2e 2f 30 31 32 33 34 35 36 37 38 39 3a 3b 3c 3d 3e 3f
```

(12 B of header + 48 B of body = 60 B of message.)

```cbor-diag
{1: 1, 2: h'e1092152bcfa5913'}
```

```
header     08 11 3d 00 32 00 00 00 00 00 09 00
body       a2 01 01 02 48 e1 09 21 52 bc fa 59 13
```

(12 B of header + 13 B of body = 25 B of message.)

### 6.9 `SAMPLES` (`0x12`)

D → H, class S. The samples of the exported signals.

| Key | Field | Type | Req. | Meaning |
|---|---|---|---|---|
| 1 | `t0` | `mono-us` | yes | time base of the batch: the oldest instant of the batch |
| 2 | samples | `[* sample]` | yes | each `sid` at most once per message |
| 3 | flags | uint | no, 0 | bit 0: carries `refresh` entries; bit 1: response to `READ`; bit 2: last message of the response to `READ` |
| 4 | request | u16 | with bit 1 | the `rseq` of the `READ` |
| 5 | coalesced | u32 | no, 0 | MCU samples that were not sent because a newer one of the same signal replaced them before sending |

`sample = [sid, seq, v, ? dt, ? q, ? ts_src]`:

| Position | Field | Type | Default | Meaning |
|---|---|---|---|---|
| 0 | `sid` | u32 | — | |
| 1 | `seq` | u16 | — | the low 16 bits of the sample's `seq` on the MCU |
| 2 | `v` | value | — | §6.2 |
| 3 | `dt` | u32 µs | 0 | instant of the sample = `t0 + dt`: the MCU's `ts_hw` if it exists, otherwise its `ts_acq` |
| 4 | `q` | u32 | 0 | quality (§6.2) |
| 5 | `ts_src` | `utc-us` or `null` | absent | UTC of the source (a piece of equipment behind the MCU that provides a timestamp) |

Missing positions at the end have the default; to give `q` without `dt`, write `dt = 0`. Extra positions are ignored.

**When D sends:**

1. **Changes:** a signal becomes dirty when its `seq` advances on the MCU. With dirty signals and at least `report` since the last `SAMPLES`, D sends them (several `SAMPLES` if they do not fit in one frame).
2. **Refresh:** each exported signal goes at least once every `refresh`, even without changing, spread over the period (flag bit 0).
3. **After `READY`:** all signals (flag bit 0).
4. **Response to `READ`** (flag bits 1 and 2, key 4).
5. **Loss:** §5.4, class S.

`refresh` is what keeps fresh on the Linux node a signal that does not change: for `jokerd`, each entry is a new sample, like a poll (01 §3.1). That is why the default `max_age` of the signals of a `joker-link` device is 3 × `refresh` (`J0739` if it is less than 2 × `refresh`).

Example: two digital inputs and a power value with `Uncertain(Stale)`, 1.8 ms after `t0`:

```cbor-diag
{
  1: 1240000,
  2: [[0, 1201, true], [1, 877, false, 250], [40, 5120, 412.5, 1800, 769]]
}
```

```
header     12 10 2c 03 92 01 ff ff ff ff
body       a2 01 1a 00 12 eb c0 02 83 83 00 19 04 b1 f5 84 01 19 03 6d f4 18 fa 85
           18 28 19 14 00 f9 5e 72 19 07 08 19 03 01
```

(10 B of header + 38 B of body = 48 B of message.)

### 6.10 `READ` (`0x11`)

H → D, class F. Requests the current value of signals, changed or not (for example `jk read --refresh` of a `param`, 02 §8; or a resynchronization). `{? 1: [* sid] (absent: all), ? 2: bool (also an ALARMS snapshot)}`. Response: one or more `SAMPLES` with flag bit 1 and key 4; the last one with bit 2. An unknown `sid` gives `ERROR` status 6.

```cbor-diag
{1: [40, 41]}
```

```
header     11 11 03 00 05 00 00 00 00 00 02 00
body       a1 01 82 18 28 18 29
```

(12 B of header + 7 B of body = 19 B of message.)

### 6.11 `CATALOG` (`0x10`)

Class F. Transfer of the catalog bytes by ranges.

| Phase (1) | Direction | Fields |
|---|---|---|
| 0, request | H → D | 2 `offset` (u32); 3 `maxlen` (u16, ≤ `bulk_max`) |
| 1, data | D → H | 2 `offset`; 3 `total` (u32); 4 bytes (`bstr`, ≤ `maxlen`) |

H requests in order until `offset + len = total`, computes the SHA-256 and compares it with key 8 of `HELLO`. If it does not match, the catalog is invalid: `J-E2203`, `BYE(error)`, and the next handshake receives the `maintenance` mode.

```cbor-diag
{1: 0, 2: 0, 3: 900}
```

```
header     10 11 01 00 00 00 00 00 00 00 00 00
body       a3 01 00 02 00 03 19 03 84
```

(12 B of header + 9 B of body = 21 B of message.)

```cbor-diag
{1: 1, 2: 0, 3: 1873, 4: h'a8012e2e2e'}
```

```
header     10 11 02 00 01 00 00 00 00 00 01 00
body       a4 01 01 02 00 03 19 07 51 04 45 a8 01 2e 2e 2e
```

(12 B of header + 16 B of body = 28 B of message.)

### 6.12 `PROPOSE` (`0x13`)

H → D, class E. The proposals of the Linux node's writers on the MCU's outputs. It is the image, on the wire, of the proposal buffer of one tick (01 §3.4).

`{1: [+ writer_block]}`, with `writer_block = [origin, level, kind, lease, props, ? calls]`:

| Position | Field | Type | Meaning |
|---|---|---|---|
| 0 | `origin` | u32 | the writer on the Linux node (ref/00 §5); informative |
| 1 | `level` | 1–16 | the level in the MCU's arbiter (A1) |
| 2 | `kind` | uint | `writer_kind` (ref/00 §6) |
| 3 | `lease` | u32 ms, ≥ 1 | A2; limited to `max_lease` (§8.1) |
| 4 | `props` | `[* [sid, v, ? at]]` | `v = null` is `relinquish()` at that level (01 §6); `at` **reserved 1.1** (scheduled writes, M6): a 1.0 D rejects the proposal with `wrong_type` |
| 5 | `calls` | `[* [mid, args]]` | method records (17 §4.1); `args` is a record or `null` |

- A block (one writer) is never split across messages. If the outputs of an atomic group of the MCU are written by the same writer, they must go in the same block (`J0735` if it does not fit in a frame).
- **When H sends** (§8.1): when the proposed value changes, and at least every `renew = max(1 ms, lease / 3)`, rounded to the writer's tick.

Example: writer 17 (`control:10`, cycle, lease 30 ms) writes two digital outputs; writer 23 (`manual:4`) releases output 18 and calls method 3 with `pct = 60 %` (the method's only argument: key 0):

```cbor-diag
{
  1: [[17, 10, 0, 30, [[16, true], [17, false]]], [23, 4, 2, 5000, [[18, null]], [[3, {0: 60.0}]]]]
}
```

```
header     13 10 92 01 2c 03 ff ff ff ff
body       a1 01 82 85 11 0a 00 18 1e 82 82 10 f5 82 11 f4 86 17 04 02 19 13 88 81
           82 12 f6 81 82 03 a1 00 f9 53 80
```

(10 B of header + 35 B of body = 45 B of message.)

### 6.13 `OUTCOMES` (`0x14`)

D → H, class F. The results that the **MCU's** envelope decided for the remote proposals.

| Key | Field | Type | Req. | Meaning |
|---|---|---|---|---|
| 1 | `ref` | u16 | yes | `seq` of the last `PROPOSE` consumed up to this tick |
| 2 | `t` | `mono-us` | yes | instant of the MCU tick |
| 3 | results | `[* [sid, origin, level, o, ? applied, ? detail]]` | yes | only those that changed (§8.2) |
| 4 | expired | `[* [sid, origin, level]]` | no | remote leases that expired in this tick (A2) |
| 5 | lost | u32 | no, 0 | results that were not sent for lack of window |

`o` is the code of ref/00 §4.4. `applied` is the applied value, present when it differs from the proposed one (`null` = equal to the proposed one, only so that `detail` can be given). `detail` is the reason (`rejected`) or the active level (`overridden`).

```cbor-diag
{
  1: 402,
  2: 1240900,
  3: [[16, 17, 10, 1], [17, 17, 10, 5, true, 3], [20, 23, 4, 3, 12.5]],
  4: [[19, 31, 13]]
}
```

```
header     14 10 2d 03 92 01 ff ff ff ff
body       a4 01 19 01 92 02 1a 00 12 ef 44 03 83 84 10 11 0a 01 86 11 11 0a 05 f5
           03 85 14 17 04 03 f9 4a 40 04 81 83 13 18 1f 0d
```

(10 B of header + 40 B of body = 50 B of message.)

(Output 16 `accepted`; output 17 `overridden` by level 3, which applies `true`; output 20 `clamped_rate` to 12.5; the lease of writer 31 at level 13 of output 19 expired.)

### 6.14 `CALL` (`0x15`)

H → D, class F. A call to a class method, outside the cycle: `jk call`, a bridge, an async task that waits for the result (17 §2.1). Calls from cycle tasks go in `PROPOSE` (§6.12).

| Key | Field | Type | Req. | Meaning |
|---|---|---|---|---|
| 1 | `id` | u32 | yes | of the request; comes back in `RESULT` |
| 2 | `mid` | u16 | yes | catalog |
| 3 | `origin` | u32 | yes | |
| 4 | `level` | 1–16 | yes | |
| 5 | `kind` | uint | yes | `writer_kind` |
| 6 | `lease` | u32 ms | yes | of the expanded writes |
| 7 | `args` | record | no | |
| 8 | key | `tstr` 1–64 | no | idempotency (§8.5) |

```cbor-diag
{1: 9001, 2: 3, 3: 23, 4: 4, 5: 2, 6: 5000, 7: {0: 60.0}}
```

```
header     15 11 93 01 2d 03 00 00 00 00 07 00
body       a7 01 19 23 29 02 03 03 17 04 04 05 02 06 19 13 88 07 a1 00 f9 53 80
```

(12 B of header + 23 B of body = 35 B of message.)

### 6.15 `ACTION` (`0x16`)

H → D, class F. Invokes a device or instance action (02 §8, 17 §4.2).

| Key | Field | Type | Req. | Meaning |
|---|---|---|---|---|
| 1 | `id` | u32 | yes | |
| 2 | `aid` | u16 | yes | catalog |
| 3 | `args` | record | no | |
| 4 | key | `tstr` 1–64, printable ASCII | **yes** | idempotency (ref/00 §5) |
| 5 | `role` | uint | yes | the checked role of the requester |
| 6 | `timeout` | u32 ms | no | may shorten the MCU's, never lengthen it |

```cbor-diag
{1: 9002, 2: 1, 4: "01J8Z3K4M5N6P7Q8R9S0T1V2W3", 5: 1}
```

```
header     16 11 94 01 2d 03 00 00 00 00 08 00
body       a4 01 19 23 2a 02 01 04 78 1a 30 31 4a 38 5a 33 4b 34 4d 35 4e 36 50 37
           51 38 52 39 53 30 54 31 56 32 57 33 05 01
```

(12 B of header + 38 B of body = 50 B of message.)

### 6.16 `RESULT` (`0x17`)

D → H, class F. The response to `CALL`, `ACTION`, `CANCEL`, `ALARM_CMD`, `RESET` and `RESTART`.

| Key | Field | Type | Req. | Meaning |
|---|---|---|---|---|
| 1 | `id` | u32 | yes | the request's |
| 2 | state | uint | yes | `ok` 0, `err` 1, `running` 2 (long action), `cancelled` 3 |
| 3 | value | any | no | the `Ok(...)` of an action (record or `null`) |
| 4 | `status` | uint | with `err` | §10.1 |
| 5 | code | `tstr` | no | `J-E23xx` |
| 6 | progress | 0–100 | no | with `running`; at most once per second |
| 7 | results | `[* outcome-entry]` | `CALL` | the `WriteOutcome`s of the expanded writes, after the commit |
| 8 | detail | `tstr` | no | |
| 9 | repeated | bool | no, `false` | `true`: returned by deduplication, without repeating the command |
| 10 | expansion | `[* [sid, v]]` | `CALL` | the proposals into which the method expanded; H uses them to renew the lease (§8.5) |

```cbor-diag
{
  1: 9001,
  2: 0,
  7: [[20, 23, 4, 1], [21, 23, 4, 1]],
  10: [[20, 60.0], [21, true]]
}
```

```
header     17 11 2e 03 94 01 00 00 00 00 1f 00
body       a4 01 19 23 29 02 00 07 82 84 14 17 04 01 84 15 17 04 01 0a 82 82 14 f9
           53 80 82 15 f5
```

(12 B of header + 29 B of body = 41 B of message.)

```cbor-diag
{1: 9002, 2: 0, 3: null}
```

```
header     17 11 2f 03 94 01 00 00 00 00 20 00
body       a3 01 19 23 2a 02 00 03 f6
```

(12 B of header + 9 B of body = 21 B of message.)

### 6.17 `CANCEL` (`0x18`)

H → D, class F. `{1: id}` of a long action in progress. Response: `RESULT` with `cancelled` for that `id`, or `ERROR` status 6 if it has already finished or does not exist.

```cbor-diag
{1: 9003}
```

```
header     18 11 95 01 2f 03 00 00 00 00 09 00
body       a1 01 19 23 2b
```

(12 B of header + 5 B of body = 17 B of message.)

### 6.18 `EVENTS` (`0x19`)

D → H, class F. Occurrences of `event` (from classes, devices and apps). `{1: t0, 2: [* [eid, dt, data, ? flags]], ? 3: lost}`. `data` is the event record (§6.2). Bounded queue on the MCU; when full, the **new** event is discarded and counted in key 3 (the order and the first events explain a failure better than the last ones).

The `tripped { code }` event of a pump (`std/pump@1`), with `code` in field 0 of the record:

```cbor-diag
{1: 1241000, 2: [[2, 120, {0: 17}]]}
```

```
header     19 11 30 03 95 01 00 00 00 00 21 00
body       a2 01 1a 00 12 ef a8 02 81 83 02 18 78 a1 00 11
```

(12 B of header + 16 B of body = 28 B of message.)

### 6.19 `ALARMS` (`0x1A`)

D → H, class F. Alarm transitions (02 §11), in the order in which they happened. `{1: t0, 2: [* [alid, state, dt, ? flags]], ? 3: snapshot, ? 4: lost}`. State: `alarm_state` of ref/00 §6. Flags: bit 0 evaluated with `Uncertain`; bit 1 shelved; bit 2 suppressed (`suppress_when`).

- With `snapshot = true`, the list carries all alarms that are not `normal`, and the absent ones are `normal`. Sent after `READY` and on request (`READ` with key 2).
- If the queue fills up, D merges the transitions per alarm (the most recent state stays), counts the lost ones in key 4, and sends a `snapshot` as soon as it can.

```cbor-diag
{1: 1241000, 2: [[1, 1, 0]], 3: false}
```

```
header     1a 11 31 03 95 01 00 00 00 00 22 00
body       a3 01 1a 00 12 ef a8 02 81 83 01 01 00 03 f4
```

(12 B of header + 15 B of body = 27 B of message.)

### 6.20 `ALARM_CMD` (`0x1B`)

H → D, class F. `{1: id, 2: alid, 3: op, ? 4: ttl (ms), 5: role}`. `op`: `ack` 0, `shelve` 1 (with `ttl`, required, 02 §11), `unshelve` 2. The effect appears as a transition in `ALARMS`; the response is a `RESULT`.

```cbor-diag
{1: 9004, 2: 1, 3: 1, 4: 7200000, 5: 1}
```

```
header     1b 11 96 01 31 03 00 00 00 00 0a 00
body       a5 01 19 23 2c 02 01 03 01 04 1a 00 6d dd 00 05 01
```

(12 B of header + 17 B of body = 29 B of message.)

### 6.21 `RESET` (`0x1C`)

H → D, class F. Ends the trip of an MCU output (`jk reset`, 16 E7). `{1: id, 2: sid, 3: role}`. An output that is not tripped gives `err` with status 16 (`J-E2320`).

```cbor-diag
{1: 9005, 2: 18, 3: 1}
```

```
header     1c 11 97 01 31 03 00 00 00 00 0b 00
body       a3 01 19 23 2d 02 12 03 01
```

(12 B of header + 9 B of body = 21 B of message.)

### 6.22 `RESTART` (`0x1D`)

H → D, class F. `{1: id, 2: type, 3: role}`. Type `warm` 0: `jk_restart` (outputs to `safe`, leases dropped, persisted state reloaded; ref/08); `reboot` 1: MCU restart. D replies `RESULT ok`, sends `BYE(reboot)` and restarts. Role `admin`. Allowed in all modes, because it is what resolves an MCU in `maintenance`.

```cbor-diag
{1: 9006, 2: 0, 3: 2}
```

```
header     1d 11 98 01 31 03 00 00 00 00 0c 00
body       a3 01 19 23 2e 02 00 03 02
```

(12 B of header + 9 B of body = 21 B of message.)

### 6.23 `TIME` (`0x20`)

Class E. Three phases (§8.4):

| Phase (1) | Direction | Fields |
|---|---|---|
| 0, probe | H → D | 2 `t1`: H's monotonic clock, µs |
| 1, echo | D → H | 2 `t1` (echo); 3 `t2`: MCU on receiving; 4 `t3`: MCU on sending (`mono-us`) |
| 2, set | H → D | 5 `M` (`mono-us`); 6 UTC at `M` (`utc-us`); 7 quality (`clock_quality`); 8 source (`none` 0, `ntp` 1, `ptp` 2, `gnss` 3, `manual` 4); 9 rate correction (i32, ppb); 10 estimated error (µs); 11 `max_holdover` (ms); 12 holdover since (`utc-us`) |

```cbor-diag
{1: 0, 2: 81234567890}
```

```
header     20 10 98 01 31 03 00 00 00 00
body       a2 01 00 02 1b 00 00 00 12 e9 f5 22 d2
```

(10 B of header + 13 B of body = 23 B of message.)

```cbor-diag
{1: 1, 2: 81234567890, 3: 1241500, 4: 1241512}
```

```
header     20 10 32 03 98 01 00 00 00 00
body       a4 01 01 02 1b 00 00 00 12 e9 f5 22 d2 03 1a 00 12 f1 9c 04 1a 00 12 f1
           a8
```

(10 B of header + 25 B of body = 35 B of message.)

```cbor-diag
{1: 2, 5: 1241512, 6: 1790244000123456, 7: 0, 8: 1, 9: -1200, 10: 180}
```

```
header     20 10 99 01 32 03 00 00 00 00
body       a7 01 02 05 1a 00 12 f1 a8 06 1b 00 06 5c 37 a9 b0 4a 40 07 00 08 01 09
           39 04 af 0a 18 b4
```

(10 B of header + 30 B of body = 40 B of message.)

### 6.24 `TRACE` (`0x21`)

The MCU's trace ring (19 §3, U9). The content is the `.jktrace` v1 stream (16 §7) that `jk_trace_read` produces (ref/08 §2.6.o): times in ns, one segment per group, bit 0 of the header `flags` at 1 (segments without compression). The `offset` and `len` of `TRACE` are those of `jk_trace_read`. `jokerd` or `jk` compress it on receiving it.

| Phase (1) | Direction | Class | Fields |
|---|---|---|---|
| 0, list | H → D | F | — |
| 1, info | D → H | F | 2 `[* [seg, reason, bytes, frozen_at]]`; reason: `continuous` 0, `trip` 1, `manual` 2, `crash` 3 |
| 2, read | H → D | F | 3 `seg`; 4 `offset`; 5 `len` (≤ `bulk_max`) |
| 3, data | D → H | E | 3 `seg`; 4 `offset`; 6 bytes; 7 last |
| 4, release | H → D | F | 3 `seg`: D may record over it again |
| 5, freeze | H → D | F | freezes the continuous ring now (`jk_trace_freeze`); D replies with `info` |

- A trip freezes the ring by itself (19 U9) and D sends `info` unrequested.
- Data is only read from frozen segments; reading the continuous ring without freezing it gives `ERROR` status 26 (`J-E2420`).
- H requests the ranges in order and repeats a `read` without a response after `RTO`.

```cbor-diag
{1: 1, 2: [[0, 1, 16384, 1240000]]}
```

```
header     21 11 33 03 99 01 00 00 00 00 23 00
body       a2 01 01 02 81 84 00 01 19 40 00 1a 00 12 eb c0
```

(12 B of header + 16 B of body = 28 B of message.)

```cbor-diag
{1: 2, 3: 0, 4: 0, 5: 512}
```

```
header     21 11 9a 01 33 03 00 00 00 00 0c 00
body       a4 01 02 03 00 04 00 05 19 02 00
```

(12 B of header + 11 B of body = 23 B of message.)

```cbor-diag
{1: 3, 3: 0, 4: 0, 6: h'4a4b545201002e2e2e', 7: false}
```

```
header     21 10 34 03 9a 01 00 00 00 00
body       a5 01 03 03 00 04 00 06 49 4a 4b 54 52 01 00 2e 2e 2e 07 f4
```

(10 B of header + 20 B of body = 30 B of message.)

### 6.25 `OTA` (`0x22`)

Signed firmware or generation blob. The phases, fields and behavior are in §9; so are the examples.

### 6.26 `ERROR` (`0x07`)

↔, class E. `{1: status, ? 2: code, ? 3: type, ? 4: seq, ? 5: rseq, ? 6: detail (≤ 128 B)}`. Tells the other side that a message was refused. An `ERROR` is never answered with another. At most 10 per second per session. L1 and L2 errors (CRC, tag, replay) do **not** generate `ERROR`: only counters, so as not to give information to an attacker or amplify noise.

```cbor-diag
{1: 6, 2: "J-E2310", 3: 22, 4: 404, 5: 8}
```

```
header     07 10 0e 00 28 00 ff ff ff ff
body       a5 01 06 02 67 4a 2d 45 32 33 31 30 03 16 04 19 01 94 05 08
```

(10 B of header + 20 B of body = 30 B of message.)

---

## 7. Session state machine

### 7.1 On the device

```
                  MCU boot
                        │
                        ▼
                  ┌───────────┐ ◀──────────────────────────────────────────────────┐
                  │   IDLE    │ ◀───────────────────────────────────┐              │
                  └─────┬─────┘                                     │              │
                        │ new nonce_d; sends HELLO(offer)           │              │
                        ▼                                           │              │
                  ┌───────────┐  no reply within 500 ms:            │              │
          ┌──────▶│  OFFERED  │──resends, backoff 100 ms → 5 s, ±20%│              │
          │       └─────┬─────┘  (new nonce_d on each attempt)      │              │
          │             │ HELLO(answer) with the right echo         │              │
          │             ├── mode refused ──▶ waits 5 s ─────────────┘              │
          │             │ derives k_d2h, k_h2d; sends READY (F)                    │
          │             ▼                                                          │
          │       ┌───────────┐  READY unacknowledged after retransmit ────────────┤
          │       │CONFIRMING │                                                    │
          │       └─────┬─────┘                                                    │
          │             │ ACK of the READY                                         │
          │             ▼                                                          │
          │       ┌───────────┐  link_timeout with no valid packet ────────────────┤
          │       │    UP     │  BYE received ─────────────────────────────────────┤
          │       │ (mode m)  │  retransmit exhausted on an F message ─────────────┤
          │       └─────┬─────┘  valid HELLO(request) and 2×keepalive of silence ──┘
          │             │ new generation active, or KEYROT applied
          └──BYE(generation_change / rehandshake)
```

- In all states, the **remote leases keep running** in the MCU's arbiter (§7.4). The session and the leases are independent.
- On leaving `UP`, D erases the session keys, the windows and the reassembly, and abandons the F messages still to be sent, except action `RESULT`s and `ALARMS`, which are kept for the next session if the MCU does not restart (the transitions are also recovered by the `snapshot`).

### 7.2 On the host

| H state | Link state (`link_state`, ref/00 §6) | Quality of the MCU's signals on the Linux node | Writes |
|---|---|---|---|
| `IDLE`, waiting for an offer | `connecting` | `Uncertain(Initial)` until the first sample; after a loss, `Bad(CommFailure)` | no |
| `HANDSHAKE` (replied, waiting for `READY`) | `connecting` | as above | no |
| `UP` · `full` | `up` | the one that comes in `SAMPLES` | yes |
| `UP` · `full` with losses ≥ 20% in 10 s | `degraded` | the one that comes; `Uncertain(Stale)` through `max_age` | yes; alarm `joker.device.<d>.degraded` |
| `UP` · `observe` | `drift` | the one that comes | **no** in `prod` (proposals do not go out; local `write`s give `J-E2319`); in `dev`, yes, with a warning |
| `UP` · `maintenance` | `down` | `bad(config_error)` | no |
| answered `refused` (§7.3, rules 2 and 3) | `down` | `bad(config_error)` | no |
| `LOST` (`link_timeout`, `BYE`, `retransmit` exhausted) | `down` | `Bad(CommFailure)`; alarm `joker.device.<d>.offline` | no |

In short, from the session mode to the link state: `full` → `up` (or `degraded`); `observe` → `drift`; `maintenance` and `refused` → `down`, with the samples in `bad(config_error)` (document 05, §12). H sends `HELLO(request)` at startup and every 1 s while there is no session, and when it receives a session frame with a `sid` it does not know. The sixth value of `link_state`, `stopped`, belongs to the core: the `joker-link` driver is not running (generation being swapped, device disabled). The session mode is the `session_mode` of ref/00 §6, also in the MCU's `jk_link_status` (ref/08 §2.6.p).

### 7.3 Checking at `HELLO` and degradation

H decides the mode in the following order; the first rule that applies decides.

| # | Condition | Mode | Code | What becomes visible |
|---|---|---|---|---|
| 1 | handshake frame with an unknown `kid`, or a wrong tag | — (no reply) | `J-E2103`, `J-E2101` | counter; alarm `joker.device.<d>.auth` after 3 |
| 2 | no common major (§13) | `refused`, status 3 | `J-E2201`, `J0461` (ref/02) | the node does not use the link |
| 3 | the identity (key 4.1) is not the one expected for this device | `refused`, status 27 | `J-E2207` | alarm |
| 4 | D has no valid generation (no key 6, or state `failed`) | `maintenance`, status 28 | `J-E2206` | only `OTA`, `TRACE`, `TIME`, `RESTART`, `CATALOG`, `KEYROT` |
| 5 | the catalog of this hash has already failed verification (§6.11) | `maintenance`, status 31 | `J-E2203` | likewise |
| 6 | firmware (image or descriptor digest), Joker Micro version, features or drivers (key 15) different from `[mcu.<d>]` in the lock; hardware id different from `[discovered.<d>] serial`; generation hash different from what `jk build` computed for the node's active generation; catalog hash different from `[discovered.<d>] digest` | `observe` in `prod`, `full` in `dev` | `J-E2202`; `J0460` or `J0401` with `kind` (ref/02 §3.1.5) | alarm `joker.device.<d>.drift` |
| 7 | none of the above | `full` | — | — |

- **Locally changed parameters** (key 16) do not change the mode: they are drift, which appears in the report to the Deck (ref/10 §2.10.1, `mcus`) and in `GET /v1/micros/{device}` (ref/09 §5.18).
- The check of **classes** (U7) is per instance, and does not change the mode: an instance whose class has the same major as the one the Linux node uses is accepted, even with a different hash; a different major does not create the implicit instance (`J-E2209` on the link, `J0630` on the app side) and the alarm `joker.device.<d>.drift` fires.
- **A session in `observe` moves to `full`** with a new generation of the Linux node that updates the lock. H then sends `BYE(rehandshake)`; since in `observe` there are no remote proposals, the new handshake does not touch the outputs.
- In `maintenance`, D does not send `SAMPLES`, `OUTCOMES`, `EVENTS` or `ALARMS`.

### 7.4 Link loss, lease and safe state

The rule of document 19, §6: **the MCU is the one in charge of the MCU's outputs**. A remote writer is a level with a lease, like any other (16 A2). When the link drops, nobody "turns off" anything: H stops renewing, the remote leases expire **on the MCU**, and the MCU's arbiter moves to the next active level or, with none, to `safe` (A6).

```
  t (ms)   0         10        20        30        40
  H        PROPOSE   PROPOSE   ✗ jokerd dies
  D        consumes  consumes  ·         ·         ·
           lease_until = t_tick + 30 ms  ──────────────▶ 40: lease expires at the latch
  output   value of level 10 ─────────────────────────────▶ next level, or safe
  link     UP ───────────────────────────── (200 ms) ──▶ LOST (does not interfere with the output)
```

- The session does not enter the decision: `link_timeout` only serves for D to restart the handshake and for H to show `Down`. There is no "drop everything when the link drops", which would be a second rule and a second clock to test.
- The worst case is `max_lease` (default 5 s), which is also the ceiling of remote manual writes: the 8 h TTL of a `jk write` stays on the Linux node, which renews it in pieces of `max_lease` (§8.1).

### 7.5 Restarts and reconnection

| Case | How it is detected | What happens |
|---|---|---|
| `jokerd` restarts | D stops receiving frames; the new H sends `HELLO(request)` | remote leases expire (§7.4); D obeys the request after 2 × `keepalive` of silence; new session; H proposes again; the pending F requests were lost on the H side, and the tickets resolve with `J-E2318` |
| the MCU restarts | `boot_count` (key 12.1) changed | the MCU boots with the outputs in `safe` (A8) and without remote levels (it does not persist them; §8.1, A14); H proposes again, including the manual levels still within their TTL; H discards the pending tickets with `J-E2318` and does **not** repeat actions (deduplication was lost with the RAM) |
| the link drops and comes back, without restarts | same `boot_count` | H repeats the `ACTION`s and the keyed `CALL`s whose `RESULT` did not arrive, with the same `id` and the same key: D's deduplication returns the result without repeating the command |
| new generation on the MCU (OTA or local swap) | `BYE(generation_change)` and a new `HELLO` | with a hot swap (`jk_generation_swap`), identical outputs **keep** the applied value, the remote leases and the trips (A8); H redoes the profile if the catalog hash changed |
| new generation on the Linux node | — | the `origin`s may change number; since remote ownership is per level (§8.1), the new proposal occupies the same level without conflict |

---

## 8. Semantics bound to the core

### 8.1 `PROPOSE` and the arbiter (16 A1–A14)

| Rule | In Joker Link |
|---|---|
| **A1** writer = (origin, level) | Each `PROPOSE` block is a remote writer: (`origin`, `level`). On the MCU, each pair (link, delegated level) is **one writer** of the blob's `WRITERS` table (ref/08 §2.6.p, 16 A12), and **ownership of a remote level belongs to the level**, not to the `origin`: a remote proposal at a delegated level occupies it, whatever the `origin`. The Linux node guarantees that there are never two of its writers at the same level of the same output (`J0210` in its `jk check`). That way a new generation of the node, which renumbers the `origin`s, does not cause `NotOwner` during a lease |
| delegated levels | Each MCU output delegates to the link a set of levels (catalog, key 1.8). By default: all those that no local writer of the MCU uses. A proposal at a non-delegated level is `Rejected(NotOwner)` and renews nothing (A3); in the Linux node's `jk check` it is `J0731` |
| **A2** lease | `lease_until = t_tick + lease`, with `t_tick` the **MCU's** tick that consumes the proposal. Link latency does not eat the lease: it counts from consumption. **Default:** a Linux writer (cycle app, `calc`, instance) that does not declare `lease` writes to an MCU output with `lease = min(3 × period of the writer's Linux group, max_lease of the link)`; an async writer without a group uses the link's `max_lease`. `J0730` (§11.6) checks the default as a declared value |
| **A3** renewal | Any proposal accepted by the MCU's arbiter renews, whatever the result of the envelope; `NotOwner` and `WrongType` do not renew |
| **A4** order | Remote proposals enter at step 2 of the owning group's tick (01 §4), like async ones, in order of reception (blocks and entries in message order). Within the same writer, the last one wins |
| **A5** no ties | guaranteed by the two `jk check`s (the node's and the MCU's) and by per-level ownership |
| **A6**, **A7**, **A8** | the MCU's, unchanged. `relinquish` (`v = null`) releases the level right at consumption |
| **A9** `max_lease` | The effective `lease` is `min(lease, max_lease of the link)` (16 A12). The node's `jk check` rejects a link `max_lease` above that of an output (`J0733`), and so the output's is never the smaller one; the declared `lease` of a remote writer is checked against the **link's** `max_lease` (`J0213`). If a larger value still arrives, the MCU limits it and counts it (`J-E2303`, warning) |
| **A10** manual | `--ttl` stays on the Linux node. H sends the manual write with `lease = min(remaining ttl, max_lease of the link)` and renews it until the end of the TTL. A dead `jokerd` stops renewing, and the override expires on the MCU in ≤ `max_lease` |
| **A14** persistence | The Linux node's. The MCU **does not persist** remote levels, manual or not: after an MCU reset (`boot_count` changed, §7.5), the level disappears and the output follows the MCU's arbiter until H renews it, at the next `renew`. A `jokerd` that restarts restores the manual levels from its record (16 A14) and sends them again |
| **A11** owning group | the MCU's; the Linux node does not need to know it |

**Sending by H.** Per remote writer and output, a *latest-wins* box (01 §3.4). H sends when the value changes and at least every `renew = max(1 ms, lease / 3)`. A `relinquish` goes in all subsequent `PROPOSE`s of that writer until an `OUTCOMES` arrives with result `none` for that output and level, or until the lease passes.

**Methods.** Calls from cycle tasks go in `PROPOSE` (position 5) and are expanded at step 5a of the MCU's tick, in order of arrival, at the writer's level, with `own()` = the last proposal of that level (17 §4.1). They are repeated at each renewal, as on Linux. Calls from outside the cycle use `CALL` (§8.5).

**The Linux node neither arbitrates nor applies an envelope** to the MCU's outputs: declaring them in the node's `[outputs]` is `J0234` (ref/01). `jk write` shows the envelope from fields 1.9 and 1.10 of the catalog, for information only.

### 8.2 `OUTCOMES` and the results (ref/00 §4.4)

- D emits an `OUTCOMES` at the end of each tick that consumed remote proposals **if** some result changed; otherwise, at most one every `report`, empty, just to advance `ref`.
- A result "changed" when the `origin`, the code, the applied value, the `detail`, or the proposed value changed, or when it is the first one for that (output, level) in the session.
- It is reliable and ordered, because each entry is a **decision** that the Linux node's decision log records (invariant 8 of AGENTS.md). With the window full, pending results are merged per (output, level), the most recent stays, and the lost ones are counted in key 5 (invariant 7: telemetry never holds back control).
- **On the Linux node:** an async ticket for a proposal that went in the `PROPOSE` with `seq = S` resolves at the first `OUTCOMES` with `ref ≥ S`: with the entry, if it is there; otherwise, with the last known result for that (output, level). Until then, it is `Deferred` (6). The `last_outcome()` of a cycle task is the last result received, which may be one or two ticks behind compared with pure Linux (16 A12: the lease and the result count at the consumer, the MCU).
- D never sends `deferred` (6) or `shadowed` (7): they are states of the Linux node. If received, they count as unknown.
- A ticket without `OUTCOMES` after 3 × `RTO` + the period of the owning group resolves with `J-E2304` ("unknown result"), which is not `accepted`.
- A remote lease that expires (key 4) gives the warning `J-E2305` and the counter `joker.device.<d>.lease_expired`. On a healthy link it should never happen: it indicates lack of bandwidth or of renewal (§11.6).

### 8.3 Samples: quality and time in `jokerd`

The `joker-link` driver writes each `SAMPLES` entry to the slot of the corresponding signal of the Linux node (`<device>.<path on the MCU>`), through the normal acquisition stage (01 §3.3):

| Slot field (01 §3.2) | Where it comes from |
|---|---|
| `value` | `v`, converted to the `u64` bits of ref/00 §4.1, in the catalog's unit |
| `quality` | `q` as is (code, reason and flags) |
| `ts_acq` | monotonic clock of the Linux node at **reception** of the frame (ns) |
| `ts_hw` | `t0 + dt` converted to the node's monotonic clock: `ns = 1000 · (t0 + dt − θ)`, with θ and the rate estimated by `TIME` (§8.4); only when the estimate exists; "link" domain flag in the slot |
| `ts_src` | `ts_src` × 1000, if it exists |
| `seq` | that of the Linux node's slot: each entry is a new sample (also the `refresh` ones). The MCU's 16-bit `seq` serves to discard old entries (§5.4) and to count samples that the MCU produced and did not send |

- The MCU's `joker.X` signals appear on the node as `joker.device.<d>.X`.
- An instance `i` of the MCU appears as an implicit instance of the node with the name `i`, or with the name given in `instances` on the device (§15). A collision with a name of the node is `J0604`. A path that exceeds 8 segments with the device prefix is `J0740`.
- A composite instance of the MCU reaches the node with the **same tree** and the **same nested paths** (document 16, P6), starting at the top-level name: with `instances = { hp = "heat_pump" }`, `hp.compressor.drive.speed` on the MCU is `heat_pump.compressor.drive.speed` on the node. Instances with 2.9 = `true` get no top-level name on the node, and only exist through the paths of the compositions they belong to. The renaming of `instances` applies only to top-level names; an entry for an instance that is only a member is `J0650`.
- The MCU's monotonic instants only make sense with the `TIME` pair of that session, and that pair lives on the Linux node (ref/00 §3). The MCU never resends a `mono-us` from a previous session, because its clock restarts at every boot and H redoes θ in every session.

### 8.4 `TIME` and `Holdover`

**On H**, every `time_interval` (and 4 times, 100 ms apart, after `READY`):

1. sends the probe with `t1` (its monotonic clock, µs) and records `t4` on receiving the echo;
2. computes the delay `δ = (t4 − t1) − (t3 − t2)` and the offset `θ = ((t2 − t1) + (t3 − t4)) / 2` (MCU − H);
3. rejects samples with `δ` above `time_max_delay` (default 5 ms on serial, USB and CAN; 50 ms on IP), with the counter `J-E2430`; among the accepted ones, uses the one with the smallest `δ` of the last 8 and adjusts the rate by regression;
4. sends the set: `M` = a recent MCU instant, the corresponding UTC (by its own wall clock), the quality, the source, the rate correction, the estimated error (`δ/2` plus H's own uncertainty) and the `max_holdover` of its `[clock]`.

The quality sent is `synced` only if H's own clock is `synced` **and** the estimated error is ≤ `time_tolerance` (default 1 ms); otherwise, `holdover` (with H's "since") or `unsynced`.

**On D** (`jk_time_set_utc`, ref/08): `utc(now) = utc_M + (now − M) · (1 + rate · 10⁻⁹)`.

| Situation on the MCU | Clock quality |
|---|---|
| before the first set since boot | `unsynced` (no UTC) |
| last `synced` set less than 3 × `time_interval` ago | `synced` |
| last `synced` set longer ago, or `holdover` set | `holdover(since the last synced)` |
| in `holdover` for longer than `max_holdover` (default 24 h), or `unsynced` set | `unsynced` |

An MCU app with `requires_clock = "synced"` sees its outputs blocked (`Rejected(ClockUnsynced)`, step 0 of the envelope; 01 §7 and §18, 16 E1) outside `synced`. Without `TIME` (`time_interval = 0`) and with such an app, it is `J0741`.

A jump of H's wall clock (NTP correcting hours after a boot without an RTC) changes only the UTC pair: the MCU's monotonic clock and the θ conversion do not change, and the samples do not get shuffled (01 §18).

### 8.5 Idempotency: `CALL` and `ACTION`

| | `CALL` (method) | `ACTION` (action) |
|---|---|---|
| repeating | a state method is idempotent; a `toggle()` is not | dangerous (17 §2.1) |
| within the session | exactly once, by `rseq` | exactly once, by `rseq` |
| across sessions, without an MCU restart | only with a key: D deduplicates | always with a key: D deduplicates |
| after an MCU restart | not repeated: `J-E2318` | not repeated: `J-E2318` |

**Deduplication table on the MCU:** fixed capacity (key 11.5 of `HELLO`, ≥ 8). Each entry keeps the key, the `id`, the state and the result if it has ≤ 64 B, for `dedup_window` (16 E9; default 60 s, from the MCU's generation). With the same key:

- still in progress → `RESULT running`, `repeated = true`;
- finished → the stored `RESULT`, `repeated = true` (without a value if it had more than 64 B);
- table full of entries within the window → the new one is refused with `err` and status 8 (`J-E2315`), and a live entry is **not** evicted: it is preferable to fail than to execute twice.

**`ACTION` on the MCU**, in the order of 17 §4.2: `aid` (`J-E2310`) → role (`J-E2313`) → deduplication → permissive (`J-E2312`) → serialized queue of the device (full: `J-E2315`) → execution → timeout (`J-E2314`) → `RESULT`.

**`CALL` on the MCU:** `mid` → delegated levels of all the method's `out`s (otherwise `err`, status 30, `J-E2303`) → deduplication, if there is a key → expansion at step 5a of the next tick of the owning group → `RESULT` after the commit, with the `WriteOutcome`s (key 7) and the expansion (key 10). **Renewal:** H renews the expanded writes with `PROPOSE` of the pairs of key 10, at the same (`origin`, `level`), until the end of the requested lease. That way a method is not executed again just to keep the lease, and a `toggle()` is not undone.

### 8.6 Alarms, events and trips

- The alarm lifecycle (02 §11) runs on the MCU. H receives the transitions (`ALARMS`) and reflects them in the device's `alarm.<name>.state` and `.active` signals and in its alarm list; `jk alarm ack` and `jk alarm shelve` of an MCU alarm become `ALARM_CMD`.
- Trips of MCU outputs (01 §7) live on the MCU and persist there (`JK_AREA_PERSIST`, ref/08). H sees them as quality `Bad(Tripped)` in the output's sample; `jk reset` becomes `RESET`.
- Events of the MCU's instances arrive by `EVENTS` and enter the node like the events of any instance.

### 8.7 `TRACE` and the format of 16 §7

What is transferred is the MCU's own `.jktrace`: header (`"JKTR"`, version 1, `flags` with bit 0 at 1, hashes of the MCU's generation and registry, the MCU's monotonic and UTC epochs, clock quality) and segments without compression. The instants are in ns (the MCU multiplies its µs by 1000) and on the MCU's clock; `jk replay` runs the same core and reproduces the MCU's decisions (19 U9). Remote proposals appear in the trace with writers `0x8000_0000 | slot`, and the Linux node's decision log keeps the mapping slot → (`origin`, level) of each session.

---

## 9. OTA

### 9.1 The artifact

An OTA artifact is a pair: the **signed manifest** (COSE_Sign1, RFC 9052, EdDSA with Ed25519, ref/00 §7) and the **image** (the firmware bytes, or the image `[0, image_len)` of the generation blob of ref/08). The manifest is small and goes first, so that D can refuse a wrong artifact **before** spending flash.

**A single form of signature.** In a generation, this COSE_Sign1 **is** the *trailer* of the `.jkgen` blob (ref/08 §2.13.5): the file that `jk build --generation` writes is the image followed by the manifest, and D writes the manifest received in `begin` right after the image, at `trailer_off = image_len`. The firmware carries the same manifest beside the image (`FW_STAGING`). The same verification serves OTA over the link, local OTA (`jk_ota_*`), boot and the micro profile (§14).

`ota-manifest` (payload of the COSE_Sign1):

| Key | Field | Type | Req. | Meaning |
|---|---|---|---|---|
| 1 | type | uint | yes | `ota_kind` (ref/00 §6): `generation` 1, `firmware` 2; `model` 3 and `app` 4 **reserved 1.4**; 0 never |
| 2 | target | `tstr` | yes | firmware family or board; it must be the `firmware.3` of `HELLO` |
| 3 | version | `tstr` | yes | SemVer (firmware) or the `gen_seq` in decimal (generation) |
| 4 | size | u32 | yes | bytes of the image (in a generation, `image_len`) |
| 5 | digest | `bstr` 32 | yes | SHA-256 of the image |
| 6 | `security_version` | u32 | yes | security counter (anti-rollback) of the firmware; in the generation, 0 |
| 7 | `gen_seq` | u64 | generation | anti-rollback of the generation (`min_seq`, ref/08); equal to the one in the blob header |
| 8 | generation hash | `bstr` 32 | generation | the blob's `source_hash`; it must be what `HELLO` shows after the swap |
| 9 | capabilities | `bstr` 32 | generation | hash of the capabilities of the firmware for which the generation was made (ref/08 §2.14); it must be the MCU's `firmware.5` (otherwise `J-E2410`) |
| 10 | created at | `utc-us` | no | |
| 11 | minimum link | `[maj, min]` | no | minimum Joker Link version that the firmware speaks |
| 12 | `board_id` | `bstr` 8 | generation | the one in the blob header (ref/08 §2.13.2) |
| 13 | format | uint | generation | the blob's `format` |
| 14 | node | `tstr` | no | name of the node the blob is bound to (`NODE_BOUND`); compared with record 16 of the `IDENTITY` area (`JK_E_GEN_NODE`) |
| 15 | registry hash | `bstr` 32 | no | the blob's `registry_hash` |

COSE protected header: `{1: -8 (EdDSA), 4: kid}`, with `kid` = the first **16** bytes of the SHA-256 of the public key (the key id of ref/02 §2.5 and ref/10 §2.1). D verifies with the keys of its trust bundle (ref/08 §2.13.5), by the roles of ref/00 §7.1: `firmware` for the firmware, `config` for the generation.

**Test vector** (Ed25519 private key with seed `60 61 … 7f`; 8 KiB image of zeros; generation 8):

```
public key     174553b456dddfc6908ecab1c101fe6ab21e2baa0617795b7d43a63482993fd5
kid            68894d58f18f2c34d49eb2f4b110e042
board_id       8bb5bccf6eedea71                                 (SHA-256("acme/io16-rp2350@1")[0..8])
manifest       {1: 1, 2: "acme/io16-rp2350", 3: "8", 4: 8192,
                5: h'9f1dcbc35c350d6027f98be0f5c8b43b42ca52b7604459c0c42be3aa88913d47',
                6: 3, 7: 8,
                8: h'85378bbfa265d5109625f8c9e2e5e280e8e8e2af3497a4dfc8d74159b357553f',
                9: h'd1d9be4b6c268eef97fb159f5743637124413bd9a93a991d3ec8225bd33faae8',
                11: [1, 0], 12: h'8bb5bccf6eedea71', 13: 1}                  (153 B)
protected      a2 01 27 04 50 68 89 4d 58 f1 8f 2c 34 d4 9e b2 f4 b1 10 e0 42
Sig_structure  ["Signature1", protected, h'', manifest]       SHA-256 796d7461…0e59a0ef1
signature      9bd8741baa97ce1667cfb12570f4cd84af9377d6d2cd5a7449a8a1d1e96ea616
               aec54e5722cf1a462703d9ebf8125162fd9041c8eca71e98517d4c9c155f960c
COSE_Sign1     d2 84 55 a2 01 27 04 50 68 89 4d 58 f1 8f 2c 34 d4 9e b2 f4 b1 10 e0 42 a0
               58 99 ac 01 01 02 70 … 0c 48 8b b5 bc cf 6e ed ea 71 0d 01
               58 40 9b d8 74 … 9c 15 5f 96 0c                                   (246 B)
```

### 9.2 Phases of the `OTA` message

| Phase (1) | Direction | Class | Fields | Meaning |
|---|---|---|---|---|
| 0 `begin` | H → D | F | 2 `xfer` (u16); 3 type; 4 size; 5 COSE_Sign1 (`bstr`); 6 proposed chunk | starts or resumes |
| 1 `ready` | D → H | F | 2 `xfer`; 6 accepted chunk; 7 resume `offset`; 8 window (chunks) | or `abort` |
| 2 `data` | H → D | E | 2 `xfer`; 7 `offset`; 9 bytes | one chunk |
| 3 `ack` | D → H | E | 2 `xfer`; 7 `offset` | contiguous bytes already written to flash |
| 4 `end` | H → D | F | 2 `xfer` | all bytes sent |
| 5 `verified` | D → H | F | 2 `xfer`; 10 status | 0 = image complete and verified |
| 6 `activate` | H → D | F | 2 `xfer`; 11 confirmation window (ms) | swap |
| 7 `confirm` | H → D | F | 3 type | confirms the trial in progress |
| 8 `abort` | ↔ | F | 2 `xfer`; 10 status; 12 code | gives up |
| 9 `state` | D → H | F | 3 type; 13 `generation_state` | response to `confirm`; also unrequested, when the window expires |

### 9.3 Transfer and resume

1. **`begin`.** D checks, before erasing any sector: the signature and the trust of the key (`J-E2401`, `J-E2402`); the type and the target (`J-E2405`); the size against the area (`JK_AREA_GEN_A/B` or `JK_AREA_FW_STAGING`; `J-E2406`); anti-rollback: `security_version` ≥ the stored floor, or `gen` > the current `gen` (`J-E2404`); in the generation, the capabilities (`J-E2410`). Only then does it reply `ready`.
2. **Chunk.** Power of 2 between 64 B and `bulk_max`; D may lower the proposed one. `offset` is always a multiple of the chunk; only the last one is shorter.
3. **Window.** H sends up to `window` chunks ahead of the last `ack`. D writes only contiguous chunks: an out-of-order chunk is discarded, and D immediately replies with an `ack` of its current `offset` (fast retransmission). With no progress in the `ack` for `RTO`, H restarts at the last acknowledged `offset` (*go-back-N*).
4. **Resume.** D keeps in `JK_AREA_PERSIST` the pair (digest, acknowledged `offset`), rounded down to the last complete sector. A `begin` with the same manifest, after a link drop or an MCU restart, receives `ready` with that `offset`. A `begin` with another manifest discards the previous transfer.
5. **`end`.** D confirms the size and computes the SHA-256 of the whole area: different from the manifest is `abort` with `J-E2403`. Equal, `verified` with status 0. It stores the manifest received in `begin` right after the image (in a generation, as the blob's *trailer*, §9.1), to verify it again at every boot (ref/08 §2.12.3).
6. An `xfer` without traffic for 60 s expires on D (`J-E2408` on the next `data`).

```cbor-diag
{1: 0, 2: 1, 3: 1, 4: 8192, 5: h'3c434f53455f5369676e313e', 6: 512}
```

```
header     22 11 9b 01 34 03 00 00 00 00 0d 00
body       a6 01 00 02 01 03 01 04 19 20 00 05 4c 3c 43 4f 53 45 5f 53 69 67 6e 31
           3e 06 19 02 00
```

(12 B of header + 29 B of body = 41 B of message.)

(Key 5 carries the whole COSE_Sign1 of §9.1; abbreviated here.)

```cbor-diag
{1: 1, 2: 1, 6: 512, 7: 0, 8: 4}
```

```
header     22 11 35 03 9b 01 00 00 00 00 24 00
body       a5 01 01 02 01 06 19 02 00 07 00 08 04
```

(12 B of header + 13 B of body = 25 B of message.)

```cbor-diag
{1: 2, 2: 1, 7: 0, 9: h'000000002e2e2e'}
```

```
header     22 10 9c 01 35 03 00 00 00 00
body       a4 01 02 02 01 07 00 09 47 00 00 00 00 2e 2e 2e
```

(10 B of header + 16 B of body = 26 B of message.)

```cbor-diag
{1: 3, 2: 1, 7: 2048}
```

```
header     22 10 36 03 9f 01 00 00 00 00
body       a3 01 03 02 01 07 19 08 00
```

(10 B of header + 9 B of body = 19 B of message.)

```cbor-diag
{1: 4, 2: 1}
```

```
header     22 11 ac 01 3e 03 00 00 00 00 0e 00
body       a2 01 04 02 01
```

(12 B of header + 5 B of body = 17 B of message.)

```cbor-diag
{1: 5, 2: 1, 10: 0}
```

```
header     22 11 3f 03 ac 01 00 00 00 00 25 00
body       a3 01 05 02 01 0a 00
```

(12 B of header + 7 B of body = 19 B of message.)

### 9.4 Activation, A/B, confirmation and rollback

H chooses the moment (the order and the window come from the `mcu-target` of ref/10 §2.6.6, or from `jk`). Firmware activation restarts the MCU and, during the restart, the outputs are in the MCU's `safe` or in the hardware's: the `jk apply` plan shows it.

| | Generation | Firmware |
|---|---|---|
| where it goes | the other generation area (`GEN_A` ↔ `GEN_B`) | `FW_STAGING`, for the bootloader (MCUboot or embassy-boot) |
| `activate` | swap at a tick boundary (`jk_generation_swap`), without restart; identical outputs keep value, leases and trips (A8); `BYE(generation_change)`; new `HELLO` with the generation on trial (12.7) | `BYE(ota_activate)`; restart; the bootloader swaps the slots in test mode; the new firmware boots on trial (12.4) |
| confirming | H observes health during the window (N clean ticks, link `full`, no new trips; 01 §16) and sends `confirm`; D marks the generation as confirmed and replies `state` `confirmed` | likewise; D marks the slot as good (`mark booted`) |
| no `confirm` within the window | D goes back to the previous generation, by itself, at a tick boundary (the window counts on the MCU, `jk_service`); `state` `rolled_back`; `J-E2409` | D restarts and the bootloader restores the previous slot; the next `HELLO` shows `rolled_back` (12.5) |
| watchdog or failure during the trial | previous generation | previous slot |

The window counts **on the MCU**, because the Linux node may have been the cause of the problem (ADR 0015 applied to the MCU). Default: 5 min (ref/10 §2.6.6).

**Who sends `confirm`.** If the MCU's OTA is part of an activation of the Linux node with a confirmation window (`jk apply --confirm`), H does **not** confirm by health: it waits for the `confirm` of the node's generation (`jk confirm`, ref/09 §5.13), which confirms both, and only then sends `confirm` to the MCU (if the MCU's health also passed; otherwise, the response to `jk confirm` says why). The MCU's window is then the node's, plus 30 s. Without a window on the node, and in OTAs requested by the Deck (ref/10 §2.6.6), H confirms by health, as in the table.

```cbor-diag
{1: 6, 2: 1, 11: 60000}
```

```
header     22 11 ad 01 3f 03 00 00 00 00 0f 00
body       a3 01 06 02 01 0b 19 ea 60
```

(12 B of header + 9 B of body = 21 B of message.)

```cbor-diag
{1: 7, 3: 1}
```

```
header     22 11 0c 00 14 00 00 00 00 00 02 00
body       a2 01 07 03 01
```

(12 B of header + 5 B of body = 17 B of message.)

```cbor-diag
{1: 9, 3: 1, 13: 2}
```

```
header     22 11 15 00 0c 00 00 00 00 00 03 00
body       a3 01 09 03 01 0d 02
```

(12 B of header + 7 B of body = 19 B of message.)

```cbor-diag
{1: 8, 2: 1, 10: 19, 12: "J-E2403"}
```

```
header     22 11 40 03 ad 01 00 00 00 00 26 00
body       a4 01 08 02 01 0a 13 0c 67 4a 2d 45 32 34 30 33
```

(12 B of header + 16 B of body = 28 B of message.)

### 9.5 Anti-rollback

- **Firmware:** D keeps the largest confirmed `security_version` (the security counter; `min_counter` of the floors of ref/10 §2.4.4) and refuses a manifest below it (`J-E2404`). Going down in version (SemVer) with the same `security_version` is allowed by D; the authorization belongs to the node and the Deck (`allow_downgrade`, ref/10). `HELLO` shows the confirmed value (key 5.10).
- **Generation:** `gen` must be greater than that of the confirmed generation (`min_seq`, ref/08). An **automatic** rollback (without `confirm`) does not go through here: it goes back to the previous area, which was already confirmed.

---

## 10. Errors

### 10.1 The `status` of messages

A small number in `ERROR` (key 1), `RESULT` (key 4), answer `HELLO` (key 8) and `OTA` (key 10), with the registry code beside it when it helps (ref/00 §8). New values are only added at the end.

| `status` | Name | Code |
|---|---|---|
| 0 | `ok` | — |
| 1 | `malformed` | `J-E2006` |
| 2 | `unknown_type` | `J-E2007` |
| 3 | `version` | `J-E2201` |
| 4 | `no_session` | `J-E2204` |
| 5 | `not_allowed` (mode or feature) | `J-E2208` |
| 6 | `unknown_id` | `J-E2310` |
| 7 | `bad_args` | `J-E2311` |
| 8 | `busy` | `J-E2315` |
| 9 | `timeout` | `J-E2314` |
| 10 | `too_large` | `J-E2008` |
| 11 | `permit` | `J-E2312` |
| 12 | `role` | `J-E2313` |
| 13 | `not_output` | `J-E2302` |
| 14 | `idem_key` | `J-E2317` |
| 15 | `cancelled` | `J-E2316` |
| 16 | `not_tripped` | `J-E2320` |
| 17 | `signature` | `J-E2401` |
| 18 | `untrusted_key` | `J-E2402` |
| 19 | `digest` | `J-E2403` |
| 20 | `downgrade` | `J-E2404` |
| 21 | `target` | `J-E2405` |
| 22 | `no_space` | `J-E2406` |
| 23 | `flash` | `J-E2407` |
| 24 | `unknown_xfer` | `J-E2408` |
| 25 | `incompatible_generation` | `J-E2410` |
| 26 | `trace_segment` | `J-E2420` |
| 27 | `refused` | `J-E2207` |
| 28 | `no_generation` | `J-E2206` |
| 29 | `internal` | `J-E2012` |
| 30 | `not_delegated` | `J-E2303` |
| 31 | `catalog` | `J-E2203` |

### 10.2 Validation codes (`jk check`, `jk build`): `J0730`–`J0741`

| Code | Severity | Message | Where |
|---|---|---|---|
| `J0730` | error-in-prod | liveness condition not met: the lease of a remote writer does not survive one lost renewal | §11.6 |
| `J0731` | error | writer level outside the levels delegated to the link on that MCU output, or delegated level already used by a local MCU writer | §8.1 |
| `J0732` | warning | estimated link traffic above 70% of the transport capacity | §11 |
| `J0742` | error | estimated link traffic above 100% of the transport capacity: does not fit | §11 |
| `J0733` | error | link `max_lease` above the `max_lease` of an MCU output | §8.1 |
| `J0734` | error | `max_frame`, `max_msg`, `window` or `bulk_max` outside the transport limits | §3.7, §5.6 |
| `J0735` | error | MCU atomic group that does not fit in one frame, or that mixes MCU outputs with outputs of another owner | §6.12 |
| `J0736` | error | invalid or duplicate link address on the same bus (RS-485: 1–247; CAN: 1–253) | §3.2, §3.4 |
| `J0737` | error | link CAN identifiers conflict with another device on the bus | §3.4 |
| `J0738` | warning (all modes) | Joker Link without authentication (`auth = "none"`) on a closed bus | §4.6 |
| `J0739` | warning | `max_age` of an MCU signal less than 2 × `refresh` | §6.9 |
| `J0740` | error | MCU path with more than 8 segments under the device prefix | §8.3 |
| `J0741` | error | the MCU has apps with `requires_clock = "synced"` and the link has no `TIME` (`time_interval = 0`) | §8.4 |

Codes from other references used here: `J0179` (`auth = "none"` on IP, ref/01), `J0234` (output of a `joker-link` in the node's `[outputs]`, ref/01), `J0210`, `J0213`, `J0401`, `J0460`, `J0461`, `J0604`, `J0630`, `J0650`, `J0710`.

### 10.3 Runtime codes: `J-E2000`–`J-E2499`

Seen from outside the MCU: on the Linux node (logs, `jk status`, API), in the `joker.device.<d>.*` counters and in the ABI's `jk_link_status`.

| Code | Severity | Message |
|---|---|---|
| `J-E2001` | warning | frame with invalid CRC-32C (counter) |
| `J-E2002` | warning | frame above the maximum size |
| `J-E2003` | warning | invalid COBS or short frame |
| `J-E2004` | warning | ISO-TP: wrong sequence, timeout or overflow |
| `J-E2005` | warning | unknown frame format (`fmt` or `kind`) |
| `J-E2006` | error | malformed message: invalid CBOR, trailing bytes, required field missing or with the wrong type |
| `J-E2007` | warning | unknown message type |
| `J-E2008` | error | reassembled message above `max_msg` |
| `J-E2009` | error | retransmissions exhausted on a reliable message: session terminated |
| `J-E2012` | error | internal error in the peer |
| `J-E2101` | error | authentication failed (invalid Poly1305 tag) |
| `J-E2102` | warning | frame replayed or outside the anti-replay window |
| `J-E2103` | error | unknown `kid`: MCU not enrolled on this node |
| `J-E2104` | error | plaintext frame refused: authentication is required |
| `J-E2105` | error | session counter exhausted without a key update |
| `J-E2106` | error | link key rotation failed; the MCU reverted to the previous key |
| `J-E2201` | error | incompatible Joker Link version (no common major) |
| `J-E2202` | error-in-prod | the MCU does not match the node's lock or generation: session in `observe` |
| `J-E2203` | error | the catalog does not match the `HELLO` hash |
| `J-E2204` | warning | unknown session |
| `J-E2205` | error | link lost (`link_timeout`) |
| `J-E2206` | error | MCU without a valid generation: session in `maintenance` |
| `J-E2207` | error | session refused: identity not authorized for this device |
| `J-E2208` | error | message not allowed in the session mode, or feature not declared |
| `J-E2209` | error | class exported by the MCU with an incompatible major: instance not created |
| `J-E2302` | error | proposal, `READ` or `RESET` on a signal that is not an MCU output, or with the wrong type |
| `J-E2303` | warning | remote lease above `max_lease`: clamped; or level not delegated to the link |
| `J-E2304` | error | unknown write result: the `OUTCOMES` did not arrive in time |
| `J-E2305` | warning | a remote lease expired on the MCU |
| `J-E2310` | error | unknown identifier (`sid`, `mid`, `aid`, `alid`, `iid`, `id`) |
| `J-E2311` | error | invalid arguments |
| `J-E2312` | error | action rejected by the permissive |
| `J-E2313` | error | insufficient role |
| `J-E2314` | error | action timeout |
| `J-E2315` | error | busy: action queue or deduplication table full |
| `J-E2316` | warning | action canceled |
| `J-E2317` | error | idempotency key missing or invalid |
| `J-E2318` | error | unknown result: the link or the MCU restarted before the `RESULT`; the request was not repeated |
| `J-E2319` | error | write not sent: session in `observe` or `maintenance` |
| `J-E2320` | warning | `RESET` of an output that is not tripped |
| `J-E2401` | error | OTA: invalid signature |
| `J-E2402` | error | OTA: key without the role required for this artifact type |
| `J-E2403` | error | OTA: the image digest does not match the manifest |
| `J-E2404` | error | OTA: below the anti-rollback floor |
| `J-E2405` | error | OTA: wrong target or type for this MCU |
| `J-E2406` | error | OTA: no space in the area |
| `J-E2407` | error | OTA: flash write failed |
| `J-E2408` | error | OTA: unknown or expired transfer |
| `J-E2409` | error | OTA: the update was not confirmed within the window; rollback |
| `J-E2410` | error | OTA: generation incompatible with the firmware capabilities |
| `J-E2420` | error | `TRACE`: segment does not exist or is not frozen |
| `J-E2430` | warning | `TIME`: sample rejected (delay above `time_max_delay`) |
| `J-E2431` | warning | MCU clock in `holdover` or `unsynced` |

`J-E2010`, `J-E2011`, `J-E2013`–`J-E2099`, `J-E2107`–`J-E2199`, `J-E2210`–`J-E2301`, `J-E2306`–`J-E2309`, `J-E2321`–`J-E2400`, `J-E2411`–`J-E2419`, `J-E2421`–`J-E2429`, `J-E2432`–`J-E2499` remain free for this reference.

---

## 11. Budgets

Sizes computed with the reference encoder (deterministic CBOR, §6.2) and counted byte by byte; times on the wire with 10 bits per byte (UART 8N1), 11 bits per byte (RS-485 8E1) and, on CAN, the worst case of bit stuffing. The CPU and flash figures are estimates to be measured in the MCU phase.

### 11.1 Overhead per layer

| Layer | Bytes |
|---|---|
| L1 stream | `addr` 1 + CRC 4 + COBS ⌈n/254⌉ + delimiter 1 ≈ 7–10 |
| L1 datagram | CRC 4 (+ ISO-TP PCI: 1–2 per CAN frame) |
| L2 | 1 in plaintext; 25 in session; 37 in handshake |
| L3 | 10; 12 with `REL` |

### 11.2 U1: 32 signals at 1 Mbaud (19 §7)

RP2350 with 16 DI and 16 DO, UART at 1 Mbaud (100,000 B/s), 10 ms cycle on Linux, lease 30 ms.

| Message | Body | On the wire (plaintext) | Time | On the wire (encrypted) | Time |
|---|---|---|---|---|---|
| `SAMPLES`, 32 bool from the same tick (no `dt`) | 210 B | 228 B | 2.28 ms | 252 B | 2.52 ms |
| `SAMPLES`, 32 bool from different ticks (with `dt`) | 306 B | 325 B | 3.25 ms | 349 B | 3.49 ms |
| `SAMPLES`, 32 arbitrary `f32` (with `dt`) | 434 B | 453 B | 4.53 ms | 476 B | 4.76 ms |
| `PROPOSE`, 1 writer, 16 DO | 66 B | 84 B | 0.84 ms | 108 B | 1.08 ms |
| `OUTCOMES`, 16 entries (first time) | 101 B | 119 B | 1.19 ms | 143 B | 1.43 ms |
| full handshake (offer, answer, `READY`, `ACK`) | — | — | — | ≈ 660 B | ≈ 6.6 ms |

The ~250 B and ~2.5 ms of 19 U1 are confirmed for the encrypted case with the 32 changes in the same tick; with changes spread over the period, 3.5 ms. Occupancy in the D → H direction is 25–35% every 10 ms, and in the H → D direction 8–11%. The UART is full-duplex: the two directions do not compete.

### 11.3 CAN at 500 kbit/s

| Case | Classic CAN (29 bits, ISO-TP) | CAN-FD 500 kbit/s / 2 Mbit/s |
|---|---|---|
| 32 bool with `dt` (321 B with CRC) | 46 frames + 1 FC ≈ **14.9 ms** | 6 frames ≈ 2.3 ms |
| likewise, encrypted (345 B) | 50 frames + 1 FC ≈ 16.2 ms | 6 frames ≈ 2.5 ms |
| 4 signals (60 B) | 9 frames + 1 FC ≈ 3.1 ms | 1 frame ≈ 0.4 ms |

**U1 does not fit on classic CAN at 500 kbit/s**: 32 changes every 10 ms would occupy 150% of the bus. On classic CAN, Joker Link serves up to ~8 changes per 10 ms, or 32 per 50 ms (≈ 30% of the bus). `jk check` estimates the traffic from the signals, `report` and `refresh`, and gives `J0742` (above 70%, only the warning `J0732`). With CAN-FD, U1 fits with room to spare.

### 11.4 Other transports

- **RS-485 at 115,200 baud (8E1):** the 325 B `SAMPLES` takes 31 ms. A 10 ms cycle is impossible; it serves for `report` ≥ 100 ms, or the bus is moved to 1 Mbaud (common transceivers handle 10 Mbaud at cabinet distances). In half-duplex, the turn time is added (§3.3).
- **UDP or TCP on Ethernet:** bandwidth does not matter; latency does (sub-ms on a switch). **Wi-Fi:** 1–20 ms typical, with peaks of 100+ ms; remote writers over Wi-Fi should have leases ≥ 1 s, and never fast control.

### 11.5 OTA and trace

| Transfer | Time |
|---|---|
| 256 KiB firmware, UART 1 Mbaud, 256 B chunks | ≈ 3.2 s on the wire + flash write (≈ 2–3 s on an RP2350) ≈ **6 s** |
| likewise, classic CAN at 500 kbit/s, bulk at 50% of the bus | ≈ 24 s |
| likewise, CAN-FD 500 k / 2 M, 50% | ≈ 3.3 s |
| 8 KiB generation blob, UART 1 Mbaud | < 0.1 s |
| 16 KiB trace, UART 1 Mbaud | ≈ 0.2 s |

### 11.6 The liveness condition (`J0730`)

A remote writer must survive **one** lost renewal without the lease expiring:

```
lease ≥ 2 · renew + T_g + t_bulk + t_prop + 1 ms       (+ report + turn_frames · t_frame, in half-duplex)
```

`renew` = renewal period (§8.1); `T_g` = period of the output's owning group on the MCU (the proposal waits for the next latch); `t_bulk` = time on the wire of a `bulk_max` frame in the same direction (the one that may be halfway through when the proposal becomes ready; 0 on CAN, where bulk has its own identifier); `t_prop` = time on the wire of the `PROPOSE`; 1 ms for the Linux node's driver.

U1 with the default values: `2·10 + 1 + 3.1 + 1.1 + 1 = 26.2 ms ≤ 30 ms`. It holds. With `bulk_max = 1024` on the UART: `2·10 + 1 + 10.8 + 1.1 + 1 = 33.9 ms > 30 ms`, and `jk check` gives `J0730`. That is the reason for the 256 B default `bulk_max` on serial.

### 11.7 Memory on the MCU

| Structure | With the defaults | Reasonable minimum |
|---|---|---|
| reception (1 frame) | 1.1 KiB | 0.6 KiB |
| unacknowledged F messages (`window` × `max_frame`) | 4 KiB | 1 KiB (`window` 2, `max_frame` 512) |
| reassembly (`max_msg`) | 4 KiB | 2 KiB |
| `SAMPLES` in flight (8 × signals / 8) and dirty bits | 0.3 KiB (256 signals) | 0.1 KiB |
| deduplication (16 × ~144 B) | 2.3 KiB | 1.2 KiB (8) |
| remote writers and pending results | 0.4 KiB | 0.2 KiB |
| cryptographic state and session | 0.5 KiB | 0.5 KiB |
| **total** | **≈ 12.6 KiB** | **≈ 5.7 KiB** |

It fits in the 48 KB budget of 19 §3.1 with the minimum configuration on a small M1 (≈ 12% of the budget's RAM). In flash: CBOR codec 3–5 KB, ChaCha20-Poly1305 ≈ 3 KB, SHA-256 and HKDF ≈ 2 KB, Ed25519 verification 8–12 KB (needed anyway for the generation blobs).

---

## 12. Why CBOR and why these messages

**The honest comparison with postcard** (the compact serde encoding of the Rust ecosystem), for the most frequent message, `SAMPLES` with 32 entries. Body bytes:

| Case | CBOR (this document) | postcard, value as a tagged `enum` | "typed" postcard (the type comes from the catalog, with no tag or `Option`) |
|---|---|---|---|
| 32 bool, same tick | 210 | 279 (+33%) | 215 (+2%) |
| 32 bool, spread `dt` | 306 | 311 (+2%) | 247 (−19%) |
| 32 arbitrary `f32` | 434 | 407 (−6%) | 343 (−21%) |
| 32 analog values exact in half precision | 369 | 407 (+10%) | 343 (−7%) |

Typed postcard gains up to ~20% in the body, which in the worst case of U1 is ~90 B, **0.9 ms per frame at 1 Mbaud**. CBOR wins when there are defaults (the optional positions at the end of the array cost nothing) and when the value fits in half precision. In CPU, decoding 32 CBOR entries takes microseconds on a Cortex-M33: it does not count against milliseconds on the wire.

**Why CBOR, despite that:**

1. **Evolution without breaking anyone.** A CBOR reader ignores keys and positions it does not know; postcard is positional and not self-describing, and a new field breaks old readers. Rule 2 of ref/00 §1 and the promise of ADR 0022 (1.0 talking to 1.x in both directions) require the former.
2. **Two teams, one in C.** There is mature, small CBOR in C (TinyCBOR, QCBOR, NanoCBOR) and in `no_std` Rust (minicbor). postcard has a specification, but the real contract is the shape of the Rust `struct`s with serde: a team in C would have to imitate it.
3. **A single codec on the MCU.** The signatures (COSE_Sign1, ref/00 §7), the generation blob, the OTA manifest and the agent protocol (ref/10) are already CBOR. A second format would be a second codec in flash.
4. **Standard and tools.** RFC 8949, diagnostic notation, CDDL to validate in conformance (`spec/cddl/joker-link.cddl`), and anyone can read a captured packet with a generic tool.

The part of postcard's gain that comes from "the type is already known" was taken: the values carry no type tag beyond CBOR's own, and defaults cost no bytes.

**Why these messages:**

- **`PROPOSE`/`OUTCOMES` and not "write register".** The MCU owns the outputs and has its envelope; the Linux node is just one more writer. The wire carries what the core already has, the proposal buffer and the `WriteOutcome`s, and does not invent a parallel write model.
- **`SAMPLES` by changes, with state retransmission.** Resending an old sample is useless; resending the current value of a signal whose change was lost is what matters. `refresh` puts a limit on the age of a signal that does not change.
- **`CALL` separate from `PROPOSE`.** Cycle calls are repeated every tick, as on Linux, and go in `PROPOSE`. Those from outside the cycle need a response, and a `toggle()` must not be repeated by a resend: that is why they are reliable, and they are renewed with the proposals of the expansion.
- **`ACTION` with a required key.** The action is the command that is dangerous to repeat (17 §2.1), and the link is where repetitions are born.
- **`ALARM_CMD`, `RESET`, `RESTART`, `READ`, `CATALOG`.** They are the operations that the CLI and the API already have for alarms, trips, restarts and local `param`s; without them, an MCU would be a second-class citizen.

---

## 13. Versions and negotiation

**`link_version` = 1.0** (ref/00 §1; `[mcu.<d>] link` in the lock, ref/02).

1. D announces, in the offer, each major it speaks and the highest minor of each: `[[1, 0]]`.
2. H chooses the **highest common major** and, within it, **the smaller of the two maximum minors**; it replies with `[major, minor]`.
3. No common major: answer `refused` with status 3 (`J-E2201`), and `J0461` on the lock side. The answer uses the format of major 1, which any D understands.
4. After that, both sides behave as that minor: they do not send message types, keys or enum values introduced after it (and tolerate them if they arrive).

**Frozen forever**, in all majors: L1 (COBS, CRC-32C, `addr`, ISO-TP, datagrams); the first byte of L2 and the `kind 0` and `kind 1` forms; the derivation of `kid` and `K_hs`; the L3 header; type `0x01` and keys 1–13 of the offer and 1–10 of the answer, with their types. This is what allows a v2 H to talk to a v1 D and vice versa (21 §1).

**A minor only adds** (ADR 0022): new types (first in `0x31`–`0x3F`), optional keys, enum values at the end, feature bits, statuses at the end of §10.1. A withdrawn key, type or `status` is never reused (ref/00 §1, rule 4). A 1.x `jokerd` talks to any 1.y Joker Micro: they negotiate the smaller of the minors.

**A new major** changes the rest (L3 and messages), with an H that speaks both majors throughout the following series.

---

## 14. Micro profile over MQTT and CoAP: framing

A standalone M2 MCU speaks to the Deck the agent protocol, in the reduced version of ref/10 (§2.14, §2.15), and not Joker Link. From Joker Link, that profile only uses the framing and the common code. The rule, so that it is the same in both references:

| | MQTT (5, or 3.1.1 in the micro profile) | CoAP (RFC 7252) |
|---|---|---|
| unit | one agent message = the payload of one `PUBLISH` | one message = the payload of a request or response; artifacts in Block2 (RFC 7959) of 512 B or 1 KiB |
| content | **one** CBOR item: `COSE_Sign1` with tag 18 (ref/10 §3.11.3) | one CBOR item; Content-Format 60 (`application/cbor`) or 18 (COSE_Sign1) |
| integrity and confidentiality | TLS with mTLS + the COSE signature | DTLS 1.2 |
| delivery | QoS 1 (duplicates possible; deduplication belongs to ref/10) | `CON` messages |
| size | ≤ 4 KiB per packet (ref/10 §2.14) | ≤ 1 KiB per payload outside Block2 |

- There is **no** COBS, CRC-32C, L2 header or L3 header: the transport already provides framing, integrity, confidentiality and delivery. Adding them would be paying twice.
- **Common with Joker Link:** the encoding rules of §6.2; the COSE_Sign1 verification and the OTA manifest of §9.1 (the `joker.micro-firmware.v1` and `joker.generation.v1` artifacts are the same); A/B activation with a confirmation window on the MCU (§9.4); resume by `offset` of §9.3, applied to HTTPS `Range`, CoAP Block2 and `blob/data`.
- The key of the micro profile is the MCU's certificate (ref/10), not `K_link`. An M2 may have, at the same time, a Joker Link session over UDP or TCP with a `jokerd` and the agent with the Deck: they are independent.
- Joker Link does **not** run over MQTT. Leases of tens of ms and per-packet acknowledgments do not go through a broker, and the QoS 1 duplicates would force a second deduplication.

---

## 15. Configuration this reference needs

The keys belong to ref/01 (appendix A, `[buses]`, `[link]` in §5.22). This table says what the link reads and with which defaults; the syntax is that of ref/01.

**On the Linux node, `[devices.<d>]` with `driver = "joker-link"`:**

| Key | Type | Default | Validation |
|---|---|---|---|
| `bus` | name from `[buses]` with `kind = "serial"` or `"can"` | — | exactly one of `bus`, `usb`, `address` |
| `usb` | path (`/dev/ttyACM0`) | — | |
| `address` | `"udp:<ip>:<port>"` \| `"tcp:<ip>:<port>"` (M2 MCUs) | port 7471 | always an IP address, and always the **MCU's**. With `udp:`, H only accepts a session whose datagrams come from this IP, sends to it (and to this port) the `HELLO(request)` while there is no session, and then replies to the source of the last authenticated packet (§3.5); H listens on 7471. With `tcp:`, H does not connect to this address (D is the one that connects, §3.6): it accepts the connection only from this IP. In the MCU manifest, `[link] address` is the opposite, the IP and port **of the node** (ref/01 §5.22) |
| `addr` | integer: the MCU's address on the bus | 0 (serial), 1 (CAN) | `J0736` |
| `node` | name expected in `HELLO` (key 4.1) | — | otherwise `refused`, `J-E2207` |
| `can_id_base` | integer 0–255 | `0x4A` | `J0737` |
| `auth` | `"required"` \| `"none"` | `"required"` | `J0179` on IP; `J0738` |
| `key` | `"secret:<name>"` | `"secret:link.<d>"` | |
| `report`, `refresh`, `keepalive`, `link_timeout`, `max_lease`, `rto_min`, `ack_delay`, `time_interval`, `time_tolerance`, `time_max_delay`, `turn_timeout` | `Duration` | §5.6, §8.4, §3.3 | `J0730`, `J0733`, `J0741` |
| `max_frame`, `max_msg`, `window`, `bulk_max`, `retransmit`, `turn_frames` | integer | §5.6 | `J0734` |
| `instances` | table `<name on the MCU> = "<name on the node>"`, only for top-level instances (§8.3) | the MCU's names | `J0604`; instance that is only a member `J0650` |
| `micro` | `File`: the path of the MCU manifest (`micro/esp/joker.toml`), relative to the file that declares it (ref/01 §7.5) | — | optional. With it, the node's `jk check` **without a connection to the node** generates the MCU catalog from that manifest (the same `jk build` the MCU uses) and validates the bindings, the delegated levels (`J0731`), `max_lease` (`J0733`) and liveness (`J0730`) as if it had the `HELLO`; `jk apply` uses it to build the MCU's generation blob and carry it over the link, when the hash of the MCU's generation differs (ref/08 §2.12.4). Without it, `jk check` only validates the MCU side with `--node` (through the `HELLO` pinned in `[discovered.<d>]`), and the blob is made by hand (`jk build --target mcu:<board> --generation`). Nonexistent file, or a manifest whose `[target] board` is not an MCU: `J0145` |

In `[buses.<b>]` of `kind = "serial"`: `duplex = "full" | "half"` (default `"full"`) and `turnaround`.

Example (U1, the Raspberry Pi's coprocessor):

```toml
[buses.uart1]
kind   = "serial"
port   = "/dev/ttyAMA1"
baud   = 1000000
parity = "none"
duplex = "full"

[devices.io]
driver    = "joker-link"
bus       = "uart1"
auth      = "required"
key       = "secret:link.io"
report    = "5ms"
refresh   = "1s"
bulk_max  = 256
instances = { p1 = "pump_a" }
```

**On the MCU, `[link]`** (ref/01 §5.22): `bus` or `address` (IP and port **of the Linux node**, M2), `addr` (the address of the MCU itself on the bus, 1–247 on RS-485 and 1–253 on CAN), `peer` and `auth`. In `[outputs.<s>]`, `remote_levels` (levels delegated to the link, ref/01; default: those that no local writer uses) is key 1.8 of the catalog.

---

## 16. Stress

**L1 — `jokerd` dies midway (U1).** The last `PROPOSE` was consumed at t; the 30 ms lease expires at the MCU's latch at t + 30 ms and the output moves to the next level or to `safe`. `link_timeout` (200 ms) only marks the link. When the new `jokerd` starts, `HELLO(request)`; the MCU obeys after 100 ms of silence; new session in ~7 ms; proposals back. **Holds**, with no rule beyond A2.

**L2 — A new generation on the Linux node renumbers the writers.** With ownership by `origin`, the new writer at level 10 got `NotOwner` until the old one's lease expired: 30 ms with the output dropping a level. **Broke in the first design.** **Fix:** remote ownership belongs to the level (§8.1); uniqueness per level is already guaranteed by the node's `jk check`.

**L3 — An OTA on the same UART as control.** 1 KiB chunks are 10.8 ms on the wire; a `PROPOSE` that becomes ready right after waits, and with one lost renewal the 30 ms lease expires. **Broke.** **Fix:** send priorities (§5.7), 256 B `bulk_max` on serial and the liveness condition `J0730` in `jk check` (§11.6). On CAN, a bulk channel with its own identifier.

**L4 — Replay on UDP.** An attacker records a `PROPOSE` with `do_1 = true` and repeats it later. In the same session, the anti-replay window discards it; in a new session, the keys are different. A recorded answer `HELLO` is of no use, because the echo of `nonce_d` does not match. **Holds.** On a plaintext RS-485 there is no protection at all, and that is declared (`J0738`).

**L5 — MCU without a TRNG repeating `nonce_d`.** With the nonce repeated after a restart, a recorded answer would be accepted again and the session's old frames could be replayed against the MCU. **Broke.** **Fix:** `nonce_d` from a counter persisted **before** sending, passed through HMAC (§4.3).

**L6 — The `SAMPLES` with the rise of an input is lost.** With change-based sending, the signal would never be sent again if it did not change again. **Fix:** state retransmission (§5.4), bounded in time by `refresh`.

**L7 — `ACTION` repeated after a link drop.** H did not receive the `RESULT` of a `reset` and resends it when the session comes back: deduplication by key returns the stored result, with `repeated = true`. If the MCU restarted in the meantime, the table was lost: H does **not** repeat, and the ticket becomes `J-E2318`. **Holds**, with the boundary declared.

**L8 — `toggle()` via `jk call` with a resend.** A resend of the same call would cancel it out. **Fix:** reliable `CALL` with `rseq` (exactly once in the session), optional key across sessions, and renewal through the expansion returned in `RESULT` (§8.5), without executing the method again.

**L9 — A technician flashes in the field a generation blob different from the pinned one.** `HELLO` shows another generation hash: session in `observe`, drift alarm, no write from the node in `prod`. The MCU keeps controlling with its logic and its `safe`. **Holds.**

**L10 — An MCU with 300 signals.** The catalog exceeds 10 KiB and does not fit in `HELLO`. **Fix:** `HELLO` carries only the hash; the catalog goes by `CATALOG` in chunks and is cached on the node (§6.11). The full `SAMPLES` after `READY` is several messages, at the rate of `report`.

**L11 — U1 on classic CAN at 500 kbit/s.** 15 ms of bus per batch every 10 ms. **Does not fit**, and `jk check` says why (`J0742`, with the calculation of §11.3). The answer is CAN-FD, fewer signals per batch, or a larger `report`.

**L12 — A 1.3 node with a 1.0 Joker Micro.** The node proposes 1.3, the MCU only goes up to 1.0: they negotiate 1.0 and the node does not use the new messages. The reverse too. **Holds** (ADR 0022).

---

## 17. Decisions and rejected alternatives

| Decision | Alternatives | Why |
|---|---|---|
| CRC-32C (Castagnoli) | CRC-32/ISO-HDLC (the Ethernet "CRC-32") | better Hamming distance at the link's frame sizes (HD = 6 up to ~650 B, against ~33 B); the RP2350's DMA sniffer only does the Ethernet CRC-32, but CRC-32C in software (1 KiB table, ~5 cycles/B) costs < 0.5% of CPU at 1 Mbaud, and the STM32G4/H5 have a CRC with a programmable polynomial |
| CRC on all transports | only on serial | one rule; separates noise from attack |
| COBS | SLIP, HDLC with *byte stuffing* | bounded overhead (1 B in 254) and known before sending; decoding in the buffer itself |
| binary L3 header | header in CBOR | acknowledge and order without decoding the body; a fixed 10 B |
| acknowledgment by `ack` + `ack_bits` in every packet | reliable ordered stream for everything (TCP style) | state messages should not be resent; reliable ones should; a single mechanism serves the three classes |
| state retransmission in `SAMPLES` | reliable `SAMPLES` | resending old values wastes bandwidth and delays the new ones |
| reliable `OUTCOMES` | state, like `SAMPLES` | each result is a decision that goes into the decision log; an out-of-order `ref` would resolve tickets with the wrong result |
| remote ownership per level | per `origin` | L2 |
| lease in ms, with a `max_lease` ceiling | µs; manual TTL on the MCU | 1 ms is enough (the smallest useful lease is 3 ms); the long TTL on the MCU would let an 8 h override survive the death of `jokerd` |
| PSK (`K_link`) with HKDF and nonces from both sides | DTLS; ECDH with certificates | fits in a few KB, the same thing on all transports, no PKI on the MCU; forward secrecy is left for a 1.x (key 14 reserved) |
| key update by the `phase` bit | new handshake | a handshake stops the proposals for ms; the update stops nothing |
| security configured on both sides, without negotiation | negotiate `none` or `required` | makes *downgrade* impossible |
| separate catalog, by hash | everything in `HELLO` | `HELLO` must fit in 512 B; the catalog may have tens of KiB and rarely changes |
| reliable `CALL` separate from `PROPOSE` | only `PROPOSE` | L8 |
| COSE manifest before the image | signature at the end of the image | refuse before erasing flash; the same manifest serves the micro profile |
| confirmation window counted on the MCU | counted on the Linux node | the node may be the cause of the failure |
| one session per MCU in v1 | several hosts | redundancy is v2 (21 §8); two owners of remote levels would be a distributed arbiter |

---

## 18. Version markers

| Item | Version |
|---|---|
| everything not in this table | **1.0** |
| `BLOCKS` (`0x30`): block signals (M7), vibration and waveforms of Joker Micro (19 U3) | **reserved 1.1** |
| `at` in proposals (position 2 of `prop`): scheduled writes (M6) | **reserved 1.1** |
| types `0x31`–`0x3F` | reserved 1.x |
| `OTA` type `model` (3): ML on MCUs | **reserved 1.4** (document 11, §"Ready for ML"; document 21, §6) |
| `OTA` type `app` (4): Wasm apps on M2 MCUs (`wasmi`) | **reserved 1.4** |
| key 14 of `HELLO`: ephemeral X25519 (forward secrecy) | reserved 1.x |
| quality reason `out_of_distribution` (6) in samples | already in 1.0 (ref/00 §4.3); only produced with ML on the MCU (1.4) |
| several sessions per MCU (active/standby, quorum arbiter on a Joker Micro) | v2 (21 §8) |
