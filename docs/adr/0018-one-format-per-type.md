# ADR 0018 — Um formato por tipo de coisa: TOML, `.jkc` e código

**Estado:** aceite (v1) · **Data:** 2026-09

## Contexto

O projeto acumulou vários tipos de ficheiro (documento 15). As classes de equipamento (ADR 0017) começaram em TOML e expuseram o problema: expressões dentro de strings, métodos em inline tables, erros maus. Havia três caminhos: TOML/YAML para tudo, uma DSL, ou código (Rust, TS, Python) como fonte.

## Decisão

Cada ficheiro tem o formato do **tipo de coisa** que descreve:

| Tipo | Formato | Exemplos |
|---|---|---|
| configuração (dados que o `jk check` analisa, fundidos por overlays, revistos em PR) | **TOML** | manifesto, overlays, lock, perfis, placas, modelos de simulação, cenários, conformidade |
| contratos (interfaces partilhadas entre linguagens e ferramentas) | **`.jkc`** (documento 17) | classes de equipamento |
| comportamento | **código** (Rust tier 1; TS e Python tier 2), sempre em Wasm | apps, implementações ativas, extension traits, classes locais |

YAML não é usado em lado nenhum. Configuração em código é recusada: o que o Joker tem de analisar tem de ser dados. Para gerar configurações em escala, usa-se qualquer linguagem que **emita** TOML.

## Razões

- A configuração tem de ser analisável, fundível e legível por quem não programa.
- Um contrato tem de ser neutro (várias linguagens, perfis, CLI, HMI, OPC UA) e ter sintaxe para métodos, ações e consultas; a `.jkc` reutiliza o parser de expressões que já existe.
- O comportamento precisa de liberdade total, e o sandbox Wasm torna-a segura.

## Consequências

- Um parser, um formatador e um LSP para `.jkc` (`jk class fmt|check`), e um LSP para os TOML com suporte às expressões embebidas.
- As classes locais em crates Rust continuam válidas para uso dentro de apps; publicá-las no registo exige escrever o `.jkc` (sem extração automática na v1).
- Validação obrigatória na Fase 1: escrever as classes de `std/` em `.jkc`; se a DX não for claramente melhor, a decisão é revista numa ADR nova.
