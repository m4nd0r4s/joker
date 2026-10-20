# ADR 0014 — Native pipelines for bulk data

**Status:** accepted (v1) · **Amended by ADR 0019 (ML in 1.1)**: streams, block signals and pipelines are in 1.1 · **Date:** 2026-09

## Context

Frames and waveforms passing through Wasm memory cost copies of dozens to hundreds of MB/s and duplicate the preprocessing work in each app.

## Decision

Streams and block signals go to models through pipelines declared in the manifest, executed natively (CPU NEON/AVX, or GPU when the model is on GPU), with zero-copy from the driver (dmabuf/memfd). Apps receive events or small signals (detections, features, scores).

## Reasons

- The hot path does not go through Wasm.
- Pipeline operations are reusable and tested once.

## Consequences

- A finite set of operations. What does not exist must be added as a `PipelineOp` (native) or done in the app, with the cost of the copy.
