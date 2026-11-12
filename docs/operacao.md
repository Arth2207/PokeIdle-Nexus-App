# Operação

Comandos do dia a dia na VPS. Tudo que mexe na produção roda como o usuário `nexus-deploy`, nunca como root, para não deixar arquivos com dono errado em `/opt/nexus-deploy`.

```bash
alias nexus='sudo -u nexus-deploy /opt/nexus-deploy/deploy.sh'
```

## Deploy

O caminho normal é o CI: um push na `main` passa pelos testes, gera a imagem e faz o deploy sozinho. Para repetir um deploy manualmente, com uma imagem já publicada:

```bash
echo "$TOKEN_GHCR" | nexus deploy <sha-completo-do-commit>
```

O `TOKEN_GHCR` é um token do GitHub com permissão `read:packages`. Se a imagem já estiver baixada na VPS, o token pode ser omitido.

## Estado e rollback

```bash
nexus status      # cor ativa e containers
nexus rollback    # volta para a cor anterior em poucos segundos
```

A cor anterior fica parada, mas não é removida. Por isso o rollback não precisa baixar nada.

## Logs

```bash
cd /opt/nexus-deploy
sudo -u nexus-deploy docker compose logs -f --tail 100 nexus-api-blue   # ou green
sudo -u nexus-deploy docker compose logs -f --tail 100 nginx
```

Os logs são JSON. Para seguir uma requisição de ponta a ponta, filtre pelo `request_id`, que é o mesmo no log do Nginx e no da aplicação:

```bash
sudo -u nexus-deploy docker compose logs nginx nexus-api-blue | grep <request_id>
```

## Tokens de acesso

Criar um token (ele aparece uma única vez):

```bash
cd /opt/nexus-deploy
sudo -u nexus-deploy docker compose exec nexus-api-blue /nexus-api token criar relay-pc relay
```

Escopos: `leitura`, `escrita`, `relay`, `admin`, separados por vírgula. Cada grupo de rotas exige um escopo: as leituras da bolsa pedem `leitura`; a ingestão vai pedir `relay` ou `escrita`. `admin` passa em todas. Token válido sem o escopo certo recebe `403`.

Revogar e listar tokens, direto no banco:

```sql
UPDATE nexus.api_token SET revogado_em = now() WHERE nome = 'relay-pc';
SELECT nome, escopos, criado_em, expira_em, revogado_em FROM nexus.api_token ORDER BY criado_em;
```

Testar um token:

```bash
# de qualquer máquina do tailnet
curl -H "Authorization: Bearer nxs_..." https://<maquina>.<tailnet>.ts.net/api/eu
# na própria VPS (ela não usa MagicDNS)
curl -H "Authorization: Bearer nxs_..." http://127.0.0.1:8480/api/eu
```

## Saúde e métricas

```bash
curl http://127.0.0.1:8480/health/ready
sudo -u nexus-deploy docker compose ps          # coluna STATUS mostra (healthy)
```

As métricas ficam no Prometheus (job `nexus-api`) e os alertas no grupo `nexus-api`. Os blocos de configuração estão versionados em `deploy/monitoring/`.

Métricas da bolsa: `nexus_bolsa_montagem_duracao_segundos`, `nexus_bolsa_tickers`, `nexus_bolsa_vendas_janela`, `nexus_bolsa_vendas_lidas_total`, `nexus_bolsa_ofertas_avaliadas`, `nexus_bolsa_gerado_em_segundos` e `nexus_bolsa_montagem_falhas_total`. As regras `NexusParidadeBaixa`, `NexusParidadeParada` e `NexusBolsaMontagemFalhando` estão no arquivo de alertas, mas ainda precisam ser carregadas no Prometheus.

## Cliente sombra do WebSocket

Mede "7 dias sem comando perdido nem duplicado" antes da A2. O subcomando `nexus-api sombra` conecta em `/ws` como um relay, recebe os comandos do espelho (`origem='espelho'`), não executa nada e responde `comando.resultado` com `sombra`. Reconecta sozinho (backoff de 1 a 30 s com jitter, `retomar` com o último `seq`, trata `sessao.ressincronizar`), responde ping e encerra limpo com SIGTERM. Comando recebido de novo depois de enviado a confirmação vai para o log (`warn`) e para o contador `duplicados`, que sai no log de 5 em 5 minutos e no encerramento. O token nunca é logado.

Envs do serviço:

