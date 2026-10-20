# ADR 0024 — Segredos cifrados por destinatário, versionáveis em Git

**Estado:** aceite (v1) · **Data:** 2026-09 · **Substitui** a regra "`secrets.toml.enc` nunca em git" (documentos 03 §1 e 15 §2.4)

## Contexto

O desenho anterior cifrava os segredos "com a chave do dispositivo" e proibia o ficheiro no Git. Isso não diz como um segredo chega a 3 000 nós, nem como o Deck o entrega sem o poder ler, nem como se roda uma credencial sem mexer na configuração. E um ficheiro fora do Git é um ficheiro que se perde.

## Decisão

1. Cada segredo é cifrado com **HPKE** (RFC 9180) para a chave KEM (X25519) de **cada nó destinatário**, atestada no certificado do nó, e assinado por entrada, com uma revisão (`rev`) que nunca desce.
2. O `secrets.toml.enc` **pode ir para o Git**: só contém valores cifrados, e é assim que o Deck os distribui sem os conseguir ler.
3. Rodar um segredo não cria uma geração nova. **Exceção nos MCUs:** o segredo vai dentro do blob de geração, e rodá-lo produz um blob novo com o mesmo manifesto (novo `gen_seq`, mesma geração Linux e mesmo `source_hash`); a regra está na ref/08 §2.17.
4. As apps pedem segredos por nome local, ligado no manifesto: `[apps.<app>.secrets] <nome_local> = "secret:<nome>"`. Só na instância async.
5. Nenhuma interface expõe o valor: a API, o relatório ao Deck e o traço mostram nomes e revisões (ref/00 §9).

O detalhe está na ref/02, §2.4 e §3.4.

## Razões

- A distribuição pela frota passa a ser a mesma que a da configuração, sem canal lateral.
- Um Deck comprometido não lê segredos.

## Consequências

- O enrolamento de um nó gera também a chave KEM (ref/02, ref/10).
- Um nó novo só recebe um segredo depois de alguém com a chave de `secrets` o cifrar para ele (`jk secret recipient add` e `jk secret push`, ref/02 §2.4).
