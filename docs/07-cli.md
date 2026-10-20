# 07 — CLI (`jk`)

A `jk` é o único ponto de contacto para quem desenvolve e opera, e é também um cliente da API local (documento 01, §14): tudo o que ela faz, uma HMI ou um script também pode fazer. A biblioteca que a `jk` usa é o crate `joker-client` (ref/05, parte B).

**Onde está o detalhe.** Os endpoints que cada comando usa estão na tabela de mapeamento da ref/09, §9.1, e os códigos de saída na ref/09, §9.2. A configuração do cliente, os contextos de nós, o lock, os segredos e a confiança estão na ref/02. Os comandos do agente e do enrolamento pelo Deck estão na ref/10, §2.18. Este documento é o índice dos comandos; onde uma referência define uma opção, vale a referência.

## 1. Princípios

1. **O mesmo comando no portátil e no dispositivo.** `--node <alvo>` (ou `JK_NODE`) aponta para um `jokerd`. Aceita um nome de contexto de `[nodes]` do ficheiro do cliente, `local`, ou um endereço literal: `unix:<caminho>`, `<host>:<porta>` (TCP com mTLS; um nome sem porta, que não seja um contexto, vai à 7443) ou `ssh://[<utilizador>@]<host>[:<porta>]` (túnel SSH até ao socket Unix do nó). A resolução está na ref/02, §3.3.1. Sem alvo, usa a runtime local.
2. **Erros ao estilo do rustc:** código, localização no ficheiro, explicação, ajuda. `jk explain J0107` mostra a página completa.
3. **Nada perigoso sem ser explícito.** Escritas manuais pedem `--ttl` e `--reason` e mostram o envelope antes de aplicar; `apply` em `prod` exige `--confirm` quando a geração mexe em rede, segurança, uplinks ou na própria API (ADR 0015). Os comandos que mudam o nó mostram na primeira linha o nó, o modo e de onde veio a escolha (`→ lisboa-01 (prod) · de JK_NODE`). `--confirm`, `--yes`, `--ttl`, `--reason`, `--level`, `--reveal` e `--allow-dev` **não têm variável de ambiente**, de propósito (ref/02, §2.3.3).
4. **Saída para humanos por omissão e `--json` sempre disponível.** As saídas humanas que aparecem nos documentos e na trilha são **ilustrativas**: o texto, a ordem e o alinhamento podem mudar entre versões sem aviso. O contrato é o `--json`, que é o JSON da API (ref/09) do endpoint que o comando usa (ref/09, §9.1), sem embrulho; nos comandos locais, o da referência do comando. Scripts e testes usam o `--json`.
5. **`--env <variante>`** (ou `JK_ENV`) escolhe o overlay de entrada pela variante: `--env lab` é `--manifest joker.lab.toml` (ADR 0026). É assim que se alterna entre overlays (`--env sim`, `--env rpi`). **`--manifest <ficheiro>`** (ou `JK_MANIFEST`) aceita qualquer caminho (`--manifest sites/lisboa-01/joker.verao.toml`); um ficheiro com nome fora da regra `joker.<variante>.toml` dá o aviso `J0169`. Por omissão, `joker.toml`. As duas opções juntas são um erro de utilização; a precedência entre opções, variáveis e `.joker/config.toml` (`defaults.env`, `defaults.manifest`) está na ref/01 §7.7. Cada manifesto de entrada tem o seu lock na mesma pasta: `joker.lock` para o `joker.toml`, `joker.<variante>.lock` para cada overlay (`--env rpi` → `joker.rpi.lock`); `--lock <ficheiro>` escolhe outro.
6. **Opções globais:** `--node`, `--env`, `--manifest`, `--lock`, `--locked` (nenhum comando escreve o lock; qualquer mudança necessária é `J0411`, é a opção para CI), `--offline` (só a cache local), `--json`, `--color`, `--timeout`, `--cert`, `--key`, `--ca`, `--sign-key`. Precedência: opção > variável `JK_*` > `.joker/config.toml` do projeto > `config.toml` do utilizador > omissão. `jk config show [--origin]` mostra o valor efetivo e de onde veio (ref/02, §2.3).
7. **Pedidos estritos, versões misturadas.** A `jk` pergunta ao nó que versão e que funcionalidades tem e recusa, com uma mensagem clara, uma opção que o nó não suporta (ref/09, §9.3). As opções 1.1 existem na `jk` 1.0 e respondem `J-E1017`.
8. **Nunca se compila no nó.** O `jokerd`, os drivers tier X, as apps e o AOT (`.cwasm`) compilam-se no portátil ou na CI, por build cruzado para o triplo da placa (`aarch64-unknown-linux-gnu`, `armv7-unknown-linux-gnueabihf`, `arm-unknown-linux-gnueabihf` para o ARMv6 do Pi Zero W, `x86_64-unknown-linux-gnu`), e só depois se enviam ao nó (`jk apply`, `jk runtime upgrade`, ou o Deck). O `jk runtime build` e o `jk dev --node` compilam **no portátil** e enviam o resultado; nenhum comando pede um compilador, o `cargo` ou os cabeçalhos do kernel no nó. Uma placa de 512 MiB não compila o `jokerd`, e um nó de produção não deve ter uma toolchain.

