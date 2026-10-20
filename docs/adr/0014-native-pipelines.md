# ADR 0014 — Pipelines nativos para dados volumosos

**Estado:** aceite (v1) · **Alterada por ADR 0019 (ML na 1.1)**: streams, sinais em bloco e pipelines são da 1.1 · **Data:** 2026-09

## Contexto

Frames e formas de onda a passar pela memória Wasm custam cópias de dezenas a centenas de MB/s e duplicam o trabalho de pré-processamento em cada app.

## Decisão

Streams e sinais em bloco vão para modelos por pipelines declarados no manifesto, executados nativamente (CPU NEON/AVX, ou GPU quando o modelo está em GPU), com zero-copy desde o driver (dmabuf/memfd). As apps recebem eventos ou sinais pequenos (detecções, features, scores).

## Razões

- O caminho quente não passa pelo Wasm.
- As operações de pipeline são reutilizáveis e testadas uma vez.

## Consequências

- Um conjunto finito de operações. O que não existe tem de ser acrescentado como `PipelineOp` (nativo) ou feito na app, com o custo da cópia.
