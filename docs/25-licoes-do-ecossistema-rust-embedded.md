# 25 — Lições do ecossistema Rust embedded

A pergunta que originou este documento: *"No mundo Rust embedded já temos HALs, BSPs, PACs. Podemos aprender com a abstração deles o que faz sentido e o que não. Sei que o que oferecemos é mais (é uma runtime), mas podemos aprender com o que o ecossistema está a fazer hoje."*

É análise, como os documentos 11, 21 e 24. As propostas do §6 só passam a normativas quando entrarem numa referência, com ADR quando for o caso. O estado do ecossistema é o de **setembro de 2026**, confirmado nas fontes do fim; as versões mudam depressa, e a tabela do §0 deve ser revista em cada fase.

---

## 0. O estado do ecossistema, em setembro de 2026

| Peça | Versão ou estado | O que interessa ao Joker |
|---|---|---|
| `embedded-hal` | 1.0 (jan. 2024), sem 2.0 planeada | traits estáveis: `digital`, `i2c`, `spi`, `delay`, `pwm::SetDutyCycle`. **Sem ADC**, sem `IoPin`: foram retirados antes da 1.0 |
| `embedded-hal-async` | 1.0, Rust ≥ 1.75 | versões async de I²C, SPI, delay e espera de pinos; sem alocação nem `dyn` |
| `embedded-hal-bus` | 1.0 | partilha de barramentos I²C e SPI entre drivers (`I2cDevice`, `SpiDevice`) |
| `embedded-io`, `embedded-io-async` | 0.7 (MSRV 1.81) | `Read`/`Write` para série e sockets; **ainda 0.x** |
| `embedded-can` | fora do `embedded-hal`; sem trait async normalizado | cada HAL tem a sua API CAN async |
| `embedded-storage` | 0.x | `NorFlash`, já usado pela persistência (ref/08 §2.11.3) |
| Embassy | `embassy-executor` 0.10 (mar. 2026), `embassy-sync` 0.8, `embassy-net` 0.9, `embassy-stm32` 0.6, `embassy-boot-stm32` 0.8 | tudo **0.x**, com quebras frequentes; corre em Rust estável |
| `stm32-metapac` | gerado do `stm32-data` com o `chiptool` | exporta `METADATA` (pinos, periféricos, memória) para `build.rs` e ferramentas |
| `embassy-stm32` | 0.x, **1 567 features**, uma por chip | o exemplo extremo de "features por chip" |
| `embassy-rp` | RP2040 e RP235x (`rp235xa`, `rp235xb`) | o HAL do alvo M1 de referência |
| `rp-hal` (`rp235x-hal`) | 0.x, "volátil até à 1.0" | alternativa sem Embassy |
| `esp-hal` | **1.0 em out. 2025**, 1.2.2 em set. 2026 | a 1.0 estabilizou só `init`, GPIO, UART, SPI, I²C (blocking e async), `time` e o reset; **ADC, LEDC, RMT, toque, watchdog e o resto ficaram atrás da feature `unstable`**, a estabilizar aos poucos (confirmar o estado na 1.2 antes da Fase 2) |
| `esp-radio` (antes `esp-wifi`) | 1.0.0-beta.1, sobre `esp-hal` 1.2 e `esp-rtos` 0.4 | Wi-Fi, BLE, ESP-NOW, 802.15.4; pede `esp-rtos` (escalonador preemptivo) e `esp-alloc` (heap) |
| `esp-rtos` | 0.4 | substitui o `esp-hal-embassy`: executores Embassy de thread e de interrupção |
| Xtensa (ESP32, ESP32-S3) | **fora do LLVM e do Rust oficiais**; toolchain `esp` pelo `espup` (LLVM 22 desde o Xtensa Rust 1.99) | o ESP32 da bancada depende de uma toolchain de terceiros |
| RISC-V (ESP32-C6) | Rust oficial (`riscv32imac-unknown-none-elf`) | sem toolchain de terceiros |
| `probe-rs` | 0.32 (jul. 2026), biblioteca e CLI | ARM, RISC-V e Xtensa por JTAG (o ESP32 original precisa de um adaptador externo) |
| `defmt` | 1.0 (2025), SemVer; **sem garantia de MSRV** | registos com formatação diferida, descodificados no anfitrião a partir do ELF |
| `critical-section` | 1.x | secção crítica "plugável": o HAL ou o integrador escolhe a implementação |
| `portable-atomic` | 1.x | atómicos onde o hardware não os tem (Cortex-M0+, 64 bits em MCUs de 32) |
| `heapless` | 0.9.x | coleções de capacidade fixa (já usadas: `Text`, `Bytes`) |
| `svd2rust` | 0.37 | gera PACs a partir de SVD |
| `device-driver` | ativo (livro v1), DSL ou manifesto (TOML, YAML, JSON) | gera drivers de registos para chips I²C e SPI |
| RTIC | 2.x, async, `rtic-monotonics` | escalonamento por prioridades de hardware, verificado em compilação |
| Tock OS | 2.2 (jan. 2025), Rust estável | processos isolados por MPU, *capsules* em Rust seguro, credenciais de processos |
| Hubris (Oxide) | em produção | tarefas estáticas declaradas em `app.toml`, supervisor `jefe`, IDL `Idol` |
| Zephyr | 4.4 (4.3, 3.7 LTS) | devicetree + bindings YAML + Kconfig + CMake |

---

## 1. O mapa das camadas, lado a lado

A pilha clássica do Rust embedded tem quatro andares: o **PAC** (acesso a registos, gerado de SVD), o **HAL** (drivers de periféricos do chip, que implementam os traits `embedded-hal`), o **BSP** (a placa: que pino é o LED, que UART vai para o conector) e os **driver crates** (chips externos, genéricos sobre os traits). Por cima, um executor (Embassy, RTIC) ou um sistema (Tock, Hubris, Zephyr).

