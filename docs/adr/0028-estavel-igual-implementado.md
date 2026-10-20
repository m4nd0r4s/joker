# ADR 0028 — Estável = implementado

**Estado:** aceite (v1, 2026-10-21; documento 26, §5 e §9) · **Data:** 2026-09

## Contexto

A especificação da v1.0 é muito mais larga do que a equipa prevista consegue implementar no calendário do documento 11 (documento 26, §5: ~350–435 pessoas-semana contra ~155). E a regra de ouro da ADR 0022 congela, durante toda a série 1, tudo o que a 1.0 declara estável. Declarar estáveis centenas de superfícies que nunca foram implementadas é garantir erros de desenho permanentes.

## Decisão

1. Na 1.0 só é **estável** o que foi implementado e passou a sua porta de saída.
2. O resto fica **reservado**, com os mecanismos que o ML já usa: `@unstable` no WIT, chave reservada no manifesto (código de "reservado para uma versão futura", como o `J0290`), `x-joker-since` > 1.0 no OpenAPI (a operação responde `J-E1017`), identificador de mensagem reservado no Joker Link e no agente, `status = "reserved"` no registo de códigos.
3. Uma superfície reservada entra numa minor quando é implementada; nada do que é estável muda. A regra de ouro fica intacta.
4. Tem de estar certo logo na 1.0, porque fica no campo sem atualização por software: o formato do traço, as camadas L1–L3 do Joker Link com negociação no `HELLO`, o recetor de OTA no firmware do MCU, o núcleo mínimo `agent/1.0`, o subconjunto estável do WIT, o formato do lock, o espaço de nomes dos códigos e os tipos de media.
5. O corte inicial é o da tabela do documento 26, §5, a reavaliar no fim da M0 com números reais.

## Consequências

- O documento 11 é refeito: nova base de calendário, portas de piloto à escala da bancada, Deck e Hub fora do caminho crítico da runtime.
- As referências marcam as superfícies reservadas (trabalho mecânico, a fazer quando a ADR for aceite).
- Especificar continua a ser barato; congelar passa a exigir implementação.
