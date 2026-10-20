# ADR 0004 — Imagem de processo, dois domínios de execução, duas instâncias por app

**Estado:** aceite (v1) · **Data:** 2026-09

## Contexto

Controlo precisa de periodicidade e consistência. ML, rede e I/O lento precisam de async. Misturar os dois na mesma thread ou na mesma instância Wasm traz inversão de prioridade e jitter.

## Decisão

- **Domínio de ciclo:** threads `SCHED_FIFO` por grupo, com latch → tarefas → árbitro → envelope → commit, sem alocação, sem bloqueio e sem `await`.
- **Domínio async:** tokio, com leituras do último valor e escritas como propostas para o grupo dono.
- **Duas instâncias do mesmo componente** por app (ciclo e async), que partilham apenas sinais internos, caixas de correio e eventos.

## Razões

- Consistência de snapshot dentro do tick, e escritas aplicadas de uma vez.
- O async não consegue atrasar o ciclo por construção.
- O isolamento entre instâncias dá robustez (um trap async não mata o ciclo).

## Consequências

- Custo de DX: o estado não é partilhado implicitamente, e o SDK tem de o tornar óbvio (documento 04, §3.4).
- Alternativas rejeitadas: uma instância com mutex; async a correr na thread RT entre ticks; threads Wasm partilhadas (ainda não estáveis). Detalhe no documento 09, ataque 6.