---

## 2. Projeto e desenvolvimento

| Comando | Faz |
|---|---|
| `jk init [--name <n>]` | transforma a pasta atual (vazia) num projeto: `joker.toml` mínimo, `sim/`, `.gitignore` e um `.taplo.toml` que associa cada sufixo (`*.profile.toml`, `*.board.toml`, `*.sim.toml`, `*.scenario.toml`, `joker*.toml`…) ao esquema JSON do seu tipo, para completação e erros no editor (ADR 0026; ref/03 §2.1.6). O `jk new` escreve o mesmo |
| `jk new <nome> [--template sim-tank\|bess\|vision\|vibration\|empty]` | cria um projeto numa pasta nova (manifesto, app de exemplo, cenários `sim/`) |
| `jk app new <nome> [--lang rust\|ts\|py]` | cria uma app em `apps/` (TypeScript na 1.0, Python na 1.2; ref/05) |
| `jk check` | valida o manifesto, as ligações, as unidades, os envelopes, a admissão RT (M23), o orçamento dos barramentos, os segredos referidos e as capacidades dos MCUs (`J0710`); com `--node`, também no nó (`dry_run`) |
| `jk fmt [<ficheiro>…] [--check]` | forma canónica dos TOML e dos cenários: literais de duração canónicos, formas de apresentação (`Bad(CommFailure)`) reescritas em `snake_case` (ref/00, §2 e §3.1) |
| `jk build [--target <placa\|triplo>] [--world joker:runtime@<v>]` | compila as apps para Wasm, pré-compila o AOT para o alvo; `--world` gera para um nó mais antigo (ref/05). Os alvos MCU estão no §2.2 |
| `jk dev [--node <alvo>] [--panel]` | corre a runtime (local com `sim`, ou remota) com hot-reload das apps e do manifesto a cada gravação; com `--node`, compila as apps e o AOT no portátil e envia-os ao nó (§1, princípio 8); `--panel` mostra um painel interativo com os sinais `sim` que declaram `panel` (lâmpadas, botões por tecla, sliders) |
| `jk test [cenário…] [--fuzz N]` | testes unitários das apps e cenários de site (`sim/`) com relógio virtual |
| `jk bench [<app>] [--target mcu:<placa>]` | mede o WCET das tarefas de ciclo no alvo e grava-o no `[wcet]` do lock (mínimo de 10 000 execuções; ref/02, §2.1.13). Num MCU, com o contador de ciclos do núcleo: por sonda, ou, numa placa sem sonda (`[probe] tool = "espflash"`), por série (ref/08, §2.12.5) |
| `jk diff [--against <geração\|ficheiro>]` | manifesto efetivo depois dos overlays; diferenças face ao que está a correr |
| `jk explain <chave\|código\|pipeline>` | origem de uma chave (overlay), explicação de um erro, plano de um pipeline |
| `jk explain <caminho de saída>` | o estado do árbitro de uma saída, no nó: grupo dono, níveis ocupados (escritor, valor, lease), nível ativo, envelope e último resultado (`GET /v1/outputs/{path}`, `OutputState`; ref/09, §5.6). Um argumento que é ao mesmo tempo uma chave do manifesto e uma saída é lido como saída; `jk explain 'outputs."<saída>"'` pede a chave |
| `jk inspect <app.wasm\|driver.wasm\|firmware.jkfw\|model.jkm>` | metadados: portas, tarefas, mundo WIT, assinatura, linhagem; o descritor de um driver; o descritor de um firmware (ref/08, §2.14) |
| `jk lsp` | servidor de linguagem para os TOML e os `.jkc` |
| `jk sim <cenário>` | corre só a planta simulada, sem apps (para ver a dinâmica) |

### 2.1 O lock

O detalhe (o que cada opção escreve e a forma canónica) está na ref/02, §2.1.15 e §3.1.