| Variável | Valor |
|---|---|
| `NEXUS_SOMBRA_URL` | `ws://nexus-api-blue:8190/ws` (só `ws://`; rede interna do compose) |
| `NEXUS_SOMBRA_TOKEN_FILE` | caminho do secret com o token, ex. `/run/secrets/nexus_sombra_token` |
| `NEXUS_SOMBRA_GERAR_S` | opcional; segundos entre comandos sintéticos (jitter ±20%). Vazio ou 0 = desligado. Exige `NEXUS_DATABASE_URL_FILE` no container |

Com `NEXUS_SOMBRA_GERAR_S` ligado, a sombra insere direto em `nexus.comando` (nunca em `public`) um `sombra.ping` (conta `sombra-teste`, origem `sintetico`, expira em 10 min); o NOTIFY entrega ao próprio cliente, que confirma `sombra`. Falha de banco só gera `warn`.

Token com escopo `relay` (aparece uma vez; grave no secret):

```bash
sudo -u nexus-deploy docker compose exec nexus-api-blue /nexus-api token criar sombra relay
```

Serviço de compose (mesma imagem, comando `sombra`; exemplo, o arquivo de deploy é editado à parte):

```yaml
nexus-sombra:
  image: <mesma imagem do nexus-api>
  command: ["sombra"]
  restart: unless-stopped
  environment:
    NEXUS_SOMBRA_URL: ws://nexus-api-blue:8190/ws
    NEXUS_SOMBRA_TOKEN_FILE: /run/secrets/nexus_sombra_token
  secrets: [nexus_sombra_token]
```

O servidor precisa estar com `NEXUS_ESPELHO_COMANDOS` ligado. Se a sombra estiver rodando junto do relay real, o token da sombra só deve ser usado por ela: duas conexões do mesmo token dividem os comandos.

Relatório (só SELECT no banco; padrão 168 h = 7 dias):

```bash
docker compose exec nexus-api-blue /nexus-api sombra-relatorio --desde 168 [--origem espelho|sintetico|todas]
```

Padrão `--origem todas`: imprime um bloco por origem (`espelho` e `sintetico`), cada um com seu veredito. `sintetico` é a carga do gerador (tráfego real quase nulo); `espelho` é o tráfego real. Leitura, para as linhas criadas no período:

- total: total; entregues: já reservados por algum consumidor; confirmados: resultado `sombra`.
- perdidos: expiraram sem nunca ter sido entregues. Tem de ser 0.
- sem confirmação: encerrados sem `sombra` (entregues mas não confirmados, ou `erro:sem confirmacao`). Tem de ser 0.
- pendentes: ainda abertos (não entram no veredito; confira de novo depois).
- entregues mais de 1 vez: não há coluna de contagem de entregas, então usa `tentativas >= 1` (cada devolução à fila, por lease vencido ou reconexão do token, soma uma). Reconexão legítima com comando reservado e não confirmado aparece aqui. Tem de ser 0 numa janela sem quedas; durante queda, compare com o log do cliente.
- atraso p50/p95: segundos entre `criado_em` e a confirmação `sombra`.
- veredito `OK` só com perdidos, duplicados e sem confirmação em 0. A meta é 7 dias seguidos nesse estado.

## Atualizar a infraestrutura

Mudanças em `compose.yml`, `nginx.conf` ou `deploy.sh` não chegam pelo CI: elas são instaladas a partir do repositório.

```bash
cd /opt/nexus-rs && git pull
sudo deploy/instalar.sh
```

Depois de mudar o `nginx.conf`:

```bash
cd /opt/nexus-deploy
sudo -u nexus-deploy docker compose exec nginx nginx -t
sudo -u nexus-deploy docker compose exec nginx nginx -s reload
```

## Certificado HTTPS

Renovado sozinho todo dia às 04:17 (`/etc/cron.d/nexus-tls`); o log vai para o syslog com a tag `nexus-tls`. Para forçar e conferir:

```bash
sudo /opt/nexus-deploy/renovar-tls.sh
openssl x509 -in /opt/nexus-deploy/tls/cert.pem -noout -enddate
```

## Banco

- As migrations rodam sozinhas em todo deploy (`nexus-api migrar`), antes da troca de cor.
- Nunca aplique migrations novas no banco de produção à mão antes do deploy. Se uma imagem anterior (ou um commit intermediário) for implantada depois disso, o `migrar` dela recusa as migrations que não conhece e o deploy aborta.
- Para aplicar à mão: `sudo -u nexus-deploy docker compose run --rm nexus-api-blue migrar`.
- Trocar a senha do `nexus_app`: altere no Postgres (`ALTER ROLE nexus_app PASSWORD '...'`), regrave `/opt/nexus-deploy/secrets/database_url` (dono 65532, modo 400) e faça um deploy, para os containers lerem o secret novo.

