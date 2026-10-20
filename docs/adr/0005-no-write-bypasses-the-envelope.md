# ADR 0005 — Nenhum caminho de escrita contorna o envelope

**Estado:** aceite (v1) · **Data:** 2026-09

## Contexto

Os sistemas de edge costumam ter "portas das traseiras": um comando de diagnóstico, uma API de manutenção, um script do integrador. É por essas portas que acontecem os acidentes.

## Decisão

Todas as escritas (apps, CLI, API, pontes, uplinks, intenções aprovadas) passam pelo árbitro e pelo envelope. Os drivers só recebem escritas aplicadas. A única exceção é o bit `force_safe` do supervisor, que só consegue levar ao estado seguro.

## Razões

- A garantia não depende de disciplina de quem integra.
- O registo de decisões fica completo, porque tudo passa pelo mesmo sítio.

## Consequências

- Um técnico que precise de "forçar" um valor fora do envelope tem de alterar o envelope por config (com `widen` justificado e auditado), e não por um comando.
- Invariante 2 do `AGENTS.md`.
