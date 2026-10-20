# ADR 0007 — O relógio é um input do núcleo

**Estado:** aceite (v1) · **Data:** 2026-09

## Contexto

Replay determinista, testes com relógio virtual e análise de incidentes exigem que "o mesmo traço dê o mesmo resultado". Uma chamada ao relógio do sistema dentro da lógica torna isto impossível.

## Decisão

O `joker-core` e o `joker-expr` recebem o tempo por um trait `Clock`. As apps recebem `now()` do latch do tick. O relógio de parede tem qualidade (M14).

## Razões

- Replay exato e testes com dias simulados em segundos.
- Timestamps corretos mesmo depois de saltos de NTP.
- É a mesma decisão do Cravo (ADR 0007 do Cravo).

## Consequências

- A disciplina interna é forte: `std::time` está proibido nesses crates, e isso é verificado com um lint.
- As apps que precisam de hora certa declaram `requires_clock = "synced"`.
