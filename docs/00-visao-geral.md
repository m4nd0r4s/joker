# 00 — Visão geral

**Joker** é a runtime coringa: corre em qualquer dispositivo de edge, de um Raspberry Pi, um RevPi ou um PC industrial com Linux (`jokerd`) a um microcontrolador de poucos euros (Joker Micro, documento 19), e controla tudo o que o dispositivo toca. Lê sensores, comanda atuadores, fala os protocolos industriais e recolhe telemetria; na 1.1, corre também modelos de ML. É uma família: a runtime, o Joker Micro, o registo (Hub) e o plano de controlo da frota (Deck, documento 20). A quem programa, oferece uma única abstração: **sinais tipados, com unidade, qualidade e tempo**.

O nome vem da carta que vale por qualquer outra. Não serve literalmente para *tudo*: o documento 12 diz exatamente para onde não vai.

---

## A tese

Hoje há três famílias de software de edge, e cada uma resolve um terço do problema.

1. **Orquestradores de containers** (balena, AWS Greengrass, Azure IoT Edge). Tratam bem o deploy e mal o dispositivo. Não sabem o que é um registo Modbus, não têm noção de estado seguro, e cada container reinventa drivers, buffers e telemetria.
2. **Gateways de protocolos** (Eclipse Kura, EdgeX Foundry, Node-RED). Falam muitos protocolos, mas o ML entra como apêndice e a robustez é a de uma aplicação web. A EdgeX são vários microserviços em containers, o que num CM4 com 2 GB é caro. O Kura é Java/OSGi. O Node-RED é ótimo a prototipar e fraco a garantir.
3. **Runtimes de PLC** (CODESYS, TwinCAT, Siemens Industrial Edge). São deterministas e seguros, mas fechados e caros, e a DX é a da automação clássica. ML, telemetria moderna e software moderno ficam do lado de fora.

**O Joker junta os três terços num só processo, com uma regra que os três violam:**

> A aplicação nunca toca no mundo. Declara que sinais lê e que sinais quer escrever. A runtime é a dona do I/O, do tempo, da segurança e da telemetria. Cada escrita é uma *proposta*, que um envelope de segurança nativo aceita, limita ou rejeita antes de chegar ao hardware.

Desta regra saem as propriedades que interessam:

- as apps são **portáveis entre sites**, porque falam de portas e não de endereços;
- as apps são **substituíveis a quente**, porque o estado seguro não depende delas;
- são **testáveis sem hardware**, porque o mundo é uma imagem de processo que se pode simular e reproduzir;
- o ML pode **atuar sem se tornar perigoso**: o envelope limita o que o modelo pode fazer, embora não garanta que o faça bem.

---

## As sete apostas

| # | Aposta | Custo assumido |
|---|---|---|
| 1 | Uma só abstração: o **sinal** (valor + qualidade + tempo + unidade) | protocolos com semântica rica perdem detalhe; as ações (M19) tapam o resto |
| 2 | A app **não escreve no hardware**: propõe, e o envelope decide | uma camada a mais em cada escrita (~µs) |
| 3 | **Dois domínios de execução**: o ciclo determinista e o assíncrono | quem programa tem de escolher onde cada coisa corre |
| 4 | Apps em **WebAssembly** (Component Model, WASI 0.3) | overhead de fronteira; o SDK nativo em Rust fica como escape |
| 5 | **O relógio é um input** do núcleo, e o mundo é reproduzível | disciplina interna forte; o ML em GPU exige gravar as saídas (M22) |
| 6 | **O mesmo núcleo em Linux e em microcontroladores** (`no_std` desde o primeiro dia); o ML entra na 1.1 como recurso da runtime | disciplina de capacidades fixas no núcleo; o ML espera pela 1.1 |
| 7 | A **telemetria nunca bloqueia o controlo** | perdas possíveis, mas sempre contadas e nunca silenciosas |

---

## Os sete mecanismos que distinguem o Joker

**1. Imagem de processo com snapshot.** No início de cada ciclo as entradas são congeladas num snapshot consistente, e as saídas escritas durante o ciclo só são aplicadas no fim, todas de uma vez. Não há leituras rasgadas nem escritas a meio. É o modelo de um PLC, oferecido a código moderno.

**2. Envelope de segurança por saída.** Cada atuador declara a sua gama, a taxa de variação máxima, os interlocks, o estado seguro e o lease. O envelope corre em código nativo, depois da arbitragem e antes do driver, e **não há caminho que o contorne**, nem sequer um comando manual do CLI.

**3. Estado seguro obrigatório.** Uma saída sem `safe` declarado é um erro de validação: não há valor por omissão. Quando o escritor deixa de renovar o lease (porque a app morreu, o modelo encravou ou a rede caiu), a saída vai para o estado seguro, e a runtime verifica que o *fallback do próprio dispositivo* está configurado de forma coerente (M1).

