# 26 — Revisão final e decisão de arranque

A última rodada antes de escrever código. Juntou quatro olhares: uma **auditoria de coerência** do corpus inteiro, as **lições do ecossistema Rust embedded** (documento 25), um **estresse com olhos de quem tem de construir** isto, e uma lista de **funcionalidades candidatas**. Termina com um veredicto e com as decisões que ficam para o dono do projeto.

É análise e planeamento (precedência de `docs/README.md`). A proposta de corte do §5 só passa a valer quando a ADR 0028 for aceite.

---

## 1. Onde estamos

| O quê | Estado |
|---|---|
| Documentos | 00–26, mais a trilha (11 partes), os exemplos e as propostas |
| Referências normativas | 11 (`docs/ref/00`–`10`), com `wit/` (3 pacotes), `spec/cddl/` (6), `spec/openapi/` e `spec/c/` |
| ADRs | 0001–0027 aceites; 0028 proposta |
| Registo de códigos | `spec/codes.toml` (a contagem vive lá; não se repete na prosa, B8) |
| Validação | todos os blocos TOML e o `codes.toml` com `tomli`; o OpenAPI com `openapi-spec-validator`; os headers C com `gcc -pedantic`; os WIT com o parser oficial (`jco`); os CDDL num parser de sintaxe |

O que **não** foi validado continua a ser a tarefa 0a da Fase 0: as assinaturas Rust e o TypeScript das referências nunca foram compilados, e os CDDL (exceto o do `joker:meta`) não foram validados contra instâncias.

## 2. Auditoria de coerência

A auditoria conferiu cerca de 4 300 remissões por script e reviu o conteúdo por amostragem. Corrigiu cerca de 30 falhas mecânicas (remissões, gamas de códigos "livres" que já tinham códigos, contagens, índices) e 17 contradições inequívocas. Ficaram **13 contradições que pediam uma decisão**, e foram todas decididas e aplicadas:

| # | Contradição | Decisão |
|---|---|---|
| A1 | `requires_good` que falha não tinha resultado | `Rejected(Quality)` e o `on_interlock` da saída (16 E4) |
| A2 | `Rejected(ClockUnsynced)` fora da ordem do envelope | passo 0 do envelope; renova o lease (01 §7, 16 E1) |
| A3 | leases e níveis manuais em saídas de MCU | lease = `min(TTL, max_lease do link)`; o MCU não persiste níveis remotos (16 A12/A14) |
| A4 | `joker:driver` 1.0 ou 1.2 | instável na 1.0, só para drivers da árvore; estável para terceiros na 1.2 |
| A5 | APIs 1.1 nos SDKs de tier 2 | o mesmo comportamento que no Rust |
| A6 | `max_age` igual ao `poll` | recusado (`J0816`) |
| A7 | três enums de estado de app | um só `app_state` (ref/00 §6) |
| A8 | rodar um segredo num MCU | um blob novo com a mesma geração Linux (exceção à ADR 0024 §3, escrita) |
| A9 | fases divergentes de drivers e classes | `v4l2`/`spi-adc` 1.1; BACnet e EtherCAT 1.2; `std/` inicial pelo 14 §10 |
| A10 | dois limiares de crashloop com o mesmo código | uma tabela por tier na ref/06 |
| A11 | três listas de crates `no_std` | a do documento 13 §1 |
| A12 | ADRs antigas sem "alterada por" | nota da ADR 0019 nas afetadas |
| A13 | menores (12 normativo?, `.jkm`, "não é o servidor de frota") | alinhados |

## 3. Lições do ecossistema Rust embedded

O detalhe e as fontes estão no **documento 25**. As cinco lições que mais pesam:

