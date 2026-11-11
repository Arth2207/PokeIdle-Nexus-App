# Glossário

| Termo | Significado |
|---|---|
| **Bolsa** | Módulo que trata o mercado do jogo como uma bolsa de valores: cada pokémon ou item, em cada moeda, é um ticker com histórico de preço |
| **Ticker** | Um ativo negociável: uma espécie ou item numa moeda. Formato do id: `p:<especie>:...:<moeda>` ou `i:<item>:<moeda>` |
| **Orb / Gold** | As duas moedas do mercado do jogo |
| **Oferta** | Um anúncio ativo no mercado (preço pedido) |
| **Venda** | Uma transação concluída no mercado (preço pago) |
| **Sinal** | Classificação de um ticker: `COMPRAR`, `VENDER`, `OBSERVAR`, `MANTER`, `SEM_BASE` |
| **Valor justo** | Preço de referência de um ticker, estimado pelas vendas recentes |
| **Ficha** | Atributos de um pokémon específico (nível, IVs, golpes), que mudam o valor dele |
| **Boss** | Inimigo de evento contra o qual se monta uma equipe |
| **Motor de combate** | Simulador de duelos por ticks, usado para estimar quem vence |
| **Otimizador** | Busca a melhor equipe dentro de restrições de orçamento e nível |
| **Beam search** | Busca que mantém só as N melhores soluções parciais a cada passo |
| **Hunt** | Área de caça onde a conta fica coletando kills, XP e drops |
| **Scoring** | Pontuação que decide qual hunt é melhor para cada conta naquele momento |
| **Relay** | Programa no PC que liga o jogo ao servidor: envia dados capturados e executa comandos |
| **Coletor** | Script injetado na página do jogo que captura as mensagens do WebSocket do jogo |
| **Fila de comandos** | Tabela onde o servidor deixa ações para o relay executar no jogo |
| **Dossiê** | Site com informações agregadas das espécies e do mercado |
| **Raros / Shiny** | Pokémon de aparição rara e as variantes de cor, acompanhados à parte |
| **Shadow** | Rodar o sistema novo recebendo uma cópia do tráfego real, sem que a resposta dele chegue ao cliente |
| **Cutover** | Momento em que o tráfego de uma rota passa do sistema antigo para o novo |
| **Paridade** | O sistema novo dá a mesma resposta que o antigo para as mesmas entradas |
