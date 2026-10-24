# ADR 0073 — Access by address in the catalog drivers, and the wire double of the action suites

**Status:** accepted (v1) · **Date:** 2026-10

## Context

ADR 0058 put the step interpreter in `joker-hal` and ADR 0066 gave actions their lifecycle and a conformance area. No catalog driver implemented `read-addr` or `write-addr` (ref/06 §2.10), so every step of an action on real equipment failed with `J-E1408` (AUR-426). Implementing them leaves four questions the references do not settle:

1. **The form of the value.** ref/06 §2.7 makes a Modbus point `regs` and leaves the decoding (`word_order`, `byte_order`, `bit`) to the core's acquisition stage. A step has no acquisition stage: the interpreter reads and writes one scalar of the step's `type` (ADR 0060), and the step's address carries `word_order` and `byte_order` with it. Who turns registers into that scalar is not written down.
2. **What a step may reach on `sysfs` and `gpio`.** Their host grants only what the device's points declare: the directories of the LEDs and zones, the lines (requested at open with a fixed direction). A step can name another LED or line.
3. **Native actions.** ref/06 §2.10 names OPC UA methods and CANopen SDOs; the 1.0 catalog drivers have no operation of their own to offer.
4. **Conformance.** The `action` suites (ADR 0066 §20–§24) run every device against a register double that answers `read-addr`/`write-addr` itself, so they never prove a real driver.

## Decision

### Access by address

1. `modbus-rtu`, `modbus-tcp`, `sysfs` and `gpio` declare the capability `raw-access` and implement `read-addr` and `write-addr`. `i2c` does not yet; a step program on an `i2c` device stays `J-E1408`.
2. **A step's value is a scalar.** `read-addr` answers with one sample whose raw value is the scalar of the step's `type` (`boolean`, `int`, `uint` or `float`, as `joker_hal::interpreter::kind_of_type` maps it), and `write-addr` receives a scalar of that kind. The driver does the protocol's encoding from the step's address: for Modbus, the register count of the type (ref/03 §2.2.7), `word_order`, `byte_order` and `bit`. `bits16`/`bits32`/`bits64` are unsigned; `string`, `bytes` and any non-scalar type are `config-error`.
3. **Modbus.** One request per step. `read`, `until` and `fail_if` read with FC 1, 2, 3 or 4; `write` writes with FC 5 (one coil), 6 (one register: types of one register only), 15 (one coil) or 16 (the type's registers). A step without `type` is `bool` on FC 1, 2, 5 and 15 and `u16` on FC 3, 4, 6 and 16; a `type` that does not fit its function code, a `bit` on a write, or a value outside the type's range is `config-error`, never a value saturated or truncated. A write is confirmed by its echo; an exception is `device-fault`, as for `read` (ref/06 §2.8).
4. **`sysfs`.** A step reads a LED's `brightness` (a `bool` for any non-zero level, or the level with an integer `type`) or a zone's temperature in °C, and writes a LED with a `bool` (`max_brightness` or 0) or a level from 0 to `max_brightness`, reading it back. A thermal zone is read only (`config-error`). The host grants the directories of the step addresses of the device's actions as well as those of its points.
5. **`gpio`.** A step reads a line one of the device's points requests and writes a line one of its outputs requests, as a `bool`, applying its own address's `active_low`. Any other line is `config-error`: the host requests lines only for points, and an output line's initial level comes from its `safe` (invariant 5).
6. **Native actions** stay `unsupported` in the four drivers: none has an operation of its own in 1.0, and a profile of their families declares none.

### The wire double

7. The `action` suites run each case twice: once with the register double of ADR 0066 §21, and once with the **wire double**, where every `modbus-tcp` device of the fixture runs the real `modbus-tcp` driver, its own planner and the daemon's io task against a Modbus TCP server on loopback that serves the same `[[double]]` declaration. Devices of other drivers keep the register double. The wire run of a case is reported as `<suite>.<case>@wire`.
8. The wire double holds one value per address. FC 1 and 2 read it as a bit (non-zero), FC 3 and 4 as a 16-bit register, an address with no value reads 0; FC 5, 6, 15 and 16 write it, record the write in order and apply `on_write`. A write of several registers is recorded once, at its first address, with the registers joined in order. `fail` takes its wire form: `device_fault` is exception 04, `timeout` is no answer, `comm_failure` an answer to another transaction. `link = "down"` is a port nobody listens on.
9. `quality` and `[double.native.<action>]` have no form on the wire. A case that depends on them says so with the case key `doubles = ["register"]`; `doubles` lists `register` and `wire`, both by default, and anything else is `J0887`.

## Consequences

- ref/06 §2.10 states rules 2 and 6; conformance/README.md documents rules 7–9; ref/03 §2.6.1 names both doubles of the `action` kind.
- `cargo xtask conformance` depends on `joker-driver-modbus` (amending nothing in ADR 0057: `xtask` is still a leaf).
- `jokerd` needs no change to run actions on real devices: its io tasks already hand every step to the driver. The real `sysfs` device grants the step directories.
- An `i2c` step program and the Modbus string and byte types stay open.