1. **Herdar do HAL para baixo, não refazer.** Os drivers do Joker Micro assentam em `embedded-hal` 1.0 e `embedded-hal-async`, que já têm implementações para todas as famílias de MCU que interessam. Onde o `embedded-hal` não chega (ADC, CAN), o Joker define dois traits próprios pequenos.
2. **O firmware genérico colide com a posse dos pinos em compilação.** Os HALs entregam cada pino uma vez, com o tipo decidido no build; a geração em flash só diz em runtime o que cada pino é. A saída é uma tabela de pinos construída no arranque, sem `steal` nem `unsafe`.
3. **`critical-section` e `portable-atomic` passam a ser invariantes do núcleo**, para os contadores de 64 bits em MCUs de 32 bits.
4. **Os chips identificam-se pelo nome exato**, e os crates do ecossistema escolhem-se por *features* por família (`chip-rp`, `chip-stm32`, `chip-esp`), com a chave `[board] chip` nos perfis de placa.
5. **O risco está abaixo do núcleo:** o Embassy está em versões 0.x, o `esp-hal` só tem estáveis os periféricos básicos, o Wi-Fi do ESP32 ainda está em beta, e o Xtensa continua fora do LLVM oficial. A regra: o núcleo só depende de crates 1.x, e o que é 0.x fica nos módulos por chip, com a toolchain fixada por família.

**Não se adota:** *typestate* na API das apps, um firmware compilado por site, Kconfig, acesso direto a registos. **O que o ecossistema não tem, e é a proposta de valor do Joker por cima dele:** qualidade, unidades, envelope, árbitro, replay e frota.

As propostas de prioridade alta (E1–E6) já estão aplicadas nas especificações; as E7–E17 ficam no documento 25.

## 4. Estresse final: doze casos novos

| # | Caso | Resultado | Correção |
|---|---|---|---|
| E1 | a primeira pessoa externa contribui um driver | **quebrava** (processo) | `CONTRIBUTING.md` com DCO, `drivers/contrib/` com nível próprio, `drvtest` obrigatório, dono por driver (depende da licença) |
| E2 | um bug de segurança no wasmtime | aguenta, com trabalho | seguir as versões com suporte longo do wasmtime; cache AOT indexada pela versão; `.cwasm` gerados na CI; `SECURITY.md`; seccomp e Landlock no `jokerd`; o `safe` no MCU como última camada |
| E3 | atualizar a runtime numa frota com MCUs antigos | **quebrava** | **aplicado:** recetor de OTA obrigatório no firmware 1.0 (`J0795`); versão do núcleo de cada MCU no lock, e o `jk check` recusa funcionalidades que ele não conhece (`J0796`) |
| E4 | um cliente sem Rust | quebrava a DX | template `jk new site --template sem-apps`; a `jk` só exige `cargo` se houver apps Rust; apps de catálogo pré-compiladas (F14) |
| E5 | compilar o `jokerd` num Pi 3 | **quebrava** | **aplicado:** nunca se compila no nó; build cruzado no portátil e na CI |
| E6 | tamanho do binário | aguenta se medido | **aplicado:** alvos para o build `minimal` e o completo, `cargo bloat` com limite na CI; o 01 §21 já não fala de ML na 1.0 |
| E7 | a documentação diverge do código | **já acontecia** antes da primeira linha | **aplicado:** as contagens remetem para a fonte e são verificadas pelo `joker-spec-tests`; proposta: mudanças em três níveis (errata, mudança de referência, ADR), e nas Fases 0–1 uma divergência abre um *issue* de triagem em 48 h |
| E8 | dependências com um só mantenedor | aguenta com política | o núcleo sem dependências; codec CBOR do link próprio; `cargo vet` e `cargo deny` desde o primeiro dia; tabela de dependências críticas com plano de *fork* |
| E9 | uma dependência muda de licença | aguenta se houver lista antes | *allowlist* de licenças no `cargo deny`, mais estreita para o MCU; o tier X é a fronteira para bibliotecas copyleft; decide-se depois da licença do próprio Joker |
| E10 | a DX com muitos códigos de erro | quebrava devagar | `jk explain` com um exemplo por código; cada código ativo com um teste que o produza, senão passa a `reserved`; fundir quase duplicados antes da 1.0 |
| E11 | o registo de decisões num cartão SD | **quebrava** (50 fsync/s por saída) | **aplicado:** o registo guarda mudanças de resultado com contagem de repetições; `fsync` imediato só para trips, manuais, rejeições e mudanças de nível; `storage` na placa e aviso `J0381` |
| E12 | a toolchain Xtensa atrasada | quebrava de vez em quando | **aplicado:** o job Xtensa não bloqueia a CI; MSRV do núcleo ≤ a toolchain Xtensa; o ESP32-C6 (RISC-V) é o MCU preferido quando se comprar |