| Comando | Faz |
|---|---|
| `jk lock` | resolve placas, perfis, classes, apps, drivers e MCUs; copia as aprovações (`accepted`); poda entradas órfãs. Nunca sobe uma versão que ainda satisfaz o manifesto |
| `jk lock --node <nó>` | também captura, pelo nó e só com leituras, a descoberta e os fallbacks **em falta**, e escreve `runtime` e `node` |
| `jk lock --update <seletor> --node <nó>` | recaptura os dispositivos indicados (depois de uma deriva) |
| `jk lock --rebind --node <nó>` | aceita um nó diferente do registado e recaptura tudo; mantém o WCET se a placa for a mesma (um template copiado para outro site) |
| `jk lock --upgrade [<referência>…]` | volta a resolver para a versão mais recente dentro da major |
| `jk lock --check` | não escreve; em CI falha com o código 7 se o lock estiver desatualizado ou com deriva |
| `jk lock --offline` | só a cache local; o que não se resolve é `J0411` |

Em `dev`, `jk check`, `jk build`, `jk dev` e `jk test` atualizam sozinhos as secções que não precisam do nó; em `prod`, nunca. `jk apply` nunca escreve o lock.

### 2.2 Joker Micro: build e gravação

| Comando | Faz |
|---|---|
| `jk build --target mcu:<placa>` | firmware com a geração embutida (ELF, UF2 ou bin) |
| `jk build --target mcu:<placa> --generic` | firmware genérico, que lê a geração da flash, e o seu descritor `firmware.jkfw`; traz todos os transportes do `[link] transports` da placa (ref/08, §2.12.1) |
| `jk build --target mcu:<placa> --generation --firmware <firmware.jkfw> [--check-reproducible]` | o blob de geração `<site>.jkgen`, assinado (em `dev`, pode ir sem assinatura, e o firmware aceita-o; em `prod`, `J0779`), no formato e na ISA que esse firmware entende (`J0789` se precisar de mais); `--check-reproducible` constrói duas vezes e compara |
| `jk build --target mcu:<placa> --lib` | modo biblioteca C: `libjoker_node.a`, `joker_node.h` (fixo) e `joker_project.h` (gerado) |
| `jk build --target mcu:<placa> --crate` | modo crate: o crate gerado `joker-project` em `target/joker/crate/` |
| `jk flash [--identity]` | grava o firmware, o blob e, com `--identity`, a área de identidade: por sonda (`probe-rs`) ou por série (`espflash`, no ESP32), conforme o `[probe]` da placa (ref/03 §2.3.3) |
| `jk trace-dump --probe` | lê os anéis de traço da RAM de um MCU por sonda, sem símbolos |

Os modos e os artefactos estão no documento 19, §4, e na ref/08, §2.2, §2.12–§2.14.

## 3. Nó e deploy

Um **nó** é uma placa com `jokerd` (documento 15, §1). A opção `--node` escolhe o nó; os `[devices]` do manifesto são outra coisa (os equipamentos com que o nó fala).

