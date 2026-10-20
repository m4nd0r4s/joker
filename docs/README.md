# Joker — documentação de desenho

A runtime coringa para dispositivos de edge: o mesmo núcleo em Linux embarcado e em microcontroladores, uma abstração só (o sinal), um envelope de segurança entre cada app e o hardware.

Estado: **documento de desenho.** Nada disto está implementado. Os números atribuídos a projetos existentes vêm de fontes citadas; os alvos propostos são estimativas a validar.

---

## Índice

| Ficheiro | Conteúdo |
|---|---|
| `00-visao-geral.md` | A tese, as sete apostas, os sete mecanismos, DX, panorama, glossário |
| `01-arquitetura.md` | Camadas, processos e threads, imagem de processo, escalonamento, envelope, árbitro, host Wasm, ML, historian, uplink, API, OTA, segurança, observabilidade, falhas, números |
| `02-modelo-de-sinais.md` | Tipos, unidades, qualidade, tempo, caminhos, sinais escalares, em bloco e streams, semântica de leitura e escrita, ações, expressões |
| `03-manifesto.md` | Referência completa de `joker.toml` e `joker.lock` |
| `04-sdk.md` | O mundo WIT, o SDK Rust, TypeScript e Python, testes, simulação |
| `05-drivers.md` | Os três tiers de drivers, o trait, planos de leitura, perfis, descoberta, catálogo |
| `06-ml.md` | O subsistema de ML, o artefacto `.jkm`, pipelines, treino no dispositivo, **análise do Burn** |
| `07-cli.md` | A CLI `jk` completa |
| `08-stress-tests.md` | Seis rodadas de estresse em casos de uso reais e o que quebrou |
| `09-arquitetura-sob-estresse.md` | Doze ataques à própria arquitetura: orçamentos, falhas, fugas da abstração, versões, segurança |
| `10-mudancas-de-desenho.md` | As 26 primitivas que saíram dos documentos 08 e 09 |
| `11-roadmap.md` | Fases até à v1.0, portas de saída, ponto de decisão |
| `12-limites.md` | **O que o Joker não faz, e porquê** |
| `13-plano-de-implementacao.md` | Crates, frentes e as primeiras tarefas da Fase 0 |
| `14-classes-e-instancias.md` | Classes de equipamento (`Led`, `Pump`, `Inverter`), instâncias implícitas, compostas e virtuais, `Many<T>`, implementações ativas, 14 ataques ao desenho |
| `15-ficheiros.md` | Mapa de todos os ficheiros (manifesto, overlays, lock, perfis, placas, classes, simulação, cenários, apps, artefactos), a regra dos nomes `<nome>.<tipo>.toml` (ADR 0026) e o vocabulário placa/dispositivo/perfil/instância/classe |
| `16-semantica-do-nucleo.md` | **Normativo para a Fase 0**: regras exatas do árbitro, do envelope, da qualidade, das expressões; catálogo de unidades; aliases; formato do traço; esquema dos casos de conformidade; registo de códigos |
| `17-jkc.md` | A IDL de classes `.jkc`: declarações, `method` vs `action`, gramática, execução, adaptadores, geração, versões |
| `18-prontidao-v1.md` | A rodada de prontidão: ataques de ponta a ponta, o que mudou, o veredicto sobre fechar a v1 e começar a implementar |
| `19-joker-micro.md` | **O Joker em microcontroladores**: núcleo `no_std`, modos firmware, biblioteca C e crate, Joker Link, coprocessador, 10 casos de estresse |
| `20-familia-e-deck.md` | A família (Joker, Micro, Hub, Deck, Studio), os princípios local-first e o Joker Deck |
| `21-da-v1-a-v2.md` | **Depois da 1.0**: as versões 1.1 a 1.5 (LTS) por tema, a v2 (o site como um sistema), as regras de compatibilidade e o estresse do plano |
| `22-rodada-da-api.md` | **A rodada da API**: o mapa das interfaces, um valor e uma escrita de ponta a ponta, as 23 decisões transversais, estresse entre superfícies, o que isto pede à Fase 0 |
| `23-bancada-de-desenvolvimento.md` | **A bancada atual**: Raspberry Pi 3 B+, Pi Zero W e ESP32, sem comprar nada; papéis, ligações, firmware de bancada, manifestos, perfis de placa, o que se valida em cada fase e o que fica por provar |
| `24-publico-e-usos.md` | **Para quem é o Joker**: o comprador, quem trabalha com ele todos os dias, o canal, o público generalista, a primeira hora e a primeira semana de cada um, e oito propostas (placa inteira como dispositivo, firmware expansor, apps externas, painel de leitura, catálogo do dia 1, OCPP, classes `std/`, ponte para o Home Assistant) |
| `25-licoes-do-ecossistema-rust-embedded.md` | O que aprender com PAC, HAL, BSP, `embedded-hal`, Embassy, `esp-hal`, `probe-rs`, `defmt`, Tock, Hubris e Zephyr: o que adotar, o que não, e os riscos de dependência |
| `26-revisao-final.md` | **A revisão final**: auditoria, lições do ecossistema, 12 casos de estresse, o âmbito contra a equipa (ADR 0028), a primeira fatia vertical (M0), funcionalidades candidatas, a fronteira da proteção de dados e o veredicto de arranque |
| `ref/` | **Referências normativas de todas as interfaces** (ADR 0023): `00` convenções · `01` manifesto · `02` lock, nó, cliente, segredos · `03` perfis, placas, simulação, cenários, conformidade · `04` WIT e SDK Rust · `05` SDKs Python e TypeScript, clientes · `06` drivers N/W/X · `07` Joker Link · `08` ABI C do Joker Micro · `09` API HTTP · `10` protocolo do agente |
| `propostas/` | Propostas e o seu raciocínio; **não normativas** (a da IDL foi incorporada no 17) |
| `adr/` | Registos de decisão de arquitetura |
| `exemplos/` | Percurso guiado e três projetos completos |
| `trilha/` | **Trilha de estudo** em 11 partes e 4 módulos, sobre a bancada do documento 23 (portátil, Pi 3 B+, Pi Zero W, ESP32; nada se compra): sinais e o nó Linux (o LED `ACT`, a temperatura do CPU, o ESP32 por Modbus RTU e TCP, falhas e replay, trocar de placa), classes (as da `std/` e a tua), o Joker Micro no ESP32 (Joker Link em série e por Wi-Fi, uma app no MCU) e a frota (`prod`, segredos, o Deck e um rollout em ondas) |