## 5. O âmbito contra a equipa

**O que a v1.0 promete hoje**, contado: 11 referências (~234 000 palavras de texto normativo), centenas de códigos, ~150 operações na API, 16 drivers Linux e 5 de MCU, 25 mensagens do Joker Link com 6 transportes, ~95 comandos da `jk`, 28 crates, três modos de entrega no MCU, mais o Deck e o Hub.

**A estimativa** (grosseira, com gente experiente, sem pilotos): **~350–435 pessoas-semana**. A capacidade do documento 11 é ~155 (3,5 pessoas × 44 semanas). **Faltam 2,2 a 2,8 vezes.** Com a equipa prevista, a v1.0 como está escrita sai em 2,5–3 anos.

**O risco maior não é o desenho, é a largura.** O núcleo é pequeno e está bem especificado; o que mata é a periferia. E a regra de ouro (ADR 0022) torna isto pior: tudo o que sai estável na 1.0 fica congelado durante a série 1, incluindo centenas de coisas especificadas mas nunca implementadas.

**A proposta: ADR 0028, "estável = implementado".** Na 1.0 só é estável o que foi implementado e passou a porta de saída; o resto fica **reservado**, como o ML já está (`@unstable` no WIT, chave reservada no manifesto, `x-joker-since` no OpenAPI, id de mensagem reservado, `status = "reserved"` no registo de códigos). Acrescentar depois é *minor*, e a regra de ouro fica intacta. O que tem de estar certo logo na 1.0, porque fica no campo sem atualização por software: o formato do traço, as camadas L1–L3 do Joker Link com negociação, o recetor de OTA no MCU, o núcleo mínimo `agent/1.0`, o subconjunto estável do WIT, o formato do lock, o espaço de nomes dos códigos e os tipos de media.

**O corte proposto:**

| Versão | O quê |
|---|---|
| **1.0 (tem de sair)** | núcleo `no_std` completo e ≥ 150 casos de conformidade; manifesto, lock, overlays, admissão RT; `jokerd`, `joker-host`, SDK Rust, `joker-test`; drivers `sim`, `modbus-tcp`, `modbus-rtu`, `sunspec`, `gpio`, `sysfs`, `i2c`, `mqtt` (entrada), `joker-link`; classes (`jkc`, `std/` mínima, instâncias implícitas, compostas e virtuais); historian e uplink MQTT/Sparkplug B; a API e a `jk` que a trilha usa; cliente Rust e Python; Joker Micro em modo firmware e crate, ESP32 e RP2350, geração em flash, recetor de OTA; Joker Link em UART, UDP e TCP; `agent/1.0` com GitOps; segredos, assinaturas, CA local |
| **1.0.x** (drivers e extensões, ADR 0022 §3) | `revpi`, `spi`, `opcua` (cliente), `m-bus`, `onewire`, `serial-line`, `modbus-slave` no MCU, ESP32-C6 e STM32, LSP, simuladores de classe |
| **1.1** (com o ML) | modo sombra de apps, orçamentos de uplink, OTLP, OTA de MCUs orquestrado, CAN, CANopen, RS-485 com vez de fala no link, cliente TypeScript |
| **1.2** | SDK TypeScript (com o Python), hosts tier W e X, pontes servidor OPC UA e Modbus, Zenoh e ROS 2, biblioteca C do Joker Micro |
| **Deck e Hub** | saem do caminho crítico da runtime: "Deck 0.x" com versões próprias, a falar `agent/1.0` |

Com o corte, o trabalho cai para ~190–240 pessoas-semana. **Ainda não cabe** em 155: é preciso escolher entre ~60–65 semanas com 4 pessoas, ou manter as 44 semanas com 5 pessoas (uma no Micro desde a Fase 0). As portas de piloto também mudam: o piloto BESS de 30 dias passa a 30 dias na bancada (com piloto real quando houver cliente), e o piloto de 50 + 50 nós passa a 10 + 10 durante 30 dias.

