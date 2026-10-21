# joker-driver-sim

The `sim` driver (tier N, ref/01 appendix A, ref/03 §2.4): one simulated device per instance, served from a `*.sim.toml` model. It is what runs the learning track's parts 1–5 on a laptop (`jk dev`), with the same device and signal names as the `sysfs` and `modbus-rtu` devices it stands in for.

The plant moves in fixed steps of virtual time driven by the host's clock (ADR 0007), so the same model, clock and writes give the same bits on every architecture. It reads the whole model format of ref/03 §2.4: every generator (`replay` from CSV), the dynamics language with units, scheduled faults and fault keys, enums, simulated actions as native actions, `[implements]`, `[model] profile` and the `joker:sim/plant` surface, whose Wasm engine the node's host provides. ADR 0051 fixes how the model reaches the driver and ADR 0054 the rest.
