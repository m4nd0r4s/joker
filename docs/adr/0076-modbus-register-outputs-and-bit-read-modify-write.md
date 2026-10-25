# ADR 0076 — Modbus register outputs, and a `bool` output in a register bit by read-modify-write

**Status:** accepted (v1) · **Date:** 2026-10

## Context

ADR 0075 gave jokerd an output stage that turns an applied value into the registers of its point, but the Modbus drivers still wrote coils only (FC 5, ADR 0047) and answered `unsupported` to anything else, so a converted register output never reached the equipment (AUR-428). ADR 0075 §10 also kept refusing a `bool` output in one bit of a register (`J0807`), because writing one bit of a 16-bit register needs the rest of the register.

The references leave three questions open:

1. ref/03 §2.2.5 admits `bit` "only with FC 3/4", so a bit output has no address at all.
2. ref/06 §2.7 says what a register output carries (`regs`, `width` words), but not what a bit output carries: the output stage knows the bit, not the other fifteen.
3. ref/03 §2.2.6 declares the `single_write` quirk, and no reference says what it does to an output.

## Decision

1. **Outputs write with FC 5, 6, 15 or 16** (ref/03 §2.2.5). FC 5 and 15 write one coil from a `boolean`; FC 6 writes one register; FC 16 writes the `width` registers of the point's type. A type of more than one register on FC 6 is `J0809`, at plan time. The registers come from the output stage, already converted and laid out with `word_order` and `byte_order` (ADR 0075 §8); the driver puts them on the wire in that order.
2. **A `bool` output in a register bit** has the address `{ at, fc = 6 | 16, bit }` and `type = "bool"`. Its point is `regs` with `width` 1, as on a read (ref/06 §2.7), but the value the output stage hands the driver is the `boolean` after `invert`. The driver reads the register with FC 3, sets or clears the bit and writes the register back with the address's function code. A failed read is the failure of the write (an exception is `device-rejected`, a silent line `timeout`), and nothing is written.
3. **What the read-modify-write does not promise.** The read and the write are two requests: another Modbus client changing the same register between them is overwritten. Within jokerd they are not interleaved with any other request of the device, because a device's writes and reads go through one serialized queue (ref/06 §2.14). Equipment where that matters exposes the bit as a coil, or its own register per command.
4. **Confirmation.** A write is confirmed by its echo: the answer to FC 5 and 6 repeats the request, the answer to FC 15 and 16 repeats its address and count. Any other answer is `comm-failure`, and the output stays as ref/06 §2.9 says.
5. **`single_write = "fc16"`** writes a one-register output, and the register of a bit output, with FC 16 even when its address says FC 6. `single_write` takes `"fc6"` or `"fc16"`; any other value is `J0818`.
6. ADR 0075 §10 is withdrawn: a `bool` output in a register bit is not refused.

## Consequences

- ref/03 §2.2.1 drops the bit output from the refused surfaces; §2.2.5 states rules 1–3; §2.2.6 states rule 5; ref/06 §2.7 states what a bit output carries and rule 4.
- `joker-driver-modbus` builds output writes in `access::OutputWrite`, shared by `modbus-rtu` and `modbus-tcp`; `jokerd::bridge` gives a Modbus `bool` with `bit` the shape `regs`/1 and no longer refuses it.
- Step programs keep ADR 0073: a `bit` on a step's `write` is still `config-error`.
