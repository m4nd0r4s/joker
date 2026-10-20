# ref/08 — Joker Micro: ABI C, crate Rust e blob de geração

Referência normativa do Joker Micro visto por quem o integra. Aplica o `ref/00` sem o repetir e remete para o documento 16 em tudo o que é semântica. Os ficheiros máquina-legíveis são `spec/c/joker_node.h` (a ABI fixa), `spec/c/example/` (um header gerado e um programa de exemplo, compilados em CI) e `spec/cddl/joker-micro.cddl` (as estruturas CBOR). Se este documento e o header divergirem, é um bug a corrigir nos dois (ADR 0023, ponto 3).

---

## 1. Âmbito e fontes

**Cobre:**

1. a **ABI C** do modo biblioteca (`libjoker_node.a` + `joker_node.h` + `joker_project.h`): todas as funções, tipos, macros, contextos de chamada, erros, memória, pilha e o que é gerado por projeto;
2. o **crate Rust** `joker-micro` (modo crate, Embassy), o crate gerado `joker-project` e o contrato com o backend nativo do `joker-sdk`;
3. o **modo firmware**: o firmware genérico, as áreas de flash, o arranque, o OTA e a confirmação;
4. o **blob de geração** (`*.jkgen`): cabeçalho, secções, registos, ISA das expressões, assinatura COSE, capacidades, `J0710` e determinismo do `jk build`;
5. os **alvos M1/M2** e os orçamentos de RAM, flash e pilha;
6. os códigos novos `J0770`–`J0793` e `J-E2501`–`J-E2599`.

**Não cobre:** as mensagens do Joker Link (`ref/07`: a ABI só expõe bytes de entrada e de saída, §2.6.p), as chaves do manifesto (`ref/01`, a quem este documento pede chaves no §2.17), os perfis de placa (`ref/03`), a API do SDK para quem escreve apps (`ref/04`) e as configurações de cada driver (`ref/06`).

**Deriva de:** 19 (todo), 16 (todo), 01 §3–4, §7–8, §15, §18, §20, 02 (todo), 03 §6–§9, 04 §2–3, 17 §2, §4, §6, 20 §3, ADR 0007, 0015, 0020, 0022, e `ref/00`.

---

## 2. A referência

### 2.1 Conceitos

| Conceito | Definição |
|---|---|
| **nó** (`jk_node_t`) | toda a memória de uma instância do Joker Micro: registo, slots, imagens, árbitros, envelopes, estado das expressões, estado das apps, filas, traço. Estática, de tamanho fixo no build. Um MCU tem normalmente um nó; a ABI permite vários. |
| **geração** (`jk_generation_t`) | o blob compilado do projeto (§2.13), embutido na biblioteca (`JK_GENERATION`) ou lido de flash. É imutável e tem de ficar mapeado enquanto o nó o usar. |
| **interface externa** | o conjunto de identificadores que o código do integrador usa: sinais, grupos, escritores, instâncias, membros, alarmes, parâmetros e links. É **fixada no build da biblioteca** e ligada **por nome** ao registo de cada geração no `jk_init`. Um `JK_SIG_…` é um índice da interface, e não um `SignalId`: por isso continua válido quando uma geração nova chega por flash (§4, D2). |
| **escritor externo** | uma app `kind = "extern"` do manifesto (§2.17): código C do integrador que propõe escritas com nível e lease, como qualquer app. |
| **dispositivo externo** | um dispositivo `driver = "extern"`: os seus sinais são alimentados (`jk_set_input`) e aplicados (`jk_get_output`) pelo integrador. |
| **contexto** | onde uma função pode ser chamada (§2.4). |
| **modo de entrega** | firmware, biblioteca C ou crate Rust (19 §4). Os três usam o mesmo núcleo, o mesmo blob e a mesma interface. |

### 2.2 Ficheiros: o que é fixo e o que é gerado

| Ficheiro | Fixo ou gerado | Conteúdo | Produzido por |
|---|---|---|---|
| `joker_node.h` | **fixo** (muda só com a versão da ABI) | tipos, constantes, funções, macros genéricas | `spec/c/joker_node.h`, copiado pelo `jk build --lib` |
| `joker_project.h` | **gerado** por projeto e por build | `JK_SIG_…`, `JK_GROUP_…`, `JK_WRITER_…`, `JK_INST_…`, `JK_C_…`, `JK_ALARM_…`, `JK_PARAM_…`, `JK_LINK_…`, o tamanho do nó, o hash da interface, as pilhas medidas, `JK_GENERATION` | `jk build --target mcu:<placa> --lib` |
| `libjoker_node.a` | gerado | núcleo, apps Rust ligadas estaticamente, drivers pedidos, tabela da interface, geração embutida (se houver) | idem |
| `firmware.jkfw` | gerado | descritor do build (§2.14), CBOR | idem, e em todos os modos |
| `<site>.jkgen` | gerado | blob de geração assinado | `jk build --target mcu:<placa> --generation --firmware <firmware.jkfw>` |

O código do integrador inclui **só** `joker_project.h`, que inclui `joker_node.h`. O header fixo compila sozinho (C11 e C++17), mas `jk_node_t` só fica completo com o gerado, e a macro `jk_init` exige-o.

### 2.3 Versão da ABI

- `JK_ABI_VERSION_MAJOR` e `JK_ABI_VERSION_MINOR` (`ref/00` §1); `JK_ABI_VERSION` = `(major << 16) | minor` (a forma usada no documento 19); `jk_abi_version()` devolve a da biblioteca ligada.
- **v1.0 = ABI 1.0.** As builds anteriores à 1.0 usam a major 0 e não têm garantias.
- **Regra de compatibilidade** (ADR 0022, `ref/00` §1): numa minor só se acrescentam funções, constantes, bits de flags e campos **no fim** das estruturas; nenhuma estrutura existente muda de tamanho para o código compilado com uma minor anterior, porque a biblioteca sabe a minor do header pelo argumento `abi_version` do `jk_init_checked` e só escreve os campos que essa minor conhece. Uma função nunca muda de assinatura numa minor.
- `jk_init_checked` recusa (`JK_E_ABI`) uma major diferente, uma minor do header acima da da biblioteca, um `node_size` menor do que o necessário e um `iface_hash` diferente do da biblioteca (0 salta esta verificação).
- O header gerado tem um `_Static_assert` com a versão para a qual foi gerado.

### 2.4 Regras gerais

1. **Sem alocação.** Nenhuma função aloca. Toda a memória está no `jk_node_t` do integrador e nos buffers que ele passa.
2. **Sem threads, sem estado global escondido.** A biblioteca não cria tarefas nem temporizadores e não tem variáveis globais mutáveis (a `JK_GENERATION` embutida é `const`). O integrador define dois símbolos: `jk_now_us` e `jk_fatal` (§2.6.b).
3. **Reentrante por nó.** Dois nós nunca partilham estado. No mesmo nó, as chamadas seguem a tabela de contextos.
4. **O tempo vem de fora** (ADR 0007). Todas as funções do caminho determinista recebem `now_us`. A biblioteca só lê `jk_now_us()` em `jk_link_poll` (carimbos de receção e temporizadores do link) e no fim do `jk_tick` (duração, para estatísticas e para a política de overrun dos ticks **seguintes**, registada no traço).
5. **Unidades:** um valor é sempre expresso na unidade declarada do sinal (`ref/00` §4.1). A ABI não converte. O header gerado mostra a unidade em comentário.
6. **Qualidade explícita:** as entradas levam sempre qualidade; as leituras devolvem-na sempre (invariante 4 do `AGENTS.md`).
7. **Erros:** nunca abortam; devolvem `jk_status_t`. Só uma invariante interna partida chama `jk_fatal` (§2.6.b).
8. **Sincronização só por `critical-section` e `portable-atomic`** (invariante do núcleo, `AGENTS.md` §2.2). Os `seq` de 64 bits e os seqlocks dos slots (documento 01, §3.2) usam `portable_atomic::AtomicU64`, que nos alvos de 32 bits sem atómicos de 64 bits (Cortex-M33, Xtensa LX6/LX7, RISC-V `imac`) assenta numa secção crítica de poucas instruções; o núcleo nunca usa primitivas do executor nem locks. É isto que mantém o `[any]` limitado (§2.4.1). **No modo biblioteca, o integrador liga uma implementação de `critical-section`**: a do seu RTOS (FreeRTOS, Zephyr, ESP-IDF) ou a do seu HAL, ou ativa a feature `cs-single-core` quando o MCU tem um só núcleo e nenhuma interrupção chama o nó (a secção crítica é desligar as interrupções). Sem nenhuma das duas, a biblioteca não liga (falta o símbolo da implementação de `critical-section`). No modo firmware e no crate, a implementação é a do HAL da família (`chip-*`, §2.11.2).

#### 2.4.1 Contextos de chamada

| Contexto | Quem | Regra |
|---|---|---|
| `[any]` | qualquer contexto, **incluindo ISR**, em concorrência com tudo | sem espera (*wait-free*), limitado, pilha ≤ `JK_PROJECT_STACK_ISR` |
| `[group g]` | o contexto que faz o tick do grupo `g` | um só por grupo; grupos diferentes podem preemptar-se e correr em paralelo noutro núcleo (19 §3: um executor de interrupção por grupo) |
| `[service]` | o contexto de serviço do nó | **um só** por nó; pode ser preemptado pelos grupos e correr em paralelo com eles |
| `[excl]` | exclusivo | nenhuma chamada `[group]` ou `[service]` no mesmo nó ao mesmo tempo; as chamadas `[any]` concorrentes (ISR) são permitidas e devolvem `JK_E_STATE` enquanto a função exclusiva dura (exceto `jk_get_output*` durante um `jk_generation_swap`, que devolvem o estado anterior) |
| `[pure]` | sem estado do nó | qualquer contexto de tarefa; não em ISR (pilha) |

No padrão de um só laço (19 §4.2), todas as chamadas estão no mesmo contexto e não há nada a coordenar.

**Deteção de mau uso.** Cada contexto exclusivo (`[group g]`, `[service]`, `[excl]`) tem uma bandeira atómica de ocupado. Uma chamada que encontre a sua bandeira ocupada devolve `JK_E_REENTRANT` e não faz nada. Chamar `jk_tick` do mesmo grupo a partir de duas tarefas é portanto detetado, e não corrompe nada.

**Regra do escritor único por sinal.** `jk_set_input*` é `[any]` **desde que** cada sinal seja escrito sempre pelo mesmo contexto (documento 01, §3.2). Dois contextos a escrever o mesmo sinal é um erro do integrador que a biblioteca não consegue detetar sem custo; o resultado é uma das duas amostras, nunca uma amostra rasgada.

**Leitores sem espera.** Um leitor (latch, `jk_read`, `jk_get_output*`) nunca espera por um escritor. Se uma leitura colide com uma escrita em curso (uma ISR que preempta o próprio escritor), devolve a **amostra completa anterior** e conta a colisão (`latch_collisions`). É o que evita o bloqueio clássico de um seqlock num MCU de um núcleo, em que o leitor numa ISR de prioridade alta esperaria para sempre por um escritor que não pode correr (§4, D3).

#### 2.4.2 Tabela de funções por contexto

| Contexto | Funções |
|---|---|
| `[any]` | `jk_abi_version`, `jk_status_str`, `jk_set_input*`, `jk_read`, `jk_read_buf`, `jk_get_output*`, `jk_take_changed` (um consumidor por grupo), `jk_group_output_count`, `jk_group_output_at`, `jk_group_period_us`, `jk_group_stats`, `jk_last_outcome`, `jk_alarm_info`, `jk_param_get`, `jk_time_utc`, `jk_link_status`, `jk_service_next_us`, `jk_supervise`, `jk_force_safe`, `jk_safe_get`, `jk_trace_freeze`, `jk_node_info` |
| `[group g]` | `jk_tick(node, g, …)` |
| `[service]` | `jk_service`, `jk_link_poll`, `jk_link_poll_at`, `jk_link_reset`, `jk_propose*`, `jk_relinquish`, `jk_manual_write`, `jk_manual_release`, `jk_call_method*`, `jk_action_*`, `jk_event_emit`, `jk_event_next`, `jk_alarm_ack`, `jk_alarm_shelve`, `jk_alarm_unshelve`, `jk_trip_reset`, `jk_param_set`, `jk_param_clear`, `jk_time_set_utc`, `jk_generation_confirm`, `jk_persist_flush`, `jk_trace_size`, `jk_trace_read`, `jk_ota_*` |
| `[excl]` | `jk_init` / `jk_init_checked`, `jk_restart`, `jk_shutdown`, `jk_generation_swap` |
| `[pure]` | `jk_generation_open`, `jk_generation_boot`, `jk_generation_info` |

Os callbacks de `jk_platform_t` só são chamados a partir de `[excl]`, `[service]`, `jk_generation_open` (que lê a área `IDENTITY`) e `jk_generation_boot`, e podem bloquear (ms). Por isso estas duas correm no contexto de serviço ou antes do `jk_init`. `jk_now_us` é chamado a partir de `[group]` e `[service]`, e tem de ser chamável de qualquer contexto.

### 2.5 Tipos e constantes

#### 2.5.1 Identificadores

| Tipo | C | Largura | Origem | Nota |
|---|---|---|---|---|
| sinal | `jk_sig_t` | `u32` | `JK_SIG_<CAMINHO>` | índice da interface, não `SignalId` |
| grupo | `jk_group_t` | `u32` | `JK_GROUP_<NOME>` | ≤ 32 por nó na ABI; ≤ 8 na v1 (`J0776`) |
| escritor externo | `jk_writer_t` | `u32` | `JK_WRITER_<APP>` | |
| instância | `jk_inst_t` | `u32` | `JK_INST_<NOME>` | `ref/00` §5 |
| membro de classe | `jk_member_t` | `u16` | `JK_C_<PACOTE>_<CLASSE>_<MAJOR>_<MEMBRO>` | ligado por (classe, major, nome) |
| alarme | `jk_alarm_t` | `u32` | `JK_ALARM_<NOME>` | |
| parâmetro | `jk_param_t` | `u32` | `JK_PARAM_<APP>_<NOME>` | parâmetros de apps; os `param` de classes são sinais |
| link | `jk_link_id_t` | `u8` | `JK_LINK_<NOME>` | |
| bilhete de ação | `jk_ticket_t` | `u32` | `jk_action_invoke` | |
| nenhum | `JK_NONE` | `0xFFFFFFFF` | | |

#### 2.5.2 Enumerações

Todas com os valores do `ref/00`: `jk_type_t` (`JK_TYPE_BOOL` 0 … `JK_TYPE_BYTES` 8; 16 e 17 reservados para bloco e stream, v1.1), qualidade (`JK_QC_*` 0–2, `JK_QR_*` 0–19, 240–255 privados; `ref/00` §4.3), `jk_outcome_t` (`JK_OUTCOME_NONE` 0 … `JK_OUTCOME_TRIPPED` 9) e `jk_reject_t` (`JK_REJECT_WRONG_TYPE` 1 … `JK_REJECT_BUDGET` 8; `ref/00` §4.4), `jk_clock_q_t`, `jk_alarm_state_t`, `jk_severity_t`, `jk_role_t` (§6). Níveis: `jk_level_t` `u8`, 1–16 absolutos, `JK_LEVEL_NONE` = 0.

#### 2.5.3 Qualidade

`jk_quality_t` é o `u32` do slot: código nos bits 0–7, motivo nos bits 8–15, flags nos bits 16–31 (documento 01, §3.2).

| Macro | Valor |
|---|---|
| `JK_Q(code, reason)` | compõe |
| `JK_GOOD` | `JK_Q(JK_QC_GOOD, JK_QR_NONE)` = 0 |
| `JK_UNCERTAIN(r)`, `JK_BAD(r)` | atalhos |
| `JK_Q_CODE(q)`, `JK_Q_REASON(q)`, `JK_Q_FLAGS(q)`, `JK_Q_IS_GOOD(q)` | decompõem |
| `JK_QF_TS_SRC_MONO` (bit 16) | o `ts_src` é monotónico local, e não UTC |

Na **entrada**, a combinação código/motivo tem de ser uma das do documento 02, §4 (por exemplo, `Good` só com `none`, `overridden` ou `local_override`; `stale` com `Uncertain` ou `Bad`); os motivos 240–255 são aceites com qualquer código. Outra combinação é `JK_E_QUALITY`. As flags não definidas têm de ser 0 na entrada e são ignoradas (limpas); na saída podem vir a 1 em minors futuras, e o integrador deve usar as macros.

#### 2.5.4 Valor e amostras

| Estrutura | Campos | Regras |
|---|---|---|
| `jk_value_t` | `type`, `as.b`, `as.i` (i64 e enum com sinal), `as.u` (u64, bits, enum sem sinal), `as.f` (f64 e f32, promovido sem perda), `as.buf.{ptr,len}` (string UTF-8 sem NUL, bytes; `len` ≤ 64) | os campos `jk_reserved*` são 0; `buf` é emprestado só durante a chamada |
| `jk_sample_in_t` | `value`, `quality`, `ts_acq_us` (obrigatório), `ts_hw_us` (ou `JK_TS_HW_NONE`), `ts_src_us` (ou `JK_TS_SRC_NONE`) | documento 02, §5 |
| `jk_sample_t` | `value`, `quality` (já com o envelhecimento de `max_age`), `seq` (64 bits), `ts_acq_us`, `ts_hw_us`, `ts_src_us` | |
| `jk_output_t` | `value`, `quality`, `outcome`, `reject`, `level`, `flags` (`JK_OUTF_*`), `seq` | §2.6.h |
| `jk_outcome_info_t` | `outcome`, `reject`, `level`, `applied`, `tick_seq` | §2.6.i |

**Conversões aceites pelos setters** (sinal de destino → setters aceites). Tudo o resto é `JK_E_TYPE`, sem efeito.

| Tipo do sinal | `_bool` | `_i64` | `_u64` | `_f64` | `_f32` | `_buf` |
|---|---|---|---|---|---|---|
| `bool` | sim | 0 ou 1 (senão `JK_E_VALUE`) | 0 ou 1 | — | — | — |
| `i64` | — | sim | se ≤ `INT64_MAX` | — | — | — |
| `u64` | — | se ≥ 0 | sim | — | — | — |
| `f64` | — | sim (arredondado) | sim (arredondado) | sim | sim | — |
| `f32` | — | sim (arredondado) | sim (arredondado) | sim (arredondado a f32) | sim | — |
| `enum` | — | se for uma variante declarada | idem | — | — | — |
| `bits<N>` | — | se ≥ 0 e < 2ᴺ | se < 2ᴺ | — | — | — |
| `string` | — | — | — | — | — | UTF-8 válido, ≤ 64 B |
| `bytes` | — | — | — | — | — | ≤ 64 B |

Um valor fora do domínio (variante inexistente, string com mais de 64 B ou UTF-8 inválido) é `JK_E_VALUE`. Um `NaN` ou um infinito **não é erro**: o estágio de aquisição grava-o como `Bad(nan)` (documento 16, §4). Os **getters** de saídas exigem o tipo exato, com duas exceções: `_f64` lê um `f32`, e `_i64`/`_u64` leem um `enum` (valor numérico) ou um `bits` (`_u64`).

#### 2.5.5 Restantes tipos

Todos definidos no header; os campos `jk_reserved*` são 0 na entrada e ignorados na saída.

| Tipo | Onde se usa | Secção |
|---|---|---|
| `jk_generation_t`, `jk_generation_info_t`, `JK_GENF_*` | geração | §2.6.d |
| `jk_platform_t`, `jk_area_t` (`JK_AREA_*`), `jk_area_info_t`, `jk_notify_t` (`JK_NOTIFY_*`) | plataforma e flash | §2.6.c, §2.10 |
| `jk_group_stats_t` | estatísticas de grupo | §2.6.f |
| `jk_action_err_t` (`JK_ACTION_*`), `jk_action_req_t` (`JK_ACTREQF_*`), `jk_ticket_t` | ações | §2.6.j |
| `jk_event_kind_t` (`JK_EV_*`), `jk_event_t` (`JK_EVF_*`) | eventos | §2.6.j |
| `jk_alarm_info_t` | alarmes | §2.6.k |
| `jk_link_rx_fn`, `jk_link_tx_fn`, `jk_link_state_t` (`JK_LINK_*`), `jk_session_mode_t` (`JK_SESSION_*`, o `session_mode` da ref/00 §6), `jk_link_status_t` | link | §2.6.p |
| `jk_supervision_t` | supervisão | §2.6.n |
| `jk_ota_kind_t` (`JK_OTA_*`) | OTA local | §2.6.r |
| `jk_node_state_t` (`JK_NODE_*`), `jk_node_info_t` | diagnóstico | §2.6.s |

### 2.6 Funções

Cada função: assinatura (a do header), contexto, comportamento, erros. Os erros comuns a quase todas não se repetem: `JK_E_ARG` (ponteiro nulo, argumento fora de gama), `JK_E_STATE` (nó não iniciado ou já parado), `JK_E_HALTED` (nó parado por falha interna), `JK_E_REENTRANT` (§2.4.1), `JK_E_UNSUPPORTED` (feature fora do build).

#### a. Utilitários

| Função | Contexto | Comportamento |
|---|---|---|
| `uint32_t jk_abi_version(void)` | `[any]` | versão da biblioteca, empacotada como `JK_ABI_VERSION` |
| `const char *jk_status_str(jk_status_t st)` | `[any]` | o código do registo (`"J-E2512"`); `"ok"` para valores ≥ 0; `"J-E2599"` para um valor desconhecido. A string é estática. O texto explicativo está no `codes.toml` e aparece com `jk explain` |

#### b. Símbolos que o integrador define

| Símbolo | Obrigatório | Regras |
|---|---|---|
| `uint64_t jk_now_us(void)` | sim (erro de ligação se faltar) | µs monotónicos desde o arranque do MCU (`ref/00` §3); nunca decresce; chamável de qualquer contexto, incluindo ISR; sem espera. Um temporizador de 32 bits tem de ser estendido pelo integrador |
| `JK_NORETURN void jk_fatal(jk_status_t code, uint32_t detail)` | sim | chamada numa invariante interna partida ou num *panic* de código de apps ligadas estaticamente; **não pode voltar** (reset do MCU). Não há *sandbox* no MCU: um *panic* de app é fatal. As saídas ficam no estado de reset do hardware, que a placa declara seguro (`J0786`) |

`JK_NORETURN` é `_Noreturn` em C e `[[noreturn]]` em C++.

#### c. Plataforma (`jk_platform_t`)

Todos os campos são opcionais (`NULL`/0). A estrutura é lida no `jk_init` e **tem de continuar válida** enquanto o nó existir (a biblioteca guarda o ponteiro).