| Ecossistema | Joker | Diferença essencial |
|---|---|---|
| **PAC** (`svd2rust`, `chiptool`, `stm32-metapac`) | **nada**. O Joker nunca toca em registos do chip | o Joker consome o PAC só através do HAL. O que o Joker tem de "PAC" é o **perfil de equipamento**: o mapa de registos de um equipamento de campo (Modbus, SunSpec). O SVD está para um chip como o `*.profile.toml` está para um inversor |
| **metadados do PAC** (`stm32_metapac::METADATA`, dados do `stm32-data`) | as chaves de hardware do `*.board.toml` (`[pins]`, `[uart.*]`, `[areas]`, `memory`, `flash`) | o ecossistema **gera** estes dados das fichas do fabricante; o Joker escreve-os à mão (ref/03 §2.3.7) |
| **HAL** (`embassy-rp`, `embassy-stm32`, `esp-hal`) | os módulos `chip-*` do `joker-micro` e os transportes do `Link` (ref/06 §3.4) | o Joker **não escreve HALs**: usa-os. Os drivers tier N do MCU (`gpio`, `i2c`, `uart`…) são adaptadores finos por cima |
| **traits `embedded-hal`/`-async`/`embedded-io`** | o contrato dos transportes do `Link` | é aqui que o Joker herda o ecossistema: um driver que só fala por estes traits corre no Linux, no MCU e nos duplos |
| **BSP** (crate por placa) | **perfil de placa** (`*.board.toml`) | um BSP é código compilado; um perfil de placa é um dado lido pelo `jk check` e pelo `jk build`. O Joker não precisa de um crate por placa |
| **driver crates** (`bme280`, `ssd1306`, os gerados pelo `device-driver`) | **drivers tier N** + **perfis de equipamento** | um driver crate dá métodos (`read_temperature()`); o Joker dá **sinais** com unidade, qualidade e tempo. Falta a ponte entre os dois (§2.7) |
| **`embedded-hal-bus`** | `Link::I2c` "restrito aos endereços da instância", o escalonador de barramento | a mesma ideia, com mais garantias: no Joker, o barramento é do núcleo, e não de quem o partilha |
| **typestate** nos pinos e periféricos | **validação no `jk check`** (`J0829`: pino em dois periféricos) e o `Parts` do ref/08 §2.11.3 | o ecossistema prova em compilação; o Joker prova antes de gravar a geração, porque a configuração chega em runtime (§3.1) |
| **executor** (Embassy, RTIC) | grupos de ciclo em executores de interrupção (ref/08 §2.11.6) | o executor é um detalhe do hospedeiro; o núcleo não depende dele |
| **`embassy-boot`**, MCUboot | OTA A/B de firmware (ref/08 §2.12.4) | já adotado |
| **`defmt`** | feature `defmt` do `joker-micro`, anel de traço | adotado a meio: só a feature (§2.5) |
| **`probe-rs`** | `jk flash`, `jk bench --target mcu` | já adotado como ferramenta |
| **devicetree** (Zephyr) | `*.board.toml` | o mesmo papel; o Joker usa um só formato e um só validador |
| **bindings YAML** (Zephyr) | esquemas do descritor de driver (ref/06 §2.3.1) | a mesma ideia: o esquema vem com o driver e valida a configuração |
| **Kconfig** (Zephyr) | features Cargo escolhidas pelo `jk build` a partir do manifesto, e `[capacities]` | o utilizador do Joker nunca escolhe features à mão (§3.3) |
| **`app.toml`** (Hubris) | geração compilada + capacidades do firmware genérico | tudo estático, tudo conhecido no build; o Joker acrescenta a geração em flash |
| **processo** (Tock), **tarefa** (Hubris) | app nativa (v1) e app `wasmi` (1.4) | o Tock e o Hubris isolam por MPU; o Joker Micro v1 não isola (um *panic* é `jk_fatal`) |

**A leitura do mapa.** Do HAL para baixo, o ecossistema resolve o problema melhor do que o Joker alguma vez o resolveria, e o Joker deve **herdar** essa parte. Do driver crate para cima, o ecossistema pára no "método que devolve um número", e é aí que o Joker começa (§4).

---

## 2. O que adotar

Cada item diz o que muda nas especificações e quanto custa. O detalhe das alterações está na tabela do §6.

### 2.1 Os drivers do MCU assentam em `embedded-hal` 1.0, com um mapa honesto das lacunas

**Já está decidido** (19 §3, ref/06 §3.4 e §3.7): os transportes do `Link` implementam `embedded-io-async` e `embedded-hal-async`, e os drivers `micro = true` só falam por eles. É a decisão certa: herda o `embassy-rp`, o `embassy-stm32` e o `esp-hal` sem escrever um HAL.

**O que falta é dizer onde o `embedded-hal` não chega.** O documento 19 §3 diz "drivers GPIO, ADC, PWM, I²C, SPI, UART, Modbus RTU, CAN, 1-Wire, sobre `embedded-hal` 1.0". Não é verdade para todos:

| Driver do MCU | Trait do ecossistema | Estado | O que o Joker faz |
|---|---|---|---|
| `gpio` | `embedded_hal::digital::{InputPin, OutputPin}` + `embedded_hal_async::digital::Wait` | estável | direto |
| `i2c`, `spi` | `embedded_hal_async::{i2c::I2c, spi::SpiDevice}` | estável | direto; partilha com `embedded-hal-bus` |
| `uart`, `modbus-rtu`, link série | `embedded_io_async::{Read, Write}` | **0.7, não estável** | direto, com a versão fixada |
| `pwm` | `embedded_hal::pwm::SetDutyCycle` | estável, só o *duty* | frequência e fase ficam no módulo `chip-*` |
| `adc` | **nenhum** (retirado antes da 1.0) | — | trait próprio no `joker-hal` (`AdcRead`), implementado em cada módulo `chip-*` |
| `can` | `embedded-can` só *blocking*/`nb` | sem async normalizado | trait próprio (`CanFrames`), implementado em cada `chip-*` sobre a API async do HAL |
| `onewire` | **nenhum** | — | sobre um pino *open-drain* (`Flex`) com temporização do HAL, ou por UART |
| `esp32` (toque, RSSI) | específico do `esp-hal` | toque atrás de `unstable` | só no `chip-esp32*` |

