# ADR 0012 — Shadow mode as the only promotion path for what actuates

**Status:** accepted (v1) · **Amended by ADR 0019 (ML in 1.1)**: the shadow of apps is in 1.0; that of models, in 1.1 · **Date:** 2026-09

## Context

New models and apps that actuate on a real process cannot be validated offline only, and "we turn it on and see" is not acceptable.

## Decision

A new version of a model or app with outputs first runs in shadow: same inputs, writes recorded as `Shadowed`, comparison with the current version or with the existing system. Promotion is done by declared criteria (automatic or manual), may be partial by outputs and has automatic demotion by KPI. In `prod`, promoting a model that actuates without going through shadow requires `--skip-shadow "<justification>"`, which is audited.

## Reasons

- It turns trust into evidence.
- The shadow traces are also evaluation data.

## Consequences

- CPU cost (two versions running) and time cost until promotion.
- Shadow does not catch closed-loop effects (the version in shadow does not see the consequences of its own decisions). It is a known limitation, and partial promotion mitigates it.
