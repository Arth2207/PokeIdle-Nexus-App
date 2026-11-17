# Caçada calibrada e comparação de XP

O dossiê, o aplicativo atualizado e o painel consultam a mesma previsão de `nucleo::motor::regras::hunt::previsao`. O motor automático usa o mesmo combate e a mesma calibração. Não são usados XP fixo ×2,64, experiência antiga da Pokédex ou aumento artificial de nível para prever a hunt atual.

A simulação avança os relógios das habilidades durante o intervalo sem alvo. O intervalo é ajustado ao ritmo observado da própria conta e inclui caminhada, captura e espera por spawn; os dados disponíveis não permitem separar causalmente essas três partes. As condições de movimento, Pokémon, equipamento, dia e bônus separam as amostras. O XP base segue `shared/recompensa-hunt.mjs` / `shared/sell-value.mjs` do jogo. O tipo sazonal e as constantes de combate vêm do contexto da conta. Selvagens usam qualidade 1 e IV médio 16,5; a variação real continua sendo uma fonte de incerteza.

As recompensas por abate são normalizadas ao bônus atual para comparar hunts. O XP efetivamente observado é mantido em campos separados. O cache de sinais do jogo não retrocede com lotes atrasados. Clientes antigos têm os bônus efetivos recuperados pela recompensa do último abate recente; uma mudança na recompensa corta a amostra anterior. O aplicativo atualizado também encaminha atributos, refino, itens, tipo de TM e bônus completos. O contexto não muda só por HP, XP ou um nível a mais.

O dossiê lê as colunas pelo cabeçalho, incluindo a coluna opcional de refino, e preserva separadores numéricos de fichas PT/EN. A conta selecionada fornece bônus e calibração de locomoção; uma ficha isolada não contém esses dados. As rotas privadas do dossiê são GET `/api/dossie/cacada-contas` e POST `/api/dossie/cacada-previsao`. A leitura autenticada do painel é GET `/api/cacada/previsao?conta=...`.

A tela nova “Melhor agora” mostra XP de treinador e Pokémon por hora, fonte da previsão e tempo entre alvos. As médias históricas de todas as contas deixam de ser apresentadas como uma recomendação para o Pokémon atual. A projeção até 3000 também conta os períodos sem abates em vez de remover intervalos longos como se nunca tivessem acontecido.

No objetivo XP, o motor avalia o catálogo vivo liberado, testa uma alternativa promissora quando a exploração está ligada e confirma o resultado com abates reais. Preserva limites de risco, retorno quando o teste piora e verificações posteriores. O papel Farm passa a priorizar XP; objetivos manuais continuam tendo prioridade. Não há garantia de máximo global: hunts nunca medidas têm estimativas e precisam de teste real. O ajuste do tempo entre alvos na mesma hunt é uma calibração, não uma validação independente de outras hunts.

Fontes públicas consultadas em 17/11/2026: https://pokeidle.io/shared/recompensa-hunt.mjs , https://pokeidle.io/shared/dia-sazonal.mjs , https://pokeidle.io/ficha-gpt.mjs . A recompensa e os atributos também foram conferidos nos eventos recebidos das contas.

Validações: testes de combate/refino, XP separado, expiração de bônus, caminhada e seleção; PostgreSQL descartável para cache e controle; navegador para o fluxo do dossiê e do painel; replay de leitura das quatro contas e leitura autenticada com o banco real. Artefatos operacionais ficam em `/root/nexus-web-controle-20261117`.

Limite de execução: o cliente antigo em modo Sombra não pode ser ativado remotamente. As previsões e o motor no servidor podem ser atualizados sem instalar nada; a execução remota por conta requer a atualização única do cliente com suporte a `relay.modo`.
