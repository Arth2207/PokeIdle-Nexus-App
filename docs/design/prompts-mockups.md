# Prompts de mockup

Prompts usados para gerar os mockups de referência das telas (gerador de imagens do ChatGPT). A especificação de cada tela está em [telas.md](telas.md); estes prompts só traduzem essa especificação para o visual aprovado.

## Como usar

1. Gere todas as telas **na mesma conversa** em que a Visão geral aprovada foi gerada, para manter o estilo.
2. Cole o **bloco de estilo** no começo de cada prompt de tela.
3. Ajustes se fazem com pedidos curtos ("menos neon", "mais respiro"), não reescrevendo o prompt.

## Bloco de estilo (colar antes de cada tela)

```
Use exatamente o mesmo sistema visual da tela "Visão geral" já gerada nesta conversa.

SISTEMA VISUAL: app desktop 16:9, interface em português do Brasil, estética solarpunk quase futurista. Paisagem ilustrada em pintura digital de cenário de animação ocupando TODO o fundo, de borda a borda, com névoa branca suave atrás dos painéis. Painéis flutuantes de vidro fosco claro (branco ~75%, desfoque forte), cantos de 20px, borda de 1px branca semitransparente, sombra difusa. Marcações técnicas discretas nos cantos (IDs, coordenadas). Uma única cor neon: verde-limão #C8FF2E, usada com moderação para o item ativo, o que está "em execução", a linha principal de gráfico e o botão de ação principal. Verde #1F9D6B para melhora, coral #E5533D para piora. Texto grafite #16181C. Tipografia sans geométrica moderna; números grandes de traço fino; rótulos pequenos em caixa alta espaçada; fonte mono para horários e dados técnicos. Barra lateral esquerda de vidro fosco com o logo "Bestiário" e o menu agrupado: OPERAÇÃO (Visão geral, Jogo), ATIVIDADES (Caçada, Combate), ECONOMIA (Mercado, Finanças), CONHECIMENTO (Bestiário), SISTEMA (Automação, Configurações); no rodapé, o seletor "Conta A · Em execução". No topo da área principal: linha de filtros (conta, período, moeda) à esquerda; busca "⌘K", sino de alertas e o botão "Modo desempenho" à direita. Todas as criaturas e itens são ORIGINAIS, de nenhuma franquia existente.

REGRAS DE DASHBOARD: no máximo 4 KPIs por tela; cada KPI com valor, unidade, comparação (Δ absoluto e Δ% com ▲ ou ▼) e o texto da janela de tempo. Gráficos planos e limpos: sem 3D, sem sombra, sem brilho dentro do gráfico, grade discreta, tempo no eixo horizontal, um único eixo Y. Meta desenhada como linha ou contorno sem preenchimento; período anterior em cinza claro. Destaque por ênfase: o item que importa em verde-limão, os demais em cinza. Criaturas em 2.5D só nos cartões de destaque, nunca dentro de gráficos.

PROIBIDO: modo escuro, serifas, aparência vintage, neon em excesso, mais de uma cor neon, partículas, gráficos de pizza ou 3D, dois eixos no mesmo gráfico.
```

## 1. Visão geral (versão corrigida)

```
TELA: "Visão geral" (item ativo no menu). Bioma de fundo: floresta com riacho e montanhas ao fundo.

KPIs (faixa superior): principal maior "PATRIMÔNIO 1,28M orbs · ▲ +52,1k (+4,2%) em 7 dias" com barra fina de progresso até a meta de 1,5M (85%). Três secundários: "LUCRO 24H +38,4k orbs · ▲ +4,1k (+12%) vs. ontem"; "RENDIMENTO LÍQUIDO 5,1k orbs/h · média da hunt 4,7k"; "DISPONIBILIDADE 97,8% · meta 95% · últimas 24h" com pequeno selo verde de ok.

DESTAQUE: cartão "Escaneamento de campo" com a criatura ativa (raposa elemental de folhas e vento) em 2.5D saindo do card (efeito out of bounds: cauda e orelhas ultrapassam a moldura, sombra projetada no vidro), anel holográfico verde-limão sob as patas, linhas de medição finas (ATQ, VEL). Ao lado: nome "Folhális", REG 0317, tipos Planta e Vento, barras de HP e XP, caçada "Bosque Nebuloso · Nv. 72 · Em execução".

DIREITA: painel "Precisa de você" com 3 cartões de recomendação empilhados, cada um com ícone, frase curta, motivo em uma linha e botão: "Comprar 40× Folha Bruma: 18% abaixo do valor justo [Comprar]"; "Riacho Cristalino rende +18% para o seu nível [Trocar hunt]"; "Automação 'Venda noturna' falhou 2× hoje [Ver]".

BASE: "Lucro por dia · 7 dias", colunas finas com a coluna de hoje em verde-limão e as outras em cinza, linha tracejada não, linha contínua fina da média; e "Diário de eventos" com horários em mono e filtro por tipo.
```

