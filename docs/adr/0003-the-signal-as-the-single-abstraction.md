# ADR 0003 — O sinal é a abstração única

**Estado:** aceite (v1) · **Data:** 2026-09

## Contexto

Cada protocolo tem o seu modelo (registos, nós, objetos, PDOs, pinos). Se as apps falarem protocolos, deixam de ser portáveis, e cada uma reimplementa escala, unidades e qualidade.

## Decisão

Tudo o que as apps veem é um **sinal**: valor tipado, unidade, qualidade (código + motivo) e tempo (`ts_src`, `ts_acq`, `seq`), com as variantes bloco, stream e evento, e ações para comandos. Os drivers entregam valores crus, e a semântica (escala, unidade, qualidade) é aplicada pelo núcleo.

## Razões

- Portabilidade das apps entre sites por ligações.
- A qualidade como cidadã de primeira classe elimina a família de bugs "valor velho tratado como bom".
- Unidades verificadas em compilação eliminam a família "W em vez de kW".

## Consequências

- Perde-se detalhe de protocolo em dois pontos (sessões por utilizador e histórico no equipamento), que ficam declarados (documento 12, §14).
- Comandos e escritas coerentes exigiram M19 e M20 (documento 09, ataque 1).
