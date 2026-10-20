# Joker

A runtime coringa para dispositivos de edge. O mesmo núcleo em Linux (Raspberry Pi, RevPi, PC industrial) e em microcontroladores (Joker Micro), e uma só abstração: **sinais tipados com unidade, qualidade e tempo**. Controla I/O, fala protocolos industriais e recolhe telemetria, com um envelope de segurança nativo entre cada app e o hardware; o ML chega na 1.1. Faz parte de uma família com o registo (Hub) e o plano de controlo da frota (Deck).

**Estado:** documento de desenho. Nada está implementado. Os números marcados como *alvo* são estimativas a validar nas Fases 0 a 2 (`docs/11-roadmap.md`).

## Estrutura proposta

```
docs/            especificação de desenho — a fonte da verdade (começa em docs/README.md)
  adr/           decisões de arquitetura; mudar o desenho = escrever uma ADR
  ref/           referências normativas de todas as interfaces (começa em ref/00-convencoes.md)
  exemplos/      percurso guiado e projetos completos
  trilha/        trilha de estudo, 11 partes na bancada do documento 23: A (sinais e o nó Linux: LED, temperatura, ESP32 por Modbus, falhas, Pi Zero W), B (classes: std/ e a tua), C (Joker Micro no ESP32), D (frota: prod, segredos, Deck)
crates/          workspace Rust (a criar na Fase 0 — ver docs/13-plano-de-implementacao.md)
  joker-units      unidades, dimensões, grandezas
  joker-core       sinais, imagem de processo, envelope, árbitro, expressões — sem I/O
  joker-expr       linguagem de expressões do manifesto
  joker-manifest   parser e validação de joker.toml, joker.lock; registo de códigos
  joker-hal        traits de driver, planos de leitura, transportes (no_std)
  joker-drivers/*  modbus, opcua, gpio, i2c, spi, can, canopen, ethercat, revpi, mqtt, sim, v4l2…
  joker-driver-sdk SDK de drivers tier W (Wasm) em Rust
  joker-jdp        Joker Driver Protocol: drivers tier X (processos)
  joker-drvtest    suite de conformidade de drivers
  joker-host       host wasmtime, mundo WIT, capacidades
  joker-sdk        SDK de apps em Rust (guest)
  joker-test       harness de testes com relógio virtual
  joker-micro      runtime para microcontroladores (Embassy); firmware, biblioteca C (módulo ffi), crate
  joker-link-proto protocolo Joker Link (no_std)
  joker-jkc        compilador de classes .jkc
  joker-ml         trait de backend, artefacto .jkm, pipelines (v1.1)
  joker-ml-burn    backend Burn (por omissão)
  joker-ml-ort     backend ONNX Runtime
  joker-ml-hailo   backend HailoRT (plugin)
  joker-store      historian local e fila store-and-forward
  joker-link       uplinks: MQTT/Sparkplug B, Zenoh, OTLP
  joker-api        API local (HTTP + WebSocket)
  joker-client     biblioteca cliente Rust da API local (a que a jk usa)
  joker-agent      agente nó ↔ Deck (agent/1)
  joker-agent-proto mensagens do agente (no_std, partilhadas com os MCUs autónomos)
  joker-spec-tests compila e valida as referências de docs/ref/ contra o código (Fase 0, tarefa 0a)
  jokerd           o daemon
  joker-cli        a CLI (binário jk)
wit/             os mundos WIT: joker:runtime (apps), joker:driver (drivers tier W), joker:sim
spec/            ficheiros máquina-legíveis das referências
  cddl/          esquemas CBOR: joker-meta, joker-link, jdp, agent, joker-micro, secrets
  openapi/       joker-api.yaml (API HTTP local, OpenAPI 3.1)
  c/             joker_node.h (ABI do Joker Micro), jdp.h (drivers tier X), exemplo
conformance/     casos de conformidade do envelope, do árbitro e da imagem de processo, um ficheiro por suite
```

## Desenvolver (quando existir código)

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Regras de trabalho, para pessoas e agents: `AGENTS.md`.