### Acesso às tabelas do legado

Até o cutover de cada fase, as tabelas do schema `public` pertencem ao sistema legado. O `nexus_app` só pode ler as que usa, e nunca grava nelas. A bolsa precisa destas (rodar como superusuário, uma vez):

```sql
GRANT USAGE ON SCHEMA public TO nexus_app;
GRANT SELECT ON public.fato_venda_mercado, public.fato_oferta_mercado,
                public.fato_resumo_item, public.dim_item TO nexus_app;
```

Conferir: `SELECT table_name, privilege_type FROM information_schema.role_table_grants WHERE grantee = 'nexus_app' AND table_schema = 'public';`

### Dados da bolsa

Parte dos dados que a bolsa usa vive em arquivos no sistema legado. O subcomando `importar` lê um arquivo do stdin e substitui o conteúdo da tabela correspondente no schema `nexus`, numa transação:

| Comando | Arquivo de origem | Tabela |
|---|---|---|
| `importar itens` | `items-catalogo.json` | `nexus.item_catalogo` |
| `importar especies` | catálogo `PI_ESPECIES` exportado como JSON | `nexus.especie` |
| `importar base` | `precos.json` (vendas, itens, pesos e viés) | `nexus.venda_base`, `nexus.item_preco_base`, `nexus.parametro` |
| `importar meus-itens` | `.bolsa-meus-itens.json` | `nexus.meus_itens` |
| `importar pokedex` | `engine/data/pokedex.json` | `nexus.pokedex_motor` |
| `importar equipes` | `data/equipes-estado.json` | `nexus.equipes_estado`, `nexus.equipes_conta` |
| `importar wiki` | `data/POKEIDLE-wiki.txt` (seção "## Bosses") | `nexus.boss_wiki` |
| `importar faltas` | `data/.bolsa-faltas.json` | `nexus.falta_coleta` |
| `importar raros` | `data/raros-estado.json` (só os nomes por id) | `nexus.parametro` (`raros.nomes`) |
| `importar welcome` | último `welcome` do coletor (`data/mapa.db`, tabela `welcome_dump`) | `nexus.parametro` (`servidor.constantes`) |

```bash
cd /opt/nexus-deploy
sudo -u nexus-deploy docker compose exec -T nexus-api-blue /nexus-api importar itens < items-catalogo.json
```

O catálogo de espécies é um arquivo JS; para gerar o JSON, basta carregar os dois arquivos do catálogo e serializar `PI_ESPECIES` (o legado já roda Node, então um `node -e` resolve). O `deploy/sincronizar-catalogos.sh`, instalado pelo `instalar.sh` com um cron a cada 10 minutos (`/etc/cron.d/nexus-catalogos`, log em `journalctl -t nexus-catalogos`), faz isso sozinho: compara o sha256 de cada arquivo com o da última importação e só reimporta o que mudou, pela API ativa. Ele roda como root porque os arquivos do legado não são legíveis pelo `nexus-deploy`, pula a rodada se houver um deploy em andamento e converte o catálogo de espécies com o Node do legado (sem Node na máquina, esse catálogo fica de fora). A API relê os catálogos da bolsa e a pokédex a cada 30 minutos, e o estado de equipes a cada 30 segundos. Com o catálogo de itens vazio, os itens sem `item_id` viram tickers por nome (`i:n:<nome>:<moeda>`) e deixam de bater com o legado.

### Escrita da bolsa

As rotas `POST /api/bolsa/ofertas`, `resumo-itens` e `meus-itens` e a limpeza periódica de ofertas gravam em tabelas que, até o cutover da fase 1, são do legado. Por isso ficam desligadas: respondem `503` enquanto `NEXUS_ESCRITA_BOLSA` não for `true`. Ligar só no cutover, junto com a troca do relay e do painel para o Nexus.

### Escrita de equipes

`POST /api/equipes/ingest`, a gravação das faltas de coleta e o registro das sugestões de equipe (`nexus.equipe_sugestao`) só acontecem com `NEXUS_ESCRITA_EQUIPES=true`. Até o cutover da fase 2, quem recebe o relay é o legado, e o estado chega pela importação; o Nexus calcula as faltas, mas não as grava.

### Escrita da fase 3 (ingestão, raros e comandos)

Quatro flags, todas desligadas por padrão. Enquanto estiverem assim, o relay segue falando só com o legado.

