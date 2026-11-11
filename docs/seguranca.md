# Segurança

## Princípios

- Nenhuma rota é pública sem autenticação, nem dentro da rede privada. Estar na VPN não basta.
- Um único ponto de entrada: só o Nginx escuta fora do host. As aplicações escutam apenas na rede interna do Docker.
- Segredos nunca entram no código nem na imagem.

## Autenticação

Todas as rotas exigem um token **Bearer**:

```
Authorization: Bearer <token>
```

- No legado também se aceitava `?token=` na URL. No Nexus esse formato só é aceito no upgrade do WebSocket, onde alguns clientes não conseguem enviar cabeçalhos, e o Nginx tira o parâmetro dos logs.
- O formato do token é `nxs_` seguido de 64 caracteres hexadecimais (32 bytes aleatórios). O prefixo fixo facilita achar um token vazado num log ou commit.
- Os tokens são guardados como hash SHA-256 no banco (`nexus.api_token`), cada um com nome, escopos, expiração opcional e revogação. A busca é feita pelo hash, então nem um vazamento da tabela entrega um token utilizável.
- O token aparece uma única vez, na criação (`nexus-api token criar`).
- Escopos: `leitura`, `escrita`, `relay`, `admin`. O relay do PC usa `relay`, que permite ingestão e fila de comandos, mas não administração.
- As exceções são `/health/live` e `/health/ready`, que não exigem token e respondem só `ok` ou o motivo da indisponibilidade, sem nenhum dado, e `/metrics`, que fica numa porta separada (9190) que o Nginx não expõe. Só o Prometheus, na rede Docker de monitoramento, chega nela.

O legado liberava qualquer requisição vinda da faixa de IPs da VPN. Isso acaba no Nexus.

## Borda (Nginx)

O tráfego externo chega por dois caminhos, e os dois terminam no Nginx:

- **Público:** passa pelo Cloudflare Tunnel (`cloudflared`), que já entrega TLS e proteção contra DDoS. Nenhuma porta da VPS fica aberta para a internet.
- **Privado:** passa pela rede Tailscale. O Nginx escuta só no IP do Tailscale e em localhost; o Docker ignora o UFW, por isso o IP fica explícito no `compose.yml`. O Nginx serve HTTPS (TLS 1.2/1.3, HTTP/2) na porta 443 do IP do Tailscale, com certificado emitido pelo `tailscale cert`. O certificado vale 90 dias e é renovado por um cron diário (`deploy/renovar-tls.sh`), que recarrega o Nginx quando o arquivo muda. HTTP puro só existe em localhost (porta 8480), para diagnóstico e healthcheck.

O que o Nginx aplica:

- Headers: `X-Content-Type-Options`, `X-Frame-Options`, `Referrer-Policy`. `Strict-Transport-Security` só nas respostas HTTPS. `Content-Security-Policy` entra junto com as páginas estáticas.
- Limite de tamanho do corpo: 2 MB no geral e 10 MB na ingestão.
- Rate limit por IP (proteção grossa) e timeouts de leitura e escrita.
- Log de acesso em JSON, sem o token.

O rate limit fino, por token, fica no `nexus-api`, porque só a aplicação sabe quem é o cliente. É um balde de fichas por token: `NEXUS_LIMITE_RPS` fichas por segundo (padrão 10), com rajada de `NEXUS_LIMITE_RAJADA` (padrão 40). Acima disso a resposta é `429` com `Retry-After: 1`.

Cada grupo de rotas exige um escopo: leituras pedem `leitura`; ingestão pede `relay` ou `escrita`; `admin` passa em todas. Token válido sem o escopo certo recebe `403`. O corpo das requisições é limitado a 2 MB, e a 10 MB nas rotas de ingestão.

## CORS

| Rotas | Origens permitidas |
|---|---|
| Painel (`/api/*` de leitura) | Origem do painel web |
| Ingestão e fila (`/api/*/ingest`, `/api/comandos`, `/ws`) | Nenhuma. O relay não é navegador e não precisa de CORS |
| `/health`, `/metrics` | Nenhuma |

O legado respondia `Access-Control-Allow-Origin: *` para tudo.

As origens do painel vêm de `NEXUS_CORS_ORIGENS` (separadas por vírgula). Sem nenhuma configurada, o Nexus não manda cabeçalho CORS. O CORS fica por fora da checagem de token, porque o preflight do navegador não manda token.

## Segredos

- Em produção: Docker secrets montados em `/run/secrets/`. A aplicação lê o arquivo indicado por variáveis `*_FILE` (por exemplo `NEXUS_DATABASE_URL_FILE`).
- Em desenvolvimento: `.env`, que fica no `.gitignore`. O repositório traz um `.env.example` sem valores reais.
- No CI: GitHub Secrets.
- O papel do banco usado pela aplicação (`nexus_app`) não é superusuário. Ele é dono só do schema `nexus` e não tem nenhum privilégio no `public`. O acesso às tabelas do legado vai ser concedido tabela por tabela, só para leitura, conforme cada fase precisar.
- A URL do banco chega ao container como Docker secret (`NEXUS_DATABASE_URL_FILE`), legível só pelo usuário do container.

## Imagem

- Imagem final distroless (`gcr.io/distroless/static-debian12:nonroot`) com binário estático: sem shell, sem libc e sem gerenciador de pacotes, rodando como usuário não-root.
- No Compose: `cap_drop: [ALL]`, `no-new-privileges` e limites de CPU e memória.
- Sistema de arquivos somente leitura (`read_only: true` no Compose).
- `cargo deny` no CI barra dependências com vulnerabilidade conhecida (base RustSec), licença fora da lista permitida ou origem desconhecida. A política está em `deny.toml`.

## Pendências herdadas do legado

Ficam resolvidas quando o legado for desligado:

- Credenciais escritas no código de alguns scripts Node. Antes do desligamento, a senha do banco é trocada.
- Servidor de estáticos e API principal expostos fora da VPN.
