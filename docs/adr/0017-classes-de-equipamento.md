# ADR 0017 — Classes de equipamento e instâncias, por cima dos sinais

**Estado:** aceite (v1) · **Data:** 2026-09

## Contexto

As portas tipadas (`Output<bool>`, `Input<f64>`) são a base certa, mas uma app com 15 equipamentos de 5 membros cada fica com 75 portas soltas, sem estrutura, e a lógica de negócio mistura-se com detalhes (flancos, grupos de escritas, qualidade de cada membro). Há dois públicos com necessidades opostas: quem constrói abstrações quer o baixo nível, e quem escreve lógica de negócio quer `pump.start()`.

## Decisão

- **Classes** versionadas (`led@1`, `pump@1`, `inverter.pv@2`) definidas em `.jkc` (documento 17, ADR 0018) num registo: membros (input, output, param, action, event), capacidades opcionais, métodos (atribuições declarativas a saídas, com grupos atómicos) e consultas (expressões avaliadas pelo núcleo).
- **Instâncias** de três tipos: implícitas (um perfil declara `implements`), compostas (o manifesto junta membros de vários dispositivos) e virtuais (uma app implementa a classe, com os comandos como saídas virtuais sujeitas a árbitro e envelope).
- No SDK: `#[device] led: Led`, `Many<Pump, 1, 8>`, classes locais com `#[derive(Class)]` e ligação por modelo de caminho.
- **Tudo compila para portas e sinais.** A camada de sinais continua completa e acessível (`raw()`).

## Razões

- A lógica de negócio fica em termos do domínio, portável entre sites e fabricantes.
- O equipamento lógico (uma bomba) raramente é um só dispositivo de comunicação; as instâncias compostas modelam isso sem código.
- As implementações ativas dão ao autor de abstrações um sítio para comportamento reutilizável (sequências, proteções), com segurança em cada camada.
- Um só avaliador de expressões (as consultas são sinais derivados do núcleo) preserva o determinismo e o replay.

## Consequências

- Mais um registo para governar (a biblioteca `std/` é pequena, revista e alinhada com normas: SunSpec, IEC 61850, CiA 402, BACnet).
- Cada camada virtual acrescenta até um tick de latência, a não ser que implementador e consumidor partilhem o grupo.
- Os métodos e as consultas estão limitados a uma gramática fechada; comportamento real exige uma implementação ativa.
- Alternativas rejeitadas: herança, métodos com código livre, consultas avaliadas no SDK, classes a definir segurança (documento 14, §12).