| Comando | Faz |
|---|---|
| `jk node enroll <alvo> …` | enrolamento: identidade P-256 (no TPM se existir) e chave KEM X25519, certificado, âncoras; opções no §3.1 |
| `jk node renew` | renova o certificado já (a chave KEM mantém-se) |
| `jk node trust show` | bundle de confiança ativo, âncoras, versão, chaves por papel |
| `jk node trust push <bundle.cose>` | instala um bundle novo (assinado por uma âncora, `version` maior) |
| `jk node trust set --anchors <keys/…pub>… [--revoked <id>…]` | troca de âncoras, assinada por uma âncora atual; é, com o enrolamento, a única escrita no `node.toml` |
| `jk doctor [--ml] [--rt] [--allow-live]` | verifica o kernel (PREEMPT_RT, `isolcpus`), permissões, grupos, watchdog, relógio, aceleradores (e se são reais), disco, fallbacks dos dispositivos, ficheiros do nó alterados à mão (`J0907`); `--rt` com saídas ativas em `prod` exige `--allow-live` (ref/09, §5.17) |
| `jk apply [--confirm 5m] [--dry-run]` | envia a configuração, os artefactos e os segredos que a geração refere, mostra o plano de diff (o que reinicia, o que recarrega, porque pede confirmação) e aplica |
| `jk publish --manifest <m>… [--registry <oci>] [--sign-key <papel>=<chave>…]` | para uma frota com Deck: para cada manifesto de entrada, calcula a geração do nó (manifesto efetivo, lock, contratos e artefactos por digest; o mesmo descritor e o mesmo hash que o `jk apply` e o Deck calculam, ref/10 §2.6.2), **assina-a** com a chave do papel `config`, assina as apps (`apps`), os drivers W e X (`drivers`) e os blobs de geração dos MCUs (`config`), e publica tudo no registo (omissão: `defaults.registry`), que é o `artifacts.store` do Deck (documento 20, §4.4.2). `--sign-key` repete-se por papel; sem ela, `user.signing_key` para todos. Não fala com nenhum nó. O Deck só distribui artefactos assim assinados: sem eles, regista `J0915` e não emite nada para esse nó. Um papel sem chave, ou uma chave que o bundle não dá a esse papel, faz o `jk publish` falhar antes de publicar (`J0915`, `J0292`) |
| `jk confirm` | confirma a geração pendente e, com ela, o OTA de cada MCU que essa ativação pôs em experiência (geração ou firmware; ref/07 §9.4, ref/09 §5.13) |
| `jk rollback [<geração>]` | reativa uma geração anterior (a mesma, com o mesmo número e o mesmo hash; ref/00, §5) |
| `jk generations` | histórico de ativações (quem, quando, origem, diff, estado) |
| `jk runtime upgrade <versão>` | OTA da runtime (slots A/B) |
| `jk runtime build --features <lista> [--target <placa\|triplo>]` | build personalizado do `jokerd` com drivers e backends tier N (documento 09, ataque 7), **no portátil ou na CI**, por build cruzado para o triplo da placa (§1, princípio 8); o resultado chega ao nó pelo `jk runtime upgrade` (upload `joker.runtime.v1`, ref/09) |
| `jk migrate [--dry-run]` | reescreve manifestos e locks para o esquema da runtime atual (opcional na série 1; obrigatório na 2.0). Também **renomeia os ficheiros de um projeto anterior à ADR 0026** (tipo no nome): perfis (`profiles/x/y@1.toml` → `…@1.profile.toml`), placas (`.board.toml`), modelos de simulação (`sim/esp.toml` → `sim/esp.sim.toml`), cenários (`.scenario.toml`), suites (`.conformance.toml`), pacotes (`package.toml` → `<pacote>.package.toml`), fixtures de drivers (`.fixture.toml`) e overlays (`rpi.toml` → `joker.rpi.toml`, com o lock `rpi.lock` → `joker.rpi.lock`). O tipo de cada ficheiro decide-se pelo conteúdo (a tabela de identificação, ref/03 §2.1.6), e não pela pasta; os ficheiros que não consegue classificar ficam como estão e são listados. Atualiza as referências por caminho (`extends`, `model`, `manifest` dos cenários) e acrescenta o `.taplo.toml` se faltar. Usa `git mv` quando o projeto está num repositório Git, para o histórico seguir os ficheiros. `--dry-run` mostra as mudanças sem as fazer |

### 3.1 Enrolamento, chaves e CA

```
jk node enroll <alvo> --ca <pasta|url> [--name <nó>] [--site <s>] [--org <o>]
               [--anchor <keys/x.pub>]… [--allow-dev] [--tpm required|preferred|off]
               [--code <código>] [--force]
jk node enroll <alvo> --deck <url> --token <t>
jk node enroll mcu:<porta> --deck <url> --token <t>
jk node enroll <mcu> --link <dispositivo> [--probe] [--ca <pasta>]
```

| Opção | Faz |
|---|---|
| `--ca <pasta>` | assina o CSR com uma CA local; se a pasta não existir, cria uma CA de desenvolvimento como `jk ca init --dev` (e o nó fica com `policy.allow_dev = true`). O contexto `[nodes.<nó>]` fica com o endereço de arranque (`ssh://…`); muda-se para `<host>:7443` quando o nó escutar TCP (ref/02, §3.2.3) |
| `--ca <url>`, `--deck <url> --token <t>` | enrola numa CA ou num Deck; a resposta traz as âncoras e o primeiro bundle (ref/10, §3.1.2) |
| `--anchor <.pub>` | âncoras de confiança do nó (repetível) |
| `--allow-dev` | aceitar gerações `mode = "dev"`; um bundle com `min_mode = prod` recusa-as na mesma |
| `--tpm required\|preferred\|off` | onde vive a chave de identidade; `required` falha sem TPM |
| `--code <código>` | o código de uso único que o nó mostra (`jokerd enroll-code`), enviado pelo socket local (acesso físico, ou `ssh://…`): prova que quem enrola teve acesso ao nó, e dispensa ser `root` ou do grupo `joker-admin` no nó (ref/02, §3.2.3). **Nunca há uma porta de enrolamento aberta à rede**; o enrolamento pelo Deck usa o token, e é o nó que liga para fora (ref/09, §2.1) |
| `--force` | volta a enrolar um nó já enrolado, com chave nova (os segredos têm de ser cifrados de novo) |
| `--link <dispositivo>` | enrola um MCU atrás de um nó: gera a `K_link` do dispositivo `joker-link` e grava-a no MCU e no nó (`secret:link.<dispositivo>`; ref/07, §4.7). Com `--ca`, numa placa M2, também gera a chave KEM X25519 do MCU, grava-lhe um certificado da CA com essa chave no SAN e acrescenta o MCU como destinatário `node:<mcu>` do `secrets.toml.enc`: é o que permite levar-lhe segredos, como a palavra-passe do Wi-Fi (`[net.wifi]`, ref/01 §5.22; ref/02 §2.4.8) |
| `--probe` | grava a identidade do MCU por sonda, em vez de pelo link |

