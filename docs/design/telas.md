# Arquitetura de telas do app

Redesenho das telas do painel do Holodex Nexus para o app novo (Tauri 2 + Leptos, [ADR 0009](../adr/0009-app-desktop-tauri-leptos.md)). A referência visual é o mockup "solarpunk" da Visão geral: paisagem ilustrada do bioma da caçada em tela cheia, painéis de vidro fosco claro e um único acento verde-limão.

## Por que redesenhar

O painel atual tem 15 telas organizadas pela **origem dos dados**, não pela tarefa de quem usa. O resultado:

- **Equipe contra boss** aparece em quatro telas (`equipes`, `builds`, `builds-avancado`, `combate`).
- **Decidir o que vender** está dividido entre `capturas` e `vender`, e os itens raros ficam à parte, no `raros`.
- **Dinheiro** fica espalhado entre `bolsa`, `financeiro` e `raros`.
- O **Financeiro** só olha o saldo de ouro por dia. Não separa receita de custo, não mostra lucro por atividade e ignora o patrimônio em itens e criaturas.
- Cada tela tem layout próprio, então não há padrão a aprender.

## Princípios

### Uma pergunta por tela

Toda tela existe para responder uma pergunta, escrita no topo deste documento para cada uma. Se um bloco não ajuda a responder essa pergunta, ele sai da tela.

### Esqueleto fixo

Toda tela segue a mesma ordem, de cima para baixo:

1. **Filtros** numa linha só, no topo, valendo para a tela inteira: conta (uma ou todas), período, moeda.
2. **KPIs**: no máximo 4, como stat tiles.
3. **Visualização principal**: o gráfico ou a cena que responde à pergunta.
4. **Detalhe**: tabelas, listas, histórico. Aparece sob demanda (abas, expandir, drill-down).

### Regras de KPI

Cada KPI tem, obrigatoriamente:

- **definição escrita** (o que é contado e como), disponível num ícone de ajuda;
- **unidade** explícita (orbs, gold, kills/h, %);
- **comparação**: período anterior, média ou meta. Um número sozinho não diz se está bom;
- **frescor**: "atualizado há 2 min" quando o dado não é em tempo real.

Métricas de vaidade (que só crescem e não levam a nenhuma decisão, como "total de kills desde sempre") não viram KPI.

### Regras de visualização

- A forma do gráfico vem da pergunta: comparar → barras ordenadas; tendência → linha; parte do todo → barra segmentada ou barras ordenadas (nunca pizza para comparar valores próximos); composição de um resultado → cascata (waterfall); densidade por dia ou hora → mapa de calor.
- **Ênfase em vez de arco-íris**: quando a história é "esta conta" ou "esta hunt", ela leva o acento e o resto fica em cinza.
- **Nunca dois eixos Y** no mesmo gráfico. Duas medidas de escala diferente viram dois gráficos alinhados.
- **Orbs e gold nunca são somados** sem conversão explícita. Há um seletor de moeda; o modo "total" só existe se houver uma taxa de câmbio declarada e visível.
- Mais de 7 categorias com significado → tabela.
- Toda visualização tem tooltip e uma visão em tabela. A cor nunca é o único jeito de ler um valor.
- Status (ok, atenção, crítico) usa cores reservadas, sempre com ícone e texto.

### Recomendação sempre com motivo e ação

Toda sugestão do sistema ("vender", "trocar de hunt", "comprar faltantes") aparece num **cartão de veredito**: a recomendação, o motivo principal em uma linha, a confiança e o botão que executa a ação ali mesmo.

## Navegação

```
OPERAÇÃO      Visão geral · Jogo
ATIVIDADES    Caçada · Combate
ECONOMIA      Mercado · Finanças
CONHECIMENTO  Bestiário
SISTEMA       Automação · Configurações
```

Elementos globais, presentes em toda tela:

