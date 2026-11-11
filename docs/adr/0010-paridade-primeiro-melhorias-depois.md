# 0010. Paridade primeiro, melhorias atrás de uma flag

- Status: aceito
- Data: 2026-11-11

## Contexto

A fase 2 portou o combate e o otimizador com paridade exata: o Nexus reproduz o legado número a número, inclusive os defeitos, porque é isso que permite comparar os dois em produção. Durante o porte apareceram problemas reais no legado:

- **Tabela de tipos errada.** O servidor manda no `welcome` uma tabela sem resistência ao próprio tipo (Fogo × Fogo = 1, Dragão × Dragão = 1, mais nove pares). O legado usa a tabela clássica (0,5 ou 2) e erra o dano sempre que carta e boss dividem tipo.
- **Boss inventado.** O legado escolhe os golpes do boss pela espécie; o jogo usa dois golpes padrão públicos. A penalidade `3^déficit` vem da wiki. Não há nenhuma luta de boss gravada para conferir.
- **Otimizador heurístico e dependente do relógio.** A busca para depois de 3 s, então o resultado muda de máquina para máquina. A busca exata em Rust resolve o mesmo problema em microssegundos, com garantia de ótimo.
- **Preços típicos frágeis.** A consulta converte o nível com `::int`: um nível decimal derruba a consulta inteira e a equipe sai sem preços, sem aviso. Ela também varre todo o histórico a cada 30 minutos.
- **Duas pokédex.** O motor de combate usa uma (1.279 espécies) e o precificador outra (1.415). `nexus-api pokedex-divergencias` mostra 136 espécies só no precificador (as formas `13xxx`, que o legado contorna com `speciesId − 13000`), nomes de forma diferentes e um tipo divergente (Mega Aggron).
- **Coleção de qualquer conta.** Qualquer token de leitura lê a coleção de qualquer conta pelo parâmetro `nick`.

## Decisão

1. **Paridade é o padrão até o cutover de cada fase.** O comparador só funciona se o Nexus responder igual ao legado.
2. **O que diverge de propósito fica atrás de `NEXUS_MELHORIAS`** (desligada): seleção exata nos três perfis de equipe, chance de vitória com a variação de dano do jogo, ordem de compra, preços típicos corrigidos (nível numérico, janela de 60 dias) e a tabela de tipos do servidor. Liga no cutover da fase 2, junto com o painel.
3. **O modelo do jogo roda ao lado, só nas métricas** (`NEXUS_MODELO=legado`), até haver tentativas reais suficientes para calibrar. A rota `POST /api/boss/tentativa` e a tabela `nexus.boss_tentativa` já existem; a captura vem do relay na fase 3.
4. **Correções sem efeito na paridade entram direto:** token preso a uma conta (`nexus.api_token.conta`) e o diagnóstico das pokédex.

## Consequências

- Ligar `NEXUS_MELHORIAS` faz o comparador acusar diferenças nas rotas de equipe. É esperado: depois do cutover, o comparador dessas rotas deixa de fazer sentido.
- Ficam para o cutover, porque dependem de desligar o legado:
  - **Pokédex única:** o motor passa a ler o catálogo completo (com as formas `13xxx`), e a importação do `pokedex.json` some.
  - **Fim da ponte de arquivos:** o relay grava direto no Nexus; o sincronizador (`sincronizar-catalogos.sh`), o `node -e` do catálogo de espécies, o parse da wiki em markdown e a leitura do `mapa.db` deixam de existir. Os catálogos passam a vir do `welcome` e dos módulos públicos do jogo.
  - **Faltas de coleta calculadas na hora:** em vez de pedir +5 espécies a cada 10 minutos até fechar uma equipe, o cálculo diz de uma vez quantas faltam.
  - **Contrato v2:** as rotas de equipe respondem hoje com `serde_json::Value` no formato do legado (campos duplicados, números sem arredondar). Para o app desktop, um contrato tipado e versionado no crate `contrato`, com OpenAPI.
- Pergunta em aberto: o `welcome` traz `ampliacaoHunt = 1.5`, um número só, enquanto o legado usa uma tabela por efetividade (2 → 2,5; 4 → 5,5). O significado não está claro, e o legado aplica a ampliação de hunt também nas lutas de boss. Fica como está até as lutas reais permitirem testar.
