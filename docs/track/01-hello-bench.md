# Part 1 — Hello, bench: an LED in the simulator

**Goal:** start from an empty directory and end with the app `luz` blinking the `ACT` LED of a **simulated** Raspberry Pi, with app tests and a site scenario that prove it.

**You will learn:** project and manifest · overlays from day one · the type in the file name · `sim` device · signal, port and binding · output with `safe` · cycle group · hot-reload of parameters and code · `jk read` and `jk trace` · `joker-test` and the virtual clock · scenarios.

**Before you start:** the laptop with stable Rust and `jk` installed (`jk --version` answers `jk 1.0.0`). There is no hardware in this part.

---

## 1. Empty directory → project

```sh
$ mkdir joker-bancada && cd joker-bancada
$ git init -q
$ jk init --name bancada
```

```
created joker.toml
created sim/
created .gitignore            (ignores target/ and .joker/)
next step: jk app new <name>
```

The `joker.toml` that `init` writes has only the project:

```toml
[project]
name  = "bancada"
joker = "1.0"           # minimum runtime version that accepts this manifest
mode  = "dev"           # dev: warnings instead of errors for what only matters in production
```

The default of `mode` is `prod` (ref/01 §5.2): forgetting the key on a real site must not turn signatures off. That is why `init` writes `dev` explicitly.

## 2. Create the app

```sh
$ jk app new luz --lang rust
```

```
apps/luz/
  Cargo.toml            depends on `joker-sdk` (the API), `joker-build`, and `joker-test` in the tests
  build.rs
  src/lib.rs
```

## 3. A simulated Pi

The device will be called `pi` in the simulator and on the hardware. That is what makes the path `pi.act_led` exist in both worlds, and it is why the app and the common manifest will never change when you move to the board.

For now, the simulated Pi has a single signal: the `ACT` LED.

```toml
# sim/pi.sim.toml
schema = 1

[model]
description = "Bench Raspberry Pi, simulated: for now, only the ACT LED"

[signals]
act_led = { type = "bool", direction = "output", label = "ACT LED", panel = { label = "ACT", kind = "lamp" } }
```

- `direction = "output"`: someone writes this signal. The value the model receives is the **applied** one, after the arbiter and the envelope, and never the raw proposal of the app (ref/03 §2.4.4).
- `panel = { kind = "lamp" }`: in the `jk dev` panel, it appears as a lamp. A lamp shows the value and never changes it.

## 4. The manifest, in two layers from the start

The manifest separates **what the system does** (the app, the output and its `safe`, the cycle) from **where the hardware is** (the devices). The first goes into `joker.toml`; the second into one overlay per target.

```toml
# joker.toml — the common part. No device here.
[project]
name  = "bancada"
joker = "1.0"
mode  = "dev"

# Every output says what is "safe". For an LED, off.
[outputs."pi.act_led"]
safe = false

# One cycle group: a clock that beats every 20 ms.
[cycles.main]
period = "20ms"

# The app, and the binding of its port `led` to the signal `pi.act_led`.
[apps.luz]
path   = "apps/luz"
bind   = { led = "pi.act_led" }
params = { half_period = "500 ms" }
cycles = { pisca = "main" }
```

```toml
# joker.sim.toml — everything simulated, on the laptop
extends = ["joker.toml"]

[devices.pi]
driver = "sim"
model  = "sim/pi.sim.toml"
```

**Why the overlay already in part 1?** Because in part 2 you will add `joker.lab.toml`, with the real Pi 3, and you will not touch anything else. If the simulated device were in `joker.toml`, you would have to take it out, and `joker.toml` would stop being "the system" and become "the system on the laptop". With two layers from the start, each target is a small file that only says where the hardware is (ref/01 §7).

**The names state the type.** Every project file carries its type in the suffix: `joker.toml` is the manifest, `joker.<variant>.toml` an overlay, `sim/pi.sim.toml` a simulation model. With the suffix, the editor knows the schema and gives completion and errors while you type, and `jk check` rejects with `J0803` a file whose content does not match its name (ADR 0026). The variant is also what you choose on the command line: `--env sim` means `joker.sim.toml`.

