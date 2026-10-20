# Part 2 — The Pi 3 as a node: install, enroll, the board, the real LED

**Goal:** turn the Raspberry Pi 3 B+ into a Joker node (`lab`) and get **the same app, without changing a line**, to blink the board's real `ACT` LED. Then watch the arbiter decide between you and the app, and find out what happens when things go wrong.

**You will learn:** installing `jokerd` · the organization CA and enrollment without open ports · `jk doctor --rt` · the board profile and `[resources]` · the `sysfs` driver · one lock per manifest · `jk apply --confirm`, generations and rollback · manual write and arbiter · the three-layer safe state, and what it does not cover.

**Before you start:** part 1 done (app `luz` 0.1). The Pi 3 B+ with a microSD card and its power supply, on the same Wi-Fi network as the laptop. No wire besides the power.

---

## 1. Prepare the Pi 3

With Raspberry Pi Imager, write **Raspberry Pi OS Lite (64-bit)**. In the Imager options: host name `lab`, user `pi`, your Wi-Fi network, SSH with your public key. Boot the Pi and confirm:

```sh
$ ssh pi@lab.local uname -m
aarch64
```

## 2. Install `jokerd`

```sh
lab$ sudo apt install ./jokerd_1.0.0_arm64.deb
lab$ sudo usermod -aG joker-admin pi          # so you can enroll the node over SSH (section 4)
lab$ exit                                     # log in again for the new group to count
```

The package creates the user `joker` (with access to the device groups), the groups `joker-admin` and `joker-ops`, the systemd unit, the udev rule that gives the `joker` group write access to the LEDs in `/sys/class/leds`, and an `/etc/joker/node.toml` with only `version = 1`.

```sh
lab$ systemctl status jokerd
```

```
● jokerd.service - Joker runtime
     Active: active (running)
jokerd[612]: warning[J0902]: node without identity: in enrollment mode
jokerd[612]: listening only on unix:/run/joker/api.sock (no identity, no TCP)
```

Without an identity, `jokerd` listens **only** on the Unix socket, even if you ask it for TCP: no enrollment port is open to the network (ref/02 §3.2.1).

## 3. The organization CA

On the laptop, create the CA that will sign the nodes' certificates and yours. Once, and outside the project:

```sh
$ jk ca init ~/org-ca --org bancada
```

```
created ~/org-ca/root.key        Ed25519 root (signs trust bundles; keep it safe)
created ~/org-ca/ca.key, ca.crt  P-256 issuer (signs node and user certificates)
created ~/org-ca/org             bancada
user certificate                 admin-dl → ~/.local/share/joker/certs/admin-dl.{crt,key}
recorded in [user] of ~/.config/joker/config.toml
```

The root and the issuer are different keys on purpose: the issuer signs every day; the root only signs the trust policy (ref/10 §3.1.1; ADR 0025). In part 11 the root will sign more things.

## 4. Enroll the node

```sh
$ jk node enroll ssh://pi@lab.local --ca ~/org-ca --name lab --allow-dev
```

```
→ ssh://pi@lab.local · socket /run/joker/api.sock · principal unix:pi (joker-admin)
CSR received          lab · P-256 key in a file (no TPM) · KEM x25519
certificate issued    CN=lab · SAN DNS:lab · CA bancada (~/org-ca)
trust                 anchor ed25519:5e0c… (root bancada) · bundle without Deck
policy                allow_dev = true (--allow-dev)
client                [nodes.lab] added to ~/.config/joker/config.toml
node enrolled. listeners: unix:/run/joker/api.sock
```

What happened, in the order of ref/02 §3.2.3:

1. **The bootstrap channel is the local socket, over SSH.** `jk` opened an SSH tunnel to `/run/joker/api.sock`. Whoever enrolls proves they have access to the node by being in the group `joker-admin` (or, without that, with the one-time code from `jokerd enroll-code`, `--code`). A TCP address does not do: `J-E1912`.
2. `jokerd` generated the keys on the node itself (the identity key and the KEM key, for the secrets of part 11). **The private key never left the Pi.** There is no TPM on the Pi 3, so the key stays in a file; `jk doctor` says so (`J0933`).
3. `jk` signed the CSR with the issuer from `~/org-ca` and installed the certificate and the first trust bundle.
4. `jokerd` wrote `node.toml`. **`--allow-dev` is required here:** the CA from `jk ca init` is not a development CA, and without it the node would refuse the project's `mode = "dev"` generations with `J0906`.
5. `jk` added the `lab` context to your configuration.

