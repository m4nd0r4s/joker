# esp32-modbus: ESP32 bench firmware (RTU only)

The Phase 1 bench firmware of `docs/23` §4 for the original ESP32 development board. It turns the board into a plain Modbus device so that the Raspberry Pi 3 (`lab` node) has real hardware for the `modbus-rtu` driver, communication loss and the device fallback (M1).

This M0 build serves **Modbus RTU only**. Modbus TCP over Wi-Fi (the second transport of `docs/23` §4) is not built yet, so the RSSI register always reads 0.

## What it does

- Modbus RTU slave, unit 1, on `UART0` through the board's USB bridge (CP2102 or CH340): 115 200 baud, 8N1.
- At boot, before serving Modbus, it writes one line on the same port:

  ```text
  bench-esp32 v1 · wifi off · modbus-rtu uart0 115200 8N1
  ```

- **Device fallback:** if no valid request arrives for 2 s, the LED goes off. A valid request is any frame with a correct CRC addressed to unit 1 or broadcast (unit 0), including requests answered with an exception. Frames for other units or with a bad CRC do not keep the timer alive.
- On a panic the chip resets; after a reset GPIO2 is pulled down, so the LED is off.

## Register table

| Register | FC | Type | Signal |
|---|---|---|---|
| coil 0 | 1 (read), 5 (write) | `bool` | board LED (GPIO2) |
| discrete input 0 | 2 | `bool` | `BOOT` button (GPIO0), already inverted: 1 = pressed |
| input register 0 | 4 | `u16` | raw touch counter of T0 (GPIO4); it drops when the pin is touched |
| input register 1 | 4 | `i16` | Wi-Fi RSSI in dBm; always 0 in this build |
| input registers 2–3 | 4 | `u32` | seconds since boot, high word first |
| holding register 0 | 3 | `u16` | bench firmware version (1) |

Any other function code is answered with exception 1 (illegal function). An address range outside the table is exception 2; a zero or too large quantity, a coil value other than `0xFF00`/`0x0000`, or a malformed PDU is exception 3.

## Layout

- `logic/`: the `bench-esp32-logic` crate. `no_std`, no dependencies, no I/O: register table, PDU server, RTU framing (CRC-16 and the 1750 µs inter-frame silence) and the fallback timer, all driven by a microsecond clock passed in by the caller. It is tested on the host.
- `src/main.rs`: the firmware. It samples the pins, moves bytes between `UART0` and the logic crate, and drives the LED.

This directory is its own Cargo workspace, excluded from the root workspace, with its own `Cargo.lock` and its own `rust-toolchain.toml` (`docs/13` §5.1).

## Pinned versions

| Item | Version |
|---|---|
| `espup` | 0.18.0 |
| Xtensa Rust toolchain (`esp` channel) | 1.97.0.0 (`rustc 1.97.0-nightly (8ea53bcd7 2026-07-08)`) |
| Xtensa GCC (linker) | `esp-16.2.0_20260914` |
| Xtensa LLVM | `esp-21.1.3_20260408` |
| `espflash` | 4.6.0 |
| `esp-hal` | `=1.2.2`, with `unstable` (needed for the touch sensor) |
| `esp-bootloader-esp-idf` | `=0.6.0` |

`esp-hal` `unstable` APIs can change in any release (`docs/25` §6, E12); every transitive version is fixed in `Cargo.lock`, which is committed.

## Toolchain setup

```sh
cargo install espup --version 0.18.0 --locked
espup install --targets esp32 --toolchain-version 1.97.0.0
cargo install espflash --version 4.6.0 --locked
. ~/export-esp.sh
```

`~/export-esp.sh` puts the Xtensa linker on `PATH`; source it in every new shell before building.

## Build

```sh
cd tools/bench/esp32-modbus
cargo build --release
```

The ELF is `target/xtensa-esp32-none-elf/release/esp32-modbus`. `rust-toolchain.toml` selects the `esp` channel, and `.cargo/config.toml` selects the `xtensa-esp32-none-elf` target and builds `core` from source.

## Flash

With the board on the laptop's USB:

```sh
cd tools/bench/esp32-modbus
espflash flash --chip esp32 --monitor target/xtensa-esp32-none-elf/release/esp32-modbus
```

`cargo run --release` does the same (the runner is set in `.cargo/config.toml`). The monitor shows the boot banner right after flashing. Leave the monitor before a Modbus master opens the port.

To read the banner later on the Pi 3, stop `jokerd` first (it is the same port):

```sh
sudo systemctl stop jokerd
picocom -b 115200 /dev/serial/by-id/usb-Silicon_Labs_CP2102_USB_to_UART_Bridge_Controller_0001-if00-port0
```

## Check on the bench

```sh
jk profile test local/bench-esp32@1 --node lab \
   --uri "modbus-rtu:/dev/serial/by-id/usb-Silicon_Labs_CP2102_USB_to_UART_Bridge_Controller_0001-if00-port0?baud=115200&unit=1"
```

Write coil 0 to 1 and the LED turns on; stop polling and it turns off 2 s later.

## Host tests

The logic crate runs on the root toolchain. From the repository root (so that the root `rust-toolchain.toml` and no Xtensa target apply):

```sh
cargo test --manifest-path tools/bench/esp32-modbus/logic/Cargo.toml
cargo clippy --manifest-path tools/bench/esp32-modbus/logic/Cargo.toml --all-targets -- -D warnings
```

CI runs both, plus `cargo fmt --check` on this workspace.
