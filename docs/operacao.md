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

### Catálogo de itens

A bolsa resolve o id de um item pelo nome usando `nexus.item_catalogo`. Para carregar ou atualizar a partir do `items-catalogo.json` do legado (substitui o catálogo inteiro numa transação):

```bash
cd /opt/nexus-deploy
sudo -u nexus-deploy docker compose exec -T nexus-api-blue /nexus-api catalogo importar < items-catalogo.json
```

Com o catálogo vazio, os itens sem `item_id` viram tickers por nome (`i:n:<nome>:<moeda>`) e deixam de bater com o legado.

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