| Campo | Assinatura | Para quê | Sem ele |
|---|---|---|---|
| `user` | `void *` | contexto passado a todos os callbacks e ao link 0 | `NULL` |
| `area_info` | `(user, area, *info) → status` | tamanho, granularidade de apagamento e de escrita, valor apagado de cada área (§2.10) | nenhuma área: sem persistência, sem OTA, geração só embutida |
| `area_read` / `area_write` / `area_erase` | `(user, area, offset, buf/data, len) → status` | acesso bloqueante à flash; `offset` e `len` respeitam `write_size`/`erase_size` | idem |
| `area_map` | `(user, area) → const void *` | endereço XIP de uma área: os blobs de flash usam-se no sítio | sem gerações de flash geridas pela biblioteca (`jk_generation_boot`, `jk_generation_swap(…, NULL, …)` dão `JK_E_UNSUPPORTED`); o integrador ainda pode ler um blob para RAM alinhada e usar `jk_generation_open`. Todos os alvos da v1 têm flash mapeável |
| `link_key` | `(user, link, key_out[32]) → status` | chave do link a partir de um elemento seguro | a chave vem da área `JK_AREA_IDENTITY` (registo 17) |
| `kem_key` | `(user, key_out[32]) → status` | chave privada X25519 de segredos a partir de um elemento seguro (MCU autónomo, ref/02 §2.4) | a chave vem da área `JK_AREA_IDENTITY` (registo 19) |
| `notify` | `(user, what, arg)` | avisos do contexto de serviço (`JK_NOTIFY_*`, tabela abaixo) | nada |
| `trust_keys`, `trust_count` | `const uint8_t (*)[32]`, `u32` | as **âncoras** (chaves Ed25519 da raiz, ref/02 §2.5.4): assinam bundles de confiança. Sem bundle na área `IDENTITY` (registo 18), valem para todos os papéis | só gerações embutidas ou `dev` |

| `JK_NOTIFY_*` | `arg` | Quando |
|---|---|---|
| `GEN_READY` (1) | `gen_seq` baixo | uma geração recebida por OTA foi verificada e espera por `jk_generation_swap(node, NULL, …)` |
| `GEN_ROLLBACK` (2) | `gen_seq` baixo | a janela de confirmação expirou; o nó voltou à geração anterior |
| `GEN_CONFIRMED` (3) | `gen_seq` baixo | confirmada |
| `FW_READY` (4) | comprimento | firmware verificado em `JK_AREA_FW_STAGING`; o integrador marca o bootloader e reinicia quando quiser |
| `TIME` (5) | `jk_clock_q_t` | a qualidade do relógio mudou |
| `LINK` (6) | link | o estado de um link mudou |
| `TRACE_FROZEN` (7) | 0 | o anel de traço congelou (trip ou pedido) |
| `RESTART` (8) | 0 | o par pediu um reinício do nó (ref/07) |

#### d. Geração

| Função | Contexto | Comportamento |
|---|---|---|
| `jk_generation_open(gen, blob, len, platform, min_seq)` | `[pure]` | valida pela ordem: alinhamento de 8 (`JK_E_ALIGN`); cabeçalho e tabela de secções (`JK_E_GEN_FORMAT`); formato, ISA e secções obrigatórias conhecidas (`JK_E_GEN_VERSION`); CRC-32C e SHA-256 da imagem (`JK_E_GEN_INTEGRITY`); a cadeia de bundles da secção `TRUST`, se houver (assinada pelas âncoras ou pelo bundle instalado, com `version` crescente; `JK_E_GEN_TRUST`); se `SIGNED`, a assinatura COSE do *trailer* (§2.13.5, `JK_E_GEN_SIGNATURE`) por uma chave com o papel `config` no bundle efetivo (`JK_E_GEN_TRUST`); `gen_seq ≥ min_seq` (`JK_E_GEN_ROLLBACK`); a ligação ao nó, se o claim a tiver (`JK_E_GEN_NODE`, comparando com o nome na área `IDENTITY`); `board_id`, apps, drivers e `min_runtime` face ao build (`JK_E_GEN_FIRMWARE`); capacidades (`JK_E_GEN_CAPACITY`); o verificador de bytecode (§2.13.4, `JK_E_GEN_FORMAT`). Um blob sem assinatura só é aceite se tiver a flag `DEV` e o build não for `prod`. Não altera nenhum nó. |
| `jk_generation_boot(gen, platform)` | `[pure]` | escolhe e abre a geração de arranque a partir das áreas `GEN_A`/`GEN_B` e do estado da geração em `PERSIST` (§2.12.3, passos 4–5): a área ativa; a outra se a ativa estava `confirming` (um reinício durante a janela conta como falha) ou não valida; `min_seq` = `confirmed_seq`. `JK_E_NOT_FOUND` se nenhuma validar (o integrador usa então a embutida, se tiver, ou fica em modo de manutenção); `JK_E_UNSUPPORTED` sem `area_map` |
| `jk_generation_info(gen, info)` | `[pure]` | `gen_seq`, `source_hash` (o hash da geração), `registry_hash`, `image_hash`, tamanho, formato, `min_runtime`, flags `JK_GENF_*` |

`JK_GENERATION` (embutida) é construída pelo build com `EMBEDDED`: não passa pela verificação de assinatura (a integridade é a do firmware), mas passa por todas as outras no `jk_init`.

#### e. Ciclo de vida

| Função | Contexto | Comportamento |
|---|---|---|
| `jk_init(node, gen, platform, now_us)` (macro) → `jk_init_checked(node, sizeof *node, JK_ABI_VERSION, JK_PROJECT_IFACE_HASH, gen, platform, now_us)` | `[excl]` | verifica a ABI (§2.3) e o alinhamento do nó (`JK_E_ALIGN`); valida a geração (as verificações do `jk_generation_open` que ainda não foram feitas); **liga a interface** por nome ao registo (uma porta sem sinal compatível em tipo, unidade e direção é `JK_E_GEN_FIRMWARE`); constrói o nó; lê a persistência (trips, alarmes, parâmetros, `kv`, estado da geração); põe **todas as saídas em `safe`** (A8) e marca-as `CHANGED`; inicia o relógio em `Unsynced` (a época monotónica muda a cada arranque, e uma correspondência UTC anterior não vale); corre o `init` das apps. Pode chamar os callbacks de flash. Se falhar, o nó fica `JK_NODE_UNINIT` e pode tentar-se de novo. |
| `jk_restart(node, now_us)` | `[excl]` | reinício a quente com a mesma geração: igual ao `jk_init`, sem voltar a validar o blob |
| `jk_shutdown(node)` | `[excl]` | força todas as saídas a `safe`, grava a persistência (bloqueante), passa a `JK_NODE_SHUTDOWN`. Depois só `jk_get_output*`, `jk_safe_get`, `jk_node_info` e `jk_init` funcionam |
| `jk_generation_swap(node, next, confirm_ms, now_us)` | `[excl]` | troca de geração num limite de tick de **todos** os grupos (o integrador garante-o por ser exclusivo). `next = NULL` usa a geração recebida por OTA (`JK_NOTIFY_GEN_READY`), senão `JK_E_NOT_FOUND`. Mantém, para cada saída com o mesmo caminho, tipo e unidade: o último valor aplicado (A8), o estado de trip, e os leases dos escritores com a mesma identidade e o mesmo nível; as rampas de taxa continuam (E2). Mantém o estado de alarmes com o mesmo nome e a mesma condição compilada. Estado das apps: `migrate` se a versão mudou (documento 01, §9), senão mantido. Com `confirm_ms > 0`, a geração fica `confirming` (ADR 0015) e, se `jk_generation_confirm` não chegar a tempo, o contexto de serviço volta à anterior pelo mesmo mecanismo e avisa `GEN_ROLLBACK`. A geração anterior tem de continuar mapeada até à confirmação. |
| `jk_generation_confirm(node)` | `[service]` | confirma; grava `confirmed_seq` (anti-rollback); `JK_E_STATE` se nada estiver a confirmar |

#### f. Tick e grupos

| Função | Contexto | Comportamento |
|---|---|---|
| `jk_tick(node, group, now_us)` | `[group g]` | um tick completo, pelos passos do documento 01, §4, com as regras do documento 16: 1 latch (entradas externas, slots de outros grupos, envelhecimento por `now_us`); 2 drena a caixa de propostas do grupo (escritores externos, link, tarefas async das apps); 3 expressões do grupo (derivados, alarmes, permissivos, consultas); 4 tarefas de ciclo das apps nativas, pela ordem do grupo, com 4b para instâncias virtuais; 5a expansão de métodos; 5 árbitro; 6 envelope; 7 commit (slots, `CHANGED`, persistência de trips marcada para o serviço); 8 traço; 9 estatísticas. `now_us` é o instante do latch: é o `cx.now()` das apps e o valor gravado no traço. `now_us` menor do que o do tick anterior do mesmo grupo é `JK_E_TIME`, sem tick. Um tick atrasado não é erro: conta em `max_lateness_us`, e a política `overrun` do grupo (documento 01, §4) aplica-se com a duração medida por `jk_now_us()` no fim. Um `Err` de uma tarefa de app não se devolve aqui (fica no traço e nas estatísticas). |
| `jk_group_period_us(node, group)` | `[any]` | período da geração atual; 0 se o grupo não existir. O integrador agenda o tick com este valor, e não com uma constante, para acompanhar gerações novas |
| `jk_group_stats(node, group, out)` | `[any]` | `ticks`, `overruns`, `skipped`, `last_duration_us`, `max_duration_us`, `max_lateness_us`, `latch_collisions`, `inbox_dropped`; leitura coerente (versionada) |

#### g. Entradas

| Função | Contexto | Comportamento |
|---|---|---|
| `jk_set_input_bool/_i64/_u64/_f64/_f32(node, sig, v, q, ts_acq_us)` | `[any]`, escritor único por sinal | ingere uma amostra num sinal de entrada de um dispositivo `extern`, pelo estágio de aquisição do núcleo (documento 01, §3.3): conversões do §2.5.4, `NaN` → `Bad(nan)`, limites físicos → `Bad(out_of_range)`, `debounce`, deadband de aquisição (se a amostra cair no deadband, não conta como nova e o `seq` não sobe), `stuck_detect`. Marca o bit "alterado" dos grupos que a leem. Fica visível no **próximo latch** de cada grupo. |
| `jk_set_input_buf(node, sig, data, len, q, ts)` | idem | string ou bytes |
| `jk_set_input_ex(node, sig, s)` | idem | com `ts_hw_us` e `ts_src_us` |
| `jk_set_input(node, sig, v, q, ts)` | idem | macro `_Generic` (C11): `_Bool` → `_bool`; `float` → `_f32`; `double` → `_f64`; inteiros com sinal → `_i64`; sem sinal → `_u64`. `char` e `long double` não estão na lista (erro de compilação: usa-se um cast). Não existe em C++. |

Erros: `JK_E_SIGNAL` (índice fora da interface), `JK_E_DIRECTION` (o sinal não é uma entrada de um dispositivo `extern`), `JK_E_TYPE`, `JK_E_VALUE`, `JK_E_QUALITY`.

#### h. Leitura de sinais e saídas

| Função | Contexto | Comportamento |
|---|---|---|
| `jk_read(node, sig, now_us, out)` | `[any]` | último valor de qualquer sinal da interface (entradas, saídas aplicadas, derivados, alarmes, sinais expostos), com o envelhecimento calculado a `now_us` (documento 01, §3.3). Strings e bytes vêm com `len = 0`: usa-se `jk_read_buf` |
| `jk_read_buf(node, sig, now_us, out, buf, cap)` | `[any]` | idem, com cópia de string ou bytes para `buf` (`cap ≥ 64` chega sempre); `JK_E_FULL` se `cap` for curto |
| `jk_get_output_bool/_i64/_u64/_f64/_f32(node, sig, v, q)` | `[any]` | o **último valor aplicado** (commit) de uma saída de um dispositivo `extern`: é o que o integrador aplica ao hardware. `JK_OK` e `*v` escrito, ou `JK_RELINQUISHED` (a saída está em `safe = "relinquish"`: `*v` fica intacto e o integrador devolve o controlo à lógica anterior, documento 03, §6.1). `*q` é `Good`, `Uncertain(not_confirmed)` depois de uma verificação falhada, ou `Bad(tripped)` em trip. **O valor aplica-se sempre**, seja qual for `q`: é sempre admissível (o seguro, se for preciso). Antes do primeiro tick, é o `safe`. |
| `jk_get_output(node, sig, &v, &q)` | `[any]` | macro `_Generic` sobre o tipo de `&v`: `_Bool *`, `int64_t *`, `uint64_t *`, `double *`, `float *` |
| `jk_get_output_ex(node, sig, out)` | `[any]` | estado completo: valor, qualidade, resultado do tick, motivo, nível ativo, flags `JK_OUTF_*` e `seq` do commit |
| `jk_take_changed(node, group, &sig)` | `[any]`, um consumidor por grupo | a próxima saída `extern` do grupo cujo estado aplicado mudou desde a última chamada (valor, `safe`, trip, relinquish, forçado), ou `JK_EMPTY`. É para drivers em que escrever custa (Modbus, SPI): uma saída `Suppressed` nunca aparece aqui (E8). O integrador que escreve um GPIO em todos os ticks não precisa dela |
| `jk_group_output_count(node, group)`, `jk_group_output_at(node, group, i)` | `[any]` | as saídas `extern` de um grupo, para as levar ao seguro depois de um `jk_supervise` |

| `JK_OUTF_*` | Bit | Significado |
|---|---|---|
| `CHANGED` | 0 | mudou desde o último `jk_take_changed` deste grupo |
| `SAFE` | 1 | sem nível ativo: o valor é o `safe` (A6) |
| `RELINQUISH` | 2 | `safe = "relinquish"` em vigor |
| `TRIPPED` | 3 | em trip (E7) |
| `FORCED` | 4 | `force_safe` ativo (supervisor ou `jk_force_safe`) |
| `UNCONFIRMED` | 5 | `verify` falhou |
| `PULSE` | 6 | impulso em curso (`pulse`) |
| `MANUAL` | 7 | o nível ativo é um escritor manual |

Erros: `JK_E_SIGNAL`, `JK_E_DIRECTION` (não é uma saída `extern`, para os getters), `JK_E_TYPE`, `JK_E_GROUP`.

#### i. Escritas do firmware: escritores externos e manuais

| Função | Contexto | Comportamento |
|---|---|---|
| `jk_propose(node, writer, sig, v, now_us)` e `jk_propose_bool/_i64/_u64/_f64` | `[service]` | proposta de um escritor externo (app `kind = "extern"`) numa saída que ele declara em `writes`. Entra na caixa do **grupo dono** (A11) e é tratada no passo 2 do próximo tick desse grupo, antes das tarefas de ciclo (A4). O lease renova-se nesse tick (A2, A3). Numa saída que o escritor não declarou, a proposta chega ao árbitro e dá `rejected(not_owner)`, sem renovar lease (A3): é o resultado que a semântica define, e não um erro da chamada. Um sinal que não é saída é `JK_E_DIRECTION`. Caixa cheia: `JK_E_FULL`, contada em `inbox_dropped`. |
| `jk_relinquish(node, writer, sig, now_us)` | `[service]` | liberta o nível do escritor nessa saída no próximo tick (documento 01, §6) |
| `jk_last_outcome(node, writer, sig, out)` | `[any]` | o resultado da última proposta desse escritor nessa saída: `JK_PENDING` até o grupo dono ter feito o tick; depois `JK_OK` com `outcome`, `reject`, `level` (o nível que ganhou, para `overridden`), `applied`, `tick_seq` |
| `jk_manual_write(node, sig, v, level, ttl_ms, user_id, now_us)` | `[service]` | escrita manual de uma HMI local (documento 16, A10): `level` na faixa `manual` (3–5) e permitido na saída (senão `JK_E_LEVEL`); `ttl_ms` > 0 e ≤ `max_manual` (senão `JK_E_TTL`). Identidade `user:<user_id>`, papel `operator`. Gravada no registo de decisões e anunciada pelo link |
| `jk_manual_release(node, sig, level, now_us)` | `[service]` | liberta um nível manual |

#### j. Classes: métodos, ações e eventos

| Função | Contexto | Comportamento |
|---|---|---|
| `jk_call_method(node, writer, inst, method, args, nargs, now_us)` | `[service]` | acrescenta um registo de método à caixa do grupo dono de cada `out` atribuído; é expandido no passo 5a no nível do escritor (17 §4.1). `args` são `jk_value_t` pela ordem da declaração, até `JK_METHOD_MAX_ARGS` (8), convertidos como no §2.5.4. `JK_E_NOT_FOUND` (instância ou membro), `JK_E_ARGS` (número, tipo ou unidade), `JK_E_FULL` |
| `jk_call_method_cbor(…, cbor, len, now_us)` | `[service]` | idem, com os argumentos em CBOR determinístico (`method-args` no CDDL), para registos e listas |
| `jk_action_invoke(node, caller, inst, action, args, len, key, key_len, now_us, &ticket)` | `[service]` | invoca uma ação (17 §4.2): papel do escritor `caller` e permissivo avaliados na receção; deduplicação por `key` dentro de `dedup_window` (E9; 1–64 ASCII imprimível, senão `JK_E_ARGS`); fila serializada por dispositivo; timeout do manifesto. Devolve um bilhete. `JK_E_FULL` se houver `action_pending` bilhetes abertos |
| `jk_action_result(node, ticket, &err, buf, cap, &len)` | `[service]` | `JK_PENDING`, ou `JK_OK` com `err` (`JK_ACTION_OK`, `DENIED`, `TIMEOUT`, `DEVICE`, `INVALID`, `CANCELLED`) e em `buf` o resultado CBOR (sucesso) ou uma mensagem UTF-8 (erro). O bilhete liberta-se no `JK_OK`; `JK_E_FULL` se `cap` não chegar (o bilhete fica) |
| `jk_action_cancel(node, ticket)` | `[service]` | pede o cancelamento (só ações `@long`; as outras dão `JK_E_STATE`) |
| `jk_action_next(node, &req, args, cap)` | `[service]` | lado de quem **implementa**: o próximo pedido para uma ação implementada pelo firmware (`implementer = extern`), já autorizado e deduplicado; `JK_EMPTY` se não houver |
| `jk_action_progress(node, id, percent)` | `[service]` | progresso de uma ação `@long` |
| `jk_action_is_cancelled(node, id)` | `[service]` | `JK_OK` se o chamador cancelou, `JK_EMPTY` se não |
| `jk_action_complete(node, id, err, result, len)` | `[service]` | responde; depois de `deadline_us` a biblioteca já respondeu `TIMEOUT` e a resposta tardia dá `JK_E_NOT_FOUND` |
| `jk_event_emit(node, inst, event, cbor, len, now_us)` | `[service]` | emite um `event` de uma instância implementada pelo firmware (registo CBOR do `.jkc`); entregue a cada consumidor (apps `on_event`, link, fila do firmware); `JK_E_ARGS` se o CBOR não for um registo válido; fila cheia: perde-se e conta-se (`dropped`) |
| `jk_event_next(node, &ev, buf, cap)` | `[service]` | o próximo evento da fila do firmware: eventos de classe, transições de alarme (`alarm_from`, `alarm_to`, `severity`, flag `UNCERTAIN`) e eventos publicados por apps; `JK_EMPTY` se não houver; `JK_EVF_TRUNCATED` e `len` real se `cap` não chegar |

#### k. Alarmes e trips

| Função | Contexto | Comportamento |
|---|---|---|
| `jk_alarm_info(node, alarm, out)` | `[any]` | estado (`ref/00` §6), severidade, `shelved`, `suppressed`, `since_us`, `shelved_until_us` |
| `jk_alarm_ack(node, alarm, user_id, now_us)` | `[service]` | reconhecimento (documento 02, §11); `JK_E_STATE` se não houver nada a reconhecer |
| `jk_alarm_shelve(node, alarm, ttl_ms, user_id, reason, len, now_us)` | `[service]` | arquivo com TTL obrigatório (`JK_E_TTL` se 0); motivo UTF-8 ≤ 1 KiB |
| `jk_alarm_unshelve(node, alarm, user_id, now_us)` | `[service]` | |
| `jk_trip_reset(node, sig, user_id, now_us)` | `[service]` | o `jk reset` local (E7); efeito no próximo tick do grupo dono; `JK_E_STATE` se a saída não estiver em trip |

#### l. Parâmetros

| Função | Contexto | Comportamento |
|---|---|---|
| `jk_param_get(node, param, out)` | `[any]` | valor em vigor de um parâmetro de app (geração ou substituição) |
| `jk_param_set(node, param, v, flags, now_us)` | `[service]` | substituição em runtime, só para parâmetros declarados alteráveis (`runtime = true`, §2.17); valida tipo, unidade e gama da app (`JK_E_PARAM`); a app recebe-a no próximo tick (`on-params`). Com `JK_PARAMF_PERSIST`, sobrevive a reinícios enquanto a declaração do parâmetro não mudar. Toda a substituição é **deriva** face ao Git (documento 20, §2): anuncia-se no `HELLO` e aparece no `jk status` |
| `jk_param_clear(node, param, now_us)` | `[service]` | volta ao valor da geração e apaga a persistida |

#### m. Tempo

| Função | Contexto | Comportamento |
|---|---|---|
| `jk_time_set_utc(node, mono_us, utc_us, q)` | `[service]` | fixa a correspondência monotónico → UTC, quando a fonte do relógio da geração é `extern` (RTC, SNTP próprio de um M2). Com a fonte `link`, é o `TIME` do link que o faz, e esta função dá `JK_E_STATE` |
| `jk_time_utc(node, mono_us, &utc_us, &q)` | `[any]` | converte; `q` passa a `Holdover` sem atualizações e a `Unsynced` depois de `max_holdover` (documento 03, §3) |

#### n. Estado seguro e watchdog

| Função | Contexto | Comportamento |
|---|---|---|
| `jk_supervise(node, now_us, out)` | `[any]` (feita para uma ISR de temporizador) | para cada grupo, compara `now_us` com o último tick: > 1 período marca `late_groups`; > 3 períodos (documento 01, §20) ativa `force_safe` nas saídas do grupo e marca `forced_groups`. Com `force_safe`, `jk_get_output*` devolve o `safe` com `JK_OUTF_FORCED`, e o integrador que tem o hardware parado nesse grupo deve escrever o seguro diretamente (`jk_group_output_at` + `jk_safe_get`). O `force_safe` limpa-se no próximo tick completo do grupo. `feed_watchdog = 1` só se nenhum grupo estiver forçado, o nó não estiver parado e o serviço tiver corrido dentro de `service_deadline` (`service_late`). O integrador **só** alimenta o watchdog de hardware com `feed_watchdog = 1` |
| `jk_force_safe(node, on)` | `[any]` | força todas as saídas a `safe` (paragem de emergência, *brown-out*); volátil; o resultado dos commits é `tripped` com `Bad(tripped)` enquanto durar; `on = false` volta ao normal sem `reset` |
| `jk_safe_get(node, sig, out)` | `[any]` | o `safe` da saída (dado imutável da geração): `JK_OK` ou `JK_RELINQUISHED` |

