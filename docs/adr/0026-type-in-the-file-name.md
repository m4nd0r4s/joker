# ADR 0026 — O tipo de um ficheiro está no nome

**Estado:** aceite (v1) · **Data:** 2026-09 · **Substitui** a decisão da ref/03 de que "o tipo de um ficheiro vem de quem o refere"

## Contexto

Um projeto tem muitos ficheiros TOML de tipos diferentes: manifestos e overlays, perfis de equipamento, perfis de placa, modelos de simulação, cenários, suites de conformidade, pacotes de classes. O tipo de cada um só se sabia pela pasta onde estava ou por quem o referia. Quem abre um `esp.toml` ou um `rpi.toml` não sabe se está a ler um perfil, uma simulação ou um overlay; na trilha havia um `sim/pi.toml` (simulação) e um `rpi.toml` (overlay) lado a lado.

## Decisão

1. **Cada ficheiro diz o seu tipo no nome**, na forma `<nome>.<tipo>.toml`:

   | Tipo | Nome | Exemplo |
   |---|---|---|
   | manifesto | `joker.toml` | `joker.toml`, `micro/esp/joker.toml` |
   | overlay | `joker.<variante>.toml` | `joker.sim.toml`, `joker.lab.toml`, `templates/joker.bess-v2.toml` |
   | lock | `joker.lock`, `joker.<variante>.lock` | `joker.lab.lock` |
   | perfil de equipamento | `<modelo>@<major>.profile.toml` | `profiles/local/bench-esp32@1.profile.toml` |
   | perfil de placa | `<placa>.board.toml` | `boards/rpi-zero-w.board.toml` |
   | modelo de simulação | `<nome>.sim.toml` | `sim/esp.sim.toml`, `classes/std/valve.sim.toml` |
   | cenário | `<nome>.scenario.toml` | `sim/scenarios/falha_esp.scenario.toml` |
   | suite de conformidade | `<nome>.conformance.toml` | `classes/local/status_light.conformance.toml`, `conformance/envelope/rate.conformance.toml` |
   | pacote de classes | `<pacote>.package.toml` | `classes/acme/water/water.package.toml` |
   | fixture de drivers | `<driver>.fixture.toml` | `drivers/modbus/conformance/modbus-tcp.fixture.toml` |
   | descritor de driver (fonte de `jk driver meta`) | `<driver>.driver.toml` | `joker-drv-acme.driver.toml` |
   | classe | `<nome>.jkc` | (sem mudança) |
   | nó, cliente, Deck, frota, segredos, registo de códigos | nomes fixos | `node.toml`, `config.toml`, `deck.toml`, `fleet.toml`, `secrets.toml.enc`, `codes.toml` |

2. **O conteúdo tem de bater com o sufixo.** Cada tipo reconhece-se pela sua tabela de identificação: `[profile]` (perfil), `[board]` (placa), `[scenario]` (cenário), `[suite]` (conformidade), `[package]` (pacote) e `[fixture]` (fixture de drivers), todas obrigatórias. Três tipos não têm tabela obrigatória, e reconhecem-se pelas chaves que os caracterizam: o **descritor de driver** tem `name`, `tier` e `protocols` na raiz (ref/06 §2.3); o **modelo de simulação** (`[model]` é opcional) tem pelo menos uma de `[model]`, `[signals]` ou `[plant]`; uma **camada de manifesto** (`[project]` só é obrigatória no manifesto efetivo, ref/01 §2.3) só tem secções do manifesto (ref/01 §4) e não tem `schema` na raiz. Um ficheiro sem a tabela do seu sufixo, ou com a tabela de identificação de outro tipo, é `J0803` (ref/03 §2.1.6).
3. **As referências não mudam.** O manifesto continua a escrever `profile = "local/bench-esp32@1"`, `board = "rpi-zero-w"` e `model = "sim/esp.sim.toml"`: a resolução de perfis e de placas acrescenta o sufixo; os caminhos de ficheiro escrevem-se inteiros.
4. **Os overlays escolhem-se pela variante.** `jk <comando> --env lab` (ou `JK_ENV=lab`) é o mesmo que `--manifest joker.lab.toml`, e o lock correspondente é sempre `joker.lab.lock`. A regra `joker.<variante>.toml` vale em qualquer pasta (`templates/`, `sites/<site>/`), e um `extends` só aceita ficheiros com esse nome. O `--manifest` continua a existir para caminhos arbitrários: um manifesto de entrada com nome fora da regra é aceite com o aviso `J0169` (ref/01 §7.7).
5. **Os editores sabem o esquema pelo nome.** A `jk` publica uma configuração do taplo que associa `*.profile.toml`, `*.board.toml`, `*.sim.toml`, `*.scenario.toml`, `*.conformance.toml`, `*.fixture.toml`, `*.driver.toml`, `*.package.toml` e `joker*.toml` ao esquema JSON de cada tipo, com completação e erros no editor. O `jk init` e o `jk new` escrevem essas associações num `.taplo.toml` na raiz do projeto (ref/03 §2.1.6), e o `jk migrate` acrescenta-o a um projeto antigo.

## Razões

- Um ficheiro percebe-se sem contexto: numa listagem, numa revisão de PR, numa pesquisa.
- Já era a forma do `.conformance.toml` e dos simuladores de classe (`.sim.toml`); passa a ser a regra.
- O sufixo bate com os tipos de media do Hub (`joker.profile.v1+toml` ↔ `.profile.toml`).
- Os manifestos de um site ficam juntos numa listagem (`joker.*`).

## Consequências

- Nomes mais compridos.
- Renomeação de todos os exemplos, da trilha e das referências (feita nesta revisão).
- `jk migrate` renomeia os ficheiros de um projeto antigo e atualiza os `extends`, os `model =` e os `manifest =` dos cenários (documento 07, §2).
- A verificação conteúdo ↔ sufixo passa a ser o `J0803`, que antes cobria "ficheiro referido no sítio errado"; o caso antigo continua lá (um `model =` que aponta para um `.profile.toml` já falha pelo sufixo).