```toml
# ~/.config/joker/config.toml
version = 1

[user]
name = "dl"
cert = "~/.local/share/joker/certs/admin-dl.crt"
key  = "~/.local/share/joker/certs/admin-dl.key"

[defaults]
search = ["oci://hub.example.com/joker"]

[nodes.lab]
address = "ssh://pi@lab.local"
ca      = "~/org-ca/ca.crt"
```

The context's `address` is the **bootstrap address**, `ssh://…`: `jk` does not guess a TCP listener that the node does not have yet (ref/02 §3.2.3, step 5). **The TCP listener with mTLS does not come from enrollment: it comes from a generation** (section 7), because listening on the network is site policy, and `node.toml` only keeps the minimum that has to survive a bad generation (ref/02 §2.2.1). Everything that follows already works over SSH.

## 5. `jk doctor --rt`

```sh
$ jk doctor --rt --node lab
```

```
→ lab · no generation · from --node
board            Raspberry Pi 3 Model B Plus Rev 1.3 → rpi3b-plus@1 (level 2)
kernel.preempt_rt    no      6.12.25-v8 (PREEMPT_DYNAMIC)
   warning[J0352]: groups ≥ 10 ms run without guaranteed jitter; < 10 ms will be rejected (J0351)
cpu.isolcpus         —       the board suggests: isolcpus=3 nohz_full=3 rcu_nocbs=3
cpu.governor         ondemand   suggestion: performance
watchdog             /dev/watchdog0 · 15 s · ok
clock                NTP synchronized (systemd-timesyncd)
permissions, groups  ok
disk                 22 GiB free
warning[J0933]: identity key in a file, without TPM or secure element
rt.cyclictest        60 s on cpu 3 · p99 118 µs · max 2.4 ms   (recorded; no target on this board)
device_fallbacks     —   (no generation)
```

- **PREEMPT_RT.** The Raspberry Pi OS kernel does not have `PREEMPT_RT`. The RT preemption code has been in the mainline kernel since 6.12, but it is a build option, and this kernel comes without it. Consequence: the group `main`, at 20 ms, runs with the warning `J0352`, and a group below 10 ms would be rejected (`J0351`). To blink an LED and read a temperature, 20 ms without RT is enough. Building an RT kernel is an optional exercise, and the measurements with and without it are recorded on the bench (document 23, §5.2).
- **`isolcpus=3`.** This one is already worth it: it takes CPU core 3 away from the general scheduler, so that the `rt` group has it to itself. Add `isolcpus=3 nohz_full=3 rcu_nocbs=3` to the end of the single line of `/boot/firmware/cmdline.txt` and reboot (`lab$ sudo reboot`). `jk doctor --rt` then shows `cpu.isolcpus  3 · ok`.
- **Figures without a target.** The jitter targets of document 01, §21 are for CM4/CM5. On the Pi 3 they are recorded as a pessimistic reference: Ethernet and Wi-Fi go through USB and generate interrupts.

Do not declare `kernel = "preempt_rt"` in the manifest while the node does not have it: the manifest says what the node has, and `jk doctor` checks it.

## 6. The board

A **board profile** describes the fixed hardware of a computer model: CPUs, memory, supported kernel, watchdog, fixed IO, ports and buses (ref/03 §2.3). The one for the Pi 3 B+ already exists in the registry:

```sh
$ jk board show rpi3b-plus
```