Three things to notice:

- **`pi.act_led` is a path**: `<device>.<signal>`. The app does not know it. It knows its **port** `led`, and it is the manifest that makes the **binding** (document 02, §9).
- **`safe = false` is not optional.** There is no "default safe value" (ADR 0006). `safe` is a decision about the system, and so it lives in the common part, and not in the hardware overlay.
- **We did not declare `writer`.** With a single writer on the output, the default applies: level `control:12` and a lease of 3 periods of the owning group, 60 ms (document 16, A9 and A11; ref/01 §3.11). This only starts to matter in part 2.

## 5. The app `luz` 0.1

```rust
// apps/luz/src/lib.rs
use joker::prelude::*;

/// Blinks an LED. Version 0.1 of the track.
#[joker::app(name = "luz", version = "0.1.0")]
pub struct Luz {
    /// The LED. The app only knows it is an output bool.
    #[output]
    led: Output<bool>,

    /// Half period of the blink: 500 ms on, 500 ms off.
    #[param(unit = "ms", range = "20..5000", default = 500.0)]
    half_period: Param<f64>,
}

/// Local state of the cycle task: lives between ticks, in the app's memory.
#[derive(Default)]
struct Pisca {
    elapsed_ms: f64,
    on: bool,
}

#[joker::cycle(period = "20ms", wcet = "5us")]
fn pisca(io: &mut CycleIo<Luz>, st: &mut Pisca) -> Result<()> {
    // The real period of the group (io.dt()), and not a constant: the manifest may change it.
    st.elapsed_ms += io.dt().as_secs_f64() * 1000.0;

    if st.elapsed_ms >= io.half_period.get() {
        st.elapsed_ms -= io.half_period.get();   // subtracting, and not resetting to zero, does not accumulate error
        st.on = !st.on;
    }

    // Proposes the value on every tick. That renews the lease: if the app stops, the LED goes to `safe`.
    io.led.set(st.on);
    Ok(())
}
```

Why write the LED on every tick, and not only when it changes? Because **proposing is renewing the lease** (document 16, A2). Joker sends nothing to the driver when the applied value does not change, but it learns that the app is alive. That is what guarantees that, if the app dies, the LED goes to `safe` within 60 ms.

The macro's `period` is a **request**: what decides is the group the manifest binds the task to (`cycles = { pisca = "main" }`). If they differ beyond the app's tolerance, `jk check` warns with `J0130`.

## 6. Check

```sh
$ jk check --env sim            # the same as --manifest joker.sim.toml
```

```
✓ manifest           joker.sim.toml ← joker.toml · 1 device (sim), 1 output, 1 app, 1 group
✓ bindings           luz.led → pi.act_led (bool)
✓ parameters         luz.half_period = 500 ms (20..5000 ms)
✓ RT admission       main 20ms: 5 µs declared (0.03%)
warning[J0352]: group ≥ 10 ms without PREEMPT_RT: jitter not guaranteed: group `main` (20 ms)
   = note: on the laptop, with the simulator, it is irrelevant
note: without `[target] board`, the checks that depend on the board are left undone
note: joker.sim.lock updated (apps.luz)
```

Notice the last line: each input manifest has its lock in the same directory (`joker.sim.toml` → `joker.sim.lock`), and in `dev` `jk check` keeps it up to date by itself (ref/02 §2.1.1 and §3.1.2). Put it under version control.

### What `jk check` says when something is wrong

Without `--env` (or `--manifest`), `jk` uses `joker.toml`, which has no devices:

```sh
$ jk check
```

```
error[J0101]: unknown signal (with suggestion): `pi.act_led`
 --> joker.toml:8:10
  |
8 | [outputs."pi.act_led"]
  |          ^^^^^^^^^^^^ no device `pi` in this manifest
  = note: `joker.toml` is extended by joker.sim.toml
  = help: jk check --env sim
```

That is the right behavior: the common part alone is not a system. So that you do not have to type `--env sim` all the time, put the default in the project's local configuration (it lives in `.joker/`, which is not versioned; ref/02 §2.3.2):

