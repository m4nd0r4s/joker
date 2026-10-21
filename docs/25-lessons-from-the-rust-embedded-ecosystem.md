# 25 — Lessons from the Rust embedded ecosystem

The question that gave rise to this document: *"In the Rust embedded world we already have HALs, BSPs, PACs. We can learn from their abstraction what makes sense and what does not. I know that what we offer is more (it is a runtime), but we can learn from what the ecosystem is doing today."*

It is analysis, like documents 11, 21 and 24. The proposals of §6 only become normative when they enter a reference, with an ADR where applicable. The state of the ecosystem is that of **September 2026**, confirmed in the sources at the end; versions change fast, and the table of §0 should be reviewed at each phase.

---

## 0. The state of the ecosystem, in September 2026

| Piece | Version or status | What matters to Joker |
|---|---|---|
| `embedded-hal` | 1.0 (Jan. 2024), no 2.0 planned | stable traits: `digital`, `i2c`, `spi`, `delay`, `pwm::SetDutyCycle`. **No ADC**, no `IoPin`: they were removed before 1.0 |
| `embedded-hal-async` | 1.0, Rust ≥ 1.75 | async versions of I²C, SPI, delay and pin waiting; without allocation or `dyn` |
| `embedded-hal-bus` | 1.0 | sharing of I²C and SPI buses between drivers (`I2cDevice`, `SpiDevice`) |
| `embedded-io`, `embedded-io-async` | 0.7 (MSRV 1.81) | `Read`/`Write` for serial and sockets; **still 0.x** |
| `embedded-can` | outside `embedded-hal`; no standardized async trait | each HAL has its own async CAN API |
| `embedded-storage` | 0.x | `NorFlash`, already used by persistence (ref/08 §2.11.3) |
| Embassy | `embassy-executor` 0.10 (Mar. 2026), `embassy-sync` 0.8, `embassy-net` 0.9, `embassy-stm32` 0.6, `embassy-boot-stm32` 0.8 | all **0.x**, with frequent breakage; runs on stable Rust |
| `stm32-metapac` | generated from `stm32-data` with `chiptool` | exports `METADATA` (pins, peripherals, memory) for `build.rs` and tools |
| `embassy-stm32` | 0.x, **1,567 features**, one per chip | the extreme example of "features per chip" |
| `embassy-rp` | RP2040 and RP235x (`rp235xa`, `rp235xb`) | the HAL of the M1 reference target |
| `rp-hal` (`rp235x-hal`) | 0.x, "volatile until 1.0" | alternative without Embassy |
| `esp-hal` | **1.0 in Oct. 2025**, 1.2.2 in Sep. 2026 | 1.0 only stabilized `init`, GPIO, UART, SPI, I²C (blocking and async), `time` and reset; **ADC, LEDC, RMT, touch, watchdog and the rest stayed behind the `unstable` feature**, to be stabilized bit by bit (confirm the status in 1.2 before Phase 2) |
| `esp-radio` (formerly `esp-wifi`) | 1.0.0-beta.1, on `esp-hal` 1.2 and `esp-rtos` 0.4 | Wi-Fi, BLE, ESP-NOW, 802.15.4; requires `esp-rtos` (preemptive scheduler) and `esp-alloc` (heap) |
| `esp-rtos` | 0.4 | replaces `esp-hal-embassy`: thread and interrupt Embassy executors |
| Xtensa (ESP32, ESP32-S3) | **outside official LLVM and Rust**; `esp` toolchain through `espup` (LLVM 22 since Xtensa Rust 1.99) | the bench ESP32 depends on a third-party toolchain |
| RISC-V (ESP32-C6) | official Rust (`riscv32imac-unknown-none-elf`) | no third-party toolchain |
| `probe-rs` | 0.32 (Jul. 2026), library and CLI | ARM, RISC-V and Xtensa over JTAG (the original ESP32 needs an external adapter) |
| `defmt` | 1.0 (2025), SemVer; **no MSRV guarantee** | logs with deferred formatting, decoded on the host from the ELF |
| `critical-section` | 1.x | "pluggable" critical section: the HAL or the integrator chooses the implementation |
| `portable-atomic` | 1.x | atomics where the hardware does not have them (Cortex-M0+, 64-bit on 32-bit MCUs) |
| `heapless` | 0.9.x | fixed-capacity collections (already used: `Text`, `Bytes`) |
| `svd2rust` | 0.37 | generates PACs from SVD |
| `device-driver` | active (book v1), DSL or manifest (TOML, YAML, JSON) | generates register drivers for I²C and SPI chips |
| RTIC | 2.x, async, `rtic-monotonics` | scheduling by hardware priorities, checked at compile time |
| Tock OS | 2.2 (Jan. 2025), stable Rust | processes isolated by MPU, *capsules* in safe Rust, process credentials |
| Hubris (Oxide) | in production | static tasks declared in `app.toml`, `jefe` supervisor, `Idol` IDL |
| Zephyr | 4.4 (4.3, 3.7 LTS) | devicetree + YAML bindings + Kconfig + CMake |

---

## 1. The map of the layers, side by side

