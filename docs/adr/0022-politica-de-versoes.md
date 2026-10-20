# ADR 0022 — Política de versões: minors temáticas, compatibilidade total na 1.x, LTS no fim da série

**Estado:** aceite (plano v1 → v2) · **Data:** 2026-09

## Contexto

Depois da 1.0 há mais trabalho do que cabe numa versão (documento 21): ML, Structured Text, protocolos, segurança, Studio, Wasm em MCUs, Deck em escala, redundância. Os clientes industriais põem um nó em campo e não querem voltar a mexer nele durante anos. É preciso decidir o que pode mudar, quando, e durante quanto tempo uma versão é suportada.

## Decisão

1. **Regra de ouro da 1.x:** uma instalação 1.0 que funciona continua a funcionar, igual, em todas as 1.x. Nenhuma minor quebra apps (`joker:runtime@1.y`), manifestos, locks, classes `std/`, o Joker Link, o protocolo do agente, nem muda um resultado de conformidade existente (documento 16). A única exceção é uma correção de segurança, com ADR própria e migração automática.
2. **Minors temáticas**, uma a cada ~3–4 meses: um tema por versão, no máximo três funcionalidades de cabeça, portas de saída como nas fases da v1. O que não passa as portas passa à versão seguinte.
3. **Os drivers e as classes novas podem sair em qualquer minor**, fora do tema, porque são pontos de extensão e não tocam no núcleo.
4. **Suporte:** cada minor recebe correções até seis meses depois da seguinte; a 1.0 é suportada 12 meses; a **última minor da série 1 é LTS**, com 3 anos de correções de segurança e de defeitos críticos.
5. **O Deck e o Hub têm versões próprias.** A compatibilidade com os nós faz-se pela versão do protocolo do agente, e um Deck suporta nós das três últimas minors e da LTS.
6. **Uma major (v2)** só acontece quando uma mudança necessária não cabe na regra de ouro (caminhos qualificados por nó, árbitro distribuído, mundo WIT novo). Tem sempre `jk migrate`, a 2.0 lê a última 1.x, e as apps do mundo 1 correm por adaptador durante toda a série 2.

## Razões

- A confiança dos clientes industriais vem da previsibilidade, e não da velocidade.
- Os temas e o limite de funcionalidades evitam minors inchadas e atrasadas.
- A separação entre núcleo e pontos de extensão permite responder a pilotos sem quebrar a cadência.
- A LTS dá uma versão para "instalar e esquecer", sem obrigar a manter todas as minors.

## Consequências

- O `jk check` e a CI têm um conjunto de conformidade por versão, que nunca perde casos.
- Cada funcionalidade de cabeça precisa de uma ADR antes de entrar numa minor.
- O documento 21 é atualizado no fim de cada versão.
- A série 2 tem de manter os adaptadores do mundo 1, o que é custo de manutenção aceite.