**Muda:** ref/06 §3.7 e ref/08 §2.11.2 ganham esta tabela; o `joker-hal` define os dois traits que faltam (`AdcRead`, `CanFrames`), com a mesma forma dos do `embedded-hal-async`, para os poder largar quando o ecossistema os normalizar.
**Custo:** baixo no papel. Na implementação, cada `chip-*` escreve dois adaptadores (ADC e CAN) por família.

### 2.2 `critical-section` e `portable-atomic` no núcleo partilhado

O `Io` do ref/08 §2.11.3 é `Copy + Send + Sync`, e as funções `[any]` podem ser chamadas de qualquer contexto, incluindo interrupções. Isso precisa de sincronização. E as amostras levam `seq: u64`, mas nenhum dos alvos M1 e M2 tem atómicos de 64 bits (Cortex-M33, Xtensa LX6/LX7 e RISC-V `imac` são de 32 bits).

O ecossistema resolveu isto com dois crates que todos os HALs já usam:

- **`critical-section`**: o núcleo pede uma secção crítica, e quem a implementa é o HAL (Embassy, `esp-hal`) ou, no modo biblioteca, o integrador (FreeRTOS, Zephyr). É o padrão *de facto*, e evita que o Joker invente a sua própria `jk_platform_t.lock`.
- **`portable-atomic`**: dá `AtomicU64` onde o hardware não o tem, com a secção crítica por baixo. Os *seqlocks* dos slots (01 §3.2) ficam iguais no Linux e no MCU.

**Muda:** AGENTS.md e ADR 0020 (invariante do núcleo: sincronização só por `critical-section` e `portable-atomic`, nunca por primitivas do executor); ref/08 §2.4 (a ABI C documenta que o integrador liga uma implementação de `critical-section`, ou ativa a feature `cs-single-core` quando só há um núcleo e nenhuma interrupção chama o nó); ref/08 §2.11.1 (dependências do núcleo).
**Custo:** baixo. É mais uma dependência estável, e poupa um desenho próprio.

### 2.3 Features por chip, com o nome exato do chip na placa

O ecossistema escolhe o chip por feature Cargo, mutuamente exclusiva: `esp-hal` com `esp32`, `esp32c6`, `esp32s3`; `embassy-rp` com `rp2040`, `rp235xa`, `rp235xb`; `embassy-stm32` com uma feature **por referência** (`stm32g474re`, `stm32h563zi`: 1 567 features). O ref/08 §2.11.2 tem `chip-stm32g4`, `chip-stm32h5`… ao nível da família, e isso **não chega** para o `embassy-stm32`, que precisa da referência completa para saber a memória, os pinos e as interrupções.

Há também uma lacuna: o ESP32 original da bancada (documento 23) não tem feature `chip-*`.

**Muda:**
- ref/03 §2.3.1: chave nova `chip` em placas MCU, com o nome do chip **como o HAL o escreve** (`"rp235xa"`, `"stm32g474re"`, `"esp32"`), distinta de `[probe] chip` (que é o nome do `probe-rs`);
- ref/08 §2.11.2: as features `chip-*` passam a ser por família (`chip-rp`, `chip-stm32`, `chip-esp`), e o `jk build` passa a referência exata ao HAL a partir de `[board] chip`; acrescenta-se o ESP32 original; exatamente uma família por build, com `compile_error!` com zero ou mais do que uma;
- ref/08 §2.15.1: uma coluna com o HAL e a versão fixada de cada alvo.

**Custo:** baixo. O utilizador continua a não ver features: escolhe uma placa.

### 2.4 Um dono para cada pino, sem `steal`, no firmware genérico

O modelo de posse do ecossistema é: o `init` do HAL devolve os periféricos **uma vez**, como *singletons*, e o código escolhe os pinos **em compilação** (`p.PIN_25`). O firmware genérico do Joker (19 §4.1) só sabe que pino é o LED quando lê o blob da geração, **em runtime**. Os HALs têm uma saída para isso (`AnyPin::steal(n)`), mas é `unsafe` e contorna a posse.

A forma segura, que os HALs já permitem: no arranque, o módulo `chip-*` recebe os periféricos do `init`, **converte todos os pinos declarados em `[pins]` num tipo apagado** (`degrade()` para `AnyPin` no `esp-hal`, `Peri<AnyPin>` no `embassy-rp`, `AnyPin` no `embassy-stm32`) e guarda-os numa tabela indexada pelo nome da placa. Cada driver tira o seu pino da tabela, uma vez. O `J0829` (pino em dois periféricos) já garante no `jk check` que não há dois donos; a tabela garante-o outra vez em runtime (um segundo pedido é `JK_E_GEN_FIRMWARE`).

**Muda:** ref/08 §2.12.3, passo 6, e um parágrafo novo no §2.11 ("posse dos pinos"); a regra "sem `unsafe` fora do módulo `ffi`" do ref/08 §2.11.3 passa a valer também para os módulos `chip-*`.
**Custo:** baixo. É o padrão *type erasure* que o próprio `esp-hal` recomenda para "arrays de pinos".

### 2.5 `defmt` como o registo do Joker Micro, também em campo

O `defmt` 1.0 é estável e é o padrão para registos em MCUs: as cadeias ficam no ELF, e o MCU só envia um índice e os argumentos. O ref/08 já tem a feature `defmt`, mas só "registos com `defmt`" por sonda.

A lição que vale mais: o formato `defmt` também serve **fora da sonda**. As tramas podem viajar no Joker Link (numa mensagem de registo, ou dentro do `TRACE`) e ser descodificadas no nó Linux ou no Deck, desde que o ELF exato esteja disponível. O `probe-rs` já faz isto por RTT.