The classic Rust embedded stack has four floors: the **PAC** (register access, generated from SVD), the **HAL** (drivers for the chip's peripherals, which implement the `embedded-hal` traits), the **BSP** (the board: which pin is the LED, which UART goes to the header) and the **driver crates** (external chips, generic over the traits). On top, an executor (Embassy, RTIC) or a system (Tock, Hubris, Zephyr).

| Ecosystem | Joker | Essential difference |
|---|---|---|
| **PAC** (`svd2rust`, `chiptool`, `stm32-metapac`) | **nothing**. Joker never touches the chip's registers | Joker consumes the PAC only through the HAL. What Joker has as a "PAC" is the **equipment profile**: the register map of a piece of field equipment (Modbus, SunSpec). SVD is to a chip what `*.profile.toml` is to an inverter |
| **PAC metadata** (`stm32_metapac::METADATA`, data from `stm32-data`) | the hardware keys of `*.board.toml` (`[pins]`, `[uart.*]`, `[areas]`, `memory`, `flash`) | the ecosystem **generates** this data from the manufacturer's datasheets; Joker writes it by hand (ref/03 §2.3.7) |
| **HAL** (`embassy-rp`, `embassy-stm32`, `esp-hal`) | the `chip-*` modules of `joker-micro` and the `Link` transports (ref/06 §3.4) | Joker **does not write HALs**: it uses them. The MCU tier N drivers (`gpio`, `i2c`, `uart`…) are thin adapters on top |
| **`embedded-hal`/`-async`/`embedded-io` traits** | the contract of the `Link` transports | this is where Joker inherits the ecosystem: a driver that only speaks through these traits runs on Linux, on the MCU and on the doubles |
| **BSP** (crate per board) | **board profile** (`*.board.toml`) | a BSP is compiled code; a board profile is data read by `jk check` and by `jk build`. Joker does not need a crate per board |
| **driver crates** (`bme280`, `ssd1306`, those generated by `device-driver`) | **tier N drivers** + **equipment profiles** | a driver crate gives methods (`read_temperature()`); Joker gives **signals** with unit, quality and time. The bridge between the two is missing (§2.7) |
| **`embedded-hal-bus`** | `Link::I2c` "restricted to the instance's addresses", the bus scheduler | the same idea, with more guarantees: in Joker, the bus belongs to the core, not to whoever shares it |
| **typestate** on pins and peripherals | **validation in `jk check`** (`J0829`: pin in two peripherals) and the `Parts` of ref/08 §2.11.3 | the ecosystem proves at compile time; Joker proves before flashing the generation, because the configuration arrives at runtime (§3.1) |
| **executor** (Embassy, RTIC) | cycle groups on interrupt executors (ref/08 §2.11.6) | the executor is a detail of the host; the core does not depend on it |
| **`embassy-boot`**, MCUboot | A/B firmware OTA (ref/08 §2.12.4) | already adopted |
| **`defmt`** | `defmt` feature of `joker-micro`, trace ring | half adopted: only the feature (§2.5) |
| **`probe-rs`** | `jk flash`, `jk bench --target mcu` | already adopted as a tool |
| **devicetree** (Zephyr) | `*.board.toml` | the same role; Joker uses a single format and a single validator |
| **YAML bindings** (Zephyr) | driver descriptor schemas (ref/06 §2.3.1) | the same idea: the schema comes with the driver and validates the configuration |
| **Kconfig** (Zephyr) | Cargo features chosen by `jk build` from the manifest, and `[capacities]` | the Joker user never chooses features by hand (§3.3) |
| **`app.toml`** (Hubris) | compiled generation + capabilities of the generic firmware | everything static, everything known at build time; Joker adds the generation in flash |
| **process** (Tock), **task** (Hubris) | native app (v1) and `wasmi` app (1.4) | Tock and Hubris isolate by MPU; Joker Micro v1 does not isolate (a *panic* is `jk_fatal`) |

**Reading the map.** From the HAL down, the ecosystem solves the problem better than Joker ever would, and Joker should **inherit** that part. From the driver crate up, the ecosystem stops at the "method that returns a number", and that is where Joker begins (§4).

---

## 2. What to adopt

Each item says what changes in the specifications and how much it costs. The detail of the changes is in the table of §6.

### 2.1 The MCU drivers sit on `embedded-hal` 1.0, with an honest map of the gaps

**It is already decided** (19 §3, ref/06 §3.4 and §3.7): the `Link` transports implement `embedded-io-async` and `embedded-hal-async`, and the `micro = true` drivers only speak through them. It is the right decision: it inherits `embassy-rp`, `embassy-stm32` and `esp-hal` without writing a HAL.

**What is missing is to say where `embedded-hal` does not reach.** Document 19 §3 says "GPIO, ADC, PWM, I²C, SPI, UART, Modbus RTU, CAN, 1-Wire drivers, on top of `embedded-hal` 1.0". That is not true for all of them:

| MCU driver | Ecosystem trait | Status | What Joker does |
|---|---|---|---|
| `gpio` | `embedded_hal::digital::{InputPin, OutputPin}` + `embedded_hal_async::digital::Wait` | stable | direct |
| `i2c`, `spi` | `embedded_hal_async::{i2c::I2c, spi::SpiDevice}` | stable | direct; sharing with `embedded-hal-bus` |
| `uart`, `modbus-rtu`, serial link | `embedded_io_async::{Read, Write}` | **0.7, not stable** | direct, with the version pinned |
| `pwm` | `embedded_hal::pwm::SetDutyCycle` | stable, *duty* only | frequency and phase stay in the `chip-*` module |
| `adc` | **none** (removed before 1.0) | — | own trait in `joker-hal` (`AdcRead`), implemented in each `chip-*` module |
| `can` | `embedded-can` *blocking*/`nb` only | no standardized async | own trait (`CanFrames`), implemented in each `chip-*` on top of the HAL's async API |
| `onewire` | **none** | — | on an *open-drain* pin (`Flex`) with HAL timing, or through UART |
| `esp32` (touch, RSSI) | specific to `esp-hal` | touch behind `unstable` | only in `chip-esp32*` |

**Changes:** ref/06 §3.7 and ref/08 §2.11.2 gain this table; `joker-hal` defines the two missing traits (`AdcRead`, `CanFrames`), with the same shape as those of `embedded-hal-async`, so that they can be dropped when the ecosystem standardizes them.
**Cost:** low on paper. In the implementation, each `chip-*` writes two adapters (ADC and CAN) per family.

### 2.2 `critical-section` and `portable-atomic` in the shared core

The `Io` of ref/08 §2.11.3 is `Copy + Send + Sync`, and the `[any]` functions can be called from any context, including interrupts. That needs synchronization. And samples carry `seq: u64`, but none of the M1 and M2 targets has 64-bit atomics (Cortex-M33, Xtensa LX6/LX7 and RISC-V `imac` are 32-bit).

The ecosystem solved this with two crates that all HALs already use:

- **`critical-section`**: the core asks for a critical section, and whoever implements it is the HAL (Embassy, `esp-hal`) or, in library mode, the integrator (FreeRTOS, Zephyr). It is the *de facto* standard, and it avoids Joker inventing its own `jk_platform_t.lock`.
- **`portable-atomic`**: gives `AtomicU64` where the hardware does not have it, with the critical section underneath. The *seqlocks* of the slots (01 §3.2) stay the same on Linux and on the MCU.

**Changes:** AGENTS.md and ADR 0020 (core invariant: synchronization only through `critical-section` and `portable-atomic`, never through executor primitives); ref/08 §2.4 (the C ABI documents that the integrator links a `critical-section` implementation, or enables the `cs-single-core` feature when there is only one core and no interrupt calls the node); ref/08 §2.11.1 (core dependencies).
**Cost:** low. It is one more stable dependency, and it saves a custom design.

### 2.3 Features per chip, with the exact name of the chip on the board

The ecosystem chooses the chip by Cargo feature, mutually exclusive: `esp-hal` with `esp32`, `esp32c6`, `esp32s3`; `embassy-rp` with `rp2040`, `rp235xa`, `rp235xb`; `embassy-stm32` with one feature **per part number** (`stm32g474re`, `stm32h563zi`: 1,567 features). Ref/08 §2.11.2 has `chip-stm32g4`, `chip-stm32h5`… at the family level, and that is **not enough** for `embassy-stm32`, which needs the full part number to know the memory, the pins and the interrupts.

There is also a gap: the original ESP32 of the bench (document 23) has no `chip-*` feature.

**Changes:**
- ref/03 §2.3.1: new key `chip` in MCU boards, with the name of the chip **as the HAL writes it** (`"rp235xa"`, `"stm32g474re"`, `"esp32"`), distinct from `[probe] chip` (which is the `probe-rs` name);
- ref/08 §2.11.2: the `chip-*` features become per family (`chip-rp`, `chip-stm32`, `chip-esp`), and `jk build` passes the exact part number to the HAL from `[board] chip`; the original ESP32 is added; exactly one family per build, with `compile_error!` with zero or more than one;
- ref/08 §2.15.1: a column with the HAL and the pinned version of each target.

**Cost:** low. The user still does not see features: they choose a board.

### 2.4 One owner for each pin, without `steal`, in the generic firmware

The ownership model of the ecosystem is: the HAL's `init` returns the peripherals **once**, as *singletons*, and the code chooses the pins **at compile time** (`p.PIN_25`). Joker's generic firmware (19 §4.1) only knows which pin is the LED when it reads the generation blob, **at runtime**. The HALs have a way out for that (`AnyPin::steal(n)`), but it is `unsafe` and bypasses ownership.

The safe way, which the HALs already allow: at boot, the `chip-*` module receives the peripherals from `init`, **converts every pin declared in `[pins]` into an erased type** (`degrade()` to `AnyPin` in `esp-hal`, `Peri<AnyPin>` in `embassy-rp`, `AnyPin` in `embassy-stm32`) and keeps them in a table indexed by the board name. Each driver takes its pin from the table, once. `J0829` (pin in two peripherals) already guarantees in `jk check` that there are no two owners; the table guarantees it again at runtime (a second request is `JK_E_GEN_FIRMWARE`).

**Changes:** ref/08 §2.12.3, step 6, and a new paragraph in §2.11 ("pin ownership"); the rule "no `unsafe` outside the `ffi` module" of ref/08 §2.11.3 now also applies to the `chip-*` modules.
**Cost:** low. It is the *type erasure* pattern that `esp-hal` itself recommends for "arrays of pins".

### 2.5 `defmt` as the Joker Micro log, in the field too

`defmt` 1.0 is stable and is the standard for logs on MCUs: the strings stay in the ELF, and the MCU only sends an index and the arguments. Ref/08 already has the `defmt` feature, but only "logs with `defmt`" through a probe.

The lesson that matters most: the `defmt` format also works **outside the probe**. The frames can travel over Joker Link (in a log message, or inside `TRACE`) and be decoded on the Linux node or in the Deck, as long as the exact ELF is available. `probe-rs` already does this over RTT.

**Changes:** ref/08 §2.14 (the `.jkfw` firmware descriptor carries the ELF's *build-id*); ref/07 (`defmt` frames as the payload of a log message, with ephemeral delivery); the Hub keeps the ELF of each published firmware; `jk logs --node <mcu>` decodes.
**Cost:** medium. Keeping the ELFs by *build-id*, and a new message in the link. `defmt` does not guarantee an MSRV, so the version is pinned (§5).

### 2.6 `probe-rs` as a library, and not only as a command

`probe-rs` is a Rust crate, not only a CLI. `jk flash`, `jk bench --target mcu` (ref/08 §2.12.5) and reading the trace ring through a probe (19 U9) can use it **linked into `jk`**, without depending on an installed executable of the right version. It is what Humility does for Hubris: a debugger that knows the format of the system and reads the state of the tasks through the probe. The node header "for probes" (ref/08 §2.9.2) already exists for this.

**Changes:** document 07 (`jk flash`, `jk trace --probe`); ref/08 §2.9.2 (the header must be findable by symbol in the ELF, as Humility finds the Hubris tables).
**Cost:** medium. `probe-rs` is large and 0.x; it goes into a `jk` feature (`probe`) and the version is pinned.

### 2.7 A bridge to the ecosystem's driver crates

There are hundreds of driver crates on top of `embedded-hal` (sensors, external ADCs, I/O expanders, displays) and `device-driver` generates more from a manifest. Today, a new I²C sensor in Joker requires a tier N driver written from scratch, or a profile with the generic `i2c` driver and the register map by hand.

The proposal: a **driver crate adapter** in `joker-hal`. The author writes a function that receives the `I2cDevice`/`SpiDevice` of the `Link`, builds the crate's driver, and declares which methods give which points (`read_temperature` → `temp`, `°C`). The core still takes care of the quality (an error from the crate is `bad(comm_failure)`), of time, of scale and of the rest.

```rust
// crates/joker-drivers/bme280/src/lib.rs (sketch)
joker_hal::crate_driver! {
    name: "bme280", bus: i2c, micro: true,
    open: |dev| bme280::Bme280::new(dev, bme280::Address::Primary),
    points: {
        "temp"     => |d| d.read_temperature().await, unit = "°C",
        "pressure" => |d| d.read_pressure().await,    unit = "Pa",
    },
}
```

**Changes:** ref/06 §3 (new section: driver crate adapter, with the cancellation and deadline rules of §3.5 applied to third-party code); ref/06 §6 (the conformance of the adapter uses `embedded-hal-mock` for transaction-based tests); document 24, P5 (day-1 catalog with the most common sensors this way).
**Cost:** medium. The risk is third-party code blocking the executor or not being *cancel-safe*; `J0883` (measurement of `poll`) catches the first, and the list of crates accepted in the Hub catches the second.

### 2.8 Generate board profiles from data, as `svd2rust` generates PACs

`stm32-data` extracts from SVDs and from ST files the description of each chip (pins, alternate functions, memory, interrupts), cleans it up by hand once, and **generates** `stm32-metapac`, with a `METADATA` designed for tools on the host. `esp-hal` also keeps a description per chip, used to generate its own code. Nobody writes the pin table of an STM32 by hand.

Ref/03 §2.3.7 says that an MCU board profile is written "by hand, from the datasheet". It is the work the ecosystem has already done.

**Changes:** ref/03 §2.3.7 gains `jk board new <name> --chip <part>`, which reads the HAL metadata (`stm32-metapac` with `metadata`, the `esp-hal` metadata, those of `embassy-rp`) and writes `memory`, `flash`, `[pins.*] functions`, `adc_channel`, `[uart.*]`, `[i2c.*]`, `[spi.*]`, `[can.*]` and `max_baud`, and marks with `# REVIEW` what belongs to the **board** and not to the chip (what is connected to each pin, `reset_safe`, `[areas]`, `[link] default`, the capacities).
**Cost:** medium. One reader per metadata source, with a pinned version; validation stays in `jk board check`.

### 2.9 `embassy-boot`, as it already is, with three notes

Ref/08 §2.12 already uses `embassy-boot` (RP2350, STM32) and the Espressif bootloader (ESP32). The research confirms the choice: `embassy-boot` does A/B swapping that is safe against power failures, trial boot with `mark_booted()` and reversion, and verifies Ed25519 signatures (`ed25519-salty`, the small option).

Three notes for the reference:

1. The `embassy-boot` partitions are called `ACTIVE`, `DFU` and `STATE`: they correspond to `firmware`, `fw_staging` and `boot_state` of ref/03 §2.3.3. It is worth a row in the table, for those coming from Embassy.
2. The "marks itself as good after N clean ticks and the `HELLO`" (ref/08 §2.12.4) is exactly `mark_booted()`; the reference should say that that is the call.
3. The RP2350 only boots an image with a valid `IMAGE_DEF` (`embassy-rp` has the options), and has secure boot in ROM. The `rp2350-pico2` profile should say which one is used, and whether the ROM verifies the bootloader.

**Cost:** low, text only.

### 2.10 The driver crate patterns that Joker already follows, and should write down

Some patterns of the ecosystem are already in the design, without a name. It is worth naming them in ref/06, so that those coming from there recognize them:

| Ecosystem pattern | In Joker |
|---|---|
| the driver is generic over the trait, and never opens the transport | "a tier N driver does not open sockets or serial ports: it uses the `Link`" (ref/06 §3.4) |
| `split()` of a peripheral into parts with separate ownership | `Parts { io, service, groups, changes }` (ref/08 §2.11.3) |
| non-generic Embassy tasks, generated by a macro | `joker_project::tasks` (ref/08 §2.11.4), because `#[embassy_executor::task]` rejects generics |
| `embedded-hal-mock` to test drivers without hardware | `joker-drvtest` doubles (ref/06 §6) |
| `#![no_std]` with `alloc` and `std` as features | `joker-hal`, `joker-core` (ADR 0020) |

**Cost:** none, beyond the text.

---

## 3. What not to adopt, and why

### 3.1 Typestate in the app API and in the configuration

Typestate proves at compile time that a pin is configured as an output before it is written. It is the best of Rust embedded when the program **is** the configuration. In Joker it is not:

- **the configuration arrives at runtime.** The same app runs at a hundred sites with a hundred maps (00, mechanism 4), and the generic firmware reads the generation from the blob (19 §4.1). A type cannot depend on a file that arrives after the build;
- **the app does not see pins.** It sees ports (`Output<bool>`). The pin belongs to the site manifest, and changes without recompiling the app;
- **the known cost of typestate is the explosion of types.** `esp-hal` itself erases pin types by default, and the Rust embedded book points out the difficulty of keeping different pins in a collection.

Joker does the same proof at another moment: **`jk check`**, before the generation reaches the node (`J0829`, `J0710`, `J0786`), and the crate's `Parts`, where typestate serves to separate contexts. That is the right place for it.

### 3.2 "Compile everything per board" as a distribution model

In the ecosystem, the board, the pins and the logic stay in one binary, and changing the configuration is recompiling. Document 19 §4.1 already rejected this for fleets: 3,000 sites would be 3,000 firmwares. Hubris does the same for good reasons (small and auditable images, everything static), but Hubris does not have to reconfigure three thousand pieces of equipment over OTA.

Joker keeps the good part (fixed capacities at build time, like Hubris's `app.toml`) and separates the configuration into a signed blob. The embedded generation still exists for closed products.

### 3.3 Kconfig, and features chosen by hand

Zephyr separates things well on paper ("devicetree describes the hardware, Kconfig chooses the software"), but pays for it in complexity: two languages, CMake on top, *overlays* per board, and its own documentation admits that Kconfig cannot configure parameters per instance and that the devicetree cannot be changed from Kconfig. The classic error (a node in the devicetree without the driver enabled in Kconfig, and `device_is_ready()` returning false) does not exist if there is only one source.

Joker does not adopt Kconfig nor expose features to the user. `jk build` derives the features from the manifest and the board (ref/08 §2.11.2), which is what Zephyr ended up doing with the `DT_HAS_<compatible>_ENABLED` symbols generated from the devicetree. Joker starts where Zephyr arrived.

### 3.4 The devicetree syntax

`phandles`, nested nodes, `compatible` as a free string and generated C macros give a steep learning curve and opaque compilation errors. Joker keeps a flat TOML with stable names and errors with a code (`J0828`, `J0829`), and keeps the good **ideas** (§3.7).

### 3.5 Register access in drivers and apps

The PAC is `unsafe`-adjacent and chip-specific. No tier N driver nor any Joker app should touch a PAC: if a peripheral has no HAL, the adapter goes into the `chip-*` module, reviewed once. It is Tock's rule for *capsules* (safe Rust, without `unsafe`), applied to drivers.

### 3.6 Embassy types in the core API

`joker-core` runs on Linux under tokio and on the MCU under Embassy, and in library mode under FreeRTOS or Zephyr. Nothing from Embassy (`Spawner`, `Channel`, `RawMutex`) enters the public types of the core. Ref/08 already does this well: `GroupRunner::tick` is synchronous, and Embassy stays in the `joker_micro::embassy` module. That also means that **RTIC** and a *bare-metal* loop work without changing the core; there is no need to adopt them, it is enough not to shut them out.

### 3.7 What Zephyr does well, and Joker already has or should have

| Zephyr idea | In Joker |
|---|---|
| bindings: the schema comes with the driver and validates the node | descriptor schemas (ref/06 §2.3) |
| `status = "okay"`: a node only exists if it is enabled | "the fixed IO only exists when it is referred to" (ref/03 §2.3.2) |
| `chosen`: which UART is the console | `[link] default` (ref/03 §2.3.3) |
| board revisions | board versions (ref/03 §2.3.6) |
| **shields**: expansion boards that join any board with the right header | **missing**. A relay or RS-485 HAT for Raspberry Pi, or an I/O board for the Pico, has to be copied into each board (proposal in §6) |

### 3.8 `embedded-hal-nb` and *blocking* APIs in drivers

The ecosystem keeps the three forms (*blocking*, `nb`, async). Joker chooses **async only** in drivers (ref/06 §3.3), and *blocking* only in `Storage` and in the service context. `nb` is not adopted.

### 3.9 MPU isolation in v1

Tock and Hubris show that it is possible to isolate tasks on an MCU with an MPU, and to restart only the one that failed (the *dispositions* of `jefe`). It is the right design for third-party apps. In v1, Joker Micro runs native apps of the project itself, and a *panic* is a reset to the hardware reset state (ref/08 §2.11.5). Adopting the MPU now would duplicate the work of the 1.4 `wasmi` (19 §5). It remains an alternative to evaluate in 1.4 (§6).

---

## 4. What the ecosystem does not solve, and Joker does

The ecosystem provides, and provides well: peripheral drivers, executors, bootloaders, flashing and logging tools. It stops at the boundary "the driver returned an `f32`". No project in the table of §0 answers these questions:

| Question | Rust embedded ecosystem | Joker |
|---|---|---|
| is this number still valid? | `Result<f32, E>`: either there is a value, or there is an error | **quality** per sample (`Good`, `Uncertain`, `Bad` with a reason), inherited from OPC UA, and the `?` that turns bad quality into an error (02) |
| in which unit is it? | `f32`, or a `uom` type chosen by whoever writes the driver | **unit on the signal and on the port**, conversion by the runtime, `J0107` on the wrong binding (00, mechanism 4) |
| from when is it? | the app reads the clock when it wants | `ts_acq`, `ts_hw`, `ts_src` and the **clock as an input** (ADR 0007) |
| what happens if the code dies? | watchdog and reset | **mandatory safe state**, leases, verified `reset_safe`, and the `safe` applied by the MCU when Linux dies (19 §6) |
| can this write reach the hardware? | if the code makes it, it does | **envelope** (range, rate, interlocks) with no path that bypasses it (ADR 0005) |
| who is in charge of this output? | whoever has the `&mut` of the pin | **arbiter** with priorities and leases (ADR 0011), with Linux as a writer equal to the others |
| can I reproduce yesterday's failure? | `defmt` and a debugger, live | **deterministic replay** of the trace ring, on the laptop, with the same core (19 U9) |
| can I change the configuration of 3,000 devices? | recompile and reflash each one | **signed generations in flash**, confirmation, anti-rollback, and the Deck (19 §4.1, 20) |
| is the new app safe to put in charge of actuating? | tests, if there are any | **shadow mode** and promotion criteria (ADR 0012) |
| is this equipment a pump or an LED? | the name of the crate | versioned **classes** with conformance (14, 17) |
| does the same logic run on a Pi and on an MCU? | only if it is written twice, or with care | **the same core, the same bytes** in conformance (ADR 0020) |

**The value proposition, in one sentence:** the Rust embedded ecosystem makes an MCU a programmable computer; Joker makes it a **manageable control node**, with the guarantees of a PLC and the operation of a fleet. The two add up, and that is why Joker should inherit the lower part instead of redoing it.

**And the other way round:** what Joker can give back to the ecosystem, when it exists, is `joker-hal` as an example of a driver contract portable between Linux, MCU and Wasm, the async ADC and CAN traits (§2.1) proposed to the HAL working group, and the driver crate adapter (§2.7) as a way of giving quality, unit and time to any existing crate.

---

## 5. Dependency risks

| Risk | Situation in September 2026 | Impact on Joker | Mitigation |
|---|---|---|---|
| **Unstable Embassy APIs** | all crates at 0.x; `embassy-executor` 0.10, `embassy-stm32` 0.6; breakage in almost every version | each update touches the `chip-*` modules and `joker_micro::embassy` | Embassy only appears in those modules (§3.6); versions pinned in the workspace per family; one update per phase, with conformance on hardware and in QEMU as the gate; the generated `joker-project` hides the tasks from the integrator |
| **`esp-hal` stable, but narrow** | in 1.0, only GPIO, UART, SPI, I²C and `time` are stable; ADC, LEDC, RMT, touch and watchdog behind `unstable` | the MCU `adc`, `pwm` and `esp32` drivers, and the Phase 1 bench firmware (touch), depend on unstable API | the `unstable` feature stays isolated in `chip-esp`; exact version (`=1.2.x`) in the workspace lock; a compilation test per driver in CI |
| **`esp-radio` in beta, with a heap** | 1.0.0-beta.1; requires `esp-rtos` (preemptive scheduler) and `esp-alloc` | the M2 standalone node (19 U2) and the link over UDP (doc 23, Phase 2) depend on it; the core is without `alloc`, but the firmware with Wi-Fi will have a heap | declare the exception: a fixed-size heap, only for the radio, outside the core and counted in `J0710`; measure the interference of the radio scheduler on the jitter of the groups; pin the beta |
| **Xtensa outside official LLVM** | `esp` toolchain through `espup`, following stable Rust with a delay; *upstream* LLVM is progressing but not done | the bench ESP32 and the ESP32-S3 (M2) depend on a third-party toolchain; the CI must install it (doc 23 §5.1) | the reference conformance runs on Cortex-M33 (QEMU and RP2350) and on RISC-V (ESP32-C6), with official toolchains; Xtensa is one more target, never the only one; `espup` with a pinned version in a CI image; the original ESP32 stays at the `dev` level (01 §22) |
| **Misaligned MSRV** | `esp-hal` only guarantees the latest stable at the date; `defmt` does not guarantee an MSRV; `embedded-io-async` 0.7 requires 1.81; the Xtensa toolchain follows its own calendar | a `cargo update` can break the build of one target and not of another | one `rust-toolchain.toml` per target family (official and `esp`), with the same Rust version whenever possible; Joker's MSRV is the pinned version, not a downstream guarantee; toolchain update as an explicit task of each phase |
| **`embedded-io` 0.x** | 0.7, still without 1.0 | the serial and TCP transports of the `Link` and the `link_stream` of ref/08 use it | a single version in the workspace; `embedded-io` types do not appear in the C ABI nor in the blob, only in the Rust crate |
| **missing traits** (ADC, async CAN) | no standardization in sight | adapters per family (§2.1) | own traits with the shape of `embedded-hal-async`, easy to swap when there is an official one |
| **`probe-rs` 0.x** | 0.32; recent Xtensa support | `jk flash`, `jk bench` and trace through a probe | `probe` feature of `jk`, pinned version; `espflash` remains as the path without a probe (doc 23) |
| **maintainers** | `esp-hal` has Espressif's backing; Embassy and `rp-hal` are community projects, with companies behind them; `device-driver` and `defmt` depend on few people | an abandoned crate blocks a target | only dependencies with more than one active maintainer on the tick path; the others in optional features; Joker contributes upstream when it fixes something |

**The rule that comes out of this:** the core (`joker-core`, `joker-expr`, `joker-units`, `joker-link-proto`) only depends on stable (1.x) or trivial crates (`heapless`, `critical-section`, `portable-atomic`, `minicbor`, `libm`; the allowlist of `policies/dependencies.md` §3, ADR 0055). Everything that is 0.x (Embassy, `embedded-io`, `esp-radio`, `probe-rs`) stays in the hosts and the tools, behind modules that can be updated without touching the core or the C ABI.

---

## 6. Proposed changes to the specifications

Priority: **high** = before Phase 2 (Joker Micro); **medium** = during Phase 2; **low** = when there is an occasion.

| # | Document / section | Change | Priority |
|---|---|---|---|
| E1 | ref/06 §3.7, ref/08 §2.11.2, 19 §3 | table of the MCU drivers with the trait of each and the gaps (ADC, async CAN, 1-Wire, touch); `AdcRead` and `CanFrames` traits in `joker-hal` (§2.1) | high |
| E2 | AGENTS.md, ADR 0020, ref/08 §2.4 and §2.11.1 | core synchronization only through `critical-section` and `portable-atomic` (`AtomicU64` for `seq` on 32-bit MCUs); the C ABI says that the integrator provides the `critical-section` implementation, or enables `cs-single-core` (§2.2) | high |
| E3 | ref/03 §2.3.1, ref/08 §2.11.2 and §2.15.1 | `[board] chip` key with the name of the chip as the HAL writes it; `chip-*` features per family, with exactly one per build; `chip-esp32` for the bench; "HAL and version" column in the targets (§2.3) | high |
| E4 | ref/08 §2.11 (new paragraph) and §2.12.3 step 6 | pin ownership in the generic firmware: table of erased pins built at boot from `[pins]`, without `steal`; no `unsafe` in the `chip-*` modules (§2.4) | high |
| E5 | 19 §3, ref/08 §2.11 and §2.15.2 | declared exception for the ESP32 radio: fixed heap (`esp-alloc`) and `esp-rtos` only in the firmware with Wi-Fi, outside the core, counted in `J0710`; measure the jitter with the radio active (§5) | high |
| E6 | 13 §1, 18 | toolchain policy: one `rust-toolchain.toml` per family (official, `esp`), pinned versions of the 0.x dependencies, update once per phase with conformance as the gate; Xtensa is never the only conformance target (§5) | high |
| E7 | ref/08 §2.14, ref/07, 07 | `defmt` in the field: ELF *build-id* in `.jkfw`, `defmt` frames in a Joker Link log message, ELFs kept in the Hub, `jk logs --node <mcu>` (§2.5) | medium |
| E8 | ref/06 §3 (new section), ref/06 §6 | `embedded-hal-async` driver crate adapter (`crate_driver!`), with the deadline and cancellation rules; tests with `embedded-hal-mock` (§2.7) | medium |
| E9 | ref/03 §2.3.7 | `jk board new <name> --chip <part>` from the HAL metadata (`stm32-metapac`, `esp-hal`, `embassy-rp`), with `# REVIEW` on what belongs to the board (§2.8) | medium |
| E10 | 07, ref/08 §2.9.2 | `probe-rs` as a library of `jk` (`probe` feature) for `jk flash`, `jk bench` and `jk trace --probe`; node header findable by symbol in the ELF (§2.6) | medium |
| E11 | ref/08 §2.11.5 | native apps compiled with `forbid(unsafe_code)` and dependencies from an accepted list, in the manner of Tock's *capsules* (§3.5) | medium |
| E12 | 23 §4 and §8 | the Phase 1 bench firmware uses `esp-hal` with `unstable` (touch) and the `esp-radio` beta: exact versions in its `Cargo.lock`, and the note that they are unstable APIs | medium |
| E13 | ref/08 §2.12.2 and §2.12.4, ref/03 §2.3.5 | correspondence with `embassy-boot` (`ACTIVE`/`DFU`/`STATE`, `mark_booted()`); `IMAGE_DEF` and ROM secure boot in the `rp2350-pico2` profile (§2.9) | low |
| E14 | ref/03 §2.3, ADR 0026 | new file type for expansion boards (`*.shield.toml`, in the manner of Zephyr's *shields*), which adds `[io]`, `[serial]`, `[pins]` to a board with the right header (§3.7) | low |
| E15 | ref/06 §2.10 (note) | name the ecosystem patterns that Joker already follows (generic driver over the trait, `split()`, generated tasks, mocks) (§2.10) | low |
| E16 | ref/06 §3.1 | explain, or change, the name `joker-hal`: it is not a HAL in the ecosystem's sense, it is the driver contract; those coming from `embedded-hal` confuse them | low |
| E17 | 21 (1.4), 19 §5 | evaluate MPU isolation (Tock, Hubris) as an alternative or a complement to `wasmi` for third-party apps on the MCU, with per-app restart instead of a reset (§3.9) | low |

**The first five (E1–E5) are the ones that change code in Phase 2.** The rest improve DX or operations and can come in later, without changing the C ABI or the blob format.

---

## Sources

- [embedded-hal v1.0 now released! (Rust Embedded WG)](https://blog.rust-embedded.org/embedded-hal-v1/)
- [Roadmap for traits removed on 1.0.0 release (embedded-hal #357)](https://github.com/rust-embedded/embedded-hal/issues/357)
- [embedded-hal: migrating from 0.2 to 1.0](https://github.com/rust-embedded/embedded-hal/blob/master/docs/migrating-from-0.2-to-1.0.md)
- [embedded-io-async (crates.io, versions)](https://crates.io/crates/embedded-io-async/versions)
- [embedded-can-interface (docs.rs)](https://docs.rs/embedded-can-interface)
- [embedded-hal-mock](https://github.com/rust-embedded/embedded-hal-mock)
- [critical-section](https://github.com/rust-embedded/critical-section)
- [Heapless v0.9.1 has been released!](https://blog.rust-embedded.org/heapless-091/)
- [svd2rust (docs.rs)](https://docs.rs/svd2rust/latest/svd2rust/)
- [stm32-data (README)](https://github.com/embassy-rs/stm32-data/blob/main/README.md)
- [stm32-metapac (stm32-data-generated)](https://github.com/embassy-rs/stm32-data-generated/tree/main/stm32-metapac)
- [embassy-stm32 (crates.io, versions)](https://crates.io/crates/embassy-stm32/versions)
- [embassy-executor 0.10.0 (docs.rs)](https://docs.rs/crate/embassy-executor/latest)
- [embassy-sync 0.8.0 (docs.rs)](https://docs.rs/crate/embassy-sync/latest)
- [embassy-net 0.9.1 (docs.rs)](https://docs.rs/crate/embassy-net/latest)
- [embassy-boot (docs)](https://docs.embassy.dev/embassy-boot)
- [embassy-boot-stm32 0.8.0 (docs.rs)](https://docs.rs/crate/embassy-boot-stm32/latest)
- [embassy-rp (docs)](https://docs.embassy.dev/embassy-rp/)
- [Embassy: task macro (non-generic tasks)](https://docs.embassy.dev/embassy-executor/git/cortex-m/attr.task.html)
- [Embassy crates released and Rust stable support](https://embassy.dev/blog/embassy-hals-released/)
- [rp-hal](https://github.com/rp-rs/rp-hal)
- [esp-hal 1.0.0 release announcement (Espressif)](https://developer.espressif.com/blog/2025/10/esp-hal-1/)
- [esp-hal 1.0.0 beta announcement (Espressif)](https://developer.espressif.com/blog/2025/02/rust-esp-hal-beta/)
- [esp-hal releases](https://github.com/esp-rs/esp-hal/releases)
- [esp-hal gpio: AnyPin and degrade](https://docs.rs/esp-hal/latest/esp_hal/gpio/index.html)
- [esp-radio 1.0.0-beta.1](https://github.com/esp-rs/esp-hal/releases/tag/esp-radio-v1.0.0-beta.1)
- [esp-rtos (docs)](https://docs.espressif.com/projects/rust/esp-rtos/0.1.0/esp32/esp_rtos/index.html)
- [The Rust on ESP Book: RISC-V and Xtensa targets](https://docs.espressif.com/projects/rust/book/installation/riscv-and-xtensa.html)
- [espup: update LLVM and GCC (PR #577)](https://github.com/esp-rs/espup/pull/577)
- [probe-rs releases](https://github.com/probe-rs/probe-rs/releases)
- [probe-rs 0.32.0 (docs.rs)](https://docs.rs/crate/probe-rs/latest)
- [defmt is now 1.0 (Ferrous Systems)](https://ferrous-systems.com/blog/defmt-1-0-released/)
- [device-driver (book)](https://diondokter.github.io/device-driver/book/v1/intro.html)
- [device-driver (GitHub)](https://github.com/diondokter/device-driver)
- [The Embedded Rust Book: GPIO and typestate](https://doc.rust-lang.org/stable/embedded-book/design-patterns/hal/gpio.html)
- [RTIC 2: migration to rtic-monotonics](https://rtic.rs/2/book/en/migration_v1_v2/monotonics.html)
- [Tock 2.2 Release](https://www.tockos.org/blog/2025/tock2.2/)
- [The Tock Book: Capsule Isolation](https://book.tockos.org/doc/threat_model/capsule_isolation)
- [Hubris: introduction (app.toml and static tasks)](https://github.com/oxidecomputer/hubris/blob/master/doc/intro.adoc)
- [Hubris Reference](https://hubris.oxide.computer/reference/)
- [Humility (the Hubris debugger)](https://github.com/oxidecomputer/humility)
- [Zephyr: Devicetree versus Kconfig](https://docs.zephyrproject.org/latest/build/dts/dt-vs-kconfig.html)
- [Zephyr: Devicetree bindings](https://docs.zephyrproject.org/latest/build/dts/bindings.html)
- [Practical Zephyr: Devicetree basics (Interrupt)](https://interrupt.memfault.com/blog/practical_zephyr_dt)
