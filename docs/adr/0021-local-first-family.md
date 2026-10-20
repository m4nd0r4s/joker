# ADR 0021 — A product family, local-first, with open contracts

**Status:** accepted (v1) · **Date:** 2026-09

## Context

Managing hundreds or thousands of nodes and MCUs requires a control plane (inventory, desired state, rollouts, observability). It was a declared limit ("it is not the fleet server"), but there is great value in building it together with the runtime.

## Decision

Joker is a family: **Joker** (Linux), **Joker Micro** (MCUs), **Joker Hub** (artifact registry), **Joker Deck** (control plane) and **Joker Studio** (commissioning). Principles (document 20, §2): local-first; the Deck is never in the control path; open contracts; Git as the source of truth for configuration; node security does not depend on the Deck. The agent protocol and the Deck MVP enter v1 (Phase 3).

## Reasons

- The node remains useful on its own, and the Deck adds scale.
- The validation crates are shared between `jk` and the Deck: a valid generation is the same in both.

## Consequences

- One person dedicated to the Deck from Phase 3 on.
- The agent protocol becomes a versioned public interface (M21 policy).
- The business model (open runtime, Deck as a product, or another) is a decision tied to the license (document 18, §6).
