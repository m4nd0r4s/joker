# ADR 0001 — Rust para a runtime

**Estado:** aceite (v1) · **Data:** 2026-09

## Contexto

A runtime corre em dispositivos com 512 MB a 8 GB, durante anos sem reinício, com uma parte em tempo real. Precisa de cross-compilação simples para aarch64, armv7 e x86_64, e de um binário único sem dependências de sistema pesadas.

## Decisão

O `jokerd`, a `jk`, os drivers tier N e os backends de ML são escritos em Rust (edition 2024), com `unsafe` confinado aos crates `*-sys` e à FFI do host.

## Razões

- Sem GC: latência previsível no domínio de ciclo, e memória controlada.
- Segurança de memória num processo que junta drivers, rede e ML.
- O ecossistema já existe onde importa: `tokio`, `wasmtime`, `tokio-modbus`, `async-opcua`, `ethercrab`, `socketcan`, `rumqttc`, `zenoh` e o **Burn**.
- Cross-compilação e binários estáticos com `cargo` e `cross`.

## Consequências

- Alternativas rejeitadas: **C/C++** (a produtividade e a segurança de memória custam mais do que o que se ganha); **Go** (GC no caminho de ciclo, e cgo para metade dos drivers); **Zig** (ecossistema industrial inexistente).
- O custo é a curva de aprendizagem para contribuidores de automação, e é por isso que as apps não obrigam a Rust (ADR 0002).