- **Seletor de conta**, com a opção "Todas as contas", que troca as telas para o modo comparativo.
- **Busca e comandos (⌘K)**: encontra espécie, item, boss ou hunt e executa ações ("pausar conta B", "ir para Floresta das Brasas").
- **Central de alertas**: estoque baixo, shiny encontrado, boss liberado, automação falhou.
- **Modo desempenho**: desliga paralaxe, animações e o 3D.
- **Fundo**: a paisagem do bioma da caçada da conta ativa.

## Mapa do legado para o novo

| Tela atual | Destino | Observação |
|---|---|---|
| `index` (ficha da espécie) | Bestiário › Espécie | vira o "Escaneamento de campo" |
| `aovivo` | Caçada › Sessão ao vivo; alertas e "O que melhorar" vão para a Visão geral | |
| `farm` | Caçada › Mapa de hunts | |
| `shiny` | Caçada › Shiny | |
| `combate`, `equipes`, `builds`, `builds-avancado` | Combate (fluxo único por boss) | quatro telas viram uma |
| `bolsa` | Mercado › Pregão e Oportunidades | |
| `vender`, `capturas`, `raros` | Mercado › Inventário | decisão guardar / vender / coleção num lugar só |
| `financeiro` | Finanças | reescrita completa |
| `automacoes` | Automação › Regras | |
| `ia-painel` (decisões, fila, scoring) | Automação › Decisões e Fila | |
| `ia-painel` (processos, banco de dados) | **removido do app** | métrica de infraestrutura fica no Grafana; o app mostra só um resumo de saúde em Configurações |
| modais do `shell` (contas, backup, diagnóstico) | Configurações | |

---

## Visão geral

**Pergunta:** está tudo bem, e o que precisa de mim agora?

**KPIs**

| KPI | Definição | Comparação |
|---|---|---|
| Patrimônio | saldo + inventário avaliado pelo valor justo da Bolsa | variação em 7 dias; meta definida pelo usuário |
| Lucro 24h | receitas − custos nas últimas 24 h (ver Finanças) | mesmo intervalo de ontem |
| Rendimento | orbs/hora da caçada atual | média da própria hunt nos últimos 7 dias |
| Automações | ativas / configuradas | indicadores por automação, com falha em destaque |

**Visualização principal:** o cartão "Escaneamento de campo" com a criatura ativa em 2.5D (saindo do card), HP, XP e a caçada atual.

**Detalhe:**

- **"Precisa de você"**: no máximo 3 cartões de veredito com o que merece ação agora (oportunidade de mercado acima do limite, hunt rendendo abaixo da média, automação parada). Substitui o "O que melhorar" do `aovivo`.
- Lucro dos últimos 7 dias (área, uma série, linha da média).
- Diário de eventos, filtrável por tipo.

**No modo "Todas as contas":** os KPIs viram a soma (patrimônio e lucro) ou a média (rendimento), e o cartão principal vira uma faixa com as contas lado a lado, cada uma com sua criatura, sua hunt e seu status.

## Jogo

**Pergunta:** como estão as contas no jogo agora, e posso intervir?

Hoje é o navegador com quatro abas. No app novo:

- **Grade 2×2** com as quatro janelas do jogo. Cada janela tem uma faixa fina sobreposta com conta, hunt, kills/h, HP e status da automação.
- **Modo foco**: um clique amplia uma conta, e as outras três viram miniaturas na lateral.
- **Controles por conta**, na faixa: pausar e retomar automação, recarregar, abrir a conta na Visão geral.
- **Indicador de desempenho**: as janelas fora de foco rodam com taxa de quadros reduzida, para quatro instâncias do jogo não disputarem a GPU.
- O coletor injetado em cada janela alimenta o servidor; um ícone mostra se a captura daquela janela está saudável.

## Caçada

**Pergunta:** onde caçar, e a caçada atual está rendendo o esperado?

### Sessão ao vivo

- **KPIs:** kills/h, orbs/h, XP/h, consumo de itens/h (poções e bolas), todos contra a média da hunt.
- **Principal:** linha do rendimento da sessão contra a faixa esperada (calculada pelo histórico da hunt), destacando quando sai da faixa.
- **Detalhe:** drops da sessão, eventos, e a comparação real × simulador (o quanto o resultado real bate com a previsão).

