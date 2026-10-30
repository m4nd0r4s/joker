# ADR 0092 — The root medium in the hardware inventory, and `jk board probe`

**Status:** accepted (v1) · **Date:** 2026-10

## Context

AUR-279 and AUR-412 build `GET /v1/node/hardware` (ref/09 §5.2), `jk board probe` and `jk board check --node` (ref/03 §2.3.7), and the Phase 1 reference boards (document 05 §11, document 11). Three gaps in the documents meet that work:

- **The medium of the root file system.** ref/03 §2.3.1 says that `jk board probe` proposes `storage` from the root device (`/sys/block/<dev>/device/type`), and the table of §2.3.7 reads it from a `storage` field of the inventory. `NodeHardware` in ref/09 and in the OpenAPI has no such field, so the probe cannot write `storage` from what the node serves.
- **The `# REVIEW` markers and the comment guard.** A draft carries `# REVIEW` comments by design (ref/03 §2.3.7), and the comment guard of this repository rejects comments in `*.toml`. A note on AUR-412 asked whether one of them had to change.
- **What a reference board promises.** `memory` is the basis of `J0380`, and `jk board check --node` accepts a unit only when `memory` is at least its `MemTotal` and less than twice it. One file per model can therefore describe one RAM size only, and the Raspberry Pi models are sold in several.

## Decision

### The inventory

1. `NodeHardware` gains `storage`: `{ device, medium }` or `null`.
   - `device` is the disk of the root file system (`/dev/mmcblk0`), read from `/proc/self/mounts`. When the mount says `/dev/root`, it goes through `root=` of the command line and the `/dev/disk/by-*` link. A partition is taken to its disk through `/sys/class/block`.
   - `medium` uses the values of `[board] storage`: `SD` and `MMC` in `/sys/block/<dev>/device/type` give `sd` and `emmc`; NVMe and SATA (`sd*`) disks give `ssd`; anything else is `null`.
   - The field is optional in the schema, so a client of an older node reads its absence as `null`.
2. `net[].addresses` is read from files, like the rest of the inventory:
   - IPv4 from the `host LOCAL` entries of `/proc/net/fib_trie`, each given to the interface of the longest route of `/proc/net/route` that holds it (`127.*` to `lo`);
   - IPv6 from `/proc/net/if_inet6`.

   The GPIO lines come from the line-information ioctl, which reports the consumer of a line without requesting it, as ref/09 §5.2 requires.
3. A LED whose name is the consumer of a requested GPIO line gets that line as its `gpio` (`leds-gpio` names the line after the LED). That replaces the walk through `device/of_node`, which needs the device tree to be readable.

### `jk board probe` and `jk board check --node`

4. A marker is `# REVIEW:` with the colon, so the first line of a draft, which names the markers (`Resolve the "# REVIEW" lines`), is not one. The draft and the notes of `jk board check` count the same lines.
5. The `# REVIEW` markers stay comments. A draft is a file of the user's project, written by `jk`, and the comment guard covers only the files of this repository. A board shipped in this repository's `boards/` is never a draft: it carries no comments and no unresolved marker.
6. `jk board check <file>` runs the local checks first; with `--node` (or `JK_NODE`), it then confirms the file against the inventory. An error is exit code 2, and `--json` prints `{ok, findings}`, each finding with `code` (`null` for a note), `message` and `line`.

### The reference boards

7. A reference board describes **one** unit of its model: the RAM size it names in `description` (`rpi4` and `cm4`: 4 GiB; `cm5` and `rpi5`: 8 GiB). Another RAM size, a Lite (SD) compute module, or the wireless option of a compute module is a variant: `jk board new <name> --from <board>`, then the line that changes.
8. A compute module promises only what the module itself carries. Ethernet comes from the carrier and is declared as `eth0`; the wireless interface is left out, because some modules have none.
9. `x86-generic` promises an `x86_64` machine with 4 cores, 8 GiB and an SSD, and nothing fixed: no watchdog, no GPIO, no fixed IO, no buses. A real x86 unit is described with `jk board probe`.
10. `rpi5` goes to 1.1.0 with `[io.act_led]` and `[io.cpu_temp]`: adding fixed IO is a minor change (ref/03 §2.3.6).

## Consequences

- ref/09 §5.2 and the OpenAPI gain `storage`, and ref/03 §2.3.7 reads `storage.medium`.
- `revpi-connect-4` stays deferred with the `revpi` driver (document 26 §5).
- Opening the fixed IO of a board as `board.<name>` (ref/03 §2.3.2) is not part of this ADR. The reference boards declare their LED and temperature; a manifest uses them through its own `sysfs` device, as the track does (track part 2), until `jokerd` opens `board.<name>` itself.
