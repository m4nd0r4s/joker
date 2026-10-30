# ADR 0098 — The world window, `GET /v1/version`, and unstable imports in 1.0

**Status:** accepted (v1) · **Date:** 2026-10

## Context

AUR-289 (M21) makes mixed-version fleets safe. Three points meet gaps between ref/04 and what the 1.0 host can do:

- **The window.** ref/04 §2.8 says the runtime serves the current world and the previous minor before 1.0, and every `1.y` with y ≤ its own after. The host loaded only the exact world it serves.
- **`GET /v1/version`.** ref/09 §5.1 lists the endpoint and the OpenAPI has its `Version` schema, but `jokerd` did not serve it. Nothing says what a build reports for a surface it does not carry: tier W drivers, JDP, Joker Link.
- **Unstable imports in `dev`.** ref/04 §2.9 says that in `dev` the runtime binds unstable items to stubs that return `J-E0601`. The 1.0 host is the cycle subset (ADR 0052): it serves no async instance. Every unstable item but `cycle-io.write-at` is either an async function (`async-io.write-at`, `propose`, `ml.infer`, `frames.next`, `blocks.next`) or a resource (`tensor`, `frame`), and the cycle subset cannot bind either.
- **The SDK features.** ref/04 §4.15 lists `unstable-<name>` Cargo features that give the 1.1 APIs. The SDK generates its bindings without the `@unstable` items, and none of those APIs exists yet.

## Decision

1. **The window** is a pure rule over `(major, minor)`; the patch never decides.
   - A newer major or minor is `J-E0001`.
   - Before 1.0, the window is the served minor and the one before it; from 1.0 on, every minor of the served major.
   - Anything else is `J-E0002`.
   - `jokerd` applies it at load. `jk apply` applies it before uploading, against the `wit.current` of the node's `GET /v1/version`: an app outside the node's window is refused with that code, before any request that changes the node. A node without the endpoint is left to refuse at load.
2. **`GET /v1/version`** reports what this build carries:
   - `runtime`, `api`, `wit` (`current` and `supported`, the worlds of the window that have been published), `manifest` (the `[project] joker` versions accepted), `lock` (`[1]`), `abi`, `trace` (`[1]`) and `agent`;
   - `driver_wit` and `jdp` are empty, and `link` has an empty `supported`, while the build has no tier W drivers, no JDP and no Joker Link.
3. **`jk build --world <world>`.** The SDK generates one world, `joker:runtime@<the SDK's minor>`. `--world` with that world builds; any other world is a usage error that says which `joker-sdk` minor to pin (ref/04 §4.1). When the generated world is newer than `[project] joker`, `jk build` warns that a node of that version refuses the apps (`J-E0001`).
4. **Unstable imports in 1.0.**
   - In `prod`, a component that imports an unstable item is refused at load with `J-E0003`.
   - In `dev`, `cycle-io.write-at` is bound to a stub that traps with `J-E0601`.
   - In `dev`, a component that imports any other unstable item is refused at load with `J-E0601`: the cycle subset serves no async instance. The stubs of ref/04 §2.9 for those items come with the async instance.
5. **No `unstable-*` features in the 1.0 SDK.** The bindings are generated without the `@unstable` items, so an app cannot reach them. The features of ref/04 §4.15 arrive with the APIs they give.

## Consequences

- ref/04 §2.9 and §4.15 state rules 4 and 5; the window of §2.8 is unchanged.
- An app built against an older minor runs on a newer node within the window, and an app newer than the node is refused by `jk` before the deploy.