## 2. Jogo

```
TELA: "Jogo" (item ativo no menu). Bioma de fundo: clareira florida, pouco visível porque os painéis ocupam quase tudo.

LAYOUT: grade 2×2 com quatro janelas de um jogo idle de criaturas (cada janela mostra uma cena diferente do jogo: criatura original lutando contra monstros selvagens numa floresta, num riacho, numa montanha nevada e numa caverna de cristais, com a interface própria do jogo em estilo pixel art simples). Cada janela é um painel de vidro com cantos de 20px.

SOBRE CADA JANELA: faixa fina de vidro no topo com: nome da conta (Conta A, B, C, D), ponto de status (verde-limão "Em execução", cinza "Pausada", coral "Erro de captura" numa delas), hunt atual, "kills/h 2.140", barra de HP mínima, e três ícones de controle (pausar, recarregar, ampliar). Uma das janelas (Conta C) está com status "Pausada" e um leve véu cinza.

TOPO: em vez de KPIs grandes, uma faixa compacta com "4 contas · 3 em execução · 1 pausada · captura OK em 3 de 4" e alternância de layout "Grade | Foco".

CANTO INFERIOR DIREITO: pequeno indicador "Desempenho: janelas fora de foco a 15 fps".
```

## 3a. Caçada · Sessão ao vivo

```
TELA: "Caçada", aba "Sessão ao vivo" ativa (abas: Sessão ao vivo | Mapa de hunts | Shiny). Bioma de fundo: Bosque Nebuloso, floresta com névoa matinal.

KPIs: "KILLS/H 2.140 · ▲ +160 (+8%) vs. média da hunt"; "ORBS/H 5,1k · ▲ +0,4k (+9%)"; "XP/H 32,1k · ▼ −1,2k (−4%)"; "CONSUMO/H 1,9 poções · 0,6 bolas · ▲ custo +6%".

PRINCIPAL: gráfico de linha "Rendimento da sessão (orbs/h)" nas últimas 3 horas, linha verde-limão fina, com uma faixa cinza clara sombreada indicando a faixa esperada para essa hunt; um pequeno trecho onde a linha caiu abaixo da faixa marcado com anotação discreta "queda: poção acabou 13:42".

DIREITA: painel "Real × simulador" com dois números lado a lado (previsto 4,9k orbs/h, real 5,1k orbs/h, Δ +4%) e uma frase "Simulador calibrado: erro médio 3%".

BASE: "Drops da sessão" em tabela limpa (item com ícone, quantidade, valor a preço justo, total) e "Eventos da sessão" em linha do tempo.
```

## 3b. Caçada · Mapa de hunts

```
TELA: "Caçada", aba "Mapa de hunts" ativa. Bioma de fundo: vale amplo com várias regiões visíveis (floresta, riacho, montanha).

KPIs: "MELHOR HUNT P/ SEU NÍVEL Riacho Cristalino · 6,0k orbs/h"; "SUA HUNT ATUAL Bosque Nebuloso · 5,1k orbs/h"; "GANHO POTENCIAL +0,9k orbs/h (+18%)".

PRINCIPAL: gráfico de barras horizontais ordenadas "Rendimento líquido por hunt (orbs/h)", 8 hunts, com a hunt atual em verde-limão e as outras em cinza, valor no fim de cada barra; seletor acima do gráfico "Métrica: Orbs/h | XP/h | Chance de raro".

DIREITA: cartão de recomendação "Trocar para Riacho Cristalino · +18% de orbs/h para o nível 72 · confiança alta" com botão verde-limão "Trocar hunt".

BASE: mapa de calor "Hunt × hora do dia" (8 linhas, 24 colunas), escala de uma só cor do cinza claro ao verde-escuro, com legenda; e uma pequena miniatura ilustrada do mapa de expedição com a rota atual em linha verde-limão.
```

## 4a. Combate · Boss

```
TELA: "Combate" (item ativo), página do boss "Drakar, o Vulcânico". Bioma de fundo: montanha vulcânica ao entardecer, ainda clara e luminosa.

TOPO: trilha "Combate › Bosses › Drakar, o Vulcânico" e o boss em 2.5D imponente à direita do cabeçalho, saindo do painel (out of bounds), com o selo "Vence com compras".

KPIs: "CHANCE DE VITÓRIA 78% · melhor equipe disponível · meta 90%"; "TEMPO DE LUTA 1m42s · mediana"; "CUSTO P/ COMPLETAR 124k orbs · 2 criaturas faltando"; "GANHO POR VITÓRIA 41k orbs · drops a preço justo".

PRINCIPAL: três colunas de equipes lado a lado, cada uma com 6 slots como cartas pequenas de criaturas originais (ilustração, nível, tipo): "IDEAL · 94%", "MELHOR DISPONÍVEL · 78%" (destacada com contorno verde-limão), "LABORATÓRIO · 83%" (dois slots com borda pontilhada indicando arraste e soltura, um com a criatura sendo arrastada). Abaixo de cada coluna, a chance de vitória numa barra fina com a meta marcada.

AÇÕES: botões "Aplicar build" (verde-limão) e "Comprar faltantes · 124k" (secundário), e um link "Simular luta ▶".
```