**Muda:** ref/08 §2.14 (o descritor de firmware `.jkfw` leva o *build-id* do ELF); ref/07 (as tramas `defmt` como carga de uma mensagem de registo, de entrega efémera); o Hub guarda o ELF de cada firmware publicado; o `jk logs --node <mcu>` descodifica.
**Custo:** médio. Guardar os ELFs por *build-id*, e uma mensagem nova no link. O `defmt` não garante MSRV, por isso a versão fica fixada (§5).

### 2.6 `probe-rs` como biblioteca, e não só como comando

O `probe-rs` é um crate Rust, não só uma CLI. O `jk flash`, o `jk bench --target mcu` (ref/08 §2.12.5) e a leitura do anel de traço por sonda (19 U9) podem usá-lo **ligado ao `jk`**, sem depender de um executável instalado com a versão certa. É o que o Humility faz para o Hubris: um depurador que conhece o formato do sistema e lê o estado das tarefas pela sonda. O cabeçalho do nó "para sondas" (ref/08 §2.9.2) já existe para isso.

**Muda:** documento 07 (`jk flash`, `jk trace --probe`); ref/08 §2.9.2 (o cabeçalho tem de ser encontrável por símbolo no ELF, como o Humility encontra as tabelas do Hubris).
**Custo:** médio. O `probe-rs` é grande e é 0.x; entra numa feature do `jk` (`probe`) e fixa a versão.

### 2.7 Uma ponte para os driver crates do ecossistema

Há centenas de driver crates sobre `embedded-hal` (sensores, ADCs externos, expansores de I/O, ecrãs) e o `device-driver` gera mais a partir de um manifesto. Hoje, um sensor I²C novo no Joker exige um driver tier N escrito de raiz, ou um perfil com o driver genérico `i2c` e o mapa de registos à mão.

A proposta: um **adaptador de driver crate** no `joker-hal`. O autor escreve uma função que recebe o `I2cDevice`/`SpiDevice` do `Link`, constrói o driver do crate, e declara que métodos dão que pontos (`read_temperature` → `temp`, `°C`). O núcleo continua a tratar da qualidade (um erro do crate é `bad(comm_failure)`), do tempo, da escala e do resto.

```rust
// crates/joker-drivers/bme280/src/lib.rs (esboço)
joker_hal::crate_driver! {
    name: "bme280", bus: i2c, micro: true,
    open: |dev| bme280::Bme280::new(dev, bme280::Address::Primary),
    points: {
        "temp"     => |d| d.read_temperature().await, unit = "°C",
        "pressure" => |d| d.read_pressure().await,    unit = "Pa",
    },
}
```

**Muda:** ref/06 §3 (secção nova: adaptador de driver crates, com as regras de cancelamento e de prazos do §3.5 aplicadas a código de terceiros); ref/06 §6 (a conformidade do adaptador usa o `embedded-hal-mock` para os testes por transação); documento 24, P5 (catálogo do dia 1 com os sensores mais comuns por esta via).
**Custo:** médio. O risco é o código de terceiros bloquear o executor ou não ser *cancel-safe*; o `J0883` (medição do `poll`) apanha o primeiro, e a lista de crates aceites no Hub apanha o segundo.

### 2.8 Gerar os perfis de placa a partir de dados, como o `svd2rust` gera PACs

O `stm32-data` extrai de SVDs e de ficheiros da ST a descrição de cada chip (pinos, funções alternativas, memória, interrupções), limpa-a à mão uma vez, e **gera** o `stm32-metapac`, com um `METADATA` pensado para ferramentas no anfitrião. O `esp-hal` mantém também uma descrição por chip, usada para gerar o seu próprio código. Ninguém escreve a tabela de pinos de um STM32 à mão.

O ref/03 §2.3.7 diz que um perfil de placa MCU se escreve "à mão, a partir da ficha técnica". É o trabalho que o ecossistema já fez.

**Muda:** ref/03 §2.3.7 ganha `jk board new <nome> --chip <referência>`, que lê os metadados do HAL (`stm32-metapac` com `metadata`, os metadados do `esp-hal`, os do `embassy-rp`) e escreve `memory`, `flash`, `[pins.*] functions`, `adc_channel`, `[uart.*]`, `[i2c.*]`, `[spi.*]`, `[can.*]` e `max_baud`, e deixa com `# REVER` o que é da **placa** e não do chip (o que está ligado a cada pino, `reset_safe`, `[areas]`, `[link] default`, as capacidades).
**Custo:** médio. Um leitor por fonte de metadados, com versão fixada; a validação continua no `jk board check`.

### 2.9 O `embassy-boot`, como já está, com três notas

O ref/08 §2.12 já usa o `embassy-boot` (RP2350, STM32) e o bootloader da Espressif (ESP32). A pesquisa confirma a escolha: o `embassy-boot` faz troca A/B segura contra falhas de energia, arranque em experiência com `mark_booted()` e reversão, e verifica assinaturas Ed25519 (`ed25519-salty`, a opção pequena).

Três notas para a referência:

1. As partições do `embassy-boot` chamam-se `ACTIVE`, `DFU` e `STATE`: correspondem a `firmware`, `fw_staging` e `boot_state` do ref/03 §2.3.3. Vale uma linha na tabela, para quem vem do Embassy.
2. O "marca-se como bom depois de N ticks limpos e do `HELLO`" (ref/08 §2.12.4) é exatamente o `mark_booted()`; a referência deve dizer que é essa a chamada.
3. O RP2350 só arranca uma imagem com um `IMAGE_DEF` válido (o `embassy-rp` tem as opções), e tem arranque seguro na ROM. O perfil `rp2350-pico2` deve dizer qual se usa, e se a ROM verifica o bootloader.

**Custo:** baixo, só texto.

### 2.10 Os padrões dos driver crates que o Joker já segue, e deve escrever

Alguns padrões do ecossistema já estão no desenho, sem nome. Vale nomeá-los na ref/06, para quem vem de lá os reconhecer:

