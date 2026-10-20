# ADR 0008 — Drivers em três tiers

**Estado:** aceite (v1) · **Data:** 2026-09

## Contexto

Os drivers são o maior investimento. Uns são críticos e universais (Modbus), outros são nichos de terceiros, e outros ainda dependem de SDKs binários de fabricantes.

## Decisão

- **N** (nativo, Rust, compilado para dentro): protocolos de base.
- **W** (Wasm, com transportes fornecidos): protocolos de terceiros.
- **X** (processo externo, JDP): SDKs binários e código não confiável.

## Razões

- O essencial é rápido e seguro; o ecossistema cresce sem tocar no `jokerd`; os blobs ficam isolados.
- Sem plugins Rust dinâmicos (ABI instável).

## Consequências

- Para acrescentar um driver N é preciso um build da runtime (`jk runtime build --features`).
- O JDP é um protocolo a manter e a versionar.
