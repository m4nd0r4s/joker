# ADR 0012 — Modo sombra como único caminho de promoção para o que atua

**Estado:** aceite (v1) · **Alterada por ADR 0019 (ML na 1.1)**: a sombra de apps é da 1.0; a de modelos, da 1.1 · **Data:** 2026-09

## Contexto

Modelos e apps novas que atuam num processo real não podem ser validados só offline, e "ativamos e vemos" não é aceitável.

## Decisão

Uma versão nova de um modelo ou app com saídas corre primeiro em sombra: mesmas entradas, escritas gravadas como `Shadowed`, comparação com a versão atual ou com o sistema existente. A promoção faz-se por critérios declarados (automática ou manual), pode ser parcial por saídas e tem despromoção automática por KPI. Em `prod`, promover um modelo que atua sem passar pela sombra exige `--skip-shadow "<justificação>"`, que fica auditado.

## Razões

- Transforma confiança em evidência.
- Os traços da sombra são também dados de avaliação.

## Consequências

- Custo de CPU (duas versões a correr) e de tempo até à promoção.
- A sombra não apanha efeitos de malha fechada (a versão em sombra não vê as consequências das suas próprias decisões). É uma limitação conhecida, e a promoção parcial mitiga-a.