## 6. A primeira fatia vertical (M0)

Antes do resto da Fase 0: **6 semanas, 1–2 pessoas, na bancada do documento 23**, para provar o desenho de ponta a ponta e medir os primeiros números reais.

| Camada | Na M0 |
|---|---|
| núcleo | registo, slots, imagem por grupo, árbitro (`safety`, `manual:4`, app), envelope (gama, taxa, `safe`, lease), trips, formato do traço; já `no_std` e sem alocação no tick |
| manifesto | `[project]`, `[target]`, `[buses]`, `[devices]`, `[outputs]`, `[cycles]`, `[apps]`, `extends`, ~40 códigos, lock mínimo |
| drivers | `sim`, `sysfs` (LED `ACT` e temperatura), `modbus-rtu` com o perfil `local/bench-esp32@1` |
| firmware de bancada | o do documento 23 §4, só RTU |
| host e SDK | o subconjunto de ciclo do `joker:runtime`, AOT em cache, `#[joker::app]` e `#[joker::cycle]` |
| `jokerd` e `jk` | uma thread de ciclo, drivers no tokio, gerações com `apply --confirm`, API no socket Unix (a `jk` chega por SSH); `new`, `dev`, `check`, `apply`, `confirm`, `read`, `write`, `record`, `replay` |
| app | a `luz` das partes 1–5 da trilha, sem classes |

**Portas de saída:** (1) a mesma `luz.wasm` corre no simulador e no Pi 3, e só muda o manifesto; (2) com o cabo do ESP32 desligado, as amostras passam a `bad(comm_failure)`, o LED apaga pelo firmware e a app vê `Err`; (3) uma escrita manual com TTL vence a app e expira sozinha; (4) uma geração que parte o barramento é revertida se não for confirmada; (5) **10 minutos gravados no Pi 3 reproduzem-se no portátil com os mesmos bytes, 100 vezes**; (6) 30 casos de conformidade, escritos por alguém que não lê o código do núcleo, passam em x86_64 e aarch64, e o núcleo compila para `thumbv8m` e `xtensa-esp32`; (7) o erro de unidade do `jk check` sai como no documento 00; (8) os primeiros números reais (jitter no Pi 3, tamanho, RSS, arranque, custo da chamada Wasm, tempo de build); (9) um **relatório de fricção**, em que cada divergência entre a especificação e a realidade vira errata ou ADR.

No fim da M0 reavalia-se o corte do §5 com números reais: é o ponto de decisão do documento 11, antecipado de ~20 semanas para 6.

## 7. Funcionalidades candidatas

Fora das propostas P1–P8 do documento 24. **As cinco recomendadas:**

| # | Funcionalidade | Porquê | Versão |
|---|---|---|---|
| **F1** | Integração com CI: `jk check`, `jk test`, `jk lock --check` e replay de regressão numa action, com diagnósticos SARIF anotados nos PRs | o modelo de frota é GitOps; é onde o integrador vive; barata | 1.0 |
| **F2** | `jk why <saída>`: porque é que a saída tem este valor (nível vencedor, lease, *clamps*, interlocks, últimas decisões e as entradas lidas), num só ecrã | o técnico de campo e a depuração; os dados já existem | 1.0 |
| **F3** | `/metrics` em formato Prometheus e painéis Grafana oficiais | é o que as equipas de operação já usam; os sinais `joker.*` já existem | 1.0 |
| **F4** | Ensaio de uma geração contra um traço gravado: "o que teria acontecido ontem com esta configuração?" | uma sombra offline, sem risco; torna o replay vendável. Só vale em malha aberta, e a `jk` tem de o dizer em cada resultado | 1.1 |
| **F5** | Dossier do site gerado a partir do manifesto (sinais, envelopes, estados seguros, interlocks, alarmes, fallbacks, versões e hashes) | é o documento *as-built* que o integrador entrega a cada cliente | 1.0.x |