```toml
# .joker/config.toml
version = 1

[defaults]
manifest = "joker.sim.toml"
```

Now delete the line `safe = false` from `joker.toml` and check again:

```
error[J0201]: output without `safe`: `pi.act_led`
 --> joker.toml:8:1
  |
8 | [outputs."pi.act_led"]
  | ^^^^^^^^^^^^^^^^^^^^^^ this output does not say what is safe
  = note: there is no default safe value (ADR 0006)
  = help: for an LED, `safe = false`
```

Put the line back.

## 7. Run

```sh
$ jk dev --panel --env sim
```

```
joker 1.0.0 · dev · bancada · generation 1 · local target (sim)
┌ pi (sim) ──────────────────────────────────┐
│ ACT        ●                               │   ← toggles every 500 ms
└────────────────────────────────────────────┘
 groups   main 20ms   jitter p99 0.7 ms   overruns 0
 apps     luz 0.1.0  running  measured wcet 1.6 µs
 outputs  pi.act_led = true  (accepted · control:12 · app:luz)
 [q] quit  [r] reload  [t] trace pi.act_led  [?] help
```

**Parameter hot-reload.** With `jk dev` running, change the parameter in `joker.toml` and save:

```toml
params = { half_period = "100 ms" }
```

`jk dev` applies a new generation (generation 2). The LED starts blinking at 5 Hz **without restarting the app**: the new value reaches the cycle instance in the image of the next tick (ref/04 §4.3.4). Try `"10 ms"`: the generation is rejected with `J0120` (outside the range `20..5000` that the app declares), and generation 2 keeps running. Go back to `"500 ms"`.

**Code hot-reload.** Change the app, for example to an asymmetric blink (`if st.on { … }` with two times), and save. `jk dev` recompiles the component and swaps it between two ticks. The task state starts again at `Default` (LED off, counter at zero), because `Pisca` does not have `#[joker::state(persist)]`; with it, the swap would migrate the state (ref/04 §4.12). The previous version stays warm for `keep_previous` (10 min), so you can go back without recompiling. Undo the change before you continue.

## 8. Look inside

In another terminal, with `jk dev` running:

```sh
$ jk read pi.act_led --watch
```

```
pi.act_led   true    good   age 4 ms    control:12 · app:luz
pi.act_led   false   good   age 2 ms    control:12 · app:luz
…
```

```sh
$ jk trace pi.act_led
```

```
tick 1250  t=25.000s  main
  app:luz.pisca     proposes true   (control:12, lease until 25.060s)
  arbiter           active level control:12 → true
  envelope          accepted
  commit            true → driver sim (changed)
tick 1251  t=25.020s  main
  app:luz.pisca     proposes true   (control:12, lease until 25.080s)
  arbiter           active level control:12 → true
  envelope          accepted
  commit            — (same as applied)
```

`jk trace` follows a signal through the whole chain: driver, acquisition, apps that read it, proposals, arbiter, envelope, commit, readback (document 07, §4). On an output, it is the first tool to reach for when something does not do what you expected.

## 9. App tests, with a virtual clock

A blinking LED seems not to need tests, but this one shows what Joker gives you for free: a **virtual clock**. Ten seconds run in milliseconds, deterministically, with the real arbiter and envelope (ref/04 §6.1).

```rust
// apps/luz/src/lib.rs (at the end)
#[cfg(test)]
mod tests {
    use super::*;
    use joker_test::prelude::*;

    #[test]
    fn pisca_a_1hz() {
        let mut h = Harness::<Luz>::new()
            .group("pisca", 20.ms())
            .param("half_period", 500.0);

        h.run_for(10.s());

        let led = h.output("led");
        assert_eq!(led.toggles(), 20);                  // 10 s at 1 Hz = 20 changes
        assert!(led.toggles_every(500.ms()));           // exact: the clock is virtual
    }

    #[test]
    fn parar_a_app_apaga_o_led() {
        let mut h = Harness::<Luz>::new()
            .group("pisca", 20.ms())
            .safe("led", false);
        h.run_for(520.ms());                            // LED on since 500 ms
        assert_eq!(h.output("led").last(), Some(true));
        h.stop_app();
        h.run_for(80.ms());
        assert_eq!(h.output("led").last(), Some(false)); // 60 ms lease expired → safe
    }
}
```

