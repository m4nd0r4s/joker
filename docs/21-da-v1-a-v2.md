# 21 — Da v1 à v2

O documento 11 leva o projeto até à **v1.0**. Este documento continua a partir daí: o que entra em cada versão **1.x**, porquê por essa ordem, como se decide o que entra, e o que faz de uma versão uma **v2**. A política de versões e de suporte é a da ADR 0022.

Os prazos são estimativas relativas ao início da Fase 0 (mês 0), para a equipa do documento 11. Servem para ordenar, não para prometer.

---

## 1. As regras do jogo

### 1.1 O que é uma 1.x e o que é uma 2.0

| | 1.x (minor) | 2.0 (major) |
|---|---|---|
| apps compiladas para `joker:runtime@1.y` | correm em qualquer 1.x com x ≥ y | correm na 2.0 por adaptador, durante toda a série 2 |
| manifestos e locks 1.y | aceites; `jk migrate` é opcional | `jk migrate` obrigatório; a 2.0 lê a 1.última |
| classes `.jkc` | só mudanças minor (documento 17, §9) | pode haver majors de classes `std/` |
| Joker Link e protocolo do agente | compatíveis nos dois sentidos dentro da 1.x | nova versão do protocolo, com negociação e suporte à anterior |
| semântica do núcleo (documento 16) | **nunca muda** um resultado de conformidade existente; só acrescenta casos | pode mudar, com cada diferença numa ADR e casos novos |

A regra de ouro: **uma instalação 1.0 que funciona continua a funcionar, igual, em todas as 1.x.** Tudo o que a quebrasse espera pela v2.

### 1.2 Como se decide o que entra numa 1.x

1. **Um tema por versão**, com no máximo três funcionalidades de cabeça. O resto espera.
2. **A procura dos pilotos decide a ordem** dentro do que é compatível com a regra de ouro. Um driver pedido por um cliente pode entrar numa versão anterior, porque os drivers são pontos de extensão (tier N, W ou X) e não tocam no núcleo.
3. **Cada versão tem portas de saída**, como as fases da v1. Sem as portas, a versão não sai, e as funcionalidades passam à seguinte.
4. **Nada entra sem documento.** Cada funcionalidade de cabeça tem uma ADR, e as mudanças nos documentos normativos entram no mesmo conjunto de commits.

### 1.3 Cadência e suporte (ADR 0022)

- Uma minor a cada **~3–4 meses**, depois da 1.0.
- Cada minor recebe correções até **seis meses depois da minor seguinte**.
- **A 1.0 é suportada 12 meses**, porque é a que os primeiros clientes industriais põem em campo.
- **A última minor da série 1 é LTS:** 3 anos de correções de segurança e de defeitos críticos, sem funcionalidades. É a versão que um cliente pode deixar numa subestação sem mexer.
- O **Deck e o Hub** têm versões próprias; a compatibilidade com os nós é pela versão do protocolo do agente, e um Deck suporta por inteiro nós das **três últimas minors e da LTS**. Um nó 1.x mais antigo recebe só o **núcleo mínimo** do protocolo (`hello`, `poll` e `desired`, `report`, `events`, `renew`, e os comandos `confirm`, `reject` e `report_full`, na forma da `agent/1.0`, que nunca muda na série 1), e isso chega para lhe mandar uma runtime nova: um nó nunca fica sem caminho para ser atualizado (ref/10 §3.14).

---

## 2. Visão geral

```
 mês:   0          11        15         19          23          27         31          ~36
        ├── v1 ────┤ 1.1 ────┤ 1.2 ─────┤ 1.3 ──────┤ 1.4 ──────┤ 1.5 ──────┤ 2.0 ───────┤
        núcleo,     ML        indústria   segurança    Micro+      frota em    o site como
        Linux,                e ecossis-  e operação               escala      um sistema
        Micro,                tema                                  (LTS)
        Deck MVP
```

| Versão | Tema | Cabeça de cartaz |
|---|---|---|
| **1.0** | a runtime coringa | núcleo `no_std`, Linux, Joker Micro, classes, Deck MVP |
| **1.1** | inteligência | ML (Burn, ORT, Hailo), streams e pipelines, sinais em bloco |
| **1.2** | indústria e ecossistema | Structured Text, SDK de drivers tier W, BACnet e EtherCAT |
| **1.3** | segurança e operação | modo gateway e protocolos seguros, Joker Studio, operação remota no Deck |
| **1.4** | Micro+ | Wasm em MCUs, ML em MCUs, nós de baixo consumo |
| **1.5** (LTS) | frota em escala | Hub público, observabilidade e auditoria da frota, MCUs autónomos em escala |
| **2.0** | o site como um sistema | redundância ativo/standby, malha de nós, mundo WIT 2 |

