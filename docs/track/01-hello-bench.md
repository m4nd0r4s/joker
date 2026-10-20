# Parte 1 — Olá, bancada: um LED no simulador

**Objetivo:** partir de uma pasta vazia e acabar com a app `luz` a piscar o LED `ACT` de um Raspberry Pi **simulado**, com testes da app e um cenário do site que o provam.

**Vais aprender:** projeto e manifesto · overlays desde o primeiro dia · o tipo no nome dos ficheiros · dispositivo `sim` · sinal, porta e ligação · saída com `safe` · grupo de ciclo · hot-reload de parâmetros e de código · `jk read` e `jk trace` · `joker-test` e relógio virtual · cenários.

**Antes de começar:** o portátil com Rust estável e a `jk` instalada (`jk --version` responde `jk 1.0.0`). Não há hardware nesta parte.

---

## 1. Pasta vazia → projeto

```sh
$ mkdir joker-bancada && cd joker-bancada
$ git init -q
$ jk init --name bancada
```

```
criado joker.toml
criado sim/
criado .gitignore            (ignora target/ e .joker/)
próximo passo: jk app new <nome>
```

O `joker.toml` que o `init` escreve tem só o projeto:

```toml
[project]
name  = "bancada"
joker = "1.0"           # versão mínima da runtime que aceita este manifesto
mode  = "dev"           # dev: avisos em vez de erros para o que só importa em produção
```

A omissão de `mode` é `prod` (ref/01 §5.2): esquecer a chave num site real não pode desligar as assinaturas. Por isso o `init` escreve `dev` explicitamente.

## 2. Criar a app

```sh
$ jk app new luz --lang rust
```

```
apps/luz/
  Cargo.toml            depende de `joker-sdk` (a API), `joker-build`, e `joker-test` nos testes
  build.rs
  src/lib.rs
```

## 3. Um Pi simulado

O dispositivo vai chamar-se `pi` no simulador e no hardware. É isso que faz o caminho `pi.act_led` existir nos dois mundos, e é por isso que a app e o manifesto comum nunca vão mudar quando passares para a placa.

Por agora, o Pi simulado tem um único sinal: o LED `ACT`.

```toml
# sim/pi.sim.toml
schema = 1

[model]
description = "Raspberry Pi da bancada, simulado: por agora, só o LED ACT"

[signals]
act_led = { type = "bool", direction = "output", label = "LED ACT", panel = { label = "ACT", kind = "lamp" } }
```

- `direction = "output"`: alguém escreve este sinal. O valor que o modelo recebe é o **aplicado**, depois do árbitro e do envelope, e nunca a proposta crua da app (ref/03 §2.4.4).
- `panel = { kind = "lamp" }`: no painel do `jk dev`, aparece como uma lâmpada. Uma lâmpada mostra e nunca muda o valor.

## 4. O manifesto, em duas camadas desde já

O manifesto separa **o que o sistema faz** (a app, a saída e o seu `safe`, o ciclo) de **onde está o hardware** (os dispositivos). O primeiro vai para `joker.toml`; o segundo para um overlay por alvo.

```toml
# joker.toml — o comum. Nenhum dispositivo aqui.
[project]
name  = "bancada"
joker = "1.0"
mode  = "dev"

# Toda a saída diz o que é "seguro". Para um LED, apagado.
[outputs."pi.act_led"]
safe = false

# Um grupo de ciclo: um relógio que bate a cada 20 ms.
[cycles.main]
period = "20ms"

# A app, e a ligação da sua porta `led` ao sinal `pi.act_led`.
[apps.luz]
path   = "apps/luz"
bind   = { led = "pi.act_led" }
params = { half_period = "500 ms" }
cycles = { pisca = "main" }
```

```toml
# joker.sim.toml — tudo simulado, no portátil
extends = ["joker.toml"]

[devices.pi]
driver = "sim"
model  = "sim/pi.sim.toml"
```

**Porquê o overlay já na parte 1?** Porque na parte 2 vais acrescentar `joker.lab.toml`, com o Pi 3 real, e não vais mexer em mais nada. Se o dispositivo simulado estivesse no `joker.toml`, terias de o tirar de lá, e o `joker.toml` deixava de ser "o sistema" para passar a ser "o sistema no portátil". Com duas camadas desde o início, cada alvo é um ficheiro pequeno que só diz onde está o hardware (ref/01 §7).