#### o. Traço

| Função | Contexto | Comportamento |
|---|---|---|
| `jk_trace_freeze(node, freeze)` | `[any]` | congela (ou descongela) os anéis no próximo limite de tick; com `freeze_on_trip`, um trip congela-os sozinho e avisa `TRACE_FROZEN` |
| `jk_trace_size(node, &bytes)` | `[service]` | tamanho do `.jktrace` que `jk_trace_read` produz |
| `jk_trace_read(node, offset, buf, cap, &len)` | `[service]` | lê o anel congelado como um fluxo `.jktrace` (documento 16, §7), sem compressão (bit 0 das `flags`), com os tempos em **ns** (µs × 1000), um segmento por grupo, e `n_saídas_ML = 0`. `JK_E_STATE` se não estiver congelado. O mesmo fluxo vai pelo `TRACE` do link |

#### p. Joker Link

A ABI só expõe o ponto de integração: bytes de entrada e de saída. As mensagens, o enquadramento, a autenticação e a negociação são do `ref/07`.

| Elemento | Definição |
|---|---|
| `int32_t (*jk_link_rx_fn)(void *ctx, uint8_t *buf, uint32_t cap)` | não bloqueante. Link de **fluxo** (UART, RS-485, USB CDC, TCP): devolve quantos bytes quiser; o enquadramento (COBS + CRC-32C, D12, ref/07 §3.2) é da biblioteca. Link de **datagrama** (UDP; CAN com ISO-TP feito pelo integrador): exatamente uma mensagem por chamada. 0 = nada agora; < 0 = transporte em baixo |
| `int32_t (*jk_link_tx_fn)(void *ctx, const uint8_t *buf, uint32_t len)` | não bloqueante. Fluxo: bytes aceites (parcial permitido; a biblioteca guarda o resto). Datagrama: `len` ou 0 (tenta mais tarde). < 0 = em baixo |
| `jk_link_poll(node, rx, tx)` | `[service]`: link 0, com `ctx = platform->user`, `now = jk_now_us()`; corre **um passo de `jk_service`** no fim |
| `jk_link_poll_at(node, link, rx, tx, ctx)` | `[service]`: qualquer link, `ctx` explícito; idem |
| `jk_link_status(node, link, out)` | `[any]`: estado (`DOWN`, `HANDSHAKE`, `UP`, `AUTH_FAILED`, `VERSION_MISMATCH`), modo da sessão (`JK_SESSION_FULL`, `_OBSERVE`, `_MAINTENANCE`, `_REFUSED`: o `session_mode` da ref/00 §6, decidido pelo par, ref/07 §7.3), versão negociada, autenticado, último erro, e contadores **separados**: tramas recebidas e enviadas, CRC-32C inválido (`crc_errors`, `J-E2001`), etiqueta inválida (`auth_errors`, `J-E2101`), replay (`replay_drops`, `J-E2102`), outros erros de enquadramento e de mensagem (`rx_errors`), retransmissões e tramas descartadas no envio; `last_rx_us`. São os mesmos contadores que o `jk doctor` mostra do lado do nó Linux (ref/07 §3.1) |
| `jk_link_reset(node, link)` | `[service]`: fecha a sessão e recomeça o `HELLO` |

Os estados de `jk_link_state_t` são os da máquina do dispositivo da ref/07 §7.1: `IDLE` → `DOWN`; `OFFERED` e `CONFIRMING` → `HANDSHAKE`; `UP` → `UP` (com o modo da sessão em `session_mode`); `AUTH_FAILED` e `VERSION_MISMATCH` dizem porque é que o último handshake não chegou a `UP` (resposta `refused` com estado 27, ou três ofertas sem resposta válida; `refused` com estado 3, ref/07 §7.3), e voltam a `HANDSHAKE` na tentativa seguinte.

O tipo de cada link (fluxo ou datagrama), o transporte, a MTU, a autenticação e os níveis permitidos vêm da geração (secção `LINKS`, §2.13.3). O tamanho dos buffers vem das capacidades. `jk_link_poll` devolve `JK_OK`, `JK_E_LINK_DOWN` (callback < 0), `JK_E_LINK_AUTH`, `JK_E_LINK_VERSION` ou `JK_E_LINK` (enquadramento); nenhum destes é fatal, e o link recomeça sozinho.

**Quem manda nas saídas:** as propostas que chegam pelo link são escritores do MCU (tabela `WRITERS`, um por nível permitido do link: cada par (link, nível) é um escritor, ref/07 §8.1, 16 A12), com lease. Se o link cair, o lease expira no latch do grupo dono e o envelope aplica o `safe` (19 §6). Nada na ABI pode contornar isto.

#### q. Serviço

| Função | Contexto | Comportamento |
|---|---|---|
| `jk_service(node, now_us)` | `[service]` | um passo do domínio assíncrono do MCU: tarefas async das apps (`every`, `on`, `on_event`, `startup`; 19 §3), expiração dos TTL manuais e das janelas de confirmação, timeouts das ações, gravação dos registos persistentes sujos (trips, alarmes, parâmetros, `kv`), passos de OTA (verificação por pedaços), avisos `notify`. Nunca faz I/O de rede: isso é o `jk_link_poll` |
| `jk_service_next_us(node)` | `[any]` | o instante monotónico até ao qual o serviço deve correr outra vez (`UINT64_MAX`: nada pendente), para quem quer dormir entre ticks |
| `jk_persist_flush(node)` | `[service]` | grava já tudo o que está sujo (bloqueante) |

**Regra:** com persistência, tarefas async, ações, TTL manuais ou uma janela de confirmação, o integrador chama `jk_service` (ou `jk_link_poll`) pelo menos a cada `service_deadline` (omissão 100 ms). O `jk_supervise` conta os atrasos.

#### r. OTA local

Para integradores que recebem blobs ou firmware por um transporte próprio (BLE, MQTT do próprio firmware). O Joker Link tem o seu (`ref/07`), que usa o mesmo mecanismo.

| Função | Contexto | Comportamento |
|---|---|---|
| `jk_ota_begin(node, kind, total_len)` | `[service]` | `JK_OTA_GENERATION` escreve na área de geração inativa; `JK_OTA_FIRMWARE` em `JK_AREA_FW_STAGING`. Apaga a área (bloqueante). `JK_E_BUSY` se já houver um OTA em curso; `JK_E_GEN_CAPACITY` se `total_len` não couber |
| `jk_ota_write(node, offset, data, len)` | `[service]` | escreve; os pedaços chegam por ordem (`offset` igual ao total já escrito, senão `JK_E_OTA`) |
| `jk_ota_finish(node)` | `[service]` | verifica: para uma geração (o ficheiro `.jkgen` inteiro, imagem e *trailer*), como o `jk_generation_open` (com `min_seq = confirmed_seq + 1`); para firmware (imagem seguida do manifesto COSE), a assinatura com o papel `firmware`, o `board_id` e o `security_version`. Devolve `JK_PENDING` enquanto a verificação corre (por pedaços, para não bloquear o serviço), `JK_OK` e o aviso `GEN_READY` ou `FW_READY` no fim, ou `JK_E_OTA_VERIFY` e o erro original em `jk_link_status`/`jk_node_info` |
| `jk_ota_abort(node)` | `[service]` | |

#### s. Diagnóstico

`jk_node_info(node, out)` `[any]`: estado (`UNINIT`, `RUNNING`, `HALTED`, `SHUTDOWN`), qualidade do relógio, `confirming`, `gen_pending`, bytes do nó em uso, `gen_seq`, `iface_hash`, prazo de confirmação, `source_hash`, `registry_hash`, versão do bundle de confiança instalado (`trust_version`, 0 sem bundle) e `security_version` do firmware.

### 2.7 Códigos de estado

`jk_status_t` é `int32_t`. `JK_OK` = 0; os valores positivos são **informativos**; os negativos são erros, cada um com um código `J-E25xx` do registo (`jk_status_str`). Os mesmos códigos aparecem no `jk` e na API quando o `jokerd` os recebe de um MCU pelo link.

| Valor | Macro | Código | Gravidade | Mensagem curta |
|---|---|---|---|---|
| 0 | `JK_OK` | — | — | sucesso |
| 1 | `JK_EMPTY` | — | — | nada (mais) para devolver |
| 2 | `JK_PENDING` | — | — | ainda por decidir |
| 3 | `JK_RELINQUISHED` | — | — | saída em relinquish: sem valor a aplicar |
| −1 | `JK_E_ARG` | `J-E2501` | error | argumento inválido |
| −2 | `JK_E_STATE` | `J-E2502` | error | chamada fora do estado permitido |
| −3 | `JK_E_ABI` | `J-E2503` | error | ABI, tamanho do nó ou interface incompatíveis com a biblioteca |
| −4 | `JK_E_ALIGN` | `J-E2504` | error | nó ou blob sem alinhamento de 8 |
| −5 | `JK_E_UNSUPPORTED` | `J-E2505` | error | função não incluída neste build |
| −6 | `JK_E_REENTRANT` | `J-E2506` | error | chamada concorrente num contexto exclusivo |
| −7 | `JK_E_HALTED` | `J-E2507` | error | nó parado por falha interna |
| −10 | `JK_E_GEN_FORMAT` | `J-E2510` | error | blob mal formado |
| −11 | `JK_E_GEN_VERSION` | `J-E2511` | error | formato, ISA ou secção obrigatória desconhecidos |
| −12 | `JK_E_GEN_CAPACITY` | `J-E2512` | error | a geração não cabe nas capacidades do build (par de `J0710`) |
| −13 | `JK_E_GEN_SIGNATURE` | `J-E2513` | error | assinatura inválida ou em falta |
| −14 | `JK_E_GEN_TRUST` | `J-E2514` | error | chave não confiada |
| −15 | `JK_E_GEN_ROLLBACK` | `J-E2515` | error | `gen_seq` abaixo do mínimo (anti-rollback) |
| −16 | `JK_E_GEN_FIRMWARE` | `J-E2516` | error | placa, apps, drivers, runtime ou interface incompatíveis |
| −17 | `JK_E_GEN_NODE` | `J-E2517` | error | blob ligado a outro nó |
| −18 | `JK_E_GEN_INTEGRITY` | `J-E2518` | error | CRC ou SHA-256 da imagem não conferem |
| −20 | `JK_E_SIGNAL` | `J-E2520` | error | sinal fora da interface |
| −21 | `JK_E_DIRECTION` | `J-E2521` | error | direção errada para esta função |
| −22 | `JK_E_TYPE` | `J-E2522` | error | tipo incompatível |
| −23 | `JK_E_GROUP` | `J-E2523` | error | grupo desconhecido |
| −24 | `JK_E_TIME` | `J-E2524` | error | tempo a andar para trás |
| −25 | `JK_E_FULL` | `J-E2525` | error | fila ou buffer cheio |
| −26 | `JK_E_NOT_FOUND` | `J-E2526` | error | instância, membro, alarme, parâmetro, escritor ou bilhete desconhecido |
| −27 | `JK_E_ARGS` | `J-E2527` | error | argumentos de método ou ação inválidos |
| −28 | `JK_E_VALUE` | `J-E2528` | error | valor fora do domínio do tipo |
| −29 | `JK_E_QUALITY` | `J-E2529` | error | combinação de código e motivo de qualidade inválida |
| −30 | `JK_E_PARAM` | `J-E2530` | error | parâmetro fora da gama ou não alterável |
| −31 | `JK_E_TTL` | `J-E2531` | error | TTL em falta ou acima do máximo |
| −32 | `JK_E_LEVEL` | `J-E2532` | error | nível não permitido |
| −33 | `JK_E_BUSY` | `J-E2533` | error | operação já em curso |
| −40 | `JK_E_PERSIST` | `J-E2540` | error | callback de flash falhou |
| −41 | `JK_E_PERSIST_CORRUPT` | `J-E2541` | warning | registo persistente corrompido (ignorado) |
| −42 | `JK_E_PERSIST_FULL` | `J-E2542` | error | área de persistência cheia |
| −50 | `JK_E_LINK` | `J-E2550` | warning | erro de enquadramento ou de protocolo no link |
| −51 | `JK_E_LINK_AUTH` | `J-E2551` | error | autenticação do link falhou |
| −52 | `JK_E_LINK_VERSION` | `J-E2552` | error | versão do link incompatível |
| −53 | `JK_E_LINK_DOWN` | `J-E2553` | warning | transporte em baixo |
| −60 | `JK_E_OTA` | `J-E2560` | error | sequência de OTA inválida |
| −61 | `JK_E_OTA_VERIFY` | `J-E2561` | error | imagem recebida não verificou |
| −99 | `JK_E_INTERNAL` | `J-E2599` | error | invariante interna (código passado a `jk_fatal`) |

Os valores de −100 a −127 ficam para uso futuro da ABI; `J-E2600`–`J-E2999` ficam livres para o Joker Micro (drivers do MCU, OTA de firmware, v1.x).

### 2.8 O header gerado (`joker_project.h`)

Gerado pelo `jk build --target mcu:<placa> --lib` (e, com os mesmos nomes em Rust, pelo `--crate`, §2.11.4). Um exemplo completo está em `spec/c/example/joker_project.h`.

#### 2.8.1 Conteúdo

| Elemento | Forma | Nota |
|---|---|---|
| verificação da ABI | `_Static_assert(JK_ABI_VERSION_MAJOR == M && JK_ABI_VERSION_MINOR >= m, …)` | a versão para a qual foi gerado |
| identidade | `JK_PROJECT_NAME`, `JK_PROJECT_BOARD`, `JK_PROJECT_RUNTIME` (strings) | |
| hash da interface | `JK_PROJECT_IFACE_HASH` = `UINT64_C(0x…)` | os primeiros 8 B (little-endian) do SHA-256 da tabela canónica da interface; o mesmo valor está na biblioteca e no descritor (§2.14) |
| tamanho do nó | `JK_PROJECT_NODE_WORDS` e `struct jk_node { _Alignas(8) uint64_t jk_opaque[JK_PROJECT_NODE_WORDS]; };` | completa o tipo incompleto do header fixo |
| pilhas medidas | `JK_PROJECT_STACK_TICK`, `_SERVICE`, `_LINK_POLL`, `_ISR`, `_GEN_OPEN` (bytes) | §2.9.3 |
| geração embutida | `JK_PROJECT_HAS_EMBEDDED_GENERATION` (0 ou 1) e, com 1, `extern const jk_generation_t JK_GENERATION;` | com `--generic`, 0 |
| contagens | `JK_GROUP_COUNT`, `JK_SIG_COUNT`, `JK_LINK_COUNT` | |
| identificadores | uma macro por entrada da interface, com cast (`((jk_sig_t)4u)`), utilizável em `switch` e em `#if` não | tabela seguinte |

| Entrada | Nome | Exemplo |
|---|---|---|
| grupo | `JK_GROUP_<GRUPO>` | `JK_GROUP_MAIN` |
| sinal | `JK_SIG_<CAMINHO>` com `.` → `_` | `pump.run` → `JK_SIG_PUMP_RUN` |
| escritor externo | `JK_WRITER_<APP>` | `JK_WRITER_PANEL` |
| instância | `JK_INST_<INSTÂNCIA>` | `JK_INST_P1` |
| membro de classe | `JK_C_<PACOTE>_<CLASSE>_<MAJOR>_<MEMBRO>`, com `/` e `.` → `_` | `std/hvac.vav@1`, `damper` → `JK_C_STD_HVAC_VAV_1_DAMPER` |
| alarme | `JK_ALARM_<NOME>` | `JK_ALARM_TANK_HIGH` |
| parâmetro de app | `JK_PARAM_<APP>_<PARÂMETRO>` | `JK_PARAM_LEVEL_TARGET` |
| link | `JK_LINK_<NOME>` | `JK_LINK_UPLINK` |
| variante de enum | `JK_ENUM_<ENUM>_<VARIANTE>` (o nome do enum passa de `CamelCase` a `SNAKE`) | `PcsMode.charge` → `JK_ENUM_PCS_MODE_CHARGE` (valor numérico) |
| bit nomeado | `JK_BIT_<CAMINHO>_<BIT>` (índice do bit) | `JK_BIT_PCS_STATUS_ALARM_OVERTEMP` |

- Em cada tipo de entrada, os índices são **densos a partir de 0, pela ordem do nome** (o caminho canónico, para sinais). É isso que torna o header determinista.
- Cada macro tem um comentário com o caminho, o tipo, a unidade, a direção e, nas saídas, o grupo dono e o `safe` da geração com que foi gerado.
- Colisão de dois nomes na mesma macro (`a.b_c` e `a_b.c`) é `J0775`; o `jk build` sugere um alias. Um identificador com mais de 63 caracteres é o aviso `J0787` (é o limite garantido pelo C11 para nomes internos e macros).

#### 2.8.2 O que entra na interface

| Entrada | Quando | Obrigatória numa geração nova |
|---|---|---|
| sinais de dispositivos `extern` | sempre | **sim** |
| saídas declaradas em `writes` de apps `extern` | sempre | **sim** |
| sinais em `reads` de apps `extern` e em `[target.extern] expose` | se declarados | não |
| grupos | todos os da geração do build | **sim** |
| escritores externos | apps `kind = "extern"` | **sim** |
| links | todos | **sim** |
| instâncias e membros das suas classes | todas as instâncias da geração do build | não |
| alarmes | todos | não |
| parâmetros de apps | todos (os alteráveis marcados no comentário) | não |

Quando uma geração nova (por flash) não tem uma entrada **obrigatória** com o mesmo nome, tipo, unidade e direção, é recusada (`JK_E_GEN_FIRMWARE` no MCU, `J0790` no `jk build --generation`). Uma entrada **opcional** que falte fica "ausente": as funções que a recebem devolvem `JK_E_NOT_FOUND`, e o `jk build --generation` mostra-o como aviso na mesma mensagem do `J0790`.

### 2.9 Memória, alinhamento e pilha

#### 2.9.1 Memória do nó

- `jk_node_t` tem `JK_PROJECT_NODE_WORDS × 8` bytes, alinhamento de 8, e aloca-se estaticamente (`static jk_node_t node;`). Pode estar em qualquer RAM que todos os contextos que o usam vejam de forma coerente. Não precisa de estar a zeros: `jk_init` escreve-o todo.
- O tamanho é a soma das capacidades do build (§2.13.3) pelos custos da tabela seguinte, mais o estado das apps (`size_of` de cada app e dos estados das suas tarefas), arredondado a 8. O `jk check` usa as mesmas fórmulas para o `J0710` e mostra o detalhe por componente.

| Componente | RAM |
|---|---|
| slot por sinal | 32 B (+ 2 × 64 B de arena por sinal `string`/`bytes`) |
| imagem de processo | 16 B por porta latchada por grupo |
| saída (envelope, árbitro, caixa com buffer duplo) | 96 B + 24 B por escritor declarado |
| estado das expressões | o `state_len` de cada programa; janelas: 16 B por amostra |
| caixa de propostas | 24 B por entrada, por grupo |
| fila de métodos | 16 B + argumentos por registo, por grupo |
| ações pendentes | 48 B + argumentos (≤ 1 KiB) |
| eventos | 32 B + carga (≤ 1 KiB) por entrada |
| link | 2 × MTU + 256 B de estado (+ 64 B com cifra) por link |
| traço | `trace_bytes` (omissão 16 KiB, U9 do documento 19) |

#### 2.9.2 Cabeçalho do nó (para sondas)

Os primeiros 64 bytes do `jk_node_t` têm um formato fixo, para que o `jk trace-dump --probe` encontre o nó e os anéis de traço na RAM sem símbolos. O resto do nó é opaco.

| Offset | Tipo | Campo |
|---|---|---|
| 0 | `u32` | magia `"JKND"` (0x444E4B4A) |
| 4 | `u16`, `u16` | major e minor da ABI da biblioteca |
| 8 | `u32` | estado (`jk_node_state_t`) |
| 12 | `u32` | `JK_PROJECT_NODE_WORDS` |
| 16 | `u64` | hash da interface |
| 24 | `u64` | `gen_seq` |
| 32 | `u32` | offset (desde o início do nó) da tabela de traço: por grupo, `u32` offset do anel, `u32` tamanho, `u32` cabeça, `u32` flags (bit 0 congelado) |
| 36 | `u32` | número de grupos |
| 40 | `[8]u8` | primeiros 8 B do `registry_hash` |
| 48 | `[16]u8` | reservado, 0 |

#### 2.9.3 Alinhamento e pilha

| Regra | Valor |
|---|---|
| alinhamento do nó | 8 (`JK_E_ALIGN`) |
| alinhamento do blob | 8, em flash mapeada ou em RAM (`JK_E_ALIGN`) |
| buffers passados à ABI | qualquer alinhamento |
| escritas em flash | múltiplos de `write_size`, apagamentos de `erase_size`, dados da biblioteca com qualquer alinhamento |
| FPU em ISR | as funções `[any]` com `float`/`double` usam a FPU; a ISR tem de guardar o contexto da FPU (o *lazy stacking* do Cortex-M chega) |
| multi-núcleo | suportado (RP2350) se a memória do nó for coerente; num MCU com cache de dados e dois núcleos (STM32H745), o nó fica em memória não cacheável ou num só núcleo |

| Ponto de entrada | Pilha alvo (a medir) | Macro medida pelo build |
|---|---|---|
| `jk_set_input*`, `jk_get_output*`, `jk_read*`, `jk_supervise` e restantes `[any]` | ≤ 256 B | `JK_PROJECT_STACK_ISR` |
| `jk_tick` | ≤ 1,5 KiB + a maior pilha das tarefas de ciclo das apps do grupo | `JK_PROJECT_STACK_TICK` (a maior de todos os grupos) |
| `jk_service` | ≤ 2 KiB + a maior pilha das tarefas async | `JK_PROJECT_STACK_SERVICE` |
| `jk_link_poll` (inclui um passo de serviço) | ≤ 3,5 KiB + tarefas async | `JK_PROJECT_STACK_LINK_POLL` |
| `jk_generation_open`, `jk_ota_finish` (Ed25519) | ≤ 3 KiB | `JK_PROJECT_STACK_GEN_OPEN` |
| `jk_init`, `jk_generation_swap` | ≤ 2 KiB + o `init`/`migrate` das apps | incluído em `_SERVICE` |

