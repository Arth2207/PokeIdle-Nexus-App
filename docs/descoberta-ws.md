# Descoberta: o que o WS do jogo entrega

Levantado em 14/11/2026 com o modo Descoberta da A2 (`ws.bruto` em `nexus.evento`), com o dono clicando em todas as telas. `in` = o jogo manda; `out` = o cliente pede. Segredos (`token`, `session`, `auth`…) saem do app como `<removido>`.

## PvP

| Mensagem | Dir | Conteúdo útil |
|---|---|---|
| `pvp` | in | Rank (tier, divisão, PR, posição, V/D, sequência), fila (`na`, `cooldownMs`), `time` (ids) e `casas` (`{pokemonId: casa}`), `regras` (`nivelMin` 150, `entrePartidasMs` 20 s, `revancheMs` 10 min), `historico` e, ao fim de cada partida, `partida` com o **replay** |
| replay (`partida.replay`, `treino.resultado.replay`) | in | `atores` (espécie, nível, HP, alcance `alc`, lado, dono), `inicio` (trincas ator, x, y: **a casa de cada pokemon dos dois lados**), `golpes`, `quadros`, `dur` |
| `pvp.time.salvar` | out | `{casas: {pokemonId: casa}, pokemonIds: [5]}`: **aplica a formação** |
| `pvp.formacao.salvar` | out | Igual, com `nome` e `slot`: guarda formação |
| `pvp.info`, `pvp.tabela`, `pvp.mensal`, `pvp.partida.ficha`/`vista` | out | Pedem rank, tabela, mensal e partidas passadas |
| `pvpTabela`, `pvpMensal` | in | Top 100 por página (semana e mês) |

O tabuleiro do adversário é secreto na ficha e no Tracker, mas **o replay mostra onde cada pokemon dele nasceu**: cada partida ensina o posicionamento do meta.

## Área de Treinamento

| Mensagem | Dir | Conteúdo útil |
|---|---|---|
| `treino.lutar` | out | `a`: até 5 `{casa, teste: {speciesId, level, ivs, qualidade, potencia, shiny, tmFisico, tmElemental}}` (pokemon teste); `b`: até 5 `{meu: pokemonId, casa}` |
| `treino` | in | `resultado`: `venceuA`, `motivo` (`wipe`…), `placar` por lado (dano, abates, mortes), replay e **`cooldownMs` 300000 (5 min por conta)** |

Dá para montar qualquer equipe do meta como pokemon teste e lutar contra a equipe real do dono em casas escolhidas: é a bancada do aprendizado de posição.

## Boss e raid

| Mensagem | Dir | Conteúdo útil |
|---|---|---|
| `boss.entrar` | out | `{key}` (ex.: `articuno`) |
| `raidFichas` | in | Ficha de cada boss: HP máx., nível, stats, tipos, golpes (max/gmax/onda), alcance, qualidade, IV, `multAtaque`, `multDefesa`, `furiaDobraMs` |
| `raid` | in | Edição atual, fases e horários, tabela de bônus, gigantamax da vez |
| `ranking.pedir` | out | `{aba, bossKey}`: ranking por boss |

## Hunt, coleção e automação

| Mensagem | Dir | Conteúdo útil |
|---|---|---|
| `batalha` | in | Eventos da luta (kill, HP, espécie, tipos, slot) |
| `estado` | in | Estado completo no login (~200 KB; coleção e inventário) e deltas pequenos depois |
| `auto.set` | out | Configuração de automação do jogo: bolas, poções, revives, limiar de HP, auto-coleção (IV, potência, nota, shiny), `bossAutoKey`, `pvpAutoFila`, travas |
| `hunt.select` | out | `{slug}`: troca de hunt |
| `centro.curar`, `centro.desistir` | out | Centro Pokemon |
| `shop.buy`, `shop.sellPokemon`, `shop.sellAllPokemons` | out | Loja |

## Mercado, ranking e social

| Mensagem | Dir | Conteúdo útil |
|---|---|---|
| `market` | in | Listagens (até ~22 KB) |
| `market.listar`, `market.especies`, `market.itens`, `market.historico*`, `market.meus`, `market.medias` | out | Filtros completos (IV, nota, potência, nível, preço, shiny, P5, lendário…) |
| `chat` | in | Mensagens; `anuncioChat` traz anúncio do mercado (item, preço, moeda, shiny) |
| `shiny.captura` | in | **Aviso global de shiny capturado** por qualquer jogador (espécie, nível) |
| `ranking`, `ranking.ficha` | in | Top 100 e a ficha completa de um pokemon de outro jogador (IVs, stats, refino, potência, nota) |
| `topCatch`, `ginasio`, `showdown`, `campeonato`, `guild`, `amigos`, `mg`, `casas.lista`, `bicicletas.lista` | in | Telas próprias; `campeonato` (~280 KB) e `guild` (~125 KB) são grandes |

## Limites do modo Descoberta

- Até 3 MB por mensagem (acima disso guarda o começo do texto) e 120 por minuto por tipo e conta.
- Liga em Configurações; vale ao clicar em Conectar. Deixar desligado fora de testes.
