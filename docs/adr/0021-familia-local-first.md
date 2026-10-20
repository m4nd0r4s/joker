# ADR 0021 — Uma família de produtos, local-first, com contratos abertos

**Estado:** aceite (v1) · **Data:** 2026-09

## Contexto

Gerir centenas ou milhares de nós e MCUs exige um plano de controlo (inventário, estado desejado, rollouts, observabilidade). Era um limite declarado ("não é o servidor de frota"), mas é de muito valor construí-lo em conjunto com a runtime.

## Decisão

O Joker é uma família: **Joker** (Linux), **Joker Micro** (MCUs), **Joker Hub** (registo de artefactos), **Joker Deck** (plano de controlo) e **Joker Studio** (comissionamento). Princípios (documento 20, §2): local-first; o Deck nunca está no caminho de controlo; contratos abertos; o Git como fonte da verdade da configuração; a segurança do nó não depende do Deck. O protocolo do agente e o MVP do Deck entram na v1 (Fase 3).

## Razões

- O nó continua útil sozinho, e o Deck acrescenta escala.
- Os crates de validação são partilhados entre a `jk` e o Deck: uma geração válida é a mesma nos dois.

## Consequências

- Uma pessoa dedicada ao Deck a partir da Fase 3.
- O protocolo do agente passa a ser uma interface pública versionada (política M21).
- O modelo de negócio (runtime aberta, Deck como produto, ou outro) é uma decisão ligada à licença (documento 18, §6).
