# joker-driver-gpio

Digital lines of a Linux `gpiochip` (chardev v1 ABI), tier N (docs/05 §10). v1, not v2: the v2 ABI needs Linux 5.10+, newer than the bench board's kernel (4.19).

Address keys (ref/03 §2.2.5): `line` (integer), `active_low` (bool),
`bias` (`as-is` | `pull-up` | `pull-down` | `disabled`), `drive`
(`push-pull` | `open-drain` | `open-source`, outputs only: `drive` on an
input is `J0853`). `Address::line_config` hands `bias` and `drive` to the
host, which requests them from the kernel (bias needs Linux 5.5+; an older
kernel refuses the request and the device does not open). This crate's
own logic applies `active_low` (invert on read and on write).

Safe state at open (AGENTS.md invariant 5): the host requests every
output already at its declared `safe`, resolved against `active_low`, so
the line never starts at a level the manifest did not declare. An output
with `safe = "relinquish"` is refused: a line has no device logic to hand
control back to.

Access by address (`raw-access`, ADR 0073): the steps of a profile's
actions read any line one of the device's points requests and write a
line one of its outputs requests, as `bool`, with the step address's own
`active_low`. Any other line is `config-error`: the host only requests
the lines the points declare.

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
with its own fixed direction, initial output level, bias and drive (v1
fixes them per handle request).
`Link::Gpio` and this driver are also verified against an in-memory
double (`joker_hal::testing::MemoryGpio`), the same way `Link::Tcp`
shipped before `TokioTcp`.