**Outras avaliadas:** relatório de comissionamento assinado (1.1), *fuzzing* dos envelopes de um site (1.1; o *fuzzing* interno dos formatos é critério de saída da Fase 1, não uma funcionalidade), segunda assinatura para mudanças que alargam envelopes ou mudam `safe` (1.1), TUI na `jk` (1.1), LSP para `.jkc` (1.1), modelo de simulação ajustado a partir do historian (1.2), *playground* no navegador com o núcleo em Wasm (1.x), linter de apps (1.1), templates de site e apps de catálogo no Hub (1.1), selo de conformidade (1.2), exportação de *tags* para SCADA (1.1), contadores de manutenção automáticos por saída (1.0.x).

## 8. Proteção de dados: a fronteira

Conclusão da rodada anterior: **a anonimização e a pseudonimização são da plataforma de dados**, não do Joker. O Joker vê um site; anonimizar a sério exige ver a população inteira, e a partilha com terceiros (acessos, consentimento, contratos) é outro produto.

O que fica do lado do Joker é **não deixar vazar**. O mínimo proposto:

| # | Mínimo | Versão |
|---|---|---|
| D1 | `data_class` por sinal (`operational`, `personal`, `sensitive`), herdada pelos derivados | 1.0 (chave reservada já na 1.0) |
| D2 | saída negada por omissão: uplinks, pontes e exportações declaram as classes que aceitam | 1.0 |
| D3 | traços, exportações e pacotes de diagnóstico cifrados por destinatário (HPKE, como os segredos) | 1.0 |
| D4 | logs e mensagens de erro nunca levam valores de sinais `personal` ou `sensitive` | 1.0 |
| D5 | retenção por classe e apagamento com auditoria | 1.0.x |
| D6 | cifra em repouso, com chave no TPM quando existe; o `jk doctor` verifica a cifra de disco do SO | 1.3 |

A tensão que fica declarada: o replay precisa dos valores exatos, por isso um traço com sinais pessoais é um dado pessoal. Sai cifrado, tem retenção própria, e o `jk replay` diz-o.

## 9. Veredicto

**Sim, está pronto para começar**, mas pela M0 (§6) e não "pela v1.0 do documento 11". A especificação está madura; o risco já não é o desenho, é o âmbito e o calendário. E o volume da especificação (1,45 MB normativo) é ele próprio um custo de manutenção, por isso a regra das Fases 0–1 é que uma divergência abre um *issue* de triagem, e não que "o código está errado" por decreto.

**Bloqueiam o primeiro commit:**

1. **A equipa:** quantas pessoas, e quem é dono de que frente (documento 13). A M0 precisa de 1–2 pessoas e de um autor de conformidade independente, que pode ser um agente com acesso só a `docs/`.
2. **A licença:** antes da primeira dependência (a *allowlist* deriva dela) e antes de qualquer contribuição externa. Recomendação: a do documento 18 (Apache-2.0 OR MIT).
3. **O corte e a nova base do roadmap:** aceitar ou não a ADR 0028, e escolher entre ~60–65 semanas com 4 pessoas e 44 semanas com 5, antes de começar. Sem isto, "atrasado" não quer dizer nada.

**Têm prazo, mas não bloqueiam:**

| Decisão | Até quando |
|---|---|
| nome e marca, organização `@joker` no npm | antes do primeiro repositório ou crate públicos |
| modelo de negócio da família | antes de publicar (bloqueia o canal OEM) |
| hardware de CI (CM4/CM5, RP2350 com sonda, ESP32-C6) | fim da Fase 1: é a primeira compra, e é barata |
| catálogo do dia 1 e quais das P1–P8 (documento 24) entram | fim da Fase 1 |
| D1–D4 (§8) | durante a Fase 0, para a chave ficar reservada no esquema |

**Durante a Fase 0 (depois da M0):** as questões abertas do documento 16 §10; o processo de mudança da especificação em três níveis; as políticas de dependências, de toolchain e de versões do wasmtime; e a tarefa 0a (compilar as referências).
