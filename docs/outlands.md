# Outlands: catálogo atual e rendimento por contexto

O app preserva `outlandTiers`, `chanceBossToken`, `outlandDex` e a identificação da regra recebida do jogo. O nome apresentado usa o rótulo do catálogo: o ID 104 corresponde a Outland 4, não Outland 104. A identificação da espécie usa o slug, com leitura compatível do texto antigo sem o sufixo da região.

O motor usa o multiplicador do tier efetivamente selecionado; não infere a região pelo nível do treinador. Espécies de Outland com hunt de nível 150 não são excluídas pelo piso de progressão de XP de um Pokémon mais forte. As demais regras de segurança do combate continuam valendo.

## Estimativa e preço

O objetivo Gemas usa uma estimativa **parcial de Bronze Boss Token** nas Outlands. A chance-base e o multiplicador vêm do login. O preço do token vem da mediana unitária das vendas em gemas dos últimos sete dias, exigindo pelo menos três vendas com quantidade explícita positiva; a leitura tem cache de cinco minutos. Sem preço observado, catálogo ou tier identificado, o motor próprio aguarda dados para esse objetivo.

As chances atuais dos demais raros não estão no login consultado. Elas não são preenchidas com a tabela antiga para inventar receita. Os drops efetivamente recebidos continuam na tela e podem ser cotados. O campo manual `outlandBonus` permanece como ajuste explícito do modelo; não representa bônus detectado automaticamente.

Na interface, as cotações do inventário usam a referência do serviço de mercado com pelo menos três vendas e uma venda nos últimos sete dias. Mostram liquidez; itens sem cotação ficam sinalizados, não recebem preço zero como afirmação de ausência de valor. Não se promete que todo inventário será vendido.

## Tela

**Caçada → Outlands** mostra:

- Região selecionada, multiplicador, próxima liberação e todas as regiões do catálogo.
- Chance-base do token antes dos multiplicadores.
- Abates, tempo observado, abates/h, expectativa de tokens/h sem bônus adicionais e valor parcial dos drops em gemas.
- Valor do inventário pós comissão usada na simulação, separado das vendas realizadas.
- Vendas da conta recebidas nas últimas 24 horas segundo o extrato disponível. Essas vendas não são atribuídas automaticamente às Outlands ou à sessão atual.

A comissão da simulação começa em 15% e pode ser ajustada ao valor informado pelo jogo. Não é uma comissão confirmada automaticamente. Custos em Coins não são convertidos em gemas, e o painel não apresenta lucro líquido completo. O antigo indicador do mapa de hunts foi renomeado de Lucro/h para Receita/h.

A região é escolhida na interface do próprio jogo. Esta entrega não inventa um comando de alteração de tier. A troca de espécie usa `hunt.select`; o coletor aguarda o estado confirmar o slug e, para destinos Outland identificados, a manutenção do tier selecionado. Expira após 12 segundos sem confirmação. O app não deve informar sucesso apenas porque conseguiu enviar no WebSocket.

## Coleta e histórico

O coletor acrescenta contexto a novos eventos de kill/drop e envia `hunt.contexto` a cada 15 segundos enquanto houver conexão e mensagens recentes. O contexto inclui slug, tier, identificador das regras, Pokémon ativo, nível e os bônus informados pelo estado. Não solicita novas informações ao jogo para criar esses sinais.

As medições são agrupadas por esse contexto; não juntam regiões, versões, lutadores/níveis e bônus distintos. O tempo observado soma intervalos entre sinais do mesmo contexto, limita cada intervalo a 30 segundos e não atribui à nova região o intervalo de uma transição. O ritmo só é apresentado após 15 minutos observados. É uma amostragem: não comprova atividade em cada instante nem detecta toda mudança de equipamento/estatística que mantenha o mesmo id e nível.

Abates antigos sem contexto não recebem tier ou versão retrospectivamente e não entram nas comparações novas. A referência de preços pode continuar usando vendas recentes anteriores à mudança; a tela informa a separação entre regra histórica e atual para o rendimento de caça.

Quando a janela antiga não preserva o catálogo, a API consulta o último evento de login disponível em até 30 dias. Não reconstitui tier selecionado quando ele não foi informado na janela. Instalar o novo app e receber um login novo é necessário para a coleta completa. Novos catálogos normalizados são arquivados em `nexus.outland_regra`; o identificador emitido pelo coletor é um fingerprint operacional, não uma assinatura criptográfica do jogo.

## Objetivo da conta

Em **Automação → Troca de hunt**, escolha XP, equilíbrio, Coins, gemas ou shiny. A alteração de objetivo marca `objetivoManual=true` atomicamente no servidor e tem prioridade sobre os campos de objetivo/limitação a Outland definidos pelo papel da conta. Não troca o papel nem liga vendas ao NPC. A interface deixa explícita essa prioridade.

Nenhum objetivo ou hunt de uma conta existente é alterado durante a instalação da mudança. O usuário escolhe o objetivo pelo app.

## Instalação e validação

Aplicar `20261125000001_outlands.sql`, atualizar a API e instalar/reconectar o app. A migração cria o arquivo de regras e inicializa a indicação de objetivo manual como falsa. A migração anterior do Safari permanece necessária para aquele controle.

Nova rota: `GET /api/outlands?conta=rotulo&taxa=0.15`. A taxa é opcional, deve estar entre 0 e 1 e só afeta a simulação; sem taxa, o valor pós comissão fica indisponível. Tokens restritos à própria conta não podem consultar outra conta.

Validações:

```sh
CARGO_BUILD_JOBS=1 CARGO_PROFILE_DEV_DEBUG=0 cargo test -p nucleo --lib --offline
CARGO_BUILD_JOBS=1 CARGO_PROFILE_DEV_DEBUG=0 cargo test -p a2-nucleo --lib --offline
CARGO_BUILD_JOBS=1 CARGO_PROFILE_DEV_DEBUG=0 SQLX_OFFLINE=true cargo check -p nexus-api --offline
CARGO_BUILD_JOBS=1 CARGO_PROFILE_DEV_DEBUG=0 cargo check --manifest-path apps/desktop/ui/Cargo.toml --target wasm32-unknown-unknown --offline
python3 ferramentas/fixtures/testar-outlands.py
node ferramentas/fixtures/testar-coletor-outlands.cjs
```

O teste SQL usa PostgreSQL 16 temporário, sem TCP e sem tocar o banco de produção. O teste JS executa o coletor real numa página simulada, sem conexão com o jogo.