| Padrão do ecossistema | No Joker |
|---|---|
| o driver é genérico sobre o trait, e nunca abre o transporte | "um driver tier N não abre sockets nem portas série: usa o `Link`" (ref/06 §3.4) |
| `split()` de um periférico em partes com posse separada | `Parts { io, service, groups, changes }` (ref/08 §2.11.3) |
| tarefas Embassy não genéricas, geradas por macro | `joker_project::tasks` (ref/08 §2.11.4), porque o `#[embassy_executor::task]` recusa genéricos |
| `embedded-hal-mock` para testar drivers sem hardware | duplos do `joker-drvtest` (ref/06 §6) |
| `#![no_std]` com `alloc` e `std` como features | `joker-hal`, `joker-core` (ADR 0020) |

**Custo:** nenhum, além do texto.

---

## 3. O que não adotar, e porquê

### 3.1 Typestate na API das apps e na configuração

O typestate prova em compilação que um pino está configurado como saída antes de se escrever nele. É o melhor do Rust embedded quando o programa **é** a configuração. No Joker não é:

- **a configuração chega em runtime.** A mesma app corre em cem sites com cem mapas (00, mecanismo 4), e o firmware genérico lê a geração do blob (19 §4.1). Um tipo não pode depender de um ficheiro que chega depois do build;
- **a app não vê pinos.** Vê portas (`Output<bool>`). O pino é do manifesto do site, e muda sem recompilar a app;
- **o custo conhecido do typestate é a explosão de tipos.** O próprio `esp-hal` apaga os tipos dos pinos por omissão, e o livro do Rust embedded aponta a dificuldade de guardar pinos diferentes numa coleção.

O Joker faz a mesma prova noutro momento: **o `jk check`**, antes de a geração chegar ao nó (`J0829`, `J0710`, `J0786`), e o `Parts` do crate, onde o typestate serve para separar contextos. É o sítio certo para ele.

### 3.2 "Compilar tudo por placa" como modelo de distribuição

No ecossistema, a placa, os pinos e a lógica ficam num binário, e mudar a configuração é recompilar. O documento 19 §4.1 já rejeitou isto para frotas: 3 000 sites seriam 3 000 firmwares. O Hubris faz o mesmo por boas razões (imagens pequenas e auditáveis, tudo estático), mas o Hubris não tem de reconfigurar três mil equipamentos por OTA.

O Joker fica com a parte boa (capacidades fixas no build, como o `app.toml` do Hubris) e separa a configuração num blob assinado. A geração embutida continua a existir para produtos fechados.

### 3.3 Kconfig, e features escolhidas à mão

O Zephyr separa bem as coisas no papel ("devicetree descreve o hardware, Kconfig escolhe o software"), mas paga-o em complexidade: duas linguagens, CMake por cima, *overlays* por placa, e a própria documentação admite que o Kconfig não consegue configurar parâmetros por instância e que não se muda o devicetree a partir do Kconfig. O erro clássico (um nó no devicetree sem o driver ativo no Kconfig, e o `device_is_ready()` a devolver falso) não existe se só houver uma fonte.

O Joker não adota o Kconfig nem expõe features ao utilizador. O `jk build` deriva as features do manifesto e da placa (ref/08 §2.11.2), que é o que o Zephyr acabou por fazer com os símbolos `DT_HAS_<compatible>_ENABLED` gerados do devicetree. O Joker começa onde o Zephyr chegou.

### 3.4 A sintaxe do devicetree

`phandles`, nós aninhados, `compatible` como cadeia livre e macros C geradas dão uma curva de aprendizagem íngreme e erros de compilação opacos. O Joker mantém um TOML plano com nomes estáveis e erros com código (`J0828`, `J0829`), e fica com as **ideias** boas (§3.7).

### 3.5 Acesso a registos em drivers e apps

O PAC é `unsafe`-adjacente e específico do chip. Nenhum driver tier N nem nenhuma app do Joker deve tocar num PAC: se um periférico não tem HAL, o adaptador vai para o módulo `chip-*`, revisto uma vez. É a regra do Tock para as *capsules* (Rust seguro, sem `unsafe`), aplicada aos drivers.

### 3.6 Tipos do Embassy na API do núcleo

O `joker-core` corre no Linux sob tokio e no MCU sob Embassy, e no modo biblioteca sob FreeRTOS ou Zephyr. Nada do Embassy (`Spawner`, `Channel`, `RawMutex`) entra nos tipos públicos do núcleo. O ref/08 já o faz bem: o `GroupRunner::tick` é síncrono, e o Embassy fica no módulo `joker_micro::embassy`. Isso também quer dizer que o **RTIC** e um laço *bare-metal* funcionam sem mudar o núcleo; não é preciso adotá-los, basta não os fechar.

### 3.7 O que o Zephyr faz bem, e o Joker já tem ou deve ter

| Ideia do Zephyr | No Joker |
|---|---|
| bindings: o esquema vem com o driver e valida o nó | esquemas do descritor (ref/06 §2.3) |
| `status = "okay"`: um nó só existe se for ativado | "o IO fixo só existe quando é referido" (ref/03 §2.3.2) |
| `chosen`: qual UART é a consola | `[link] default` (ref/03 §2.3.3) |
| revisões de placa | versões de placa (ref/03 §2.3.6) |
| **shields**: placas de expansão que se juntam a qualquer placa com o conector certo | **falta**. Um HAT de relés ou de RS-485 para Raspberry Pi, ou uma placa de I/O para o Pico, tem de ser copiado para cada placa (proposta no §6) |

### 3.8 `embedded-hal-nb` e APIs *blocking* nos drivers

O ecossistema mantém as três formas (*blocking*, `nb`, async). O Joker escolhe **só async** nos drivers (ref/06 §3.3), e o *blocking* só no `Storage` e no contexto de serviço. Não se adota o `nb`.

### 3.9 Isolamento por MPU na v1