```
rpi3b-plus 1.0.0 · Raspberry Pi 3 Model B+ · Raspberry Pi
  arch aarch64 · 4 CPUs · 1 GiB · 1.4 GHz · level 2 (no CI with hardware)
  supported kernel   preempt_rt   (boot_cmdline: isolcpus=3 nohz_full=3 rcu_nocbs=3)
  recommended        rt [3] · io [2] · bg [0, 1]
  watchdog           /dev/watchdog0 · 15 s
  gpio               pinctrl-bcm2835
  fixed IO           board.act_led    sysfs led ACT, output    fallback: —
                     board.cpu_temp   sysfs thermal_zone0
  i2c                i2c1 → /dev/i2c-1
  spi                spi0 → /dev/spidev0.0
  serial             uart0 → /dev/serial0
  net                eth → eth0 · wifi → wlan0
  source             oci://hub.example.com/joker/boards/rpi3b-plus:1.0.0
```

What this tells you, and why it matters:

- **`recommended`** is the CPU split the board suggests: CPU core 3 for the cycle, 2 for the drivers, 0 and 1 for the rest. It is the default of `[resources]` (ref/01 §5.4).
- **`memory`** is the basis of `J0380` (estimated memory above 70% of RAM).
- **`watchdog`** is the second layer of the safe state (section 12).
- **`fixed IO … fallback: —`**: the `ACT` LED **has no hardware fallback**. A board profile states the state that the hardware imposes when `jokerd` stops (the `fallback` key of `[io.*]`), and here there is none. Keep this in mind for section 12.

## 7. `joker.lab.toml`: the real Pi 3

```toml
# joker.lab.toml — lab node (Raspberry Pi 3 B+)
extends = ["joker.toml"]

[target]
board = "rpi3b-plus@1"

# The board's recommendation, written out in full.
[resources]
rt = { cpus = [3] }
io = { cpus = [2] }
bg = { cpus = [0, 1] }

# The board's ACT LED, through the sysfs driver.
[devices.pi]
driver = "sysfs"

[devices.pi.signals]
act_led = { led = "ACT", direction = "output" }

# The API on TCP as well, always with mTLS, so jk can reach it from the laptop without SSH.
[security.api]
listen = ["tcp:[::]:7443"]
```

- **`[target] board`** with the major (`@1`): the lock will pin the exact version and the digest (ref/02 §2.1.4). A board that does not exist is `J0172`.
- **`[resources]`** repeats `recommended`. You could omit it and the effect would be the same (`jk diff` would show it as `# default`); writing it makes the decision visible. The group `main` goes to the `rt` class by itself, because `[resources.rt]` exists, and `jk check` puts it on CPU 3. Overlapping `rt` with another class is `J0360`.
- **`[devices.pi]`** has the same name as the simulated device, and the signal `act_led` the same name as the model's signal. That is why the path `pi.act_led` exists in both worlds, and **neither the app nor `joker.toml` changes**. The board offers the same LED as `board.act_led`, but the prefix `board.` is reserved for fixed IO and does not exist in the simulator; a device with its own name keeps the path the same everywhere. `board.act_led` only exists if the manifest refers to it, and referring to it **and** having a `[devices.pi]` with `led = "ACT"` would be the same kernel resource twice (`J0178`, ref/03 §2.3.2). Consequence: the `fallback` of a board `[io]` only counts through the path `board.<name>`.
- **`[security.api] listen`** adds a listener to those of `node.toml`, and never removes them (ref/02 §3.2.2). TCP is always TLS 1.3 with mTLS: there is no key to turn it off (ref/09 §2.1).

**Why `sysfs`, and not `gpio`?** The `ACT` LED is wired to a GPIO line of the SoC, but that line **already has an owner**: the kernel's `leds-gpio` driver, which exposes it as `/sys/class/leds/ACT` and attaches a *trigger* to it (by default, SD card activity). Joker's `gpio` driver requests lines from the kernel through the chardev, and the kernel refuses a line that another driver has already requested. The `sysfs` driver talks to the LED through the interface the kernel offers for that: it writes `max_brightness` or `0` to `brightness`. When it opens the LED, it saves the current *trigger* and sets it to `none`, so that SD activity does not blink over the app; when it closes (orderly stop, `jk device stop`, a generation that removes the signal) it restores it (ref/01, appendix A; document 05, §10).

