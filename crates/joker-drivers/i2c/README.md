# joker-driver-i2c

Register-style chips on a Linux I²C bus, tier N (docs/05 §10).

Address keys (ref/03 §2.2.5): `reg` (integer 0–65535, the chip register),
`len` (1–64 bytes, with `reg`), `byte_order` (`big` | `little`, default
`big`). `channel` (a chip-handler channel, for chips with their own read
sequences like multiplexed ADCs) is not implemented here: it needs
per-chip protocol logic beyond the generic write-then-read transfer.

Device config: `address` (the 7-bit I²C device address, 0x03–0x77, ref/03
§2.2.20's URI table), resolved against the bus the manifest declares
(`BusKind::I2c`, e.g. `/dev/i2c-1`).

On a Linux node the bus is `joker_hal::std_host::LinuxI2c`: the `dev` of
the device's `i2c` bus (`/dev/i2c-1`), opened through `i2c-dev`, with each
register read sent as one `I2C_RDWR` call (`joker-i2c-sys`), the register
write and the data read joined by a repeated start. A chip that does not
acknowledge reads `bad(device_fault)`, a refused or busy address
`bad(config_error)`. Tests run the same driver against an in-memory bus
(`joker_hal::testing::MemoryI2c`).
