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

The real Linux adapter (opening `/dev/i2c-N` and issuing `I2C_SLAVE` +
the read/write, or `I2C_RDWR` for an atomic repeated start) is not
implemented here yet — `Link::I2c` and this driver are verified against
an in-memory double (`joker_hal::testing::MemoryI2c`), the same way
`Link::Gpio` and `Link::Tcp` shipped before their own adapters.
