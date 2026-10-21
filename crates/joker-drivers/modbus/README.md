# joker-driver-modbus

The `modbus-rtu` driver (tier N, ref/01 appendix A, ref/03 §2.2): a Modbus RTU client on a `serial` bus, driven by a `modbus` profile. In M0 it talks to the bench ESP32 through its USB bridge with `profiles/local/bench-esp32@1.profile.toml` (docs/23 §4).

- `plan` coalesces the points of one function code and poll period into requests (`max_regs`, `max_bits`, `max_gap`), and computes the bus cost from the baud rate and the `latency` quirk.
- `read` makes one request per batch (FC 1, 2, 3, 4) and delivers `boolean` or `regs` raw values; `write` sets coils with FC 5 and checks the echo.
- A silent line is `timeout`, a port that went away is `not-connected`, a bad CRC or an unexpected answer is `comm-failure`: all three become `bad(comm_failure)` on every point of the batch. An exception response is `device-fault`.
- `profile` is the M0 stand-in for the core's profile handling (parsing, points of a device, register decoding and deadband) until the manifest front provides it (AUR-201).

Register writes (FC 6, 15, 16) and `modbus-tcp` are not in M0 (ADR 0047).
