# joker-i2c-sys

The Linux `i2c-dev` ioctl `I2C_RDWR` (`<linux/i2c-dev.h>`, `<linux/i2c.h>`), for the `i2c` driver's real Linux adapter: one call carries a write and a read to the same 7-bit address, joined by a repeated start, which is what a register-style chip expects (write the register number, read its bytes). It is the only `unsafe` that adapter needs (AGENTS.md invariant 10: `unsafe` only in `*-sys` crates).

[`messages`] builds the `struct i2c_msg` headers of a transfer (address, `I2C_M_RD` flag, length) without touching the kernel, so the layout and the checks are tested on any machine: an address above `0x7F` or a buffer longer than `u16::MAX` is refused before the ioctl, and an empty transfer sends nothing.

Off Linux (macOS, the BSDs) the crate builds without `libc` and [`transfer`] fails with `std::io::ErrorKind::Unsupported`, so the workspace builds and tests on a development machine and an I²C device there is refused at its first transfer, never half-working.
