# joker-driver-revpi

The `revpi` driver (tier N, ref/01 appendix A, ADR 0078): the I/O modules of a Revolution Pi (DIO, DI, DO, AIO, MIO and the gateways) through the process image of the Kunbus `piControl` kernel module, `/dev/piControl0`, granted as `Link::Paths`.

- Each signal is a PiCtory variable, named by the address key `var` (the signal's name by default). At `connect` the driver opens the device, asks piControl where each variable is (`KB_FIND_VARIABLE`) and which modules are present (`KB_GET_DEVICE_INFO_LIST`).
- `bool` signals are variables of one bit; `i64` and `u64` signals are variables of 8, 16 or 32 bits, sign- or zero-extended (the process image is little-endian). A variable piControl does not know, or whose length does not fit the type, reads `bad(config_error)`.
- `read` reads each variable from the process image (`pread`); a variable of a module that is not present reads `bad(not_connected)`. When piControl stops answering or its module list changes (a reset after a new PiCtory configuration), the batch is `not-connected`, and the next `connect` resolves every variable again.
- `write` sets a bit in the kernel (`KB_SET_VALUE`, so the other bits of the byte are never rewritten) or writes the bytes of a wider variable (`pwrite`); a value wider than the variable, or a module that is not present, is `device-rejected`.

`image::PiImage` is the seam between the driver and piControl: `image::PiControl` is the real one (Linux; elsewhere every call is `Unsupported`), and `image::FakeImage` is an in-memory process image with variables and modules, for tests and for running a manifest without the hardware.