O Tock e o Hubris mostram que é possível isolar tarefas num MCU com MPU, e reiniciar só a que falhou (as *dispositions* do `jefe`). É o desenho certo para apps de terceiros. Na v1, o Joker Micro corre apps nativas do próprio projeto, e um *panic* é um reset para o estado de reset do hardware (ref/08 §2.11.5). Adotar MPU agora duplicaria o trabalho do `wasmi` da 1.4 (19 §5). Fica como alternativa a avaliar na 1.4 (§6).

---

## 4. O que o ecossistema não resolve, e o Joker sim

O ecossistema dá, e dá bem: drivers de periféricos, executores, bootloaders, ferramentas de gravação e de registo. Pára na fronteira "o driver devolveu um `f32`". Nenhum projeto da tabela do §0 responde a estas perguntas:

| Pergunta | Ecossistema Rust embedded | Joker |
|---|---|---|
| este número ainda é válido? | `Result<f32, E>`: ou há valor, ou há erro | **qualidade** por amostra (`Good`, `Uncertain`, `Bad` com motivo), herdada do OPC UA, e o `?` que torna a qualidade má num erro (02) |
| em que unidade está? | `f32`, ou um tipo do `uom` escolhido por quem escreve o driver | **unidade no sinal e na porta**, conversão pela runtime, `J0107` na ligação errada (00, mecanismo 4) |
| de quando é? | a app lê o relógio quando quer | `ts_acq`, `ts_hw`, `ts_src` e o **relógio como input** (ADR 0007) |
| o que acontece se o código morrer? | watchdog e reset | **estado seguro obrigatório**, leases, `reset_safe` verificado, e o `safe` aplicado pelo MCU quando o Linux morre (19 §6) |
| pode esta escrita chegar ao hardware? | se o código a fizer, chega | **envelope** (gama, taxa, interlocks) sem caminho que o contorne (ADR 0005) |
| quem manda nesta saída? | quem tem o `&mut` do pino | **árbitro** com prioridades e leases (ADR 0011), com o Linux como escritor igual aos outros |
| posso reproduzir a falha de ontem? | `defmt` e um depurador, ao vivo | **replay determinista** do anel de traço, no portátil, com o mesmo núcleo (19 U9) |
| posso mudar a configuração de 3 000 equipamentos? | recompilar e reflashar cada um | **gerações assinadas em flash**, confirmação, anti-rollback, e o Deck (19 §4.1, 20) |
| a app nova é segura para pôr a atuar? | testes, se os houver | **modo sombra** e critérios de promoção (ADR 0012) |
| este equipamento é uma bomba ou um LED? | o nome do crate | **classes** versionadas com conformidade (14, 17) |
| a mesma lógica corre num Pi e num MCU? | só se for escrita duas vezes, ou com cuidado | **o mesmo núcleo, os mesmos bytes** na conformidade (ADR 0020) |

**A proposta de valor, numa frase:** o ecossistema Rust embedded faz de um MCU um computador programável; o Joker faz dele um **nó de controlo gerível**, com as garantias de um PLC e a operação de uma frota. As duas coisas somam-se, e é por isso que o Joker deve herdar o de baixo em vez de o refazer.

**E ao contrário:** o que o Joker pode devolver ao ecossistema, quando existir, é o `joker-hal` como exemplo de um contrato de driver portável entre Linux, MCU e Wasm, os traits de ADC e CAN async (§2.1) propostos ao grupo de trabalho do HAL, e o adaptador de driver crates (§2.7) como forma de dar qualidade, unidade e tempo a qualquer crate existente.

---

## 5. Riscos de dependência

| Risco | Situação em setembro de 2026 | Impacto no Joker | Mitigação |
|---|---|---|---|
| **APIs do Embassy instáveis** | todos os crates em 0.x; `embassy-executor` 0.10, `embassy-stm32` 0.6; quebras em quase todas as versões | cada atualização mexe nos módulos `chip-*` e no `joker_micro::embassy` | o Embassy só aparece nesses módulos (§3.6); versões fixadas no workspace por família; uma atualização por fase, com a conformidade em hardware e em QEMU como porta; o `joker-project` gerado esconde as tarefas do integrador |
| **`esp-hal` estável, mas estreito** | na 1.0, estável só GPIO, UART, SPI, I²C e `time`; ADC, LEDC, RMT, toque e watchdog atrás de `unstable` | os drivers `adc`, `pwm` e `esp32` do MCU, e o firmware de bancada da Fase 1 (toque), dependem de API instável | a feature `unstable` fica isolada no `chip-esp`; versão exata (`=1.2.x`) no lock do workspace; um teste de compilação por driver em CI |
| **`esp-radio` em beta, com heap** | 1.0.0-beta.1; pede `esp-rtos` (escalonador preemptivo) e `esp-alloc` | o nó autónomo M2 (19 U2) e o link por UDP (doc 23, Fase 2) dependem dele; o núcleo é sem `alloc`, mas o firmware com Wi-Fi terá um heap | declarar a exceção: um heap de tamanho fixo, só para o rádio, fora do núcleo e contado no `J0710`; medir a interferência do escalonador do rádio no jitter dos grupos; fixar a beta |
| **Xtensa fora do LLVM oficial** | toolchain `esp` pelo `espup`, a seguir o Rust estável com atraso; o *upstream* do LLVM avança mas não está feito | o ESP32 da bancada e o ESP32-S3 (M2) dependem de uma toolchain de terceiros; a CI tem de a instalar (doc 23 §5.1) | a conformidade de referência corre em Cortex-M33 (QEMU e RP2350) e em RISC-V (ESP32-C6), com toolchains oficiais; o Xtensa é um alvo a mais, nunca o único; `espup` com versão fixada numa imagem de CI; o ESP32 original fica no nível `dev` (01 §22) |
| **MSRV desalinhado** | `esp-hal` só garante o último estável à data; `defmt` não garante MSRV; `embedded-io-async` 0.7 pede 1.81; a toolchain Xtensa segue o seu próprio calendário | um `cargo update` pode partir o build de um alvo e não de outro | um `rust-toolchain.toml` por família de alvo (oficial e `esp`), com a mesma versão de Rust sempre que possível; o MSRV do Joker é a versão fixada, e não uma garantia a jusante; atualização de toolchain como tarefa explícita de cada fase |
| **`embedded-io` 0.x** | 0.7, ainda sem 1.0 | os transportes série e TCP do `Link` e o `link_stream` do ref/08 usam-no | uma só versão no workspace; os tipos do `embedded-io` não aparecem na ABI C nem no blob, só no crate Rust |
| **traits em falta** (ADC, CAN async) | sem normalização à vista | adaptadores por família (§2.1) | traits próprios com a forma do `embedded-hal-async`, fáceis de trocar quando houver um oficial |
| **`probe-rs` 0.x** | 0.32; suporte Xtensa recente | `jk flash`, `jk bench` e traço por sonda | feature `probe` do `jk`, versão fixada; o `espflash` continua como caminho sem sonda (doc 23) |
| **mantenedores** | o `esp-hal` tem o apoio da Espressif; o Embassy e o `rp-hal` são comunitários, com empresas por trás; o `device-driver` e o `defmt` dependem de poucas pessoas | um crate abandonado bloqueia um alvo | só dependências com mais de um mantenedor ativo no caminho do tick; as restantes em features opcionais; o Joker contribui a montante quando corrige |

