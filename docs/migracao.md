# Migração

## Estratégia

O sistema Node continua funcionando sem nenhuma alteração até a última fase. O Nexus sobe ao lado dele e assume as responsabilidades uma fase de cada vez. Cada fase passa por três etapas:

1. **Shadow.** O Nexus recebe uma cópia do tráfego real, mas quem responde ao cliente ainda é o Node. As respostas dos dois são comparadas.
2. **Cutover.** Quando a paridade está confirmada, o Nginx passa a mandar as rotas daquela fase para o Nexus.
3. **Observação.** O processo Node daquela fase fica ligado, mas sem receber tráfego, por pelo menos 7 dias. Se algo der errado, o rollback é só voltar a rota no Nginx.

### Como o tráfego é copiado

O Nginx usa `mirror` para duplicar as requisições de leitura para o Nexus e descarta a resposta dele. O Nexus grava cada par (requisição, resposta) e um comparador gera o relatório de diferenças.

Rotas de escrita **não** são espelhadas, porque gravariam em dobro. Elas são validadas com as fixtures capturadas, rodando contra um banco de teste (veja [testes](testes.md)).

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

### Fase 1: Bolsa (3 a 4 semanas)

Carregamento de vendas e ofertas, cálculo de tickers e indicadores (médias, RSI, Bollinger, MACD, VWAP), sinais de compra e venda, e todas as rotas `/api/bolsa/*`, exceto as de equipe.

**Pronto quando:** 3 dias de shadow sem diferenças fora da tolerância; ingestão de ofertas idempotente (enviar o mesmo lote duas vezes resulta no mesmo estado).

### Fase 2: Combate e otimizador (3 a 4 semanas)

Motor de combate, otimizador de equipe com `rayon`, `/api/bolsa/equipe`, `/api/bolsa/equipe-propria` e `/api/equipes/plano`.

**Pronto quando:** o score das equipes encontradas fica dentro da tolerância do Node em todas as fixtures (o otimizador é heurístico, então a equipe pode ser diferente); o p95 no teste de carga é menor que o do Node; a API continua respondendo em menos de 50 ms enquanto o otimizador está rodando.

### Fase 3: Ingestão, comandos e WebSocket (2 a 3 semanas)

Rotas de ingestão, raros, shiny, fila de comandos e o servidor WebSocket. O relay do PC passa a usar token e WebSocket.

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

**Total estimado:** de 5 a 7 meses.

## Rollback

| Situação | Ação | Tempo |
|---|---|---|
| Rota com problema depois do cutover | Voltar a rota para o Node no Nginx e recarregar | < 1 min |
| Versão nova do Nexus com problema | Voltar o upstream para a cor anterior | < 1 min |
| Dado gravado errado pelo Nexus | Restaurar a tabela afetada do backup e devolver a escrita ao Node | depende do volume |

Até a fase 7, todo rollback de rota é possível, porque o Node continua instalado e configurado.