| Comando | Faz |
|---|---|
| `jk ca init <pasta> --org <org> [--user <nome>] [--dev]` | cria a CA local (raiz Ed25519, emissora P-256 e o nome da organização; ref/10, §3.1.1) **e** o certificado de utilizador `admin-<utilizador>` (papel `admin` pela regra do nó), que regista em `[user]` do `config.toml` do cliente (ref/02, §3.3.3) |
| `jk ca bundle <pasta> --key <papel>=<pub>… [--deck <id>=<pub> --deck-url <url>] [--min-mode prod] --out <f>` | constrói e assina com a raiz da CA o bundle de confiança (chaves por papel, o Deck, o modo mínimo), que o `jk node trust push` instala e o Deck entrega no enrolamento (ref/02, §2.5.5) |
| `jk key gen <nome> --kind signing\|secrets` | chave de assinatura Ed25519 (artefactos, gerações, segredos, bundles) ou de escrow X25519 (segredos); a privada fica fora do projeto, o `.pub` em `keys/` (ref/02, §2.5) |
| `jk config show [--origin]` | configuração efetiva do cliente e a origem de cada valor |
| `jk config set <chave> <valor> [--project]` | muda uma chave do `config.toml` do utilizador (ou, com `--project`, do `.joker/config.toml`), validada pelo esquema da ref/02 §2.3.2: por exemplo `jk config set nodes.pi.address pi.local:7443` depois do enrolamento (ref/02, §3.2.3, passo 5) |

### 3.2 Segredos

Os segredos vivem no `secrets.toml.enc`, cifrados por destinatário e versionáveis (ADR 0024; ref/02, §2.4).

| Comando | Faz |
|---|---|
| `jk secret set <nome> [--nodes a,b] [--file <f>] [--description <d>]` | pede o valor sem eco (ou lê do stdin ou de `--file`), sobe o `rev`, cifra para os destinatários e assina |
| `jk secret get <nome> --reveal` | decifra com a chave de escrow; sem `--reveal`, recusa |
| `jk secret list` | nomes, `rev`, tipo, nós, destinatários em falta; nunca valores |
| `jk secret rm <nome>` | remove a entrada |
| `jk secret import <ficheiro\|->` | importa um ficheiro em claro (cada entrada é um `set`) |
| `jk secret recipient add --node <nó>` \| `--user <nome> --key <x25519.pub>` | acrescenta um nó (pela chave KEM atestada no certificado) ou uma pessoa de escrow |
| `jk secret recipient rm <destinatário>` | retira um destinatário; **não revoga**: roda-se o valor a seguir |
| `jk secret push --node <nó>` | envia as entradas para o nó sem criar uma geração |
| `jk secret status --node <nó>` | compara nomes e `rev` do ficheiro com os do nó |

### 3.3 Agente e sessões

Falam com a API local do nó, que fala com o Deck (ref/10, §2.18).

| Comando | Faz |
|---|---|
| `jk agent status` | ligação ao Deck, autoridade, `seq`, fila, pausa, espera por alterações locais, sessões |
| `jk agent desired [--json]` | o estado desejado atual, descodificado, e o estado da aplicação |
| `jk agent pause --ttl <d> --reason "…"`, `jk agent resume` | suspende as trocas de geração pedidas pelo Deck (o download e os relatórios continuam) |
| `jk agent import <pasta>` | aplica um estado desejado e artefactos trazidos à mão (sem rede) |
| `jk session list`, `jk session accept <id>`, `jk session close <id>` | sessões remotas pedidas pelo Deck (**1.3**) |

## 4. Operação

