# Migração

## Estratégia

O sistema Node continua funcionando sem nenhuma alteração até a última fase. O Nexus sobe ao lado dele e assume as responsabilidades uma fase de cada vez. Cada fase passa por três etapas:

1. **Shadow.** O Nexus calcula as mesmas respostas que o Node, mas quem responde ao cliente ainda é o Node. As respostas dos dois são comparadas.
2. **Cutover.** Quando a paridade está confirmada, o Nginx passa a mandar as rotas daquela fase para o Nexus.
3. **Observação.** O processo Node daquela fase fica ligado, mas sem receber tráfego, por pelo menos 7 dias. Se algo der errado, o rollback é só voltar a rota no Nginx.

### Como as respostas são comparadas

O plano original era o Nginx espelhar o tráfego (`mirror`). Na prática o tráfego da bolsa não passa pelo Nginx do Nexus: o painel chama o motor local, que repassa para o Node. Espelhar exigiria mexer nesse caminho, e o Node não pode ser tocado.

Também avaliamos capturar requisições com `tcpdump` e reexecutá-las depois. Descartamos porque a resposta depende do momento: reexecutar mais tarde compara com dados que já mudaram e gera diferenças falsas. Além disso, o painel faz poucas requisições (cerca de 0,1 por segundo) e cobriria só os tickers que alguém abriu.

A solução adotada é a **comparação ativa**. Um comparador dentro do próprio `nexus-api` chama o Node com GETs comuns e monta a resposta do Nexus para os mesmos parâmetros:

- o mercado do Nexus é montado no mesmo instante (`geradoEm`) que o Node usou, para os dois olharem os mesmos dados;
- a cada rodada entram todos os tickers do `/mercado` e uma amostra rotativa para as rotas por id (`ticker`, `balao`, `serie`);
- a comparação normaliza o JSON: ordem de chaves e campos `undefined` não contam, os números têm tolerância relativa de `1e-6`, e os campos voláteis (`geradoEm`, `ultimaCaptura`) ficam de fora;
- o resultado vira métricas no Prometheus (`nexus_paridade_total{rota,resultado}` e `nexus_paridade_razao{rota}`) e uma linha de log por rota, com exemplos das diferenças.

Os handlers HTTP e o comparador chamam as mesmas funções, então o que se compara é exatamente o que a API responde. O comparador liga com `NEXUS_PARIDADE_URL` e também roda sob demanda: `nexus-api paridade <url>`.

Rotas de escrita não são comparadas em produção, porque gravariam em dobro. Elas são validadas nos testes de integração, contra um banco de teste (veja [testes](testes.md)).

### Banco compartilhado

Node e Nexus usam o mesmo banco durante toda a migração. Para não haver conflito:

