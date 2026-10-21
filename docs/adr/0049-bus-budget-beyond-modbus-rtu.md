# ADR 0049 — The bus budget beyond Modbus RTU

**Status:** accepted (v1) · **Date:** 2026-10

## Context

Document 05 §5 defines the bus occupancy that `jk check` estimates (`J0370`) with a worked Modbus RTU example, and document 16 §10 leaves the model for the other buses to this task (document 13 §4, task 17). Three things are open: how the occupancy of a bus that is not RTU is computed, where the per-request cost comes from, and at what occupancy `J0370` fires (the example says 135% fails and uses 80% as the target for the viable poll, but does not say what happens between 80% and 100%).

## Decision

1. **Occupancy** of a bus = the sum, over the devices on it and over their read batches, of `cost(batch) / poll(batch)`, where `poll` is the batch's effective period (the device `poll`, or the signal `poll`). Only buses that serialise access are budgeted: `serial`, `i2c`, `spi`, and an `ethernet` bus shared by devices (ref/01 §5.6). A `can` bus is budgeted by load instead (point 3). TCP devices without `bus` are not budgeted.
2. **The cost of one batch**, in this order:
   - the `cost-ns` the driver's read plan gives for the batch (ref/06 §2.6), when there is one;
   - otherwise, by transport, from the request and response sizes of the batch:
     - serial: `(request + response) bytes × bits per character / baud + turnaround + slave latency`, with bits per character = start + data bits + parity + stop bits (8E1 is 11) and the slave latency of the profile (`quirks.latency`, 10 ms by default, ref/03 §2.2.6). This is document 05 §5;
     - I²C: `(request + response + 2) bytes × 9 bits / speed`, at 100 kHz when the bus declares no speed;
     - SPI: `(request + response) bytes × 8 bits / speed`, at the bus `speed`;
     - a shared `ethernet` bus: the slave latency per batch, since one connection carries one request at a time.
3. **CAN load** = the sum, over the frames per second of all the devices on the bus, of the worst-case frame length (8-byte classic frame: 135 bits with stuffing; CAN FD: header at `bitrate`, data at `data_bitrate`) divided by the bit rate. Frames per second come from the driver plans or, for Joker Link, from ref/07 §15.
4. **Thresholds.** An occupancy or load above **100%** is `J0370`: the bus cannot serve the polls. The help gives the minimum poll that brings it to the **80%** target of document 05 §5. Between 80% and 100%, `jk check` writes a note, not a code.
5. **Inputs.** The batch sizes and costs come from the driver planners (`Planner`, ref/06 §3), which `jk check` runs before the budget; the manifest crate takes them as data (the batches of each device), so it stays independent of the drivers.

## Consequences

- The RTU example of document 05 §5 gives the same numbers under this ADR (135%, `J0370`).
- A driver whose planner gives `cost-ns` overrides the transport formula, which is how tier W and X drivers enter the budget.
- The 80–100% band is a warning without a code on purpose: occupancy estimates are approximate, and failing at 80% would reject buses that work.
