# ref/07 — Joker Link: o protocolo no fio

Referência normativa do Joker Link v1.0, o protocolo entre um Joker Micro e o nó que o usa (um `jokerd`, ou o Deck para um MCU M2). Fecha o documento 19, §6, ao nível do byte: duas equipas que só leiam este documento e `spec/cddl/joker-link.cddl`, uma em Rust `no_std` e outra em C, têm de produzir implementações que interoperam.

Aplica a ref/00 sem a repetir. A semântica (árbitro, envelope, qualidade, traço) é a do documento 16: aqui só se diz como ela atravessa o fio.

Palavras normativas: **tem de** / **não pode** (obrigatório), **deve** (recomendado; desviar exige razão), **pode** (opcional).

---

## 1. Âmbito e fontes

| Cobre | Não cobre |
|---|---|
| enquadramento em UART/RS-485, USB CDC, CAN/CAN-FD (ISO-TP), UDP, TCP | a ABI C e o formato do blob de geração (ref/08) |
| segurança: chaves, handshake, cifra, anti-replay, rotação | o enrolamento da identidade do nó Linux (ref/02) |
| sessão: cabeçalho, confirmação, retransmissão, fragmentação, temporizadores | as mensagens do protocolo do agente (ref/10) |
| as 25 mensagens, com CDDL, sentido, fiabilidade e exemplos em bytes | as chaves TOML do dispositivo `joker-link` e do `[link]` do MCU (ref/01; §15 lista o que esta referência precisa) |
| a máquina de estados da sessão e a degradação | a API HTTP que mostra o estado do link (ref/09) |
| a ligação ao núcleo: árbitro, resultados, qualidade, tempo, traço | |
| OTA de firmware e de geração pelo link | |
| orçamentos de tamanho e de tempo no fio | |
| versões e negociação; enquadramento do perfil micro (ref/10) | |

**Fontes.** 19 §6 (o protocolo), §7 (U1, U7, U9, U10); 16 §1 (A1–A11), §2 (E1–E9), §3, §7 (traço), §8; 01 §3 (slot e seq), §6–§8 (árbitro, envelope, estado seguro), §15–§18 (gerações, OTA, segurança, tempo); 02 §4–§8 (qualidade, tempo, escrita, ações); 05 §12 (estados do link); 17 §2, §4, §9 (métodos, ações, versões de classes); 20 §3.1; ADR 0015, 0020, 0022; ref/00 (tudo).

**Liga-se a:**

| Referência | O quê |
|---|---|
| ref/00 | tempo (§3), valores e qualidade (§4), identificadores (§5), enums (§6), CBOR e COSE (§7), erros (§8) |
| ref/01 | dispositivo `driver = "joker-link"` do nó Linux; `[link]` do manifesto do MCU (`bus`, `address`, `addr`, `peer`, `auth`); `J0179`, `J0234` |
| ref/02 | `[mcu.<d>]` e `[discovered.<d>]` do lock; deriva `J0460`, `J0461`; o segredo da chave do link |
| ref/08 | `jk_link_poll`, `jk_link_status` (contadores de CRC, autenticação e replay, modo da sessão), áreas de flash e registos de identidade, `jk_generation_swap`, `jk_restart`, `jk_trace_read`, formato do blob (o catálogo e o manifesto de OTA vão lá dentro); códigos `J-E25xx`. Os códigos do link são `J-E2000`–`J-E2499`; os da ABI, `J-E2500` em diante |
| ref/09 | estado do link (os seis valores de `link_state`, ref/00 §6), modo da sessão, versão negociada, contadores, último `HELLO`, descritor de firmware, `gen_seq` e estado de confirmação, na API local (`GET /v1/micros/{device}`) |
| ref/10 | fase `mcu` do estado desejado, que usa `OTA` e `HELLO` daqui; perfil micro sobre MQTT/CoAP (§14) |

**Ficheiros:** este documento e `spec/cddl/joker-link.cddl` (os corpos das mensagens, o catálogo e o manifesto de OTA). Os cabeçalhos binários estão só aqui (§3–§5).

**Papéis.** Em cada sessão há um **dispositivo** (D, o Joker Micro) e um **anfitrião** (H, o `jokerd`; ou o Deck, para um MCU M2 cujo `[link] peer = "deck"`). O D é sempre quem começa o handshake. Na v1, um D tem no máximo **uma** sessão de cada vez.

---

## 2. As camadas

```
 ┌──────────────────────────────────────────────────────────────────────────────┐
 │ L4  mensagens     CBOR, chaves inteiras (§6)       HELLO SAMPLES PROPOSE …    │
 ├──────────────────────────────────────────────────────────────────────────────┤
 │ L3  sessão        cabeçalho binário 10–12 B (§5)   tipo seq ack ack_bits rseq │
 ├──────────────────────────────────────────────────────────────────────────────┤
 │ L2  segurança     1 B | +24 B (sessão) | +36 B (handshake) (§4)               │
 │                   em claro, ou ChaCha20-Poly1305                              │
 ├──────────────────────────────────────────────────────────────────────────────┤
 │ L1  enquadramento CRC-32C sempre; COBS + 0x00 em fluxos; 1 datagrama = 1 trama│
 ├──────────────────────────────────────────────────────────────────────────────┤
 │ L0  transporte    UART · RS-485 · USB CDC · TCP  │  CAN/CAN-FD (ISO-TP) · UDP │
 └──────────────────────────────────────────────────────────────────────────────┘
```

| Termo | Significado |
|---|---|
| **trama L2** | os bytes entre o L1 e o L3: cabeçalho de segurança, mensagem (cifrada ou não) e etiqueta |
| **mensagem** | cabeçalho L3 + corpo CBOR |
| **pacote** | uma trama L2 com a sua mensagem; é a unidade que a sessão numera (`seq`) e confirma |
| `max_frame` | o tamanho máximo de uma trama L2, negociado no `HELLO` (§3.6) |
| `max_msg` | o tamanho máximo de uma mensagem depois de remontada (§5.5) |

Todos os inteiros binários (L1, L2, L3) são **little-endian**, como no traço (16 §7).

---

## 3. Enquadramento (L1)

### 3.1 Transportes

| Transporte | Tipo | Enquadramento | CRC-32C | Segurança (§4.6) | Endereço |
|---|---|---|---|---|---|
| UART | fluxo | COBS + `0x00` (§3.2) | sim | opcional | byte `addr` (0) |
| RS-485 full-duplex | fluxo | COBS + `0x00` | sim | opcional | `addr` (0 ou 1–247) |
| RS-485 half-duplex | fluxo, com vez (§3.3) | COBS + `0x00` | sim | opcional | `addr` 1–247 |
| USB CDC-ACM | fluxo | COBS + `0x00` | sim | opcional | `addr` (0) |
| TCP | fluxo | COBS + `0x00` | sim | **obrigatória** | `addr` (0) |
| CAN / CAN-FD | datagrama (ISO-TP) | uma mensagem ISO-TP = uma trama (§3.4) | sim | opcional | identificador CAN |
| UDP | datagrama | um datagrama = uma trama (§3.5) | sim | **obrigatória** | `sid` do L2 |

O CRC-32C está em **todos** os transportes, mesmo onde há outro controlo por baixo (TCP, UDP) ou a etiqueta Poly1305 por cima. Custa 4 B e dá uma só regra de implementação, e permite distinguir ruído (CRC errado) de ataque ou chave errada (CRC certo, etiqueta errada), o que o `jk doctor` mostra em contadores separados.

### 3.2 Fluxos: COBS + CRC-32C

```
no fio:   COBS( addr | trama L2 | crc32c ) | 0x00
          └ 1 B ┘              └ 4 B LE ┘   └ delimitador
```

| Elemento | Regra |
|---|---|
| `addr` | u8. 0 = ponto-a-ponto; 1–247 = endereço do D num barramento partilhado; 248–254 reservados; 255 = difusão do H (só mensagens da classe E, §5.4). Numa trama do H é o endereço do destino; numa do D, o do próprio D |
| `crc32c` | CRC-32C sobre `addr` e a trama L2, escrito em little-endian |
| COBS | *Consistent Overhead Byte Stuffing* (Cheshire e Baker, 1999), na forma original: blocos de até 254 bytes, código `0xFF` sem zero implícito. A saída não tem `0x00` |
| delimitador | um `0x00` a seguir a cada trama |

**CRC-32C** (catálogo *reveng*: CRC-32/ISCSI):

| Parâmetro | Valor |
|---|---|
| largura | 32 |
| polinómio | `0x1EDC6F41` (refletido: `0x82F63B78`) |
| valor inicial | `0xFFFFFFFF` |
| entrada e saída refletidas | sim, sim |
| XOR final | `0xFFFFFFFF` |
| verificação (`"123456789"`) | `0xE3069283` |
| resíduo | `0xB798B438` |

**Emissor.** Codifica, envia e termina com `0x00`. Pode enviar `0x00` extra antes de uma trama (tramas vazias são ignoradas); deve fazê-lo antes da primeira trama depois de o transporte abrir, para descartar lixo na linha.

**Recetor.** Acumula bytes até um `0x00`, descodifica o COBS e valida pela ordem seguinte. Uma falha descarta a trama em silêncio (sem `ERROR`) e conta no contador indicado:

| # | Verificação | Contador (código) |
|---|---|---|
| 1 | acumulado ≤ `max_frame` + 5 + ⌈(`max_frame` + 5) / 254⌉; senão descarta até ao próximo `0x00` | `oversize` (`J-E2002`) |
| 2 | COBS válido e comprimento descodificado ≥ 6 | `framing` (`J-E2003`) |
| 3 | CRC-32C dos bytes descodificados menos os últimos 4 = esses 4 (LE) | `crc` (`J-E2001`) |
| 4 | `addr` é 0, o próprio endereço (D), ou 255 (D, só classe E) | — (não é para mim) |

**Exemplo.** A mensagem `SAMPLES` do §6.9, em claro, no UART (`addr` 0):

```
trama L2  00 12 10 2c 03 92 01 ff ff ff ff a2 01 1a 00 12 eb c0 02 83 83 00 19 04 b1 f5
          84 01 19 03 6d f4 18 fa 85 18 28 19 14 00 f9 5e 72 19 07 08 19 03 01          (49 B)
crc32c    sobre 00 | trama L2  = 0x3C5F95E8  → e8 95 5f 3c
no fio    01 01 0e 12 10 2c 03 92 01 ff ff ff ff a2 01 1a 07 12 eb c0 02 83 83 12 19 04
          b1 f5 84 01 19 03 6d f4 18 fa 85 18 28 19 14 0e f9 5e 72 19 07 08 19 03 01 e8
          95 5f 3c 00                                                                   (56 B)
```

### 3.3 RS-485 half-duplex: a vez

Num par RS-485 a dois fios só um fala de cada vez. O H é o mestre:

1. O H envia uma ou mais tramas para o `addr` A; a última leva a flag `EOT` (§5.2). Isso dá a vez a A.
2. A tem de começar a responder em `turn_timeout` (omissão 5 ms, contado a partir do `0x00` final do H, depois de `turnaround`) e envia de 1 a `turn_frames` tramas (omissão 4), a última com `EOT`. Sem nada para enviar, responde com um `ACK` com `EOT`.
3. Sem resposta em `turn_timeout`, o H conta uma vez perdida e segue para o próximo D. Três vezes perdidas seguidas contam como uma trama perdida para o §5.3.
4. O H dá a vez a cada D pelo menos a cada `report` (§5.6). As tramas com `addr` 255 não dão a vez.
5. `turnaround` (silêncio mínimo antes de transmitir, para o transceptor comutar) vem do barramento (`[buses.<b>] turnaround`, ref/01); omissão 100 µs.

Em full-duplex, `EOT` é ignorada.

### 3.4 CAN e CAN-FD: ISO-TP

Cada trama L2, seguida do seu CRC-32C (4 B, LE), é **uma mensagem ISO-TP** (ISO 15765-2:2016), endereçamento normal, com identificadores de 29 bits:

```
bits 28–24  prioridade     0x06 = controlo, 0x07 = bulk
bits 23–16  protocolo      0x4A (configurável: can_id_base)
bits 15–8   destino        0x00 = H; 0x01–0xFD = D
bits 7–0    origem
```

| Regra | Valor |
|---|---|
| canais | cada D tem dois pares de identificadores: **controlo** e **bulk**. `CATALOG`, `TRACE` e `OTA` vão no bulk; tudo o resto no controlo. Uma transferência ISO-TP em curso no bulk não bloqueia o controlo, porque a arbitragem CAN intercala as tramas pelo identificador |
| CAN clássico | tramas de 8 B; a última pode ser mais curta (sem enchimento); os recetores aceitam as duas |
| CAN-FD | `BRS` ligado; DLC até 64 B; primeira trama com 62 B de dados |
| tamanho | ≤ 4095 B por mensagem ISO-TP (sem o escape de 32 bits), portanto `max_frame` ≤ 4091 |
| controlo de fluxo | `BS = 0`, `STmin = 0` por omissão (configuráveis); `N_Bs` e `N_Cr` = 100 ms |
| erro ISO-TP | sequência errada, timeout ou `FC(Overflow)`: a mensagem perde-se e conta em `isotp` (`J-E2004`); a sessão recupera pelo §5.3 |

Na ABI C, o ISO-TP é do integrador: `jk_link_rx_fn` entrega uma mensagem ISO-TP inteira (trama L2 + CRC) por chamada (ref/08).

### 3.5 UDP

Um datagrama = uma trama L2 seguida do CRC-32C (4 B, LE). Porta do H por omissão **7471** (configurável). O D envia para o H; o H responde para o endereço e porta de origem do último pacote autenticado dessa sessão (o D pode mudar de endereço: DHCP, NAT). A sessão identifica-se pelo `sid` do L2 (§4.1), porque a segurança é obrigatória. Carga útil do datagrama ≤ 1232 B (IPv6 1280 − 40 − 8), portanto `max_frame` ≤ 1228.

### 3.6 TCP e USB CDC

O TCP e o USB CDC são fluxos: usam o §3.2 tal e qual, com `addr` 0. Em TCP, o D liga-se ao H (porta 7471 por omissão), com `TCP_NODELAY`; fechar a ligação é perda de link (§7). Uma ligação = uma sessão.

### 3.7 Tamanhos de trama

| Transporte | `max_frame` mínimo aceite por qualquer implementação | Omissão | Máximo |
|---|---|---|---|
| UART, RS-485, USB CDC, TCP | 512 | 1024 | 4096 |
| CAN, CAN-FD | 512 | 1024 | 4091 |
| UDP | 512 | 1024 | 1228 |

`max_frame` conta só a trama L2 (sem `addr`, CRC, COBS, delimitador ou PCI do ISO-TP). O valor da sessão é o menor entre o que o D declara em `caps` e o configurado no H. Um valor configurado fora destes limites é `J0734`. **O `HELLO` tem de caber em 512 B** (§6.4), para que o handshake funcione antes de qualquer negociação.

---

## 4. Segurança (L2)

### 4.1 O primeiro byte e as três formas da trama

```
bit   7 6 | 5 4  | 3     | 2 1 0
      fmt | kind | phase | reservados (0; o recetor ignora)
fmt   = 0 na v1 (outro valor: descartar, contador "format", J-E2005)
kind  = 0 em claro · 1 handshake · 2 sessão · 3 reservado (descartar)
phase = fase da chave de sessão (só kind 2; §4.4)
```

| `kind` | Forma | Sobrecarga | Mensagens |
|---|---|---|---|
| 0, em claro | `hdr(1) | mensagem` | 1 B | todas, só com `auth = "none"` |
| 1, handshake | `hdr(1) | kid(8) | nonce(12) | cifra(mensagem) | etiqueta(16)` | 37 B | só `HELLO`, com `auth = "required"` |
| 2, sessão | `hdr(1) | sid(4, LE) | ctr(4, LE) | cifra(mensagem) | etiqueta(16)` | 25 B | todas menos `HELLO`, com `auth = "required"` |

- A cifra é **ChaCha20-Poly1305** (RFC 8439): chave de 32 B, nonce de 12 B, etiqueta de 16 B.
- **AAD** = os bytes em claro antes da cifra: `hdr | kid | nonce` (handshake) ou `hdr | sid | ctr` (sessão).
- **Nonce da sessão** = `00 00 00 00 | ctr como u64 LE` (12 B). **Nonce do handshake** = os 12 B aleatórios da trama.
- Uma trama com `kind` que não corresponde ao modo configurado é descartada (`J-E2104` para uma trama em claro com `auth = "required"`).

### 4.2 Chaves

| Chave | Derivação | Uso |
|---|---|---|
| `K_link` | 32 B aleatórios, gerados no enrolamento do MCU (§4.7) | segredo partilhado D ↔ H; nunca vai no fio, a não ser cifrada no `KEYROT` |
| `kid` | `SHA-256("joker-link/1 kid" ‖ K_link)[0..8]` | diz ao H que `K_link` usar, antes de decifrar |
| `K_hs` | `HKDF-SHA256(salt = vazio, IKM = K_link, info = "joker-link/1 hs", L = 32)` | cifra os `HELLO` |
| `PRK` | `HKDF-Extract(salt = nonce_d ‖ nonce_h, IKM = K_link)` | por sessão |
| `k_d2h` | `HKDF-Expand(PRK, "joker-link/1 d2h" ‖ sid(4, LE), 32)` | tramas D → H |
| `k_h2d` | `HKDF-Expand(PRK, "joker-link/1 h2d" ‖ sid(4, LE), 32)` | tramas H → D |
| `k'` (fase seguinte) | `HKDF-Expand(k, "joker-link/1 ku", 32)` | atualização de chave (§4.4), por sentido |

As strings `info` são ASCII, sem terminador. HKDF: RFC 5869, com SHA-256; "salt vazio" é o salt de 32 zeros do RFC 5869, §2.2.

### 4.3 Handshake

```
  D                                                                   H
  │ HELLO oferta  [kind 1, K_hs, nonce aleatório]  nonce_d (16 B)     │
  │──────────────────────────────────────────────────────────────────▶│ kid → K_link; decifra;
  │                                                                   │ verifica versões, hashes (§7.3)
  │        HELLO resposta  [kind 1, K_hs]  nonce_h, eco nonce_d, sid,  │
  │◀──────────────────────────────────────────────────── modo, params │
  │ confirma o eco; deriva k_d2h, k_h2d                               │ deriva k_d2h, k_h2d
  │ READY  [kind 2, k_d2h, ctr 0]  (classe F)                         │
  │──────────────────────────────────────────────────────────────────▶│ sessão UP: o READY prova
  │                                      ACK  [kind 2, k_h2d, ctr 0]  │ que o D tem K_link
  │◀──────────────────────────────────────────────────────────────────│
  │ sessão UP                                                         │
```

- `nonce_d` e `nonce_h` têm 16 B e são **novos em cada handshake**. O H tem de os tirar de um gerador criptográfico. O D deve usar o TRNG do chip; sem TRNG, tem de usar `nonce_d = HMAC-SHA256(K_link, "joker-link/1 nd" ‖ c)[0..16]`, com `c` um contador de 64 bits (LE) incrementado e persistido em flash **antes** de enviar a oferta, para que nunca se repita. O nonce de 12 B da trama de handshake deriva-se do mesmo modo, com `"joker-link/1 hn"`.
- A frescura do `nonce_d` protege o D: sem ela, um atacante podia repetir uma resposta antiga, levar o D a derivar chaves antigas e repetir tramas de sessão gravadas. A do `nonce_h` protege o H.
- Os nonces de 12 B das tramas de handshake são aleatórios (ou derivados do mesmo contador). Com `K_hs` fixa, a probabilidade de colisão é desprezável ao ritmo dos handshakes (limitado pelo §7.1).
- O H responde no máximo a um `HELLO` por `kid` a cada 100 ms, para limitar o custo de ofertas repetidas por um atacante.
- Sem `auth` (em claro), o handshake é o mesmo com `kind 0`: os nonces vão na mesma, mas não protegem nada.
- **Sem sigilo futuro na v1**: quem obtiver `K_link` e tiver gravado o tráfego decifra-o. A chave `14` do `HELLO` está reservada para uma troca X25519 efémera numa 1.x (§18).

