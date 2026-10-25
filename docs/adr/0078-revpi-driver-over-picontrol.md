# ADR 0078 — The `revpi` driver over piControl

**Status:** accepted (v1) · **Date:** 2026-10

## Context

The Revolution Pi is in the README's target list and `boards/revpi-connect-5.board.toml` exists, but `revpi` is a reserved 1.0.x driver (ref/01 §15, appendix A) with no implementation (AUR-402). Its I/O modules (DIO, DI, DO, AIO, MIO, gateways) are not separate devices on a bus: the Kunbus kernel module `piControl` exchanges them with a process image of 4096 bytes, `/dev/piControl0`, and PiCtory names each byte or bit of it as a variable. Appendix A gives the driver one device key, `config` (the PiCtory `.rsc` file), and says "the signals are the PiCtory variables, with the same name", which leaves open:

1. **Where a variable is.** The `.rsc` describes the configuration PiCtory saved; piControl holds the one it loaded, and they differ between a save and a reset. piControl answers `KB_FIND_VARIABLE` with the address, bit and length it uses.
2. **Names.** PiCtory names (`I_1`, `InputValue_1`) are not signal names (ref/03 §2.1.2).
3. **Types.** A variable is 1, 8, 16 or 32 bits; ref/06 §2.7 only gives `revpi` bits as `boolean`.
4. **Absence and restarts.** A module can leave the bus, and a new PiCtory configuration resets piControl and can move every module in the image.
5. **Bits that share a byte.** Writing a byte to change one bit would overwrite the other outputs of that byte.

## Decision

1. **Transport.** The driver gets `Link::Paths` granting `piControl0` under `/dev` (ref/06 §3.4) and opens it at `connect`. The three ioctls it needs (`KB_FIND_VARIABLE`, `KB_GET_DEVICE_INFO_LIST`, `KB_SET_VALUE`) are in `joker-picontrol-sys`, the only `unsafe` (AGENTS.md invariant 10). The image itself is read and written with `pread`/`pwrite`. Off Linux every call is `Unsupported`.
2. **Signals.** A `revpi` device has no profile and no device keys. Each signal is declared in `[devices.<d>.signals]` with `type` and the address key `var`: the PiCtory variable name, 1–31 bytes, by default the signal's own name. `config` is not read: variables are resolved in the configuration piControl has loaded, at each `connect`.
3. **Types.** `bool` is a variable of one bit (`boolean`); `i64` and `u64` are variables of 8, 16 or 32 bits, read little-endian and sign- or zero-extended (`int`, `uint`). Another type is refused when the device is planned. A variable piControl does not know, or whose length does not fit the type, reads `bad(config_error)` and its writes are `unsupported`; `connect` logs it.
4. **Writes.** A `bool` is set or cleared by `KB_SET_VALUE`, in the kernel, so the other bits of its byte are never rewritten. A wider variable is written as its bytes. A value wider than the variable is `device-rejected`; the manifest's `range` is what keeps it from reaching the driver.
5. **Absent modules.** Each read and write asks piControl for its module list. A variable of a module that is not active on the bus reads `bad(not_connected)` and its writes are `device-rejected`; the others are unaffected.
6. **Restarts.** When piControl stops answering, or its module list changes (other modules, types or ranges: a reset with a new configuration), the batch is `not-connected`; the link goes down and the next `connect` reopens the device and resolves every variable again, so a moved module is never read at its old address.
7. Profiles of the `revpi` family (`[profile] driver = "revpi"`) stay reserved (`J0807`, ref/03 §2.2.2): this runtime only has the declared form.

## Consequences

- ref/01 appendix A and §15 give the `revpi` row its keys and mark it implemented; ref/06 §2.7 gives its types; ref/03 §2.2.5 gives `var` its default.
- New crates `joker-picontrol-sys` and `joker-driver-revpi` (with `image::FakeImage`, an in-memory process image with variables and modules); `joker_hal::link::DevicePaths::open_rw` and `joker_hal::testing::Bench::with_driver`; jokerd knows `revpi` in its catalog, shapes its points and starts it in `RealDevices`.
- The struct layouts of `piControl.h` (`SPIVariable` 38 bytes, `SPIValue` 4, `SDeviceInfo` 64, natural alignment) and the ioctls are checked only on a Revolution Pi; the tests run against the fake image.
