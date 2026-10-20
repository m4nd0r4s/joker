# ADR 0016 — Um processo, não microserviços

**Estado:** aceite (v1) · **Alterada por ADR 0019 (ML na 1.1)**: os processos de modelos (`isolation = "process"`) são da 1.1 · **Data:** 2026-09

## Contexto

Plataformas como a EdgeX dividem a runtime em vários serviços em containers. Isso dá isolamento, mas custa memória, latência e complexidade de operação num dispositivo pequeno.

## Decisão

Um processo `jokerd` com threads por domínio; isolamento por sandbox Wasm (apps e drivers W) e por processos auxiliares apenas onde o código não é de confiança ou pode encravar (drivers X, modelos com `isolation = "process"`).

## Razões

- Uma leitura vai do driver à app sem IPC.
- Um só artefacto para atualizar, monitorizar e depurar.

## Consequências

- Um bug de memória num driver N derruba o processo. Mitigado por Rust sem `unsafe` fora de `*-sys`, pelo supervisor do systemd, pelo watchdog de hardware e pelo fallback do dispositivo.