### 4.4 Contador, anti-replay e atualização de chave

**Contador.** Cada sentido tem o seu `ctr`, que começa em 0 em cada fase de chave e sobe 1 a cada trama. Nunca se reutiliza um `ctr` com a mesma chave.

**Janela anti-replay** (por sentido e por fase), no estilo do RFC 6479:

1. Verificar a etiqueta. Falha: descartar, contador `auth` (`J-E2101`).
2. Com `max` = o maior `ctr` aceite: `ctr > max` aceita e desliza a janela; `max − 64 < ctr ≤ max` aceita só se o bit desse `ctr` estiver a zero, e marca-o; `ctr ≤ max − 64` descarta, contador `replay` (`J-E2102`).

Nos transportes com ordem (série, CAN, TCP), uma trama fora da ordem é sempre um sinal de problema; a regra é a mesma.

**Atualização da chave de sessão** (por sentido, sem mensagem própria):

- O emissor troca o bit `phase`, passa a cifrar com `k' = HKDF-Expand(k, "joker-link/1 ku", 32)` e recomeça o `ctr` em 0.
- O recetor, ao ver a fase trocada, deriva `k'` e tenta decifrar. Se a etiqueta bater, instala `k'` e guarda a chave anterior durante 3 × `rto_max` (6 s) para tramas atrasadas da fase anterior.
- Um emissor só pode voltar a atualizar depois de receber a confirmação (§5.3) de uma trama já na fase nova.
- **Quando:** tem de atualizar antes de `ctr` chegar a 2³² − 2¹⁶; deve atualizar a cada 2³¹ tramas ou 24 h, o que vier primeiro. Um `ctr` esgotado sem atualização termina a sessão (`J-E2105`).

### 4.5 Rotação da `K_link`: `KEYROT`

A `K_link` muda num novo enrolamento ou por política. O H gera a nova chave e envia-a num `KEYROT` (§6.8), que só existe em tramas de sessão (cifrado com `k_h2d`). O D guarda-a como **pendente** em memória persistente e responde `KEYROT` guardada. O H envia `BYE(rehandshake)`. O D faz o próximo handshake com a chave pendente; se ele terminar em `READY` confirmado, promove-a e apaga a antiga. Se três handshakes seguidos com a pendente falharem, o D volta à antiga (para não ficar isolado) e o H regista `J-E2106`.

### 4.6 Quando é obrigatória

| Transporte | `auth` |
|---|---|
| UDP, TCP | **tem de** ser `"required"`. `"none"` num transporte IP é `J0179` (ref/01) no `jk check`, e as tramas em claro recebidas são descartadas (`J-E2104`) |
| UART, RS-485, USB CDC, CAN | `"required"` (omissão) ou `"none"`. `"none"` só é aceitável num barramento fechado dentro de um quadro; é o aviso `J0738` em todos os modos, e aparece no `jk doctor` (19 U10) |

Não há negociação de segurança: D (`[link] auth`, compilado na geração) e H (chave `auth` do dispositivo) são configurados; se não coincidirem, não há sessão e os dois lados contam tramas recusadas. Assim não existe ataque de *downgrade*.

### 4.7 Enrolamento e guarda da chave

- `K_link` nasce no enrolamento do MCU: `jk node enroll <mcu> --link <dispositivo>` (documento 07; ref/10 §3.1.3), com o MCU ligado por sonda, por USB ou já em `maintenance` pelo próprio link. A `jk` gera a chave, escreve no MCU os registos **16** (nome do nó) e **17** (chave do link, chaveada pelo *path-hash* do nome do link) da área `JK_AREA_IDENTITY` (ref/08 §2.10.2), ou entrega-a a um elemento seguro (callback `link_key`, ref/08), e guarda-a no nó Linux como o segredo `secret:link.<dispositivo>` (ref/00 §9, ref/02), cifrado para o nó. Um MCU autónomo recebe no mesmo enrolamento a sua chave X25519 de segredos (registo 19, ref/08 §2.10.2).
- A rotação da `K_link` depois do enrolamento é o `KEYROT` (§4.5).
- A `K_link` nunca aparece em logs, na API, no `jk doctor` ou em mensagens de erro. O `kid` pode aparecer.

### 4.8 Vetores de teste

`K_link = 00 01 02 … 1f`, `nonce_d = 10 11 … 1f`, `nonce_h = 20 21 … 2f`, `sid = 1`:

```
kid     47d78759c31e7116
K_hs    773e29c63397f8122ebf0daba10addbe8bb96ca12fee10f037b64b34c476c278
k_d2h   34e79c45b75e7288b5e7c76e92fca2f304f6d063eb6d8b4a5eb02fb2d789738d
k_h2d   79631917c20c7f57af300949270905396b76bbc760d5d2513725029ff751f213
k_d2h'  e6fea0e448d8bd138bdc0ad699ced2ccac9295bdd179224804982065cf3d3948   (fase 1)
```

`PING` do D (mensagem `04 10 0c 00 09 00 ff 00 00 00 a1 01 1a 00 12 ec 2e`, 17 B) cifrado com `k_d2h`, `ctr = 5`:

```
trama L2  20 01 00 00 00 05 00 00 00 cb 54 0e 42 30 6a 12 41 1a 73 f3 9b 4e f2 79 b4 62
          15 78 81 d6 41 4e d6 f7 0d 1d 3e 25 e1 be b1 60                               (42 B)
UART      01 03 20 01 01 01 02 05 01 01 26 cb 54 0e 42 30 6a 12 41 1a 73 f3 9b 4e f2 79
          b4 62 15 78 81 d6 41 4e d6 f7 0d 1d 3e 25 e1 be b1 60 69 4f b2 dc 00          (49 B)
```

`HELLO` pedido do H (mensagem `01 00 00 00 00 00 00 00 00 00 a1 01 02`), handshake, nonce `40 41 … 4b`:

```
trama L2  10 47 d7 87 59 c3 1e 71 16 40 41 42 43 44 45 46 47 48 49 4a 4b 7f 91 30 3c bd
          d9 f5 49 98 3e 0c 16 b4 3e 55 36 a6 db 43 22 04 46 be f4 7b c8 e6 0c a9       (50 B)
```

### 4.9 Custo

ChaCha20-Poly1305 em software num Cortex-M33 a 150 MHz: da ordem de 15–25 ciclos/B (a medir na fase do MCU), ou seja ≈ 50 µs para uma trama de 350 B. HKDF e SHA-256 só no handshake. Não há AES no RP2350, e a ChaCha20 é de tempo constante sem hardware: é por isso a escolha do documento 19.

---

## 5. Sessão e cabeçalho comum (L3)

### 5.1 Cabeçalho

Cada mensagem começa por um cabeçalho binário de 10 B, ou 12 B com `REL`:

| Offset | Campo | Tipo | Significado |
|---|---|---|---|
| 0 | `type` | u8 | tipo da mensagem (§6.1) |
| 1 | `flags` | u8 | §5.2 |
| 2 | `seq` | u16 LE | número do pacote, por sentido |
| 4 | `ack` | u16 LE | maior `seq` recebido do outro lado (válido só com `AV`) |
| 6 | `ack_bits` | u32 LE | bit *i* = 1: o pacote `ack − 1 − i` foi recebido |
| 10 | `rseq` | u16 LE | só com `REL`: número da mensagem fiável (§5.4) |

O corpo vem a seguir: **um** item CBOR (sempre um mapa com chaves inteiras), ou nada (`ACK`). Bytes a mais depois do item são um erro de mensagem (`J-E2006`).

O cabeçalho é binário, e não CBOR, porque o recetor tem de confirmar, ordenar e encaminhar pacotes sem descodificar o corpo, e porque 10 B fixos custam menos a analisar num MCU do que um mapa.

### 5.2 Flags

| Bit | Nome | Significado |
|---|---|---|
| 0 | `REL` | mensagem fiável; o campo `rseq` está presente |
| 1 | `MORE` | fragmento com continuação (§5.5) |
| 2 | `EOT` | fim da vez em half-duplex (§3.3) |
| 3 | `ACKNOW` | pede um `ACK` imediato, sem esperar `ack_delay` |
| 4 | `AV` | `ack` e `ack_bits` são válidos |
| 5–7 | — | reservados: o emissor põe 0, o recetor ignora |

Os `HELLO` levam `seq = ack = ack_bits = 0` e `flags = 0`, e não entram na numeração. O primeiro pacote de cada sentido depois do handshake tem `seq = 0` (o `READY` no D). A comparação de `seq` e `rseq` é a aritmética de números de série de 16 bits (RFC 1982).

### 5.3 Confirmação

- **Aceitar.** Um pacote conta como recebido só depois de passar o L1, o L2 (etiqueta e janela), ter um cabeçalho válido e, se for `REL`, de a mensagem ter sido entregue ou guardada na janela. Um pacote que o recetor não guardou (janela cheia, sem memória) **não** é confirmado.
- **Confirmar.** Todos os pacotes levam `AV` e o estado de receção atual (`ack`, `ack_bits`) depois do handshake. Os pacotes das classes F e S (§5.4) pedem confirmação: se o recetor não tiver nada para enviar em `ack_delay`, envia um `ACK`. Os pacotes das classes E e os `ACK` não a pedem, mas são confirmados de boleia. `ACKNOW` força um `ACK` imediato.
- **Dar como perdido.** O emissor dá um pacote como perdido quando não foi confirmado e (a) três pacotes enviados depois dele já foram confirmados, ou (b) passou `RTO` desde que foi enviado.
- **RTT e RTO.** Cada confirmação de um pacote dá uma amostra de RTT (uma retransmissão é sempre um pacote novo, com `seq` novo, por isso não há a ambiguidade de Karn). `RTO = max(rto_min, SRTT + 4·RTTVAR)`, limitado a `rto_max` = 2 s, com o SRTT e o RTTVAR do RFC 6298. RTO inicial = 10 × `rto_min`.

### 5.4 Classes de entrega

| Classe | Mensagens | Se o pacote se perde |
|---|---|---|
| **E**, efémera | `BYE`, `PING`, `PONG`, `ERROR`, `PROPOSE`, `TIME`, `TRACE` dados, `OTA` dados e `ack` | nada; a mensagem seguinte substitui-a, ou o pedido repete-se |
| **S**, estado | `SAMPLES` | o emissor volta a marcar como sujos os sinais desse pacote e envia os **valores atuais** (nunca os antigos) |
| **F**, fiável | todas as outras | a mensagem é reenviada num pacote novo, com o mesmo `rseq`, até `retransmit` vezes |

**Classe F.**

- `rseq` começa em 0 em cada sentido de cada sessão e sobe 1 por mensagem (ou fragmento).
- No máximo `window` mensagens F por confirmar em cada sentido. O recetor entrega-as por ordem de `rseq`; guarda as que chegam fora de ordem, dentro da janela; descarta sem confirmar as que estão fora dela; e descarta (confirmando) as repetidas.
- Esgotados os `retransmit` reenvios de uma mensagem, a sessão termina: `BYE(error)`, `J-E2009`, estado `LOST` (§7).

**Classe S.** O D guarda, para os últimos 8 pacotes `SAMPLES` por confirmar, o conjunto dos `sid` que levavam. Ao dar um pacote como perdido, marca esses sinais como sujos, a não ser que já tenham seguido num pacote posterior. Se a tabela encher, o pacote mais antigo conta como perdido.

**Mensagens velhas.** Em UDP os pacotes podem chegar trocados. O D descarta um `PROPOSE` com `seq` anterior ao do último `PROPOSE` que já consumiu. O H descarta uma amostra cuja `seq` de 16 bits seja anterior à última aceite desse sinal.

### 5.5 Fragmentação

Só as mensagens F se fragmentam. Uma mensagem que não cabe numa trama é partida em fragmentos com `rseq` consecutivos; todos menos o último levam `MORE`, e todos têm o mesmo `type`. O corpo de cada fragmento é uma fatia dos bytes CBOR da mensagem inteira (não é um item CBOR); o recetor concatena-os por ordem de `rseq`. Uma mensagem remontada acima de `max_msg` é descartada (`J-E2008`, `ERROR` com estado 10). As mensagens E e S têm de caber numa trama: o emissor parte-as em várias mensagens (vários `SAMPLES`), e nunca parte um bloco de escritor de `PROPOSE` (§6.12).

`max_msg`: mínimo 2048, omissão 4096, máximo 65 535.

### 5.6 Parâmetros da sessão e temporizadores

O H escolhe-os no `HELLO` de resposta, dentro das capacidades do D. Omissões:

| Parâmetro | Série, USB, CAN | UDP, TCP | Significado |
|---|---|---|---|
| `max_frame` | 1024 | 1024 | §3.7 |
| `max_msg` | 4096 | 4096 | §5.5 |
| `window` | 4 | 8 | mensagens F por confirmar (1–16) |
| `keepalive` | 50 ms | 1 s | sem nada enviado durante este tempo, envia-se um `PING` |
| `link_timeout` | 200 ms | 5 s | sem nenhum pacote válido recebido durante este tempo, o link está perdido (§7) |
| `report` | 10 ms | 10 ms | intervalo mínimo entre `SAMPLES` com alterações |
| `refresh` | 1 s | 5 s | período de reenvio completo dos sinais (§6.9); 0 desliga |
| `max_lease` | 5 s | 5 s | teto do lease de um escritor remoto (§8.1) |
| `rto_min` | 10 ms | 100 ms | §5.3 |
| `retransmit` | 8 | 8 | reenvios de uma mensagem F antes de desistir (não é a chave comum `retries` dos dispositivos, que conta repetições de pedidos do núcleo, ref/01 §5.8) |
| `ack_delay` | 2 ms | 20 ms | §5.3 |
| `time_interval` | 10 s | 10 s | período das sondas `TIME` (§8.4); 0 desliga |
| `bulk_max` | 256 | 1024 | carga útil máxima de um bloco de `OTA`, `TRACE` ou `CATALOG` (§5.7) |
| `turn_frames` | 4 | — | só half-duplex (§3.3) |

`link_timeout` tem de ser ≥ 3 × `keepalive`.

### 5.7 Prioridade no envio

Cada lado envia pela ordem seguinte, e uma classe só avança quando as de cima não têm nada pronto:

1. `HELLO`, `READY`, `BYE`, `ERROR`, `PONG`, `ACK` devido;
2. `PROPOSE` (H), `ALARMS` (D);
3. `RESULT`, `OUTCOMES`, `CALL`, `ACTION`, `CANCEL`, `ALARM_CMD`, `RESET`, `RESTART`, `TIME`, `KEYROT`, `PING`;
4. `SAMPLES`, `READ`;
5. `EVENTS`;
6. `CATALOG`, `TRACE`, `OTA` (bulk), com blocos de no máximo `bulk_max` B.

O `bulk_max` limita o tempo que um bloco já no fio pode atrasar um `PROPOSE` ou um `SAMPLES` (é o termo `t_bulk` da condição de vivacidade, §11.6). No CAN, o bulk vai no seu próprio identificador e não bloqueia o controlo (§3.4).

---

## 6. Mensagens

### 6.1 Tabela geral

| Tipo | Nome | Sentido | Classe | Modos (§7.3) | Resposta | Papel mínimo na origem | Versão |
|---|---|---|---|---|---|---|---|
| `0x00` | — | — | — | — | — | — | nunca usado |
| `0x01` | `HELLO` | ↔ | handshake | — | `HELLO` | — | 1.0 |
| `0x02` | `READY` | D → H | F | todos | — | — | 1.0 |
| `0x03` | `BYE` | ↔ | E | todos | — | — | 1.0 |
| `0x04` | `PING` | ↔ | E | todos | `PONG` | — | 1.0 |
| `0x05` | `PONG` | ↔ | E | todos | — | — | 1.0 |
| `0x06` | `ACK` | ↔ | — | todos | — | — | 1.0 |
| `0x07` | `ERROR` | ↔ | E | todos | — | — | 1.0 |
| `0x08` | `KEYROT` | ↔ | F | todos, só `kind 2` | `KEYROT` | `admin` | 1.0 |
| `0x10` | `CATALOG` | ↔ | F | todos | `CATALOG` | `viewer` | 1.0 |
| `0x11` | `READ` | H → D | F | full, observe | `SAMPLES` | `viewer` | 1.0 |
| `0x12` | `SAMPLES` | D → H | S | full, observe | — | — | 1.0 |
| `0x13` | `PROPOSE` | H → D | E | full | `OUTCOMES` | o do escritor (manifesto do nó) | 1.0 |
| `0x14` | `OUTCOMES` | D → H | F | full | — | — | 1.0 |
| `0x15` | `CALL` | H → D | F | full | `RESULT` | o do escritor; `operator` em manual | 1.0 |
| `0x16` | `ACTION` | H → D | F | full | `RESULT` | `role` da ação (manifesto do MCU) | 1.0 |
| `0x17` | `RESULT` | D → H | F | todos | — | — | 1.0 |
| `0x18` | `CANCEL` | H → D | F | full | `RESULT` | o da ação | 1.0 |
| `0x19` | `EVENTS` | D → H | F | full, observe | — | — | 1.0 |
| `0x1A` | `ALARMS` | D → H | F | full, observe | — | — | 1.0 |
| `0x1B` | `ALARM_CMD` | H → D | F | full | `RESULT` | `operator` | 1.0 |
| `0x1C` | `RESET` | H → D | F | full | `RESULT` | `operator` | 1.0 |
| `0x1D` | `RESTART` | H → D | F | todos | `RESULT`, depois `BYE` | `admin` | 1.0 |
| `0x20` | `TIME` | ↔ | E | todos | `TIME` | — | 1.0 |
| `0x21` | `TRACE` | ↔ | F (dados: E) | todos | `TRACE` | `viewer`; `operator` para congelar e libertar | 1.0 |
| `0x22` | `OTA` | ↔ | F (dados e `ack`: E) | todos | `OTA` | `admin` | 1.0 |
| `0x30` | `BLOCKS` | D → H | — | — | — | — | **reservado 1.1** (sinais em bloco, M7) |
| `0x31`–`0x3F` | — | — | — | — | — | — | reservados 1.x |
| `0x40`–`0xDF` | — | — | — | — | — | — | reservados |
| `0xE0`–`0xFF` | privados | — | — | — | — | — | nunca normalizados; um par normal ignora-os |

- **Tipo desconhecido:** o recetor ignora a mensagem; se tiver `REL`, confirma-a (para não bloquear a janela) e responde `ERROR` com estado 2 (`J-E2007`), uma vez por tipo e por sessão.
- **Mensagem fora do modo** (coluna "Modos"): `ERROR` com estado 5 (`J-E2208`).
- **Papel.** O papel é verificado no nó Linux, que conhece o utilizador (ref/09, ref/00 §9). As mensagens com `role` levam o papel verificado, e o D volta a compará-lo com o que a sua geração exige (defesa em profundidade): um papel insuficiente dá `RESULT` com estado 12 (`J-E2313`).

### 6.2 Codificação comum

**Emissão:** CBOR determinístico (RFC 8949, §4.2.1): mapas com chaves por ordem crescente de bytes, inteiros e comprimentos na forma mais curta, `float` na forma mais curta que representa o valor **exatamente** (meia, simples ou dupla precisão; `NaN` = `f9 7e 00`), sem comprimentos indefinidos, sem etiquetas além das indicadas. **Leitura:** qualquer CBOR bem formado; chaves desconhecidas ignoradas; `float` de qualquer largura; um valor de enum desconhecido conta como desconhecido (ref/00 §1). As chaves ausentes têm a omissão da tabela da mensagem.

**Valores** (o tipo do sinal vem do catálogo, §6.3):