---

## Precedência (v1)

Quando dois documentos divergem, vale, por esta ordem: **16** e **ref/00** (semântica do núcleo e representação) → **17** (`.jkc`) → **ref/01–ref/10** (referências das interfaces, com os ficheiros de `wit/` e `spec/`) → **01–05, 14, 19** (normativos por área, que explicam) → **20** (princípios e contratos da família) → **06–07, 15** → ADRs → **08–13, 18, 21, 22, 24–26** (análise e planeamento) → **exemplos/**, **trilha/** e **propostas/** (ilustrativos). Um exemplo que contradiga um normativo é um bug no exemplo.

## Por onde começar

**Se tens 10 minutos:** `00-visao-geral.md` e depois `12-limites.md`.

**Se vais decidir se isto se constrói:** `08-stress-tests.md` e `09-arquitetura-sob-estresse.md`. É onde o desenho é atacado em vez de defendido.

**Se vais implementar:** `01` → `02` → `09` → `04` para perceber, depois `22` e a referência da tua frente em `ref/`, que é o que se implementa. As ADRs quando discordares de alguma coisa.

**Se estás a aprender:** `trilha/README.md`, parte a parte, com a bancada do documento 23: um Raspberry Pi 3 B+, um Pi Zero W, um ESP32 e o portátil, sem breadboard nem componentes. A parte 1 corre só no portátil, no simulador.

**Se vais escrever apps:** `exemplos/00-primeiro-projeto.md`, depois `02` e `04`.

**Se vais pôr ML a atuar:** `06-ml.md`, a rodada 6 do `08` e a ADR 0012.

---

## Em três frases

A app nunca toca no mundo: lê um snapshot consistente de sinais e propõe escritas, que um envelope nativo (gama, taxa, interlocks, lease, estado seguro) aceita, limita ou rejeita antes de chegarem ao driver.

O controlo corre num domínio de ciclo de tempo real, sem alocação nem bloqueio. ML, rede e historian correm num domínio assíncrono separado, que nunca o pode atrasar, e a telemetria perde amostras contadas em vez de travar o controlo.

Como o relógio é um input e todo o I/O passa pela imagem de processo, qualquer execução gravada no campo pode ser reproduzida bit a bit num portátil, e qualquer versão nova pode correr primeiro em modo sombra.
