# ADR 0002 — As apps são componentes WebAssembly

**Estado:** aceite (v1) · **Data:** 2026-09

## Contexto

As apps precisam de ser portáveis entre arquiteturas, substituíveis a quente, isoladas umas das outras e da runtime, e escritas em mais do que uma linguagem. Containers resolvem o isolamento, mas custam memória, arranque e IPC em cada leitura.

## Decisão

As apps são **componentes** do Component Model no mundo `joker:runtime`, sobre WASI 0.3 (async nativo, `stream<T>`, `future<T>`), executados no wasmtime com AOT obrigatório em produção. Rust é tier 1; TypeScript e Python são tier 2. Apps nativas existem apenas como porta de saída (documento 04, §5).

## Razões

- Um artefacto `.wasm` corre em aarch64, x86_64 e armv7 (neste com Pulley).
- Sandbox sem ambient authority: a app só vê as portas ligadas.
- Hot-swap num limite de tick, com migração de estado.
- Custo de fronteira pequeno com a imagem empacotada (documento 09, ataque 2).
- WASI 0.3 foi publicado em junho de 2026 e o wasmtime 46 implementa a versão final.

## Consequências

- O Component Model ainda evolui, e a política de versões M21 existe por isso.
- Os SDKs de tier 2 embutem um motor JS ou Python (MB e pausas de GC) e ficam limitados a ciclos lentos.
- Alternativas rejeitadas: **containers** (memória e latência); **Lua/scripting embutido** (sem tipos, sem ecossistema); **plugins nativos** (sem isolamento, sem ABI estável).