| Tipo do sinal (ref/00 §6) | CBOR | Nota |
|---|---|---|
| `bool` | `true` / `false` | |
| `i64` | inteiro (maior tipo 0 ou 1) | |
| `u64` | inteiro sem sinal | |
| `f64` | `float` | a forma mais curta exata: `412.5` → `f9 5e 72` (3 B) |
| `f32` | `float` | idem; nunca precisa de dupla precisão |
| `enum<N>` | inteiro sem sinal | o valor numérico da variante |
| `bits<N>` | inteiro sem sinal | a palavra |
| `string` | `tstr` ≤ 64 B | |
| `bytes` | `bstr` ≤ 64 B | |

Um valor com o tipo CBOR errado para o sinal é `Rejected(WrongType)` numa proposta, e é ignorado com o contador `malformed` numa amostra.

**Tempo** (ref/00 §3): instantes do MCU em µs monotónicos desde o arranque do MCU (`mono-us`, u64); UTC em µs (`utc-us`, s64), só depois de um `TIME` (§8.4); durações em ms (`dur-ms`, u32) ou em µs quando o nome o diz. Dentro de um lote, os instantes são `t0` (absoluto) mais um `dt` em µs (u32).

**Qualidade** (ref/00 §4.3): u32 com o código nos bits 0–7, o motivo nos bits 8–15 e as flags nos bits 16–31, como no slot (01 §3.2). `Uncertain(Stale)` = `1 | 3 << 8` = 769. Os motivos privados 240–255 saem do MCU como 0.

**Registos** (argumentos, resultados, campos de eventos): um mapa CBOR determinístico com **chaves inteiras**, a posição do campo no esquema (o contrato `.jkc` da classe, ou o `joker:meta` da app), a contar de 0 (D2, ref/00 §4.1, ref/04 §4.10). É a mesma forma que atravessa o WIT, o JDP e a ABI C; o link transporta-a sem a reescrever, e só as interfaces de texto mostram os nomes. Limites da ref/00 §10: strings ≤ 1 KiB, listas ≤ 64 elementos.

**Identificadores:** `sid` (u32) é o `SignalId` na geração **do MCU**; `iid`, `mid`, `aid`, `eid`, `alid`, `gid` são índices do catálogo (§6.3). Todos valem só com o hash do catálogo da sessão (ref/00 §5). O `origin` é o identificador do escritor na geração **do nó Linux** (u32), e é só informativo para o MCU (§8.1).

### 6.3 O catálogo

O catálogo é tudo o que o nó Linux precisa para gerar o perfil do MCU (19 §6): sinais, instâncias, métodos, ações, eventos, alarmes e grupos. É um item CBOR determinístico, `catalog` no CDDL, gerado pelo `jk build --target mcu:…` e guardado **tal e qual** na geração do MCU (secção `CATALOG` do blob, ref/08 §2.13.3). O **hash do catálogo** é o SHA-256 desses bytes; vai no `HELLO` (chave 8) e é o `digest` do `[discovered.<d>]` para um dispositivo `joker-link` (ref/02).

| Chave | Conteúdo | Campos (chave: significado) |
|---|---|---|
| 1 | sinais | 1 `sid`; 2 caminho no MCU; 3 tipo (ref/00 §6); 4 unidade (`"1"` = adimensional); 5 direção (`input` 0, `output` 1, `internal` 2, `param` 3, `derived` 4); 6 `gid` do grupo dono; 7 variantes do enum ou nomes dos bits `[[valor, nome]]`; 8 níveis delegados ao link (saídas: bit *L−1* = nível *L*); 9 `range` e 10 `safe` (informativos, para o `jk write` mostrar o envelope); 11 `max_age` em ms; 12 grandeza; 13 largura *N* de `enum<N>`/`bits<N>` |
| 2 | instâncias | 1 `iid`; 2 nome; 3 classe (`"std/pump@1"`); 4 versão completa; 5 hash do contrato; 6 capacidades; 7 membros `[[nome, sid]]`; 8 membros de classe `[[nome, [iid…]]]` (um `iid` num membro fixo, a lista pela ordem da coleção); 9 só membro (`true`: a instância não tem nome de topo no nó) |
| 3 | métodos | 1 `mid`; 2 caminho (`"p1.run_at"`); 3 `iid`; 4 `atomic` |
| 4 | ações | 1 `aid`; 2 caminho (`"p1.reset"`, `"pcs.reset_fault"`); 3 `iid` (ausente numa ação de dispositivo); 4 papel exigido; 5 timeout em ms; 6 `@long` |
| 5 | eventos | 1 `eid`; 2 caminho; 3 `iid`; 4 campos `[[nome, tipo, unidade]]` |
| 6 | alarmes | 1 `alid`; 2 nome; 3 severidade (ref/00 §6); 4 mensagem; 5 `sid` do sinal `alarm.<nome>.state` |
| 7 | grupos | 1 `gid`; 2 nome; 3 período em µs |
| 8 | nome do MCU | o mesmo da identidade do `HELLO` |

- Os índices (`sid`, `iid`, `mid`, `aid`, `eid`, `alid`, `gid`) são densos, começam em 0 e seguem a ordem do registo do MCU (caminho canónico, 01 §3.1). Só aparecem os sinais que o MCU exporta; os internos das apps (`#[internal]`) não saem.
- **Instâncias compostas** (documento 17, §8). Uma instância com membros de classe leva a árvore na chave 2.8, com os `iid` das instâncias ligadas; cada uma delas está também na lista 2, com os seus sinais. Uma instância que no MCU é membro de outra leva 2.9 = `true`. Um submembro sem nome (ref/03 §2.2.14) tem como nome o caminho aninhado (`hp.compressor`). As listas de métodos (3), ações (4) e eventos (5) têm os caminhos de topo de cada um.
- Os sinais `joker.*` do MCU (métricas, 01 §19) estão no catálogo como os outros. O conjunto mínimo é `joker.link.rx_frames`, `joker.link.crc_errors`, `joker.link.auth_errors`, `joker.link.retransmits`, `joker.micro.boot_count` e `joker.group.<g>.overruns`.
- O catálogo transfere-se por `CATALOG` (§6.11), em pedaços, e o H guarda-o em cache pelo hash. Um H que já o tem (cache, ou porque a geração do nó o traz do `jk build`) não o pede.

### 6.4 `HELLO` (`0x01`)

Três fases no mesmo tipo (chave 1). Sempre em tramas de handshake (`kind 1`) ou em claro (`kind 0`); nunca fragmentado; tem de caber em 512 B de trama L2.

**Oferta** (fase 0, D → H):

| Chave | Campo | Tipo | Obr. | Significado |
|---|---|---|---|---|
| 1 | fase | 0 | sim | |
| 2 | versões | `[+ [major, max_minor]]` | sim | as majors que o D fala e a minor mais alta de cada (§13) |
| 3 | `nonce_d` | `bstr` 16 | sim | §4.3 |
| 4 | identidade | mapa | sim | 1 nome do MCU (rótulo DNS); 2 id de hardware (4–32 B); 3 placa |
| 5 | firmware | mapa | sim | 1 versão; 2 digest da imagem (SHA-256); 3 alvo; 4 `JK_ABI_VERSION` `[maj, min]`; 5 hash das capacidades (ref/08 §2.14); 6 versão do Joker Micro; 7 features compiladas; 8 capacidades: mapa `cap_id` → valor com a numeração e os nomes da ref/08 §2.13.3, com pelo menos `signals` (1), `outputs` (3), `groups` (4), `instances` (16), `trace_bytes` (36) e `kv_bytes` (37); 9 digest do descritor de firmware (SHA-256 do `firmware.jkfw`, ref/08 §2.14); 10 `security_version` confirmado (anti-rollback, §9.5) |
| 6 | geração | mapa | não | 1 `gen_seq` do blob; 2 hash da geração (o `source_hash` do blob, ref/08 §2.13.2); 3 estado (ref/00 §6 `generation_state`); 4 origem (`embedded` 0, `flash` 1). Ausente: o D não tem geração válida |
| 7 | hash do registo | `bstr` 32 | com 6 | o `registry_hash` do blob, o do traço (16 §7) |
| 8 | hash do catálogo | `bstr` 32 | com 6 | §6.3 |
| 9 | contratos | `[* [classe, versão, hash]]` | não | as classes que o MCU exporta (19 §6, U7) |
| 10 | truncado | bool | não, `false` | `true`: a lista 9 não coube; o H lê-a do catálogo |
| 11 | capacidades do link | mapa | sim | 1 `max_frame`; 2 `max_msg`; 3 `window`; 4 features (§6.4.1); 5 capacidade da deduplicação; 6 capacidade de escritores remotos; 7 bytes do anel de traço |
| 12 | arranque | mapa | sim | 1 contador de arranques; 2 motivo do último reset; 3 slot de firmware ativo; 4 firmware em experiência; 5 resultado da última atualização de firmware; 6 slot de geração; 7 geração em experiência; 8 ms que faltam na janela de confirmação |
| 13 | agora | `mono-us` | sim | relógio do MCU no envio |
| 14 | — | `bstr` 32 | não | **reservado 1.x** (X25519 efémera) |
| 15 | drivers | `bstr` 32 | com drivers planeados | SHA-256 do CBOR determinístico de `[* [nome, major, digest do descritor]]`, por nome: os drivers do MCU que o `jk build` planeou, com o digest do descritor com que planeou (ref/06, ref/08 §2.13.3 `DEVICES`) |
| 16 | parâmetros alterados | `[* [path-hash, v]]` | não | até 8 parâmetros de apps substituídos localmente (`jk_param_set`, ref/08 §2.6.l): deriva face ao Git |
| 17 | parâmetros truncados | bool | não, `false` | `true`: há mais do que 8; o H lê o resto por `READ` dos sinais `param` |

Motivos de reset: `unknown` 0, `power_on` 1, `pin` 2, `watchdog` 3, `software` 4, `brownout` 5, `ota` 6, `fault` 7.

#### 6.4.1 Features (chave 11.4)

| Bit | Feature |
|---|---|
| 0 | tramas cifradas (`kind 1` e `2`) |
| 1 | tramas em claro |
| 2 | half-duplex |
| 3 | troca de geração a quente (`jk_generation_swap`) |
| 4 | `TRACE` |
| 5 | `OTA` de geração |
| 6 | `OTA` de firmware |
| 7 | `ACTION` |
| 8 | ações longas (`@long`) |
| 9 | `TIME` |
| 10 | `KEYROT` |
| 11–31 | reservados (0) |

Um H não envia uma mensagem cuja feature o D não declara (`ERROR` estado 5 se o fizer).

**Resposta** (fase 1, H → D):

| Chave | Campo | Tipo | Obr. | Significado |
|---|---|---|---|---|
| 1 | fase | 1 | sim | |
| 2 | versão | `[major, minor]` | sim | a escolhida (§13) |
| 3 | `nonce_h` | `bstr` 16 | sim | |
| 4 | eco | `bstr` 16 | sim | o `nonce_d` da oferta; o D descarta a resposta se não bater |
| 5 | modo | uint | sim | `full` 0, `observe` 1, `maintenance` 2, `refused` 3 (§7.3) |
| 6 | `sid` | u32 ≠ 0 | se não `refused` | escolhido pelo H, aleatório, único entre as suas sessões |
| 7 | parâmetros | mapa | se não `refused` | 1 `max_frame`; 2 `max_msg`; 3 `window`; 4 `keepalive`; 5 `link_timeout`; 6 `report`; 7 `refresh`; 8 `max_lease`; 9 `rto_min`; 10 `retransmit`; 11 `ack_delay`; 12 `time_interval`; 13 `bulk_max`; 14 `turn_frames` (§5.6; ms) |
| 8 | estado | uint | não | porque não é `full` (§10.1) |
| 9 | código | `tstr` | não | `J-E22xx` |
| 10 | detalhe | `tstr` | não | |

**Pedido** (fase 2, H → D): `{1: 2}`. Pede ao D que recomece o handshake (por exemplo, o `jokerd` reiniciou e perdeu a sessão). O D só lhe obedece se não tiver recebido nenhum pacote de sessão válido nos últimos 2 × `keepalive`; assim, um pedido repetido por um atacante não derruba uma sessão viva.

Exemplo de oferta (MCU `io-rp2350-01`, geração 7, uma classe):

```cbor-diag
{
  1: 0,
  2: [[1, 0]],
  3: h'101112131415161718191a1b1c1d1e1f',
  4: {1: "io-rp2350-01", 2: h'e66038b7c3a2f12d', 3: "acme/io16-rp2350"},
  5: {1: "1.0.3", 2: h'e5c2ed6fe0a694040f93b1e0bfaa1b140df0e51de29f6903f76ac093089ca470', 3: "acme/io16-rp2350", 6: "1.0.3", 7: ["gpio", "modbus-rtu"]},
  6: {1: 7, 2: h'69e2071fe2dbff9a0f8a270b5d88c4b886a9b048d61f21ec6793faef6adbbd85', 3: 2, 4: 1},
  7: h'9be86532d0f649500bb9e54961364246fc50c2ab410232ef8a8b5c2dd52b5976',
  8: h'25c7c73ece719aafc154ea525c9d0616cf4499a4297d1f703cfa06e10bbb3d63',
  9: [["std/relay@1", "1.0.0", h'36df9772a6e35a5decb09ff8bbc1a1aab2b9266c65c2bebb638de85d2ef6514a']],
  11: {1: 1024, 2: 4096, 3: 4, 4: 1023, 5: 16, 6: 8, 7: 16384},
  12: {1: 42, 2: 1, 3: 0, 4: false, 5: 2},
  13: 1234567
}
```

```
cabeçalho  01 00 00 00 00 00 00 00 00 00
corpo      ac 01 00 02 81 82 01 00 03 50 10 11 12 13 14 15 16 17 18 19 1a 1b 1c 1d
           1e 1f 04 a3 01 6c 69 6f 2d 72 70 32 33 35 30 2d 30 31 02 48 e6 60 38 b7
           c3 a2 f1 2d 03 70 61 63 6d 65 2f 69 6f 31 36 2d 72 70 32 33 35 30 05 a5
           01 65 31 2e 30 2e 33 02 58 20 e5 c2 ed 6f e0 a6 94 04 0f 93 b1 e0 bf aa
           1b 14 0d f0 e5 1d e2 9f 69 03 f7 6a c0 93 08 9c a4 70 03 70 61 63 6d 65
           2f 69 6f 31 36 2d 72 70 32 33 35 30 06 65 31 2e 30 2e 33 07 82 64 67 70
           69 6f 6a 6d 6f 64 62 75 73 2d 72 74 75 06 a4 01 07 02 58 20 69 e2 07 1f
           e2 db ff 9a 0f 8a 27 0b 5d 88 c4 b8 86 a9 b0 48 d6 1f 21 ec 67 93 fa ef
           6a db bd 85 03 02 04 01 07 58 20 9b e8 65 32 d0 f6 49 50 0b b9 e5 49 61
           36 42 46 fc 50 c2 ab 41 02 32 ef 8a 8b 5c 2d d5 2b 59 76 08 58 20 25 c7
           c7 3e ce 71 9a af c1 54 ea 52 5c 9d 06 16 cf 44 99 a4 29 7d 1f 70 3c fa
           06 e1 0b bb 3d 63 09 81 83 6b 73 74 64 2f 72 65 6c 61 79 40 31 65 31 2e
           30 2e 30 58 20 36 df 97 72 a6 e3 5a 5d ec b0 9f f8 bb c1 a1 aa b2 b9 26
           6c 65 c2 be bb 63 8d e8 5d 2e f6 51 4a 0b a7 01 19 04 00 02 19 10 00 03
           04 04 19 03 ff 05 10 06 08 07 19 40 00 0c a5 01 18 2a 02 01 03 00 04 f4
           05 02 0d 1a 00 12 d6 87
```

(10 B de cabeçalho + 368 B de corpo = 378 B de mensagem.)

Resposta, modo `full`, `sid = 1`:

```cbor-diag
{
  1: 1,
  2: [1, 0],
  3: h'202122232425262728292a2b2c2d2e2f',
  4: h'101112131415161718191a1b1c1d1e1f',
  5: 0,
  6: 1,
  7: {1: 1024, 2: 4096, 3: 4, 4: 50, 5: 200, 6: 10, 7: 1000, 8: 5000, 9: 10, 10: 8, 11: 2, 12: 10000, 13: 256}
}
```

```
cabeçalho  01 00 00 00 00 00 00 00 00 00
corpo      a7 01 01 02 82 01 00 03 50 20 21 22 23 24 25 26 27 28 29 2a 2b 2c 2d 2e
           2f 04 50 10 11 12 13 14 15 16 17 18 19 1a 1b 1c 1d 1e 1f 05 00 06 01 07
           ad 01 19 04 00 02 19 10 00 03 04 04 18 32 05 18 c8 06 0a 07 19 03 e8 08
           19 13 88 09 0a 0a 08 0b 02 0c 19 27 10 0d 19 01 00
```

(10 B de cabeçalho + 89 B de corpo = 99 B de mensagem.)

Pedido:

```cbor-diag
{1: 2}
```

```
cabeçalho  01 00 00 00 00 00 00 00 00 00
corpo      a1 01 02
```

(10 B de cabeçalho + 3 B de corpo = 13 B de mensagem.)

### 6.5 `READY` (`0x02`)

D → H, classe F, `rseq = 0`, a primeira trama de sessão. Confirma o modo e prova a posse das chaves. `{1: modo (eco), 2: agora (mono-us)}`. Depois do `ACK` do H, o D envia um `SAMPLES` completo (`refresh`) e um `ALARMS` com `snapshot`.

```cbor-diag
{1: 0, 2: 1240110}
```

```
cabeçalho  02 01 00 00 00 00 00 00 00 00 00 00
corpo      a2 01 00 02 1a 00 12 ec 2e
```

(12 B de cabeçalho + 9 B de corpo = 21 B de mensagem.)

### 6.6 `BYE` (`0x03`)

↔, classe E. Fecha a sessão. `{1: motivo, ? 2: código, ? 3: detalhe}`. Motivos: `shutdown` 0, `rehandshake` 1, `generation_change` 2, `reboot` 3, `ota_activate` 4, `error` 5. O recetor apaga as chaves da sessão; o D volta a `IDLE` (§7.1). Os leases remotos **não** são largados por um `BYE` (§7.4).

```cbor-diag
{1: 2}
```

```
cabeçalho  03 10 8f 03 4d 00 ff ff ff ff
corpo      a1 01 02
```

(10 B de cabeçalho + 3 B de corpo = 13 B de mensagem.)

### 6.7 `PING` (`0x04`), `PONG` (`0x05`), `ACK` (`0x06`)

- `PING` {1: t (µs, relógio monotónico de quem envia)}: enviado quando nada foi enviado durante `keepalive`. Classe E.
- `PONG` {1: eco de t, 2: t de quem responde}: resposta imediata a um `PING`. Dá um RTT a quem pingou.
- `ACK`: corpo vazio (0 B); só o cabeçalho conta. Nunca pede confirmação.

```cbor-diag
{1: 1240110}
```

```
cabeçalho  04 10 0c 00 09 00 ff 00 00 00
corpo      a1 01 1a 00 12 ec 2e
```

(10 B de cabeçalho + 7 B de corpo = 17 B de mensagem.)

```cbor-diag
{1: 1240110, 2: 88012345}
```

```
cabeçalho  05 10 0a 00 0c 00 ff ff 00 00
corpo      a2 01 1a 00 12 ec 2e 02 1a 05 3e f6 39
```

(10 B de cabeçalho + 13 B de corpo = 23 B de mensagem.)

```cbor-diag
(corpo vazio)
```

```
cabeçalho  06 10 0d 00 0b 00 ff ff ff ff
corpo      (vazio)
```

(10 B de cabeçalho + 0 B de corpo = 10 B de mensagem.)

### 6.8 `KEYROT` (`0x08`)

Só em tramas de sessão (§4.5). Classe F.

| Fase (chave 1) | Sentido | Campos |
|---|---|---|
| 0, oferta | H → D | 2 `kid` novo (8 B); 3 `K_link` nova (32 B) |
| 1, guardada | D → H | 2 `kid` guardado |