**4. Portas e ligações.** Uma app declara no código portas tipadas com unidade (`grid_power: kW`). O manifesto do site liga cada porta a um sinal concreto (`meter.p_total`). Uma unidade incompatível é um erro de compilação, e unidades da mesma dimensão (W → kW) são convertidas pela runtime. A mesma app, sem recompilar, corre em cem sites com cem mapas de registos diferentes.

**5. Arbitragem com prioridades e leases.** Cada saída tem um árbitro com níveis de prioridade, à maneira do *priority array* do BACnet: `safety > manual > supervisor > optimizer`. Vence o lease ativo de maior prioridade, e um override manual expira sozinho. Resolve de raiz o conflito "o operador mexeu, o modelo desfez" (M15).

**6. Modo sombra.** Uma versão nova de uma app ou de um modelo corre em paralelo com a atual, com as mesmas entradas, mas as suas escritas são gravadas em vez de aplicadas. `jk shadow diff` compara as duas, e a promoção fica condicionada a critérios declarados. É o único caminho para pôr em produção algo que atua (M16).

**7. Reprodução determinista.** Como o relógio é um input e todo o I/O passa pela imagem de processo, uma gravação (`jk record`) chega para reproduzir exatamente o que uma app fez (`jk replay`), num portátil e sem hardware. Os bugs de campo passam a ser testes de regressão.

---

## A DX em três ecrãs

**Primeiro contacto: cinco minutos, sem hardware.**

```sh
jk new tanque --template sim-tank     # planta simulada: tanque, bomba, sensor de nível
cd tanque
jk dev                                # compila, arranca a runtime local, hot-reload
```

**Uma app completa em Rust:**

```rust
use joker::prelude::*;

#[joker::app]
struct Level {
    #[input(unit = "m")]    level: Input<f64>,
    #[output(unit = "%")]   pump:  Output<f64>,
    #[param(unit = "m", default = 1.5)] target: Param<f64>,
}

#[joker::cycle(period = "100ms")]
fn control(io: &mut CycleIo<Level>, _st: &mut ()) -> Result<()> {
    let err = io.level.get()? - io.target.get();     // `?`: qualidade má é erro, nunca um número
    io.pump.set((err * 80.0).clamp(0.0, 100.0));     // a bomba esvazia o tanque
    Ok(())
}
```

**Um erro, como o `jk check` o mostra:**

```
error[J0107]: unidade incompatível na ligação
  --> joker.toml:42:12
   |
42 | bind.soc = "pcs.p_ac"
   |            ^^^^^^^^^^ `pcs.p_ac` é uma potência (kW); a porta `soc` espera uma fração (%)
   |
   = ajuda: sinais com unidade % neste site: bms.soc, bms.soh, inv1.derate
```

O mesmo artefacto `.wasm` que corre no portátil contra a planta simulada corre no RevPi contra o hardware real. Só muda o manifesto.

---

## O que existe hoje, e onde o Joker entra

| Projeto | O que faz bem | O que lhe falta face ao Joker |
|---|---|---|
| Eclipse Kura | muitos protocolos (Modbus, OPC UA, S7) e MQTT | Java/OSGi, pesado; sem envelope de segurança; ML fraco |
| EdgeX Foundry | normalização de devices, ecossistema | microserviços em containers; latência entre serviços; sem ciclo determinista |
| Node-RED | milhares de nós de protocolo, prototipagem | robustez, tipos, testes, performance |
| AWS Greengrass / Azure IoT Edge | deploy e ML acoplados à cloud | não modelam o dispositivo; lock-in |
| balena | OTA e frotas de containers | não sabe nada de I/O |
| CODESYS / TwinCAT | determinismo, IEC 61131-3 | fechado, caro, DX clássica, ML externo |
| WasmEdge / wasmCloud | Wasm no edge | genéricos: sem sinais, sem drivers, sem envelope |
| ForestHub edge-agents | agentes de IA offline em RPi/Jetson | foco em agentes; protocolos industriais ainda no roadmap |

O Joker **não concorre** com o SO nem com o SCADA, e integra-se com os dois (documento 12). A gestão de frota é do **Joker Deck**, o plano de controlo da família (documento 20, ADR 0021), e o Joker fala com ele (ou com outro gestor) pelo agente (ref/10).

---

## Como ler esta documentação

