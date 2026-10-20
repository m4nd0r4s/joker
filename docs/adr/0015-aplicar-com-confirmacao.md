# ADR 0015 — Aplicar configuração com confirmação e rollback automático

**Estado:** aceite (v1) · **Data:** 2026-09

## Contexto

Uma configuração nova pode cortar a ligação ao próprio dispositivo (uma rede, uma rota, um certificado), e num site remoto isso significa uma deslocação.

## Decisão

`jk apply --confirm <janela>` aplica a nova geração e espera um `jk confirm`. Sem confirmação dentro da janela, a geração anterior é reposta automaticamente. Em `prod`, a janela é obrigatória para gerações que mexem em rede, segurança, uplinks ou na própria API local.

## Razões

- É o `commit confirmed` dos equipamentos de rede, aplicado ao edge.
- Gerações imutáveis tornam o rollback trivial.

## Consequências

- Um segundo passo em cada deploy. Automatizável pelo agente de frota com verificação de saúde.

## Nota (rodada da API, 2026-09)

- **Razões de confirmação.** O plano da API diz porque pede confirmação (`confirm_reasons`: `network`, `security`, `uplinks`, `api`). A razão `api` é nova: uma geração que muda as escutas ou os papéis da API pode cortar a própria `jk confirm`. Em `prod`, ativar sem janela uma geração com alguma destas razões é `J-E1610` (ref/09 §5.13, S7).
- **Rollback reativa.** A geração reposta é a anterior, com o mesmo hash e o mesmo número; o histórico regista a ativação com a sua origem (documento 22, D9).
- **Confirmação no agente.** Com o Deck, a política de confirmação vai no estado desejado (ref/10 §2.6.4): `auto` (o nó confirma-se pela saúde local), `deck` (espera um comando `confirm`) ou `manual` (espera um `jk confirm`). Em `auto`, a saúde local não chega: a confirmação exige também uma troca autenticada com o Deck depois da troca (`require_uplink`, H7). Uma geração que isolou o nó nunca se confirma a si própria, e a janela expira com rollback (ref/10 §3.5). A API e o agente partilham o mesmo trinco de transição de gerações.