---

## 3. v1.1 — Inteligência (≈ meses 11–15)

O ML adiado da v1 (ADR 0019), já definido no documento 11 e detalhado no documento 06.

| Área | Conteúdo | Documento |
|---|---|---|
| backends | Burn (Flex, CubeCL CPU, Vulkan, CUDA), ORT, HailoRT | 06 §4, §12 |
| artefactos | `.jkm`, linhagem (M10), OTA de modelos pelo Deck | 06 §2, §11 |
| dados volumosos | streams (M4), pipelines nativos (M5), escritas agendadas (M6) | 01 §11, 06 §5, 02 §7.2 |
| formas de onda | sinais em bloco (M7), com aquisição de alta taxa no Joker Micro | 01 §11, 19 U3 |
| confiança | sombra e promoção de modelos (M16), guarda de entrada e controlador de fallback (M17), saídas não deterministas no traço (M22) | 06 §7, §8, §10 |
| LLMs | escritores só-proposta e intenções com aprovação (M18), servidores de modelo tier X | 01 §6.1, 06 §13 |
| aprendizagem | treino no dispositivo (Burn) | 06 §9 |
| drivers | `v4l2`/`libcamera`, `spi-adc` (Linux) | 05 §10 |

**Portas de saída:** as bancadas de visão e de vibração (documento 08, rodadas 2 e 3); as seis validações do Burn (documento 06, §12.6); um modelo promovido em campo só por sombra.

**Plano B:** se a validação do Burn falhar em CPU, a 1.1 sai com ORT como backend por omissão em CPU e o Burn para GPU e treino (ADR 0009).

---

## 4. v1.2 — Indústria e ecossistema (≈ meses 15–19)

O objetivo é que um integrador de automação consiga usar o Joker **sem aprender Rust**, e que terceiros consigam escrever drivers **sem mexer no `jokerd`**.

| Área | Conteúdo | Porquê agora |
|---|---|---|
| **Structured Text** | frontend IEC 61131-3 (ST) como linguagem tier 2, compilado para Wasm (Linux) e nativo (Joker Micro); as classes aparecem como blocos funcionais | é a língua do público de automação (documento 12, §17) |
| **SDK de drivers tier W** | mundo `joker:driver` estável (`@since(1.2.0)`; na 1.0 e na 1.1 é `@unstable`, só para drivers da árvore, ref/06 §4), aberto a terceiros, SDK Rust e C, testes de conformidade de drivers, publicação no Hub | abre o catálogo ao ecossistema (documento 05, §8) |
| **SDK Python** | tier 2, para lógica lenta e integrações | pedido comum de equipas de dados |
| protocolos de edifícios | **BACnet/IP** (tier X e nativo; nenhum tier de BACnet está na 1.0) e BACnet/SC | rodada 6 do documento 08 |
| protocolos de máquinas | **EtherCAT** (`ethercrab`), GigE Vision (tier X) | rodada 4; visão da 1.1 |
| protocolos de energia | **IEC 60870-5-104**, **IEC 61850 MMS** (tier X), **DLMS/COSEM** (tier W), **EEBus** | subestações, contadores, cargas controláveis |
| classes | `std/` alargada para além da lista inicial da 1.0 (documento 14, §10): `meter.dc`, `battery.bms` e as que um protocolo novo pedir, cada uma com ADR | alinhadas com as normas do documento 14, §10 |

**Portas de saída:** um engenheiro de automação sem Rust faz a trilha de estudo em ST (partes 1–7); três drivers tier W escritos por pessoas de fora da equipa do núcleo, publicados no Hub; um site de edifício real com BACnet em piloto.

---

## 5. v1.3 — Segurança e operação (≈ meses 19–23)

A pergunta de todos os clientes de energia e infraestruturas (diretiva NIS2, requisitos dos operadores de rede): "como é que isto é seguro, e como é que o opero?"