```cbor-diag
{
  1: 0,
  2: h'e1092152bcfa5913',
  3: h'202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f'
}
```

```
cabeçalho  08 11 32 00 3c 00 00 00 00 00 05 00
corpo      a3 01 00 02 48 e1 09 21 52 bc fa 59 13 03 58 20 20 21 22 23 24 25 26 27
           28 29 2a 2b 2c 2d 2e 2f 30 31 32 33 34 35 36 37 38 39 3a 3b 3c 3d 3e 3f
```

(12 B de cabeçalho + 48 B de corpo = 60 B de mensagem.)

```cbor-diag
{1: 1, 2: h'e1092152bcfa5913'}
```

```
cabeçalho  08 11 3d 00 32 00 00 00 00 00 09 00
corpo      a2 01 01 02 48 e1 09 21 52 bc fa 59 13
```

(12 B de cabeçalho + 13 B de corpo = 25 B de mensagem.)

### 6.9 `SAMPLES` (`0x12`)

D → H, classe S. As amostras dos sinais exportados.

| Chave | Campo | Tipo | Obr. | Significado |
|---|---|---|---|---|
| 1 | `t0` | `mono-us` | sim | base de tempo do lote: o instante mais antigo do lote |
| 2 | amostras | `[* sample]` | sim | cada `sid` no máximo uma vez por mensagem |
| 3 | flags | uint | não, 0 | bit 0: leva entradas de `refresh`; bit 1: resposta a `READ`; bit 2: última mensagem da resposta a `READ` |
| 4 | pedido | u16 | com bit 1 | o `rseq` do `READ` |
| 5 | coalescidas | u32 | não, 0 | amostras do MCU que não seguiram porque outra mais nova do mesmo sinal as substituiu antes do envio |

`sample = [sid, seq, v, ? dt, ? q, ? ts_src]`:

| Posição | Campo | Tipo | Omissão | Significado |
|---|---|---|---|---|
| 0 | `sid` | u32 | — | |
| 1 | `seq` | u16 | — | os 16 bits baixos da `seq` da amostra no MCU |
| 2 | `v` | valor | — | §6.2 |
| 3 | `dt` | u32 µs | 0 | instante da amostra = `t0 + dt`: o `ts_hw` do MCU se existir, senão o seu `ts_acq` |
| 4 | `q` | u32 | 0 | qualidade (§6.2) |
| 5 | `ts_src` | `utc-us` ou `null` | ausente | UTC da origem (um equipamento atrás do MCU que dá carimbo) |

As posições em falta no fim têm a omissão; para dar `q` sem `dt`, escreve-se `dt = 0`. Posições a mais são ignoradas.

**Quando o D envia:**

1. **Alterações:** um sinal fica sujo quando a sua `seq` avança no MCU. Com sinais sujos e pelo menos `report` desde o último `SAMPLES`, o D envia-os (vários `SAMPLES` se não couberem numa trama).
2. **Refresh:** cada sinal exportado segue pelo menos uma vez a cada `refresh`, mesmo sem mudar, repartido ao longo do período (flag bit 0).
3. **Depois do `READY`:** todos os sinais (flag bit 0).
4. **Resposta a `READ`** (flag bits 1 e 2, chave 4).
5. **Perda:** o §5.4, classe S.

O `refresh` é o que mantém fresco no nó Linux um sinal que não muda: para o `jokerd`, cada entrada é uma amostra nova, como um poll (01 §3.1). Por isso o `max_age` por omissão dos sinais de um dispositivo `joker-link` é 3 × `refresh` (`J0739` se for menor do que 2 × `refresh`).

Exemplo: duas entradas digitais e uma potência com `Uncertain(Stale)`, 1,8 ms depois de `t0`:

```cbor-diag
{
  1: 1240000,
  2: [[0, 1201, true], [1, 877, false, 250], [40, 5120, 412.5, 1800, 769]]
}
```

```
cabeçalho  12 10 2c 03 92 01 ff ff ff ff
corpo      a2 01 1a 00 12 eb c0 02 83 83 00 19 04 b1 f5 84 01 19 03 6d f4 18 fa 85
           18 28 19 14 00 f9 5e 72 19 07 08 19 03 01
```

(10 B de cabeçalho + 38 B de corpo = 48 B de mensagem.)

### 6.10 `READ` (`0x11`)

H → D, classe F. Pede o valor atual de sinais, mudados ou não (por exemplo `jk read --refresh` de um `param`, 02 §8; ou uma ressincronização). `{? 1: [* sid] (ausente: todos), ? 2: bool (também um ALARMS snapshot)}`. Resposta: um ou mais `SAMPLES` com a flag bit 1 e a chave 4; o último com o bit 2. Um `sid` desconhecido dá `ERROR` estado 6.

```cbor-diag
{1: [40, 41]}
```

```
cabeçalho  11 11 03 00 05 00 00 00 00 00 02 00
corpo      a1 01 82 18 28 18 29
```

(12 B de cabeçalho + 7 B de corpo = 19 B de mensagem.)

### 6.11 `CATALOG` (`0x10`)

Classe F. Transferência dos bytes do catálogo por intervalos.

| Fase (1) | Sentido | Campos |
|---|---|---|
| 0, pedido | H → D | 2 `offset` (u32); 3 `maxlen` (u16, ≤ `bulk_max`) |
| 1, dados | D → H | 2 `offset`; 3 `total` (u32); 4 bytes (`bstr`, ≤ `maxlen`) |

O H pede por ordem até `offset + len = total`, calcula o SHA-256 e compara com a chave 8 do `HELLO`. Se não bater, o catálogo é inválido: `J-E2203`, `BYE(error)`, e o próximo handshake recebe o modo `maintenance`.

```cbor-diag
{1: 0, 2: 0, 3: 900}
```

```
cabeçalho  10 11 01 00 00 00 00 00 00 00 00 00
corpo      a3 01 00 02 00 03 19 03 84
```

(12 B de cabeçalho + 9 B de corpo = 21 B de mensagem.)

```cbor-diag
{1: 1, 2: 0, 3: 1873, 4: h'a8012e2e2e'}
```

```
cabeçalho  10 11 02 00 01 00 00 00 00 00 01 00
corpo      a4 01 01 02 00 03 19 07 51 04 45 a8 01 2e 2e 2e
```

(12 B de cabeçalho + 16 B de corpo = 28 B de mensagem.)

### 6.12 `PROPOSE` (`0x13`)

H → D, classe E. As propostas dos escritores do nó Linux nas saídas do MCU. É a imagem, no fio, do buffer de propostas de um tick (01 §3.4).

`{1: [+ writer_block]}`, com `writer_block = [origin, level, kind, lease, props, ? calls]`:

| Posição | Campo | Tipo | Significado |
|---|---|---|---|
| 0 | `origin` | u32 | o escritor no nó Linux (ref/00 §5); informativo |
| 1 | `level` | 1–16 | o nível no árbitro do MCU (A1) |
| 2 | `kind` | uint | `writer_kind` (ref/00 §6) |
| 3 | `lease` | u32 ms, ≥ 1 | A2; limitado a `max_lease` (§8.1) |
| 4 | `props` | `[* [sid, v, ? at]]` | `v = null` é `relinquish()` nesse nível (01 §6); `at` **reservado 1.1** (escritas agendadas, M6): um D 1.0 rejeita a proposta com `wrong_type` |
| 5 | `calls` | `[* [mid, args]]` | registos de método (17 §4.1); `args` é um registo ou `null` |

- Um bloco (um escritor) nunca é partido entre mensagens. Se as saídas de um grupo atómico do MCU forem escritas pelo mesmo escritor, têm de ir no mesmo bloco (`J0735` se não couber numa trama).
- **Quando o H envia** (§8.1): quando o valor proposto muda, e pelo menos a cada `renew = max(1 ms, lease / 3)`, arredondado ao tick do escritor.

Exemplo: o escritor 17 (`control:10`, ciclo, lease 30 ms) escreve duas saídas digitais; o escritor 23 (`manual:4`) liberta a saída 18 e chama o método 3 com `pct = 60 %` (o único argumento do método: chave 0):

```cbor-diag
{
  1: [[17, 10, 0, 30, [[16, true], [17, false]]], [23, 4, 2, 5000, [[18, null]], [[3, {0: 60.0}]]]]
}
```

```
cabeçalho  13 10 92 01 2c 03 ff ff ff ff
corpo      a1 01 82 85 11 0a 00 18 1e 82 82 10 f5 82 11 f4 86 17 04 02 19 13 88 81
           82 12 f6 81 82 03 a1 00 f9 53 80
```

(10 B de cabeçalho + 35 B de corpo = 45 B de mensagem.)

### 6.13 `OUTCOMES` (`0x14`)

D → H, classe F. Os resultados que o envelope **do MCU** decidiu para as propostas remotas.

| Chave | Campo | Tipo | Obr. | Significado |
|---|---|---|---|---|
| 1 | `ref` | u16 | sim | `seq` do último `PROPOSE` consumido até este tick |
| 2 | `t` | `mono-us` | sim | instante do tick do MCU |
| 3 | resultados | `[* [sid, origin, level, o, ? applied, ? detail]]` | sim | só os que mudaram (§8.2) |
| 4 | expirados | `[* [sid, origin, level]]` | não | leases remotos que expiraram neste tick (A2) |
| 5 | perdidos | u32 | não, 0 | resultados que não seguiram por falta de janela |

`o` é o código da ref/00 §4.4. `applied` é o valor aplicado, presente quando difere do proposto (`null` = igual ao proposto, só para poder dar `detail`). `detail` é o motivo (`rejected`) ou o nível ativo (`overridden`).

```cbor-diag
{
  1: 402,
  2: 1240900,
  3: [[16, 17, 10, 1], [17, 17, 10, 5, true, 3], [20, 23, 4, 3, 12.5]],
  4: [[19, 31, 13]]
}
```

```
cabeçalho  14 10 2d 03 92 01 ff ff ff ff
corpo      a4 01 19 01 92 02 1a 00 12 ef 44 03 83 84 10 11 0a 01 86 11 11 0a 05 f5
           03 85 14 17 04 03 f9 4a 40 04 81 83 13 18 1f 0d
```

(10 B de cabeçalho + 40 B de corpo = 50 B de mensagem.)

(Saída 16 `accepted`; saída 17 `overridden` pelo nível 3, que aplica `true`; saída 20 `clamped_rate` para 12,5; o lease do escritor 31 no nível 13 da saída 19 expirou.)

### 6.14 `CALL` (`0x15`)

H → D, classe F. Uma chamada a um método de classe, fora do ciclo: `jk call`, uma ponte, uma tarefa async que espera pelo resultado (17 §2.1). As chamadas de tarefas de ciclo vão no `PROPOSE` (§6.12).

| Chave | Campo | Tipo | Obr. | Significado |
|---|---|---|---|---|
| 1 | `id` | u32 | sim | do pedido; volta no `RESULT` |
| 2 | `mid` | u16 | sim | catálogo |
| 3 | `origin` | u32 | sim | |
| 4 | `level` | 1–16 | sim | |
| 5 | `kind` | uint | sim | `writer_kind` |
| 6 | `lease` | u32 ms | sim | das escritas expandidas |
| 7 | `args` | registo | não | |
| 8 | chave | `tstr` 1–64 | não | idempotência (§8.5) |

```cbor-diag
{1: 9001, 2: 3, 3: 23, 4: 4, 5: 2, 6: 5000, 7: {0: 60.0}}
```

```
cabeçalho  15 11 93 01 2d 03 00 00 00 00 07 00
corpo      a7 01 19 23 29 02 03 03 17 04 04 05 02 06 19 13 88 07 a1 00 f9 53 80
```

(12 B de cabeçalho + 23 B de corpo = 35 B de mensagem.)

### 6.15 `ACTION` (`0x16`)

H → D, classe F. Invoca uma ação de dispositivo ou de instância (02 §8, 17 §4.2).

| Chave | Campo | Tipo | Obr. | Significado |
|---|---|---|---|---|
| 1 | `id` | u32 | sim | |
| 2 | `aid` | u16 | sim | catálogo |
| 3 | `args` | registo | não | |
| 4 | chave | `tstr` 1–64, ASCII imprimível | **sim** | idempotência (ref/00 §5) |
| 5 | `role` | uint | sim | o papel verificado de quem pediu |
| 6 | `timeout` | u32 ms | não | pode encurtar o do MCU, nunca alongá-lo |

```cbor-diag
{1: 9002, 2: 1, 4: "01J8Z3K4M5N6P7Q8R9S0T1V2W3", 5: 1}
```

```
cabeçalho  16 11 94 01 2d 03 00 00 00 00 08 00
corpo      a4 01 19 23 2a 02 01 04 78 1a 30 31 4a 38 5a 33 4b 34 4d 35 4e 36 50 37
           51 38 52 39 53 30 54 31 56 32 57 33 05 01
```

(12 B de cabeçalho + 38 B de corpo = 50 B de mensagem.)

### 6.16 `RESULT` (`0x17`)

D → H, classe F. A resposta a `CALL`, `ACTION`, `CANCEL`, `ALARM_CMD`, `RESET` e `RESTART`.

| Chave | Campo | Tipo | Obr. | Significado |
|---|---|---|---|---|
| 1 | `id` | u32 | sim | o do pedido |
| 2 | estado | uint | sim | `ok` 0, `err` 1, `running` 2 (ação longa), `cancelled` 3 |
| 3 | valor | qualquer | não | o `Ok(...)` de uma ação (registo ou `null`) |
| 4 | `status` | uint | com `err` | §10.1 |
| 5 | código | `tstr` | não | `J-E23xx` |
| 6 | progresso | 0–100 | não | com `running`; no máximo 1 por segundo |
| 7 | resultados | `[* outcome-entry]` | `CALL` | os `WriteOutcome` das escritas expandidas, depois do commit |
| 8 | detalhe | `tstr` | não | |
| 9 | repetido | bool | não, `false` | `true`: devolvido pela deduplicação, sem repetir o comando |
| 10 | expansão | `[* [sid, v]]` | `CALL` | as propostas em que o método se expandiu; o H usa-as para renovar o lease (§8.5) |

```cbor-diag
{
  1: 9001,
  2: 0,
  7: [[20, 23, 4, 1], [21, 23, 4, 1]],
  10: [[20, 60.0], [21, true]]
}
```

```
cabeçalho  17 11 2e 03 94 01 00 00 00 00 1f 00
corpo      a4 01 19 23 29 02 00 07 82 84 14 17 04 01 84 15 17 04 01 0a 82 82 14 f9
           53 80 82 15 f5
```

(12 B de cabeçalho + 29 B de corpo = 41 B de mensagem.)

```cbor-diag
{1: 9002, 2: 0, 3: null}
```

```
cabeçalho  17 11 2f 03 94 01 00 00 00 00 20 00
corpo      a3 01 19 23 2a 02 00 03 f6
```

(12 B de cabeçalho + 9 B de corpo = 21 B de mensagem.)

### 6.17 `CANCEL` (`0x18`)

H → D, classe F. `{1: id}` de uma ação longa em curso. Resposta: `RESULT` com `cancelled` para esse `id`, ou `ERROR` estado 6 se já terminou ou não existe.

```cbor-diag
{1: 9003}
```

```
cabeçalho  18 11 95 01 2f 03 00 00 00 00 09 00
corpo      a1 01 19 23 2b
```

(12 B de cabeçalho + 5 B de corpo = 17 B de mensagem.)

### 6.18 `EVENTS` (`0x19`)

D → H, classe F. Ocorrências de `event` (de classes, de dispositivos e de apps). `{1: t0, 2: [* [eid, dt, data, ? flags]], ? 3: perdidos}`. `data` é o registo do evento (§6.2). Fila limitada no MCU; cheia, o evento **novo** é descartado e contado na chave 3 (a ordem e os primeiros eventos explicam uma falha melhor do que os últimos).

O evento `tripped { code }` de uma bomba (`std/pump@1`), com `code` no campo 0 do registo:

```cbor-diag
{1: 1241000, 2: [[2, 120, {0: 17}]]}
```

```
cabeçalho  19 11 30 03 95 01 00 00 00 00 21 00
corpo      a2 01 1a 00 12 ef a8 02 81 83 02 18 78 a1 00 11
```

(12 B de cabeçalho + 16 B de corpo = 28 B de mensagem.)

### 6.19 `ALARMS` (`0x1A`)

D → H, classe F. Transições de alarme (02 §11), pela ordem em que aconteceram. `{1: t0, 2: [* [alid, estado, dt, ? flags]], ? 3: snapshot, ? 4: perdidos}`. Estado: `alarm_state` da ref/00 §6. Flags: bit 0 avaliado com `Uncertain`; bit 1 arquivado (*shelved*); bit 2 suprimido (`suppress_when`).

- Com `snapshot = true`, a lista traz todos os alarmes que não estão `normal`, e os ausentes estão `normal`. Enviado depois do `READY` e a pedido (`READ` com a chave 2).
- Se a fila encher, o D junta as transições por alarme (fica o estado mais recente), conta as perdidas na chave 4, e envia um `snapshot` logo que possa.

```cbor-diag
{1: 1241000, 2: [[1, 1, 0]], 3: false}
```

```
cabeçalho  1a 11 31 03 95 01 00 00 00 00 22 00
corpo      a3 01 1a 00 12 ef a8 02 81 83 01 01 00 03 f4
```

(12 B de cabeçalho + 15 B de corpo = 27 B de mensagem.)

### 6.20 `ALARM_CMD` (`0x1B`)

H → D, classe F. `{1: id, 2: alid, 3: op, ? 4: ttl (ms), 5: role}`. `op`: `ack` 0, `shelve` 1 (com `ttl`, obrigatório, 02 §11), `unshelve` 2. O efeito aparece como transição em `ALARMS`; a resposta é um `RESULT`.

```cbor-diag
{1: 9004, 2: 1, 3: 1, 4: 7200000, 5: 1}
```

```
cabeçalho  1b 11 96 01 31 03 00 00 00 00 0a 00
corpo      a5 01 19 23 2c 02 01 03 01 04 1a 00 6d dd 00 05 01
```

(12 B de cabeçalho + 17 B de corpo = 29 B de mensagem.)

### 6.21 `RESET` (`0x1C`)

H → D, classe F. Termina o trip de uma saída do MCU (`jk reset`, 16 E7). `{1: id, 2: sid, 3: role}`. Uma saída que não está em trip dá `err` com estado 16 (`J-E2320`).

```cbor-diag
{1: 9005, 2: 18, 3: 1}
```

```
cabeçalho  1c 11 97 01 31 03 00 00 00 00 0b 00
corpo      a3 01 19 23 2d 02 12 03 01
```

(12 B de cabeçalho + 9 B de corpo = 21 B de mensagem.)

### 6.22 `RESTART` (`0x1D`)

H → D, classe F. `{1: id, 2: tipo, 3: role}`. Tipo `warm` 0: `jk_restart` (saídas para `safe`, leases largados, estado persistido recarregado; ref/08); `reboot` 1: reinício do MCU. O D responde `RESULT ok`, envia `BYE(reboot)` e reinicia. Papel `admin`. Permitido em todos os modos, porque é o que resolve um MCU em `maintenance`.

```cbor-diag
{1: 9006, 2: 0, 3: 2}
```

```
cabeçalho  1d 11 98 01 31 03 00 00 00 00 0c 00
corpo      a3 01 19 23 2e 02 00 03 02
```

(12 B de cabeçalho + 9 B de corpo = 21 B de mensagem.)

### 6.23 `TIME` (`0x20`)

Classe E. Três fases (§8.4):

