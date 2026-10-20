# ADR 0013 — A telemetria nunca bloqueia o controlo

**Estado:** aceite (v1) · **Data:** 2026-09

## Contexto

Um disco lento, uma rede em baixo ou um broker MQTT sobrecarregado não podem atrasar um tick.

## Decisão

Os serviços consomem o núcleo apenas por filas lock-free limitadas. Filas cheias descartam segundo a política declarada, e cada descarte é contado e exposto como sinal `joker.*`. O store-and-forward lê do historian em vez de manter filas em memória.

## Razões

- O controlo não depende de nada que esteja fora do dispositivo.
- As perdas nunca são silenciosas.

## Consequências

- Em condições extremas perdem-se amostras de telemetria. Os alarmes e o registo de decisões têm a prioridade mais alta e fsync próprio.
