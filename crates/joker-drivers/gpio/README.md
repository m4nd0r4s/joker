# joker-driver-gpio

Digital lines of a Linux `gpiochip` (chardev v2 ABI), tier N (docs/05 §10).

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

The real Linux chardev adapter (opening `/dev/gpiochipN` and issuing the
v2 ioctl) is not implemented here yet — `Link::Gpio` and this driver are
verified against an in-memory double (`joker_hal::GpioLines`), the same
way `Link::Tcp` shipped before `TokioTcp`.
