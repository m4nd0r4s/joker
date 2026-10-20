# ADR 0009 — Burn como backend de ML por omissão, não exclusivo

**Estado:** aceite (v1) · **Alterada por ADR 0019 (ML na 1.1)**: onde diz "Fase 2", lê-se 1.1 · **Data:** 2026-09

## Contexto

É preciso um motor de ML que corra em CPU ARM, GPU (Vulkan/CUDA), eventualmente MCU e Wasm, que caiba num binário estático e que permita treino no dispositivo. O hardware alvo mais provável inclui NPUs (Hailo) que nenhum motor genérico suporta.

## Decisão

O **Burn** é o backend por omissão e de referência (Flex e CubeCL CPU, Vulkan, CUDA; Metal para desenvolvimento), e o formato dos modelos "da casa" (catálogo de arquiteturas + pesos). O trait `InferenceBackend` existe desde o início, com **ORT** e **HailoRT** como segundo e terceiro backends na 1.1 (ADR 0019), e TensorRT opcional.

## Razões

- Rust puro, portável, com treino e inferência no mesmo código e Flex para `no_std` e Wasm.
- A seleção de dispositivo em runtime permite a cadeia de fallback.
- Não suporta NPUs e o burn-onnx cobre um conjunto limitado de operadores. Por isso não pode ser exclusivo.

## Consequências

- O Burn é pré-1.0 com breaking changes: fixa-se a versão por minor da runtime, e os `.jkm` registam-na (M21).
- As seis validações do documento 06, §12.6, são porta de saída da 1.1 (ADR 0019). Se o CPU ficar mais de 2× atrás do ORT, o backend por omissão em CPU muda para ORT e esta ADR é revista.
