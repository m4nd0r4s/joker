# ADR 0009 — Burn as the default ML backend, not the exclusive one

**Status:** accepted (v1) · **Amended by ADR 0019 (ML in 1.1)**: where it says "Phase 2", read 1.1 · **Date:** 2026-09

## Context

An ML engine is needed that runs on ARM CPUs, GPUs (Vulkan/CUDA), eventually MCUs and Wasm, that fits in a static binary and that allows on-device training. The most likely target hardware includes NPUs (Hailo) that no generic engine supports.

## Decision

**Burn** is the default and reference backend (Flex and CubeCL CPU, Vulkan, CUDA; Metal for development), and the format of the "in-house" models (architecture catalog + weights). The `InferenceBackend` trait exists from the start, with **ORT** and **HailoRT** as second and third backends in 1.1 (ADR 0019), and TensorRT optional.

## Reasons

- Pure Rust, portable, with training and inference in the same code and Flex for `no_std` and Wasm.
- Device selection at runtime enables the fallback chain.
- It does not support NPUs, and burn-onnx covers a limited set of operators. That is why it cannot be exclusive.

## Consequences

- Burn is pre-1.0 with breaking changes: the version is pinned per runtime minor, and the `.jkm` files record it (M21).
- The six validations of document 06, §12.6, are an exit gate of 1.1 (ADR 0019). If the CPU falls more than 2× behind ORT, the default CPU backend changes to ORT and this ADR is revised.