| Área | Conteúdo | Documento |
|---|---|---|
| **modo gateway** | nó com duas interfaces; `jk doctor --network` verifica firewall, forwarding, serviços expostos; perfis de hardening por placa | 12 §5 |
| **protocolos seguros** | endurecimento: Modbus/TCP Security (TLS) e BACnet/SC novos; o OPC UA `SignAndEncrypt` já é a omissão na 1.0 (ref/01 §5.18), e a 1.3 só o endurece (verificação no `jk doctor --network`, perfis de hardening) | 12 §5 |
| **cadeia de fornecimento** | SBOM por artefacto, builds reprodutíveis verificáveis, atestados de build | 09 ataque 9 |
| **dados assinados** | transporte intacto de registos assinados por contadores (OCMF, SML) | 12 §9 |
| **Joker Studio** (`jk ui`) | comissionamento local: navegador de sinais e instâncias, painel, escritas manuais com TTL, alarmes, traço e replay | 20 §1, 12 §12 |
| **Deck: operação remota** | sessões remotas (`jk` e Studio por túnel), aprovação de intenções, exportação de auditoria | 20 §4.1 |
| **Deck: multi-inquilino** | um integrador, muitos clientes | 20 D4 |

**Portas de saída:** um teste de intrusão externo a um nó em modo gateway sem achados críticos; comissionamento de um site novo feito só pelo Studio, sem a CLI; um integrador a gerir três clientes num Deck multi-inquilino.

---

## 6. v1.4 — Micro+ (≈ meses 23–27)

O Joker Micro da v1 é o núcleo com apps nativas. A 1.4 leva-o para onde o Linux não chega.

| Área | Conteúdo | Porquê |
|---|---|---|
| **Wasm em MCUs M2** | `wasmi` (`no_std`, determinista, com *fuel*): trocar apps num ESP32-S3 ou STM32H7 sem reflash | frotas de MCUs com lógica que muda (documento 19, §5) |
| **ML em MCUs** | Burn Flex `no_std` em produção para modelos pequenos (anomalia, classificação), a partir da experiência da 1.1 | inferência no sensor, sem Linux |
| **baixo consumo** | nós a pilhas: grupos de ciclo acordados por eventos ou por temporizador, o MCU dorme entre ticks, orçamento energético no `jk check` | sensores remotos, contadores |
| **uplinks de longo alcance** | LoRaWAN e NB-IoT para nós autónomos (telemetria e alarmes; escritas só com lease longo) | sites sem rede (**só se houver procura** de pilotos) |
| telemetria no MCU | Sparkplug B completo; Zenoh-pico | integração direta sem nó Linux |
| alvos | nRF52/nRF54 (com BLE), STM32U5 (baixo consumo) | mais famílias |

**Portas de saída:** uma app trocada por OTA em 100 MCUs M2 sem reflash; um modelo de anomalia a correr num MCU com < 100 KB de RAM; um nó a pilhas com autonomia medida de > 1 ano num ciclo de 1 min.

---

## 7. v1.5 — Frota em escala (LTS) (≈ meses 27–31)

A versão que fecha a série 1 e fica suportada durante 3 anos. Por isso é de **consolidação**, e não de novidades de núcleo.

| Área | Conteúdo |
|---|---|
| **Hub público** | registo aberto de perfis, placas, classes e drivers da comunidade, com verificação automática (conformidade de classes, `jk profile test` contra simuladores) |
| **Deck: observabilidade** | métricas da runtime agregadas, alarmes da frota, painéis por site e por modelo de equipamento |
| **Deck: auditoria** | registos de decisões agregados e pesquisáveis, com retenção por política |
| **MCUs autónomos em escala** | gestão direta pelo Deck de milhares de Joker Micro com rede |
| classes | `jk class extract` (de crates Rust para `.jkc`), se a procura o justificar |
| consolidação | remoção de avisos de *deprecation* (sem remover funcionalidades), documentação de operação para 3 anos, migração assistida da 1.0 |

**Portas de saída:** 1 000 nós e 5 000 MCUs num Deck, com 99,9% dos nós no estado desejado em 24 h após um rollout; o guia de migração 1.0 → 1.5 testado em três sites reais.

---

## 8. v2.0 — O site como um sistema (≈ meses 31–36)

Até à v1.5, um nó é a unidade: cada `jokerd` tem os seus sinais, e os MCUs são dispositivos dele. A v2 muda a unidade para o **site**: vários nós e MCUs que se comportam como **um sistema**, com redundância.

### 8.1 O que entra

