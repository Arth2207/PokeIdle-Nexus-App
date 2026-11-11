# Infraestrutura

## Servidor

VPS com 2 vCPU, 8 GB de RAM e Ubuntu. A mesma máquina já roda outras cargas em Docker (observabilidade, automações), então o Nexus tem que caber num orçamento apertado e declarado.

## Containers

| Serviço | Imagem | CPU (limite) | RAM (limite) | Observação |
|---|---|---|---|---|
| `nginx` | nginx:alpine | 0.25 | 64 MB | única porta exposta |
| `nexus-api` | própria, distroless | 1.5 | 512 MB | rayon limitado a 2 threads |
| `nexus-worker` | própria, distroless | 0.75 | 384 MB | prioridade menor que a API |

O PostgreSQL continua no host ([ADR 0004](adr/0004-postgres-nativo.md)). Os containers chegam até ele pelo **socket Unix** (`/var/run/postgresql` montado no container), autenticando com senha. Assim nenhuma porta de banco fica exposta em rede, nem para a rede Docker.

A soma dos limites de CPU passa de 2 de propósito. Os limites dizem quanto cada container pode usar no pico, não quanto ele tem reservado. A API pode usar quase os dois núcleos durante um cálculo de equipe, mas o worker nunca consegue tirar CPU dela além do próprio teto.

Todos os serviços do Compose têm:

- `restart: unless-stopped`
- `healthcheck` apontando para `/health/ready`
- `read_only: true` com `tmpfs` em `/tmp`
- rotação de logs (`max-size: 10m`, `max-file: 3`)
- `stop_grace_period: 30s`, para o desligamento gracioso funcionar

## Imagem

Build multi-stage:

1. **chef:** `cargo-chef` prepara a receita de dependências, para o cache de camadas sobreviver a mudanças no código.
2. **builder:** compila em `release` com `SQLX_OFFLINE=true`. As queries foram validadas contra o banco no desenvolvimento e ficam salvas em `.sqlx/`, então o build não precisa de banco.
3. **final:** `distroless/static-debian12:nonroot` com só o binário. O binário é compilado contra musl e sai estático, então a imagem final não tem nem libc, além de não ter shell.

Perfil de release: `lto = "fat"`, `codegen-units = 1`, `strip = true`, `panic = "abort"`.

**Meta:** imagem final abaixo de 50 MB. O tamanho é medido no CI, e o build falha se passar disso.

## Deploy sem downtime

O Nginx aponta para um upstream com dois nomes possíveis, `nexus-api-blue` e `nexus-api-green`. O deploy:

1. Sobe a cor inativa com a imagem nova.
2. Espera o `/health/ready` dela ficar verde.
3. Troca o upstream do Nginx e faz `nginx -s reload`, sem derrubar conexões.
4. Manda `SIGTERM` para a cor antiga, que para de aceitar conexões, termina as requisições em andamento, fecha os WebSockets com o código `1012` (service restart) e encerra.

Os clientes WebSocket reconectam sozinhos (veja [websocket](websocket.md)).

Antes de subir a cor nova, o deploy roda `nexus-api migrar` com a imagem nova. Como as migrations são sempre aditivas, a versão antiga continua funcionando com o schema novo. Se a cor nova não ficar saudável em 60 segundos, o deploy é abortado e a antiga continua no ar.

### Como o CI chega na VPS

- O job de deploy entra por SSH como `nexus-deploy`, um usuário sem senha e sem shell interativo.
- A chave desse usuário é restrita no `authorized_keys` (`command=`, `no-pty`, `no-port-forwarding`). Ela só consegue executar o `deploy.sh`, e qualquer outro comando cai na mensagem de uso.
- O token para baixar a imagem do GHCR é o `GITHUB_TOKEN` do próprio job, enviado pela entrada padrão. Ele expira quando o job termina, e o `deploy.sh` faz logout logo depois do pull.

Os arquivos de produção ficam em `/opt/nexus-deploy` e são instalados ou atualizados com `deploy/instalar.sh`. Os comandos do dia a dia estão em [operação](operacao.md).

## CI/CD (GitHub Actions)

**A cada push e pull request:**

1. `cargo fmt --check`
2. `cargo clippy -- -D warnings` (inclui a proibição de `unwrap`/`expect`)
3. `cargo test`, com testes de integração usando Postgres efêmero (testcontainers)
4. `cargo audit` e `cargo deny`
5. `cargo sqlx prepare --check`, para garantir que o cache offline está atualizado

**Na branch `main`:**

6. Build da imagem e checagem do tamanho
7. Push para o GitHub Container Registry
8. Deploy via SSH: `docker compose pull` e a troca blue/green

**A imagem nunca é compilada na VPS.** Compilar Rust em release ocupa os dois núcleos por minutos e afetaria a produção.

## Observabilidade

A VPS já roda Prometheus, Grafana e Alertmanager. O Nexus se integra a eles:

- **Métricas** (`/metrics`, numa porta separada, a 9190, que o Nginx não expõe): latência por rota (histograma), requisições por status, conexões WebSocket ativas, tamanho das filas de saída, duração dos jobs do worker, uso do pool do banco, tempo do motor de combate e do otimizador, taxa de acerto dos caches.
- **Logs:** `tracing` em JSON, com `request_id` propagado do Nginx (`X-Request-Id`) até a aplicação.
- **Alertas:** `/health/ready` falhando por mais de 1 minuto, p95 do otimizador acima de 3 s, erros 5xx acima de 1%, job do worker sem rodar no horário.

## Health checks

| Rota | Verifica | Uso |
|---|---|---|
| `/health/live` | O processo responde | diagnóstico manual |
| `/health/ready` | `SELECT 1` no pool com timeout de 2 s | healthcheck do Docker (`nexus-api healthcheck`) e deploy |

Quando o banco cai, o `ready` fica vermelho, o container é marcado como `unhealthy` e o alerta `NexusBancoInacessivel` dispara. O processo não é reiniciado: ele continua de pé e volta a responder sozinho quando o banco volta, porque o pool reconecta. A imagem não tem `curl`, então o próprio binário faz o healthcheck (`nexus-api healthcheck`).

## Desligamento gracioso

Ao receber `SIGTERM`:

1. Para de aceitar conexões novas.
2. Avisa os clientes WebSocket (código `1012`).
3. Espera as requisições em andamento terminarem, até 25 segundos.
4. No worker, deixa o job atual terminar e não inicia outro.
5. Fecha o pool do banco e encerra.
