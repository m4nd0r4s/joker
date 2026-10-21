# The bench project

The project of the learning track (`docs/track/`), in the part of it that the M0 slice runs (docs/26 §6): the `light` app of part 1, built to a Wasm component, and the bench manifests of docs/23 §6 reduced to the Raspberry Pi.

| File | What it is |
|---|---|
| `joker.toml` | the common part: the output `pi.act_led` with its `safe`, the `main` group and the app |
| `joker.sim.toml` | the laptop: the `pi` device simulated |
| `joker.rpi.toml` | any Raspberry Pi: the `pi` device through `sysfs` (ACT LED and CPU temperature) |
| `joker.lab.toml` | the Pi 3 B+ of the bench (`rpi3b-plus@1`), with its CPU classes |
| `apps/light/` | the app, version 0.1: blinks `led` every `half_period` |

```sh
cd examples/bench
jk check --env sim
jk dev --env sim
jk --node unix:.joker/dev.sock read pi.act_led
```

`jk check` and `jk dev` build the app with cargo for `wasm32-wasip2` and run the component through `joker-host`. The end-to-end test is `cargo test -p jokerd --test bench_example`: it runs the simulated bench for 200 ticks, checks that the LED toggles every 500 ms, and replays the trace with zero divergences.

## What the track uses beyond M0

These are the places where parts 1–5 of the track need something the M0 slice does not have yet. They feed the friction report of M0 (AUR-211).

| Track | Needs | M0 today |
|---|---|---|
| part 1 §3 | `model = "sim/pi.sim.toml"` and the `panel` of a simulation model | the signals are declared inline in `[devices.pi.signals]`; `jokerd` runs its own minimal `sim`, not `joker-drivers/sim` |
| part 1 §2, §7 | `jk app new`, `jk build`, `jk test`, the `joker-build` and `joker-test` crates | `jk` builds the app with cargo when it checks or runs the project; the app depends on `joker-sdk` by path |
| parts 1–2 | the lock written by `jk check` in `dev` and by `jk lock` | no lock is written |
| part 2 §9–§10 | `jk apply` to the Pi 3 over SSH, `[security.api]` on TCP with mTLS, AOT for aarch64 | the API is on the local Unix socket only |
| part 3 | `calc` (windows such as `avg(pi.cpu_temp, 10s)`), alarms, the historian, a simulated dynamic for the CPU temperature | not in `jokerd`; `light` 0.2 and later need `calc.temp_avg` |
| part 4 | the ESP32 over Modbus RTU in `jokerd` (`local/bench-esp32@1`), the bus budget at run time | the profile and the driver are known to `jk check`; the driver crate does not run in `jokerd` yet |
| part 5 | `calc.touch_level`, `#[joker::state(persist)]` with `export-state` and `migrate`, scenarios with `[[inject]]` | the cycle subset of `joker-host` has no `export-state`, so a replay of a Wasm app starts from `init` |
| all | hot reload of the app on save | `jk dev` reloads the manifest; a changed app needs a restart |