| Fase (1) | Sentido | Campos |
|---|---|---|
| 0, sonda | H → D | 2 `t1`: relógio monotónico do H, µs |
| 1, eco | D → H | 2 `t1` (eco); 3 `t2`: MCU ao receber; 4 `t3`: MCU ao enviar (`mono-us`) |
| 2, acerto | H → D | 5 `M` (`mono-us`); 6 UTC em `M` (`utc-us`); 7 qualidade (`clock_quality`); 8 fonte (`none` 0, `ntp` 1, `ptp` 2, `gnss` 3, `manual` 4); 9 correção de taxa (i32, ppb); 10 erro estimado (µs); 11 `max_holdover` (ms); 12 holdover desde (`utc-us`) |

```cbor-diag
{1: 0, 2: 81234567890}
```

```
cabeçalho  20 10 98 01 31 03 00 00 00 00
corpo      a2 01 00 02 1b 00 00 00 12 e9 f5 22 d2
```

(10 B de cabeçalho + 13 B de corpo = 23 B de mensagem.)

```cbor-diag
{1: 1, 2: 81234567890, 3: 1241500, 4: 1241512}
```

```
cabeçalho  20 10 32 03 98 01 00 00 00 00
corpo      a4 01 01 02 1b 00 00 00 12 e9 f5 22 d2 03 1a 00 12 f1 9c 04 1a 00 12 f1
           a8
```

(10 B de cabeçalho + 25 B de corpo = 35 B de mensagem.)

```cbor-diag
{1: 2, 5: 1241512, 6: 1790244000123456, 7: 0, 8: 1, 9: -1200, 10: 180}
```

```
cabeçalho  20 10 99 01 32 03 00 00 00 00
corpo      a7 01 02 05 1a 00 12 f1 a8 06 1b 00 06 5c 37 a9 b0 4a 40 07 00 08 01 09
           39 04 af 0a 18 b4
```

(10 B de cabeçalho + 30 B de corpo = 40 B de mensagem.)

### 6.24 `TRACE` (`0x21`)

O anel de traço do MCU (19 §3, U9). O conteúdo é o fluxo `.jktrace` v1 (16 §7) que o `jk_trace_read` produz (ref/08 §2.6.o): tempos em ns, um segmento por grupo, bit 0 das `flags` do cabeçalho a 1 (segmentos sem compressão). Os `offset` e `len` do `TRACE` são os do `jk_trace_read`. O `jokerd` ou a `jk` comprimem-no ao recebê-lo.

| Fase (1) | Sentido | Classe | Campos |
|---|---|---|---|
| 0, listar | H → D | F | — |
| 1, info | D → H | F | 2 `[* [seg, motivo, bytes, congelado_em]]`; motivo: `continuous` 0, `trip` 1, `manual` 2, `crash` 3 |
| 2, ler | H → D | F | 3 `seg`; 4 `offset`; 5 `len` (≤ `bulk_max`) |
| 3, dados | D → H | E | 3 `seg`; 4 `offset`; 6 bytes; 7 último |
| 4, libertar | H → D | F | 3 `seg`: o D pode voltar a gravar por cima |
| 5, congelar | H → D | F | congela o anel contínuo agora (`jk_trace_freeze`); o D responde com `info` |

- Um trip congela o anel sozinho (19 U9) e o D envia `info` sem pedido.
- Os dados só se leem de segmentos congelados; ler o anel contínuo sem o congelar dá `ERROR` estado 26 (`J-E2420`).
- O H pede os intervalos por ordem e repete um `ler` sem resposta ao fim de `RTO`.

```cbor-diag
{1: 1, 2: [[0, 1, 16384, 1240000]]}
```

```
cabeçalho  21 11 33 03 99 01 00 00 00 00 23 00
corpo      a2 01 01 02 81 84 00 01 19 40 00 1a 00 12 eb c0
```

(12 B de cabeçalho + 16 B de corpo = 28 B de mensagem.)

```cbor-diag
{1: 2, 3: 0, 4: 0, 5: 512}
```

```
cabeçalho  21 11 9a 01 33 03 00 00 00 00 0c 00
corpo      a4 01 02 03 00 04 00 05 19 02 00
```

(12 B de cabeçalho + 11 B de corpo = 23 B de mensagem.)

```cbor-diag
{1: 3, 3: 0, 4: 0, 6: h'4a4b545201002e2e2e', 7: false}
```

```
cabeçalho  21 10 34 03 9a 01 00 00 00 00
corpo      a5 01 03 03 00 04 00 06 49 4a 4b 54 52 01 00 2e 2e 2e 07 f4
```

(10 B de cabeçalho + 20 B de corpo = 30 B de mensagem.)

### 6.25 `OTA` (`0x22`)

Firmware ou blob de geração, assinados. As fases, os campos e o comportamento estão no §9; os exemplos também.

### 6.26 `ERROR` (`0x07`)

↔, classe E. `{1: status, ? 2: código, ? 3: tipo, ? 4: seq, ? 5: rseq, ? 6: detalhe (≤ 128 B)}`. Diz ao outro lado que uma mensagem foi recusada. Nunca se responde a um `ERROR` com outro. No máximo 10 por segundo por sessão. Os erros de L1 e L2 (CRC, etiqueta, replay) **não** geram `ERROR`: só contadores, para não dar informação a um atacante nem amplificar ruído.

```cbor-diag
{1: 6, 2: "J-E2310", 3: 22, 4: 404, 5: 8}
```

```
cabeçalho  07 10 0e 00 28 00 ff ff ff ff
corpo      a5 01 06 02 67 4a 2d 45 32 33 31 30 03 16 04 19 01 94 05 08
```

(10 B de cabeçalho + 20 B de corpo = 30 B de mensagem.)

---

## 7. Máquina de estados da sessão

### 7.1 No dispositivo

```
                  arranque do MCU
                        │
                        ▼
                  ┌───────────┐ ◀──────────────────────────────────────────────────┐
                  │   IDLE    │ ◀───────────────────────────────────┐              │
                  └─────┬─────┘                                     │              │
                        │ nonce_d novo; envia HELLO(oferta)         │              │
                        ▼                                           │              │
                  ┌───────────┐  sem resposta em 500 ms:            │              │
          ┌──────▶│  OFFERED  │──reenvia, backoff 100 ms → 5 s, ±20%│              │
          │       └─────┬─────┘  (nonce_d novo em cada tentativa)   │              │
          │             │ HELLO(resposta) com o eco certo           │              │
          │             ├── modo refused ──▶ espera 5 s ────────────┘              │
          │             │ deriva k_d2h, k_h2d; envia READY (F)                     │
          │             ▼                                                          │
          │       ┌───────────┐  READY sem confirmação após retransmit ────────────┤
          │       │CONFIRMING │                                                    │
          │       └─────┬─────┘                                                    │
          │             │ ACK do READY                                             │
          │             ▼                                                          │
          │       ┌───────────┐  link_timeout sem pacote válido ───────────────────┤
          │       │    UP     │  BYE recebido ─────────────────────────────────────┤
          │       │ (modo m)  │  retransmit esgotado numa mensagem F ──────────────┤
          │       └─────┬─────┘  HELLO(pedido) válido e 2×keepalive em silêncio ───┘
          │             │ nova geração ativa, ou KEYROT aplicada
          └──BYE(generation_change / rehandshake)
```

- Em todos os estados, os **leases remotos continuam a correr** no árbitro do MCU (§7.4). A sessão e os leases são independentes.
- Ao sair de `UP`, o D apaga as chaves de sessão, as janelas e a remontagem, e abandona as mensagens F por enviar, exceto os `RESULT` de ações e os `ALARMS`, que ficam para a sessão seguinte se o MCU não reiniciar (as transições também se recuperam pelo `snapshot`).

### 7.2 No anfitrião

| Estado do H | Estado do link (`link_state`, ref/00 §6) | Qualidade dos sinais do MCU no nó Linux | Escritas |
|---|---|---|---|
| `IDLE`, à espera de oferta | `connecting` | `Uncertain(Initial)` até à primeira amostra; depois de uma perda, `Bad(CommFailure)` | não |
| `HANDSHAKE` (respondeu, espera o `READY`) | `connecting` | como acima | não |
| `UP` · `full` | `up` | a que vem no `SAMPLES` | sim |
| `UP` · `full` com perdas ≥ 20% em 10 s | `degraded` | a que vem; `Uncertain(Stale)` pelo `max_age` | sim; alarme `joker.device.<d>.degraded` |
| `UP` · `observe` | `drift` | a que vem | **não** em `prod` (as propostas não saem; os `write` locais dão `J-E2319`); em `dev`, sim, com aviso |
| `UP` · `maintenance` | `down` | `bad(config_error)` | não |
| respondeu `refused` (§7.3, regras 2 e 3) | `down` | `bad(config_error)` | não |
| `LOST` (`link_timeout`, `BYE`, `retransmit` esgotado) | `down` | `Bad(CommFailure)`; alarme `joker.device.<d>.offline` | não |

Em resumo, do modo da sessão para o estado do link: `full` → `up` (ou `degraded`); `observe` → `drift`; `maintenance` e `refused` → `down`, com as amostras em `bad(config_error)` (documento 05, §12). O H envia `HELLO(pedido)` no arranque e a cada 1 s enquanto não houver sessão, e quando recebe uma trama de sessão com um `sid` que não conhece. O sexto valor de `link_state`, `stopped`, é do núcleo: o driver `joker-link` não está a correr (geração a trocar, dispositivo desativado). O modo da sessão é o `session_mode` da ref/00 §6, também no `jk_link_status` do MCU (ref/08 §2.6.p).

### 7.3 Verificação no `HELLO` e degradação

O H decide o modo pela ordem seguinte; a primeira regra que se aplica decide.

| # | Condição | Modo | Código | O que fica visível |
|---|---|---|---|---|
| 1 | trama de handshake com `kid` desconhecido, ou etiqueta errada | — (sem resposta) | `J-E2103`, `J-E2101` | contador; alarme `joker.device.<d>.auth` após 3 |
| 2 | nenhuma major comum (§13) | `refused`, estado 3 | `J-E2201`, `J0461` (ref/02) | o nó não usa o link |
| 3 | a identidade (chave 4.1) não é a esperada para este dispositivo | `refused`, estado 27 | `J-E2207` | alarme |
| 4 | o D não tem geração válida (sem chave 6, ou estado `failed`) | `maintenance`, estado 28 | `J-E2206` | só `OTA`, `TRACE`, `TIME`, `RESTART`, `CATALOG`, `KEYROT` |
| 5 | o catálogo deste hash já falhou a verificação (§6.11) | `maintenance`, estado 31 | `J-E2203` | idem |
| 6 | firmware (digest da imagem ou do descritor), versão do Joker Micro, features ou drivers (chave 15) diferentes de `[mcu.<d>]` no lock; id de hardware diferente de `[discovered.<d>] serial`; hash da geração diferente do que o `jk build` calculou para a geração ativa do nó; hash do catálogo diferente do `[discovered.<d>] digest` | `observe` em `prod`, `full` em `dev` | `J-E2202`; `J0460` ou `J0401` com `kind` (ref/02 §3.1.5) | alarme `joker.device.<d>.drift` |
| 7 | nenhuma das anteriores | `full` | — | — |

- Os **parâmetros alterados localmente** (chave 16) não mudam o modo: são deriva, que aparece no relatório ao Deck (ref/10 §2.10.1, `mcus`) e em `GET /v1/micros/{device}` (ref/09 §5.18).
- A verificação das **classes** (U7) é por instância, e não muda o modo: uma instância cuja classe tem a mesma major da que o nó Linux usa é aceite, mesmo com hash diferente; uma major diferente não cria a instância implícita (`J-E2209` no link, `J0630` do lado da app) e o alarme `joker.device.<d>.drift` dispara.
- Uma **sessão em `observe` passa a `full`** com uma geração nova do nó Linux que atualize o lock. O H envia então `BYE(rehandshake)`; como em `observe` não há propostas remotas, o novo handshake não mexe nas saídas.
- Em `maintenance`, o D não envia `SAMPLES`, `OUTCOMES`, `EVENTS` nem `ALARMS`.

### 7.4 Perda de link, lease e estado seguro

A regra do documento 19, §6: **quem manda nas saídas do MCU é o MCU**. Um escritor remoto é um nível com lease, como qualquer outro (16 A2). Quando o link cai, ninguém "desliga" nada: o H deixa de renovar, os leases remotos expiram **no MCU**, e o árbitro do MCU passa ao nível ativo seguinte ou, sem nenhum, ao `safe` (A6).

```
  t (ms)   0         10        20        30        40
  H        PROPOSE   PROPOSE   ✗ jokerd morre
  D        consome   consome   ·         ·         ·
           lease_until = t_tick + 30 ms  ──────────────▶ 40: lease expira no latch
  saída    valor do nível 10 ─────────────────────────────▶ nível seguinte, ou safe
  link     UP ───────────────────────────── (200 ms) ──▶ LOST (não interfere com a saída)
```

- A sessão não entra na decisão: o `link_timeout` só serve para o D recomeçar o handshake e para o H mostrar `Down`. Não há um "largar tudo quando o link cai", que seria uma segunda regra e um segundo relógio a testar.
- O pior caso é `max_lease` (omissão 5 s), que é também o teto das escritas manuais remotas: o TTL de 8 h de um `jk write` fica no nó Linux, que o renova em pedaços de `max_lease` (§8.1).

### 7.5 Reinícios e reconexão

| Caso | Como se deteta | O que acontece |
|---|---|---|
| o `jokerd` reinicia | o D deixa de receber tramas; o H novo envia `HELLO(pedido)` | leases remotos expiram (§7.4); o D obedece ao pedido ao fim de 2 × `keepalive` de silêncio; nova sessão; o H volta a propor; os pedidos F pendentes perderam-se do lado do H, e os tickets resolvem-se com `J-E2318` |
| o MCU reinicia | `boot_count` (chave 12.1) mudou | o MCU arranca com as saídas em `safe` (A8) e sem níveis remotos (não os persiste; §8.1, A14); o H volta a propor, incluindo os níveis manuais ainda no TTL; o H descarta os tickets pendentes com `J-E2318` e **não** repete ações (a deduplicação perdeu-se com a RAM) |
| o link cai e volta, sem reinícios | `boot_count` igual | o H repete as `ACTION` e os `CALL` com chave cujo `RESULT` não chegou, com o mesmo `id` e a mesma chave: a deduplicação do D devolve o resultado sem repetir o comando |
| nova geração no MCU (OTA ou troca local) | `BYE(generation_change)` e novo `HELLO` | com troca a quente (`jk_generation_swap`), as saídas idênticas **mantêm** o valor aplicado, os leases remotos e os trips (A8); o H refaz o perfil se o hash do catálogo mudou |
| nova geração no nó Linux | — | os `origin` podem mudar de número; como a posse remota é por nível (§8.1), a nova proposta ocupa o mesmo nível sem conflito |

---

## 8. Semântica ligada ao núcleo

### 8.1 `PROPOSE` e o árbitro (16 A1–A14)

| Regra | No Joker Link |
|---|---|
| **A1** escritor = (origem, nível) | Cada bloco de `PROPOSE` é um escritor remoto: (`origin`, `level`). No MCU, cada par (link, nível delegado) é **um escritor** da tabela `WRITERS` do blob (ref/08 §2.6.p, 16 A12), e a **posse de um nível remoto é do nível**, e não do `origin`: uma proposta remota num nível delegado ocupa-o, seja qual for o `origin`. O nó Linux garante que nunca há dois escritores seus no mesmo nível da mesma saída (`J0210` no seu `jk check`). Assim uma geração nova do nó, que renumera os `origin`, não provoca `NotOwner` durante um lease |
| níveis delegados | Cada saída do MCU delega ao link um conjunto de níveis (catálogo, chave 1.8). Por omissão: todos os que nenhum escritor local do MCU usa. Uma proposta num nível não delegado é `Rejected(NotOwner)` e não renova nada (A3); no `jk check` do nó Linux é `J0731` |
| **A2** lease | `lease_until = t_tick + lease`, com `t_tick` o tick **do MCU** que consome a proposta. A latência do link não come o lease: conta a partir do consumo. **Omissão:** um escritor Linux (app de ciclo, `calc`, instância) que não declara `lease` escreve numa saída de MCU com `lease = min(3 × período do grupo Linux do escritor, max_lease do link)`; um escritor async sem grupo usa o `max_lease` do link. O `J0730` (§11.6) verifica a omissão como um valor declarado |
| **A3** renovação | Qualquer proposta aceite pelo árbitro do MCU renova, seja qual for o resultado do envelope; `NotOwner` e `WrongType` não renovam |
| **A4** ordem | As propostas remotas entram no passo 2 do tick do grupo dono (01 §4), como as async, pela ordem de receção (blocos e entradas pela ordem da mensagem). Dentro do mesmo escritor, vence a última |
| **A5** sem empates | garantido pelos dois `jk check` (o do nó e o do MCU) e pela posse por nível |
| **A6**, **A7**, **A8** | do MCU, sem alterações. `relinquish` (`v = null`) liberta o nível logo no consumo |
| **A9** `max_lease` | O `lease` efetivo é `min(lease, max_lease do link)` (16 A12). O `jk check` do nó recusa um `max_lease` do link acima do de uma saída (`J0733`), e por isso o da saída nunca é o menor; o `lease` declarado de um escritor remoto verifica-se contra o `max_lease` **do link** (`J0213`). Se mesmo assim chegar um valor maior, o MCU limita-o e conta (`J-E2303`, aviso) |
| **A10** manual | O `--ttl` fica no nó Linux. O H envia a escrita manual com `lease = min(ttl restante, max_lease do link)` e renova-a até ao fim do TTL. Um `jokerd` morto deixa de renovar, e o override expira no MCU em ≤ `max_lease` |
| **A14** persistência | Do nó Linux. O MCU **não persiste** níveis remotos, manuais ou não: depois de um reset do MCU (`boot_count` mudou, §7.5), o nível desaparece e a saída segue o árbitro do MCU até o H o renovar, no `renew` seguinte. Um `jokerd` que reinicia repõe os níveis manuais do seu registo (16 A14) e volta a enviá-los |
| **A11** grupo dono | o do MCU; o nó Linux não precisa de o saber |

**Envio pelo H.** Por escritor remoto e saída, uma caixa *latest-wins* (01 §3.4). O H envia quando o valor muda e pelo menos a cada `renew = max(1 ms, lease / 3)`. Um `relinquish` segue em todos os `PROPOSE` seguintes desse escritor até chegar um `OUTCOMES` com resultado `none` para essa saída e nível, ou até passar o lease.

**Métodos.** As chamadas de tarefas de ciclo vão no `PROPOSE` (posição 5) e são expandidas no passo 5a do tick do MCU, pela ordem de chegada, no nível do escritor, com `own()` = a última proposta desse nível (17 §4.1). Repetem-se a cada renovação, como no Linux. As chamadas de fora do ciclo usam `CALL` (§8.5).

**O nó Linux não arbitra nem aplica envelope** às saídas do MCU: declará-las em `[outputs]` do nó é `J0234` (ref/01). O `jk write` mostra o envelope a partir dos campos 1.9 e 1.10 do catálogo, só para informação.

### 8.2 `OUTCOMES` e os resultados (ref/00 §4.4)

- O D emite um `OUTCOMES` no fim de cada tick que consumiu propostas remotas **se** algum resultado mudou; senão, no máximo um a cada `report`, vazio, só para fazer avançar o `ref`.
- Um resultado "mudou" quando mudou o `origin`, o código, o valor aplicado, o `detail`, ou o valor proposto, ou quando é o primeiro desse (saída, nível) na sessão.
- É fiável e ordenado, porque cada entrada é uma **decisão** que o registo de decisões do nó Linux regista (invariante 8 do AGENTS.md). Com a janela cheia, os resultados pendentes juntam-se por (saída, nível), fica o mais recente, e os perdidos contam-se na chave 5 (invariante 7: a telemetria nunca trava o controlo).
- **No nó Linux:** um ticket async de uma proposta que seguiu no `PROPOSE` com `seq = S` resolve-se no primeiro `OUTCOMES` com `ref ≥ S`: com a entrada, se ela lá estiver; senão, com o último resultado conhecido desse (saída, nível). Até lá, é `Deferred` (6). O `last_outcome()` de uma tarefa de ciclo é o último resultado recebido, que pode ter um ou dois ticks de atraso face ao Linux puro (16 A12: o lease e o resultado contam no consumidor, o MCU).
- O D nunca envia `deferred` (6) nem `shadowed` (7): são estados do nó Linux. Recebidos, contam como desconhecidos.
- Um ticket sem `OUTCOMES` ao fim de 3 × `RTO` + o período do grupo dono resolve-se com `J-E2304` ("resultado desconhecido"), que não é `accepted`.
- Um lease remoto que expira (chave 4) dá o aviso `J-E2305` e o contador `joker.device.<d>.lease_expired`. Num link saudável nunca devia acontecer: indica falta de banda ou de renovação (§11.6).