### Mapa de hunts

- **Principal:** barras horizontais ordenadas por rendimento líquido (orbs/h já descontando o consumo), com a hunt atual em destaque e as outras em cinza. Um seletor troca a métrica (orbs/h, XP/h, chance de raro).
- **Detalhe:** mapa de calor hunt × hora do dia, mostrando quando cada hunt rende mais; e o mapa de expedição atual.
- **Veredito:** "Trocar para Riacho Cristalino: +18% de orbs/h para o seu nível".

### Shiny

- Probabilidade real por hunt (com intervalo de confiança, não só a média), encontros recentes e melhores lugares.

## Combate

**Pergunta:** como venço este boss, e o que me falta para isso?

Quatro telas viram um fluxo:

1. **Bosses**: grade com todos os bosses. Cada cartão mostra a chance de vitória com a melhor equipe que a conta já tem, e um selo: "vence agora", "vence com compras" ou "fora de alcance".
2. **Boss** (ao escolher um):
   - **KPIs:** chance de vitória, tempo médio de luta, custo para completar a equipe, ganho esperado por vitória.
   - **Três equipes lado a lado:** a *ideal* (independe do que a conta tem), a *melhor disponível* (só com o que a conta tem) e a *do laboratório* (montada à mão).
   - **Laboratório:** arrastar e soltar criaturas nos seis slots, com a chance de vitória recalculada a cada mudança. O motor em Rust com `rayon` permite resposta em menos de um segundo.
   - **Ações:** "aplicar build", e "comprar faltantes", que abre o Mercado já filtrado nas criaturas e itens que faltam.
3. **Simulação (a parte dinâmica):**
   - **Replay animado** de uma luta simulada: as criaturas em 2.5D na arena, barras de HP, golpes, críticos e trocas acontecendo em sequência, com play, pausa, velocidade (1×, 4×, 16×) e uma linha do tempo clicável marcando os eventos decisivos.
   - **Distribuição de resultados** de mil simulações: histograma da duração da luta e a proporção de vitórias, para mostrar se a vitória é segura ou se depende de sorte.
   - **"Por que perdeu"**: nas simulações perdidas, o motivo mais comum ("a equipe cai no 3º golpe de área").

As análises da tela `equipes` (escudo, cabo de guerra, espectro) são reavaliadas uma a uma na fase A3. As que respondem "por que esta equipe vence ou perde" entram na Simulação; as outras saem.

## Mercado

**Pergunta:** o que compro, o que vendo e o que guardo?

### Pregão

- Lista de ativos à esquerda (busca, filtro por tipo e moeda), gráfico do ativo selecionado ao centro (candles, médias, bandas), e o cartão de veredito à direita (sinal, confiança, preço-alvo, valor justo, os três motivos).

### Oportunidades

- Tabela de ofertas abaixo do valor justo, ordenada por desconto × confiança, com ação de compra em um clique. Filtros de tipo, moeda e orçamento.

### Inventário

Junta `vender`, `capturas`, `raros` e "meus itens":

- Tudo o que a conta possui (itens, criaturas, raros), avaliado a valor justo.
- Para cada linha, um veredito: **vender** (e por quanto), **guardar** (e por quê), **mandar para a coleção** (quando é melhor que o exemplar atual).
- Seleção em lote para vender ou guardar várias coisas de uma vez, com prévia do total.

## Finanças

**Pergunta:** quanto estou ganhando de verdade, com o quê, e o patrimônio está crescendo?

Substitui o Financeiro atual, que só mostrava o saldo de ouro.

**Filtros:** conta, período (hoje, 7 d, 30 d, personalizado), moeda (orbs ou gold).

**KPIs**