**Os nomes dizem o tipo.** Cada ficheiro do projeto leva o tipo no sufixo: `joker.toml` é o manifesto, `joker.<variante>.toml` um overlay, `sim/pi.sim.toml` um modelo de simulação. Com o sufixo, o editor sabe o esquema e dá completação e erros enquanto escreves, e o `jk check` recusa com `J0803` um ficheiro cujo conteúdo não bate com o nome (ADR 0026). A variante é também o que escolhes na linha de comandos: `--env sim` quer dizer `joker.sim.toml`.

Três coisas a reparar:

- **`pi.act_led` é um caminho**: `<dispositivo>.<sinal>`. A app não o conhece. Conhece a sua **porta** `led`, e é o manifesto que faz a **ligação** (documento 02, §9).
- **`safe = false` não é opcional.** Não existe um "valor seguro por omissão" (ADR 0006). O `safe` é uma decisão sobre o sistema, e por isso vive no comum, e não no overlay do hardware.
- **Não declarámos `writer`.** Com um só escritor na saída, vale a omissão: nível `control:12` e um lease de 3 períodos do grupo dono, 60 ms (documento 16, A9 e A11; ref/01 §3.11). Isto só passa a importar na parte 2.

## 5. A app `luz` 0.1

```rust
// apps/luz/src/lib.rs
use joker::prelude::*;

/// Pisca um LED. Versão 0.1 da trilha.
#[joker::app(name = "luz", version = "0.1.0")]
pub struct Luz {
    /// O LED. A app só sabe que é um bool de saída.
    #[output]
    led: Output<bool>,

    /// Meio período do pisca: 500 ms aceso, 500 ms apagado.
    #[param(unit = "ms", range = "20..5000", default = 500.0)]
    half_period: Param<f64>,
}

/// Estado local da tarefa de ciclo: vive entre ticks, na memória da app.
#[derive(Default)]
struct Pisca {
    elapsed_ms: f64,
    on: bool,
}

#[joker::cycle(period = "20ms", wcet = "5us")]
fn pisca(io: &mut CycleIo<Luz>, st: &mut Pisca) -> Result<()> {
    // O período real do grupo (io.dt()), e não uma constante: o manifesto pode mudá-lo.
    st.elapsed_ms += io.dt().as_secs_f64() * 1000.0;

    if st.elapsed_ms >= io.half_period.get() {
        st.elapsed_ms -= io.half_period.get();   // subtrair, e não pôr a zero, não acumula erro
        st.on = !st.on;
    }

    // Propõe o valor a cada tick. Isso renova o lease: se a app parar, o LED vai para `safe`.
    io.led.set(st.on);
    Ok(())
}
```

Porque é que se escreve o LED a cada tick, e não só quando muda? Porque **propor é renovar o lease** (documento 16, A2). O Joker não envia nada ao driver quando o valor aplicado não muda, mas fica a saber que a app está viva. É isto que garante que, se a app morrer, o LED vai para `safe` em 60 ms.

O `period` da macro é um **pedido**: quem manda é o grupo a que o manifesto liga a tarefa (`cycles = { pisca = "main" }`). Se forem diferentes além da tolerância da app, o `jk check` avisa com `J0130`.

## 6. Verificar

```sh
$ jk check --env sim            # o mesmo que --manifest joker.sim.toml
```

```
✓ manifesto          joker.sim.toml ← joker.toml · 1 dispositivo (sim), 1 saída, 1 app, 1 grupo
✓ ligações           luz.led → pi.act_led (bool)
✓ parâmetros         luz.half_period = 500 ms (20..5000 ms)
✓ admissão RT        main 20ms: 5 µs declarados (0,03%)
aviso[J0352]: o grupo `main` (20 ms) não tem kernel PREEMPT_RT neste alvo; jitter não garantido
   = nota: no portátil, com o simulador, é irrelevante
nota: sem `[target] board`, as verificações que dependem da placa ficam por fazer
nota: joker.sim.lock atualizado (apps.luz)
```

Repara na última linha: cada manifesto de entrada tem o seu lock na mesma pasta (`joker.sim.toml` → `joker.sim.lock`), e em `dev` o `jk check` mantém-no atualizado sozinho (ref/02 §2.1.1 e §3.1.2). Versiona-o.

### O que o `jk check` diz quando algo está mal

Sem `--env` (nem `--manifest`), a `jk` usa o `joker.toml`, que não tem dispositivos:

```sh
$ jk check
```

```
erro[J0101]: sinal desconhecido `pi.act_led`
 --> joker.toml:8:10
  |
8 | [outputs."pi.act_led"]
  |          ^^^^^^^^^^^^ nenhum dispositivo `pi` neste manifesto
  = nota: `joker.toml` é estendido por joker.sim.toml
  = ajuda: jk check --env sim
```

