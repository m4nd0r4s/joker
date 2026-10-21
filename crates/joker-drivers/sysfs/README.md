# joker-driver-sysfs

The `sysfs` driver (tier N, ref/01 appendix A, docs/05 §10): the LEDs of `/sys/class/leds` (`led = "ACT"`, `bool`) and the thermal zones of `/sys/class/thermal` (`thermal = "thermal_zone0"`, `f64` in °C, read in m°C) of any Linux board. It is the `pi` device of the bench (docs/23 §6.2).

The driver only touches the paths the host grants (`Link::Paths`): `/sys` on a node, any directory in tests.

- Opening a LED as an output saves its `trigger` and sets it to `none`; `disconnect` restores it. An abrupt stop leaves `none` and the last `brightness`, which is why what the LED does with `jokerd` stopped is the board's `[io]` `fallback` (ref/03 §2.3.2), and not this driver's: `sysfs` has no device fallback.
- A write sets `max_brightness` or 0 and reads `brightness` back; a mismatch is `device-rejected`.
- A LED that cannot be opened (no `max_brightness`, or no write access to `trigger`, the udev rule of docs/05 §10) reads `bad(config_error)` and refuses writes; the link reports `degraded`.
