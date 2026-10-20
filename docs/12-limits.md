# 12 — Limites

O que o Joker **não** faz, e porquê. Este documento é de **análise** (não normativo; a precedência está no `docs/README.md`): as regras vivem nos normativos e nas referências. Mesmo assim, prometer a um cliente algo que esteja aqui como "não" é um erro de venda, não de engenharia.

Na revisão de âmbito da v1, cada limite foi reavaliado com uma pergunta: **impacta-nos?** O resultado divide-os em quatro tipos:

| Tipo | Significado |
|---|---|
| **Verdade de fundo** | não é uma limitação do Joker, é uma propriedade do problema; nenhum produto a resolve |
| **Fora do nicho** | não é o que o Joker quer ser; outro produto resolve melhor e o Joker integra-se com ele |
| **Resolvido na v1** | era um limite, deixou de ser (sobretudo graças ao Joker Micro, documento 19) |
| **A atingir** | impacta o público-alvo; há um caminho, com fase marcada |

---

## Resumo

| # | Limite | Tipo | Resposta |
|---|---|---|---|
| 1 | Segurança funcional (SIL/PL) | a atingir, a longo prazo | Joker Micro + toolchain qualificada; decisão de produto |
| 2 | Tempo real duro | **resolvido na v1** | Joker Micro (ciclos até ~10 kHz) |
| 3 | Amostragem acima de ~1 kHz | **resolvido na v1**; formas de onda na 1.1 | Joker Micro com DMA; sinais em bloco na 1.1 |
| 4 | O envelope não garante boas decisões | verdade de fundo | sombra, KPIs, replay (ferramentas, não garantias) |
| 5 | Equipamentos acessíveis sem passar pelo Joker | a atingir, em parte | modo gateway, protocolos seguros; o resto é física da rede |
| 6 | Compilar para NPUs de fabricante | fora do nicho | com o ML na 1.1: transportar e executar, não compilar |
| 7 | Servidor de frota | **a atingir, na v1** | Joker Deck (documento 20) |
| 8 | Microcontroladores | **resolvido na v1** | Joker Micro (documento 19) |
| 9 | Contador certificado | fora do nicho | transportar dados assinados intactos |
| 10 | Enquadramento legal dos dados | verdade de fundo | mecanismos, não conformidade |
| 11 | Atualização do SO | fora do nicho; talvez depois | integração com RAUC/Mender; uma imagem de SO é uma hipótese futura |
| 12 | SCADA e HMI | fora do nicho; comissionamento a atingir | Studio (`jk ui`) na v1.x |
| 13 | Historian de planta | fora do nicho | uplink para historians existentes |
| 14 | Histórico e sessões de terceiros | fora do nicho | ações quando for preciso |
| 15 | Classes não tornam equipamentos intercambiáveis | verdade de fundo | conformidade reduz o risco, não o elimina |
| 16 | Redundância de nós (alta disponibilidade) | **a atingir**, v2 | ativo/standby por leases |
| 17 | Linguagens IEC 61131-3 | **a atingir**, v1.x | Structured Text compilado para Wasm e nativo |
| 18 | ML | adiado para a **1.1** | a v1 é construída preparada para ele |

---

## 1. Segurança funcional (SIL/PL)

**O limite.** O Joker não é certificado segundo IEC 61508 (SIL), ISO 13849 (PL) nem IEC 62061. Paragens de emergência, cortinas de luz, STO, monitorização segura de velocidade e proteções elétricas vivem em hardware e PLCs de segurança certificados. O nível `safety` do árbitro serve para proteções **de software**.

**Impacta-nos?** Hoje, pouco: nos nichos-alvo (energia, edifícios, máquinas), a segurança funcional já é de um relé ou de um PLC de segurança, e o Joker **lê** o seu estado. A prazo, pode ser interessante: um controlador de bateria certificável seria um produto diferente.

**Caminho, se um dia se quiser.** Não é certificar o `jokerd` (Linux não é certificável a níveis SIL úteis), é certificar um **subconjunto do Joker Micro**: o núcleo (`joker-core`, sem Wasm nem rede) num MCU com suporte de segurança, compilado com uma toolchain qualificada. A Ferrocene está qualificada pela TÜV SÜD para IEC 61508 SIL 3, e tem um subconjunto da `core` certificado a SIL 2. O núcleo determinista, sem alocação e com conformidade formal é exatamente o tipo de código que se certifica. **Custo:** anos e um orçamento de certificação. **Decisão:** de produto, não para a v1. O que se faz já é não fechar a porta: o `joker-core` fica pequeno, `no_std`, sem `unsafe` e com a bateria de conformidade.

## 2. Tempo real duro — resolvido na v1

