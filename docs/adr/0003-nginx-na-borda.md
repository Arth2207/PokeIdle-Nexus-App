# 0003. Nginx como ponto único de entrada

- Status: aceito
- Data: 2026-11-11

## Contexto

O tráfego chega por dois caminhos: Cloudflare Tunnel (que já entrega TLS) e Tailscale. O axum, com os middlewares do tower, consegue fazer compressão e rate limit sozinho. Então foi considerado não ter proxy reverso nenhum.

Ainda assim, há coisas que a aplicação não consegue fazer bem sozinha:

- trocar a versão sem derrubar as conexões em andamento;
- ter um único ponto onde TLS, headers de segurança e limites de corpo são aplicados igualmente a tudo;
- servir arquivos estáticos sem ocupar o runtime da aplicação;
- registrar log de acesso padronizado (o legado não tem nenhum);
- espelhar tráfego para o shadow durante a migração.

## Decisão

Usar Nginx como único serviço que escuta fora da rede Docker. O `cloudflared` e o Tailscale apontam para ele, e as aplicações só escutam na rede interna.

A responsabilidade fica dividida:

| Nginx | nexus-api |
|---|---|
| TLS no acesso via Tailscale | Autenticação por token |
| Headers de segurança, limite de corpo, timeouts | Rate limit por token e por rota |
| Rate limit grosso por IP | Validação dos dados |
| Compressão gzip | Lógica de negócio |
| Log de acesso, `X-Request-Id` | Métricas da aplicação |
| Blue/green e `mirror` para o shadow | |

## Consequências

- Deploy sem downtime pela troca de upstream.
- Uma superfície de ataque pequena e conhecida.
- O shadow da migração não exige nenhuma alteração no Node.
- Custo: um container a mais (cerca de 10 MB de RAM) e um arquivo de configuração para manter. Ficou decidido que vale a pena.
