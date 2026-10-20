# ADR 0025 — Identidade P-256 nos nós, Ed25519 na organização, COSE em tudo o que se assina

**Estado:** aceite (v1) · **Data:** 2026-09

## Contexto

O documento 01 §17 falava só em Ed25519. Mas a identidade de um nó deve viver num TPM 2.0 ou num elemento seguro, e esses suportam P-256, não Ed25519. Ao mesmo tempo, os artefactos, as gerações e os blobs de MCU precisam de assinaturas verificáveis num microcontrolador, e o estado desejado do Deck precisa de uma forma que não dependa do transporte.

## Decisão

1. **Identidade dos nós:** certificados X.509 **P-256** emitidos pela CA da organização, com a chave em TPM ou elemento seguro quando existe. Usada no mTLS da API e do agente, e nas assinaturas feitas pelo próprio nó (`ES256`).
2. **Chaves da organização** (runtime, config, apps, drivers, perfis, placas, classes, firmware, segredos, deck): **Ed25519**, fora dos nós.
3. **Tudo o que se assina é COSE_Sign1 sobre CBOR determinístico** (estado desejado, bundle de confiança, blob de geração, manifesto de OTA, relatórios), mesmo quando viaja por HTTPS. Os artefactos OCI levam também assinaturas compatíveis com cosign ou notation.
4. **Confiança:** um bundle assinado por uma raiz fora de linha diz que chave assina que papel (um TUF reduzido). O `[security.trust]` do manifesto só **restringe** esse bundle, nunca acrescenta chaves.
5. O nó continua a controlar com o certificado expirado, revogado ou em falta: a identidade serve para falar com fora, não para decidir sobre saídas (documento 20, princípio 5).

O detalhe está na ref/02 §2.5, na ref/10 §2.4 e na ref/08 §2.13.

## Consequências

- Duas curvas na base de código (P-256 e Ed25519); as duas têm implementações `no_std` maduras.
- O documento 01 §17 e o 03 §13 são atualizados.