**A regra que sai daqui:** o núcleo (`joker-core`, `joker-expr`, `joker-units`, `joker-link-proto`) só depende de crates estáveis (1.x) ou triviais (`heapless`, `critical-section`, `portable-atomic`, `minicbor`). Tudo o que é 0.x (Embassy, `embedded-io`, `esp-radio`, `probe-rs`) fica nos hospedeiros e nas ferramentas, atrás de módulos que se podem atualizar sem tocar no núcleo nem na ABI C.

---

## 6. Propostas de mudança às especificações

Prioridade: **alta** = antes da Fase 2 (Joker Micro); **média** = durante a Fase 2; **baixa** = quando houver ocasião.

| # | Documento / secção | Mudança | Prioridade |
|---|---|---|---|
| E1 | ref/06 §3.7, ref/08 §2.11.2, 19 §3 | tabela dos drivers do MCU com o trait de cada um e as lacunas (ADC, CAN async, 1-Wire, toque); traits `AdcRead` e `CanFrames` no `joker-hal` (§2.1) | alta |
| E2 | AGENTS.md, ADR 0020, ref/08 §2.4 e §2.11.1 | sincronização do núcleo só por `critical-section` e `portable-atomic` (`AtomicU64` para `seq` em MCUs de 32 bits); a ABI C diz que o integrador fornece a implementação de `critical-section`, ou ativa `cs-single-core` (§2.2) | alta |
| E3 | ref/03 §2.3.1, ref/08 §2.11.2 e §2.15.1 | chave `[board] chip` com o nome do chip como o HAL o escreve; features `chip-*` por família, com exatamente uma por build; `chip-esp32` para a bancada; coluna "HAL e versão" nos alvos (§2.3) | alta |
| E4 | ref/08 §2.11 (parágrafo novo) e §2.12.3 passo 6 | posse dos pinos no firmware genérico: tabela de pinos apagados construída no arranque a partir de `[pins]`, sem `steal`; sem `unsafe` nos módulos `chip-*` (§2.4) | alta |
| E5 | 19 §3, ref/08 §2.11 e §2.15.2 | exceção declarada para o rádio do ESP32: heap fixo (`esp-alloc`) e `esp-rtos` só no firmware com Wi-Fi, fora do núcleo, contados no `J0710`; medir o jitter com o rádio ativo (§5) | alta |
| E6 | 13 §1, 18 | política de toolchain: um `rust-toolchain.toml` por família (oficial, `esp`), versões fixadas das dependências 0.x, atualização uma vez por fase com a conformidade como porta; o Xtensa nunca é o único alvo da conformidade (§5) | alta |
| E7 | ref/08 §2.14, ref/07, 07 | `defmt` em campo: *build-id* do ELF no `.jkfw`, tramas `defmt` numa mensagem de registo do Joker Link, ELFs guardados no Hub, `jk logs --node <mcu>` (§2.5) | média |
| E8 | ref/06 §3 (secção nova), ref/06 §6 | adaptador de driver crates `embedded-hal-async` (`crate_driver!`), com as regras de prazos e cancelamento; testes com `embedded-hal-mock` (§2.7) | média |
| E9 | ref/03 §2.3.7 | `jk board new <nome> --chip <referência>` a partir dos metadados dos HALs (`stm32-metapac`, `esp-hal`, `embassy-rp`), com `# REVER` no que é da placa (§2.8) | média |
| E10 | 07, ref/08 §2.9.2 | `probe-rs` como biblioteca do `jk` (feature `probe`) para `jk flash`, `jk bench` e `jk trace --probe`; cabeçalho do nó encontrável por símbolo no ELF (§2.6) | média |
| E11 | ref/08 §2.11.5 | apps nativas compiladas com `forbid(unsafe_code)` e dependências numa lista aceite, à maneira das *capsules* do Tock (§3.5) | média |
| E12 | 23 §4 e §8 | o firmware de bancada da Fase 1 usa `esp-hal` com `unstable` (toque) e `esp-radio` beta: versões exatas no seu `Cargo.lock`, e a nota de que são APIs instáveis | média |
| E13 | ref/08 §2.12.2 e §2.12.4, ref/03 §2.3.5 | correspondência com o `embassy-boot` (`ACTIVE`/`DFU`/`STATE`, `mark_booted()`); `IMAGE_DEF` e arranque seguro da ROM no perfil `rp2350-pico2` (§2.9) | baixa |
| E14 | ref/03 §2.3, ADR 0026 | tipo de ficheiro novo para placas de expansão (`*.shield.toml`, à maneira dos *shields* do Zephyr), que acrescenta `[io]`, `[serial]`, `[pins]` a uma placa com o conector certo (§3.7) | baixa |
| E15 | ref/06 §2.10 (nota) | nomear os padrões do ecossistema que o Joker já segue (driver genérico sobre o trait, `split()`, tarefas geradas, mocks) (§2.10) | baixa |
| E16 | ref/06 §3.1 | explicar, ou mudar, o nome `joker-hal`: não é um HAL no sentido do ecossistema, é o contrato de driver; quem vem do `embedded-hal` confunde-os | baixa |
| E17 | 21 (1.4), 19 §5 | avaliar o isolamento por MPU (Tock, Hubris) como alternativa ou complemento ao `wasmi` para apps de terceiros no MCU, com reinício por app em vez de reset (§3.9) | baixa |

