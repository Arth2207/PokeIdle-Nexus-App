# Fase 6 — scoring e IA

**Decisão (2026-11-13, dono do projeto): aposentar sem portar.**

## Levantamento

| Legado | Estado em 2026-11-13 |
|---|---|
| `scoring.js` (pm2 `scoring`, porta 8182, só loopback) | no ar, **sem clientes**: nenhuma conexão, log só com inicialização |
| `ai-observer.js` (pm2, parado) | único cliente do scoring junto com o `ai-decisao` |
| `ai-decisao.js`, `compras-ai.js`, `boss-builder.js`, `coordinator.js`, `sessao-monitor.js` | fora do pm2 e do cron; últimos logs de 2026-11-07 |

Sem tráfego não há o que comparar em sombra, e portar ~1.700 linhas de JS parado não traz
ganho hoje. Se a IA voltar a ser usada, porta-se nessa hora (com o comparador ativo do
padrão das outras fases).

## O que fica para a Fase 7

- Parar o pm2 `scoring` e remover `ai-observer` do pm2.
- As tabelas que só esses scripts usavam (`ia_memoria`, `ia_playbook`, `v_ia_performance`,
  `config_ia`, `ia_estado`) ficam como estão; as rotas `/api/ia/*` do dossiê respondem 501
  com chave válida (não há GRANT nem cliente).
- `comando_fila`: os produtores de comando da IA estão parados; a ponte (espelho da Fase 3)
  continua até o cutover da Fase 3.
