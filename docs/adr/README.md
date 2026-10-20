# ADRs

| # | Decisão |
|---|---|
| [0001](0001-rust-for-the-runtime.md) | Rust para a runtime |
| [0002](0002-apps-as-wasm-components.md) | As apps são componentes WebAssembly |
| [0003](0003-the-signal-as-the-single-abstraction.md) | O sinal é a abstração única |
| [0004](0004-process-image-and-two-domains.md) | Imagem de processo, dois domínios de execução, duas instâncias por app |
| [0005](0005-no-write-bypasses-the-envelope.md) | Nenhum caminho de escrita contorna o envelope |
| [0006](0006-mandatory-safe-state.md) | Estado seguro obrigatório e sem valor por omissão |
| [0007](0007-clock-as-input.md) | O relógio é um input do núcleo |
| [0008](0008-drivers-in-three-tiers.md) | Drivers em três tiers |
| [0009](0009-burn-as-default-backend.md) | Burn como backend de ML por omissão, não exclusivo |
| [0010](0010-toml-manifest-and-lock.md) | Manifesto declarativo em TOML e `joker.lock` |
| [0011](0011-arbiter-with-priorities-and-leases.md) | Árbitro com prioridades e leases |
| [0012](0012-shadow-before-actuating.md) | Modo sombra como único caminho de promoção para o que atua |
| [0013](0013-telemetry-never-blocks.md) | A telemetria nunca bloqueia o controlo |
| [0014](0014-native-pipelines.md) | Pipelines nativos para dados volumosos |
| [0015](0015-apply-with-confirmation.md) | Aplicar configuração com confirmação e rollback automático |
| [0016](0016-one-process.md) | Um processo, não microserviços |
| [0017](0017-equipment-classes.md) | Classes de equipamento e instâncias, por cima dos sinais |
| [0018](0018-one-format-per-type.md) | Um formato por tipo de coisa: TOML, `.jkc` e código |
| [0019](0019-ml-in-1-1.md) | ML na 1.1, com a v1 preparada para ele |
| [0020](0020-no-std-core-and-joker-micro.md) | Núcleo `no_std` desde o primeiro dia, e o Joker Micro na v1 |
| [0021](0021-local-first-family.md) | Uma família de produtos, local-first, com contratos abertos |
| [0022](0022-versioning-policy.md) | Política de versões: minors temáticas, compatibilidade total na 1.x, LTS no fim da série |
| [0023](0023-normative-references.md) | As interfaces fecham-se em referências normativas (`docs/ref/`) |
| [0024](0024-per-recipient-secrets.md) | Segredos cifrados por destinatário, versionáveis em Git |
| [0025](0025-identity-and-signatures.md) | Identidade P-256 nos nós, Ed25519 na organização, COSE em tudo o que se assina |
| [0026](0026-type-in-the-file-name.md) | O tipo de um ficheiro está no nome (`<nome>.<tipo>.toml`, `joker.<variante>.toml`) |
| [0027](0027-composite-classes.md) | Classes compostas de ponta a ponta: ligação, caminhos aninhados, métodos, saúde, perfis e Joker Micro |
| [0028](0028-stable-equals-implemented.md) | Estável = implementado; o que não sai na 1.0 fica reservado |
| [0029](0029-english-project-language.md) | English is the project language; code has docstrings only, no comments |

Mudar uma decisão = escrever uma ADR nova que a substitui, e atualizar os documentos afetados no mesmo conjunto de commits.
