# joker-serial-sys

The modem control lines of a serial port (`TIOCMBIS`, `TIOCMBIC`), for the `dtr` and `rts` keys of a `serial` bus (ref/01 §5.6). It is the only `unsafe` the serial transport of `joker-hal` needs (AGENTS.md invariant 10: `unsafe` only in `*-sys` crates).