## 8. Check

```sh
$ jk check --env lab
```

```
✓ manifest           joker.lab.toml ← joker.toml · board rpi3b-plus@1 · 1 device (sysfs), 1 output, 1 app, 1 group
✓ bindings           luz.led → pi.act_led (bool)
✓ resources          rt [3] · io [2] · bg [0, 1]
✓ RT admission       main 20ms → cpu 3: 5 µs declared (0.03%)
✓ memory             estimated 38 MiB of 1 GiB
warning[J0352]: group `main` (20 ms) has no PREEMPT_RT kernel on this target; jitter not guaranteed
warning[W0205]: `pi.act_led` has no device fallback
   = note: an LED in sysfs has no watchdog of its own; if jokerd dies, it stays at the last value
   = help: in `mode = "prod"` this is an error; accept it with `accept_missing_fallback = "<justification>"`,
            or use hardware with a fallback
note: joker.lab.lock updated (boards, apps.luz)
```

Keep the `W0205` in mind: you will see it happen in section 12.

### What `jk check` says in production

Change to `mode = "prod"` in `joker.toml` and check again:

```
error[J0205]: `pi.act_led` has no device fallback
 --> joker.lab.toml:14:1
   |
14 | [devices.pi]
   | ^^^^^^^^^^^^ sysfs driver: no fallback; the board rpi3b-plus declares no fallback for the ACT LED
   = help: `accept_missing_fallback = "<justification ≥ 10 characters>"` in [outputs."pi.act_led"];
            `jk lock` copies it into [accepted]
```

The same warning became an error, with the same number. That is the `dev` and `prod` rule (document 03, §15). Put back `mode = "dev"`.

## 9. The lock

```sh
$ jk build --env lab                    # builds the app and precompiles the AOT for aarch64
$ jk lock --env lab --node lab
```

```
joker.lab.lock
  = boards."rpi3b-plus@1"   1.0.0   (oci://hub.example.com/joker/boards/rpi3b-plus:1.0.0)
  = apps.luz                0.1.0   (path:apps/luz)
  + runtime 1.0.0 · node lab
```

```toml
# This file is generated by jk. Do not edit by hand.
version = 1
runtime = "1.0.0"
node = "lab"

[boards."rpi3b-plus@1"]
resolved = "1.0.0"
digest = "sha256:…"
source = "oci://hub.example.com/joker/boards/rpi3b-plus:1.0.0"
oci = "sha256:…"

[apps.luz]
version = "0.1.0"
digest = "sha256:…"
wit = "joker:runtime@1.0.0"
source = "path:apps/luz"
```

One lock per input manifest (`joker.lab.toml` → `joker.lab.lock`, next to the `joker.sim.lock` from part 1), because the simulator and the board resolve different things (ref/02 §2.1.1). With `--node`, `jk` asks the node, with reads only, for the facts that depend on it and writes `runtime` and `node`; applying this lock on another node gives `J0480`. Here there is no discovery and no fallback to capture: `sysfs` has none. Put `joker.lab.lock` under version control.

## 10. Apply, confirm, generations

```sh
$ jk apply --env lab --node lab --confirm 5m
```

```
→ lab (dev) · from --node · ssh://pi@lab.local
plan of generation 1
  + board rpi3b-plus@1 · resources rt [3] io [2] bg [0, 1]
  + device pi (sysfs, 1 signal)
  + app luz 0.1.0 (aarch64 AOT, 208 KiB)
  + group main 20ms (rt, cpu 3)
  + api tcp:[::]:7443 (mTLS)                       ← touches the API: asks for confirmation
applied. confirmation pending: `jk confirm` within 5 min, or automatic rollback
```

The Pi's `ACT` LED blinks at 1 Hz. **The app is the same as in part 1, with the same digest.**

`--confirm` seems excessive for an LED, but it is the right habit, and here it has a concrete reason: this generation touches the API. On a remote installation, a generation that cuts off access undoes itself if nobody confirms it (ADR 0015). So **confirm through the new path**: switch the `lab` context to TCP and confirm through it. If the TCP listener did not work, you could not confirm, and the generation would roll back by itself.

