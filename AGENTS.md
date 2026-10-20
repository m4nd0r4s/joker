# AGENTS.md

Instruções para qualquer agent (ou pessoa) que trabalhe neste repositório. Curto de propósito: o detalhe está em `docs/`.

## 1. A especificação manda

- `docs/` é normativo. Se o código e a documentação divergem, o código está errado.
- Se a documentação estiver errada ou incompleta, **não a contornes no código**. Escreve uma ADR nova em `docs/adr/` (próximo número livre), atualiza os documentos afetados no mesmo conjunto de commits, e só depois implementa.
- Leitura mínima antes de mexer em código: `docs/00-visao-geral.md`, `docs/12-limites.md`, `docs/16-semantica-do-nucleo.md`, `docs/ref/00-convencoes.md`, o documento da tua área e a referência da tua superfície em `docs/ref/`.
- **As referências em `docs/ref/` e os ficheiros de `wit/` e `spec/` mudam juntos** (ADR 0023): mudar uma interface é mudar a referência e o ficheiro máquina-legível no mesmo conjunto de commits, com ADR se tocar num contrato público (ADR 0022). Se um ficheiro e o texto divergem, é um bug nos dois; o `joker-spec-tests` (documento 13, tarefa 0a) falha a CI.
- Precedência entre documentos: a de `docs/README.md` (as referências de `docs/ref/` ficam acima dos documentos por área). Os exemplos e a trilha são ilustrativos; se contradisserem a especificação, corrige-se o exemplo.

## 2. Invariantes que não se negociam

1. **`joker-core` e `joker-expr` não fazem I/O**, não leem o relógio e não geram aleatoriedade. O relógio entra pelo trait `Clock` (ADR 0007).
2. **`joker-units`, `joker-expr`, `joker-core` e `joker-link-proto` são `#![no_std]`** (a lista completa é a do documento 13, §1) e não alocam no caminho do tick; todas as estruturas têm capacidade fixa. O `alloc` é uma feature opcional, só para construir o registo no Linux (ADR 0020). Uma dependência nova nestes crates tem de ser `no_std`. **A sincronização faz-se só por `critical-section` e `portable-atomic`** (`AtomicU64` para os `seq` em MCUs de 32 bits), nunca por primitivas do executor nem por locks (ref/08 §2.4, regra 8). O `joker-agent-proto` também é `no_std`, porque é partilhado com os MCUs autónomos.
3. **Nenhum caminho de escrita contorna o envelope**: nem apps, nem CLI, nem API, nem pontes. Os drivers só recebem escritas já aplicadas (*committed*).
4. **A qualidade nunca desaparece em silêncio.** Uma amostra `Bad` não se converte num número. A API que devolve só o valor devolve `Result`.
5. **Toda a saída tem estado seguro declarado.** Não há valor por omissão; faltar o `safe` é um erro de validação.
6. **O domínio de ciclo não aloca, não bloqueia e não faz `await`.** A memória é pré-alocada no arranque e fixada (`mlockall`). Os locks são proibidos; só se usam estruturas lock-free com limite conhecido.
7. **A telemetria nunca bloqueia o controlo.** Filas cheias perdem amostras segundo a política declarada, e cada perda é contada.
8. **Toda a mudança de resultado entra no registo de decisões:** cada mudança de (valor aplicado, resultado, nível ativo) de uma saída, aceite ou não, com a causa e a contagem de repetições (M3; documento 01, §19). Os trips, as escritas manuais, as rejeições e as mudanças de nível gravam-se com fsync; o resto, em lote.
9. **Determinismo do núcleo:** o mesmo traço dá os mesmos bytes. Sem `HashMap` com ordem observável em saídas; usa `BTreeMap` ou `IndexMap`.
10. Sem `unwrap()` fora de testes; sem `unsafe` fora dos crates `*-sys`, de `joker-host/ffi` e de `joker-micro/ffi` (a ABI C do Joker Micro, só com a feature `c-abi`; ref/08 §2.11).
11. **Código em inglês:** identificadores, comentários, docstrings e mensagens de erro no código são escritos em inglês. A documentação em `docs/` e as mensagens de commit continuam em português.

## 3. Mapa e fronteiras de posse

| Frente | Dona de | Consome |
|---|---|---|
| Núcleo | `crates/joker-core/`, `crates/joker-expr/`, `crates/joker-units/` | `docs/01`, `docs/02`, `docs/16`, `docs/ref/00` |
| Manifesto | `crates/joker-manifest/` (com `codes.toml`) | `joker-core`, `docs/03`, `docs/ref/01`, `docs/ref/02` (lock) |
| HAL e drivers | `crates/joker-hal/`, `crates/joker-drivers/*`, `crates/joker-driver-sdk/`, `crates/joker-jdp/`, `crates/joker-drvtest/` | `joker-core`, `docs/05`, `docs/ref/03`, `docs/ref/06` |
| Host Wasm e SDK | `crates/joker-host/`, `crates/joker-sdk/`, `wit/` | `joker-core`, `docs/04`, `docs/ref/04`, `docs/ref/05` |
| ML (1.1) | `crates/joker-ml*/` | `joker-core`, `docs/06` |
| Store e link | `crates/joker-store/`, `crates/joker-link/` | `joker-core` |
| Daemon, API e CLI | `crates/jokerd/`, `crates/joker-api/`, `crates/joker-client/`, `crates/joker-cli/`, `spec/openapi/` | tudo; `docs/07`, `docs/ref/02`, `docs/ref/09` |
| Agente | `crates/joker-agent/`, `crates/joker-agent-proto/`, `spec/cddl/agent.cddl` | `docs/20`, `docs/ref/10` |
| Joker Micro e Link | `crates/joker-micro/`, `crates/joker-link-proto/`, `spec/c/`, `spec/cddl/joker-link.cddl`, `spec/cddl/joker-micro.cddl` | `joker-core`, `docs/19`, `docs/ref/07`, `docs/ref/08` |
| Classes (`jkc`) | `crates/joker-jkc/`, `classes/std/` | `docs/17` |
| Conformidade | `conformance/`, `crates/joker-spec-tests/` | só a spec (sobretudo `docs/16` e `docs/ref/`) |

A lista completa das referências de que cada frente é dona está no documento 13, §2.

`joker-core` e `wit/` são os pontos de contenção. Mudanças lá são pequenas, ficam isoladas num commit próprio e são feitas antes do código que as usa. Mudar o `wit/` ou o `spec/` exige uma ADR quando toca num contrato público (M21, ADRs 0022 e 0023), e a referência correspondente muda no mesmo commit.

## 4. Controlo de versões (jj)

- Repositório jj colocado sobre git. Um agent = um workspace: `jj workspace add ../joker-<frente>`.
- **Commits atómicos:** uma mudança lógica por commit, e cada commit compila e passa os testes.
- Mensagem: `tipo(âmbito): resumo` em português, com corpo a explicar o porquê. Tipos: `feat`, `fix`, `docs`, `test`, `refactor`, `build`, `chore`, `perf`.

## 5. Antes de dar uma mudança por feita

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

- Se tocaste na semântica do envelope, do árbitro ou da imagem de processo, há um caso em `conformance/` que a cobre.
- Se tocaste no domínio de ciclo, o teste de latência (`cargo xtask rt-bench`) não regrediu mais de 5% no p99.9.
