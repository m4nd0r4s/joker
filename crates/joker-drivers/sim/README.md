# joker-driver-sim

The `sim` driver (tier N, ref/01 appendix A, ref/03 §2.4): one simulated device per instance, served from a `*.sim.toml` model. It is what runs the learning track's parts 1–5 on a laptop (`jk dev`), with the same device and signal names as the `sysfs` and `modbus-rtu` devices it stands in for.

The plant moves in fixed steps of virtual time driven by the host's clock (ADR 0007), so the same model, clock and writes give the same bits. The M0 subset of the model format is in ADR 0051.