```sh
$ jk config set nodes.lab.address lab.local:7443      # or by hand, in [nodes.lab] of config.toml
$ jk confirm --node lab
```

```
→ lab (dev) · from --node · lab.local:7443 (mTLS, cert:admin-dl)
generation 1 confirmed
```

The node's certificate verified against your CA and has the name `lab` in the SAN; your certificate verified against the node's client CA. In `dev`, without `[security.roles]`, any authenticated principal is `admin` (ref/01 §5.20).

Now a second generation. Change `half_period` to `"250 ms"` in `joker.toml` and apply:

```sh
$ jk apply --env lab --node lab --confirm 5m && jk confirm --node lab
$ jk generations --node lab
```

```
gen  state        origin   who             when                  hash
  2  confirmed    local    cert:admin-dl   2026-09-25 10:31:07   sha256:9e41…   ~ apps.luz.params.half_period 500 ms → 250 ms
  1  confirmed    local    cert:admin-dl   2026-09-25 10:24:52   sha256:3b0c…
```

Generation 2 only reloaded a parameter: the app did not restart. Roll back:

```sh
$ jk rollback 1 --node lab
```

```
→ lab (dev) · from --node · lab.local:7443
generation 1 reactivated (sha256:3b0c…) · origin rollback
```

A rollback **reactivates** the previous generation, with the same number and the same hash, and the history records the activation with the origin `rollback` (ref/00 §5). But your `joker.toml` still says 250 ms: the node and the repository diverge. Rollback is for emergencies; the truth is in Git. Restore the file (`git checkout joker.toml`).

## 11. Manual write and arbiter

With the app blinking, force the LED on for 30 s:

```sh
$ jk write pi.act_led true --ttl 30s --reason "arbiter test" --node lab
```

```
→ lab (dev) · from --node · lab.local:7443
pi.act_led: manual write (manual:4) for 30 s
  envelope: safe = false · no range, rate or interlock
  prevails over: control:12 (app:luz)
confirm? [y/N] y
accepted · manual:4 · until 10:42:37
```

The LED stays on. The app keeps proposing the blink, but the level `manual:4` has higher priority (1 is the highest). After 30 s, the manual level expires by itself and the blink comes back (document 16, A5 and A10). `--ttl` and `--reason` are required and have no environment variable, on purpose (document 07, §1).

While it lasts, ask Joker who controls this output:

```sh
$ jk explain pi.act_led --node lab
```

```
pi.act_led   bool · output · device pi (sysfs, led ACT)                  joker.lab.toml:18
  envelope   safe = false                                                joker.toml:9
  owning group main 20ms (the highest-priority app that writes it in a cycle, A11)
  levels     manual:4     true    user:cert:admin-dl · "arbiter test" · until 10:42:37   ← active
             control:12   false   app:luz · lease 60 ms (default, 3 × 20 ms)
  applied    true   (overridden: control:12 loses to manual:4)
  last       control:12 → overridden(manual:4) · manual:4 → accepted
```

And the decision log, which keeps every write with its result and its reason:

```sh
$ jk decisions --output pi.act_led --since 1m --node lab
```

```
10:42:07.300  manual:4     true   accepted              (user:cert:admin-dl, "arbiter test")
10:42:07.320  control:12   false  overridden(manual:4)
…
10:42:37.300  manual:4     —      expired (ttl 30 s)
10:42:37.320  control:12   true   accepted
```

## 12. When things go wrong: the three-layer safe state

Three experiments, in the right order (document 01, §8).

**(a) The app stops.**

```sh
$ jk app stop luz --node lab
```

The LED goes off within ≤ 60 ms. Nobody wrote `false`: the app's lease expired, no level was left active, and the arbiter applied the `safe` (document 16, A6). **First layer: the lease.** It covers the app that is dead, stuck or in a *crashloop*. Turn it back on with `jk app start luz --node lab`.