| Área | Conteúdo | Documento |
|---|---|---|
| **redundância ativo/standby** | dois nós com a mesma geração; o standby mantém a imagem de processo quente; troca quando o lease do ativo expira; um árbitro de quórum (um terceiro nó leve ou um Joker Micro) evita *split-brain* | 12 §16 |
| **malha de nós** | um espaço de sinais do site partilhado entre nós (por Zenoh), com caminhos qualificados por nó (`@caldeira/pump.run_cmd`); instâncias que atravessam nós; alarmes e árbitros por site | novo |
| **mundo WIT 2** | se as *shared-everything threads* estiverem estáveis no Component Model, uma instância por app em vez de duas (documento 09, ataque 6); remoção dos adaptadores do mundo 0.x | 04, 09 |
| **manifesto de site** | um manifesto para vários nós, com a colocação das apps e das instâncias decidida pelo `jk check` ou declarada | 03 |
| **Joker OS** (se houver procura) | imagem de referência (Yocto/Buildroot) com PREEMPT_RT, A/B do sistema e `jokerd` pré-instalado | 12 §11 |

### 8.2 Porque é uma major

- Os **caminhos** ganham um qualificador de nó, o que muda a gramática do documento 02, §9.
- O **árbitro** passa a poder estar num nó diferente das saídas que arbitra, o que muda regras do documento 16.
- O **mundo WIT** muda de estrutura.
- O **protocolo do agente** passa a descrever sites, e não só nós.

Todas estas mudanças têm migração (`jk migrate`), e a 2.0 corre apps do mundo 1 por adaptador.

### 8.3 O que fica como trilha própria

| Tema | Porquê à parte |
|---|---|
| **Joker Micro Safety** (subconjunto certificável IEC 61508, com Ferrocene) | é um produto com o seu próprio processo de certificação, de anos (documento 12, §1); pode começar em paralelo a partir da 1.5, se houver decisão de produto |
| aprendizagem federada | depende de frotas com ML em produção; é v2.x |

---

## 9. Estresse do plano

**T1 — A 1.1 atrasa porque o Burn falha a validação.** O plano B está escrito: ORT como backend por omissão em CPU. A 1.1 não depende de o Burn ganhar.

**T2 — Um piloto precisa de BACnet em 2026, e o BACnet está na 1.2.** Os drivers não tocam no núcleo. O JDP (tier X) é estável desde a 1.0, e um integrador pode escrever um driver BACnet próprio sobre uma biblioteca C para o piloto; o driver do catálogo, em qualquer tier, é da 1.2 (documento 05, §10) e pode adiantar-se numa minor anterior sem mexer no tema. A regra 1.2.2 existe para isto.

**T3 — As 1.x incham.** O limite de três funcionalidades de cabeça por versão, e as portas de saída, são a contenção. O que não cabe passa à seguinte, e a lista de espera é pública.

**T4 — Uma falha de segurança obriga a uma mudança incompatível numa 1.x.** É a única exceção à regra de ouro: uma ADR de segurança pode quebrar a compatibilidade, com migração automática e um aviso a todos os Decks.

**T5 — A v2 é grande demais.** A malha de nós e o ativo/standby podem separar-se: a 2.0 com ativo/standby (que reutiliza quase tudo da v1), e a malha numa 2.1. A decisão toma-se no fim da 1.5, com os pedidos dos clientes à frente.

**T6 — O Deck compete pelas mesmas pessoas que o núcleo.** O Deck tem versões e equipa próprias a partir da 1.3, e a compatibilidade é pelo protocolo do agente. Um atraso no Deck não atrasa a runtime.

**T7 — Um cliente industrial não quer atualizar nunca.** A 1.0 tem 12 meses e a 1.5 LTS tem 3 anos. Depois disso, um contrato de suporte estendido é uma decisão comercial, e não técnica.

**T8 — O Structured Text revela-se difícil de integrar com classes e Wasm.** A 1.2 pode sair com ST para o domínio de ciclo apenas (o uso típico de um PLC), deixando as tarefas async para Rust. É o caso mais importante, e o resto espera.

---

## 10. Como este documento evolui

- No fim de cada versão, este documento é atualizado: a versão que saiu passa a histórico, com o que ficou de fora e porquê, e a seguinte ganha detalhe.
- As secções de cada versão 1.x tornam-se, na altura, um documento normativo próprio (tal como o 16, 17 e 19 foram para a v1).
- Uma funcionalidade só muda de versão com uma nota aqui e, se tocar num contrato, com uma ADR.
