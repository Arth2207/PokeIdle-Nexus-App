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
- Os tokens são guardados como hash SHA-256 no banco (`nexus.api_token`), cada um com nome, escopo e data de expiração. A comparação é feita em tempo constante.
- Escopos: `leitura`, `escrita`, `relay`, `admin`. O relay do PC usa `relay`, que permite ingestão e fila de comandos, mas não administração.
- As exceções são `/health/live` e `/health/ready`, que não exigem token mas só respondem pela rede interna, e `/metrics`, liberada apenas para o Prometheus.

O legado liberava qualquer requisição vinda da faixa de IPs da VPN. Isso acaba no Nexus.

## Borda (Nginx)

O tráfego externo chega por dois caminhos, e os dois terminam no Nginx:

- **Público:** passa pelo Cloudflare Tunnel (`cloudflared`), que já entrega TLS e proteção contra DDoS. Nenhuma porta da VPS fica aberta para a internet.
- **Privado:** passa pela rede Tailscale. O Nginx serve HTTPS com o certificado emitido por `tailscale cert`.

O que o Nginx aplica:

- Headers: `Strict-Transport-Security`, `X-Content-Type-Options`, `X-Frame-Options`, `Referrer-Policy`, `Content-Security-Policy` nas páginas estáticas.
- Limite de tamanho do corpo: 2 MB no geral e 10 MB na ingestão.
- Rate limit por IP (proteção grossa) e timeouts de leitura e escrita.
- Log de acesso em JSON, sem o token.

O rate limit fino, por token e por rota, fica no `nexus-api` (tower), porque só a aplicação sabe quem é o cliente.

## CORS

| Rotas | Origens permitidas |
|---|---|
| Painel (`/api/*` de leitura) | Origem do painel web |
| Ingestão e fila (`/api/*/ingest`, `/api/comandos`, `/ws`) | Nenhuma. O relay não é navegador e não precisa de CORS |
| `/health`, `/metrics` | Nenhuma |

O legado respondia `Access-Control-Allow-Origin: *` para tudo.

## Segredos

- Em produção: Docker secrets montados em `/run/secrets/`. A aplicação lê o arquivo indicado por variáveis `*_FILE` (por exemplo `DATABASE_URL_FILE`).
- Em desenvolvimento: `.env`, que fica no `.gitignore`. O repositório traz um `.env.example` sem valores reais.
- No CI: GitHub Secrets.
- O usuário do banco usado pela aplicação não é superusuário e só tem permissão nas tabelas de que precisa.

## Imagem

- Imagem final distroless (`gcr.io/distroless/cc-debian12:nonroot`): sem shell e sem gerenciador de pacotes, rodando como usuário não-root.
- Sistema de arquivos somente leitura (`read_only: true` no Compose).
- `cargo audit` e `cargo deny` no CI contra dependências com vulnerabilidade conhecida ou licença incompatível.

## Pendências herdadas do legado

Ficam resolvidas quando o legado for desligado:

- Credenciais escritas no código de alguns scripts Node. Antes do desligamento, a senha do banco é trocada.
- Servidor de estáticos e API principal expostos fora da VPN.