**Antes:** o domínio de ciclo em Linux é *soft real-time* (~100 µs de jitter a 1 kHz com PREEMPT_RT).
**Agora:** o **Joker Micro** corre os grupos de ciclo em executores de interrupção com prioridade de hardware, com jitter de microssegundos e ciclos até ~10 kHz (a medir). O padrão recomendado para malhas rápidas é **nó Linux + MCU coprocessador**: o Linux decide, o MCU executa e segura as saídas.
**Continua fora:** malhas acima de ~10 kHz (FOC de motores, eletrónica de potência), que pertencem ao firmware do drive ou do conversor.

## 3. Amostragem de alta taxa — resolvido na v1

**Antes:** uma thread Linux não consegue amostrar a 25 kHz com timing útil.
**Agora:** um Joker Micro com ADC por DMA amostra ao ritmo do hardware e envia blocos ou features ao nó Linux (documento 19, U3). Os **sinais em bloco** (M7) chegam na 1.1, com o ML, porque o caso de uso principal (vibração) é de ML. Na v1, o MCU já pode calcular e enviar features escalares (RMS, picos).

## 4. O envelope limita ações, não garante boas decisões — verdade de fundo

Gama, taxa, permissivos, leases e orçamentos de atuação limitam **o que** uma app ou um modelo pode fazer, e nada disto garante que o faça **bem**. Nenhum sistema resolve isto: é a diferença entre segurança e qualidade. O Joker dá as ferramentas (modo sombra, KPIs, replay, rastreio causal), e a qualidade é de quem escreve e valida a lógica.

## 5. Equipamentos acessíveis sem passar pelo Joker — a atingir, em parte

**O limite.** Modbus TCP/RTU, BACnet e muitas instalações OPC UA não têm autenticação: quem está na rede de campo escreve no equipamento sem passar pelo Joker. As escritas de grupos atómicos não são transacionais no equipamento.

**Impacta-nos?** Sim. Em energia, com a diretiva NIS2 e os requisitos dos operadores de rede, a segurança da rede de campo é uma pergunta de todos os clientes.

**O que o Joker pode fazer (v1.x):**
- **modo gateway:** o nó com duas interfaces, a de campo sem rota para fora, e o `jk doctor --network` a verificar a configuração (firewall, sem forwarding, serviços expostos);
- **regra de cliente único** (documento 09, ataque 11) com as pontes para SCADA;
- suporte às variantes seguras dos protocolos quando o equipamento as tem: Modbus/TCP Security (TLS), OPC UA com `SignAndEncrypt`, BACnet/SC;
- com o **Joker Micro** como coprocessador, o I/O crítico deixa de estar numa rede: está num cabo série dentro do quadro.

**O que continua fora:** proteger um equipamento sem autenticação numa rede que o cliente não segmenta. Isso é física da rede, e fica escrito na documentação de instalação.

**O que já está fechado do lado do Joker:** a API local só aceita TCP com mTLS, com papéis e anti-replay (ref/09 §2, §8.4; ref/00 §9); o agente e um Deck comprometido estão no modelo de ameaças da ref/10 (§2.4, §4); as chaves, os papéis de assinatura e os segredos cifrados por destinatário estão na ref/02 (§2.4, §2.5) e nas ADRs 0024 e 0025.

## 6. Compilar modelos para NPUs — fora do nicho

Com o ML adiado para a 1.1 (limite 18), a posição mantém-se para essa altura: o Joker transporta, verifica e executa payloads de fabricante (HEF, TensorRT), mas não os compila. Os compiladores são proprietários e é aí que devem ficar.

## 7. Servidor de frota — a atingir, na v1

**Antes:** "é outro produto".
**Agora:** continua a ser outro produto, mas **da mesma família**, e o MVP entra na v1: o **Joker Deck** (documento 20), com inventário, estado desejado a partir de Git, rollouts com ondas e paragem por KPI, e saúde da frota. O nó continua a funcionar sem ele (local-first). O contrato entre os dois é o protocolo `agent/1` (ref/10).

## 8. Microcontroladores — resolvido na v1

O **Joker Micro** (documento 19): o mesmo núcleo `no_std`, as mesmas apps em Rust compiladas nativamente, três modos de entrega (firmware, biblioteca C, crate Rust) e o Joker Link para falar com os nós Linux e com o Deck. A ABI C e os artefactos estão na ref/08, e o Joker Link ao nível do fio na ref/07.

## 9. Contador certificado — fora do nicho

O Joker não certifica medições de energia para faturação (MID, *Eichrecht*). **O que pode fazer, e é barato:** transportar os registos **assinados pelo contador** (formatos como OCMF ou SML com assinatura) intactos, do contador ao destino, sem os alterar, para que a cadeia de prova continue a ser a do contador certificado.

## 10. Enquadramento legal dos dados — verdade de fundo

