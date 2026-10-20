# ADR 0011 — Árbitro com prioridades e leases

**Estado:** aceite (v1) · **Data:** 2026-09

## Contexto

Várias fontes querem escrever na mesma saída: operador, proteção de software, supervisor, controlador, otimizador, ML. Sem regra explícita, ganha quem escreveu por último, e isso é o problema "o operador mexeu, o modelo desfez".

## Decisão

16 níveis (1 é o mais alto) com faixas nomeadas; cada escritor ocupa um nível declarado; cada escrita tem lease; vence o lease ativo de maior prioridade; sem nenhum ativo, vai para `safe`; os níveis manuais têm TTL obrigatório.

## Razões

- É o modelo do *priority array* do BACnet, testado em décadas de gestão técnica de edifícios, com leases acrescentados para cobrir escritores que morrem.
- O lease é o homem-morto de cada escritor.

## Consequências

- Uma saída com vários escritores obriga cada um a declarar nível e lease (`J0211`). Com um só escritor há um valor por omissão (`control:12`, lease de 3 períodos), para o manifesto mínimo continuar pequeno.
- Com equipamentos que também têm prioridades (BACnet), o nível no equipamento é declarado por dispositivo.