## 4b. Combate · Simulação

```
TELA: "Combate", aba "Simulação" ativa da luta contra "Drakar, o Vulcânico". Bioma de fundo: interior de cratera vulcânica luminosa.

PRINCIPAL (grande, centro): arena 2.5D cinematográfica com a equipe de 3 criaturas originais à esquerda e o boss à direita, em meio a uma luta: um golpe de vento atravessando a cena, número de dano flutuando "−2.480 CRÍTICO", barras de HP sobre cada lutador. Abaixo da arena, controles de reprodução: play/pausa, velocidade "1× 4× 16×" (4× ativo), e uma linha do tempo horizontal com marcadores dos eventos decisivos (ícones pequenos: troca, crítico, desmaio), cursor no tick 214 de 400.

DIREITA: painel "Mil simulações": histograma da duração da luta (colunas finas cinza, com a faixa de vitória em verde-limão), e abaixo "Vitórias 78% · Derrotas 22%" num medidor de barra única. Embaixo, cartão "Por que perde" com a frase "Em 64% das derrotas, a equipe cai no 3º Sopro de Magma (dano em área)" e sugestão "Trocar Lumisco por uma criatura tipo Água: +9%".
```

## 5a. Mercado · Pregão

```
TELA: "Mercado", aba "Pregão" ativa (abas: Pregão | Oportunidades | Inventário). Bioma de fundo: vila de mercado à beira de um riacho, com toldos e árvores.

ESQUERDA: lista de ativos com busca e filtros (Criatura | Item, Orb | Gold); cada linha com ícone, nome, preço e Δ 24h colorido; "Cristal de Vento" selecionado com contorno verde-limão.

CENTRO: gráfico de candles limpo de "Cristal de Vento" com seletor de período (24h | 7d | 30d, 7d ativo), média móvel em linha fina verde-limão, bandas de Bollinger como faixa cinza muito clara, linha horizontal tracejada não: linha contínua fina cinza para o "valor justo 47.800". Volume em colunas finas cinza embaixo, em gráfico separado com o mesmo eixo de tempo.

DIREITA: cartão "Veredito" com o sinal grande "COMPRAR" (verde-limão), medidor de confiança 82%, preço atual 44.100, valor justo 47.800 (Δ −7,7%), preço-alvo de venda 49.500, e os 3 motivos curtos em lista. Abaixo, "Melhores ofertas" com 4 linhas ordenadas por desconto e botão "Comprar" em cada.
```

## 5b. Mercado · Inventário

```
TELA: "Mercado", aba "Inventário" ativa. Bioma de fundo: interior de uma casa na árvore com estantes e luz natural, visto através do vidro.

KPIs: "VALOR DO INVENTÁRIO 812k orbs · a preço justo"; "PARA VENDER 23 itens · 96k orbs"; "PARA A COLEÇÃO 3 criaturas melhores que as atuais"; "PARADO HÁ +30 DIAS 41k orbs".

PRINCIPAL: tabela densa e limpa com abas de filtro (Tudo | Itens | Criaturas | Raros): colunas ícone, nome, quantidade, valor justo unitário, total, tendência 7d (mini linha cinza), veredito (pílula: "Vender" coral, "Guardar" cinza, "Coleção" verde-limão) e o motivo em uma linha. Caixas de seleção à esquerda, 5 linhas selecionadas.

RODAPÉ FIXO: barra de ação em vidro "5 selecionados · total 38,2k orbs · [Vender selecionados] [Guardar]".
```

## 6. Finanças

```
TELA: "Finanças" (item ativo). Filtros no topo: "Todas as contas", período "7 dias" ativo, moeda "Orbs". Bioma de fundo: campos dourados com moinhos de vento modernos ao longe.

KPIs: "LUCRO LÍQUIDO 268,4k orbs · ▲ +31,2k (+13%) vs. 7d anteriores · meta 250k" com selo verde; "RECEITAS 412,7k · ▲ +9%"; "CUSTOS 144,3k · ▲ +3% (piora, em coral)"; "PATRIMÔNIO 1,28M · ▲ +52,1k no período".

PRINCIPAL: gráfico de cascata (waterfall) horizontal no tempo da esquerda para a direita: barras de receita subindo (Loot de caçada 231k, Vendas no mercado 158k, Recompensas de boss 23,7k) em verde, barras de custo descendo (Compras no mercado −98k, Poções e bolas −41k, Taxas −5,3k) em coral, e a última barra "Lucro líquido 268,4k" em verde-limão. Rótulos de valor no topo de cada barra. Plano, sem 3D.

DIREITA: "Lucro por atividade" em barras horizontais ordenadas (Bosque Nebuloso, Revenda de Cristal de Vento, Drakar, Riacho Cristalino...), a primeira em verde-limão e as outras em cinza, com o ROI em texto ao lado.

BASE: "Patrimônio no período" em áreas empilhadas (caixa em cinza médio, itens em verde claro) com a linha do total; e "Calendário do lucro" como mapa de calor de 5 semanas, uma só cor.
```

