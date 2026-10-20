# ADR 0010 — Manifesto declarativo em TOML e `joker.lock`

**Estado:** aceite (v1) · **Data:** 2026-09

## Contexto

A configuração de um site é revista por pessoas de automação e de software, vive em Git e é multiplicada por centenas de sites. Uma parte não é declarável (mapas descobertos, firmware, hashes).

## Decisão

`joker.toml` em TOML, com quantidades com unidade em strings, JSON Schema publicado e overlays (`extends`). Tudo o que é resolvido ou descoberto vai para o `joker.lock`, versionado, com deteção de deriva.

## Razões

- TOML é conhecido, revisível em PR e bem suportado por editores.
- O lock dá reprodutibilidade (o mesmo par manifesto + lock produz o mesmo sistema) e apanha trocas de firmware.

## Consequências

- Alternativas rejeitadas: **YAML** (ambiguidades de tipos); **uma DSL própria** (mais uma linguagem para aprender; fica reservada às expressões); **configuração em código** (menos revisível por quem não programa).