É o comportamento certo: o comum sozinho não é um sistema. Para não escreveres `--env sim` a toda a hora, põe a omissão na configuração local do projeto (fica em `.joker/`, que não se versiona; ref/02 §2.3.2):

```toml
# .joker/config.toml
version = 1

[defaults]
manifest = "joker.sim.toml"
```

Agora apaga a linha `safe = false` do `joker.toml` e volta a verificar:

```
erro[J0201]: a saída `pi.act_led` não tem `safe`
 --> joker.toml:8:1
  |
8 | [outputs."pi.act_led"]
  | ^^^^^^^^^^^^^^^^^^^^^^ esta saída não diz o que é seguro
  = nota: não existe valor seguro por omissão (ADR 0006)
  = ajuda: para um LED, `safe = false`
```

Repõe a linha.

## 7. Correr

```sh
$ jk dev --panel --env sim
```

```
joker 1.0.0 · dev · bancada · geração 1 · alvo local (sim)
┌ pi (sim) ──────────────────────────────────┐
│ ACT        ●                               │   ← alterna a cada 500 ms
└────────────────────────────────────────────┘
 grupos   main 20ms   jitter p99 0,7 ms   overruns 0
 apps     luz 0.1.0  a correr  wcet medido 1,6 µs
 saídas   pi.act_led = true  (accepted · control:12 · app:luz)
 [q] sair  [r] recarregar  [t] trace pi.act_led  [?] ajuda
```

**Hot-reload de parâmetros.** Com o `jk dev` a correr, muda o parâmetro no `joker.toml` e grava:

```toml
params = { half_period = "100 ms" }
```

O `jk dev` aplica uma geração nova (geração 2). O LED passa a piscar a 5 Hz **sem reiniciar a app**: o valor novo chega à instância de ciclo na imagem do tick seguinte (ref/04 §4.3.4). Experimenta `"10 ms"`: a geração é recusada com `J0120` (fora da gama `20..5000` que a app declara), e a geração 2 continua a correr. Volta a `"500 ms"`.

**Hot-reload de código.** Muda a app, por exemplo para um pisca assimétrico (`if st.on { … }` com dois tempos), e grava. O `jk dev` recompila o componente e troca-o entre dois ticks. O estado da tarefa recomeça em `Default` (LED apagado, contador a zero), porque `Pisca` não tem `#[joker::state(persist)]`; com ele, a troca migraria o estado (ref/04 §4.12). A versão anterior fica quente durante `keep_previous` (10 min), para voltar atrás sem recompilar. Desfaz a mudança antes de continuar.

## 8. Ver por dentro

Noutro terminal, com o `jk dev` a correr:

```sh
$ jk read pi.act_led --watch
```

```
pi.act_led   true    good   idade 4 ms    control:12 · app:luz
pi.act_led   false   good   idade 2 ms    control:12 · app:luz
…
```

```sh
$ jk trace pi.act_led
```

```
tick 1250  t=25.000s  main
  app:luz.pisca     propõe true   (control:12, lease até 25.060s)
  árbitro           nível ativo control:12 → true
  envelope          accepted
  commit            true → driver sim (mudou)
tick 1251  t=25.020s  main
  app:luz.pisca     propõe true   (control:12, lease até 25.080s)
  árbitro           nível ativo control:12 → true
  envelope          accepted
  commit            — (igual ao aplicado)
```

O `jk trace` segue um sinal pela cadeia toda: driver, aquisição, apps que o leem, propostas, árbitro, envelope, commit, readback (documento 07, §4). Numa saída, é a primeira ferramenta quando alguma coisa não faz o que esperavas.

## 9. Testes da app, com relógio virtual

Um LED a piscar parece não precisar de testes, mas este mostra o que o Joker dá de graça: **relógio virtual**. Dez segundos correm em milissegundos, de forma determinista, com o árbitro e o envelope reais (ref/04 §6.1).

```rust
// apps/luz/src/lib.rs (no fim)
#[cfg(test)]
mod tests {
    use super::*;
    use joker_test::prelude::*;

    #[test]
    fn pisca_a_1hz() {
        let mut h = Harness::<Luz>::new()
            .group("pisca", 20.ms())
            .param("half_period", 500.0);

        h.run_for(10.s());

        let led = h.output("led");
        assert_eq!(led.toggles(), 20);                  // 10 s a 1 Hz = 20 mudanças
        assert!(led.toggles_every(500.ms()));           // exato: o relógio é virtual
    }

    #[test]
    fn parar_a_app_apaga_o_led() {
        let mut h = Harness::<Luz>::new()
            .group("pisca", 20.ms())
            .safe("led", false);
        h.run_for(520.ms());                            // LED aceso desde os 500 ms
        assert_eq!(h.output("led").last(), Some(true));
        h.stop_app();
        h.run_for(80.ms());
        assert_eq!(h.output("led").last(), Some(false)); // lease de 60 ms expirou → safe
    }
}
```