**As cinco primeiras (E1–E5) são as que mudam código na Fase 2.** As restantes melhoram a DX ou a operação e podem entrar depois, sem mudar a ABI C nem o formato do blob.

---

## Fontes

- [embedded-hal v1.0 now released! (Rust Embedded WG)](https://blog.rust-embedded.org/embedded-hal-v1/)
- [Roadmap for traits removed on 1.0.0 release (embedded-hal #357)](https://github.com/rust-embedded/embedded-hal/issues/357)
- [embedded-hal: migrating from 0.2 to 1.0](https://github.com/rust-embedded/embedded-hal/blob/master/docs/migrating-from-0.2-to-1.0.md)
- [embedded-io-async (crates.io, versões)](https://crates.io/crates/embedded-io-async/versions)
- [embedded-can-interface (docs.rs)](https://docs.rs/embedded-can-interface)
- [embedded-hal-mock](https://github.com/rust-embedded/embedded-hal-mock)
- [critical-section](https://github.com/rust-embedded/critical-section)
- [Heapless v0.9.1 has been released!](https://blog.rust-embedded.org/heapless-091/)
- [svd2rust (docs.rs)](https://docs.rs/svd2rust/latest/svd2rust/)
- [stm32-data (README)](https://github.com/embassy-rs/stm32-data/blob/main/README.md)
- [stm32-metapac (stm32-data-generated)](https://github.com/embassy-rs/stm32-data-generated/tree/main/stm32-metapac)
- [embassy-stm32 (crates.io, versões)](https://crates.io/crates/embassy-stm32/versions)
- [embassy-executor 0.10.0 (docs.rs)](https://docs.rs/crate/embassy-executor/latest)
- [embassy-sync 0.8.0 (docs.rs)](https://docs.rs/crate/embassy-sync/latest)
- [embassy-net 0.9.1 (docs.rs)](https://docs.rs/crate/embassy-net/latest)
- [embassy-boot (docs)](https://docs.embassy.dev/embassy-boot)
- [embassy-boot-stm32 0.8.0 (docs.rs)](https://docs.rs/crate/embassy-boot-stm32/latest)
- [embassy-rp (docs)](https://docs.embassy.dev/embassy-rp/)
- [Embassy: task macro (tarefas não genéricas)](https://docs.embassy.dev/embassy-executor/git/cortex-m/attr.task.html)
- [Embassy crates released and Rust stable support](https://embassy.dev/blog/embassy-hals-released/)
- [rp-hal](https://github.com/rp-rs/rp-hal)
- [esp-hal 1.0.0 release announcement (Espressif)](https://developer.espressif.com/blog/2025/10/esp-hal-1/)
- [esp-hal 1.0.0 beta announcement (Espressif)](https://developer.espressif.com/blog/2025/02/rust-esp-hal-beta/)
- [esp-hal releases](https://github.com/esp-rs/esp-hal/releases)
- [esp-hal gpio: AnyPin e degrade](https://docs.rs/esp-hal/latest/esp_hal/gpio/index.html)
- [esp-radio 1.0.0-beta.1](https://github.com/esp-rs/esp-hal/releases/tag/esp-radio-v1.0.0-beta.1)
- [esp-rtos (docs)](https://docs.espressif.com/projects/rust/esp-rtos/0.1.0/esp32/esp_rtos/index.html)
- [The Rust on ESP Book: RISC-V and Xtensa targets](https://docs.espressif.com/projects/rust/book/installation/riscv-and-xtensa.html)
- [espup: update LLVM and GCC (PR #577)](https://github.com/esp-rs/espup/pull/577)
- [probe-rs releases](https://github.com/probe-rs/probe-rs/releases)
- [probe-rs 0.32.0 (docs.rs)](https://docs.rs/crate/probe-rs/latest)
- [defmt is now 1.0 (Ferrous Systems)](https://ferrous-systems.com/blog/defmt-1-0-released/)
- [device-driver (livro)](https://diondokter.github.io/device-driver/book/v1/intro.html)
- [device-driver (GitHub)](https://github.com/diondokter/device-driver)
- [The Embedded Rust Book: GPIO e typestate](https://doc.rust-lang.org/stable/embedded-book/design-patterns/hal/gpio.html)
- [RTIC 2: migração para rtic-monotonics](https://rtic.rs/2/book/en/migration_v1_v2/monotonics.html)
- [Tock 2.2 Release](https://www.tockos.org/blog/2025/tock2.2/)
- [The Tock Book: Capsule Isolation](https://book.tockos.org/doc/threat_model/capsule_isolation)
- [Hubris: introdução (app.toml e tarefas estáticas)](https://github.com/oxidecomputer/hubris/blob/master/doc/intro.adoc)
- [Hubris Reference](https://hubris.oxide.computer/reference/)
- [Humility (depurador do Hubris)](https://github.com/oxidecomputer/humility)
- [Zephyr: Devicetree versus Kconfig](https://docs.zephyrproject.org/latest/build/dts/dt-vs-kconfig.html)
- [Zephyr: Devicetree bindings](https://docs.zephyrproject.org/latest/build/dts/bindings.html)
- [Practical Zephyr: Devicetree basics (Interrupt)](https://interrupt.memfault.com/blog/practical_zephyr_dt)
