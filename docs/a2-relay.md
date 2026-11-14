# A2 como relay oficial (pré-requisito da Fase 3)

O Electron do PC tem dois pedaços: o **motor** local (`/opt/mapper/pacote/motor.js`, porta 47821), que lê o jogo e varre o mercado, e o **relay** (`/opt/mapper/holodex-relay.js`), que lê o motor e manda ao servidor. Para o Electron sair, a A2 assume as duas partes, em Rust, reaproveitando as rotas do servidor que já existem e têm paridade (o servidor não muda de contrato).

| # | Fluxo do relay | Rota no servidor | Fonte no motor | Na A2 |
|---|---|---|---|---|
| 1 | eventos | `/ingest` (WS na A2) | coletor da página | feito |
| 2 | comandos | `/api/comandos`, `/api/confirmar-comando` (WS na A2) | — | feito (Ativo sem teste real) |
| 3 | ofertas do mercado | `POST /api/bolsa/ofertas` | varredura `market.listar` (faixas de nível × moeda, ou espécies alvo), 1 pedido a cada 5 s | feito (sombra) |
| 4 | resumo de itens | `POST /api/bolsa/resumo-itens` | `market.itens` + `market.item` por item e moeda | feito: anúncios de item vão junto das ofertas |
| 5 | vendas (Dossiê) | `/ingest` | `market.historicoGlobal`, página 0 a cada 3 min, retroage até 36 h | a fazer |
| 6 | alvos de coleta | `GET /api/bolsa/alvos-coleta` (30 min; 10 min com faltas) | entrada da varredura | feito |
| 7 | equipe e estado | `POST /api/equipes/ingest` | `welcome`/`estado` do WS | a fazer |
| 8 | meus itens | `POST /api/bolsa/meus-itens` | inventário do `estado` | a fazer |
| 9 | raros e histórico | `POST /api/raros/ingest` | coleção + resumo do dia | a fazer |
| 10 | hunt | `POST /api/hunt-sync` | configuração da conta | a fazer |

## Como fica

- **Varredura do mercado em Rust** (`a2-nucleo`): máquina de estados com o mesmo ritmo do motor (uma conta dona; um pedido a cada 5 s; pausa de 5 min por volta; limite de pedidos por rodada). Os pedidos saem pelo `__hdxCmd` do coletor; as respostas `market`/`market.*` voltam pelo coletor (fora do modo Descoberta, só esses tipos).
- **Conversão** para o formato que o relay mandava fica no `a2-nucleo`, com testes contra amostras reais do modo Descoberta.
- **Envio** pelas rotas HTTP atuais com o mesmo token do app (escopo `relay`).
- Cada fluxo roda em **sombra** ao lado do Electron (o servidor marca a origem) até bater com o do relay; só então a Fase 3 vira.

Melhorias que mudam resultado (mais páginas, outra cadência) ficam atrás de `NEXUS_MELHORIAS`.
