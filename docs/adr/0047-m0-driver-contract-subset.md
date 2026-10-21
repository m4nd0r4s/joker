# ADR 0047 — The M0 subset of the driver contract (`joker-hal`)

**Status:** accepted (M0) · **Date:** 2026-10

## Context

M0 needs three tier N drivers (`sim`, `sysfs`, `modbus-rtu`; document 26, §6) and therefore the part of ref/06 §3 they use. Writing `joker-hal` against ref/06 showed facts the reference does not state, or states in a way that cannot be built as written:

- ref/06 §3.1 names `minicbor` for the CBOR views. `minicbor` is licensed `BlueOak-1.0.0`, which is not in the licence allow list of `deny.toml`, so CI refuses it.
- ref/06 §2.9 and invariant 3 say that drivers only receive committed writes, but nothing says how that is enforced, which side builds a `committed-write`, nor what `seq` is unique over.
- ref/06 §3.4 lists the transports as concrete types implementing the `embedded-io-async` traits. Those traits use `async fn` and are not object-safe, so a host cannot hide its port behind one type through them.
- The serial transport must honour `dtr` and `rts` (ref/01 §5.6, default `"low"`). Setting the modem lines needs an `ioctl` that no safe crate in the allow list exposes; invariant 10 only allows `unsafe` in `*-sys` crates.
- The acquisition deadband (ref/01 §5.8.1, ref/03 §2.2.4) has no stated comparison: whether a move equal to the band passes, and what a band of 0 does.

## Decision

### Scope of M0

1. `joker-hal` implements the records of ref/06 §3.2 with the CDDL's numeric values, the `Driver` trait of §3.3 with every method and its default, `Planner` (feature `plan`), `RtPart`, `RtCapable` and a minimal `RtIo`, the plan of §2.6 with its validation (`J0856`), and `Cx` with `now`, `deadline`, `sleep_until`, `report_link`, `progress`, `cancelled` and `log`. `Cx::log(level, message)` replaces the `Logger` accessor.
2. `Cx` also offers `until(at, future)` and `within(future)` (the call's deadline), the one combinator every driver needs to turn a silent equipment into `timeout` without a runtime of its own.
3. Deferred, with their surface reserved as in ref/06: `Cx::locked`, `Cx::spawn_blocking`, `Scanner`, `DriverEntry` and the registry by features (§3.8), the schema language of §2.3.1 (each driver's `plan` validates its keys until then, with the same codes), `AdcRead`, `CanFrames`, `RtChannel`, and the `Tcp`, `Udp`, `I2c`, `Spi`, `Can` and `Gpio` links.

### CBOR

4. `joker-hal` carries its own reader and writer for the deterministic CBOR subset of ref/06 §2.4.3 (integers, strings, arrays, maps with text keys, booleans, `null`, floats). Reading borrows and never allocates; writing sorts map keys by their encoded bytes and always writes floats as 64-bit. Adding `BlueOak-1.0.0` to the allow list, and moving to `minicbor`, stays open for the project owner; the views would not change.

### Committed writes

5. `CommittedWrite` has a private field. Outside `joker-hal` it can only be obtained from `commit::CommitRx`, which builds it from two sources: the applied value of an output, read by `commit::CommitTx` from `joker_core::Outputs` after `Outputs::commit`, and the output's declared `safe`, sent when the supervisor sets the box's `force_safe` bit. No other constructor exists, so a driver cannot be handed a value that did not go through the envelope.
6. The channel is one **latest-wins box** per output bound to a driver point (document 01, §3.4): a seqlock of 32-bit atomics, written by the cycle side only when the applied value changed, read by the io side, which emits one write per new value. After each `connect`, `CommitRx::resync` makes every box with a value emit it again as `resync` (ref/06 §2.1, step 6). While `force_safe` is set, new values are held back and the box emits one `force-safe` write; when it is cleared, the current value is sent as `resync`.
7. `seq` is unique and increasing per receiver (one per driver instance), which is what the acks are matched against.
8. M0 converts applied values to `bool`, `int`, `uint` and `float` raw values only. The bindings are checked against the output types when the channel is set up. Scaled register writes (`(v − offset) / scale`, ref/06 §2.7) come with the output stage.

### Transports

9. A serial line is `SerialPort`, a boxed `SerialIo`: a poll-based, object-safe trait the host implements (tokio on Linux, a double in tests, Embassy later). `SerialPort` implements `embedded_io_async::{Read, Write}` (0.7) on top of it, so drivers stay portable. The variant needs `alloc`.
10. `Link::Paths` carries a root and the relative prefixes under it that the manifest grants (`DevicePaths`). Paths outside them, or with `..`, are refused. On a node the root of `sysfs` is `/sys`; in tests it is a temporary directory.
11. The Linux serial transport (`std_host::TokioSerial`) uses `rustix` termios and tokio's reactor; the modem lines go through the new crate `joker-serial-sys`, whose single function wraps `TIOCMBIS`/`TIOCMBIC`. The port is opened in raw mode with `VMIN = 1, VTIME = 0`, so that a non-blocking read with no data is `EAGAIN` on every Unix and a read of 0 bytes means the device went away.

### Acquisition

12. The checks of ref/06 §3.6, steps 1–2, live in `joker-hal` (`acquire`), shared by the three tiers: a sample whose kind differs from `expect`, whose `regs` width differs from the point's, a disallowed pair, a `good` without a value, or a second sample for the same point is `J0869` and `bad(config_error)`; a point without a sample is `bad(comm_failure)`; a batch failure applies its quality to every point. A `bad` result never carries a value (invariant 4).
13. **Acquisition deadband:** a numeric value is suppressed when it moved by **no more than** the band from the last published value (`|Δ| ≤ band`). A band of 0 suppresses nothing, and a change of quality, or the first value, is always published.

## Consequences

- `cargo build --target thumbv8m.main-none-eabihf -p joker-hal` (and with `alloc,plan`) is part of the `no_std` CI job.
- The type-level guarantee rests on `joker_core::Outputs`: its applied values only change through `commit` and, before the first commit of a generation, `inherit` (A8). Any future way of setting them must go through the envelope as well.
- When ref/06 is next revised (the English translation is in review), §3.1 must say "deterministic CBOR (own subset codec, ADR 0047)" instead of `minicbor`, §3.4 must list `SerialIo`, and the deferred items above stay marked as M0 gaps.
- `joker-hal` has a `testing` feature (virtual-time host and executor, in-memory serial line, bench scheduler, a real envelope with a manual writer). Driver tests obtain their writes through the envelope like production code.
