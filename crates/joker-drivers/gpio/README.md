# joker-driver-gpio

Digital lines of a Linux `gpiochip` (chardev v1 ABI), tier N (docs/05 §10). v1, not v2: the v2 ABI needs Linux 5.10+, newer than the bench board's kernel (4.19).

Address keys (ref/03 §2.2.5): `line` (integer), `active_low` (bool),
`bias` (`as-is` | `pull-up` | `pull-down` | `disabled`), `drive`
(`push-pull` | `open-drain` | `open-source`). `bias`/`drive` are recorded
for the host's real line configuration; this crate's own logic only
applies `active_low` (invert on read and on write).

Not yet supported: named lines (only integer `line` indices), `edges`
(interrupt-driven push reads), `timestamp` and `debounce`. A point
declaring any of these is a manifest error, not a silent no-op.

Device config: `chip` (string, the `gpiochip` name, e.g. `gpiochip0`),
used by the host to resolve the real `Link::Gpio`; this crate only
validates its presence and shape.

The real Linux chardev v1 adapter (`joker_hal::std_host::LinuxGpio`,
backed by `joker-gpio-sys`'s ioctls: `GPIO_GET_LINEHANDLE_IOCTL`,
`GPIOHANDLE_GET_LINE_VALUES_IOCTL`, `GPIOHANDLE_SET_LINE_VALUES_IOCTL`)
opens `/dev/gpiochipN` and requests one handle per granted line, each
with its own fixed direction (v1 fixes input/output per handle request).
`Link::Gpio` and this driver are also verified against an in-memory
double (`joker_hal::testing::MemoryGpio`), the same way `Link::Tcp`
shipped before `TokioTcp`.