### 8.3 Amostras: qualidade e tempo no `jokerd`

O driver `joker-link` escreve cada entrada de `SAMPLES` no slot do sinal correspondente do nó Linux (`<dispositivo>.<caminho no MCU>`), pelo estágio de aquisição normal (01 §3.3):

| Campo do slot (01 §3.2) | De onde vem |
|---|---|
| `value` | `v`, convertido para os bits `u64` da ref/00 §4.1, na unidade do catálogo |
| `quality` | `q` tal e qual (código, motivo e flags) |
| `ts_acq` | relógio monotónico do nó Linux na **receção** da trama (ns) |
| `ts_hw` | `t0 + dt` convertido para o relógio monotónico do nó: `ns = 1000 · (t0 + dt − θ)`, com θ e a taxa estimados pelo `TIME` (§8.4); só quando a estimativa existe; flag de domínio "link" no slot |
| `ts_src` | `ts_src` × 1000, se existir |
| `seq` | a do slot do nó Linux: cada entrada é uma amostra nova (também as de `refresh`). A `seq` de 16 bits do MCU serve para descartar entradas velhas (§5.4) e para contar amostras que o MCU produziu e não enviou |

- Os sinais do MCU `joker.X` aparecem no nó como `joker.device.<d>.X`.
- Uma instância `i` do MCU aparece como instância implícita do nó com o nome `i`, ou com o nome dado em `instances` no dispositivo (§15). Uma colisão com um nome do nó é `J0604`. Um caminho que passe os 8 segmentos com o prefixo do dispositivo é `J0740`.
- Uma instância composta do MCU chega ao nó com a **mesma árvore** e os **mesmos caminhos aninhados** (documento 16, P6), a começar no nome de topo: com `instances = { hp = "bomba_calor" }`, `hp.compressor.drive.speed` no MCU é `bomba_calor.compressor.drive.speed` no nó. As instâncias com 2.9 = `true` não ganham nome de topo no nó, e só existem pelos caminhos das composições a que pertencem. A renomeação de `instances` aplica-se só aos nomes de topo; uma entrada para uma instância que só é membro é `J0650`.
- Os instantes monotónicos do MCU só têm sentido com o par do `TIME` dessa sessão, e esse par vive no nó Linux (ref/00 §3). O MCU nunca reenvia um `mono-us` de uma sessão anterior, porque o seu relógio recomeça em cada arranque e o H refaz θ em cada sessão.

### 8.4 `TIME` e o `Holdover`

**No H**, a cada `time_interval` (e 4 vezes, com 100 ms de intervalo, depois do `READY`):

1. envia a sonda com `t1` (o seu relógio monotónico, µs) e regista `t4` ao receber o eco;
2. calcula o atraso `δ = (t4 − t1) − (t3 − t2)` e o desvio `θ = ((t2 − t1) + (t3 − t4)) / 2` (MCU − H);
3. rejeita amostras com `δ` acima de `time_max_delay` (omissão 5 ms em série, USB e CAN; 50 ms em IP), com o contador `J-E2430`; entre as aceites, usa a de menor `δ` das últimas 8 e ajusta a taxa por regressão;
4. envia o acerto: `M` = um instante recente do MCU, a UTC correspondente (pelo seu próprio relógio de parede), a qualidade, a fonte, a correção de taxa, o erro estimado (`δ/2` mais a incerteza do próprio H) e o `max_holdover` do seu `[clock]`.

A qualidade enviada é `synced` só se o relógio do próprio H estiver `synced` **e** o erro estimado for ≤ `time_tolerance` (omissão 1 ms); senão, `holdover` (com o "desde" do H) ou `unsynced`.

**No D** (`jk_time_set_utc`, ref/08): `utc(agora) = utc_M + (agora − M) · (1 + taxa · 10⁻⁹)`.

| Situação no MCU | Qualidade do relógio |
|---|---|
| antes do primeiro acerto desde o arranque | `unsynced` (sem UTC) |
| último acerto `synced` há menos de 3 × `time_interval` | `synced` |
| último acerto `synced` há mais, ou acerto `holdover` | `holdover(desde o último synced)` |
| em `holdover` há mais do que `max_holdover` (omissão 24 h), ou acerto `unsynced` | `unsynced` |

Uma app do MCU com `requires_clock = "synced"` vê as suas saídas bloqueadas (`Rejected(ClockUnsynced)`, passo 0 do envelope; 01 §7 e §18, 16 E1) fora de `synced`. Sem `TIME` (`time_interval = 0`) e com uma app dessas, é `J0741`.

Um salto do relógio de parede do H (NTP a corrigir horas depois de um arranque sem RTC) muda só o par UTC: o monotónico do MCU e a conversão θ não mudam, e as amostras não se baralham (01 §18).

### 8.5 Idempotência: `CALL` e `ACTION`

| | `CALL` (método) | `ACTION` (ação) |
|---|---|---|
| repetir | um método de estado é idempotente; um `toggle()` não é | perigoso (17 §2.1) |
| dentro da sessão | exatamente uma vez, pelo `rseq` | exatamente uma vez, pelo `rseq` |
| entre sessões, sem reinício do MCU | só com chave: o D deduplica | sempre com chave: o D deduplica |
| depois de um reinício do MCU | não se repete: `J-E2318` | não se repete: `J-E2318` |

**Tabela de deduplicação no MCU:** capacidade fixa (chave 11.5 do `HELLO`, ≥ 8). Cada entrada guarda a chave, o `id`, o estado e o resultado se tiver ≤ 64 B, durante `dedup_window` (16 E9; omissão 60 s, da geração do MCU). Com a mesma chave:

- ainda em curso → `RESULT running`, `repetido = true`;
- terminada → o `RESULT` guardado, `repetido = true` (sem valor se ele tinha mais de 64 B);
- tabela cheia de entradas dentro da janela → a nova é recusada com `err` e estado 8 (`J-E2315`), e **não** se despeja uma entrada viva: é preferível falhar do que executar duas vezes.

**`ACTION` no MCU**, pela ordem de 17 §4.2: `aid` (`J-E2310`) → papel (`J-E2313`) → deduplicação → permissivo (`J-E2312`) → fila serializada do dispositivo (cheia: `J-E2315`) → execução → timeout (`J-E2314`) → `RESULT`.

**`CALL` no MCU:** `mid` → níveis delegados de todos os `out` do método (senão `err`, estado 30, `J-E2303`) → deduplicação, se houver chave → expansão no passo 5a do próximo tick do grupo dono → `RESULT` depois do commit, com os `WriteOutcome` (chave 7) e a expansão (chave 10). **Renovação:** o H renova as escritas expandidas com `PROPOSE` dos pares da chave 10, no mesmo (`origin`, `level`), até ao fim do lease pedido. Assim um método não se executa outra vez só para manter o lease, e um `toggle()` não se desfaz.

### 8.6 Alarmes, eventos e trips

- O ciclo de vida do alarme (02 §11) corre no MCU. O H recebe as transições (`ALARMS`) e reflete-as nos sinais `alarm.<nome>.state` e `.active` do dispositivo e na sua lista de alarmes; `jk alarm ack` e `jk alarm shelve` de um alarme do MCU viram `ALARM_CMD`.
- Os trips de saídas do MCU (01 §7) vivem no MCU e persistem lá (`JK_AREA_PERSIST`, ref/08). O H vê-os como qualidade `Bad(Tripped)` na amostra da saída; `jk reset` vira `RESET`.
- Os eventos das instâncias do MCU chegam por `EVENTS` e entram no nó como os eventos de qualquer instância.

### 8.7 `TRACE` e o formato do 16 §7

O que se transfere é o próprio `.jktrace` do MCU: cabeçalho (`"JKTR"`, versão 1, `flags` com o bit 0 a 1, hashes da geração e do registo do MCU, épocas monotónica e UTC do MCU, qualidade do relógio) e segmentos sem compressão. Os instantes estão em ns (o MCU multiplica os seus µs por 1000) e no relógio do MCU; o `jk replay` corre o mesmo núcleo e reproduz as decisões do MCU (19 U9). As propostas remotas aparecem no traço com escritores `0x8000_0000 | slot`, e o registo de decisões do nó Linux guarda a correspondência slot → (`origin`, nível) de cada sessão.

---

## 9. OTA

### 9.1 O artefacto

Um artefacto OTA é um par: o **manifesto assinado** (COSE_Sign1, RFC 9052, EdDSA com Ed25519, ref/00 §7) e a **imagem** (os bytes do firmware, ou a imagem `[0, image_len)` do blob de geração da ref/08). O manifesto é pequeno e vai primeiro, para o D recusar um artefacto errado **antes** de gastar flash.

**Uma só forma de assinatura.** Numa geração, este COSE_Sign1 **é** o *trailer* do blob `.jkgen` (ref/08 §2.13.5): o ficheiro que o `jk build --generation` escreve é a imagem seguida do manifesto, e o D grava o manifesto recebido no `begin` a seguir à imagem, em `trailer_off = image_len`. O firmware leva o mesmo manifesto ao lado da imagem (`FW_STAGING`). A mesma verificação serve o OTA pelo link, o OTA local (`jk_ota_*`), o arranque e o perfil micro (§14).

`ota-manifest` (payload do COSE_Sign1):

| Chave | Campo | Tipo | Obr. | Significado |
|---|---|---|---|---|
| 1 | tipo | uint | sim | `ota_kind` (ref/00 §6): `generation` 1, `firmware` 2; `model` 3 e `app` 4 **reservados 1.4**; 0 nunca |
| 2 | alvo | `tstr` | sim | família de firmware ou placa; tem de ser o `firmware.3` do `HELLO` |
| 3 | versão | `tstr` | sim | SemVer (firmware) ou o `gen_seq` em decimal (geração) |
| 4 | tamanho | u32 | sim | bytes da imagem (numa geração, `image_len`) |
| 5 | digest | `bstr` 32 | sim | SHA-256 da imagem |
| 6 | `security_version` | u32 | sim | contador de segurança (anti-rollback) do firmware; na geração, 0 |
| 7 | `gen_seq` | u64 | geração | anti-rollback da geração (`min_seq`, ref/08); igual ao do cabeçalho do blob |
| 8 | hash da geração | `bstr` 32 | geração | o `source_hash` do blob; tem de ser o que o `HELLO` mostra depois da troca |
| 9 | capacidades | `bstr` 32 | geração | hash das capacidades do firmware para o qual a geração foi feita (ref/08 §2.14); tem de ser o `firmware.5` do MCU (senão `J-E2410`) |
| 10 | criado em | `utc-us` | não | |
| 11 | link mínimo | `[maj, min]` | não | versão mínima do Joker Link que o firmware fala |
| 12 | `board_id` | `bstr` 8 | geração | o do cabeçalho do blob (ref/08 §2.13.2) |
| 13 | formato | uint | geração | o `format` do blob |
| 14 | nó | `tstr` | não | nome do nó a que o blob está ligado (`NODE_BOUND`); comparado com o registo 16 da área `IDENTITY` (`JK_E_GEN_NODE`) |
| 15 | hash do registo | `bstr` 32 | não | o `registry_hash` do blob |

Cabeçalho protegido do COSE: `{1: -8 (EdDSA), 4: kid}`, com `kid` = os **16** primeiros bytes do SHA-256 da chave pública (o id das chaves da ref/02 §2.5 e da ref/10 §2.1). O D verifica com as chaves do seu bundle de confiança (ref/08 §2.13.5), pelos papéis da ref/00 §7.1: `firmware` para o firmware, `config` para a geração.

**Vetor de teste** (chave privada Ed25519 com semente `60 61 … 7f`; imagem de 8 KiB a zeros; geração 8):

```
chave pública  174553b456dddfc6908ecab1c101fe6ab21e2baa0617795b7d43a63482993fd5
kid            68894d58f18f2c34d49eb2f4b110e042
board_id       8bb5bccf6eedea71                                 (SHA-256("acme/io16-rp2350@1")[0..8])
manifesto      {1: 1, 2: "acme/io16-rp2350", 3: "8", 4: 8192,
                5: h'9f1dcbc35c350d6027f98be0f5c8b43b42ca52b7604459c0c42be3aa88913d47',
                6: 3, 7: 8,
                8: h'85378bbfa265d5109625f8c9e2e5e280e8e8e2af3497a4dfc8d74159b357553f',
                9: h'd1d9be4b6c268eef97fb159f5743637124413bd9a93a991d3ec8225bd33faae8',
                11: [1, 0], 12: h'8bb5bccf6eedea71', 13: 1}                  (153 B)
protegido      a2 01 27 04 50 68 89 4d 58 f1 8f 2c 34 d4 9e b2 f4 b1 10 e0 42
Sig_structure  ["Signature1", protegido, h'', manifesto]      SHA-256 796d7461…0e59a0ef1
assinatura     9bd8741baa97ce1667cfb12570f4cd84af9377d6d2cd5a7449a8a1d1e96ea616
               aec54e5722cf1a462703d9ebf8125162fd9041c8eca71e98517d4c9c155f960c
COSE_Sign1     d2 84 55 a2 01 27 04 50 68 89 4d 58 f1 8f 2c 34 d4 9e b2 f4 b1 10 e0 42 a0
               58 99 ac 01 01 02 70 … 0c 48 8b b5 bc cf 6e ed ea 71 0d 01
               58 40 9b d8 74 … 9c 15 5f 96 0c                                   (246 B)
```

### 9.2 Fases da mensagem `OTA`

| Fase (1) | Sentido | Classe | Campos | Significado |
|---|---|---|---|---|
| 0 `begin` | H → D | F | 2 `xfer` (u16); 3 tipo; 4 tamanho; 5 COSE_Sign1 (`bstr`); 6 bloco proposto | começa ou retoma |
| 1 `ready` | D → H | F | 2 `xfer`; 6 bloco aceite; 7 `offset` de retoma; 8 janela (blocos) | ou `abort` |
| 2 `data` | H → D | E | 2 `xfer`; 7 `offset`; 9 bytes | um bloco |
| 3 `ack` | D → H | E | 2 `xfer`; 7 `offset` | bytes contíguos já escritos em flash |
| 4 `end` | H → D | F | 2 `xfer` | todos os bytes enviados |
| 5 `verified` | D → H | F | 2 `xfer`; 10 estado | 0 = imagem completa e verificada |
| 6 `activate` | H → D | F | 2 `xfer`; 11 janela de confirmação (ms) | troca |
| 7 `confirm` | H → D | F | 3 tipo | confirma a experiência em curso |
| 8 `abort` | ↔ | F | 2 `xfer`; 10 estado; 12 código | desiste |
| 9 `state` | D → H | F | 3 tipo; 13 `generation_state` | resposta a `confirm`; também sem pedido, quando a janela expira |

### 9.3 Transferência e retoma

1. **`begin`.** O D verifica, antes de apagar qualquer setor: a assinatura e a confiança da chave (`J-E2401`, `J-E2402`); o tipo e o alvo (`J-E2405`); o tamanho contra a área (`JK_AREA_GEN_A/B` ou `JK_AREA_FW_STAGING`; `J-E2406`); o anti-rollback: `security_version` ≥ o piso guardado, ou `gen` > o `gen` atual (`J-E2404`); na geração, as capacidades (`J-E2410`). Só depois responde `ready`.
2. **Bloco.** Potência de 2 entre 64 B e `bulk_max`; o D pode baixar o proposto. `offset` é sempre múltiplo do bloco; só o último é mais curto.
3. **Janela.** O H envia até `janela` blocos à frente do último `ack`. O D escreve só blocos contíguos: um bloco fora de ordem é descartado, e o D responde logo com um `ack` do seu `offset` atual (retransmissão rápida). Sem progresso no `ack` durante `RTO`, o H recomeça no último `offset` confirmado (*go-back-N*).
4. **Retoma.** O D guarda em `JK_AREA_PERSIST` o par (digest, `offset` confirmado), arredondado ao último setor completo. Um `begin` com o mesmo manifesto, depois de uma queda do link ou de um reinício do MCU, recebe `ready` com esse `offset`. Um `begin` com outro manifesto descarta a transferência anterior.
5. **`end`.** O D confirma o tamanho e calcula o SHA-256 da área inteira: diferente do manifesto é `abort` com `J-E2403`. Igual, `verified` com estado 0. Guarda o manifesto recebido no `begin` a seguir à imagem (numa geração, como o *trailer* do blob, §9.1), para a voltar a verificar em cada arranque (ref/08 §2.12.3).
6. Um `xfer` sem tráfego durante 60 s expira no D (`J-E2408` no próximo `data`).

```cbor-diag
{1: 0, 2: 1, 3: 1, 4: 8192, 5: h'3c434f53455f5369676e313e', 6: 512}
```

```
cabeçalho  22 11 9b 01 34 03 00 00 00 00 0d 00
corpo      a6 01 00 02 01 03 01 04 19 20 00 05 4c 3c 43 4f 53 45 5f 53 69 67 6e 31
           3e 06 19 02 00
```

(12 B de cabeçalho + 29 B de corpo = 41 B de mensagem.)

(Na chave 5 vai o COSE_Sign1 inteiro do §9.1; aqui abreviado.)

```cbor-diag
{1: 1, 2: 1, 6: 512, 7: 0, 8: 4}
```

```
cabeçalho  22 11 35 03 9b 01 00 00 00 00 24 00
corpo      a5 01 01 02 01 06 19 02 00 07 00 08 04
```

(12 B de cabeçalho + 13 B de corpo = 25 B de mensagem.)

```cbor-diag
{1: 2, 2: 1, 7: 0, 9: h'000000002e2e2e'}
```

```
cabeçalho  22 10 9c 01 35 03 00 00 00 00
corpo      a4 01 02 02 01 07 00 09 47 00 00 00 00 2e 2e 2e
```

(10 B de cabeçalho + 16 B de corpo = 26 B de mensagem.)

```cbor-diag
{1: 3, 2: 1, 7: 2048}
```

```
cabeçalho  22 10 36 03 9f 01 00 00 00 00
corpo      a3 01 03 02 01 07 19 08 00
```

(10 B de cabeçalho + 9 B de corpo = 19 B de mensagem.)

```cbor-diag
{1: 4, 2: 1}
```

```
cabeçalho  22 11 ac 01 3e 03 00 00 00 00 0e 00
corpo      a2 01 04 02 01
```

(12 B de cabeçalho + 5 B de corpo = 17 B de mensagem.)

```cbor-diag
{1: 5, 2: 1, 10: 0}
```

```
cabeçalho  22 11 3f 03 ac 01 00 00 00 00 25 00
corpo      a3 01 05 02 01 0a 00
```

(12 B de cabeçalho + 7 B de corpo = 19 B de mensagem.)

### 9.4 Ativação, A/B, confirmação e rollback

O H escolhe o momento (a ordem e a janela vêm do `mcu-target` da ref/10 §2.6.6, ou da `jk`). A ativação de firmware reinicia o MCU e, durante o reinício, as saídas estão no `safe` do MCU ou no do hardware: o plano do `jk apply` mostra-o.

