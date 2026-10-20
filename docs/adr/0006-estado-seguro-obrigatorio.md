# ADR 0006 — Estado seguro obrigatório e sem valor por omissão

**Estado:** aceite (v1) · **Data:** 2026-09

## Contexto

O que é "seguro" depende do processo: 0 kW numa bateria, fechado numa válvula de gás, aberto numa válvula de alívio, "devolver ao SGTC" num edifício, "ejetar" numa linha de inspeção. Qualquer valor por omissão está errado para alguém.

## Decisão

Cada saída declara `safe` (um valor ou `relinquish`). Faltar é `J0201`. O mesmo princípio aplica-se a `on_late` (`J0206`). O fallback do próprio equipamento é declarado no perfil e verificado face ao `safe` (M1).

## Razões

- Obriga quem configura a pensar no modo de falha de cada saída, no momento em que a declara.
- Torna as três camadas de estado seguro (lease, supervisor, equipamento) coerentes.

## Consequências

- Mais uma linha por saída no manifesto. Os perfis podem sugerir um valor, mas o manifesto tem de o aceitar explicitamente.