- Cada tabela tem um único dono por vez ([banco de dados](banco-de-dados.md#quem-escreve-o-quê)).
- A troca de dono acontece no cutover, nunca antes.
- A fila de comandos usa `FOR UPDATE SKIP LOCKED`, que é seguro com consumidores dos dois lados.
- Nenhuma migration altera o schema `public` antes do fim.

## Fases

As estimativas consideram que o Rust está sendo aprendido junto com o projeto.

### Fase 0: Fundação (1 a 2 semanas)

Workspace, configuração, erros, logs, `/health`, `/metrics`, autenticação por token, Dockerfile, Compose, Nginx e pipeline de CI.

**Pronto quando:** o CI gera uma imagem com menos de 50 MB; o deploy blue/green funciona; `/health/ready` fica vermelho quando o banco é parado e volta sozinho quando o banco volta.

**Resultado (2026-11-11):** imagem de 11,4 MB; a API em repouso usa menos de 1 MB de RAM; na troca blue → green, 219 requisições seguidas durante o deploy deram 219 respostas 200. O comportamento sem banco é coberto por testes automatizados (o banco de produção não foi parado).

### Fase 1: Bolsa (3 a 4 semanas)

Carregamento de vendas e ofertas, precificador de pokémon, cálculo de tickers e indicadores (médias, RSI, Bollinger, MACD, VWAP), sinais, veredito e valor justo, e todas as rotas `/api/bolsa/*`, exceto as de equipe e de alvos de coleta, que dependem do motor de combate (fase 2).

**Pronto quando:** 3 dias de shadow sem diferenças fora da tolerância; ingestão de ofertas idempotente (enviar o mesmo lote duas vezes resulta no mesmo estado).

#### Estado

Implementada. Na primeira comparação com o Node, todas as rotas de leitura saíram idênticas: 467 de 467 tickers no `/mercado`, a amostra de `ticker`, `balao` e `serie`, o `feedback`, as listas do `dicas` e os 23 itens do `meus-itens`. O precificador também foi conferido à parte: 200 fichas reais avaliadas pelos dois lados, com o mesmo valor justo, faixa e confiança.

As rotas de ofertas (`/ofertas` e as oportunidades do `/dicas`) ainda não tinham dado para comparar ao vivo, porque nenhuma oferta estava ativa no período. Elas usam o mesmo avaliador já conferido e entram na comparação automaticamente quando o relay voltar a enviar ofertas.

#### O que o Nexus faz a mais

- **Mercado sempre pronto.** Uma task remonta o mercado a cada 3 minutos e troca o snapshot de uma vez; nenhuma requisição espera a montagem. No Node, a primeira requisição depois de vencer o cache espera, com o event loop travado.
- **Carga incremental.** As vendas ficam em memória; cada ciclo lê só as das últimas 12 horas (pelo índice de `em`), com recarga completa a cada 30 minutos. O Node relê 30 dias de vendas a cada ciclo.
- **Todas as ofertas avaliadas.** O Node só mostra ofertas de pokémon cujo valor justo já estava em cache (calculado por outro processo). O Nexus avalia todas em segundo plano, junto com a montagem do mercado. Por isso o `/ofertas` do Nexus pode trazer mais ofertas de pokémon que o do Node; a comparação considera as que existem nos dois.
- **Respostas menores.** Compressão gzip/br e ETag nas leituras: o `/mercado` cai de cerca de 540 KB para 130 KB, e uma nova consulta sem mudança recebe `304`.
- **Métricas.** Tempo de montagem, número de tickers, vendas lidas por ciclo e paridade por rota.
- **Dados do legado no Postgres.** O catálogo de espécies, a base antiga do precificador e o inventário saem de arquivos JS/JSON e vão para o schema `nexus` ([ADR 0006](adr/0006-consolidar-estado-no-postgres.md)).

#### Correções em relação ao legado

Ficam desligadas de propósito no que afeta a comparação, ou só aparecem nas rotas de escrita, que não rodam em produção antes do cutover:

| Legado | Nexus |
|---|---|
| `POST /ofertas` com `completo: true` desativa todas as ofertas não vistas no lote, de qualquer tipo e moeda | desativa só as dos pares (tipo, moeda) presentes no lote |
| Uma oferta com preço, id ou quantidade fracionários faz o `INSERT` falhar e o lote inteiro se perde | só a oferta inválida é descartada (quantidade fracionária é arredondada para baixo) |
| `meus-itens` gravado em arquivo JSON, sem escrita atômica | tabela `nexus.meus_itens`, gravada numa transação |
| Vendas no mesmo milissegundo saem em ordem arbitrária | ordem estável por `em` e `id` |
| Pokémon vendido sem espécie vira o ticker `p:null:...` | a venda é ignorada |
| `ticker` de item identificado só pelo nome falha ao buscar ofertas (`item_id = NaN`) | devolve a lista de ofertas vazia |

#### Antes do cutover

1. 3 dias de comparação sem diferenças fora da tolerância, inclusive nas rotas de ofertas.
2. Atualizar o catálogo de itens e o de espécies, se o jogo tiver mudado (`nexus-api importar`).
3. Ligar `NEXUS_ESCRITA_BOLSA=true` no Nexus e, na mesma janela, apontar o relay e o painel para o Nexus.
4. Parar de chamar as rotas da bolsa no Node; o processo fica de pé por 7 dias, para rollback.

### Fase 2: Combate e otimizador (3 a 4 semanas)

Motor de combate, otimizador de equipe com `rayon`, `/api/bolsa/equipe`, `/api/bolsa/equipe-propria`, `/api/bolsa/alvos-coleta` e `/api/equipes/*`. O plano completo, com o que a investigação do código público do jogo mostrou, está em [fase 2: combate](fase-2-combate.md).

**Pronto quando:** o score das equipes encontradas fica dentro da tolerância do Node em todas as fixtures (o otimizador é heurístico, então a equipe pode ser diferente); o p95 no teste de carga é menor que o do Node; a API continua respondendo em menos de 50 ms enquanto o otimizador está rodando.

### Fase 3: Ingestão, comandos e WebSocket (2 a 3 semanas)

Rotas de ingestão, raros, shiny, fila de comandos e o servidor WebSocket. O relay do PC passa a usar token e WebSocket, e a enviar as lutas de boss para calibrar o modelo de combate. O plano está em [fase 3: ingestão](fase-3-ingestao.md).

**Pronto quando:** os testes de reconexão e backpressure passam; 7 dias sem comando perdido nem duplicado.

### Fase 4: Motor de automação (5 a 8 semanas)

O estado das contas, builds, venda, hunts, pedras e o restante das rotas do motor. É a fase maior: o motor concentra boa parte da regra de negócio do sistema.

**Pronto quando:** paridade em todas as rotas do motor; o estado migrado para o Postgres bate com o arquivo original.

### Fase 5: ETL e dossiê (2 a 3 semanas)

ETL contínuo, eficiência de hunts, preços de mercado e o site do dossiê, no `nexus-worker`.

**Pronto quando:** rodar o mesmo período duas vezes produz o mesmo resultado; as tabelas fato geradas pelo Nexus batem com as do Node no mesmo intervalo.

### Fase 6: Scoring e IA (2 a 3 semanas)

Scoring e as rotinas de decisão, observação, compras e montagem de builds.

**Pronto quando:** as decisões do Nexus coincidem com as do Node nas mesmas entradas, em modo de simulação, por 7 dias.

### Fase 7: Desligamento (1 semana)

- Desligar o pm2 e o serviço do dossiê.
- Migrar o SQLite para hypertables.
- Ligar compressão e retenção no TimescaleDB.
- Trocar a senha do banco.
- Remover as portas antigas do firewall.
- Arquivar o código Node.

**Pronto quando:** o host não tem mais nenhum processo Node; um backup restaurado num banco vazio sobe o sistema completo.

**Total estimado:** de 5 a 7 meses para o servidor, mais de 2 a 3 meses para o app, em parte em paralelo.

## App desktop

O app depende do contrato da API e do WebSocket, por isso as fases dele começam depois das fases correspondentes do servidor. Ele roda em paralelo ao Electron até a A2 estar validada.

### Fase A1: Esqueleto e bolsa (depois da fase 1; 2 a 3 semanas)

Projeto Tauri 2 + Leptos em `apps/desktop`, crate `contrato`, login por token e as telas da bolsa, só leitura.

**Pronto quando:** o app mostra mercado, ticker e dicas a partir da API do Nexus; build Windows gerado no CI.

### Fase A2: Relay e janela do jogo (depois da fase 3; 3 a 4 semanas)

Núcleo em Rust com o cliente WebSocket (retomada por `seq`, backoff), execução de comandos, janela do jogo com o coletor injetado e envio da captura ao servidor.

**Pronto quando:** 7 dias rodando no lugar do Electron e do `holodex-relay.js` sem perder captura nem comando.

### Fase A3: Painel completo e versão web (depois da fase 4; 3 a 5 semanas)

Telas de automação, builds, venda, hunts e equipes. A mesma interface é publicada como site estático pelo Nginx.

**Pronto quando:** tudo o que o painel atual faz é possível no app e no navegador.

### Fase A4: Distribuição (1 semana)

Instalador Windows e atualização automática (updater do Tauri) a partir dos releases do GitHub, com artefatos assinados.

**Pronto quando:** um push com tag gera o instalador, e o app instalado se atualiza sozinho.

O Electron é desinstalado junto com a fase 7 do servidor.

## Rollback

| Situação | Ação | Tempo |
|---|---|---|
| Rota com problema depois do cutover | Voltar a rota para o Node no Nginx e recarregar | < 1 min |
| Versão nova do Nexus com problema | Voltar o upstream para a cor anterior | < 1 min |
| Dado gravado errado pelo Nexus | Restaurar a tabela afetada do backup e devolver a escrita ao Node | depende do volume |

Até a fase 7, todo rollback de rota é possível, porque o Node continua instalado e configurado.