| Flag | O que liga |
|---|---|
| `NEXUS_ESCRITA_INGESTAO` | `POST /ingest` e `POST /api/hunt-sync` (gravam `nexus.evento` e `nexus.config_hunt`). Métrica `nexus_ingest_eventos_total{tipo,resultado}` (aceito, duplicado, erro) |
| `NEXUS_ESCRITA_RAROS` | `POST /api/raros/ingest` e `POST /api/shiny/registrar` (gravam `nexus.estado_raros`). Desligada, o estado vem do `importar raros` (cron de 10 min), que agora grava o estado completo; com ela ligada, o import não sobrescreve |
| `NEXUS_ESCRITA_COMANDOS` | `GET /api/comandos` (reserva), `POST /api/confirmar-comando` e `POST /api/inserir-comando` sobre `nexus.comando` |
| `NEXUS_ESPELHO_COMANDOS` | A cada 5 s copia as linhas novas de `public.comando_fila` para `nexus.comando` (só lê o legado; `origem='espelho'`). É o modo sombra: o relay novo recebe, não executa e confirma `sombra` |

A migration `20261114000001_fase3.sql` é só aditiva (schema `nexus`); aplica no deploy como as outras. Métricas da fila: `nexus_comandos_entregues_total{origem}`, `nexus_comandos_duplicados_total` (2ª entrega ao mesmo relay), `nexus_comandos_confirmados_total{sombra}`, `nexus_comandos_perdidos_total` (expirou sem ninguém receber), `nexus_comandos_devolvidos_total` e `nexus_comandos_sem_confirmacao_total`. O `GET /ws` (veja [websocket](websocket.md)) responde 503 se `NEXUS_ESCRITA_COMANDOS` e `NEXUS_ESPELHO_COMANDOS` estiverem desligadas; o nginx precisa liberar o upgrade nessa rota. O comparador passou a incluir `raros2`, `raros2.cartas` e `shiny`; só compara quando os dois lados têm o mesmo `dadosEm`.

### Timescale em `nexus.evento` e observabilidade da fase 3

A migration `20261115000001_timescale_evento.sql` (sem transação, só com a extensão) liga compressão de `nexus.evento` (segmentada por conta e tipo, ordenada por `em DESC`, após 7 dias) e cria o agregado contínuo `nexus.evento_hora` (contagem por hora, conta e tipo, em tempo real, refresh a cada 30 min). O `evento_hora` materializa só os últimos 3 dias (`start_offset` da policy); lote atrasado mais antigo só aparece na leitura em tempo real. `Idempotency-Key` com payload diferente devolve o id antigo (a chave manda, o corpo não é comparado). **Sem retenção:** apagar dados exige autorização. Se um dia for autorizado: `SELECT add_retention_policy('nexus.evento', interval '180 days')` (qualificando pelo schema da extensão). A `20261115000002_comando_melhorias.sql` acrescenta `chave_idem`, `executar_em` e `agendado_avisado` em `nexus.comando` e amplia o trigger de aviso.

Flags novas: `NEXUS_LIMITE_INGESTAO_RPS` (50) e `NEXUS_LIMITE_INGESTAO_RAJADA` (200), limite por token da ingestão, **aplicado só com `NEXUS_MELHORIAS=true`** (sem ela nunca há `429`, como no legado). O push ao vivo (`raro.novo`, `shiny.novo`, `comando.estado`), `/api/comandos/falhos`, `/reenviar`, `executar_em` e `/api/eventos` ficam atrás de `NEXUS_MELHORIAS`. O webhook do Discord não foi feito: o `reqwest` do workspace está sem TLS.

Métricas novas: `nexus_comandos_pendentes{idade=ate_1m|ate_5m|mais_5m}`, `nexus_comando_pendente_mais_antigo_segundos`, `nexus_espelho_comandos_lag_ids`, `nexus_ws_lotes_limitados_total`, `nexus_comandos_agendados_liberados_total` (e `nexus_ws_conexoes_abertas`, que já existia). Dashboard e alertas de referência (não aplicados): `docs/observabilidade/fase3-grafana.json` e `fase3-alertas.yml`.

### Melhorias e tokens por conta

`NEXUS_MELHORIAS=true` liga o que diverge do legado de propósito ([ADR 0010](adr/0010-paridade-primeiro-melhorias-depois.md)): seleção exata nos perfis de equipe, chance de vitória, ordem de compra, preços típicos corrigidos e a tabela de tipos do servidor. Só no cutover da fase 2.

`nexus-api token criar <nome> <escopos> [conta]`, com a conta, cria um token preso a ela: nas rotas de equipes ele só enxerga essa conta (pedir outra dá `403`). `nexus-api pokedex-divergencias` compara as duas pokédex do sistema.

