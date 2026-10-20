# ADRs

| # | Decisão |
|---|---|
| [0001](0001-rust-para-a-runtime.md) | Rust para a runtime |
| [0002](0002-apps-como-componentes-wasm.md) | As apps são componentes WebAssembly |
| [0003](0003-o-sinal-como-abstracao-unica.md) | O sinal é a abstração única |
| [0004](0004-imagem-de-processo-e-dois-dominios.md) | Imagem de processo, dois domínios de execução, duas instâncias por app |
| [0005](0005-nenhuma-escrita-contorna-o-envelope.md) | Nenhum caminho de escrita contorna o envelope |
| [0006](0006-estado-seguro-obrigatorio.md) | Estado seguro obrigatório e sem valor por omissão |
| [0007](0007-relogio-como-input.md) | O relógio é um input do núcleo |
| [0008](0008-drivers-em-tres-tiers.md) | Drivers em três tiers |
| [0009](0009-burn-como-backend-por-omissao.md) | Burn como backend de ML por omissão, não exclusivo |
| [0010](0010-manifesto-toml-e-lock.md) | Manifesto declarativo em TOML e `joker.lock` |
| [0011](0011-arbitro-com-prioridades-e-leases.md) | Árbitro com prioridades e leases |
| [0012](0012-sombra-antes-de-atuar.md) | Modo sombra como único caminho de promoção para o que atua |
| [0013](0013-telemetria-nunca-bloqueia.md) | A telemetria nunca bloqueia o controlo |
| [0014](0014-pipelines-nativos.md) | Pipelines nativos para dados volumosos |
| [0015](0015-aplicar-com-confirmacao.md) | Aplicar configuração com confirmação e rollback automático |
| [0016](0016-um-processo.md) | Um processo, não microserviços |
| [0017](0017-classes-de-equipamento.md) | Classes de equipamento e instâncias, por cima dos sinais |
| [0018](0018-um-formato-por-tipo.md) | Um formato por tipo de coisa: TOML, `.jkc` e código |
| [0019](0019-ml-na-1-1.md) | ML na 1.1, com a v1 preparada para ele |
| [0020](0020-nucleo-no-std-e-joker-micro.md) | Núcleo `no_std` desde o primeiro dia, e o Joker Micro na v1 |
| [0021](0021-familia-local-first.md) | Uma família de produtos, local-first, com contratos abertos |
| [0022](0022-politica-de-versoes.md) | Política de versões: minors temáticas, compatibilidade total na 1.x, LTS no fim da série |
| [0023](0023-referencias-normativas.md) | As interfaces fecham-se em referências normativas (`docs/ref/`) |
| [0024](0024-segredos-por-destinatario.md) | Segredos cifrados por destinatário, versionáveis em Git |
| [0025](0025-identidade-e-assinaturas.md) | Identidade P-256 nos nós, Ed25519 na organização, COSE em tudo o que se assina |
| [0026](0026-tipo-no-nome-do-ficheiro.md) | O tipo de um ficheiro está no nome (`<nome>.<tipo>.toml`, `joker.<variante>.toml`) |
| [0027](0027-classes-compostas.md) | Classes compostas de ponta a ponta: ligação, caminhos aninhados, métodos, saúde, perfis e Joker Micro |
| [0028](0028-estavel-igual-implementado.md) | Estável = implementado; o que não sai na 1.0 fica reservado |
| [0029](0029-english-project-language.md) | English is the project language; code has docstrings only, no comments |
| [0030](0030-unit-syntax-and-kinds.md) | Unit syntax, quantity kinds in compounds, and literal error codes |
| [0031](0031-registry-hash-encoding.md) | Signal ordering and the registry hash encoding |
| [0032](0032-dt-after-overrun-skip.md) | `dt()` counts whole periods, also after `overrun = "skip"` |
| [0033](0033-write-outcomes-and-decision-records.md) | Write outcomes per proposal and per tick, and decision records |
| [0034](0034-expression-typing-and-evaluation.md) | Expression typing, `J0304` and `J0307`, and the semantics of windows and stateful functions |
| [0037](0037-trace-v1-encoding-details.md) | Encoding details of the `.jktrace` v1 format |
| [0038](0038-envelope-steps-2-to-6.md) | Details of envelope steps 2–6 |
| [0039](0039-atomic-groups-in-the-envelope.md) | Atomic groups in the envelope |
| [0040](0040-alarm-lifecycle-and-unknown.md) | Alarm lifecycle: delays, `Unknown`, suppression and shelving |

Mudar uma decisão = escrever uma ADR nova que a substitui, e atualizar os documentos afetados no mesmo conjunto de commits.
