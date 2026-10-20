# ADR 0023 — As interfaces fecham-se em referências normativas (`docs/ref/`)

**Estado:** aceite (v1) · **Data:** 2026-09

## Contexto

Os documentos por área (03 manifesto, 04 SDK, 05 drivers, 19 Joker Micro, 20 Deck) explicam o desenho com exemplos, mas não eram exaustivos: havia chaves usadas nos exemplos sem definição, mensagens sem campos, erros sem código e dois documentos a dizer coisas diferentes sobre o mesmo campo. Implementar a partir deles obrigaria a inventar detalhe durante a implementação, e cada equipa inventaria o seu.

## Decisão

1. Cada interface tem uma **referência exaustiva** em `docs/ref/`, com a representação comum fixada em `ref/00-conventions.md` (documento 22).
2. **Precedência:** `ref/00` tem o nível do documento 16. `ref/01`–`ref/10` ficam **acima** dos documentos por área, que passam a ser a explicação e remetem para a referência no detalhe. A ordem completa está em `docs/README.md`.
3. Os ficheiros máquina-legíveis (`wit/`, `spec/cddl/`, `spec/openapi/`, `spec/c/`) são parte da referência: se um ficheiro e o texto divergem, é um bug a corrigir nos dois, e a CI da Fase 0 (tarefa 0a, documento 22, §8) impede que isso aconteça em silêncio.
4. Mudar uma interface é mudar a referência e o ficheiro máquina-legível no mesmo conjunto de commits, com ADR se tocar num contrato público (ADR 0022).

## Razões

- A implementação só trabalha sobre specs; as dúvidas resolvem-se na referência, não no código.
- Os ficheiros máquina-legíveis geram código (bindings WIT, clientes do OpenAPI, validação CBOR) e testes de conformidade.

## Consequências

- Os documentos por área encolhem no detalhe e ganham remissões.
- As referências são grandes (cerca de 1,2 MB de texto) e precisam de dono por frente (documento 13).
- A tarefa 0a da Fase 0 compila e valida as referências antes de qualquer funcionalidade.