## 7. Bestiário · Espécie

```
TELA: "Bestiário", ficha da espécie "Folhális" (abas: Espécie | Coleção). Bioma de fundo: bosque úmido com samambaias gigantes, o habitat natural da criatura.

DESTAQUE (metade esquerda): a criatura original (raposa elemental de folhas e vento) grande em 2.5D, saindo do painel com a cauda sobre a barra lateral, anel holográfico de escaneamento verde-limão no chão, e 4 linhas de medição finas com rótulos (ATQ 4.820, DEF 3.190, VEL 2.340, CRIT 24,6%). Seletor de nível "Nv. 72" com controle deslizante abaixo.

DIREITA: nome "Folhális", REG 0317, tipos; "Escada evolutiva" com três estágios em miniatura conectados por setas; "Onde rende" com barras horizontais ordenadas das 4 melhores hunts (a primeira em verde-limão); "Valor de mercado 21.300 orbs · ▲ +6,8% 7d"; e um cartão de veredito "Seu exemplar está no top 12% · Guardar e evoluir no Nv. 80".

BASE: "Golpes" em tabela compacta (nome, tipo, poder, efeito) e "Bosses em que é útil" como fileira de pequenos retratos com a chance de vitória.
```

## 8. Automação

```
TELA: "Automação", aba "Regras" ativa (abas: Regras | Decisões | Fila). Bioma de fundo: estufa futurista de vidro cheia de plantas.

KPIs: "DISPONIBILIDADE 97,8% · meta 95% · 24h"; "FALHAS 24H 2 · ▼ −3 vs. ontem"; "LUCRO ATRIBUÍDO 84,2k orbs · 7 dias"; "ACERTO DA IA 81% · decisões que melhoraram o resultado".

PRINCIPAL: lista de regras em cartões horizontais: nome ("Caçada automática", "Venda noturna", "Comprar poções", "Retomar após queda"), interruptor liga/desliga (verde-limão quando ligado), última e próxima execução em mono, lucro atribuído em 7 dias, e um selo de status com ícone ("OK", "Falhou 2×" em coral). A regra "Venda noturna" expandida mostrando seu editor simples ("SE estoque > 50 E preço ≥ valor justo ENTÃO vender 30%") e a prévia "Nos últimos 7 dias teria vendido 412 itens por 61k orbs".

BASE: "Decisões recentes da IA" em linha do tempo com o motivo e o resultado depois ("✓ +4,1k vs. alternativa", "✗ −0,8k").
```

## 9. Configurações

```
TELA: "Configurações" (item ativo). Bioma de fundo: mirante na montanha com vista ampla e céu limpo.

LAYOUT: navegação interna vertical à esquerda (Contas, Metas dos KPIs, Tokens e dispositivos, Aparência, Backup, Saúde), seção "Metas dos KPIs" aberta: lista dos KPIs com o objetivo de cada um, campo de meta editável, faixas (verde ≥ meta, amarelo ≥ 80%, vermelho < 80%) mostradas como uma barra segmentada fina e o sentido ("maior é melhor"). À direita, cartão "Saúde do servidor" compacto: "API no ar · banco acessível · versão a02a797 · último deploy há 2h" com ícones de ok e um link "Abrir no Grafana".
```

## 10. Tema noturno

```
Gere a MESMA tela "Visão geral" (layout, textos, números e criatura idênticos), agora no TEMA NOTURNO.

FUNDO: EXATAMENTE a mesma paisagem da versão diurna, com a mesma composição (as mesmas árvores, o riacho, as montanhas e a cidade-jardim nas mesmas posições), só que à noite: céu azul-profundo com estrelas e uma lua grande e suave, a cidade-jardim ao fundo com janelas acesas em luz quente, plantas e cogumelos bioluminescentes em verde-limão e ciano suave, vagalumes discretos perto do riacho, reflexo da lua na água. Atmosfera calma e mágica, não sombria.

PAINÉIS: vidro fumê escuro (#0E1116 a ~70% de opacidade, desfoque forte), borda de 1px branca a 10%, sem sombra pesada. Texto principal off-white #EEF0F3, secundário #9AA3AE.

CORES: o mesmo verde-limão #C8FF2E como único neon, agora com um brilho levíssimo que conversa com as plantas bioluminescentes; verde de melhora #3DDC97 e coral de piora #FF6B57 (tons ajustados para fundo escuro). Gráficos continuam planos: linha verde-limão, área com preenchimento verde muito transparente, grade em branco a 8%.

CRIATURA: a raposa Folhális com as folhas emitindo brilho suave verde-limão, como se fosse bioluminescente; anel holográfico mais visível no escuro.

Mesmas regras: sem neon em excesso, sem partículas além dos vagalumes, sem gráficos 3D.
```

