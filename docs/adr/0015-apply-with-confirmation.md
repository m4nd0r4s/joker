# ADR 0015 — Apply configuration with confirmation and automatic rollback

**Status:** accepted (v1) · **Date:** 2026-09

## Context

A new configuration can cut the connection to the device itself (a network, a route, a certificate), and at a remote site that means a trip on site.

## Decision

`jk apply --confirm <window>` applies the new generation and waits for a `jk confirm`. Without confirmation within the window, the previous generation is restored automatically. In `prod`, the window is required for generations that touch networking, security, uplinks or the local API itself.

## Reasons

- It is the `commit confirmed` of network equipment, applied to the edge.
- Immutable generations make rollback trivial.

## Consequences

- A second step in every deploy. It can be automated by the fleet agent with a health check.

## Note (API round, 2026-09)

- **Confirmation reasons.** The API plan says why it asks for confirmation (`confirm_reasons`: `network`, `security`, `uplinks`, `api`). The `api` reason is new: a generation that changes the API's listeners or roles can cut off the very `jk confirm`. In `prod`, activating without a window a generation with any of these reasons is `J-E1610` (ref/09 §5.13, S7).
- **Rollback reactivates.** The restored generation is the previous one, with the same hash and the same number; the history records the activation with its origin (document 22, D9).
- **Confirmation in the agent.** With the Deck, the confirmation policy goes in the desired state (ref/10 §2.6.4): `auto` (the node confirms itself from local health), `deck` (waits for a `confirm` command) or `manual` (waits for a `jk confirm`). In `auto`, local health is not enough: confirmation also requires an authenticated exchange with the Deck after the swap (`require_uplink`, H7). A generation that isolated the node never confirms itself, and the window expires with rollback (ref/10 §3.5). The API and the agent share the same generation-transition mutex.