| Comando | Faz |
|---|---|
| `jk status [--instances]` | resumo: gerações, grupos (jitter, overruns), dispositivos (link), apps (estado), alarmes ativos, trips; `--instances` lista as instâncias de classes, a origem (perfil, manifesto, app) e a saúde |
| `jk top` | TUI ao vivo com ciclo, drivers, envelope, store, link |
| `jk read <seletor> [--watch]` | lê sinais com qualidade e idade (`jk read 'pcs.**' --watch`) |
| `jk read <seletor> --refresh` | força a leitura de `param` com `poll = "on_demand"` |
| `jk write <saída> <valor> --ttl <t> --reason "…" [--level manual:<n>] [--takeover]` | escrita manual como escritor `user:<principal>`: mostra o envelope, o nível ativo e o resultado previsto, e pede confirmação. O nível por omissão é `manual:4`; `--ttl` e `--reason` são obrigatórios; um nível ocupado por outra pessoa só se toma com `--takeover`, que fica no registo de decisões |
| `jk release <saída> [--level …]` | liberta um nível manual |
| `jk trace <sinal>` | segue um sinal: driver → aquisição → apps que o leem → propostas → árbitro → envelope → commit → readback |
| `jk decisions [--output <s>] [--since 1h \| --from … --to …] [--cause <sinal>] [--report pdf\|csv]` | registo de decisões (M3): cada escrita, com resultado, motivo e as entradas de que dependia; `--cause` filtra as escritas que leram um dado sinal; `--report` gera um relatório de cumprimento (o PDF é feito pela `jk`, não pelo nó) |
| `jk alarms [--active]`, `jk alarm ack <id>`, `jk alarm shelve <id> <ttl> "<motivo>"`, `jk alarm unshelve <id>` | alarmes; o arquivo tem TTL obrigatório, até `max_shelve` |
| `jk trips`, `jk reset <saída> --reason "…"` | trips e reset |
| `jk intents`, `jk intent approve <id>`, `jk intent reject <id>` | intenções de escritores só-proposta (M18, **1.1**; na 1.0 a lista está sempre vazia) |
| `jk action <dispositivo\|instância>.<ação> [args] --key <k>` | invoca uma ação de um dispositivo ou de uma instância (M19) |
| `jk call <instância>.<método> [args] --ttl <t> --reason "…"` | chama um método de classe como escritor `manual` (documento 17, §4.1) |
| `jk app <start\|stop\|restart\|logs> <app>` | ciclo de vida de apps |
| `jk device stop <dispositivo> --reason "…"`, `jk device start <dispositivo>` | para e volta a arrancar o driver de um dispositivo (papel `operator`, auditado): parado, o link fica `stopped`, os sinais `bad(not_connected)` e as saídas com o fallback do dispositivo. É volátil, como o `stop` de uma app: vale até ao próximo arranque do `jokerd` ou à próxima geração (ref/09, §5.12) |
| `jk logs [--app <a>] [--follow]` | logs estruturados |

## 5. Dados

| Comando | Faz |
|---|---|
| `jk history <seletor> --from … --to … [--step 1m]` | consulta o historian |
| `jk export <seletor> --from … --to … --parquet out.parquet` | exporta (o Parquet é escrito pela `jk`) |
| `jk record --for 2h [--groups fast] --out x.jktrace` | grava um traço reproduzível; no início, guarda o estado de cada app (`export-state`) num segmento de estado (documento 16, §7) |
| `jk replay x.jktrace [--app a@versão] [--diff] [--rerun-models]` | reproduz (documento 04, §7), a partir do estado gravado pelo `jk record`; num traço sem estado (`trace-dump`, `trace-from-history`), a partir do `init`, e avisa que os primeiros ticks podem divergir; `--rerun-models` é 1.1 |
| `jk trace-dump --group fast --last 30m --out x.jktrace` | extrai do anel de traço contínuo do grupo (exato nas entradas, mas sem o estado das apps: o replay começa do `init`) |
| `jk trace-from-history <intervalo> --app <a> --out x.jktrace` | reconstrói um traço aproximado a partir do historian |
| `jk link status` | uplinks: ligação, lag do cursor, orçamento (M12), perdas |

## 6. Sombra e ML

| Comando | Faz | Versão |
|---|---|---|
| `jk shadow start <app> <candidato>`, `jk shadow status`, `jk shadow diff`, `jk shadow promote`, `jk shadow abort` | modo sombra de apps (M16) | 1.0 |
| `jk shadow … <modelo>` | modo sombra de modelos | **1.1** |
| `jk model build <onnx\|dir> [--via burn\|ort] [--arch joker.tcn@2 --weights w.safetensors]` | cria o `.jkm`; com `--via burn` lista os operadores em falta | **1.1** |
| `jk model inspect <m.jkm>` | assinatura, guarda, backends disponíveis, linhagem | **1.1** |
| `jk model bench <m.jkm> [--node …] [--backend …]` | latência e memória por backend no alvo | **1.1** |
| `jk model lineage <m>` | árvore de linhagem (M10) | **1.1** |
| `jk model migrate <m.jkm> --burn <versão>` | migra os pesos entre versões do Burn | **1.1** |
| `jk train <job> [--now]`, `jk train status` | treino no dispositivo | **1.1** |

