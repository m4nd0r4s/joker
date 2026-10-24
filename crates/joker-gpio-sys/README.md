# joker-gpio-sys

The GPIO character-device ioctls (`GPIO_GET_LINEHANDLE_IOCTL`, `GPIOHANDLE_GET_LINE_VALUES_IOCTL`, `GPIOHANDLE_SET_LINE_VALUES_IOCTL`) of the chardev **v1** ABI (`<linux/gpio.h>`, kernel 4.19+), for the `gpio` driver's real Linux adapter. It is the only `unsafe` that adapter needs (AGENTS.md invariant 10: `unsafe` only in `*-sys` crates).

v1, not v2: the chardev v2 ABI (`GPIO_V2_*`) needs Linux 5.10+; the bench board (a Raspberry Pi 3 B+ on Raspbian Stretch) runs 4.19, which only has v1. v1 lacks v2's atomic multi-line get/set and in-kernel debounce, but covers one independent line at a time, which is all `joker-driver-gpio` asks of [`GpioLines`](https://docs.rs/joker-hal) today.

Off Linux (macOS, the BSDs) the crate still builds, without `libc`, and every call fails with `std::io::ErrorKind::Unsupported`, so the workspace builds and tests on a development machine and a GPIO device there is refused at open, never half-working.