| KPI | Definição | Comparação |
|---|---|---|
| Lucro líquido | receitas − custos no período | período anterior |
| Receitas | loot de caçada + vendas no mercado + recompensas de boss | período anterior |
| Custos | compras no mercado + consumo (poções, bolas) + taxas | período anterior |
| Patrimônio | saldo + inventário a valor justo, no fim do período | início do período |

**Principal:** **gráfico de cascata (waterfall)** do período: cada fonte de receita soma, cada tipo de custo subtrai, e a última barra é o lucro líquido. Em um gráfico você vê de onde veio e para onde foi o dinheiro.

**Detalhe:**

- **Lucro por atividade**: barras ordenadas por lucro líquido de cada hunt, boss e operação de mercado, com o ROI ao lado. Responde "o que vale meu tempo".
- **Lucro por conta** (no modo "Todas as contas"): barras ordenadas, uma por conta.
- **Patrimônio ao longo do tempo**: linha, com o saldo e o inventário como duas áreas empilhadas, para ver se o crescimento é dinheiro em caixa ou itens valorizando.
- **Calendário**: mapa de calor do lucro diário, para achar padrões de dia da semana (mantido do Financeiro atual, que já fazia isso bem).
- **Extrato**: todas as movimentações, filtráveis e exportáveis.

## Bestiário

**Pergunta:** o que esta criatura vale, onde ela rende e como eu a melhoro?

### Espécie

O "Escaneamento de campo" em tela cheia: a criatura em 2.5D, tipos, stats no nível escolhido, escada evolutiva, onde ela rende (hunts e bosses), golpes, valor de mercado e o veredito para o exemplar que a conta tem.

### Coleção

- Grade de todas as espécies, com as que a conta possui em cor e as que faltam em silhueta.
- Medidor de completude da coleção, e filtros por tipo, raridade e região.

## Automação

**Pergunta:** o que o sistema está fazendo sozinho, e está fazendo bem?

### Regras

- Lista de automações com estado, última execução, próxima execução e **impacto** (lucro atribuído a ela no período).
- Editor de regra com prévia: "se esta regra estivesse ligada nos últimos 7 dias, teria feito X".

### Decisões

- Feed das decisões da IA: o que decidiu, o motivo, e **o resultado depois** (a decisão deu certo?).
- KPI de acerto: porcentagem de decisões que melhoraram o resultado em relação à alternativa.

### Fila

- Comandos pendentes, em execução e concluídos por conta, com a possibilidade de cancelar.

## Configurações

- Contas (adicionar, remover, renomear).
- Tokens de acesso e dispositivos conectados.
- Aparência (modo desempenho, densidade).
- Backup e restauração.
- Saúde: resumo do servidor (no ar, banco acessível, versão, último deploy) e link para o Grafana. As métricas de infraestrutura não ficam no app.

---

## Componentes padronizados

Todas as telas são montadas a partir deste conjunto, que vai para o design system:

| Componente | Uso |
|---|---|
| Stat tile | KPI com valor, unidade, comparação e ajuda |
| Hero number | o KPI principal de uma tela |
| Medidor (meter) | uma razão contra uma meta ou limite |
| Barras ordenadas | comparar itens (hunts, contas, atividades) |
| Linha / área | tendência no tempo |
| Cascata | composição do lucro |
| Mapa de calor | dia × hora, hunt × hora, calendário |
| Histograma | distribuição de resultados das simulações |
| Tabela densa | listas com mais de 7 itens, sempre com ordenação e busca |
| Cartão de veredito | recomendação + motivo + confiança + ação |
| Diário de eventos | linha do tempo filtrável |
| Cena 2.5D | criatura em destaque (Visão geral, Bestiário, Combate) |

## Fases de implementação

| Fase do app | Telas |
|---|---|
| A1 | Navegação, Visão geral (sem automação), Mercado › Pregão e Oportunidades |
| A2 | Jogo, Caçada › Sessão ao vivo, Automação › Fila |
| A3 | Combate, Finanças, Mercado › Inventário, Bestiário, Caçada (restante), Automação (restante), Configurações |
