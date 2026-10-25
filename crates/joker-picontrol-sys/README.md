# joker-picontrol-sys

The three `piControl` ioctls (`<piControl.h>` of the Kunbus kernel module, the driver behind `/dev/piControl0` on a Revolution Pi) that the `revpi` driver needs, and the only `unsafe` it needs (AGENTS.md invariant 10, ADR 0078):

- `KB_FIND_VARIABLE`: the address, bit and length of a PiCtory variable, by name, in the configuration piControl has loaded;
- `KB_GET_DEVICE_INFO_LIST`: the modules piControl knows, with their process-image ranges and whether each is present on the bus;
- `KB_SET_VALUE`: one bit of the process image, set or cleared by the kernel, so a bit write never rewrites the other bits of its byte.

The process image itself is read and written with `pread`/`pwrite` on the same descriptor (`std::os::unix::fs::FileExt`), which needs no `unsafe`.

The records follow the C declarations of `piControl.h` with their natural alignment (`#[repr(C)]`): `SPIVariable` is 38 bytes, `SPIValue` 4, `SDeviceInfo` 64. The request numbers are `_IO('K', n)`, as the header defines them. The tests check the sizes and the pure encoding; the ioctls themselves need a Revolution Pi.

Off Linux the crate still builds, without `libc`, and every call fails with `std::io::ErrorKind::Unsupported`.