**(b) `jokerd` gets stuck.** You will not provoke it, but it is there: the `jokerd` supervisor thread arms the board's hardware watchdog (`/dev/watchdog0`, 15 s, what `jk board show` showed) and systemd's `WatchdogSec`. If a group gets stuck, the supervisor restarts the runtime; if the whole process stops responding, the watchdog reboots the board. **Second layer: the supervisor.** It covers what the first does not, because a lease only expires if there is someone to count it.

**(c) `jokerd` dies.** Wait until the LED is on and kill the process:

```sh
lab$ sudo systemctl kill -s KILL jokerd
```

**The LED stays on.** With the process dead, nobody writes to `brightness` or restores the *trigger*, and the kernel keeps the last value. systemd restarts `jokerd` within 1–2 s; `jokerd` starts with every output in `safe` (document 16, A8), the LED goes off, and `luz` blinks again. With the LED off at the instant of the `kill`, you would see no difference at all: do it both ways.

This is the **honest lesson of part 2: the `ACT` LED has no hardware fallback.** The third layer of the safe state is the **device fallback** (M1): the equipment itself goes to a safe value when it stops hearing from Joker, and it is the only layer that covers a dead `jokerd`, a board without power and a cut cable. An LED in `sysfs` has none of that, and the board declares none. It is exactly the `W0205` of section 8, and in `prod` it would be the error `J0205`.

On an LED it does no harm. On a heater or a motor, "stays as it was" can mean "stays on". That is why Joker, in production, requires equipment with its own watchdog, or a written justification (`accept_missing_fallback`) that stays in the lock with who gave it and when. In part 4 you will have a piece of equipment that protects itself: the ESP32 bench firmware turns its LED off if it hears nothing for 2 s.

## 13. What happened underneath

- **Identity without open ports.** The node without an identity only listens on the Unix socket; enrollment runs over that socket (over SSH); the key is born on the node and stays there; the TCP listener only exists afterwards, with mandatory mTLS, and because a generation asked for it (ref/02 §3.2; ref/09 §2.1; ADR 0025).
- **The generation** is the effective manifest, the lock and the artifacts, with a hash. `jk apply` sends it, the node verifies each artifact by the lock digest, activates it, and waits for the confirmation within the window; without it, it goes back to the last confirmed one (document 01, §15; ADR 0015).
- **The board** entered the lock by the digest of the file's bytes. From it came the default of `[resources]`, the memory for `J0380` and the supervisor's watchdog. `jokerd` put the group `main` in a cpuset with only CPU 3, which `isolcpus` took away from the general scheduler (document 01, §2).
- **The same core, another driver.** The tick is the one from part 1; the difference is that the commit goes to the `sysfs` driver instead of `sim`. The arbiter and the envelope do not know, and do not need to know, where the LED comes from.
- **The arbiter** decides by level and by lease, and never by who arrived first: a human at `manual:4` beats an app at `control:12` until the end of the TTL, and everything stays in the decision log (document 16, §1; ADR 0011).

## 14. Exercises

1. With `jk dev --env lab --node lab`, change `half_period` and watch the real LED change without `jk apply`. What appears in `jk generations` afterwards?
2. Set `rt = { cpus = [2] }` in `joker.lab.toml`, without touching `io`. Read what `jk check` says and explain why.
3. Write `jk write pi.act_led false --ttl 10m --reason "…"` and, in another terminal, a second write at the same level from another user (`ssh pi@lab.local jk write … --node local`). What does the second one answer? And with `--takeover`?
4. Do experiment 12(c) with the LED off and then on. Explain the difference to someone, using the words "lease", "supervisor" and "device fallback".

```sh
$ git add -A && git commit -qm "track(02): the Pi 3 as node lab; real ACT LED with luz 0.1"
```

**State at the end of this part:** app `luz` **0.1, unchanged**: only the manifests changed. New files: `joker.lab.toml`, `joker.lab.lock`. Outside the project: `~/org-ca/`, the `lab` context in `~/.config/joker/config.toml`, and the node `lab` enrolled, at confirmed generation 1.

**Next:** [Part 3 — A real sensor: the CPU temperature, alarms, historian and the API](03-temperature-sensor.md).
