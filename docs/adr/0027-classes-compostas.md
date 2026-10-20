# ADR 0027 — Classes compostas de ponta a ponta

**Estado:** aceite (v1) · **Data:** 2026-09

## Contexto

Uma classe já podia ter membros que são outras classes (`member pumps: std/pump@1[1..8]`), com profundidade máxima 3. Mas faltava o resto: como se liga um membro de classe numa instância, que caminhos ganha, se uma instância pode estar em duas composições, se um método pode chamar métodos dos membros, como se calcula a saúde, como um único perfil cumpre uma classe composta, e como um Joker Micro exporta uma. Sem isto, não se consegue abstrair um equipamento inteiro (uma bomba de calor, o nó de I/O da bancada) numa só instância.

## Decisão

1. No `map` de uma instância composta, um membro de classe liga-se ao **nome de uma instância**, e uma coleção a uma **lista** de nomes, verificados na classe, na major, nas capacidades e nos limites (`J0650`–`J0654`).
2. Um membro de classe cria **caminhos aninhados** `<instância>.<membro>.<submembro>`, que são aliases dos mesmos sinais (16 P6).
3. Uma instância pode estar em várias composições; os seus `out` continuam a ter um só envelope e um só árbitro, e o `jk check` avisa quando duas apps chegam à mesma saída por caminhos diferentes (`J0655`).
4. Um método pode chamar métodos de **membros fixos** (`call drive.stop()`), nunca de coleções (`JC0502`–`JC0504`). Numa instância **virtual**, os membros pertencem à implementação: quem chama só lê (`J-E0507`, `J0656`).
5. A saúde de uma composta é a pior entre as suas regras e a dos membros obrigatórios.
6. Um perfil cumpre uma classe composta com `[implements."<classe>".members.<membro>]`, recursivamente.
7. O Joker Micro exporta a árvore no catálogo do `HELLO`, e o nó Linux vê os mesmos caminhos.
8. A profundidade de composição (≤ 3, `JC0501`) e a das camadas de implementação (≤ 3, `J0620`) contam-se à parte.
9. **Uma classe descreve um papel, não um chip:** `bench_io@1` ("o nó de I/O da bancada") é aceitável; `esp32@1` é o erro.

O detalhe está no documento 14 (§2, §4.6, §9.3), no 17 (§3.2, §4, §7, §8), no 16 §6, na ref/01 §5.13, na ref/03 §2.2.14, na ref/04 §4.8.6, na ref/05 §14.6, na ref/07 e na ref/09 §5.11.

## Consequências

- Um equipamento inteiro pode ser uma instância, até num Joker Micro, e o nó Linux vê o papel, não o chip.
- O `joker:meta`, o catálogo do Joker Link e a API passam a descrever árvores de instâncias.