## Prompt único (todas as telas, em 2 lotes)

São 18 imagens. Para o gerador não travar no meio, mande em dois lotes, na mesma conversa: primeiro o lote 1; quando as 9 imagens saírem, o lote 2.

### Lote 1 (imagens 1 a 9)

```
Vou pedir 9 imagens. Gere UMA imagem por tela, em sequência, uma de cada vez, sem juntar telas na mesma imagem. Depois de cada imagem, siga para a próxima. Todas usam o mesmo sistema visual abaixo (o mesmo da "Visão geral" desta conversa).

SISTEMA VISUAL: app desktop 16:9, português do Brasil, estética solarpunk quase futurista. Paisagem ilustrada em pintura digital de cenário de animação ocupando TODO o fundo, com névoa branca suave atrás dos painéis; o bioma muda por tela. Painéis flutuantes de vidro fosco claro (branco ~75%, desfoque forte), cantos de 20px, borda de 1px branca semitransparente, sombra difusa, marcações técnicas discretas nos cantos (IDs, coordenadas). Único neon: verde-limão #C8FF2E, com moderação (item ativo, "em execução", linha principal de gráfico, botão principal). Verde #1F9D6B melhora, coral #E5533D piora, texto grafite #16181C. Sans geométrica moderna, números grandes de traço fino, rótulos em caixa alta espaçada, mono para horários. Barra lateral de vidro com logo "Bestiário" e menu agrupado: OPERAÇÃO (Visão geral, Jogo), ATIVIDADES (Caçada, Combate), ECONOMIA (Mercado, Finanças), CONHECIMENTO (Bestiário), SISTEMA (Automação, Configurações); rodapé "Conta A · Em execução". Topo: filtros (conta, período, moeda) à esquerda; busca "⌘K", alertas e "Modo desempenho" à direita.

CRIATURAS: originais, de nenhuma franquia. Sempre como RENDER 3D ESTILIZADO de figura de jogo: superfície lisa, volume e sombreamento suave, luz de contorno, sem pelagem realista. Nos cartões de destaque, saem do card (out of bounds) com sombra projetada no vidro, aura de luz suave atrás e reflexo verde-limão do anel holográfico nas patas. Nunca dentro de gráficos. Em listas, aparecem pequenas e sem efeitos.

REGRAS DE DASHBOARD: até 4 KPIs por tela, cada um com valor, unidade, Δ absoluto e Δ% com ▲/▼ e a janela de tempo. Gráficos planos (sem 3D, sombra ou brilho), grade discreta, tempo no eixo horizontal, um só eixo Y, meta como linha ou contorno, período anterior em cinza claro, destaque por ênfase (verde-limão no que importa, cinza no resto). Status sempre com ícone e texto, nunca só cor.

PROIBIDO (exceto quando a imagem pedir tema noturno): modo escuro, serifas, visual vintage, neon em excesso, partículas, pizza, gráfico 3D, dois eixos.

IMAGEM 1 · VISÃO GERAL (bioma: floresta com riacho e montanhas). KPIs: "PATRIMÔNIO 1,28M orbs · ▲ +52,1k (+4,2%) 7 dias" com barra até a meta 1,5M (85%); "LUCRO 24H +38,4k · ▲ +12% vs. ontem"; "RENDIMENTO LÍQUIDO 5,1k orbs/h · média 4,7k"; "DISPONIBILIDADE 97,8% · meta 95%". Destaque: "Escaneamento de campo" com a raposa elemental de folhas e vento "Folhális" saindo do card, anel holográfico, HP/XP, caçada "Bosque Nebuloso · Nv. 72 · Em execução". Direita: "Precisa de você" com 3 recomendações com botão (comprar Folha Bruma 18% abaixo do justo; trocar para Riacho Cristalino +18%; automação "Venda noturna" falhou 2×). Base: "Lucro por dia · 7 dias" em colunas finas (hoje em verde-limão, demais cinza, linha da média) e "Diário de eventos".

IMAGEM 2 · JOGO (bioma: clareira florida). Grade 2×2 com quatro janelas de um jogo idle de criaturas em pixel art simples (floresta, riacho, montanha nevada, caverna de cristais). Sobre cada janela, faixa de vidro com conta (A, B, C, D), status com ícone (verde-limão "Em execução"; Conta C "Pausada" com véu cinza; uma com "Erro de captura" em coral), hunt, kills/h, HP mínimo e ícones pausar/recarregar/ampliar. Topo: "4 contas · 3 em execução · 1 pausada · captura OK em 3 de 4" e alternância "Grade | Foco". Canto: "Janelas fora de foco a 15 fps".

IMAGEM 3 · CAÇADA › SESSÃO AO VIVO (bioma: bosque com névoa matinal). Abas: Sessão ao vivo | Mapa de hunts | Shiny. KPIs: KILLS/H 2.140 ▲+8% vs. média; ORBS/H 5,1k ▲+9%; XP/H 32,1k ▼−4%; CONSUMO/H custo ▲+6% (coral). Principal: linha verde-limão "Rendimento da sessão (orbs/h)" das últimas 3h sobre faixa cinza da faixa esperada, anotação "queda: poção acabou 13:42". Direita: "Real × simulador" (previsto 4,9k, real 5,1k, Δ +4%). Base: tabela "Drops da sessão" e linha do tempo de eventos.

IMAGEM 4 · CAÇADA › MAPA DE HUNTS (bioma: vale com várias regiões). KPIs: melhor hunt p/ seu nível "Riacho Cristalino 6,0k orbs/h"; sua hunt "5,1k"; ganho potencial "+0,9k (+18%)". Principal: barras horizontais ordenadas de 8 hunts por rendimento líquido (atual em verde-limão, demais cinza), seletor "Orbs/h | XP/h | Chance de raro". Direita: recomendação "Trocar para Riacho Cristalino · confiança alta" com botão. Base: mapa de calor hunt × hora do dia (uma cor, cinza→verde-escuro, com legenda) e miniatura do mapa de expedição com a rota em verde-limão.

IMAGEM 5 · CAÇADA › SHINY (bioma: gruta com cristais e água azul-turquesa iluminada). KPIs: "SHINIES NO MÊS 3 · ▲ +1 vs. mês anterior"; "CHANCE NA HUNT ATUAL 1 em 3.870 encontros"; "ENCONTROS DESDE O ÚLTIMO 2.912". Principal: barras horizontais ordenadas "Chance de shiny por hunt", cada barra com o intervalo de confiança desenhado como traço fino nas pontas, a hunt atual em verde-limão. Direita: cartão "Melhor lugar agora: Gruta Turquesa · 1 em 2.950" com botão "Trocar hunt". Base: galeria dos shinies capturados como pequenas cartas com a criatura em versão de cor alternativa, data e hunt.

IMAGEM 6 · COMBATE › BOSSES (bioma: cordilheira com picos e ruínas antigas ao longe). KPIs: "VENCE AGORA 7 de 25"; "VENCE COM COMPRAS 9"; "FORA DE ALCANCE 9"; "VITÓRIAS NO MÊS 42 · ▲ +6". Principal: grade de cartões de bosses originais (retrato do boss em render 3D pequeno, nome, nível, chance de vitória em barra fina com a meta marcada, e selo com ícone: "Vence agora" verde, "Vence com compras" âmbar, "Fora de alcance" cinza). Filtros acima: "Todos | Vence agora | Com compras | Fora de alcance" e ordenar por "Ganho por vitória".

IMAGEM 7 · COMBATE › BOSS (bioma: montanha vulcânica luminosa ao entardecer). Trilha "Combate › Bosses › Drakar, o Vulcânico", boss original saindo do cabeçalho, selo "Vence com compras". KPIs: CHANCE DE VITÓRIA 78% (meta 90%); TEMPO DE LUTA 1m42s; CUSTO P/ COMPLETAR 124k (2 faltando); GANHO POR VITÓRIA 41k. Principal: três colunas de 6 cartas de criaturas: "IDEAL 94%", "MELHOR DISPONÍVEL 78%" (contorno verde-limão), "LABORATÓRIO 83%" (slots pontilhados, uma carta sendo arrastada); sob cada uma, barra de chance com a meta marcada. Botões "Aplicar build", "Comprar faltantes · 124k", "Simular luta ▶".

IMAGEM 8 · COMBATE › SIMULAÇÃO (bioma: cratera vulcânica luminosa). Arena 2.5D cinematográfica: 3 criaturas contra o boss, golpe de vento, dano "−2.480 CRÍTICO", barras de HP. Abaixo: play/pausa, velocidade 1× 4× 16× (4× ativo), linha do tempo com marcadores de eventos, cursor no tick 214/400. Direita: "Mil simulações" com histograma da duração (faixa de vitória em verde-limão), medidor "Vitórias 78% · Derrotas 22%" e cartão "Por que perde: em 64% das derrotas a equipe cai no 3º Sopro de Magma" com sugestão "+9% trocando por tipo Água".

IMAGEM 9 · MERCADO › PREGÃO (bioma: vila de mercado à beira do riacho). Abas: Pregão | Oportunidades | Inventário. Esquerda: lista de ativos com busca e filtros, "Cristal de Vento" selecionado. Centro: candles 7d com média móvel verde-limão, bandas em cinza claro, linha cinza do valor justo 47.800; volume em gráfico separado embaixo, mesmo eixo de tempo. Direita: "Veredito COMPRAR", confiança 82%, preço 44.100, valor justo 47.800 (−7,7%), alvo de venda 49.500, 3 motivos; "Melhores ofertas" com botão Comprar.
```