```sh
$ jk test
```

```
apps/luz (cargo test, wasm component on wasmtime) ... 2 passed
```

`jk test` runs the **real `.wasm` component**, and not a native build of the app: it is what goes to the node. `cargo test` also works, with the native backend, and warns that it is not the production component.

## 10. A site scenario

The app tests run the app alone. A **scenario** runs the **site**: the complete manifest, the simulated model, the real core and the virtual clock (ref/03 §2.5).

```toml
# sim/scenarios/pisca.scenario.toml
schema = 1

[scenario]
name     = "blinks at 1 Hz and goes to safe when the app stops"
manifest = "joker.sim.toml"
duration = "10s"

[[expect]]
during        = "1s .. 7s"
signal        = "pi.act_led"
toggles_every = "500ms ± 20ms"

[[app]]
at  = "8s"
app = "luz"
do  = "stop"

[[expect]]
at     = "8.1s"
signal = "pi.act_led"
equals = false
level  = "none"
```

The last `[[expect]]` says two things: the LED is off **and** there is no active level in the arbiter. Nobody wrote `false`: the app's lease expired and the arbiter applied the `safe` (document 16, A6).

```sh
$ jk test
```

```
apps/luz (cargo test, wasm component on wasmtime) ... 2 passed
sim/scenarios/pisca.scenario.toml (joker.sim.toml, 10 virtual s in 41 ms) ... passed
```

A failing expectation is `J0842`, with the instant, the value seen and the decision chain from `jk trace`. Try it: set `toggles_every = "400ms ± 20ms"` and read the report. `jk test` exits with code 6 (document 07, §8), which is what CI uses.

## 11. What happened underneath

Every 20 ms, the group `main` runs a **tick** (document 01, §4):

1. **latch:** freezes the inputs into a process image (there are none yet);
2. **tasks:** calls `pisca` in the app's Wasm instance, which proposes `led = true/false`;
3. **arbiter:** the proposal takes level `control:12` and renews the lease (document 16, A2);
4. **envelope:** everything passes, because for a `bool` we only declared `safe` (document 01, §7);
5. **commit:** if the applied value changed, it goes to the `sim` driver, which shows it in the panel.

`jk dev` and `jk test` run exactly this core. The only difference is the clock: real in `jk dev`, virtual in `jk test` (ADR 0007). That is why what passes in a scenario is what happens on the node.

The manifest layers are resolved before anything else: `joker.sim.toml` extends `joker.toml`, the tables are merged key by key, and the result is the **effective manifest**, with every default materialized (ref/01 §7 and §8). See it with `jk diff --env sim`: the 60 ms lease and the level `control:12` appear there, marked `# default`.

## 12. Exercises

1. Make the LED blink with a 10% *duty cycle* (100 ms on, 900 ms off), with two parameters. Update the test and the scenario.
2. Change the group's `period` to `"50ms"` without touching the app. Is the blink still right? Why? (Hint: `io.dt()`.) What does `jk check` say?
3. In the scenario, add a manual write (`[[write]]`, with `output`, `value = true`, `ttl = "2s"`, `at = "3s"`) and an expectation `level = "manual:4"` during that interval. What happens to `toggles_every` between 3 s and 5 s?

```sh
$ git add -A && git commit -qm "track(01): luz 0.1 blinks the simulated ACT LED; tests and scenario"
```

**State at the end of this part:** app `luz` **0.1**. Files: `joker.toml`, `joker.sim.toml`, `joker.sim.lock`, `sim/pi.sim.toml`, `sim/scenarios/pisca.scenario.toml`, `apps/luz/`.

**Next:** [Part 2 — The Pi 3 as a node: install, enroll, the board, the real LED](02-the-pi-as-a-node.md).
