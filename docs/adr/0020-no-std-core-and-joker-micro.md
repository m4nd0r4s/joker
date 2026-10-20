# ADR 0020 — Núcleo `no_std` desde o primeiro dia, e o Joker Micro na v1

**Estado:** aceite (v1) · **Data:** 2026-09

## Contexto

Correr em microcontroladores era a Fase 5, como um produto à parte ("Joker Lite"). Mas é de muito valor para o objetivo do projeto: tempo real duro, amostragem por hardware, custo baixo, coprocessadores de I/O e integração em firmware existente. E a decisão que o torna barato (um núcleo `no_std`, sem alocação, com capacidades fixas) só é barata se for tomada antes da primeira linha de código.

## Decisão

- Os crates do núcleo são `#![no_std]`, sem alocação no tick e com capacidades fixas; o `alloc` é uma feature opcional para o Linux. A lista dos crates `no_std` é a do documento 13, §1 (regra `no_std`).
- A sincronização do núcleo faz-se **só** por `critical-section` e `portable-atomic` (`AtomicU64` para os `seq` onde o hardware não tem atómicos de 64 bits), nunca por primitivas do executor. No modo biblioteca, o integrador fornece a implementação de `critical-section`, ou ativa `cs-single-core` (ref/08 §2.4, regra 8).
- O **Joker Micro** (documento 19) faz parte da v1, na Fase 2, com três modos de entrega (firmware, biblioteca C, crate Rust), o Joker Link e o padrão de coprocessador.
- A conformidade corre nos dois hospedeiros e tem de dar os mesmos bytes.

## Razões

- O mesmo núcleo, a mesma semântica e as mesmas apps em Linux e em MCUs.
- Três limites passam a ter resposta: tempo real duro, amostragem de alta taxa e microcontroladores (documento 12).
- O fallback do dispositivo (M1) passa a ser natural num coprocessador: o `safe` é garantido no próprio MCU.

## Consequências

- A Fase 0 ganha alvos de CI `no_std` (`thumbv8m.main-none-eabihf` e `riscv32imac-unknown-none-elf`, com conformidade em QEMU, a bloquear; Xtensa sem bloquear) e a conformidade no ESP32 da bancada, e num RP2350 quando houver placa (documento 13, §5).
- As estruturas do núcleo não podem crescer em runtime; a capacidade é uma decisão de build.
- Sem Wasm no MCU na v1: as apps compilam nativamente (a mesma fonte), e o `wasmi` fica para a v1.x.
