# PvP, Área de Treinamento, shiny do servidor e raid

O app (A2) tira do WebSocket do jogo o que precisa para o Nexus aprender o PvP e medir o servidor. Nicks de outros jogadores não saem do app.

## O que o app grava (`nexus.evento`)

| Tipo | De onde | Conteúdo |
|---|---|---|
| `pvp.partida` | `pvp` com `partida.replay`, ao fim de cada partida | pontos antes e delta, vitória, motivo, mapa e, para cada lado (`meu` e `oponente`), espécie, nível, HP, alcance e **casa inicial** de cada pokémon |
| `pvp.estado` | `pvp` com `rank` | elo (tier, divisão, pontos, posição), time e casas atuais, regras (`nivelMin`) |
| `treino.resultado` | `treino` com `resultado` | quem venceu, motivo e placar por lado (dano, abates, casa de cada pokémon) |
| `shiny.servidor` | `shiny.captura` (aviso global) | id do pokémon, espécie, nome e nível |
| `boss.fichas` | `raidFichas` (o app pede `raid.fichas` a cada 6 h) | ficha de cada boss de raid: HP, stats, tipos, golpes, alcance, multiplicadores |

## Rotas (escopo `leitura`)

| Rota | O que devolve |
|---|---|
| `GET /api/pvp/meta?faixa=&dias=` | Por faixa de 100 pontos: espécies (usos, taxa de vitória, alcance, nível médio, casas mais usadas), melhores casas por alcance e times de oponente mais comuns. A taxa é suavizada: pouca amostra fica perto de 50%. |
| `GET /api/pvp/sugestao?nick=&faixa=` | Time de 5 e casas para a conta: força (`poder`), vantagem de tipo contra as 20 espécies mais usadas do meta e taxa da espécie no meta. Acima do nível mínimo do PvP. Traz o comando pronto (`pvp.formacao`). Com menos de 20 partidas na faixa, usa o meta de todas. |
| `GET /api/pvp/treinos?nick=` | As 50 últimas lutas da Área de Treinamento. |
| `GET /api/shiny/servidor?dias=` | Shiny do servidor inteiro: total, por hora, por espécie, por dia e por hora do dia (UTC−3). Cada shiny conta uma vez, mesmo que várias contas recebam o aviso. |
| `GET /api/boss/fichas` | Última ficha dos bosses de raid. |

Comando novo na fila: `pvp.formacao` `{pokemonIds: [≤5], casas: {id: 1..25}}` → o app manda `pvp.time.salvar`.

## Aprendizado de posição (Área de Treinamento)

Liga em Configurações (**Aprender PvP**), só no modo Ativo, na primeira conta da lista.

1. O app lê o time e as casas atuais (`pvp`) e o meta da faixa da conta (`/api/pvp/meta`, a cada 30 min).
2. A cada luta liberada (o jogo dá uma a cada 5 min), monta um dos 3 times mais comuns do meta como pokémon teste (nível médio, IV 31) nas casas que eles mais usam, e luta com o time do dono numa formação candidata.
3. Nota da luta: 1 por vitória, mais ou menos 0,5 pela diferença de dano.
4. Primeiro mede a formação atual (3 lutas); depois muda um pokémon por vez para outra casa do alcance dele (as casas que mais vencem no meta primeiro) e confirma as que vão bem.
5. Quando uma formação com pelo menos 3 lutas tem média 0,15 acima da atual, o app aplica no PvP (`pvp.time.salvar`).

As notas ficam em `relay-treino.json` na pasta de dados do app e sobrevivem a reinícios.

## Tabuleiro

Casas de 1 a 25 por lado. Pelo que as partidas mostram, as baixas ficam na frente (corpo a corpo: 1 a 5) e as altas no fundo (alcance 4: 21 a 25). Sem amostra no meta, a sugestão usa essa regra.