| | Geração | Firmware |
|---|---|---|
| onde fica | a outra área de geração (`GEN_A` ↔ `GEN_B`) | `FW_STAGING`, para o bootloader (MCUboot ou embassy-boot) |
| `activate` | troca num limite de tick (`jk_generation_swap`), sem reinício; saídas idênticas mantêm valor, leases e trips (A8); `BYE(generation_change)`; novo `HELLO` com a geração em experiência (12.7) | `BYE(ota_activate)`; reinício; o bootloader troca os slots em modo de teste; o firmware novo arranca em experiência (12.4) |
| confirmar | o H observa a saúde durante a janela (N ticks limpos, link `full`, sem trips novos; 01 §16) e envia `confirm`; o D marca a geração como confirmada e responde `state` `confirmed` | idem; o D marca o slot como bom (`mark booted`) |
| sem `confirm` na janela | o D volta à geração anterior, sozinho, num limite de tick (a janela conta no MCU, `jk_service`); `state` `rolled_back`; `J-E2409` | o D reinicia e o bootloader repõe o slot anterior; o `HELLO` seguinte mostra `rolled_back` (12.5) |
| watchdog ou falha durante a experiência | geração anterior | slot anterior |

A janela conta **no MCU**, porque o nó Linux pode ter sido a causa do problema (ADR 0015 aplicada ao MCU). Omissão: 5 min (ref/10 §2.6.6).

**Quem manda o `confirm`.** Se o OTA do MCU faz parte de uma ativação do nó Linux com janela de confirmação (`jk apply --confirm`), o H **não** confirma pela saúde: espera pelo `confirm` da geração do nó (`jk confirm`, ref/09 §5.13), que confirma as duas, e só então envia `confirm` ao MCU (se a saúde do MCU também passou; senão, a resposta ao `jk confirm` diz porquê). A janela do MCU é então a do nó, mais 30 s. Sem janela no nó, e nos OTA pedidos pelo Deck (ref/10 §2.6.6), o H confirma pela saúde, como na tabela.

```cbor-diag
{1: 6, 2: 1, 11: 60000}
```

```
cabeçalho  22 11 ad 01 3f 03 00 00 00 00 0f 00
corpo      a3 01 06 02 01 0b 19 ea 60
```

(12 B de cabeçalho + 9 B de corpo = 21 B de mensagem.)

```cbor-diag
{1: 7, 3: 1}
```

```
cabeçalho  22 11 0c 00 14 00 00 00 00 00 02 00
corpo      a2 01 07 03 01
```

(12 B de cabeçalho + 5 B de corpo = 17 B de mensagem.)

```cbor-diag
{1: 9, 3: 1, 13: 2}
```

```
cabeçalho  22 11 15 00 0c 00 00 00 00 00 03 00
corpo      a3 01 09 03 01 0d 02
```

(12 B de cabeçalho + 7 B de corpo = 19 B de mensagem.)

```cbor-diag
{1: 8, 2: 1, 10: 19, 12: "J-E2403"}
```

```
cabeçalho  22 11 40 03 ad 01 00 00 00 00 26 00
corpo      a4 01 08 02 01 0a 13 0c 67 4a 2d 45 32 34 30 33
```

(12 B de cabeçalho + 16 B de corpo = 28 B de mensagem.)

### 9.5 Anti-rollback

- **Firmware:** o D guarda o maior `security_version` confirmado (o contador de segurança; `min_counter` dos pisos da ref/10 §2.4.4) e recusa um manifesto abaixo dele (`J-E2404`). Descer de versão (SemVer) com o mesmo `security_version` é permitido pelo D; a autorização é do nó e do Deck (`allow_downgrade`, ref/10). O `HELLO` mostra o valor confirmado (chave 5.10).
- **Geração:** o `gen` tem de ser maior do que o da geração confirmada (`min_seq`, ref/08). Um rollback **automático** (sem `confirm`) não passa por aqui: volta à área anterior, que já estava confirmada.

---

## 10. Erros

### 10.1 O `status` das mensagens

Um número pequeno em `ERROR` (chave 1), `RESULT` (chave 4), `HELLO` de resposta (chave 8) e `OTA` (chave 10), com o código do registo ao lado quando ajuda (ref/00 §8). Valores novos só se acrescentam no fim.

| `status` | Nome | Código |
|---|---|---|
| 0 | `ok` | — |
| 1 | `malformed` | `J-E2006` |
| 2 | `unknown_type` | `J-E2007` |
| 3 | `version` | `J-E2201` |
| 4 | `no_session` | `J-E2204` |
| 5 | `not_allowed` (modo ou feature) | `J-E2208` |
| 6 | `unknown_id` | `J-E2310` |
| 7 | `bad_args` | `J-E2311` |
| 8 | `busy` | `J-E2315` |
| 9 | `timeout` | `J-E2314` |
| 10 | `too_large` | `J-E2008` |
| 11 | `permit` | `J-E2312` |
| 12 | `role` | `J-E2313` |
| 13 | `not_output` | `J-E2302` |
| 14 | `idem_key` | `J-E2317` |
| 15 | `cancelled` | `J-E2316` |
| 16 | `not_tripped` | `J-E2320` |
| 17 | `signature` | `J-E2401` |
| 18 | `untrusted_key` | `J-E2402` |
| 19 | `digest` | `J-E2403` |
| 20 | `downgrade` | `J-E2404` |
| 21 | `target` | `J-E2405` |
| 22 | `no_space` | `J-E2406` |
| 23 | `flash` | `J-E2407` |
| 24 | `unknown_xfer` | `J-E2408` |
| 25 | `incompatible_generation` | `J-E2410` |
| 26 | `trace_segment` | `J-E2420` |
| 27 | `refused` | `J-E2207` |
| 28 | `no_generation` | `J-E2206` |
| 29 | `internal` | `J-E2012` |
| 30 | `not_delegated` | `J-E2303` |
| 31 | `catalog` | `J-E2203` |

### 10.2 Códigos de validação (`jk check`, `jk build`): `J0730`–`J0741`

| Código | Gravidade | Mensagem | Onde |
|---|---|---|---|
| `J0730` | error-in-prod | condição de vivacidade não cumprida: o lease de um escritor remoto não aguenta uma renovação perdida | §11.6 |
| `J0731` | error | nível do escritor fora dos níveis delegados ao link nessa saída do MCU, ou nível delegado já usado por um escritor local do MCU | §8.1 |
| `J0732` | warning | tráfego estimado do link acima de 70% da capacidade do transporte | §11 |
| `J0742` | error | tráfego estimado do link acima de 100% da capacidade do transporte: não cabe | §11 |
| `J0733` | error | `max_lease` do link acima do `max_lease` de uma saída do MCU | §8.1 |
| `J0734` | error | `max_frame`, `max_msg`, `window` ou `bulk_max` fora dos limites do transporte | §3.7, §5.6 |
| `J0735` | error | grupo atómico do MCU que não cabe numa trama, ou que mistura saídas do MCU com saídas de outro dono | §6.12 |
| `J0736` | error | endereço de link inválido ou duplicado no mesmo barramento (RS-485: 1–247; CAN: 1–253) | §3.2, §3.4 |
| `J0737` | error | identificadores CAN do link em conflito com outro dispositivo do barramento | §3.4 |
| `J0738` | warning (todos os modos) | Joker Link sem autenticação (`auth = "none"`) num barramento fechado | §4.6 |
| `J0739` | warning | `max_age` de um sinal do MCU menor do que 2 × `refresh` | §6.9 |
| `J0740` | error | caminho do MCU com mais de 8 segmentos debaixo do prefixo do dispositivo | §8.3 |
| `J0741` | error | o MCU tem apps com `requires_clock = "synced"` e o link não tem `TIME` (`time_interval = 0`) | §8.4 |

Códigos de outras referências usados aqui: `J0179` (`auth = "none"` em IP, ref/01), `J0234` (saída de um `joker-link` em `[outputs]` do nó, ref/01), `J0210`, `J0213`, `J0401`, `J0460`, `J0461`, `J0604`, `J0630`, `J0650`, `J0710`.

### 10.3 Códigos de runtime: `J-E2000`–`J-E2499`

Vistos de fora do MCU: no nó Linux (logs, `jk status`, API), nos contadores `joker.device.<d>.*` e nos `jk_link_status` da ABI.

| Código | Gravidade | Mensagem |
|---|---|---|
| `J-E2001` | warning | trama com CRC-32C inválido (contador) |
| `J-E2002` | warning | trama acima do tamanho máximo |
| `J-E2003` | warning | COBS inválido ou trama curta |
| `J-E2004` | warning | ISO-TP: sequência errada, timeout ou overflow |
| `J-E2005` | warning | formato de trama desconhecido (`fmt` ou `kind`) |
| `J-E2006` | error | mensagem malformada: CBOR inválido, bytes a mais, campo obrigatório em falta ou com tipo errado |
| `J-E2007` | warning | tipo de mensagem desconhecido |
| `J-E2008` | error | mensagem remontada acima de `max_msg` |
| `J-E2009` | error | reenvios esgotados numa mensagem fiável: sessão terminada |
| `J-E2012` | error | erro interno do par |
| `J-E2101` | error | autenticação falhou (etiqueta Poly1305 inválida) |
| `J-E2102` | warning | trama repetida ou fora da janela anti-replay |
| `J-E2103` | error | `kid` desconhecido: MCU não enrolado neste nó |
| `J-E2104` | error | trama em claro recusada: a autenticação é obrigatória |
| `J-E2105` | error | contador da sessão esgotado sem atualização de chave |
| `J-E2106` | error | a rotação da chave do link falhou; o MCU voltou à anterior |
| `J-E2201` | error | versão do Joker Link incompatível (sem major comum) |
| `J-E2202` | error-in-prod | o MCU não corresponde ao lock ou à geração do nó: sessão em `observe` |
| `J-E2203` | error | o catálogo não corresponde ao hash do `HELLO` |
| `J-E2204` | warning | sessão desconhecida |
| `J-E2205` | error | link perdido (`link_timeout`) |
| `J-E2206` | error | MCU sem geração válida: sessão em `maintenance` |
| `J-E2207` | error | sessão recusada: identidade não autorizada para este dispositivo |
| `J-E2208` | error | mensagem não permitida no modo da sessão, ou feature não declarada |
| `J-E2209` | error | classe exportada pelo MCU com major incompatível: instância não criada |
| `J-E2302` | error | proposta, `READ` ou `RESET` num sinal que não é saída do MCU, ou com o tipo errado |
| `J-E2303` | warning | lease remoto acima do `max_lease`: limitado; ou nível não delegado ao link |
| `J-E2304` | error | resultado de escrita desconhecido: o `OUTCOMES` não chegou a tempo |
| `J-E2305` | warning | um lease remoto expirou no MCU |
| `J-E2310` | error | identificador desconhecido (`sid`, `mid`, `aid`, `alid`, `iid`, `id`) |
| `J-E2311` | error | argumentos inválidos |
| `J-E2312` | error | ação recusada pelo permissivo |
| `J-E2313` | error | papel insuficiente |
| `J-E2314` | error | timeout da ação |
| `J-E2315` | error | ocupado: fila de ações ou tabela de deduplicação cheia |
| `J-E2316` | warning | ação cancelada |
| `J-E2317` | error | chave de idempotência ausente ou inválida |
| `J-E2318` | error | resultado desconhecido: o link ou o MCU reiniciou antes do `RESULT`; o pedido não foi repetido |
| `J-E2319` | error | escrita não enviada: sessão em `observe` ou `maintenance` |
| `J-E2320` | warning | `RESET` de uma saída que não está em trip |
| `J-E2401` | error | OTA: assinatura inválida |
| `J-E2402` | error | OTA: chave sem o papel exigido para este tipo de artefacto |
| `J-E2403` | error | OTA: o digest da imagem não bate com o manifesto |
| `J-E2404` | error | OTA: abaixo do piso anti-rollback |
| `J-E2405` | error | OTA: alvo ou tipo errado para este MCU |
| `J-E2406` | error | OTA: sem espaço na área |
| `J-E2407` | error | OTA: escrita em flash falhou |
| `J-E2408` | error | OTA: transferência desconhecida ou expirada |
| `J-E2409` | error | OTA: a atualização não foi confirmada na janela; rollback |
| `J-E2410` | error | OTA: geração incompatível com as capacidades do firmware |
| `J-E2420` | error | `TRACE`: segmento inexistente ou não congelado |
| `J-E2430` | warning | `TIME`: amostra rejeitada (atraso acima de `time_max_delay`) |
| `J-E2431` | warning | relógio do MCU em `holdover` ou `unsynced` |

`J-E2010`, `J-E2011`, `J-E2013`–`J-E2099`, `J-E2107`–`J-E2199`, `J-E2210`–`J-E2301`, `J-E2306`–`J-E2309`, `J-E2321`–`J-E2400`, `J-E2411`–`J-E2419`, `J-E2421`–`J-E2429`, `J-E2432`–`J-E2499` ficam livres para esta referência.

---

## 11. Orçamentos

Tamanhos calculados com o codificador de referência (CBOR determinístico, §6.2) e contados byte a byte; tempos no fio com 10 bits por byte (UART 8N1), 11 bits por byte (RS-485 8E1) e, em CAN, o pior caso de bit-stuffing. Os números de CPU e de flash são estimativas a medir na fase do MCU.

### 11.1 Sobrecarga por camada

| Camada | Bytes |
|---|---|
| L1 fluxo | `addr` 1 + CRC 4 + COBS ⌈n/254⌉ + delimitador 1 ≈ 7–10 |
| L1 datagrama | CRC 4 (+ PCI do ISO-TP: 1–2 por trama CAN) |
| L2 | 1 em claro; 25 em sessão; 37 em handshake |
| L3 | 10; 12 com `REL` |

### 11.2 U1: 32 sinais a 1 Mbaud (19 §7)

RP2350 com 16 DI e 16 DO, UART a 1 Mbaud (100 000 B/s), ciclo de 10 ms no Linux, lease 30 ms.

| Mensagem | Corpo | No fio (em claro) | Tempo | No fio (cifrado) | Tempo |
|---|---|---|---|---|---|
| `SAMPLES`, 32 bool do mesmo tick (sem `dt`) | 210 B | 228 B | 2,28 ms | 252 B | 2,52 ms |
| `SAMPLES`, 32 bool de ticks diferentes (com `dt`) | 306 B | 325 B | 3,25 ms | 349 B | 3,49 ms |
| `SAMPLES`, 32 `f32` arbitrários (com `dt`) | 434 B | 453 B | 4,53 ms | 476 B | 4,76 ms |
| `PROPOSE`, 1 escritor, 16 DO | 66 B | 84 B | 0,84 ms | 108 B | 1,08 ms |
| `OUTCOMES`, 16 entradas (primeira vez) | 101 B | 119 B | 1,19 ms | 143 B | 1,43 ms |
| handshake completo (oferta, resposta, `READY`, `ACK`) | — | — | — | ≈ 660 B | ≈ 6,6 ms |

Os ~250 B e ~2,5 ms do 19 U1 confirmam-se para o caso cifrado com as 32 alterações no mesmo tick; com alterações espalhadas pelo período, 3,5 ms. A ocupação no sentido D → H é de 25–35% a cada 10 ms, e no sentido H → D de 8–11%. O UART é full-duplex: os dois sentidos não competem.

### 11.3 CAN a 500 kbit/s

| Caso | CAN clássico (29 bits, ISO-TP) | CAN-FD 500 kbit/s / 2 Mbit/s |
|---|---|---|
| 32 bool com `dt` (321 B com CRC) | 46 tramas + 1 FC ≈ **14,9 ms** | 6 tramas ≈ 2,3 ms |
| idem, cifrado (345 B) | 50 tramas + 1 FC ≈ 16,2 ms | 6 tramas ≈ 2,5 ms |
| 4 sinais (60 B) | 9 tramas + 1 FC ≈ 3,1 ms | 1 trama ≈ 0,4 ms |

**O U1 não cabe em CAN clássico a 500 kbit/s**: 32 alterações a cada 10 ms ocupariam 150% do barramento. Em CAN clássico, o Joker Link serve até ~8 alterações por 10 ms, ou 32 por 50 ms (≈ 30% do barramento). O `jk check` estima o tráfego a partir dos sinais, do `report` e do `refresh`, e dá `J0742` (acima de 70%, só o aviso `J0732`). Com CAN-FD, o U1 cabe com folga.

### 11.4 Outros transportes

- **RS-485 a 115 200 baud (8E1):** o `SAMPLES` de 325 B leva 31 ms. Um ciclo de 10 ms é impossível; serve para `report` ≥ 100 ms, ou passa-se o barramento para 1 Mbaud (os transceptores comuns aguentam 10 Mbaud em distâncias de quadro). Em half-duplex, acrescenta-se o tempo da vez (§3.3).
- **UDP ou TCP em Ethernet:** a banda não conta; a latência sim (sub-ms num switch). **Wi-Fi:** 1–20 ms típicos, com picos de 100+ ms; escritores remotos por Wi-Fi devem ter leases ≥ 1 s, e nunca controlo rápido.

### 11.5 OTA e traço

| Transferência | Tempo |
|---|---|
| firmware de 256 KiB, UART 1 Mbaud, blocos de 256 B | ≈ 3,2 s no fio + escrita em flash (≈ 2–3 s num RP2350) ≈ **6 s** |
| idem, CAN clássico a 500 kbit/s, bulk a 50% do barramento | ≈ 24 s |
| idem, CAN-FD 500 k / 2 M, 50% | ≈ 3,3 s |
| blob de geração de 8 KiB, UART 1 Mbaud | < 0,1 s |
| traço de 16 KiB, UART 1 Mbaud | ≈ 0,2 s |

### 11.6 A condição de vivacidade (`J0730`)

Um escritor remoto tem de aguentar **uma** renovação perdida sem o lease expirar:

```
lease ≥ 2 · renew + T_g + t_bulk + t_prop + 1 ms       (+ report + turn_frames · t_frame, em half-duplex)
```

`renew` = período de renovação (§8.1); `T_g` = período do grupo dono da saída no MCU (a proposta espera pelo latch seguinte); `t_bulk` = tempo no fio de uma trama de `bulk_max` no mesmo sentido (a que pode estar a meio quando a proposta fica pronta; 0 em CAN, onde o bulk tem identificador próprio); `t_prop` = tempo no fio do `PROPOSE`; 1 ms para o driver do nó Linux.

U1 com os valores por omissão: `2·10 + 1 + 3,1 + 1,1 + 1 = 26,2 ms ≤ 30 ms`. Aguenta. Com `bulk_max = 1024` no UART: `2·10 + 1 + 10,8 + 1,1 + 1 = 33,9 ms > 30 ms`, e o `jk check` dá `J0730`. É a razão do `bulk_max` de 256 B por omissão em série.

### 11.7 Memória no MCU

| Estrutura | Com as omissões | Mínimo razoável |
|---|---|---|
| receção (1 trama) | 1,1 KiB | 0,6 KiB |
| mensagens F por confirmar (`window` × `max_frame`) | 4 KiB | 1 KiB (`window` 2, `max_frame` 512) |
| remontagem (`max_msg`) | 4 KiB | 2 KiB |
| `SAMPLES` em voo (8 × sinais / 8) e bits de sujidade | 0,3 KiB (256 sinais) | 0,1 KiB |
| deduplicação (16 × ~144 B) | 2,3 KiB | 1,2 KiB (8) |
| escritores remotos e resultados pendentes | 0,4 KiB | 0,2 KiB |
| estado criptográfico e sessão | 0,5 KiB | 0,5 KiB |
| **total** | **≈ 12,6 KiB** | **≈ 5,7 KiB** |

Cabe nos 48 KB do orçamento do 19 §3.1 com a configuração mínima num M1 pequeno (≈ 12% da RAM do orçamento). Em flash: codec CBOR 3–5 KB, ChaCha20-Poly1305 ≈ 3 KB, SHA-256 e HKDF ≈ 2 KB, verificação Ed25519 8–12 KB (necessária de qualquer modo para os blobs de geração).

---

## 12. Porque CBOR e porque estas mensagens

**A comparação honesta com o postcard** (a codificação serde compacta do ecossistema Rust), para a mensagem mais frequente, `SAMPLES` com 32 entradas. Bytes do corpo:

| Caso | CBOR (este documento) | postcard, valor como `enum` com etiqueta | postcard "tipado" (o tipo vem do catálogo, sem etiqueta nem `Option`) |
|---|---|---|---|
| 32 bool, mesmo tick | 210 | 279 (+33%) | 215 (+2%) |
| 32 bool, `dt` espalhado | 306 | 311 (+2%) | 247 (−19%) |
| 32 `f32` arbitrários | 434 | 407 (−6%) | 343 (−21%) |
| 32 analógicos exatos em meia precisão | 369 | 407 (+10%) | 343 (−7%) |

O postcard tipado ganha até ~20% no corpo, o que no pior caso do U1 são ~90 B, **0,9 ms por trama a 1 Mbaud**. O CBOR ganha quando há omissões (as posições opcionais no fim do array não custam nada) e quando o valor cabe em meia precisão. Em CPU, descodificar 32 entradas CBOR são microssegundos num Cortex-M33: não conta contra milissegundos no fio.

**Porque CBOR, apesar disso:**

1. **Evolução sem quebrar ninguém.** Um leitor CBOR ignora chaves e posições que não conhece; o postcard é posicional e não se descreve, e um campo novo parte os leitores antigos. A regra 2 da ref/00 §1 e a promessa da ADR 0022 (1.0 a falar com 1.x nos dois sentidos) exigem o primeiro.
2. **Duas equipas, uma em C.** Há CBOR maduro e pequeno em C (TinyCBOR, QCBOR, NanoCBOR) e em Rust `no_std` (minicbor). O postcard tem especificação, mas o contrato real é a forma dos `struct` Rust com serde: uma equipa em C teria de a imitar.
3. **Um só codec no MCU.** As assinaturas (COSE_Sign1, ref/00 §7), o blob de geração, o manifesto de OTA e o protocolo do agente (ref/10) já são CBOR. Um segundo formato seria um segundo codec em flash.
4. **Norma e ferramentas.** RFC 8949, notação de diagnóstico, CDDL para validar em conformidade (`spec/cddl/joker-link.cddl`), e qualquer pessoa lê um pacote capturado com uma ferramenta genérica.

A parte do ganho do postcard que vem de "o tipo já é conhecido" foi aproveitada: os valores não levam etiqueta de tipo além da do próprio CBOR, e as omissões não custam bytes.

**Porque estas mensagens:**

- **`PROPOSE`/`OUTCOMES` e não "escrever registo".** O MCU é dono das saídas e tem o seu envelope; o nó Linux é só mais um escritor. O fio transporta o que o núcleo já tem, o buffer de propostas e os `WriteOutcome`, e não inventa um modelo de escrita paralelo.
- **`SAMPLES` por alterações, com retransmissão de estado.** Reenviar uma amostra velha é inútil; reenviar o valor atual de um sinal cuja alteração se perdeu é o que interessa. O `refresh` põe um limite à idade de um sinal que não muda.
- **`CALL` à parte do `PROPOSE`.** As chamadas de ciclo repetem-se a cada tick, como no Linux, e vão no `PROPOSE`. As de fora do ciclo precisam de resposta, e um `toggle()` não pode ser repetido por um reenvio: por isso são fiáveis, e renovam-se com as propostas da expansão.
- **`ACTION` com chave obrigatória.** A ação é o comando perigoso de repetir (17 §2.1), e o link é onde as repetições nascem.
- **`ALARM_CMD`, `RESET`, `RESTART`, `READ`, `CATALOG`.** São as operações que a CLI e a API já têm para os alarmes, trips, reinícios e `param` locais; sem elas, um MCU seria um cidadão de segunda.

---

## 13. Versões e negociação

**`link_version` = 1.0** (ref/00 §1; `[mcu.<d>] link` no lock, ref/02).

1. O D anuncia, na oferta, cada major que fala e a minor mais alta de cada uma: `[[1, 0]]`.
2. O H escolhe a **major mais alta comum** e, nela, **a menor das duas minors máximas**; responde com `[major, minor]`.
3. Sem major comum: resposta `refused` com estado 3 (`J-E2201`), e `J0461` do lado do lock. A resposta usa o formato da major 1, que qualquer D entende.
4. Depois disso, os dois lados comportam-se como essa minor: não enviam tipos de mensagem, chaves ou valores de enum introduzidos depois dela (e toleram-nos se chegarem).

**Congelado para sempre**, em todas as majors: o L1 (COBS, CRC-32C, `addr`, ISO-TP, datagramas); o primeiro byte do L2 e as formas `kind 0` e `kind 1`; a derivação de `kid` e `K_hs`; o cabeçalho L3; o tipo `0x01` e as chaves 1–13 da oferta e 1–10 da resposta, com os seus tipos. É isto que permite a um H da v2 falar com um D da v1 e vice-versa (21 §1).

**Numa minor só se acrescenta** (ADR 0022): tipos novos (primeiro em `0x31`–`0x3F`), chaves opcionais, valores de enum no fim, bits de features, estados no fim do §10.1. Uma chave, tipo ou `status` retirado nunca se reutiliza (ref/00 §1, regra 4). Um `jokerd` 1.x fala com qualquer Joker Micro 1.y: negociam a menor das minors.

**Numa major nova** muda o resto (L3 e mensagens), com um H que fala as duas majors durante toda a série seguinte.

---

## 14. Perfil micro sobre MQTT e CoAP: enquadramento

Um MCU M2 autónomo fala com o Deck o protocolo do agente, na versão reduzida da ref/10 (§2.14, §2.15), e não o Joker Link. Do Joker Link, esse perfil só usa o enquadramento e o código comum. A regra, para ser a mesma nas duas referências:

| | MQTT (5, ou 3.1.1 no perfil micro) | CoAP (RFC 7252) |
|---|---|---|
| unidade | uma mensagem do agente = o payload de um `PUBLISH` | uma mensagem = o payload de um pedido ou resposta; artefactos em Block2 (RFC 7959) de 512 B ou 1 KiB |
| conteúdo | **um** item CBOR: `COSE_Sign1` com etiqueta 18 (ref/10 §3.11.3) | um item CBOR; Content-Format 60 (`application/cbor`) ou 18 (COSE_Sign1) |
| integridade e sigilo | TLS com mTLS + a assinatura COSE | DTLS 1.2 |
| entrega | QoS 1 (duplicados possíveis; a deduplicação é da ref/10) | mensagens `CON` |
| tamanho | ≤ 4 KiB por pacote (ref/10 §2.14) | ≤ 1 KiB por payload fora do Block2 |

- **Não** há COBS, CRC-32C, cabeçalho L2 nem cabeçalho L3: o transporte já dá o enquadramento, a integridade, o sigilo e a entrega. Acrescentá-los seria pagar duas vezes.
- **Comum ao Joker Link:** as regras de codificação do §6.2; a verificação do COSE_Sign1 e o manifesto de OTA do §9.1 (os artefactos `joker.micro-firmware.v1` e `joker.generation.v1` são os mesmos); a ativação A/B com janela de confirmação no MCU (§9.4); a retoma por `offset` do §9.3, aplicada a HTTPS `Range`, CoAP Block2 e `blob/data`.
- A chave do perfil micro é o certificado do MCU (ref/10), e não a `K_link`. Um M2 pode ter, ao mesmo tempo, uma sessão Joker Link por UDP ou TCP com um `jokerd` e o agente com o Deck: são independentes.
- O Joker Link **não** corre sobre MQTT. Leases de dezenas de ms e confirmações por pacote não passam por um broker, e os duplicados do QoS 1 obrigariam a uma segunda deduplicação.

---

## 15. Configuração de que esta referência precisa

As chaves são da ref/01 (anexo A, `[buses]`, `[link]` no §5.22). Esta tabela diz o que o link lê e com que omissões; a sintaxe é a da ref/01.

**No nó Linux, `[devices.<d>]` com `driver = "joker-link"`:**

| Chave | Tipo | Omissão | Validação |
|---|---|---|---|
| `bus` | nome de `[buses]` com `kind = "serial"` ou `"can"` | — | exatamente um de `bus`, `usb`, `address` |
| `usb` | caminho (`/dev/ttyACM0`) | — | |
| `address` | `"udp:<ip>:<porta>"` \| `"tcp:<ip>:<porta>"` (MCUs M2) | porta 7471 | sempre um endereço IP, e sempre o **do MCU**. Em `udp:`, o H só aceita uma sessão cujos datagramas venham deste IP, envia para ele (e para esta porta) o `HELLO(pedido)` enquanto não há sessão, e depois responde à origem do último pacote autenticado (§3.5); o H escuta na 7471. Em `tcp:`, o H não liga para este endereço (quem liga é o D, §3.6): aceita a ligação só deste IP. No manifesto do MCU, `[link] address` é o contrário, o IP e a porta **do nó** (ref/01 §5.22) |
| `addr` | inteiro: o endereço do MCU no barramento | 0 (série), 1 (CAN) | `J0736` |
| `node` | nome esperado no `HELLO` (chave 4.1) | — | senão `refused`, `J-E2207` |
| `can_id_base` | inteiro 0–255 | `0x4A` | `J0737` |
| `auth` | `"required"` \| `"none"` | `"required"` | `J0179` em IP; `J0738` |
| `key` | `"secret:<nome>"` | `"secret:link.<d>"` | |
| `report`, `refresh`, `keepalive`, `link_timeout`, `max_lease`, `rto_min`, `ack_delay`, `time_interval`, `time_tolerance`, `time_max_delay`, `turn_timeout` | `Duração` | §5.6, §8.4, §3.3 | `J0730`, `J0733`, `J0741` |
| `max_frame`, `max_msg`, `window`, `bulk_max`, `retransmit`, `turn_frames` | inteiro | §5.6 | `J0734` |
| `instances` | tabela `<nome no MCU> = "<nome no nó>"`, só para instâncias de topo (§8.3) | os nomes do MCU | `J0604`; instância que só é membro `J0650` |
| `micro` | `Ficheiro`: o caminho do manifesto do MCU (`micro/esp/joker.toml`), relativo ao ficheiro que o declara (ref/01 §7.5) | — | opcional. Com ela, o `jk check` do nó **sem ligação ao nó** gera o catálogo do MCU a partir desse manifesto (o mesmo `jk build` que o MCU usa) e valida as ligações, os níveis delegados (`J0731`), o `max_lease` (`J0733`) e a vivacidade (`J0730`) como se tivesse o `HELLO`; o `jk apply` usa-a para construir o blob de geração do MCU e levá-lo pelo link, quando o hash da geração do MCU difere (ref/08 §2.12.4). Sem ela, o `jk check` só valida o lado do MCU com `--node` (pelo `HELLO` fixado em `[discovered.<d>]`), e o blob faz-se à mão (`jk build --target mcu:<placa> --generation`). Ficheiro inexistente, ou um manifesto cujo `[target] board` não é um MCU: `J0145` |

Em `[buses.<b>]` de `kind = "serial"`: `duplex = "full" | "half"` (omissão `"full"`) e `turnaround`.

Exemplo (U1, o coprocessador do Raspberry Pi):

```toml
[buses.uart1]
kind   = "serial"
port   = "/dev/ttyAMA1"
baud   = 1000000
parity = "none"
duplex = "full"

[devices.io]
driver    = "joker-link"
bus       = "uart1"
auth      = "required"
key       = "secret:link.io"
report    = "5ms"
refresh   = "1s"
bulk_max  = 256
instances = { p1 = "pump_a" }
```

**No MCU, `[link]`** (ref/01 §5.22): `bus` ou `address` (IP e porta **do nó Linux**, M2), `addr` (o endereço do próprio MCU no barramento, 1–247 em RS-485 e 1–253 em CAN), `peer` e `auth`. Em `[outputs.<s>]`, `remote_levels` (níveis delegados ao link, ref/01; omissão: os que nenhum escritor local usa) é a chave 1.8 do catálogo.

---

## 16. Estresse

**L1 — O `jokerd` morre a meio (U1).** O último `PROPOSE` foi consumido em t; o lease de 30 ms expira no latch do MCU em t + 30 ms e a saída passa ao nível seguinte ou ao `safe`. O `link_timeout` (200 ms) só marca o link. No arranque do `jokerd` novo, `HELLO(pedido)`; o MCU obedece ao fim de 100 ms de silêncio; sessão nova em ~7 ms; propostas de volta. **Aguenta**, sem nenhuma regra além de A2.

**L2 — Uma geração nova no nó Linux renumera os escritores.** Com posse por `origin`, o escritor novo no nível 10 levava `NotOwner` até o lease do antigo expirar: 30 ms com a saída a cair de nível. **Quebrou no primeiro desenho.** **Correção:** a posse remota é do nível (§8.1); a unicidade por nível já é garantida pelo `jk check` do nó.

**L3 — Um OTA no mesmo UART do controlo.** Blocos de 1 KiB são 10,8 ms no fio; um `PROPOSE` que fica pronto logo a seguir espera, e com uma renovação perdida o lease de 30 ms expira. **Quebrou.** **Correção:** prioridades de envio (§5.7), `bulk_max` de 256 B em série e a condição de vivacidade `J0730` no `jk check` (§11.6). Em CAN, canal bulk com identificador próprio.

**L4 — Replay em UDP.** Um atacante grava um `PROPOSE` com `do_1 = true` e repete-o mais tarde. Na mesma sessão, a janela anti-replay descarta-o; numa sessão nova, as chaves são outras. Um `HELLO` de resposta gravado não serve, porque o eco do `nonce_d` não bate. **Aguenta.** Num RS-485 em claro não há proteção nenhuma, e isso é declarado (`J0738`).

**L5 — MCU sem TRNG a repetir o `nonce_d`.** Com o nonce repetido depois de um reinício, uma resposta gravada voltava a ser aceite e as tramas antigas da sessão podiam ser repetidas contra o MCU. **Quebrou.** **Correção:** `nonce_d` de um contador persistido **antes** do envio, passado por HMAC (§4.3).

**L6 — Perde-se o `SAMPLES` com a subida de uma entrada.** Por alterações, o sinal nunca mais seria enviado se não voltasse a mudar. **Correção:** retransmissão de estado (§5.4), limitada no tempo pelo `refresh`.

**L7 — `ACTION` repetida depois de uma queda do link.** O H não recebeu o `RESULT` de um `reset` e reenvia-o quando a sessão volta: a deduplicação pela chave devolve o resultado guardado, com `repetido = true`. Se o MCU reiniciou entretanto, a tabela perdeu-se: o H **não** repete, e o ticket fica `J-E2318`. **Aguenta**, com a fronteira declarada.

**L8 — `toggle()` pelo `jk call` com um reenvio.** Um reenvio da mesma chamada anulava-a. **Correção:** `CALL` fiável com `rseq` (exatamente uma vez na sessão), chave opcional entre sessões, e renovação pela expansão devolvida no `RESULT` (§8.5), sem voltar a executar o método.

**L9 — Um técnico grava em campo um blob de geração diferente do fixado.** O `HELLO` mostra outro hash de geração: sessão em `observe`, alarme de deriva, nenhuma escrita do nó em `prod`. O MCU continua a controlar com a sua lógica e o seu `safe`. **Aguenta.**

**L10 — Um MCU com 300 sinais.** O catálogo passa dos 10 KiB e não cabe no `HELLO`. **Correção:** o `HELLO` leva só o hash; o catálogo segue por `CATALOG` em blocos e fica em cache no nó (§6.11). O `SAMPLES` completo depois do `READY` são várias mensagens, ao ritmo do `report`.

**L11 — U1 em CAN clássico a 500 kbit/s.** 15 ms de barramento por lote a cada 10 ms. **Não cabe**, e o `jk check` diz porquê (`J0742`, com o cálculo do §11.3). A resposta é CAN-FD, menos sinais por lote, ou um `report` maior.

**L12 — Um nó 1.3 com um Joker Micro 1.0.** O nó propõe 1.3, o MCU só vai até 1.0: negociam 1.0 e o nó não usa as mensagens novas. O inverso também. **Aguenta** (ADR 0022).

---

## 17. Decisões e alternativas rejeitadas

| Decisão | Alternativas | Porquê |
|---|---|---|
| CRC-32C (Castagnoli) | CRC-32/ISO-HDLC (o "CRC-32" do Ethernet) | melhor distância de Hamming nos tamanhos de trama do link (HD = 6 até ~650 B, contra ~33 B); o sniffer de DMA do RP2350 só faz o CRC-32 do Ethernet, mas o CRC-32C em software (tabela de 1 KiB, ~5 ciclos/B) custa < 0,5% de CPU a 1 Mbaud, e os STM32G4/H5 têm CRC com polinómio programável |
| CRC em todos os transportes | só em série | uma regra; separa ruído de ataque |
| COBS | SLIP, HDLC com *byte stuffing* | sobrecarga limitada (1 B em 254) e conhecida antes de enviar; descodificação no próprio buffer |
| cabeçalho L3 binário | cabeçalho em CBOR | confirmar e ordenar sem descodificar o corpo; 10 B fixos |
| confirmação por `ack` + `ack_bits` em cada pacote | fluxo fiável ordenado para tudo (estilo TCP) | as mensagens de estado não devem ser reenviadas; as fiáveis sim; um só mecanismo serve as três classes |
| retransmissão de estado nos `SAMPLES` | `SAMPLES` fiáveis | reenviar valores velhos gasta banda e atrasa os novos |
| `OUTCOMES` fiáveis | estado, como os `SAMPLES` | cada resultado é uma decisão que entra no registo de decisões; um `ref` fora de ordem resolveria tickets com o resultado errado |
| posse remota por nível | por `origin` | L2 |
| lease em ms, com teto `max_lease` | µs; TTL manual no MCU | 1 ms chega (o menor lease útil é 3 ms); o TTL longo no MCU deixaria um override de 8 h a sobreviver à morte do `jokerd` |
| PSK (`K_link`) com HKDF e nonces dos dois lados | DTLS; ECDH com certificados | cabe em poucos KB, a mesma coisa em todos os transportes, sem PKI no MCU; o sigilo futuro fica para uma 1.x (chave 14 reservada) |
| atualização de chave pelo bit `phase` | handshake novo | um handshake pára as propostas durante ms; a atualização não pára nada |
| segurança configurada nos dois lados, sem negociação | negociar `none` ou `required` | impossibilita o *downgrade* |
| catálogo à parte, por hash | tudo no `HELLO` | o `HELLO` tem de caber em 512 B; o catálogo pode ter dezenas de KiB e raramente muda |
| `CALL` fiável separado do `PROPOSE` | só `PROPOSE` | L8 |
| manifesto COSE antes da imagem | assinatura no fim da imagem | recusar antes de apagar flash; o mesmo manifesto serve o perfil micro |
| janela de confirmação contada no MCU | contada no nó Linux | o nó pode ser a causa da falha |
| uma sessão por MCU na v1 | vários anfitriões | redundância é v2 (21 §8); dois donos de níveis remotos seriam um árbitro distribuído |

---

## 18. Marcações de versão

| Item | Versão |
|---|---|
| tudo o que não está nesta tabela | **1.0** |
| `BLOCKS` (`0x30`): sinais em bloco (M7), vibração e formas de onda do Joker Micro (19 U3) | **reservado 1.1** |
| `at` nas propostas (posição 2 de `prop`): escritas agendadas (M6) | **reservado 1.1** |
| tipos `0x31`–`0x3F` | reservados 1.x |
| `OTA` tipo `model` (3): ML em MCUs | **reservado 1.4** (Preparado para ML; documento 21, §6) |
| `OTA` tipo `app` (4): apps Wasm em MCUs M2 (`wasmi`) | **reservado 1.4** |
| chave 14 do `HELLO`: X25519 efémera (sigilo futuro) | reservado 1.x |
| motivo de qualidade `out_of_distribution` (6) em amostras | já na 1.0 (ref/00 §4.3); só é produzido com ML no MCU (1.4) |
| várias sessões por MCU (ativo/standby, árbitro de quórum num Joker Micro) | v2 (21 §8) |