## 7. Descoberta, perfis, placas e drivers

| Comando | Faz |
|---|---|
| `jk discover <uri>` | descoberta só de leitura (documento 05, §7) |
| `jk profile import <sunspec\|nodeset\|eds\|esi> <ficheiro>` | gera um perfil para revisão |
| `jk profile import csv <ficheiro> --name <fabricante>/<modelo> --driver modbus [--out <ficheiro>] [--modicon]` | gera um **rascunho** de perfil a partir de um mapa de registos em CSV (colunas `signal,fc,at,type,direction,unit,scale,offset,label,description`); o que o CSV não diz fica marcado `# REVER`; `--modicon` lê o `at` na notação antiga de base 1 (`30002` → registo 1). Formato e regras na ref/03 §2.2.19 |
| `jk profile check <perfil>` | valida um perfil sem equipamento (esquema, unidades, `plan` do driver) |
| `jk profile diff <a> <b>` | compara duas versões e diz que parte da versão tem de subir (ref/03, §2.2) |
| `jk profile test <perfil> --uri <uri> [--node <alvo>]` | lê todos os sinais do perfil de um equipamento real e mostra valores, unidades, qualidade, latência medida e erros; **nunca escreve**. A URI diz o protocolo, a porta ou o endereço e a unidade: `modbus-rtu:<porta>?baud=<b>&unit=<u>`, `modbus-tcp://<host>:<porta>?unit=<u>` (gramática completa na ref/03 §2.2.20). Sem `--node`, o driver corre no portátil |
| `jk profile publish <perfil>` | publica no registo (exige `jk profile diff` e relatórios de conformidade, ref/03 §2.8) |
| `jk board show <placa>` | o perfil de uma placa: IO fixo, portas, watchdog, capacidades, sonda |
| `jk board probe --node <nó> [--name <placa>] [--out <ficheiro>]` | lê o inventário de hardware do nó (`GET /v1/node/hardware`, só leituras) e escreve um **rascunho** de perfil de placa (omissão: `boards/<placa>.board.toml`); o que o inventário não pode saber (nível, kernel alvo, `fallback` do IO fixo, `recommended`) fica marcado `# REVER` (ref/03 §2.3.7) |
| `jk board check <ficheiro> [--node <nó>]` | valida um perfil de placa (`J0800`–`J0849`); com `--node`, confirma no nó que a arquitetura, as CPUs, os periféricos, as linhas e os caminhos declarados existem (`J0828`, `J0829`) e lista os `# REVER` que faltam resolver |
| `jk board new <nome> --from <placa>` | copia um perfil de placa existente (do projeto ou do registo) para `boards/<nome>.board.toml`, com `name` novo, `version = "1.0.0"` e `tier = "dev"` marcado `# REVER` |
| `jk board publish <ficheiro>` | publica uma placa no registo (sem `# REVER` por resolver; ref/03 §2.3.7 e §2.8.4). Uma placa `tier = "dev"` nunca se publica (`J0848`) |
| `jk driver test <ref> [--fixture <f>] [--case <glob>] [--report <out.cbor>]` | suite de conformidade de drivers contra um driver N, W (`w:./x.wasm`) ou X (`x:./binário`) (ref/06, §6) |
| `jk driver meta <driver>.driver.toml` | gera o descritor CBOR de um driver a partir de TOML (para drivers C) |

## 7.1 Classes (M24)