### Aviso no Discord

`NEXUS_DISCORD_WEBHOOK` (URL do webhook, tratada como segredo e nunca logada; vazia = desligado) só vale com `NEXUS_MELHORIAS`. Cada `raro.novo` e `shiny.novo` gera uma mensagem curta, enviada por uma task separada: a gravação nunca espera. A fila tem 64 posições; cheia, a mensagem é descartada e contada em `nexus_discord_webhook_descartadas_total`. O envio tem timeout de 5 s, respeita o `retry_after` do `429` (até 2 repetições) e o resultado vai em `nexus_discord_webhook_envios_total{resultado}`. Entregue o valor por arquivo de ambiente ou secret, não na linha de comando.

### Modelos de combate

O modelo `legado` responde nas rotas; o `jogo` roda ao lado a cada 10 minutos e só aparece nas métricas: `nexus_modelo_cobertura{boss,modelo}` e `nexus_modelo_especies_em_comum{boss}` mostram quanto as equipes teóricas dos dois divergem. Com 5 ou mais tentativas em `nexus.boss_tentativa`, a mesma tarefa calibra HP, dano e base da penalidade do boss (`nexus_modelo_jogo_parametro`, `nexus_modelo_jogo_erro_rms_log`) e, com `NEXUS_ESCRITA_EQUIPES=true`, grava os parâmetros em `nexus.parametro`. `NEXUS_MODELO=jogo` troca o modelo das respostas; só faz sentido depois da calibração.

### Comparador de paridade

O serviço `nexus-paridade` do Compose roda `nexus-api paridade-continua`. A cada 10 minutos ele compara as rotas de leitura da bolsa com o legado e grava o resultado em `nexus.paridade`. A API publica a última rodada de cada rota em `/metrics` (`nexus_paridade_razao`, `nexus_paridade_comparados`, `nexus_paridade_idade_segundos`).

```bash
# Última rodada, com exemplos das diferenças
psql -c "SELECT DISTINCT ON (rota) rota, comparados, iguais, so_legado, so_nexus, exemplos FROM nexus.paridade ORDER BY rota, em DESC"
# Uma rodada avulsa, com relatório no terminal
sudo -u nexus-deploy docker compose run --rm nexus-paridade paridade http://127.0.0.1:8181
```

Além da bolsa, ele compara `/api/equipes/bosses`, `/api/equipes/plano` e `/api/bolsa/alvos-coleta`. Com `NEXUS_PARIDADE_EQUIPE=true`, compara também `/api/bolsa/equipe` (por qualidade: dano ≥ 99% do legado, restrições respeitadas, custo dentro de ±5%) e `/equipe-propria`; fica desligado por padrão porque essas chamadas disparam o cálculo pesado do legado.

O serviço usa a rede do host porque o firewall não deixa a rede do Docker chegar na porta do legado. Ele não abre nenhuma porta e só faz GETs.

## Primeira instalação

Feita uma vez, com o repositório clonado em `/opt/nexus-rs`:

1. Criar o papel e o schema no Postgres:
   ```sql
   CREATE ROLE nexus_app LOGIN PASSWORD '...' CONNECTION LIMIT 10;
   GRANT CONNECT ON DATABASE pokeidle TO nexus_app;
   CREATE SCHEMA nexus AUTHORIZATION nexus_app;
   ALTER ROLE nexus_app IN DATABASE pokeidle SET search_path = nexus;
   ```
2. Liberar o socket com senha no `pg_hba.conf`, acima da linha `local all all peer`, e recarregar com `SELECT pg_reload_conf()`:
   ```
   local   pokeidle   nexus_app   scram-sha-256
   ```
3. Rodar o instalador:
   ```bash
   sudo NEXUS_DATABASE_URL_SOCKET='postgres://nexus_app:SENHA@localhost/pokeidle?host=/var/run/postgresql' \
     deploy/instalar.sh
   ```
4. Cadastrar no GitHub, em Settings → Secrets and variables → Actions, dentro do environment `producao`:
   - `VPS_HOST`: IP público da VPS
   - `VPS_SSH_KEY`: a chave privada que o instalador mostrou (e depois apagar o arquivo da VPS)
   - `VPS_KNOWN_HOSTS`: a saída de `ssh-keyscan -t ed25519 <ip>`
5. Acrescentar o job e os alertas de `deploy/monitoring/` ao Prometheus e mandar `docker kill -s HUP obs-prometheus`.
