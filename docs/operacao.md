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

```bash
cd /opt/nexus-deploy
sudo -u nexus-deploy docker compose exec -T nexus-api-blue /nexus-api importar itens < items-catalogo.json
```

O catálogo de espécies é um arquivo JS; para gerar o JSON, basta carregar os dois arquivos do catálogo e serializar `PI_ESPECIES` (o legado já roda Node, então um `node -e` resolve). O `deploy/sincronizar-catalogos.sh`, instalado pelo `instalar.sh` com um cron a cada 10 minutos (`/etc/cron.d/nexus-catalogos`, log em `journalctl -t nexus-catalogos`), faz isso sozinho: compara o sha256 de cada arquivo com o da última importação e só reimporta o que mudou, pela API ativa. Ele roda como root porque os arquivos do legado não são legíveis pelo `nexus-deploy`, pula a rodada se houver um deploy em andamento e converte o catálogo de espécies com o Node do legado (sem Node na máquina, esse catálogo fica de fora). A API relê os catálogos a cada 30 minutos. Com o catálogo de itens vazio, os itens sem `item_id` viram tickers por nome (`i:n:<nome>:<moeda>`) e deixam de bater com o legado.

### Escrita da bolsa

As rotas `POST /api/bolsa/ofertas`, `resumo-itens` e `meus-itens` e a limpeza periódica de ofertas gravam em tabelas que, até o cutover da fase 1, são do legado. Por isso ficam desligadas: respondem `503` enquanto `NEXUS_ESCRITA_BOLSA` não for `true`. Ligar só no cutover, junto com a troca do relay e do painel para o Nexus.

### Escrita de equipes

`POST /api/equipes/ingest`, a gravação das faltas de coleta e o registro das sugestões de equipe (`nexus.equipe_sugestao`) só acontecem com `NEXUS_ESCRITA_EQUIPES=true`. Até o cutover da fase 2, quem recebe o relay é o legado, e o estado chega pela importação; o Nexus calcula as faltas, mas não as grava.

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
