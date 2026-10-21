# joker-host

The Wasm host of Joker (docs/01 §9, ref/04): wasmtime `=49.0.4`, the **cycle subset** of `joker:runtime@0.1.0` and the AOT cache.

What `jokerd` calls:

```rust,ignore
let engine = HostEngine::native()?;                          // or HostEngine::for_target("aarch64-unknown-linux-gnu")
let cache = AotCache::new("/var/lib/joker/aot");
cache.install(&cross_engine, &wasm)?;                        // jk build --target, on the laptop
let app = cache.load(&engine, &wasm, RuntimeMode::Prod)?;    // the node: AOT only in prod
let mut cycle = CycleInstance::new(&engine, &app, &registry, &image, &setup)?;  // calls init
// every tick, after image.latch() and the async staging:
let report = cycle.run(task, &mut image, &outputs)?;         // one on-cycle call; writes staged
// then outputs.commit(), image.end_tick(); a supervisor thread calls engine.increment_epoch() every 100 µs
```

- Engines always target an explicit triple with the baseline CPU features, so a `.cwasm` cross-compiled on the laptop has the same `EngineId` as the node's engine (`aot/<engine>/<sha256>.cwasm`, ADR 0052).
- NaN canonicalization is on and relaxed SIMD off: the same component gives the same bits on x86_64 and aarch64 (the `cycle` test checks one recorded digest on both).
- A trap poisons the instance (`CycleInstance::is_poisoned`): the caller drops it and creates a new one, which counts toward the crashloop. A malformed proposal buffer (`J-E0907`) drops the app's proposals of the tick without poisoning.
- Not served yet by the cycle subset: the async instance (components that import `async func`s are refused with `J-E0601`), class devices and implementations, wide values (`string`, `bytes`, records), unit conversion between a port and its signal, forwarding of guest stdout/stderr to the log.

## Testing

The tests build the guest fixture `fixtures/echo` (a hand-written app over `joker-sdk-sys`) for `wasm32-wasip2` with the workspace lock, in `target/tmp/guests`, and add its `joker:meta` section. By hand:

```sh
cargo build --locked --release --target wasm32-wasip2 -p joker-host-fixture-echo
```

The cost of an empty cycle call (docs/01 §21, ≤ 5 µs):

```sh
cargo test -p joker-host --release --test overhead -- --ignored --nocapture
```