### Lote 2 (imagens 10 a 18)

```
Agora mais 9 imagens, no MESMO sistema visual das anteriores desta conversa. Uma imagem por tela, em sequência.

SISTEMA VISUAL: app desktop 16:9, português do Brasil, estética solarpunk quase futurista. Paisagem ilustrada em pintura digital de cenário de animação ocupando TODO o fundo, com névoa branca suave atrás dos painéis; o bioma muda por tela. Painéis flutuantes de vidro fosco claro (branco ~75%, desfoque forte), cantos de 20px, borda de 1px branca semitransparente, sombra difusa, marcações técnicas discretas nos cantos (IDs, coordenadas). Único neon: verde-limão #C8FF2E, com moderação (item ativo, "em execução", linha principal de gráfico, botão principal). Verde #1F9D6B melhora, coral #E5533D piora, texto grafite #16181C. Sans geométrica moderna, números grandes de traço fino, rótulos em caixa alta espaçada, mono para horários. Barra lateral de vidro com logo "Bestiário" e menu agrupado: OPERAÇÃO (Visão geral, Jogo), ATIVIDADES (Caçada, Combate), ECONOMIA (Mercado, Finanças), CONHECIMENTO (Bestiário), SISTEMA (Automação, Configurações); rodapé "Conta A · Em execução". Topo: filtros (conta, período, moeda) à esquerda; busca "⌘K", alertas e "Modo desempenho" à direita.

CRIATURAS: originais, de nenhuma franquia. Sempre como RENDER 3D ESTILIZADO de figura de jogo: superfície lisa, volume e sombreamento suave, luz de contorno, sem pelagem realista. Nos cartões de destaque, saem do card (out of bounds) com sombra projetada no vidro, aura de luz suave atrás e reflexo verde-limão do anel holográfico nas patas. Nunca dentro de gráficos. Em listas, aparecem pequenas e sem efeitos.

REGRAS DE DASHBOARD: até 4 KPIs por tela, cada um com valor, unidade, Δ absoluto e Δ% com ▲/▼ e a janela de tempo. Gráficos planos (sem 3D, sombra ou brilho), grade discreta, tempo no eixo horizontal, um só eixo Y, meta como linha ou contorno, período anterior em cinza claro, destaque por ênfase (verde-limão no que importa, cinza no resto). Status sempre com ícone e texto, nunca só cor.

PROIBIDO (exceto quando a imagem pedir tema noturno): modo escuro, serifas, visual vintage, neon em excesso, partículas, pizza, gráfico 3D, dois eixos.

IMAGEM 10 · MERCADO › OPORTUNIDADES (bioma: porto fluvial com barcos e armazéns modernos de madeira e vidro). KPIs: "OFERTAS ABAIXO DO JUSTO 37"; "DESCONTO MÉDIO 14,2%"; "LUCRO POTENCIAL 186k orbs · se revender ao justo"; "ORÇAMENTO 250k orbs" com barra de uso. Principal: tabela ordenada por "desconto × confiança": ícone e nome do item ou criatura, preço, valor justo, desconto em %, confiança em mini medidor, liquidez (alta/média/baixa com ícone), botão "Comprar" em cada linha; filtros acima (tipo, moeda, desconto mínimo, orçamento). Direita: cartão "Seleção sugerida para o orçamento: 9 ofertas · 241k · lucro potencial 41k" com botão "Comprar seleção".

IMAGEM 11 · MERCADO › INVENTÁRIO (bioma: casa na árvore com estantes, luz natural). KPIs: VALOR DO INVENTÁRIO 812k; PARA VENDER 23 itens · 96k; PARA A COLEÇÃO 3 criaturas; PARADO +30 DIAS 41k. Principal: tabela com filtros (Tudo | Itens | Criaturas | Raros), colunas ícone, nome, qtd, valor justo, total, mini tendência 7d, veredito em pílula com ícone (Vender coral, Guardar cinza, Coleção verde-limão) e motivo; 5 linhas selecionadas. Rodapé: "5 selecionados · 38,2k orbs · [Vender selecionados] [Guardar]".

IMAGEM 12 · FINANÇAS (bioma: campos dourados com moinhos modernos). Filtros: Todas as contas, 7 dias, Orbs. KPIs: LUCRO LÍQUIDO 268,4k ▲+13% (meta 250k, selo ok com ícone); RECEITAS 412,7k ▲+9%; CUSTOS 144,3k ▲+3% (coral, piora); PATRIMÔNIO 1,28M ▲+52,1k. Principal: gráfico de cascata plano: receitas subindo em verde (Loot 231k, Vendas 158k, Bosses 23,7k), custos descendo em coral (Compras −98k, Poções e bolas −41k, Taxas −5,3k), última barra "Lucro líquido 268,4k" em verde-limão. Direita: "Lucro por atividade" em barras ordenadas com ROI em texto. Base: patrimônio em áreas empilhadas (caixa e itens) e calendário de lucro em mapa de calor.

IMAGEM 13 · BESTIÁRIO › ESPÉCIE (bioma: bosque úmido com samambaias gigantes). Folhális grande saindo do painel, anel de escaneamento, linhas de medição (ATQ 4.820, DEF 3.190, VEL 2.340, CRIT 24,6%), controle deslizante "Nv. 72". Direita: nome, REG 0317, tipos, escada evolutiva com 3 estágios, "Onde rende" em barras ordenadas, "Valor de mercado 21.300 ▲+6,8%", veredito "Seu exemplar está no top 12% · guardar e evoluir no Nv. 80". Base: tabela de golpes e retratos dos bosses em que é útil com a chance de vitória.

IMAGEM 14 · BESTIÁRIO › COLEÇÃO (bioma: biblioteca-estufa com plantas e prateleiras altas). KPIs: "COMPLETUDE 63% · 806 de 1.279" em medidor; "NOVAS NA SEMANA 7 · ▲ +2"; "FALTAM NA SUA REGIÃO 41". Principal: grade densa de pequenos cartões de espécies (as que a conta possui em cor, as que faltam em silhueta cinza com o número de registro), filtros por tipo, raridade e região, e um cartão destacado "Mais perto de capturar: Brumelo · Riacho Cristalino · 1 em 420". Lateral: barras de completude por região.

IMAGEM 15 · AUTOMAÇÃO › REGRAS (bioma: estufa futurista de vidro cheia de plantas). Abas: Regras | Decisões | Fila. KPIs: DISPONIBILIDADE 97,8% (meta 95%); FALHAS 24H 2 ▼−3; LUCRO ATRIBUÍDO 84,2k · 7 dias; ACERTO DA IA 81%. Principal: cartões horizontais de regras ("Caçada automática", "Venda noturna", "Comprar poções", "Retomar após queda") com interruptor, última/próxima execução em mono, lucro atribuído e status com ícone ("OK", "Falhou 2×" em coral). "Venda noturna" expandida com editor simples ("SE estoque > 50 E preço ≥ valor justo ENTÃO vender 30%") e prévia "Nos últimos 7 dias teria vendido 412 itens por 61k orbs".

IMAGEM 16 · AUTOMAÇÃO › DECISÕES (mesmo bioma da imagem 15, outro ângulo). Aba "Decisões" ativa. KPIs: ACERTO DA IA 81% ▲+3 pp; DECISÕES 7 DIAS 214; GANHO VS. ALTERNATIVA +23,6k orbs. Principal: linha do tempo de decisões, cada uma com ícone, o que decidiu ("Trocou para Riacho Cristalino"), o motivo em uma linha, a confiança, e o resultado depois com ícone ("✓ +4,1k vs. alternativa", "✗ −0,8k"). Direita: barras horizontais "Acerto por tipo de decisão" (troca de hunt, compra, venda, build), e uma pequena fila "Próximas na fila" com 3 comandos e botão "Cancelar".

IMAGEM 17 · CONFIGURAÇÕES (bioma: mirante na montanha com vista ampla). Navegação interna à esquerda (Contas, Metas dos KPIs, Tokens e dispositivos, Aparência, Backup, Saúde), seção "Metas dos KPIs" aberta: lista dos KPIs com o objetivo de cada um, campo de meta editável, faixas (verde ≥ meta, âmbar ≥ 80%, coral < 80%) como barra segmentada fina e o sentido ("maior é melhor"). Na seção Aparência, visível em miniatura: "Tema: Claro | Noturno | Seguir horário" e "Imagem das criaturas: Render 3D | Arte oficial". À direita, cartão "Saúde do servidor": "API no ar · banco acessível · versão a02a797 · último deploy há 2h" com ícones e o link "Abrir no Grafana".

IMAGEM 18 · VISÃO GERAL NO TEMA NOTURNO. A mesma tela da imagem 1, idêntica em layout e dados, com EXATAMENTE a mesma paisagem de fundo (mesmas árvores, riacho, montanhas e cidade-jardim nas mesmas posições), só que à noite: céu azul-profundo com estrelas e lua suave refletida no riacho, janelas da cidade-jardim acesas em luz quente, plantas e cogumelos bioluminescentes em verde-limão e ciano, vagalumes discretos; calmo e mágico, não sombrio. Painéis em vidro fumê escuro (#0E1116 ~70%, desfoque forte, borda branca a 10%), texto off-white #EEF0F3 e secundário #9AA3AE. Mesmo verde-limão com brilho levíssimo; melhora #3DDC97, piora #FF6B57. A criatura com luz de contorno azulada de luar e o reflexo do anel mais forte. Gráficos continuam planos.
```