| Documento | Para quem | Quando |
|---|---|---|
| **00** este | toda a gente | primeiro |
| **01 Arquitetura** | quem implementa | referência normativa |
| **02 Modelo de sinais** | toda a gente | antes de escrever uma app |
| **03 Manifesto** | quem configura sites | referência diária |
| **04 SDK** | quem escreve apps | referência diária |
| **05 Drivers** | quem integra hardware | referência |
| **06 ML** | quem põe modelos a correr (inclui a análise do Burn) | referência |
| **07 CLI** | toda a gente | referência diária |
| **08 Stress tests** | quem decide | antes de aceitar o desenho |
| **09 Arquitetura sob estresse** | quem implementa | antes de escrever o núcleo |
| **10 Mudanças de desenho** | quem acompanhou a v1 | depois do 08 e do 09 |
| **11 Roadmap** | quem planeia | — |
| **12 Limites** | **toda a gente** | **antes de prometer seja o que for** |
| **13 Plano de implementação** | quem começa a Fase 0 | — |
| **14 Classes e instâncias** | quem escreve lógica de negócio ou abstrações | depois do 04 |
| **15 Mapa de ficheiros** | toda a gente | quando te perderes entre ficheiros |
| **16 Semântica do núcleo** | quem implementa a Fase 0 | referência normativa |
| **17 IDL `.jkc`** | quem escreve classes | referência |
| **18 Prontidão v1** | quem decide | antes do primeiro commit |
| **19 Joker Micro** | quem integra MCUs | referência normativa |
| **20 Família e Deck** | quem planeia o produto | — |
| **21 Da v1 à v2** | quem planeia o produto | depois do 11 |
| **22 Rodada da API** | quem implementa ou integra | antes de abrir uma referência: o mapa das interfaces e as decisões D1–D23 |
| **23 Bancada** | quem desenvolve e testa agora | antes de ligar hardware: o que cada placa faz e o que se valida com ela |
| **24 Público e usos** | quem decide o produto | antes de priorizar a 1.x |
| **25 Ecossistema Rust embedded** | quem implementa o Joker Micro e os drivers | antes da Fase 2 |
| **26 Revisão final** | quem decide começar | **antes do primeiro commit** |
| **ref/** Referências | quem implementa uma interface | referência normativa da sua superfície, acima dos documentos por área; começa em `ref/00-convencoes.md` |
| **trilha/** | quem aprende | primeiro, se nunca usaste o Joker |
| **adr/** | quem questiona uma decisão | quando questionar |
| **exemplos/** | quem aprende | a seguir ao 02 |

---

## Glossário

| Termo | Significado |
|---|---|
| **Sinal** | grandeza nomeada e tipada, com unidade; cada leitura é uma **amostra** (valor, qualidade, tempo, seq) |
| **Qualidade** | `Good`, `Uncertain(motivo)` ou `Bad(motivo)`; herdada do OPC UA |
| **Caminho** | nome estável de um sinal: `dispositivo.grupo.sinal`, p. ex. `inv1.ac.p` |
| **Porta** | entrada, saída, parâmetro ou ação declarada por uma app, com tipo e unidade |
| **Ligação** (*binding*) | associação, no manifesto, entre uma porta e um sinal concreto |
| **Imagem de processo** | vetor denso com o último valor de cada sinal escalar, congelado por ciclo |
| **Grupo de ciclo** | conjunto de tarefas periódicas que partilham período, prioridade e snapshot |
| **Domínio de ciclo** | threads de tempo real onde correm os grupos de ciclo |
| **Domínio assíncrono** | runtime tokio onde correm tarefas por evento, ML, rede e historian |
| **Envelope** | gama, taxa, interlocks, estado seguro e lease de uma saída |
| **Lease** | validade temporal de uma escrita; expirado, o árbitro passa ao próximo nível ou ao estado seguro |
| **Árbitro** | decide, por prioridade, qual das escritas concorrentes numa saída é a efetiva |
| **Trip** | estado de uma saída forçada ao estado seguro até haver um reset explícito |
| **Fallback do dispositivo** | o que o próprio equipamento faz quando perde a comunicação; declarado e verificado |
| **Stream** | sinal de dados volumosos (imagens, nuvens de pontos) com buffers partilhados |
| **Sinal em bloco** | forma de onda: blocos de N amostras com t0 e período de amostragem |
| **Pipeline** | cadeia nativa stream → pré-processamento → modelo → pós-processamento → sinal |
| **Perfil** | mapa reutilizável de um equipamento: registos, escalas, unidades, fallback |
| **joker.lock** | resolução fixada de perfis, mapas descobertos, versões de firmware e hashes |
| **Modo sombra** | execução paralela sem efeito, com as escritas gravadas para comparação |
| **Traço** | gravação do snapshot de entradas e das decisões, suficiente para reproduzir |
| **Classe de recursos** | conjunto de CPUs, memória e aceleradores atribuído a um trabalho (`rt`, `ml`, `bg`) |
| **Ação** | porta do tipo comando (reset, iniciar calibração, método OPC UA), com resultado |
| **Classe** | contrato versionado de um tipo de equipamento para um programa (`led@1`, `pump@1`): membros, capacidades, métodos, consultas |
| **Instância** | um equipamento concreto que cumpre uma classe: implícita (perfil), composta (manifesto) ou virtual (app) |
| **Implementação ativa** | uma app que implementa uma classe com comportamento próprio, sobre sinais de mais baixo nível |