As pilhas são medidas no build por análise estática do grafo de chamadas (sem recursão no núcleo; as apps com recursão ou chamadas indiretas não analisáveis dão o aviso `J0791` ("não medido"). Com grupos em executores de interrupção aninhados, o `jk check` soma as pilhas de todos os níveis de prioridade que podem estar ativos ao mesmo tempo e compara com a pilha da placa (`J0774`).

### 2.10 Persistência

#### 2.10.1 Áreas

| Área | Conteúdo | Tamanho mínimo | Quem escreve |
|---|---|---|---|
| `JK_AREA_PERSIST` | estado da geração, trips, alarmes (reconhecimento, arquivo), parâmetros persistentes, `kv` das apps, orçamentos de atuação | 2 setores de apagamento e ≥ 4 × o conteúdo vivo | a biblioteca, no serviço |
| `JK_AREA_GEN_A`, `JK_AREA_GEN_B` | um blob de geração cada (A/B) | o maior blob (`J0781`) | a biblioteca (OTA) ou o `jk flash` |
| `JK_AREA_FW_STAGING` | imagem de firmware recebida por OTA, para o bootloader | a imagem do firmware | a biblioteca (OTA) |
| `JK_AREA_IDENTITY` | nome do nó, chaves do link, bundle de confiança (≤ 2 KiB), chave X25519 de segredos (MCU autónomo) | 2 setores | o enrolamento (`jk node enroll <mcu> --link <d>`, ref/07 §4.7; ref/10 §3.1.3), o `jk flash --identity` e a instalação de bundles (secção `TRUST` do blob, ou o agente num MCU autónomo) |

No modo biblioteca, o integrador escolhe onde ficam as áreas (`area_info`); uma área com `size = 0` não existe.

**Níveis remotos não se persistem.** Os níveis dos escritores remotos (link), incluindo as escritas manuais que chegam pelo link, vivem só em RAM: depois de um reset do MCU desaparecem até o nó Linux os renovar. A persistência dos níveis manuais remotos (16 A14) é do nó Linux, que guarda o TTL e os envia com `lease = min(TTL restante, max_lease do link)` (16 A12, ref/07 §8.1). No modo firmware, a placa declara-as (§2.12.2).

#### 2.10.2 Formato

A área é um diário circular de setores. É a biblioteca que o define, para que um firmware novo leia os registos de um antigo.

```
setor    := cabeçalho(16 B) registo* espaço-apagado
cabeçalho:= "JKPS" u16:formato(=1) u16:flags u32:contador-de-apagamentos u32:crc32c(dos 12 B anteriores)
registo  := u8:tipo u8:flags(bit 0 = lápide) u16:tamanho-da-chave u16:tamanho-do-valor u16:0
            u32:seq(global, crescente) u32:crc32c(cabeçalho sem o crc + chave + valor)
            chave valor enchimento(até múltiplo de max(8, write_size), com o valor apagado)
```

- Inteiros em little-endian; **CRC-32C** (Castagnoli, D12), o mesmo do Joker Link (ref/07 §3.2) e do blob.
- **Chaves:** nunca `SignalId` (só vale numa geração; `ref/00` §5). Um sinal, alarme ou parâmetro é identificado pelo **path-hash**: os primeiros 8 bytes do SHA-256 do caminho canónico em UTF-8 (`alarm.<nome>`, `<app>.<parâmetro>`).
- **Valores:** CBOR determinístico, com os esquemas do `joker-micro.cddl` (§4), exceto o `kv`, que guarda os bytes da app.

| Tipo | Chave | Valor (CDDL) |
|---|---|---|
| 1 estado da geração | — (único) | `gen-state` |
| 2 trip | path-hash da saída | `trip-state` |
| 3 alarme | path-hash de `alarm.<nome>` | `alarm-persist` |
| 4 parâmetro | path-hash de `<app>.<parâmetro>` | `param-persist` |
| 5 `kv` | path-hash da app ‖ chave (≤ 64 B) | bytes (≤ 1 KiB por valor, ≤ 16 KiB por app; `J0784`) |
| 6 orçamento de atuações | path-hash da saída | `budget-persist` |
| 16 nome do nó | — | `identity-node` (área `IDENTITY`) |
| 17 chave de link | path-hash do nome do link | `identity-link-key` (área `IDENTITY`) |
| 18 bundle de confiança | — (único) | `identity-trust`: o `trust-bundle` COSE da ref/10 §2.4.3, perfil micro (≤ 2 KiB, ≤ 8 chaves; área `IDENTITY`) |
| 19 chave de segredos | — (único) | `identity-kem`: X25519 do MCU autónomo, atestada no seu certificado (ref/02 §2.4; área `IDENTITY`) |
| 7–15, 20–127 | reservados | |
| 128–255 | privados do integrador (a biblioteca ignora-os e preserva-os na compactação) | |

- **Leitura no arranque:** percorre-se a área; para cada (tipo, chave) vale o registo com o maior `seq` e CRC válido. Um registo com CRC errado é ignorado e contado (`JK_E_PERSIST_CORRUPT` nas estatísticas, alarme `joker.persist.corrupt`), e não impede o arranque.
- **Compactação:** quando o setor ativo enche, os registos vivos copiam-se para o próximo setor apagado, e o antigo apaga-se só depois de o novo estar completo. Uma falta de energia a meio deixa sempre um conjunto válido. Sem espaço depois de compactar: `JK_E_PERSIST_FULL` e alarme.
- **Desgaste:** o contador de apagamentos por setor distribui a escrita. O `jk check` estima os apagamentos por dia a partir das taxas declaradas (trips, `kv`) e avisa se a vida da flash ficar abaixo de 10 anos (`J0792`).

#### 2.10.3 Quando se grava

A escrita em flash demora milissegundos e **nunca** acontece num contexto de grupo. Um trip, um reconhecimento ou um parâmetro alterado marcam o registo como sujo, e o próximo `jk_service` grava-o. A janela entre o acontecimento e a gravação é, no máximo, o intervalo entre chamadas ao serviço (limitado por `service_deadline`). `jk_shutdown` e `jk_persist_flush` gravam tudo já. Esta diferença face ao Linux (documento 01, §7: trip com fsync) está declarada no §4, D5.

### 2.11 O crate Rust (`joker-micro`)

#### 2.11.1 Crates

| Crate | Papel | `no_std` |
|---|---|---|
| `joker-core`, `joker-expr`, `joker-units`, `joker-link-proto` | o núcleo partilhado com o Linux (ADR 0020) | sim |
| `joker-micro` | hospedeiro do núcleo em MCUs: nó, contextos, persistência, link, OTA, blob, traço; módulo `embassy`; drivers do MCU | sim |
| `joker-micro/ffi` (módulo) | a ABI C (`jk_*`), só com a feature `c-abi` | sim |
| `joker-sdk` com a feature `backend-native` | o SDK das apps, compilado nativamente (§2.11.5) | sim |
| `joker-project` | **gerado** pelo `jk build --crate`: interface tipada, geração, apps, armazenamento e tarefas Embassy do projeto | sim |

**Dependências do núcleo** (`joker-core`, `joker-expr`, `joker-units`, `joker-link-proto`): só crates estáveis (1.x) ou triviais, sem `alloc` no caminho do tick: `heapless`, `critical-section`, `portable-atomic` (com `critical-section` por baixo onde não há atómicos de 64 bits) e `minicbor`. A sincronização é só por estes dois (§2.4, regra 8). Tudo o que é 0.x (Embassy, `embedded-io`, `esp-hal` `unstable`, `esp-radio`) fica no `joker-micro` e nos módulos `chip-*`, atrás de interfaces que se atualizam sem tocar no núcleo nem na ABI C (documento 25, §5; documento 13, §5.1).

#### 2.11.2 Features de `joker-micro`

| Feature | Omissão | O que liga |
|---|---|---|
| `embassy` | sim | módulo `embassy` (tarefas, temporizadores, `embassy-time`) |
| `link` | sim | Joker Link (`joker-link-proto`), fluxo e datagrama |
| `link-crypto` | sim | ChaCha20-Poly1305 (obrigatório em IP; `ref/07`) |
| `gen-flash` | sim | abrir blobs de flash: Ed25519, SHA-256, COSE |
| `persist` | sim | persistência (§2.10) sobre `embedded-storage` |
| `trace` | sim | anéis de traço |
| `ota` | sim, **obrigatória** | OTA de gerações e de firmware. No modo firmware e no modo crate, o recetor de OTA faz sempre parte da imagem (§2.12.6): sem ela, o `joker-micro` não compila (`compile_error!` com `J0795`), a não ser com `c-abi`, em que o firmware do integrador é que leva a imagem |
| `defmt` | não | registos com `defmt` |
| `c-abi` | não | exporta os `jk_*` (modo biblioteca) |
| `cs-single-core` | não | implementação de `critical-section` por desligar as interrupções, para um MCU de um só núcleo em que nenhuma interrupção chama o nó (§2.4, regra 8); sem ela, no modo biblioteca, o integrador fornece a implementação |
| `drv-gpio`, `drv-adc`, `drv-pwm`, `drv-i2c`, `drv-spi`, `drv-uart`, `drv-modbus-rtu`, `drv-modbus-rtu-slave`, `drv-can`, `drv-onewire` | não | drivers do MCU, cada um sobre o trait da tabela da ref/06 §3.7 (`embedded-hal` 1.0 onde existe; `AdcRead` e `CanFrames` do `joker-hal` onde não); o `extern` está sempre presente |
| `chip-rp`, `chip-stm32`, `chip-esp` | não | ligação ao HAL de **uma família** (só no modo firmware): `embassy-rp`, `embassy-stm32` ou `esp-hal`. **Exatamente uma** por build: zero ou mais do que uma é `compile_error!`. A referência exata do chip não é uma feature do `joker-micro`: o `jk build` passa-a ao HAL (a feature do HAL, como `esp-hal/esp32` ou `embassy-stm32/stm32g474re`) a partir de `[board] chip` (ref/03 §2.3.1). O ESP32 original da bancada é `chip-esp` com `chip = "esp32"`. Com Wi-Fi, o `chip-esp` traz o heap fixo do rádio (§2.15.2). Os módulos `chip-*` não têm `unsafe` (§2.11.3, posse dos pinos) |

O `jk build` escolhe as features a partir do manifesto e da placa. Sem uma feature, as funções correspondentes existem e devolvem `Error::Unsupported` (`JK_E_UNSUPPORTED`), para que o mesmo código compile com qualquer conjunto.

#### 2.11.3 API pública

```rust
#![no_std]
// Tipos partilhados, reexportados do núcleo (os mesmos do SDK e do Linux):
pub use joker_core::{Quality, QualityCode, QualityReason, WriteOutcome, RejectReason,
                     ClockQuality, AlarmState, Severity, Role, Level, ValueType, Value};

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Micros(pub u64);          // monotónico, µs desde o arranque do MCU
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct UtcMicros(pub i64);

/// Um erro por `JK_E_*`; `code()` devolve "J-E25xx".
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Error { Arg, State, Abi, Align, Unsupported, Reentrant, Halted,
                 GenFormat, GenVersion, GenCapacity, GenSignature, GenTrust, GenRollback,
                 GenFirmware, GenNode, GenIntegrity, Signal, Direction, Type, Group, Time,
                 Full, NotFound, Args, Value, Quality, Param, Ttl, Level, Busy,
                 Persist, PersistCorrupt, PersistFull, Link, LinkAuth, LinkVersion, LinkDown,
                 Ota, OtaVerify, Internal }
impl Error { pub const fn code(self) -> &'static str; pub const fn status(self) -> i32; }

// Handles tipados (gerados em joker_project; o tipo apanha em compilação o que o C vê como JK_E_TYPE)
pub struct In<T: SignalType>(/* índice da interface */);   // entrada de um dispositivo extern
pub struct Out<T: SignalType>(/* … */);                    // saída
pub struct Sig<T: SignalType>(/* … */);                    // sinal só de leitura
pub struct GroupId(/* … */); pub struct WriterId(/* … */); pub struct LinkId(/* … */);
pub struct InstId(/* … */); pub struct Member(/* … */); pub struct AlarmId(/* … */);
pub struct ParamId<T: SignalType>(/* … */);
pub trait SignalType: Copy + sealed::Sealed { const TYPE: ValueType; }
// bool, i64, u64, f64, f32, enums #[derive(joker::Enum)], Bits<N>, Text (heapless::String<64>), Bytes (heapless::Vec<u8, 64>)

pub struct Sample<T> { pub value: T, pub quality: Quality, pub seq: u64,
                       pub ts_acq: Micros, pub ts_hw: Option<Micros>, pub ts_src: Option<UtcMicros> }
pub struct OutputState<T> { pub value: Option<T> /* None = relinquish */, pub quality: Quality,
                            pub outcome: WriteOutcome, pub level: Option<Level>, pub flags: OutFlags, pub seq: u64 }

// Geração
pub struct Generation<'a> { /* … */ }
impl<'a> Generation<'a> {
    pub fn open(blob: &'a [u8], trust: &[[u8; 32]], identity: Option<&str>, min_seq: u64)
        -> Result<Self, Error>;
    pub fn boot(storage: &'a dyn Storage, trust: &[[u8; 32]]) -> Result<Self, Error>;   // jk_generation_boot
    pub const fn embedded(blob: &'static AlignedBlob) -> Generation<'static>;
    pub fn info(&self) -> GenerationInfo;
}
#[repr(C, align(8))] pub struct AlignedBlob(pub [u8]);
#[macro_export] macro_rules! include_generation { ($path:literal) => { /* &'static AlignedBlob */ } }

// Plataforma (o jk_platform_t em Rust)
pub trait Platform: Sync {
    fn now(&self) -> Micros;                                  // = jk_now_us
    fn storage(&self) -> Option<&dyn Storage> { None }
    fn trust_keys(&self) -> &[[u8; 32]] { &[] }                     // âncoras (§2.13.5)
    fn link_key(&self, _link: LinkId) -> Option<[u8; 32]> { None }
    fn kem_key(&self) -> Option<[u8; 32]> { None }                  // X25519 de segredos (MCU autónomo)
    fn notify(&self, _n: Notify) {}
}
pub trait Storage: Sync {                                     // bloqueante; só [excl] e [service]
    fn info(&self, area: Area) -> Option<AreaInfo>;
    fn read(&self, area: Area, offset: u32, buf: &mut [u8]) -> Result<(), Error>;
    fn write(&self, area: Area, offset: u32, data: &[u8]) -> Result<(), Error>;
    fn erase(&self, area: Area, offset: u32, len: u32) -> Result<(), Error>;
    fn map(&self, _area: Area) -> Option<&'static [u8]> { None }
}
/// Adaptador sobre embedded-storage, com a tabela de áreas da placa.
pub struct NorStorage<F: embedded_storage::nor_flash::NorFlash> { /* … */ }
impl<F: embedded_storage::nor_flash::NorFlash> NorStorage<F> {
    pub fn new(flash: F, areas: &'static [(Area, u32 /*offset*/, u32 /*size*/)]) -> Self;
}

// O projeto (implementado pelo crate gerado)
pub trait Project: 'static {
    type Storage: NodeStorage;       // memória do nó: tipos concretos de capacidade fixa, sem arena nem unsafe
    type Apps: AppSet;               // as apps nativas do projeto (§2.11.5)
    const IFACE: &'static Iface;
    const IFACE_HASH: u64;
}

pub fn init<'n, P: Project>(storage: &'n mut P::Storage, apps: &'n mut P::Apps,
                            gen: &'n Generation<'n>, platform: &'n dyn Platform, now: Micros)
    -> Result<Parts<'n, P>, Error>;                         // [excl]

/// Capacidades separadas por contexto: o sistema de tipos faz o que o C faz com JK_E_REENTRANT.
pub struct Parts<'n, P: Project> {
    pub io: Io<'n, P>,              // Copy + Send + Sync: funções [any]
    pub service: Service<'n, P>,    // Send, !Sync: o contexto de serviço (um)
    pub groups: Groups<'n, P>,      // de onde se tiram os GroupRunner (um por grupo)
    pub changes: Changes<'n, P>,    // de onde se tiram os ChangeFeed (um por grupo)
}
impl<'n, P: Project> Groups<'n, P> {
    pub fn take(&mut self, g: GroupId) -> Option<GroupRunner<'n, P>>;
    pub fn give_back(&mut self, r: GroupRunner<'n, P>);
    pub fn is_complete(&self) -> bool;                      // todos devolvidos
}
impl<'n, P: Project> GroupRunner<'n, P> {                   // Send, !Sync: [group g]
    pub fn tick(&mut self, now: Micros) -> Result<(), Error>;
    pub fn id(&self) -> GroupId;
    pub fn period(&self) -> Micros;
}
impl<'n, P: Project> Changes<'n, P> { pub fn take(&mut self, g: GroupId) -> Option<ChangeFeed<'n, P>>; }
impl<'n, P: Project> ChangeFeed<'n, P> { pub fn next(&mut self) -> Option<OutAny>; }

impl<'n, P: Project> Io<'n, P> {                            // [any]
    pub fn set_input<T: SignalType>(&self, s: In<T>, v: T, q: Quality, ts_acq: Micros) -> Result<(), Error>;
    pub fn set_input_sample<T: SignalType>(&self, s: In<T>, v: T, q: Quality, ts_acq: Micros,
                                           ts_hw: Option<Micros>, ts_src: Option<UtcMicros>) -> Result<(), Error>;
    pub fn read<T: SignalType, S: Readable<T>>(&self, s: S, now: Micros) -> Result<Sample<T>, Error>;
    pub fn output<T: SignalType>(&self, o: Out<T>) -> Result<OutputState<T>, Error>;
    pub fn safe<T: SignalType>(&self, o: Out<T>) -> Option<T>;             // None = relinquish
    pub fn group_outputs(&self, g: GroupId) -> impl Iterator<Item = OutAny> + 'n;
    pub fn last_outcome<T: SignalType>(&self, w: WriterId, o: Out<T>) -> Option<OutcomeInfo<T>>;
    pub fn supervise(&self, now: Micros) -> Supervision;
    pub fn force_safe(&self, on: bool);
    pub fn trace_freeze(&self, on: bool);
    pub fn group_stats(&self, g: GroupId) -> GroupStats;
    pub fn alarm(&self, a: AlarmId) -> Result<AlarmInfo, Error>;
    pub fn param<T: SignalType>(&self, p: ParamId<T>) -> Result<T, Error>;
    pub fn utc(&self, at: Micros) -> (Option<UtcMicros>, ClockQuality);
    pub fn link_status(&self, l: LinkId) -> LinkStatus;
    pub fn node_info(&self) -> NodeInfo;
    pub fn service_next(&self) -> Option<Micros>;
}

impl<'n, P: Project> Service<'n, P> {                       // [service]
    pub fn poll(&mut self, now: Micros) -> Result<(), Error>;                            // jk_service
    pub fn link_stream<T>(&mut self, l: LinkId, io: &mut T, now: Micros) -> Result<(), Error>
        where T: embedded_io::Read + embedded_io::ReadReady + embedded_io::Write + embedded_io::WriteReady;
    pub fn link_datagram<D: Datagram>(&mut self, l: LinkId, io: &mut D, now: Micros) -> Result<(), Error>;
    pub fn link_reset(&mut self, l: LinkId);
    pub fn propose<T: SignalType>(&mut self, w: WriterId, o: Out<T>, v: T, now: Micros) -> Result<(), Error>;
    pub fn relinquish<T: SignalType>(&mut self, w: WriterId, o: Out<T>, now: Micros) -> Result<(), Error>;
    pub fn manual_write<T: SignalType>(&mut self, o: Out<T>, v: T, level: Level, ttl_ms: u32,
                                       user: u32, now: Micros) -> Result<(), Error>;
    pub fn manual_release<T: SignalType>(&mut self, o: Out<T>, level: Level, now: Micros) -> Result<(), Error>;
    pub fn call_method(&mut self, w: WriterId, i: InstId, m: Member, args: &[Value], now: Micros) -> Result<(), Error>;
    pub fn call_method_cbor(&mut self, w: WriterId, i: InstId, m: Member, cbor: &[u8], now: Micros) -> Result<(), Error>;
    pub fn invoke_action(&mut self, caller: WriterId, i: InstId, a: Member, args_cbor: &[u8],
                         key: &str, now: Micros) -> Result<Ticket, Error>;
    pub fn action_result(&mut self, t: Ticket, buf: &mut [u8]) -> core::task::Poll<Result<ActionReply, Error>>;
    pub fn cancel_action(&mut self, t: Ticket) -> Result<(), Error>;
    pub fn next_action_request(&mut self, args: &mut [u8]) -> Option<ActionRequest>;
    pub fn action_progress(&mut self, id: u32, percent: u8) -> Result<(), Error>;
    pub fn action_cancelled(&self, id: u32) -> bool;
    pub fn complete_action(&mut self, id: u32, r: Result<&[u8], (ActionError, &str)>) -> Result<(), Error>;
    pub fn emit_event(&mut self, i: InstId, e: Member, cbor: &[u8], now: Micros) -> Result<(), Error>;
    pub fn next_event(&mut self, buf: &mut [u8]) -> Option<Event>;
    pub fn alarm_ack(&mut self, a: AlarmId, user: u32, now: Micros) -> Result<(), Error>;
    pub fn alarm_shelve(&mut self, a: AlarmId, ttl_ms: u32, user: u32, reason: &str, now: Micros) -> Result<(), Error>;
    pub fn alarm_unshelve(&mut self, a: AlarmId, user: u32, now: Micros) -> Result<(), Error>;
    pub fn trip_reset<T: SignalType>(&mut self, o: Out<T>, user: u32, now: Micros) -> Result<(), Error>;
    pub fn param_set<T: SignalType>(&mut self, p: ParamId<T>, v: T, persist: bool, now: Micros) -> Result<(), Error>;
    pub fn param_clear<T: SignalType>(&mut self, p: ParamId<T>, now: Micros) -> Result<(), Error>;
    pub fn time_set_utc(&mut self, mono: Micros, utc: UtcMicros, q: ClockQuality) -> Result<(), Error>;
    pub fn confirm_generation(&mut self) -> Result<(), Error>;
    /// [excl] no Rust: exige todos os GroupRunner devolvidos (Groups::is_complete).
    pub fn swap_generation(&mut self, groups: &mut Groups<'n, P>, next: Option<&'n Generation<'n>>,
                           confirm_ms: u32, now: Micros) -> Result<(), Error>;
    pub fn persist_flush(&mut self) -> Result<(), Error>;
    pub fn trace_size(&self) -> Result<u32, Error>;
    pub fn trace_read(&self, offset: u32, buf: &mut [u8]) -> Result<usize, Error>;
    pub fn ota_begin(&mut self, kind: OtaKind, total_len: u32) -> Result<(), Error>;
    pub fn ota_write(&mut self, offset: u32, data: &[u8]) -> Result<(), Error>;
    pub fn ota_finish(&mut self) -> core::task::Poll<Result<(), Error>>;
    pub fn ota_abort(&mut self);
}

pub trait Datagram { fn recv(&mut self, buf: &mut [u8]) -> Result<Option<usize>, Error>;
                     fn send(&mut self, buf: &[u8]) -> Result<bool, Error>; }
```

- As regras são as da ABI C, função a função (a ABI C é uma camada fina por cima deste crate).
- **Sem `unsafe` fora do módulo `ffi`**, e isto vale também para os módulos `chip-*`. A memória do nó é um tipo concreto gerado (`P::Storage`), com arrays de capacidade fixa, e não uma arena; as apps vivem em `P::Apps`, também concreto.
- **Posse dos pinos no firmware genérico.** O firmware genérico só sabe que pino é o LED quando lê o blob, em runtime, e os HALs dão os periféricos uma vez, como *singletons*, com os pinos escolhidos em compilação. O `joker-micro` **nunca** usa `steal` (`AnyPin::steal(n)` e semelhantes são `unsafe` e contornam a posse). Em vez disso, no passo 6 do arranque (§2.12.3), o módulo `chip-*` recebe os periféricos do `init` do HAL, converte **todos** os pinos declarados em `[pins]` da placa num tipo apagado (`degrade()` para `AnyPin` no `esp-hal`, `Peri<AnyPin>` no `embassy-rp`, `AnyPin` no `embassy-stm32`) e guarda-os numa **tabela de pinos** indexada pelo nome da placa. Cada driver tira o seu pino da tabela **uma vez**; um segundo pedido do mesmo pino é `JK_E_GEN_FIRMWARE` (o `jk check` já o recusa antes, com `J0829`, ref/03 §2.3.3). Um pino que não está em `[pins]` não existe para o firmware.
- Um *panic* numa app ou no núcleo é tratado pelo *panic handler* do firmware (recomendado: `panic-reset`); a biblioteca não o apanha. No modo biblioteca, o *panic handler* do `libjoker_node.a` chama `jk_fatal(JK_E_INTERNAL, …)`.

#### 2.11.4 O crate gerado (`joker-project`)

`jk build --target mcu:<placa> --crate` gera, em `target/joker/crate/`, um crate chamado `joker-project` que o firmware do integrador usa por caminho (`joker-project = { path = "…" }`):

| Item | Conteúdo |
|---|---|
| `pub struct Project;` e `impl joker_micro::Project for Project` | tipos de armazenamento e de apps, interface, hash |
| `pub type Storage`, `pub type Apps` | os tipos concretos |
| `pub mod sig { pub const TANK_LEVEL: In<f32> = …; pub const PUMP_RUN: Out<bool> = …; … }` | os mesmos nomes do C, sem o prefixo `JK_SIG_` |
| `pub mod group`, `writer`, `inst`, `member`, `alarm`, `param`, `link`, `enums` | idem |
| `pub static GENERATION: Generation<'static>` | só com geração embutida |
| `pub const NODE_BYTES: usize`, `pub const STACK_*: usize` | como no C |
| `pub mod tasks` (feature `embassy`) | `#[embassy_executor::task] group_task(GroupRunner<'static, Project>)`, `service_task(…)`, `supervisor_task(…)`: o Embassy não aceita tarefas genéricas, por isso são geradas aqui |
| `pub fn init(storage, apps, platform, now) -> Result<Parts<'static, Project>, Error>` | atalho para `joker_micro::init::<Project>` com a geração embutida |

#### 2.11.5 Contrato com o `joker-sdk` (backend nativo)

A mesma app (19 §5) compila com `joker-sdk` e a feature `backend-native` (em vez da `backend-wasm`, a de omissão). A macro `#[joker::app]` gera então, em vez dos exports WIT:

```rust
pub trait NativeApp: Sized + 'static {                // implementado só pela macro
    const META: &'static [u8];                        // o mesmo CBOR da secção joker:meta (ref/04 §5)
    type Cycle: Default;                              // estado de cada tarefa de ciclo (tuplo gerado)
    type Tasks;                                       // futuros das tarefas async, de tamanho fixo
    fn init(binder: &mut Binder<'_>, params: &ParamSet<'_>) -> Result<Self, InitError>;
    fn on_params(&mut self, params: &ParamSet<'_>) -> Result<(), InitError>;
    fn on_cycle(&mut self, st: &mut Self::Cycle, task: u16, cx: &mut CycleCx<'_>) -> TaskResult;
    fn poll_task(&mut self, tasks: &mut Self::Tasks, task: u16, cx: &mut TaskCx<'_>,
                 waker: &mut core::task::Context<'_>) -> core::task::Poll<TaskResult>;
    fn on_action(&mut self, instance: u32, action: u16, args: &[u8], out: &mut [u8],
                 cx: &mut TaskCx<'_>) -> core::task::Poll<Result<usize, ActionFail>>;
    fn export_state(&self, out: &mut [u8]) -> usize;
    fn migrate(&mut self, previous: &[u8], from_version: &str) -> Result<(), InitError>;
}
pub trait AppSet { /* gerado: despacho por índice de app, sem dyn */ }
```

- `CycleCx` dá à app a imagem do latch do grupo e o buffer de propostas, com os mesmos registos da imagem empacotada da ref/04 §3 (escrita: 14 B; método: 10 B + argumentos). O `io.pump.set(v)` do SDK escreve lá diretamente: é o mesmo caminho do Linux sem a fronteira Wasm.
- As tarefas async (`every`, `on`, `on_event`, `startup`) são futuros guardados em `Tasks` (tamanho conhecido no build) e avançados por `jk_service`.
- **API indisponível no MCU** (`history`, `infer`, `frames`, `subscribe` de listas arbitrárias, rede, `kv` acima de 16 KiB): com `backend-native`, estas funções exigem um *trait* que o alvo não implementa, anotado com `#[diagnostic::on_unimplemented(message = "J0720: … não existe no Joker Micro", note = "<sugestão>")]`. O erro sai no `rustc`, e o `jk build` traduz a mensagem para o `J0720` com a sugestão (19 §5, U6).
- **`J0721`** (f64 sem FPU de dupla precisão) e **`J0780`** (vírgula flutuante sem FPU): além das portas e dos campos `f64` de `joker:meta` (ref/04 §4.14), o `jk build` procura, no objeto de cada app, chamadas às rotinas de vírgula flutuante por software (`__aeabi_d*`, `__adddf3`… para f64; `__aeabi_f*`, `__addsf3`… para f32) e dá o aviso com os nomes das funções da app que as chamam.
- Uma app nativa não tem *sandbox*, *fuel* nem *epoch*: um ciclo infinito é um overrun do grupo, que o `jk_supervise` apanha, e um *panic* é `jk_fatal`. É o custo declarado da v1 (o `wasmi` chega na 1.4).

#### 2.11.6 Embassy

```rust
// Exemplo mínimo: um grupo num executor de interrupção, o serviço e o supervisor no executor da thread.
static STORAGE: StaticCell<joker_project::Storage> = StaticCell::new();
static APPS: StaticCell<joker_project::Apps> = StaticCell::new();
static EXEC_FAST: InterruptExecutor = InterruptExecutor::new();

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    let platform = PLATFORM.init(Board::new(/* timer, flash */));
    let mut parts = joker_project::init(STORAGE.init(Default::default()), APPS.init(Default::default()),
                                        platform, Micros(Instant::now().as_micros())).unwrap();
    interrupt::SWI_IRQ_1.set_priority(Priority::P1);
    let fast = EXEC_FAST.start(interrupt::SWI_IRQ_1);
    fast.spawn(joker_project::tasks::group_task(parts.groups.take(joker_project::group::MAIN).unwrap())).unwrap();
    spawner.spawn(joker_project::tasks::service_task(parts.service /*, link */)).unwrap();
    spawner.spawn(joker_project::tasks::supervisor_task(parts.io, Watchdog::new(p.WATCHDOG))).unwrap();
}
```

| Função de `joker_micro::embassy` | Faz |
|---|---|
| `async fn run_group<P>(r: GroupRunner<'static, P>) -> !` | `Ticker` com o período do grupo, prazos absolutos; cada volta chama `tick(now)`; relê o período depois de uma troca de geração |
| `async fn run_service<P, L: LinkIo>(s: Service<'static, P>, links: L) -> !` | espera por `service_next`, pelo próximo prazo de `service_deadline` ou por dados num link, e chama `link_*` + `poll` |
| `async fn run_supervisor<P, W: Watchdog>(io: Io<'static, P>, wd: W, every: Duration) -> !` | `supervise` periódico; alimenta o watchdog só com `feed_watchdog` |
| `trait Watchdog { fn feed(&mut self); }` | implementado para os watchdogs dos HAL suportados |
| `trait LinkIo` | junta os transportes dos links (`embedded-io-async` para fluxo, `Datagram` para UDP/CAN) |

### 2.12 Modo firmware

#### 2.12.1 O que é

O `joker-micro` compilado como firmware completo para uma placa: HAL, drivers, apps, link, persistência, OTA e o laço do §2.11.6, sem código do integrador. Duas variantes:

| Variante | Comando | Geração |
|---|---|---|
| **firmware com geração embutida** | `jk build --target mcu:<placa>` | `const`, dentro da imagem |
| **firmware genérico** | `jk build --target mcu:<placa> --generic` | lida da área `GEN_A`/`GEN_B`; o blob vem de `jk build --generation --firmware <firmware.jkfw>` |

O firmware genérico é por (placa, conjunto de apps, conjunto de drivers, capacidades): um só para os 3 000 sites da rodada 5 do documento 08. **Traz todos os transportes do Joker Link** que a placa declara em `[link] transports` (ref/03 §2.3.3), para que a mesma imagem sirva uma geração por série, por USB CDC ou por UDP/TCP: é a geração que escolhe o transporte (`[link]`, ref/01 §5.22). Um firmware com geração embutida só traz os transportes que essa geração usa, mais o `default` da placa (modo de manutenção). Em `dev`, o firmware aceita um blob de geração **sem assinatura** (`J0779` é `error-in-prod`); em `prod`, recusa-o no passo 5 do §2.12.3. `jk flash` (probe-rs) grava o firmware, o blob e, com `--identity`, a área de identidade; o OTA faz o mesmo pelo link.

#### 2.12.2 Partições da flash

A placa (perfil de placa MCU, `ref/03`) declara as áreas; os tamanhos típicos são:

| Partição | Tamanho típico (RP2350, 4 MiB) | Conteúdo |
|---|---|---|
| bootloader | 32 KiB | `embassy-boot` (ou MCUboot noutras placas), com a chave pública de firmware |
| estado do bootloader | 8 KiB | marcas de troca A/B do bootloader |
| firmware ativo | 1 MiB | a imagem em execução |
| `FW_STAGING` | 1 MiB | a imagem recebida por OTA |
| `GEN_A`, `GEN_B` | 64 KiB cada | blobs de geração |
| `PERSIST` | 64 KiB | diário persistente (§2.10) |
| `IDENTITY` | 8 KiB | nome do nó, chaves |

Num STM32G4 (512 KiB de flash interna) as mesmas partições encolhem (firmware 192 KiB × 2, gerações 16 KiB × 2, persistência 16 KiB) e o `jk check` verifica que cabem (`J0781`).

#### 2.12.3 Arranque

| # | Passo | Falha → |
|---|---|---|
| 0 | o bootloader verifica a assinatura do firmware ativo (e troca A/B se houver um pendente) | firmware anterior |
| 1 | relógios, temporizador monotónico, watchdog de hardware (timeout da placa) | reset |
| 2 | as saídas ficam no **estado de reset do hardware**, que a placa declara seguro (`reset_safe = true`; senão `J0786`, aceitável com `accept_unsafe_reset`, §2.16); nenhum pino de saída é configurado antes do passo 6 | — |
| 3 | lê `IDENTITY` (nome, chaves do link, bundle de confiança) e `PERSIST` (estado da geração, trips, alarmes, parâmetros) | persistência vazia: arranque de fábrica |
| 4 | escolhe a geração: a da área ativa; se estava `confirming` e o arranque anterior não confirmou, volta à outra área (`rolled_back`) | a outra área; sem nenhuma válida: **modo de manutenção** |
| 5 | `jk_generation_open` (formato, integridade, assinatura, anti-rollback, nó, firmware, capacidades) | a outra área; senão modo de manutenção |
| 6 | constrói a **tabela de pinos** a partir de `[pins]` da placa (§2.11.3, posse dos pinos, sem `steal`); `init` do nó: saídas em `safe` (A8), trips restaurados, apps inicializadas; só então os drivers tiram os seus pinos da tabela e configuram-nos, já com o valor seguro | modo de manutenção (pino pedido duas vezes: `JK_E_GEN_FIRMWARE`) |
| 7 | arranca os grupos (executores de interrupção, prioridade rate-monotonic), o serviço, os links e o supervisor | — |
| 8 | primeiro tick | alvo ≤ 100 ms desde o reset em M1 (a medir; `firmware.jkfw` chave 16) |

**Modo de manutenção:** sem geração válida, não há sinais nem saídas. O firmware mantém os pinos no estado de reset, sobe o link com a configuração por omissão da placa (`ref/03`) e só aceita `HELLO`, `OTA` de geração ou firmware e pedidos de estado; alimenta o watchdog; o `jokerd` vê-o como um dispositivo `Bad(config_error)`.

#### 2.12.4 OTA e confirmação

- **Geração:** o blob chega (link ou `jk_ota_*`) à área inativa, é verificado com `min_seq = confirmed_seq + 1` e, no firmware, trocado no próximo limite de tick com a janela de confirmação da mensagem (`confirm_ms` = chave 11 do `OTA` `activate`, ref/07 §9.4), pelo `jk_generation_swap`. Sem confirmação dentro da janela, volta à anterior a quente. Um reinício durante a janela conta como falha (passo 4).
- **Firmware:** a imagem chega a `FW_STAGING`, é verificada (o manifesto COSE da ref/07 §9.1 com uma chave do papel `firmware`; `board_id`; `security_version` ≥ o confirmado, senão `JK_E_GEN_ROLLBACK`; e o descritor de firmware compatível com a geração ativa, senão `JK_E_GEN_FIRMWARE`), e o firmware marca a troca no bootloader e reinicia num momento sem escritas pendentes. O bootloader volta a verificar a assinatura com as suas próprias chaves (ref/10 §3.6). O novo firmware só se marca como bom depois de N ticks limpos e do `HELLO` (o teste de saúde do documento 01, §16); senão o bootloader reverte no reinício seguinte.
- **Confirmação por `jk confirm`:** quando o OTA faz parte de uma ativação do nó Linux com janela de confirmação, o `confirm` chega ao MCU só depois do `jk confirm` do nó (ref/07 §9.4; ref/09 §5.13); até lá, a geração ou o firmware estão em experiência, e a janela conta no MCU.
- **Anti-rollback:** `confirmed_seq` só sobe; um blob com `gen_seq` menor ou igual ao confirmado é recusado (`JK_E_GEN_ROLLBACK`), exceto o próprio blob ativo depois de um reinício.

#### 2.12.5 Medição do WCET: `jk bench --target mcu:<placa>`

O `jk bench --target mcu:<placa>` grava no MCU um firmware de medição (o genérico da placa, com as apps do manifesto e um executor de bancada), corre cada tarefa de ciclo pelo menos 10 000 vezes com as entradas do gerador de bancada, e escreve o máximo medido em `[wcet]` do lock (ref/02 §2.1.13). O tempo mede-se sempre com o **contador de ciclos** do núcleo (`DWT_CYCCNT` nos Cortex-M, `mcycle` nos RISC-V, `CCOUNT` nos Xtensa), convertido pelo `clock` da placa; nunca pelo relógio do anfitrião. O resultado sai do MCU conforme o `[probe]` da placa (ref/03 §2.3.3):

| `[probe] tool` | Como grava e lê |
|---|---|
| `probe-rs` (omissão) | grava e lê os contadores pela sonda (SWD/JTAG), sem tocar no link |
| `espflash` (placa **sem sonda**, como o ESP32 da bancada) | grava por série (`espflash`) e lê os resultados **pela mesma porta série**, num registo CBOR por tarefa (`{tarefa, n, min, max, p99}` em ciclos) que o executor escreve no fim; para a leitura, a porta abre-se com o DTR e o RTS desativados, como a omissão de `[buses.*]` (ref/01 §5.6), para não reiniciar a placa |

Os dois caminhos dão o mesmo `[wcet]`: o lock não distingue a origem. No fim, o `jk bench` volta a gravar o firmware que lá estava, se o conseguir ler (senão, avisa: o MCU fica com o firmware de medição até ao próximo `jk flash`).

#### 2.12.6 Recetor de OTA obrigatório

**Regra (1.0):** todo o firmware Joker Micro, no modo firmware (com geração embutida ou genérico) e no modo crate, leva o **recetor de OTA** de gerações e de firmware: um MCU no campo tem de poder receber firmware novo sem visita. Concretamente:

| Exigência | Verificação |
|---|---|
| a feature `ota` ligada (§2.11.2) | `compile_error!` no `joker-micro` e `J0795` no `jk build` |
| a placa declara `FW_STAGING`, `GEN_A` e `GEN_B` com tamanho para a imagem e para o maior blob, e um bootloader com troca A/B (§2.12.2) | `J0795` (partição em falta), `J0781` (não cabe) |
| o modo de manutenção (§2.12.3) aceita `OTA` pelo `[link] default` da placa | faz parte do firmware; não é configurável |
| no modo crate, o `joker-project` gerado liga o serviço de OTA ao laço do §2.11.6; o integrador não o pode retirar sem desligar a feature | `J0795` |

No **modo biblioteca** (ABI C), o firmware é do integrador: os `jk_ota_*` estão sempre na ABI, mas levá-los até ao bootloader é trabalho dele (documento 19, §4.2). Uma placa sem flash para duas imagens (A/B) não é alvo do modo firmware na 1.0.

### 2.13 O blob de geração (`*.jkgen`)

Tipo de media OCI `joker.generation.v1` (documento 20, §3.2). É a mesma estrutura em memória que o núcleo usa, serializada: o MCU usa-a **no sítio** (XIP), sem a copiar nem a interpretar para outra forma. A geração embutida é exatamente o mesmo formato, com a flag `EMBEDDED`.

#### 2.13.1 Regras gerais

| Regra | Valor |
|---|---|
| endianness | **little-endian** em todos os inteiros e `f64` (IEEE 754). Todos os alvos da v1 são little-endian; um alvo big-endian é `J0770` |
| alinhamento | o blob começa num endereço múltiplo de 8; cada secção começa num offset múltiplo de 8; os registos de cada secção têm tamanho múltiplo de 8 (salvo `STRINGS`, `CODE` e `BLOBS`, que são bytes); o enchimento é 0 |
| offsets | relativos ao início do blob, `u32` |
| `strref` | 8 B: `u32` offset em `STRINGS`, `u16` comprimento, `u16` 0. UTF-8 sem NUL |
| `poolref` | 8 B: `u32` índice em `U32POOL` (em elementos), `u32` número de elementos |
| índices | `u32`, `0xFFFFFFFF` = nenhum |
| durações | `u32` µs, salvo os campos `_ms` (`u32` ms) e `stuck_detect_us` (`u64`) (`ref/00` §3) |
| valores | `u64` com os bits do tipo (`ref/00` §4.1) |
| path-hash | os primeiros 8 B do SHA-256 do caminho canónico em UTF-8, lidos como `u64` LE |

#### 2.13.2 Cabeçalho (128 B)

| Offset | Tipo | Campo | Regra |
|---|---|---|---|
| 0 | `[4]u8` | magia `"JKGN"` | |
| 4 | `u16` | `format` | 1 na v1 |
| 6 | `u16` | `header_size` | 128 |
| 8 | `u32` | `image_len` | bytes de `[0, image_len)`, múltiplo de 8; é o que o SHA-256 cobre |
| 12 | `u32` | `flags` | bit 0 `SIGNED` (há *trailer*), bit 1 `DEV` (`mode = "dev"`; um nó em `prod` nunca envia um blob com este bit, `J0794`, §2.16), bit 2 `EMBEDDED`, bit 3 `NODE_BOUND`; outros 0 |
| 16 | `u64` | `gen_seq` | inteiro crescente por nó (`ref/00` §5) |
| 24 | `[32]u8` | `source_hash` | **o hash da geração** do MCU: o SHA-256 do descritor de geração da ref/10 §2.6.2, calculado sobre o manifesto efetivo do MCU, o lock e os contratos. É o que o `jk`, o `jokerd` e o Deck mostram, o que o `HELLO` anuncia (chave 6.2) e o `hash` de `mcu-generation` no estado desejado |
| 56 | `[32]u8` | `registry_hash` | o do documento 16, §7 (lista ordenada de `SignalId`, tipos, grupos) |
| 88 | `[8]u8` | `board_id` | primeiros 8 B do SHA-256 de `"<placa>@<major>"` |
| 96 | `u16` | `expr_isa` | 1 na v1 (§2.13.4) |
| 98 | `u16` | `section_count` | |
| 100 | `u32` | `section_table_off` | 128 |
| 104 | `u32` | `trailer_off` | `image_len` se `SIGNED`, senão 0 |
| 108 | `u32` | `trailer_len` | |
| 112 | `u32` | `min_runtime` | `major << 16 \| minor` do `joker-micro` mínimo |
| 116 | `u32` | `image_crc32c` | CRC-32C da imagem com este campo a 0 (deteção rápida de flash corrompida, antes do SHA-256) |
| 120 | `[8]u8` | reservado | 0 |

**Tabela de secções:** `section_count` entradas de 16 B, ordenadas por `kind`: `u16 kind`, `u16 flags` (bit 0 `REQUIRED`), `u32 offset`, `u32 length` (bytes), `u32 count` (registos; 0 para secções de bytes). Um leitor que encontre um `kind` desconhecido com `REQUIRED` recusa (`JK_E_GEN_VERSION`); sem `REQUIRED`, salta-o (regra 2 do `ref/00` §1). Todas as secções da v1 são `REQUIRED`.

#### 2.13.3 Secções

O índice de um registo numa secção **é** o seu identificador na geração: o registo *n* de `SIGNALS` é o `SignalId` *n*, e assim por diante. As secções vazias omitem-se.

| `kind` | Secção | Registo | Conteúdo |
|---|---|---|---|
| 0x0001 | `STRINGS` | bytes | todas as strings, sem repetição, pela ordem da primeira referência |
| 0x0002 | `CAPS` | 8 B | necessidades da geração: `u16 cap_id`, `u16 0`, `u32 valor`, por `cap_id` crescente (tabela seguinte) |
| 0x0003 | `REQUIRES` | 32 B | `u8 kind` (1 app, 2 driver), `u8 0`, `u16 major`, `u16 minor`, `u16 patch`, `strref nome`, `[8]u8 hash` (apps: o `meta_hash`, os primeiros 8 B do SHA-256 do CBOR `joker:meta`; drivers: os primeiros 8 B do digest do descritor do driver com que o `jk build` planeou, ref/06 §2.3, §3.7; `extern`: 0), `[8]u8 0` |
| 0x0004 | `SIGNALS` | 40 B | `u64 path_hash`, `strref caminho` (canónico), `u8 tipo` (`ref/00` §6), `u8 direção` (0 entrada, 1 saída, 2 derivado, 3 parâmetro de classe), `u8 grandeza` (0 nenhuma, 1 `active_power` … 7 `temperature_difference`, pela ordem do 16 §5), `u8 flags` (bit 0 tem `AUX`, bit 1 exportado no `HELLO`, bit 2 enviado em `SAMPLES`, bit 3 texto), `u32 max_age_us` (`0xFFFFFFFF` = sem), `u32 aux` (índice em `SIGNAL_AUX`), `strref unidade`, `u32 0`. Ordenados por caminho canónico (documento 01, §3.1) |
| 0x0005 | `PATHS` | 24 B | `u64 path_hash`, `u32 SignalId`, `strref caminho` (só `u32` offset + `u16` comprimento), `u16 flags` (bit 0 canónico, bit 1 membro de instância, bit 2 alias), `u32 0`; **todos** os caminhos (canónicos e aliases, P1), ordenados por `path_hash` para busca binária |
| 0x0006 | `SIGNAL_AUX` | 48 B | `f64 limit_lo`, `f64 limit_hi` (NaN = sem), `f64 deadband_aquisição` (0 = sem), `u64 stuck_detect_us` (0 = sem), `u32 debounce_us`, `u32 enum` (índice em `ENUMS`), `u32 0`, `u32 0` |
| 0x0007 | `ENUMS` | 16 B | `strref nome`, `u32 primeira variante`, `u16 n`, `u8 kind` (0 enum, 1 bits), `u8 0` |
| 0x0008 | `VARIANTS` | 16 B | `strref nome`, `u64 valor` (enum) ou índice do bit (bits) |
| 0x0009 | `GROUPS` | 48 B | `strref nome`, `u32 period_us`, `u8 overrun` (0 `skip`, 1 `catch_up`, 2 `degrade`, 3 `trip`), `u8 flags`, `u16 trip_after`, `u32 primeira tarefa`, `u16 n_tarefas`, `u16 degrade_ticks`, `poolref latch` (SignalIds latchados, ordenados), `poolref saídas` (saídas de que é dono), `u32 trace_bytes`, `u32 0` |
| 0x000A | `TASKS` | 8 B | `u16 app`, `u16 tarefa` (índice nos metadados da app), `u16 flags` (bit 0 `optional`), `u16 0`; pela ordem do tick (`order`) |
| 0x000B | `WRITERS` | 24 B | `strref identidade` (`app:<nome>`, `extern:<app>`, `bridge:<link>:<nível>`, `impl:<instância>`), `u8 writer_kind` (`ref/00` §6), `u8 nível` (1–16), `u16 flags`, `u32 lease_us` (0 = A9), `u32 dono` (app, link ou `NONE`), `u8 papel`, `[3]u8 0` |
| 0x000C | `OUTPUTS` | 152 B | ver a tabela abaixo |
| 0x000D | `OUTPUT_WRITERS` | 8 B | `u32 escritor`, `u8 nível`, `u8 flags`, `u16 0`: os níveis do árbitro de cada saída (A5; a unicidade já foi verificada pelo `J0210`) |
| 0x000E | `ATOMIC` | 32 B | `strref nome`, `poolref membros` (pela `order`), `u8 on_rate` (0 `independent`, 1 `proportional`), `u8 tem_commit`, `u16 0`, `u32 sinal de commit`, `u64 valor de commit` |
| 0x000F | `U32POOL` | 4 B | listas de `u32` referidas por `poolref` |
| 0x0010 | `PROGRAMS` | 32 B | `u32 code_off`, `u32 code_len`, `u32 state_off`, `u16 state_len`, `u8 max_stack` (≤ 32), `u8 tipo do resultado`, `u8 kind` (1 `calc`, 2 alarme `when`, 3 alarme `suppress_when`, 4 `interlock`, 5 limite de `range`, 6 `reset_when`, 7 consulta, 8 atribuição de método, 9 permissivo de ação, 10 argumento de uma chamada a um método de membro), `u8 flags`, `u16 n_args`, `u32 grupo de avaliação`, `u32 alvo` (sinal, alarme, saída ou membro; no `kind` 10, o membro de classe), `u32 0` (no `kind` 10, o índice em `METHODS` do método chamado) |
| 0x0011 | `CODE` | bytes | bytecode (§2.13.4) |
| 0x0012 | `CONSTS` | 16 B | `u8 tipo`, `[7]u8 0`, `u64 bits` |
| 0x0013 | `ALARMS` | 56 B | `strref nome`, `u32 when`, `u32 suppress_when` (programas), `u32 delay_on_us`, `u32 delay_off_us`, `u8 severidade`, `u8 flags` (bit 0 da app), `u16 0`, `u32 sinal state`, `u32 sinal active`, `strref mensagem` (modelo com `{caminho}`), `poolref trips` (saídas de `actions.trip`), `u32 0` |
| 0x0014 | `CLASSES` | 56 B | `strref "pacote/nome"`, `u16 major`, `u16 minor`, `u16 patch`, `u16 n_capacidades`, `[32]u8 hash do contrato` (17 §6), `u32 primeiro membro`, `u16 n_membros`, `u16 0` |
| 0x0015 | `MEMBERS` | 24 B | `strref nome`, `u8 kind` (1 `in`, 2 `out`, 3 `param`, 4 `event`, 5 `action`, 6 `method`, 7 `query`, 8 membro de classe), `u8 tipo`, `u8 capacidade` (`0xFF` = nenhuma), `u8 flags` (bit 0 `@optional`, bit 1 obrigatório para `health`, bit 2 `atomic`, bit 3 `@long`), `u32 aux` (método: índice em `METHODS`; ação: `@timeout` em ms; evento: offset do esquema em `BLOBS`; membro de classe: índice em `CLASSES`), `u32 aux2` (membro de classe: `u16 min`, `u16 max` da coleção, ou `1`, `1` num membro fixo), `u32 0` |
| 0x0016 | `METHODS` | 16 B | `u32 classe`, `u16 membro`, `u8 n_atribuições` (atribuições e argumentos de chamadas), `u8 flags` (bit 0 `atomic`), `u32 primeiro programa` (consecutivos: `kind` 8 por atribuição, `kind` 10 por argumento de uma chamada a um método de membro, 17 §4.1; uma chamada sem argumentos ocupa um `kind` 10 com `code_len` 0), `u32 order` (índice em `U32POOL` de `n_atribuições` membros) |
| 0x0017 | `INSTANCES` | 32 B | `strref nome`, `u32 classe`, `u32 mapa` (índice em `U32POOL` de `n_membros` SignalIds; `NONE` para membros sem sinal; num membro de classe, o índice em `U32POOL` da lista de instâncias ligadas, precedida do seu comprimento), `u32 capacidades` (bits), `u8 origem` (0 implícita, 1 composta, 2 virtual), `u8 flags` (bit 0: só membro, sem nome de topo no link, ref/07 §6.3), `[2]u8 0`, `u32 implementada por` (app ou `NONE`), `u32 0` |
| 0x0018 | `ACTIONS` | 32 B | `u32 instância`, `u16 membro`, `u8 implementador` (1 driver, 2 `extern`, 3 app, 4 link), `u8 papel mínimo`, `u32 permit` (programa), `u32 timeout_ms`, `u32 dedup_window_ms`, `u32 índice do implementador`, `u32 fila` (dispositivo que serializa), `u32 0` |
| 0x0019 | `EVENTS` | 24 B | `u32 instância` (ou `NONE` para evento de app), `u16 membro`, `u16 flags` (bit 0 para o link, bit 1 para a fila do firmware), `u32 origem` (porta de app), `u32 esquema` (offset em `BLOBS`), `u32 comprimento do esquema`, `u32 profundidade da fila` |
| 0x001A | `APPS` | 48 B | `strref nome`, `u16 major`, `u16 minor`, `u16 patch`, `u16 flags` (bit 0 `extern`), `u32 escritor`, `u32 primeira porta`, `u16 n_portas`, `u16 n_parâmetros`, `u32 primeiro parâmetro`, `u32 kv_bytes`, `[8]u8 meta_hash`, `u32 0` |
| 0x001B | `APP_PORTS` | 24 B | `u32 alvo` (sinal, instância, alarme ou `NONE` se opcional sem ligação), `u8 kind` (1 entrada, 2 saída, 3 interna, 4 publicada, 5 evento de entrada, 6 evento de saída, 7 equipamento, 8 alarme, 9 elemento de `Many`), `u8 conversão` (0 nenhuma, 1 afim), `u16 índice em Many`, `f64 fator`, `f64 offset` — a conversão de unidade da ligação (documento 02, §3) já resolvida |
| 0x001C | `PARAMS` | 40 B | `strref nome`, `u32 app`, `u8 tipo`, `u8 flags` (bit 0 alterável em runtime, bit 1 persistível), `u16 0`, `u64 valor`, `f64 mínimo`, `f64 máximo` (NaN = sem) |
| 0x001D | `DEVICES` | 32 B | `strref nome`, `strref driver`, `u16 major do driver`, `u16 flags`, `u32 config` (offset em `BLOBS`), `u32 comprimento`, `u32 descritor` (offset em `BLOBS` dos 32 B do SHA-256 do descritor do driver com que o `jk build` planeou, ref/06 §2.3; `NONE` para `extern`; os primeiros 8 B repetem-se no registo `REQUIRES` do driver) — a configuração é o `micro-device-config` do `spec/cddl/jdp.cddl` (ref/06 §3.7): o digest do descritor (chave 0, o mesmo deste campo), o `device-spec` com o esquema `config` do descritor, as `point-spec` com os `SignalId`s do MCU, o plano de leitura e o barramento. O `HELLO` resume os descritores na chave 15 (ref/07 §6.4) |
| 0x001E | `BLOBS` | bytes | CBOR determinístico: configurações de drivers e de links, esquemas de eventos, cada item alinhado a 8 |
| 0x001F | `LINKS` | 32 B | `strref nome`, `u8 transporte` (1 `uart`, 2 `rs485`, 3 `can`, 4 `usb_cdc`, 5 `udp`, 6 `tcp`), `u8 enquadramento` (1 fluxo, 2 datagrama), `u8 autenticação` (0 nenhuma, 1 opcional, 2 obrigatória), `u8 par` (0 nó Linux, 1 Deck), `u16 níveis permitidos` (bit *n*−1 = nível *n*), `u16 mtu`, `u32 primeiro escritor` (um por nível permitido, consecutivos), `u32 config` (offset em `BLOBS`), `u32 comprimento`, `u32 0` |
| 0x0020 | `NODE` | 48 B | `strref nome do nó` (se `NODE_BOUND`), `u32 service_deadline_ms`, `u8 fonte do relógio` (0 nenhuma, 1 `link`, 2 `extern`), `u8 flags` (bit 0 `freeze_on_trip`), `u16 0`, `u32 max_holdover_ms` (`0xFFFFFFFF` = sem limite), `u32 trace_bytes` total, `u32 anel de SAMPLES` (entradas), `u32 fila de eventos do firmware`, `u32 confirmação por omissão (ms)`, `[12]u8 0` |
| 0x0021 | `CATALOG` | bytes | o catálogo do Joker Link (ref/07 §6.3), **tal e qual**: o que o `CATALOG` transfere; o seu SHA-256 é o hash do catálogo do `HELLO`. Presente se a geração tiver links |
| 0x0022 | `SECRETS` | bytes | um `secret-bundle` (ref/02 §2.4, `spec/cddl/secrets.cddl`) com as entradas cifradas para a chave X25519 do MCU (registo 19), só com o `wrap` do MCU; presente em MCUs autónomos com segredos (ref/04 §4.14) e em qualquer MCU M2 cujo `[net.wifi]` tenha `password` (ref/01 §5.22) |
| 0x0023 | `TRUST` | bytes | cadeia de bundles de confiança novos (`trust-chain` no CDDL), por ordem de versão; o MCU instala-os (registo 18) antes de verificar a assinatura do blob (§2.13.5) |
| 0x0024 | `NET` | bytes | `micro-net` em CBOR determinístico (`spec/cddl/joker-micro.cddl`): a configuração do Wi-Fi do `[net.wifi]` (ref/01 §5.22): `ssid`, `country`, `hostname` e o **nome** do segredo da palavra-passe, que se procura em `SECRETS`; nunca o valor. Só em MCUs M2 com `[net.wifi]` |
| 0x0040–0x0043 | reservados | — | `MODELS`, `BLOCKS`, `STREAMS` (v1.1, "Preparado para ML"), `SCHEDULES` (escritas agendadas, M6, v1.1) |
| 0x0044 | reservado | — | `WASM_APPS` (apps em `wasmi`, v1.4) |
| 0x8000–0xFFFF | privados | — | nunca `REQUIRED`; ignorados pelo `joker-micro` |

**`OUTPUTS` (152 B):**

| Offset | Tipo | Campo |
|---|---|---|
| 0 | `u32` | sinal |
| 4 | `u32` | grupo dono (A11) |
| 8 | `u8` × 4 | `on_range` (0 `clamp`, 1 `reject`, 2 `trip`), `on_interlock` (0 `safe`, 1 `hold`), `safe_kind` (0 valor, 1 `relinquish`), `safer` (0 `equal`, 1 `lower`, 2 `higher`) |
| 12 | `u32` | flags: bit 0 `permit_uncertain`, bit 1 tem `rate`, bit 2 tem `safe_ramp`, bit 3 tem `deadband`, bit 4 tem `verify`, bit 5 `pulse`, bit 6 trip persistente (omissão 1) |
| 16 | `u64` | valor `safe` |
| 24 | 16 B | limite inferior: `u8 kind` (0 nenhum, 1 constante, 2 programa), `[3]u8 0`, `u32 programa`, `f64 constante` (E6) |
| 40 | 16 B | limite superior, idem |
| 56 | `f64` | `rate` (unidade/s) |
| 64 | `f64` | `safe_ramp` |
| 72 | `f64` | `deadband` |
| 80 | `u32` | `min_interval_us` |
| 84 | `u32` | `grace_us` (E4) |
| 88 | `u32` | `interlock` (programa) |
| 92 | `u32` | `reset_when` (programa) |
| 96 | `poolref` | `requires_good` |
| 104 | `u32` | `max_lease_ms` (A9; omissão 15 min) |
| 108 | `u32` | `max_manual_ms` (A10; omissão 8 h) |
| 112 | `u16`, `u16`, `u32` | `max_actuations`: contagem, 0, janela em s |
| 120 | `u32` | `verify.readback` (sinal) |
| 124 | `u32` | `verify.within_us` |
| 128 | `f64` | `verify.tolerance` |
| 136 | `u32` | `pulse_us` |
| 140 | `u32` | grupo atómico (índice em `ATOMIC`) |
| 144 | `u32` | primeiro em `OUTPUT_WRITERS` |
| 148 | `u16`, `u16` | número de escritores; níveis manuais permitidos (bits 3–5) |

**Capacidades (`cap_id`).** A mesma numeração em `CAPS` (o que a geração precisa), no descritor de firmware (o máximo do build, §2.14) e nas chaves de `[target.capacity]` (§2.17), com os nomes em `snake_case`:

| id | nome | id | nome | id | nome |
|---|---|---|---|---|---|
| 1 | `signals` | 15 | `members` | 29 | `u32pool` |
| 2 | `paths` | 16 | `instances` | 30 | `image_entries` |
| 3 | `outputs` | 17 | `methods` | 31 | `text_signals` |
| 4 | `groups` | 18 | `actions` | 32 | `inbox` (por grupo) |
| 5 | `tasks` | 19 | `events` | 33 | `method_queue` (por grupo) |
| 6 | `writers` | 20 | `apps` | 34 | `action_pending` |
| 7 | `output_writers` | 21 | `app_ports` | 35 | `event_queue` |
| 8 | `atomic` | 22 | `params` | 36 | `trace_bytes` |
| 9 | `programs` | 23 | `devices` | 37 | `kv_bytes` |
| 10 | `code_bytes` | 24 | `links` | 38 | `window_samples` |
| 11 | `consts` | 25 | `strings_bytes` | 39 | `manual_writes` |
| 12 | `expr_state_bytes` | 26 | `blobs_bytes` | 40 | `app_state_bytes` |
| 13 | `alarms` | 27 | `enums` | | |
| 14 | `classes` | 28 | `variants` | 41 | `radio_heap_bytes` (só ESP32 com Wi-Fi, §2.15.2) |

**`J0710` e `JK_E_GEN_CAPACITY`.** Uma geração cabe num build se, para cada `cap_id`, a necessidade (`CAPS`) for ≤ ao máximo do descritor. O `jk check` e o `jk build --generation` fazem esta comparação antes de gravar (`J0710`, com a lista das capacidades excedidas e a estimativa de RAM e flash por componente do §2.9.1); o `joker-micro` repete-a no `jk_generation_open` (`JK_E_GEN_CAPACITY`) e nunca confia no `jk`. Com geração embutida, as capacidades do build são as da geração mais a folga de `[target.capacity]`.

#### 2.13.4 Bytecode das expressões (ISA 1)

O mesmo bytecode é avaliado pelo `joker-expr` no Linux (em memória) e no MCU (no blob). A semântica é a do documento 02, §10 e do documento 16, §3–4; a ISA só fixa a codificação.

**Máquina:** pilha de células de 16 B (`u64` bits, `u8` tipo, `u8` código de qualidade, `u8` motivo, `u8` flags, `u32` 0), profundidade ≤ `max_stack` ≤ 32. Cada programa tem o seu estado (`state_off`, `state_len`) no nó: janelas, temporizadores de `held`, bits de `rose`/`fell`/`hyst`. O contexto de avaliação dá o snapshot do grupo, o instante do latch e, nas atribuições de métodos, os argumentos e o chamador.

**Codificação:** um `u8` de *opcode*, seguido dos operandos em little-endian, sem alinhamento. Os saltos são relativos ao fim da instrução e **só para a frente**: não há ciclos (as expressões são totais), e o tempo de um programa é limitado pelo seu tamanho.

| Op | Mnemónica | Operandos | Pilha | Semântica |
|---|---|---|---|---|
| 0x01 | `CONST` | `u16 k` | → v | `CONSTS[k]`, `Good` |
| 0x02 | `TRUE` / 0x03 `FALSE` | — | → b | |
| 0x04 | `LOAD` | `u32 sinal` | → s | amostra do snapshot, com envelhecimento; um `out` dá o valor **aplicado** (17 §11) |
| 0x05 | `OWN` | `u32 sinal` | → v | `own(x)` (só em atribuições de métodos; 16 §4) |
| 0x06 | `ARG` | `u8 i` | → v | argumento *i* do método |
| 0x08 | `AGE` | `u32 sinal` | → f64 s | `age(x)` |
| 0x10 | `AFFINE` | `u16 ka`, `u16 kb` | x → x·a+b | conversão de unidade resolvida no `jk check` |
| 0x11 | `TOF` | — | i → f64 | inteiro para f64 |
| 0x20–0x24 | `ADD`, `SUB`, `MUL`, `DIV`, `NEG` | — | a b → r | em f64; divisão por zero, raiz de negativo, `NaN` → `Bad(nan)` (16 §4); qualidade: a pior (16 §3.2) |
| 0x28–0x2D | `EQ`, `NE`, `LT`, `LE`, `GT`, `GE` | — | a b → bool | inteiros com o mesmo sinal comparam-se exatamente; o resto em f64 |
| 0x30 | `NOT` | — | b → b | |
| 0x31 | `JFG` | `i16` | b → b | se o topo é `false` com `Good`, salta (mantém-no): o curto-circuito de `&&` (02 §4) |
| 0x32 | `JTG` | `i16` | b → b | se o topo é `true` com `Good`, salta: o de `\|\|` |
| 0x33 | `AND` / 0x34 `OR` | — | a b → b | lógico, com o curto-circuito simétrico do 16 §3.2: um `false (Good)` em qualquer dos lados decide o `AND`, um `true (Good)` decide o `OR`; senão, a pior qualidade (o `JFG`/`JTG` só poupa a avaliação do lado direito) |
| 0x38 | `JMP` | `i16` | — | só para a frente |
| 0x39 | `IF` | `i16 else`, `i16 fim` | c → | `c` `Bad` → empurra `Bad` do tipo do resultado e salta para o fim; senão guarda a qualidade de `c` e segue (`true`) ou salta para `else` (16 §3.2) |
| 0x3A | `ENDIF` | — | r → r | se `c` era `Uncertain`, o resultado fica, no máximo, `Uncertain` |
| 0x40–0x45 | `ABS`, `MIN`, `MAX`, `CLAMP`, `SQRT`, `POW` | — | … → r | |
| 0x48 | `GOOD` | — | s → bool | `good(x)`, sempre `Good` |
| 0x49 | `QUALITY` | — | s → u64 | `quality(x)` (código), sempre `Good` |
| 0x50–0x53 | `WAVG`, `WMIN`, `WMAX`, `WRATE` | `u16 estado`, `u32 janela_us` | x → r | janelas (≤ 15 min, `J0305`); qualidade pela regra dos 10% (16 §3.2) |
| 0x58 | `HELD` | `u16 estado`, `u32 dur_us` | b → b | 16 §3.2 |
| 0x59 | `ROSE` / 0x5A `FELL` | `u16 estado` | b → b | |
| 0x5B | `HYST` | `u16 estado` | x lo hi → b | |
| 0x5C | `BIT` | — | x n → b | |
| 0x60 | `AGG` | `u8 f`, `u32 lista`, `u16 n` | → r | `any` 0, `all` 1, `count` 2, `sum` 3, `maxs` 4, `mins` 5 sobre `n` sinais de `U32POOL` |
| 0x70 | `RET` | — | r → | resultado do programa |

**Verificador** (no `jk_generation_open`; falha → `JK_E_GEN_FORMAT`): *opcodes* conhecidos na ISA do blob; saltos dentro do programa e para a frente; profundidade da pilha ≤ `max_stack` em todos os caminhos; sinais, constantes, listas e estado dentro das secções e do `state_len`; `OWN` e `ARG` só em atribuições de métodos; o programa termina em `RET` com o tipo declarado.

Uma ISA nova (2, …) só acrescenta *opcodes*; um firmware que só conheça a 1 recusa um blob com `expr_isa = 2` (`JK_E_GEN_VERSION`), e o `jk build --generation` não o produz para esse firmware (`J0789`).

#### 2.13.5 Assinatura

Um blob assinado tem, a seguir à imagem (`trailer_off = image_len`), um **COSE_Sign1** (RFC 9052, `ref/00` §7) cuja carga útil é o **manifesto de OTA do Joker Link** (`ota-manifest`, ref/07 §9.1; `generation-trailer` no CDDL), que identifica a imagem pelo SHA-256. É **a única forma** de assinatura de uma geração de MCU: o OTA pelo link envia este manifesto no `begin`, e o MCU grava-o como *trailer*; o `jk_ota_*` recebe o ficheiro `.jkgen` inteiro (imagem e *trailer*); o arranque volta a verificá-lo. A imagem fica no sítio, alinhada e mapeável, e a verificação Ed25519 corre sobre poucas dezenas de bytes.

| Parte | Conteúdo |
|---|---|
| cabeçalho protegido | `{1: -8 (EdDSA), 4: kid}`; `kid` = os 16 primeiros bytes do SHA-256 da chave pública (ref/02 §2.5) |
| não protegido | `{}` |
| carga útil (`ota-manifest`) | `1` = 1 (`generation`), `2` alvo, `3` o `gen_seq` em decimal, `4` = `image_len`, `5` SHA-256 de `[0, image_len)`, `6` = 0, `7` = `gen_seq`, `8` = `source_hash`, `9` hash das capacidades do firmware alvo (§2.14), `12` = `board_id`, `13` = `format`, `? 14` nome do nó, `? 15` = `registry_hash` |
| assinatura | Ed25519 sobre o `Sig_structure` `["Signature1", protegido, h'', carga]` |

A verificação exige que `4`, `5`, `7`, `8`, `12`, `13` (e `15`, se existir) coincidam com a imagem e o cabeçalho, que `9` seja o hash das capacidades deste firmware (senão `JK_E_GEN_CAPACITY`) e, se houver `14`, que seja o nome do nó na área `IDENTITY` (`JK_E_GEN_NODE`).

**Quem assina** (D7, ref/00 §7.1): a geração, uma chave com o papel `config`; o firmware, uma chave com o papel `firmware` (o mesmo manifesto, com `1` = 2 e o `security_version` do firmware em `6`). As chaves e os papéis vêm do **bundle de confiança** do MCU (registo 18 da área `IDENTITY`, ≤ 2 KiB, o perfil micro do `trust-bundle` da ref/10 §2.4.3); sem bundle, as âncoras de `platform->trust_keys` valem para todos os papéis (ref/02 §2.5.4). Um bundle novo chega na secção `TRUST` de um blob (MCUs atrás de um nó) ou pelo agente (MCUs autónomos, ref/10 §2.15), e só é aceite se estiver assinado por uma âncora ou por uma chave `root` do bundle instalado e tiver `version` maior; o MCU instala a cadeia da secção `TRUST` **antes** de verificar a assinatura do blob que a traz.

#### 2.13.6 Determinismo do `jk build --generation`

O mesmo conjunto (manifesto efetivo, `joker.lock`, contratos, descritor de firmware, chave) dá **sempre os mesmos bytes**:

- registos pela ordem canónica: sinais pelo caminho; grupos, escritores, apps, classes, instâncias, alarmes, dispositivos e links pelo nome; membros pela ordem de declaração do contrato canónico (17 §6); programas pela ordem das secções que os referem;
- strings deduplicadas pela ordem da primeira referência, na ordem das secções;
- enchimento a zeros, CBOR determinístico, sem datas, caminhos de ficheiros ou nomes de máquina;
- o formato e a ISA são os **mais altos que o firmware suporta** (descritor, chaves 5 e 6) e que a geração precisa; nunca mais altos;
- Ed25519 é determinístico (RFC 8032).

`jk build --generation --check-reproducible` constrói duas vezes e compara. A conformidade tem blobs de referência: o mesmo projeto tem de dar os mesmos bytes em qualquer máquina.

### 2.14 Descritor de firmware (`firmware.jkfw`)

Cada build de MCU (firmware, biblioteca ou crate) produz um descritor em CBOR determinístico (`firmware-descriptor` no CDDL), que também fica dentro da imagem (secção de ligação `.joker.fw`) e é anunciado no `HELLO`. É o contrato entre um firmware genérico e os blobs que aceita.

| Chave | Conteúdo |
|---|---|
| 1 | versão do `joker-micro` (SemVer) |
| 2 | versão da ABI, empacotada |
| 3, 4 | placa e `board_id` |
| 5, 6 | formato de blob e ISA mais altos que entende |
| 7 | capacidades máximas (`cap_id` → valor) |
| 8 | apps ligadas: `[nome, versão, meta_hash]` |
| 9 | drivers: `[nome, major, digest do descritor]` (o digest falta só no `extern`) |
| 10 | transportes de link |
| 11 | features |
| 12 | hash da interface (biblioteca e crate; zeros no modo firmware) |
| 13 | pilha por contexto |
| 14 | áreas de flash (modo firmware) |
| 15 | FPU, o mesmo enum do campo `fpu` das placas (ref/03 §2.3): `"none"` (sem FPU: `f32` e `f64` por software, `J0780` e `J0721`), `"sp"` (precisão simples em hardware; `f64` por software, `J0721`), `"sp+dcp"` (precisão simples mais um coprocessador de dupla precisão, como o DCP do RP2350: o `f64` é acelerado e não dá `J0721`), `"dp"` (simples e dupla em hardware) |
| 16 | tempo medido entre o reset e o primeiro tick, ms |
| 17 | `security_version` do firmware (o contador de segurança, anti-rollback; ref/07 §9.5) |
| 18 | versão do **núcleo** compilado (`"<maj>.<min>"` do `joker-core`); é o que o lock fixa em `[mcu.<d>] core` e o que o `jk check` usa para o `J0796` (§3.11). Sem ela (firmware anterior à regra), conta como a major.minor da chave 1 |

- O **hash das capacidades** é o SHA-256 do CBOR determinístico da chave 7. Vai no `HELLO` (chave 5.5) e no manifesto de uma geração (chave 9): um blob feito para outro conjunto de capacidades é recusado antes de gastar flash.
- O **digest do descritor** é o SHA-256 do descritor inteiro; vai no `HELLO` (chave 5.9) e no relatório ao Deck (ref/10 §2.10.1, `mcus`).
- Os drivers (chave 9) levam o digest do descritor de cada driver (ref/06 §2.3), para o `jk build --generation` recusar um plano feito com outro descritor (`J0773`).

O `jk` mostra-o em JSON com os mesmos campos (`jk inspect firmware.jkfw`).

### 2.15 Alvos e orçamentos

#### 2.15.1 Classes de alvo (19 §3.1)

| Alvo | Classe | Núcleo | FPU | RAM | Flash | Capacidades por omissão (sinais / saídas / grupos / instâncias / apps) | HAL (`[board] chip` de exemplo), feature |
|---|---|---|---|---|---|---|---|
| RP2350 | **M1** (referência) | 2× Cortex-M33, 150 MHz | `sp+dcp` | 520 KiB | 4 MiB (externa, XIP) | 256 / 64 / 8 / 32 / 4 | `embassy-rp` (`rp235xa`), `chip-rp` |
| STM32G4 (G474) | M1 | Cortex-M4F, 170 MHz | `sp` | 128 KiB | 512 KiB | 128 / 32 / 4 / 16 / 2 | `embassy-stm32` (`stm32g474re`), `chip-stm32` |
| ESP32-C6 | **M2** | RISC-V, 160 MHz | **`none`** | 512 KiB | 4 MiB (externa) | 256 / 64 / 8 / 32 / 4 | `esp-hal` (`esp32c6`), `chip-esp` |
| ESP32-S3 | M2 | 2× Xtensa LX7, 240 MHz | `sp` | 512 KiB (+ PSRAM) | 8 MiB (externa) | 512 / 128 / 8 / 64 / 8 | `esp-hal` (`esp32s3`), `chip-esp` |
| ESP32 (original) | M2, **só desenvolvimento** (documento 23) | 2× Xtensa LX6, 240 MHz | `sp` | 520 KiB | 4 MiB (externa) | 256 / 64 / 4 / 32 / 4 | `esp-hal` (`esp32`), `chip-esp` |
| STM32H5 (H563) | M2 | Cortex-M33, 250 MHz | `sp` | 640 KiB | 2 MiB | 512 / 128 / 8 / 64 / 8 | `embassy-stm32` (`stm32h563zi`), `chip-stm32` |
| STM32H7 (H743) | M2 | Cortex-M7, 480 MHz | `dp` | 1 MiB | 2 MiB | 1024 / 256 / 8 / 128 / 8 | `embassy-stm32` (`stm32h743zi`), `chip-stm32` |

As capacidades por omissão vêm do perfil de placa (`ref/03`) e sobrepõem-se com `[target.capacity]`. A versão de cada HAL é **exata** e fixada no `Cargo.lock` do workspace, e muda uma vez por fase (documento 13, §5.1); o `esp-hal` usa `unstable` para o ADC, o toque e o watchdog, isolado no `chip-esp`. O ESP32-C6 não tem FPU: toda a vírgula flutuante é por software, e as apps com `f32` recebem o aviso `J0780`.

#### 2.15.2 Orçamento de referência (a medir na Fase 2)

Configuração do documento 19, §3.1: núcleo + Joker Link (UART, com cifra) + Modbus RTU + 1 app + 64 sinais (16 saídas, 2 grupos), num RP2350. Alvo: **≤ 160 KiB de flash e ≤ 48 KiB de RAM** (porta de saída 4 da Fase 2, documento 11).

| Componente | Flash (alvo) | RAM (alvo) |
|---|---|---|
| núcleo (registo, árbitro, envelope, alarmes, classes, métodos) | 44 KiB | — |
| `joker-expr` (VM, janelas) | 12 KiB | — |
| Joker Link (CBOR, COBS, CRC) | 14 KiB | 3 KiB (buffers de 1 KiB, estado) |
| cifra e assinatura (ChaCha20-Poly1305, Ed25519 só verificação, SHA-256/512) | 24 KiB | — |
| Embassy + HAL + temporizadores | 18 KiB | 2 KiB |
| driver Modbus RTU | 8 KiB | 0,5 KiB |
| persistência + OTA | 7 KiB | 0,5 KiB (buffer de página) |
| app (código) | 4 KiB | 1 KiB (estado) |
| `libm` f32, formatação mínima | 5 KiB | — |
| geração (64 sinais, embutida) | 6 KiB | — |
| slots (64 × 32 B) e imagens (2 grupos × 32 portas × 16 B) | — | 3 KiB |
| saídas (16 × (96 + 3 × 24) B) | — | 2,7 KiB |
| expressões, filas (caixas, métodos, eventos, ações) | — | 4 KiB |
| traço (U9) | — | 16 KiB |
| pilhas (serviço 4 KiB, 2 grupos 2 × 1,5 KiB, ISR 1 KiB) | — | 8 KiB |
| **Total** | **142 KiB** | **40,7 KiB** |

Se o orçamento falhar, o primeiro corte é o traço (8 KiB) e o segundo a verificação Ed25519 em firmware sem geração em flash (feature `gen-flash`).

**A exceção declarada: o rádio do ESP32.** O núcleo não aloca, mas o Wi-Fi do ESP32 (`esp-radio`) precisa de um heap (`esp-alloc`) e de um escalonador preemptivo (`esp-rtos`). A regra:

| Regra | Porquê |
|---|---|
| o heap e o `esp-rtos` só existem no firmware de uma placa ESP32 com `[net] wifi = true` e um `[net.wifi]` no manifesto do MCU (ref/01 §5.22); sem Wi-Fi, não entram na imagem | o resto dos alvos não paga o custo |
| o heap tem **tamanho fixo**, declarado na placa (`[capacities] radio_heap_bytes`, omissão 72 KiB), é só do rádio e fica fora do núcleo: nenhum crate do núcleo, driver ou app lhe acede (o `#[global_allocator]` só é visível no módulo `chip-esp`) | o invariante "o núcleo não aloca" continua verdadeiro |
| o heap e as pilhas do `esp-rtos` **contam no `J0710`**, somados à RAM da geração | a geração não pode caber "no papel" e falhar com o rádio ligado |
| as tarefas do rádio correm com prioridade abaixo dos executores dos grupos (`[irq] reserved` da placa, ref/03 §2.3.3) | o rádio não preempta o ciclo |
| o **jitter dos grupos mede-se com o rádio ativo** (tráfego UDP no link), além de com ele desligado, na porta 5 da Fase 2 e no `jk bench --target mcu` | a interferência do escalonador do rádio é o risco (documento 25, §5) |

A versão do `esp-radio` (beta em 2026) fica fixada exatamente no workspace (documento 13, §5.1).

#### 2.15.3 Pilha e tempo

| Alvo | Métrica | Alvo (a medir) |
|---|---|---|
| RP2350 | jitter do tick a 1 kHz, p99,9 | ≤ 10 µs (porta 5 da Fase 2) |
| RP2350 | `jk_tick` de 64 sinais, 16 saídas, 1 app simples | ≤ 50 µs |
| RP2350 | envelope + árbitro por saída | ≤ 1 µs |
| RP2350 | `jk_set_input_f32` | ≤ 0,5 µs |
| RP2350 | `jk_generation_open` de um blob de 16 KiB (SHA-256 + Ed25519) | ≤ 30 ms |
| M1 | reset → primeiro tick (sem link) | ≤ 100 ms |
| todos | ciclo máximo | ~10 kHz com lógica simples (19 U4) |

### 2.16 Códigos `J07xx` novos

Erros do `jk check` e do `jk build` para alvos MCU. Os `J0710`, `J0720` e `J0721` vêm do documento 19 e ficam definidos aqui (o `J0720` também na ref/04 §4.16).

| Código | Gravidade | Mensagem curta |
|---|---|---|
| `J0710` | error | a geração não cabe nas capacidades do firmware ou do alvo (com as capacidades excedidas e a estimativa de RAM e flash, §2.13.3; num ESP32 com Wi-Fi, inclui o heap do rádio, §2.15.2) |
| `J0720` | error | API ou secção do manifesto indisponível no Joker Micro (§2.11.5, ref/04 §4.14) |
| `J0721` | warning | `f64` num MCU sem FPU de dupla precisão (`fpu` = `none` ou `sp`, §2.14) |
| `J0770` | error | alvo MCU desconhecido ou não suportado (placa sem perfil MCU, arquitetura big-endian) |
| `J0771` | error | `--generation` sem descritor de firmware válido (`--firmware`) |
| `J0772` | error | a geração usa uma app que o firmware não contém (nome, major ou `meta_hash` diferentes) |
| `J0773` | error | a geração usa um driver que o firmware não contém (nome, major ou digest do descritor diferentes) |
| `J0774` | error | pilha estimada acima do orçamento de um contexto |
| `J0775` | error | colisão de identificadores gerados (C ou Rust) |
| `J0776` | error | mais grupos de ciclo do que o alvo suporta (8 na v1) |
| `J0777` | error | período de grupo abaixo do mínimo do alvo (100 µs) ou não representável no temporizador |
| `J0778` | error | persistência pedida (trips, `kv`, parâmetros persistentes) sem área de persistência |
| `J0779` | error-in-prod | blob de geração sem assinatura |
| `J0780` | warning | vírgula flutuante (`f32`) por software num alvo sem FPU |
| `J0781` | error | blob ou firmware maior do que a sua área de flash |
| `J0782` | error | tipo de sinal indisponível no Joker Micro v1 (bloco, stream) |
| `J0783` | error | escritas agendadas (`schedule_depth > 0`, `timed`) num MCU: chegam na 1.1 |
| `J0784` | error | `kv` de app acima de 16 KiB |
| `J0785` | error | sinal de dispositivo `extern` sem `type`, `direction` ou (se numérico) `unit` |
| `J0786` | error-in-prod | placa sem saídas seguras no reset (`reset_safe = false`); aceitável com justificação por `[target] accept_unsafe_reset` (ver a seguir à tabela) |
| `J0787` | warning | identificador gerado com mais de 63 caracteres |
| `J0788` | error-in-prod | geração em flash não ligada a um nó (`node`) |
| `J0789` | error | a geração precisa de um formato de blob, ISA ou `min_runtime` acima do que o firmware suporta |
| `J0790` | error | entrada obrigatória da interface externa sem correspondência na geração |
| `J0791` | warning | pilha de um contexto não medível (recursão ou chamadas indiretas); o orçamento não fica verificado |
| `J0792` | warning | desgaste estimado da flash: vida abaixo de 10 anos com as taxas declaradas |
| `J0793` | warning | entrada opcional da interface externa sem correspondência na geração |
| `J0794` | error | nó em `prod` com um MCU cuja geração é `dev` (o manifesto do MCU tem `mode = "dev"`, ou o blob tem o bit `DEV`) |
| `J0795` | error | firmware Joker Micro sem recetor de OTA: feature `ota` desligada, ou placa sem `FW_STAGING`, `GEN_A`/`GEN_B` ou bootloader A/B (§2.12.6) |
| `J0796` | error | a geração usa funcionalidades que o núcleo do MCU (`[mcu.<d>] core` no lock) não conhece (ref/02 §2.1; §3.11) |

`J0797`–`J0799` ficam livres.

**O MCU num nó em `prod`.** Um nó em `prod` só envia a um MCU blobs de geração `prod` (bit `DEV` do cabeçalho a 0, §2.13.2). O `jk check` do manifesto do nó verifica-o a partir do manifesto do MCU (a chave `micro` do dispositivo `joker-link`, ref/01 anexo A) e dá `J0794`; o `jokerd` volta a verificá-lo antes de levar um blob pelo link (`OTA` de geração, ref/07 §9) e recusa a geração do nó com o mesmo código, e o Deck também, na validação. Um nó em `dev` leva os dois. O MCU não decide isto sozinho: o que ele aceita é a sua própria política (o `min_mode` do bundle de confiança, ref/02 §2.5.4).

**`J0786` com justificação.** Com o manifesto do MCU em `prod`, o `J0786` é erro. Uma placa sem saídas seguras no reset pode ser aceite com justificação, como o fallback em falta (`accept_missing_fallback`, ref/01 §5.10): a chave `accept_unsafe_reset = "<justificação>"` no `[target]` do manifesto do MCU (ref/01 §5.22) passa o `J0786` a aviso, e o `jk lock` copia-a para `[accepted] accept_unsafe_reset."<placa>"` do lock desse manifesto (ref/02 §2.1.14). A aceitação não muda o firmware: os pinos continuam no estado de reset do hardware até ao passo 6 do arranque (§2.12.3); só fica escrito, assinado com a geração, quem aceitou isso e porquê.

### 2.17 O que isto pede ao manifesto e às placas

Este documento não define chaves do manifesto nem de placas: estão na `ref/01` (`[target]`, `[target.capacity]`, `[target.extern]`, `[target.micro]`, `[link]` no §5.22, apps `extern`, `[clock] sync`) e na `ref/03` (placas MCU). A tabela resume o que o `jk build` lê delas.

| Onde | Chave | Tipo | Para quê |
|---|---|---|---|
| `[target]` | `board = "<placa MCU>"` | string | o `jk build --target mcu:<placa>` e o `board_id` |
| `[target]` | `accept_unsafe_reset = "<justificação>"` (ref/01 §5.22) | string | aceita o `J0786` em `prod`; vai para o `[accepted]` do lock do MCU e não muda o comportamento do firmware (§2.16) |
| `[project]` | `mode` | `"dev"` \| `"prod"` | bit `DEV` do cabeçalho; um nó em `prod` só envia blobs `prod` (`J0794`, §2.16) |
| `[target.capacity]` | os nomes do §2.13.3 (`signals = 128`, `trace_bytes = "16 KiB"`, …) | inteiro ou tamanho | capacidades do build |
| `[target.extern]` | `expose = ["alarm.**", "calc.*"]` | seletores | sinais que o firmware pode ler (§2.8.2) |
| `[target.micro]` | `service_deadline = "100ms"`, `freeze_on_trip = true`, `samples_ring = 256`, `event_queue = 16`, `confirm = "5m"` | | secção `NODE` |
| `[clock]` | `sync = "link" \| "extern" \| "none"` (valores novos para MCU) | string | fonte do relógio |
| `[devices.<d>]` | `driver = "extern"` | string | dispositivo alimentado pelo integrador |
| `[devices.<d>.signals]` | `<nome> = { type, unit, direction, … }` | tabela | sinais de um dispositivo sem perfil (`J0785`) |
| `[apps.<a>]` | `kind = "extern"`, `writer = { … }`, `writes = [saídas]`, `reads = [seletores]` | | escritor externo |
| `[apps.<a>.params]` | `<p> = { value = …, runtime = true, persist = true }` | | parâmetros alteráveis (§2.6.l) |
| `[link]` (ref/01 §5.22) e `[outputs.<s>] remote_levels` | transporte (`bus` ou `address`), `addr`, `auth`, `peer`, níveis delegados | | secção `LINKS` |
| `[net.wifi]` (ref/01 §5.22) | `ssid`, `password = "secret:<nome>"`, `country`, `hostname` | strings | secções `NET` e `SECRETS`; exige `[net] wifi = true` na placa |
| `[devices.<d>]` com `driver = "esp32"` (ref/01 anexo A) | sinais com `touch = <0–9>`, `rssi = true`, `uptime = true` | | secção `DEVICES`; o `touch` resolve-se pelo `touch_channel` dos `[pins]` da placa |
| perfil de placa MCU (`ref/03` §2.3.1, §2.3.3) | `[board]` com `os = "none"`, `mcu_class`, `chip` (o nome do HAL, que escolhe a família `chip-*`), `arch` (triplo Rust), `fpu` (o enum do §2.14, chave 15), `memory`, `flash`, `reset_safe`, `stack`; `[areas]` (offsets e tamanhos), `[irq] priorities`, `[watchdog]`, `[capacities]` por omissão, `[link] default` (modo de manutenção) | | §2.12, §2.15 |

**Como a palavra-passe do Wi-Fi chega ao MCU.** O MCU precisa dela antes de ter rede, e ninguém no caminho a deve poder ler:

1. **Chave do MCU.** Um MCU M2 com `[net.wifi] password` tem uma chave X25519 de segredos no registo 19 da área `IDENTITY`, como um MCU autónomo. É gerada no enrolamento: `jk node enroll <mcu> --link <d> --ca <pasta>` (coprocessador, documento 07 §3.1) ou o enrolamento de um MCU autónomo; a chave pública vai no certificado do MCU, e o MCU passa a ser o destinatário `node:<mcu>` do `secrets.toml.enc` (ref/02 §2.4.8, com a atestação do §3.4.2).
2. **Cifra.** `jk secret set wifi_bancada` cifra o valor para os destinatários, incluindo `node:<mcu>`, como qualquer segredo. Um nome em falta é `J0920`; sem o `wrap` do MCU, `J0922`.
3. **Build.** `jk build --generation` copia a entrada, **ainda cifrada**, para a secção `SECRETS` do blob, só com o `wrap` de `node:<mcu>`, e escreve em `NET` o nome do segredo. O `jk` não decifra nada: não precisa da chave de escrow. A assinatura do blob (§2.13.5) cobre as duas secções.
4. **Transporte.** O blob chega ao MCU por `jk flash` ou pelo link (OTA de geração, ref/07 §9). O nó Linux e o Deck levam-no sem o conseguir ler, tal como os outros segredos (ref/02 §3.4.3).
5. **Arranque.** O firmware verifica o blob, decifra a entrada com a chave do registo 19 (HPKE, ref/02 §2.4.4), liga-se ao Wi-Fi e apaga a cópia em claro da RAM depois de a passar ao rádio. O valor nunca vai para o traço, para o `HELLO` nem para a persistência.

Como o primeiro blob tem de chegar antes de haver rede, a primeira gravação é sempre por `jk flash` ou pelo link série; um link só por IP num MCU sem geração válida cai no modo de manutenção, que usa o `[link] default` da placa (§2.12.3). **Rodar um segredo num MCU: a exceção à ADR 0024 §3.** Num nó Linux, rodar um segredo não cria uma geração nova (ref/02 §3.4.4). Num MCU, o segredo vai **dentro do blob** (secção `SECRETS`), e por isso rodá-lo produz um **blob novo com o mesmo manifesto**: `jk secret set` seguido de `jk build --generation` (ou do `jk apply`, que o faz quando a `rev` de um segredo do MCU sobe) dá um blob com **novo `gen_seq`** (o anti-rollback do §2.12 continua a valer) e novo `image_hash`, mas o mesmo `source_hash` e o mesmo `registry_hash`: a geração Linux e o seu digest não mudam, e o Deck não vê nada que dependa do valor. O blob novo segue o caminho normal (confirmação, rollback, §3.7), e o MCU tem de chegar à rede nova com o blob anterior ou pelo link série.

---

## 3. Comportamento esperado

### 3.1 O tick

- A ordem dos passos é a do documento 01, §4, e as regras são as do documento 16 (árbitro A1–A14, envelope E1–E10, qualidade §3, expressões §4). O MCU não tem regras próprias: é o mesmo `joker-core`.
- **Passo 2:** a caixa de cada grupo recebe, por ordem de chegada, as propostas e os registos de método dos escritores externos (`jk_propose`, `jk_call_method`), do link (`PROPOSE`, `CALL`) e das tarefas async das apps. Todas contam como async para a regra A4: a última do mesmo escritor vence, e uma de ciclo do mesmo escritor no mesmo tick vence-as.
- **Passo 4:** as tarefas de ciclo são chamadas nativas. Não há *epoch*: uma tarefa que não termina encrava o grupo, e o `jk_supervise` força o `safe` das suas saídas a 3 períodos (§3.4). Um `Err` termina a tarefa sem descartar as propostas já feitas; um *panic* é `jk_fatal` (§2.6.b).
- **Passo 7:** o commit escreve o valor aplicado na caixa de buffer duplo de cada saída (o leitor nunca espera), marca `CHANGED`, marca para o serviço os registos persistentes a gravar e põe na fila do link as mudanças a anunciar (`OUTCOMES`, `SAMPLES`).
- `now_us` do `jk_tick` é o instante do latch: o `cx.now()` das apps, o `t_mono` do traço e a base dos leases (A2). A política `overrun` usa a duração medida com `jk_now_us()` e só afeta os ticks seguintes, com a flag gravada no traço, para o replay ser exato.

### 3.2 Determinismo e conformidade

- Os casos de conformidade (documento 16, §8) correm no MCU com um firmware de conformidade (`joker-micro-conformance`, documento 13, tarefa 22), que recebe os ticks do caso por sonda (RTT) ou pelo link, chama `jk_set_input_ex`, `jk_propose`, `jk_tick` com os tempos do caso, e devolve os resultados. **Têm de dar os mesmos bytes que no Linux** (ADR 0020).
- Nada de não determinista entra nas decisões de um tick: nem `jk_now_us`, nem a ordem de preempção entre grupos (um grupo só vê de outro os valores aplicados à data do seu latch, documento 02, §6.3), nem o momento em que uma ISR escreveu uma entrada (o latch fixa-a, e o traço grava-a).
- O traço do MCU é o do documento 16, §7, com os tempos em ns (µs × 1000, exato) e sem compressão; o `jk replay` corre o mesmo núcleo no portátil (19 U9).

### 3.3 Entradas e leituras

- Uma entrada escrita antes do `jk_tick` de um grupo é vista nesse tick; escrita durante o tick, no seguinte. Nunca a meio.
- O envelhecimento (`max_age`, documento 01, §3.3) calcula-se no latch com o `now_us` do tick e, no `jk_read`, com o `now_us` passado.
- Uma colisão do leitor com o escritor dá a amostra anterior completa, e conta-se. Com o escritor numa ISR e o grupo numa tarefa, só acontece se a ISR preemptar o latch entre as duas metades de uma cópia de 32 B: é raro e inofensivo.

### 3.4 Saídas e estado seguro

| Situação | O que o integrador lê em `jk_get_output` | Garantido por |
|---|---|---|
| antes do `jk_init` | nada (`JK_E_STATE`): os pinos estão no estado de reset do hardware | a placa (`reset_safe`, `J0786`) |
| depois do `jk_init`, antes do primeiro tick | o `safe` | A8 |
| nenhum escritor ativo (leases expirados, link em baixo) | o `safe` (`JK_OUTF_SAFE`) | A6 |
| grupo encravado > 3 períodos | o `safe` (`JK_OUTF_FORCED`), se o integrador ainda estiver a ler; senão, escreve-o a partir da ISR do supervisor | `jk_supervise` |
| trip | o `safe` (`JK_OUTF_TRIPPED`, `Bad(tripped)`), persistente | E7, §2.10 |
| `jk_force_safe(true)` | o `safe` (`JK_OUTF_FORCED`) | |
| nó parado (`HALTED`) | o `safe`; `feed_watchdog = 0`, e o watchdog reinicia o MCU | §3.9 |
| MCU congelado | nada: o watchdog de hardware reinicia; até lá, o último valor fica nos pinos | watchdog + fallback do equipamento (documento 01, §8) |
| `safe = "relinquish"` em vigor | `JK_RELINQUISHED` | documento 03, §6.1 |

O valor devolvido é **sempre admissível**: o integrador aplica-o sem pensar. É isso que torna o MCU coprocessador seguro por desenho (19 §6, U1).

### 3.5 Concorrência

- As estruturas partilhadas entre contextos são só estas, todas sem locks: slots (escritor único), caixas das saídas (buffer duplo, escritor = grupo dono), caixas de propostas por grupo (SPSC: produtor = serviço, consumidor = grupo), filas de eventos, `OUTCOMES` e `SAMPLES` por grupo (SPSC: produtor = grupo, consumidor = serviço), bits de `force_safe` e de congelamento do traço (atómicos). A ordem de memória é *acquire/release*; em multi-núcleo, a memória do nó tem de ser coerente (§2.9.3).
- Grupos diferentes nunca partilham estado mutável: cada um tem a sua imagem, as suas saídas, o seu anel de traço e as suas estatísticas (documento 01, §3.4).

### 3.6 Link e leases

- As propostas do par (nó Linux ou Deck) entram com o nível que trazem, desde que esteja nos níveis permitidos do link (senão `rejected(not_owner)`), e com um lease ≤ `max_lease` do link (A9, 16 A12; o MCU não os persiste, §2.10.1). O escritor é o par (link, nível) da tabela `WRITERS`.
- Se o link cair, nada acontece de imediato: os leases expiram no latch do grupo dono (A2), e o envelope aplica o `safe`. O tempo até ao seguro é, no máximo, o lease mais um período do grupo (19 U1: ≤ 40 ms com um lease de 3 × 10 ms).
- O `HELLO` anuncia o `source_hash` (o hash da geração), o `registry_hash`, o `gen_seq`, o descritor de firmware (versão, digest da imagem e do descritor, hash das capacidades, `security_version`), o resumo dos descritores dos drivers planeados, os parâmetros alterados localmente e as exportações da geração (sinais, instâncias, contratos das classes, ações), a partir do blob. O formato é do `ref/07` §6.4.

### 3.7 Gerações

- Troca num limite de tick, com herança do valor aplicado, dos leases e dos trips das saídas que se mantêm (A8, documento 18, P6). Uma saída nova começa em `safe`; uma saída removida deixa de existir (o firmware deixa de a aplicar: o integrador vê `JK_E_SIGNAL` se ainda a tiver na interface, ou a interface impediu a geração, `J0790`).
- Confirmação e rollback como no documento 01, §15, a quente, sem reiniciar o MCU.
- A geração ativa e a anterior têm de ficar mapeadas até à confirmação. No modo firmware, são as duas áreas A/B.

### 3.8 Tempo

- O relógio começa `Unsynced`. Com a fonte `link`, cada `TIME` fixa a correspondência e põe-no `Synced`; sem `TIME` passa a `Holdover` e, depois de `max_holdover`, a `Unsynced` (documento 03, §3). Com a fonte `extern`, o mesmo com `jk_time_set_utc`.
- As apps com `requires_clock = "synced"` têm as saídas bloqueadas (`rejected(clock_unsynced)` no passo 0 do envelope, que leva ao `safe`) enquanto o relógio não estiver `Synced` (documento 01, §7 e §18; 16, E1).
- Os instantes monotónicos do MCU nunca saem dele sem o par UTC (`ref/00` §3); o `jokerd` converte µs ↔ ns na fronteira.

### 3.9 Erros e falhas internas

- Nenhum erro de chamada muda estado: uma função que devolve um erro não fez nada (exceto as que o dizem: `jk_action_result` com `JK_E_FULL` mantém o bilhete).
- Uma invariante interna partida (um estado impossível, um índice fora do blob que o verificador devia ter apanhado) põe o nó em `HALTED`: todas as saídas em `safe`, todas as funções com estado devolvem `JK_E_HALTED`, `jk_supervise` deixa de pedir o watchdog, e `jk_fatal(JK_E_INTERNAL, detalhe)` é chamado no próximo contexto de serviço ou de grupo. O detalhe identifica o sítio e vai para o registo do próximo arranque, se houver persistência.

### 3.10 Limites

| Coisa | Limite v1 |
|---|---|
| grupos por nó | 8 (ABI: 32) |
| período mínimo de grupo | 100 µs (~10 kHz, 19 U4) |
| argumentos de um método pela ABI (`jk_value_t`) | 8; mais, por CBOR |
| chave de idempotência | 1–64 B ASCII imprimível (`ref/00` §10) |
| `string`/`bytes` num sinal | 64 B |
| evento ou argumento de ação | 1 KiB, listas de 64 |
| `kv` por app | 16 KiB (19 §3) |
| níveis do árbitro | 16 |
| pilha de expressões | 32 células |
| blob | 16 MiB (`u32` offsets; na prática, a área de geração) |
| MTU do link | a do `ref/07`, por transporte |

### 3.11 Compatibilidade

| Entre | Regra |
|---|---|
| código C e biblioteca | ABI: mesma major, minor do header ≤ minor da biblioteca, mesmo hash de interface (§2.3) |
| firmware e blob | o firmware lê os formatos e ISAs até aos do seu descritor; o `jk` escreve o mais alto que o firmware lê (§2.13.6). Um `jk` 1.3 produz blobs para um firmware genérico 1.0; um firmware 1.3 lê os blobs de um `jk` 1.0 |
| firmware novo e persistência antiga | o formato do diário (§2.10.2) só cresce: tipos novos, campos CBOR novos (ignorados pelos antigos) |
| firmware e `jokerd` | pelo `link_version` do `HELLO` (`ref/07`) |
| geração e núcleo do firmware | o lock fixa a versão do núcleo de cada MCU (`[mcu.<d>] core = "1.0"`, ref/02 §2.1.10). O `jk check` só deixa numa geração do MCU o que esse núcleo conhece: uma funcionalidade de uma minor posterior (regra do núcleo, instrução de expressão, secção do blob, chave do manifesto) é `J0796`, com a versão que a introduziu. Para a usar, grava-se primeiro um firmware com um núcleo mais novo (OTA de firmware, §2.12.6) e atualiza-se o lock |
| apps e firmware | uma app nativa é código do firmware; mudar de app é um firmware novo (ou, na 1.4, `wasmi`). A geração refere a app por nome, major e `meta_hash` (`J0772`) |
| 1.x | regra de ouro do ADR 0022: nenhum blob, persistência, header ou chamada que funcione numa 1.y deixa de funcionar numa 1.z > y |

---

## 4. Estresse

**D1 — O header foi regenerado, mas a biblioteca antiga ficou no projeto.** O integrador acrescentou um sinal ao manifesto, correu `jk build --lib` e copiou só o header. Os índices mudaram, e `JK_SIG_TANK_LEVEL` agora aponta para outro sinal. **Quebrava** em silêncio. **Correção incorporada:** `jk_init` é uma macro que passa `sizeof(node)`, `JK_ABI_VERSION` e `JK_PROJECT_IFACE_HASH`; a biblioteca recusa com `JK_E_ABI` (`J-E2503`) antes de ler qualquer entrada.

**D2 — Firmware genérico e uma geração nova com mais sinais.** No modo biblioteca com geração em flash, a geração do site B tem um sinal que a do site A não tinha, e todos os `SignalId` a seguir deslocam-se. Um header com `SignalId`s estaria errado em metade dos sites. **Quebrava. Correção incorporada:** os identificadores do header são índices da **interface externa**, fixada no build da biblioteca e ligada por nome a cada geração no `jk_init` (§2.1, §2.8.2). Uma geração a que falte uma entrada obrigatória é recusada (`J0790` no build, `JK_E_GEN_FIRMWARE` no MCU).

**D3 — Seqlock num MCU de um núcleo.** O integrador escreve uma entrada numa tarefa de prioridade baixa e o grupo corre numa interrupção de prioridade alta. Com um seqlock clássico, o latch que interrompe o escritor a meio espera que a versão fique par, e a tarefa que a tornaria par nunca volta a correr. **Quebrava** (bloqueio total). **Correção incorporada:** leitores sem espera: uma colisão devolve a amostra completa anterior e conta-se (§2.4.1). Proposta de nota ao documento 01, §3.2 (relatório).

**D4 — Duas tarefas chamam `jk_tick` do mesmo grupo.** Um integrador de FreeRTOS arranca o laço duas vezes por engano. **Aguenta:** a bandeira de ocupado do contexto de grupo devolve `JK_E_REENTRANT` à segunda chamada, sem efeito. Grupos **diferentes** em tarefas diferentes são o uso previsto.

**D5 — Falta de energia 1 ms depois de um trip.** O trip está no nó, mas o `jk_service` ainda não o gravou. **Não aguenta por inteiro:** o trip perde-se, e a saída arranca em `safe` mas sem trip. **Mitigação declarada:** o registo do trip é gravado no primeiro `jk_service` (≤ `service_deadline`); `jk_shutdown` grava em *brown-out* se a placa tiver aviso; e se a condição persistir (o caso habitual, um alarme `actions.trip`), o trip volta a acontecer no primeiro tick. A diferença face ao `fsync` do Linux fica escrita no §2.10.3 e no relatório para o documento 19.

**D6 — O `jokerd` morre (U1).** Os leases das propostas do link expiram no latch do grupo dono do MCU; em ≤ lease + 1 período as saídas estão em `safe`. Se for o **laço do MCU** a encravar, a ISR do integrador com `jk_supervise` força o `safe` a 3 períodos e deixa de alimentar o watchdog, que reinicia o MCU. **Aguenta.**

**D7 — Um blob válido do site A carregado no site B.** Mesmo firmware genérico, mesma chave de assinatura: a assinatura verifica. **Quebrava** (o site B passava a ter os envelopes do site A). **Correção incorporada:** o manifesto COSE pode ligar o blob a um nó (chave 14), comparado com a área `IDENTITY` (`JK_E_GEN_NODE`); em `prod` a ligação é obrigatória (`J0788`). O anti-rollback (`gen_seq > confirmed_seq`) impede também a reposição de um blob antigo do próprio site.

**D8 — Firmware genérico 1.0 em campo, `jk` 1.3 no portátil.** O `jk` 1.3 tem uma função de expressão nova. **Aguenta:** o `jk build --generation` lê o descritor do firmware, escreve o formato e a ISA que o firmware entende, e dá `J0789` se a geração precisar de algo que só existe num firmware mais novo, com a sugestão de atualizar o firmware primeiro.

**D9 — Uma app com `f32` num ESP32-C6.** O C6 não tem FPU; o `J0721` só falava de `f64`. Uma malha PID de 1 kHz em `f32` custa 10× mais do que o esperado. **Quebrava** (o aviso não saía). **Correção incorporada:** o descritor declara a FPU, e o `jk build` procura as rotinas de vírgula flutuante por software no objeto da app: `J0721` para `f64` sem FPU de dupla precisão e o novo `J0780` para `f32` sem FPU.

**D10 — Um `int` num sinal `bool`, um `double` num `f32`, uma string de 80 B.** Erros comuns de C. **Aguenta:** a macro `_Generic` escolhe o setter pelo tipo do valor; a tabela do §2.5.4 aceita `0`/`1` num `bool` e arredonda um `double` para `f32`; um `double` num `u64` é `JK_E_TYPE` e a string longa `JK_E_VALUE`, ambos sem efeito e com código. Handles tipados em compilação existem no crate Rust (`In<T>`, `Out<T>`), e não em C (§5).

**D11 — Uma geração nova a meio de uma rampa (documento 18, P6).** A saída `p_set` está a subir a 10 kW/s. **Aguenta:** `jk_generation_swap` mantém o último valor aplicado, os leases com a mesma identidade e o estado de trip; a taxa continua a partir do último aplicado (E2). Com a janela de confirmação expirada, o regresso à geração anterior é igual, no sentido contrário.

**D12 — Pilha esgotada com quatro grupos aninhados.** Cada grupo é um executor de interrupção com prioridade própria; os quatro podem estar ativos ao mesmo tempo sobre a mesma pilha principal. **Quebrava** (corrupção silenciosa de memória). **Correção incorporada:** o `jk check` soma as pilhas de todos os níveis de prioridade e compara com a da placa (`J0774`); no modo firmware, o `joker-micro` liga com `flip-link` (Cortex-M), e um transbordo passa a ser uma falha de hardware, logo um reset, e não corrupção.

---

## 5. Decisões e alternativas rejeitadas

| Decisão | Alternativa rejeitada | Porquê |
|---|---|---|
| dois headers: `joker_node.h` fixo e `joker_project.h` gerado | um só header gerado (a leitura literal do 19 §4.2) | o fixo pode ser revisto, versionado e compilado em CI como contrato; o gerado muda a cada build |
| identificadores = índices da interface externa, ligados por nome no `jk_init` | `SignalId`s da geração no header | partiam com cada geração em flash (D2) |
| `jk_now_us` definido pelo integrador, mas lido só no link e nas estatísticas | a biblioteca lê o relógio no tick | ADR 0007: o tempo do tick tem de ser o do argumento, para o traço e o replay |
| `jk_platform_t` com callbacks e `user` | símbolos fracos (`__attribute__((weak))`) | não são C11, e não permitem dois nós |
| valores positivos informativos (`JK_EMPTY`, `JK_PENDING`, `JK_RELINQUISHED`) | só `JK_OK` e erros, com parâmetros de saída | "não há mais eventos" não é um erro; o `ref/00` §8 só fixa o 0 e os negativos |
| setters genéricos com `_Generic` + funções tipadas | handles tipados em C (`struct` por tipo) | não servem em `switch`, pesam no DX e o C++ não tem `_Generic`; a verificação fica em runtime (`JK_E_TYPE`), e em compilação no Rust |
| escritores externos com propostas "async" antes do tick | *callbacks* C chamados dentro do tick (passo 4) | o WCET do C do integrador não entra na admissão, e a reentrância a partir do tick seria uma armadilha; drenadas no passo 2 dão o mesmo efeito no laço único |
| `jk_link_poll` inclui um passo de `jk_service` | serviço só explícito | o exemplo do documento 19 não chamaria o serviço, e a persistência nunca correria |
| um contexto de serviço por nó | serviço multi-thread | as filas ficam SPSC e sem locks; o trabalho do serviço é pequeno |
| blob binário de tabelas fixas, usado no sítio (XIP) | CBOR, FlatBuffers | CBOR obrigaria a copiar para RAM; FlatBuffers traz uma dependência e um verificador que não conhece a nossa semântica; as tabelas são o formato do núcleo |
| COSE_Sign1 sobre o manifesto de OTA da ref/07, com o SHA-256 da imagem | COSE com a imagem na carga útil; um *claim* próprio do blob ao lado do manifesto de OTA | a imagem deixaria de estar alinhada e mapeável, e o Ed25519 teria de percorrer tudo; duas formas de assinar a mesma coisa seriam dois verificadores |
| saltos só para a frente no bytecode | saltos livres | as expressões são totais; o verificador garante tempo limitado pelo tamanho |
| diário de persistência definido pela biblioteca | um KV do integrador | a atomicidade, o desgaste e a compatibilidade entre versões de firmware são garantias nossas |
| memória do nó como tipo concreto gerado | arena genérica | sem `unsafe` fora do `ffi` (invariante 10 do `AGENTS.md`) |
| *panic* de app = `jk_fatal` | apanhar o *panic* | não há `catch_unwind` em `no_std`; um estado meio atualizado é pior do que um reset para o estado seguro |

---

## 6. Marcações de versão

| Elemento | v1.0 | Reservado |
|---|---|---|
| ABI C | 1.0, todas as funções deste documento | numa 1.x só se acrescenta (§2.3) |
| tipos de sinal | 0–8 | 16 bloco, 17 stream (v1.1, "Preparado para ML") |
| secções do blob | 0x0001–0x0024 | 0x0040 `MODELS`, 0x0041 `BLOCKS`, 0x0042 `STREAMS` (v1.1); 0x0043 `SCHEDULES` (escritas agendadas, M6, v1.1); 0x0044 `WASM_APPS` (`wasmi`, v1.4) |
| traço | `n_saídas_ML = 0` sempre | saídas de modelos no MCU (v1.4, Burn Flex `no_std`) |
| escritas agendadas (`write_at`, `timed`) | não no MCU (`J0783`) | v1.1 |
| ML | não | v1.1 experiência em M2, v1.4 produção (documento 21, §6) |
| apps | nativas em Rust | `wasmi` em M2 (v1.4); Structured Text nativo (v1.2), pelo mesmo `NativeApp` |
| transportes do link | `uart`, `rs485`, `can`, `usb_cdc`, `udp`, `tcp` | 7 `lorawan`, 8 `nbiot` (v1.4, só com procura) |
| baixo consumo | `jk_service_next_us` já existe | grupos acordados por eventos e orçamento energético (v1.4) |
| capacidades | `cap_id` 1–41 | 42+ acrescentam-se no fim |
| códigos | `J0770`–`J0794`, `J-E2501`–`J-E2599` | `J0795`–`J0799`, `J-E2600`–`J-E2999` |
| caminhos qualificados por nó (`@nó.…`) | não | v2 (documento 21, §8) |