| Comando | Faz |
|---|---|
| `jk class list [--std\|--project]` | classes disponíveis, com versões |
| `jk class show <classe>` | membros, capacidades, métodos, consultas, normas de referência |
| `jk class new <nome>@<major>` | esqueleto de classe + ficheiro de conformidade |
| `jk class verify <classe> --instance <i> [--allow-actuation] [--report <f>]` | corre a suite de conformidade da classe contra uma instância. Numa instância que não é simulada exige `--allow-actuation` (`J0640`) e `mode = "dev"`, recusa-se se houver um nível `safety` ou `manual` ativo nas saídas da instância (`J0641`), e escreve em `manual:3` com TTL igual à duração da suite: as apps continuam a correr e ficam `overridden` durante o teste (ref/03, §2.6.3); `--report` escreve o relatório que a publicação exige (ref/03, §2.6 e §2.8) |
| `jk class publish <ficheiro>` | publica no registo (exige conformidade e corre o `jk class diff`) |
| `jk class fmt\|check\|diff\|doc\|gen` | formatar, validar (JC-codes), comparar versões (documento 17, §9), gerar documentação e tipos |
| `jk explain <instância>` | mapeamento de cada membro, origem (perfil/manifesto/app), latência das camadas virtuais; numa instância composta, a árvore dos membros de classe (instância ligada, classe, origem e saúde de cada nó, indentados), a profundidade (`JC0501`) e as camadas (`J0620`), e as composições de que a instância é membro (`GET /v1/instances/{i}`, ref/09 §5.11; exemplo no documento 14, §9.3) |

Fora da v1.0: `jk ui` (Joker Studio, 1.3) e `jk class extract` (depois da v1).

## 8. Códigos de saída

| Código | Significado |
|---|---|
| 0 | sucesso |
| 1 | erro de utilização |
| 2 | validação falhou (J-codes) |
| 3 | o dispositivo recusou (permissões, assinatura, envelope) |
| 4 | timeout ou dispositivo inacessível |
| 5 | confirmação expirou e houve rollback |
| 6 | teste ou cenário falhou |
| 7 | deriva detetada (`jk lock --check`) |

A correspondência com as respostas da API está na ref/09, §9.2.

## 9. Fluxos completos

**Do portátil ao campo:**

```sh
jk new site-porto --template bess
cd site-porto
jk dev                                      # planta simulada; itera na app
jk test                                     # cenários sim/ contra o manifesto real
jk node enroll revpi-porto.local --ca ~/org-ca
jk doctor --node revpi-porto.local --rt   # kernel, isolcpus, watchdog, fallbacks
jk discover sunspec://192.168.10.30 --node revpi-porto.local
jk lock --node revpi-porto.local          # fixa mapas descobertos, firmware, fallbacks
jk bench --node revpi-porto.local         # WCET real → lock
jk check                                    # admissão RT com WCET medido
jk secret recipient add --node revpi-porto # cifra os segredos também para este nó
jk apply --node revpi-porto.local --confirm 10m
jk status --node revpi-porto.local
jk confirm --node revpi-porto.local
```

**Um incidente:**

```sh
jk decisions --output pcs.p_set --since "2026-09-24 03:00" --to "03:30"
jk trace-from-history "03:00..03:30" --app peak_shaver --out incidente.jktrace
#   ↑ traço aproximado: só é exato se as entradas da app estavam gravadas ao ritmo do grupo
#     (o `trace.continuous` do grupo guarda um anel de traço exato das últimas N horas)
jk replay incidente.jktrace --diff --app peak_shaver@1.4.3
```

**Uma versão nova de uma app, em sombra (1.0):**

```sh
jk shadow start peak_shaver peak-shaver-1.5.0.wasm --node linha2
jk shadow status peak_shaver --node linha2   # 48 h depois
jk shadow promote peak_shaver --node linha2
```

**Um modelo novo (1.1):**

```sh
jk model build yolo-parts-v8.onnx --via ort
jk model bench yolo-parts-v8.jkm --node linha2 --backend hailo,burn:cpu
jk shadow start detector yolo-parts-v8.jkm --node linha2
jk shadow status detector --node linha2   # 48 h depois
jk shadow promote detector --node linha2
```

**Uma placa que ainda não tem perfil, e um equipamento com mapa de registos** (ref/03 §2.3.7 e §2.2.19):

```sh
jk board probe --node zero --name rpi-zero-w           # rascunho em boards/rpi-zero-w.board.toml, com # REVER
jk board check boards/rpi-zero-w.board.toml --node zero      # depois de resolver os # REVER
jk profile import csv mapa.csv --name local/xy-md02 --driver modbus
jk profile test local/xy-md02@1 --node zero --uri "modbus-tcp://192.168.1.50:502?unit=1"
jk check --env zero                                      # joker.zero.toml, lock joker.zero.lock
```

**Um MCU coprocessador, com firmware genérico:**

```sh
jk build --target mcu:rp2350 --generic          # firmware.uf2 + firmware.jkfw
jk flash --identity                                      # por sonda, uma vez
jk node enroll pump-mcu --link pump_mcu --node revpi-porto.local
jk build --target mcu:rp2350 --generation --firmware firmware.jkfw
jk apply --node revpi-porto.local --confirm 10m          # o nó leva o blob ao MCU pelo link
```
