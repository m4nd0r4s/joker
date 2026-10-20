# joker-core

Signals, registry, process image, arbiter, envelope, alarms and trace (docs/01, docs/02, docs/16). `no_std`; no I/O, no clock reads, no randomness: the clock comes in through `Clock` (ADR 0007).

## Testing

The seqlock of `Slot` is model-checked with [loom](https://docs.rs/loom):

```sh
RUSTFLAGS="--cfg loom" LOOM_MAX_PREEMPTIONS=2 cargo test -p joker-core --release --lib loom
```