Imagens com pessoas, consumos de habitações e localização de veículos são dados pessoais. O Joker dá mecanismos (não gravar por omissão, retenção por política, cifra, controlo do que sai por uplink), e a base legal é de quem integra.

**Anonimização e partilha com terceiros — fora do nicho.** Pseudonimizar sites, anonimizar entre sites (k-anonimato, privacidade diferencial) e servir vários consumidores com controlo de acesso e consentimento é da plataforma de dados: o edge vê um site só, e anonimizar a sério exige ver a população. O Joker fica com o que evita a fuga à saída do nó (classificação dos sinais, saída negada por omissão, cifra do que sai, redação nos logs, retenção): documento 26, §8.

## 11. Atualização do SO — fora do nicho (por agora)

Kernel, bootloader, secure boot e partições do sistema ficam com RAUC, SWUpdate, Mender ou a distribuição da placa. O Deck integra-se com eles para atualizações coordenadas. **Hipótese para depois da v1:** uma imagem mínima de Linux ("Joker OS", Yocto ou Buildroot) com PREEMPT_RT, A/B e o `jokerd` pré-instalado, para as placas de referência. Resolveria de uma vez o kernel RT, o `isolcpus` e as atualizações. Só se justifica com frotas grandes a pedi-lo.

## 12. SCADA e HMI — fora do nicho; comissionamento a atingir

Sinópticos, ecrãs de operação e gestão de alarmes multi-site são de um SCADA ou de uma HMI, que se ligam pelas pontes ou pela API. **Mas** uma interface de **comissionamento** (painel de sinais, escritas manuais, traço, replay, estado das instâncias) é DX, e faz falta: o **Joker Studio** (`jk ui`, servido pelo nó ou pelo Deck), na v1.x, como cliente da API local (ref/09) e da biblioteca TypeScript no navegador (ref/05, 1.3).

## 13. Historian de planta — fora do nicho

O historian local guarda dias a meses num nó. O histórico de anos de uma planta vive num historian central (TimescaleDB, InfluxDB, PI…), e o Joker envia para ele.

## 14. Histórico e sessões de terceiros — fora do nicho

O histórico interno dos equipamentos vai-se buscar por ação quando for preciso. As sessões com identidade por operador não se propagam aos equipamentos: o Joker liga-se com uma identidade de serviço, e a identidade do operador (o principal da ref/09 §2.2) fica no registo de decisões.

## 15. As classes não tornam equipamentos intercambiáveis — verdade de fundo

Duas instâncias da mesma classe aceitam os mesmos métodos e não respondem da mesma maneira (rampas, tempos, falhas). A conformidade das classes garante um comportamento mínimo. Trocar um equipamento continua a exigir rever o envelope e correr os cenários.

## 16. Redundância de nós — a atingir, v2

**O limite (novo).** Um site crítico com um só nó tem um ponto único de falha. Hoje, a resposta é o fallback dos equipamentos e, com o Joker Micro, o `safe` no coprocessador.

**Caminho.** O desenho já tem as peças para **ativo/standby**: o árbitro com leases, as gerações imutáveis e o traço. Dois nós com a mesma geração; o ativo escreve, e o standby lê tudo e mantém a imagem de processo quente; a troca acontece quando o lease do ativo expira, com um árbitro de quórum (um terceiro nó leve, ou um MCU) a evitar o *split-brain*. É trabalho de v2, e não muda nada no núcleo da v1. As referências já reservam o que a v2 precisa: o primeiro segmento `@` nos caminhos (ref/00 §2), `/v2` na API (ref/09 §13) e `agent/2` com as chaves de site (ref/10 §3.14).

## 17. Linguagens IEC 61131-3 — a atingir, v1.x

**O limite (novo).** Engenheiros de automação escrevem em Structured Text (ST), e muitos não vão aprender Rust.

**Caminho.** O ST é, por desenho, cíclico e determinista, o que encaixa exatamente no domínio de ciclo. Existem compiladores de ST abertos sobre LLVM (por exemplo, o RuSTy, escrito em Rust). Um frontend ST com o SDK do Joker compilaria a mesma app para Wasm (Linux) e para nativo (Joker Micro), como uma linguagem tier 2. As classes aparecem ao ST como blocos funcionais. O ladder e o FBD ficam fora.

## 18. Machine learning — adiado para a 1.1

A v1 é a runtime que interage com os dispositivos. O ML (documento 06) passa para a **1.1**, e a v1 é construída preparada para ele (documento 11, §"Preparado para ML"): o pacote WIT reserva as interfaces `ml` e `frames` como `@unstable` (ref/04 §2.9), as classes de recursos incluem `ml`, o traço reserva as saídas de modelos, e o agente e o lock reservam o tipo de artefacto `.jkm`.