```sh
$ jk test
```

```
apps/luz (cargo test, componente wasm no wasmtime) ... 2 passed
```

O `jk test` corre o **componente `.wasm` real**, e não uma compilação nativa da app: é o que vai para o nó. O `cargo test` também funciona, com o backend nativo, e avisa que não é o componente de produção.

## 10. Um cenário do site

Os testes da app correm a app sozinha. Um **cenário** corre o **site**: o manifesto completo, o modelo simulado, o núcleo real e o relógio virtual (ref/03 §2.5).

```toml
# sim/scenarios/pisca.scenario.toml
schema = 1

[scenario]
name     = "pisca a 1 Hz e vai para safe quando a app para"
manifest = "joker.sim.toml"
duration = "10s"

[[expect]]
during        = "1s .. 7s"
signal        = "pi.act_led"
toggles_every = "500ms ± 20ms"

[[app]]
at  = "8s"
app = "luz"
do  = "stop"

[[expect]]
at     = "8.1s"
signal = "pi.act_led"
equals = false
level  = "none"
```

O último `[[expect]]` diz duas coisas: o LED está apagado **e** não há nenhum nível ativo no árbitro. Ninguém escreveu `false`: o lease da app expirou e o árbitro aplicou o `safe` (documento 16, A6).

```sh
$ jk test
```

```
apps/luz (cargo test, componente wasm no wasmtime) ... 2 passed
sim/scenarios/pisca.scenario.toml (joker.sim.toml, 10 s virtuais em 41 ms) ... passed
```

Uma expectativa que falhe é `J0842`, com o instante, o valor visto e a cadeia de decisão do `jk trace`. Experimenta: põe `toggles_every = "400ms ± 20ms"` e lê o relatório. O `jk test` sai com o código 6 (documento 07, §8), que é o que a CI usa.

## 11. O que aconteceu por baixo

A cada 20 ms, o grupo `main` faz um **tick** (documento 01, §4):

1. **latch:** congela as entradas numa imagem de processo (ainda não há nenhuma);
2. **tarefas:** chama `pisca` na instância Wasm da app, que propõe `led = true/false`;
3. **árbitro:** a proposta ocupa o nível `control:12` e renova o lease (documento 16, A2);
4. **envelope:** passa tudo, porque para um `bool` só declarámos `safe` (documento 01, §7);
5. **commit:** se o valor aplicado mudou, vai para o driver `sim`, que o mostra no painel.

O `jk dev` e o `jk test` correm exatamente este núcleo. A única diferença é o relógio: real no `jk dev`, virtual no `jk test` (ADR 0007). É por isso que o que passa num cenário é o que acontece no nó.

As camadas do manifesto resolvem-se antes de tudo: `joker.sim.toml` estende `joker.toml`, as tabelas fundem-se chave a chave, e o resultado é o **manifesto efetivo**, com todas as omissões materializadas (ref/01 §7 e §8). Vê-o com `jk diff --env sim`: o lease de 60 ms e o nível `control:12` aparecem lá, marcados `# omissão`.

## 12. Exercícios

1. Faz o LED piscar com um *duty cycle* de 10% (100 ms aceso, 900 ms apagado), com dois parâmetros. Atualiza o teste e o cenário.
2. Muda o `period` do grupo para `"50ms"` sem mexer na app. O pisca continua certo? Porquê? (Dica: `io.dt()`.) O que diz o `jk check`?
3. No cenário, acrescenta uma escrita manual (`[[write]]`, com `output`, `value = true`, `ttl = "2s"`, `at = "3s"`) e uma expectativa `level = "manual:4"` durante esse intervalo. O que acontece ao `toggles_every` entre os 3 s e os 5 s?

```sh
$ git add -A && git commit -qm "trilha(01): luz 0.1 pisca o LED ACT simulado; testes e cenário"
```

**Estado no fim desta parte:** app `luz` **0.1**. Ficheiros: `joker.toml`, `joker.sim.toml`, `joker.sim.lock`, `sim/pi.sim.toml`, `sim/scenarios/pisca.scenario.toml`, `apps/luz/`.

**Próximo:** [Parte 2 — O Pi 3 como nó: instalar, enrolar, a placa, o LED real](02-the-pi-as-a-node.md).
