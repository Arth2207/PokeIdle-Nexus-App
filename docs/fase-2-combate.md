# Fase 2: combate e otimizador de equipe

Plano completo da fase 2. Resume o que existe no sistema legado, o que o código público do jogo confirma, o que o Nexus vai fazer igual e o que vai fazer melhor.

## O que existe hoje

| Peça | Onde | O que faz |
|---|---|---|
| Fórmulas | `engine/sim.js` | stats, dano, recarga, ataque básico ("investida"), constantes do servidor |
| Motor de equipe | `equipes-core.js` | duelo de cada carta contra o boss, `equipeIdeal` (equipe teórica), `equipeColecao` (com a coleção da conta), próximo boss |
| Otimizador | `bolsa-equipe-selecao.js` | sementes + busca local por trocas + busca em feixe, perfis econômico, equilibrado e máximo, reservas e alternativas |
| Equipe da bolsa | `bolsa.js` (`equipeBolsa`) | avalia as ofertas contra o boss e roda o otimizador numa worker thread; o painel recebe `202` até sair |
| Equipe própria | `bolsa.js` (`equipePropria`) | a melhor equipe só com os pokémon da conta |
| Alvos de coleta | `bolsa-alvos.js` | espécies que valem a pena coletar e as "faltas" de cada boss |
| Lucro | `bolsa-lucro.js` | lucro esperado por boss a partir dos drops |
| Estado | `equipes-estado.json`, `.bolsa-faltas.json`, `.bolsa-equipes-cache.json`, `equipe-sugestoes.jsonl` | arquivos JSON |

Rotas: `GET /api/bolsa/equipe`, `GET /api/bolsa/equipe-propria`, `GET /api/bolsa/alvos-coleta`, `GET /api/equipes/bosses`, `GET /api/equipes/plano` e `POST /api/equipes/ingest`.

### Como o legado calcula

- Cada carta "duela" sozinha contra o boss, em passos de 0,1 s, por 40 s. Do duelo saem o dano por segundo que ela causa e o que recebe.
- O valor da carta é `D = dano por segundo × tempo até cair` (no máximo 300 s).
- A equipe é a soma dos `D`. A força da equipe é `Σ mín(1, nível / nível do boss)`, e o dano recebido é multiplicado por `3^(5 − força)`.
- O otimizador testa cerca de 38 equipes iniciais, melhora cada uma trocando uma carta por vez e completa com uma busca em feixe. Não há garantia de ótimo.

## O que a investigação confirmou

Fontes: os módulos públicos que o jogo serve em `/shared/*.mjs`, as mensagens do WebSocket gravadas pelo coletor (`welcome`, `batalha`) e o banco de eventos do legado.

**Bate com o jogo:**

- stat = `round((base + 2·IV) × nível/100 × qualidade^k × multiplicador)`, com `k` 0,95 para HP e speed e 0,8 para os demais; multiplicador = potência (1; 1,05; 1,1; 1,25; 2) × 3 se shiny × 4 se lendário;
- HP de combate = `máx(24, round(HP × 12))`;
- dano = `((2·nível/5 + 2) × poder × atk/def / 50 + 2) × STAB(1,5) × efetividade`. O legado multiplica por 0,925, a média da variação de dano do jogo (0,85 a 1,0);
- recarga do golpe pela curva de poder (10 s a 60 s), com desconto de até 7% pelo speed base e pelo IV de speed.

**Diverge ou falta no legado:**

| Ponto | Jogo | Legado |
|---|---|---|
| Golpes do boss | padrão público: *Legendary Strike* (280, 10 s) e *Legendary Burst* (400, 15 s), especiais | escolhe os dois golpes mais fortes da espécie ("a ficha real do boss não é pública") |
| Lista de bosses | 27 bosses em 9 trincas por região (Articuno, nível 300, até Fezandipiti, 179.500), confirmada pela lista viva que o relay envia e pelo módulo público | a lista viva (`equipes-estado.json`) está certa, mas o `sim.js` ainda carrega uma lista antiga (Regirock…) e o `equipes-core.js` tem a tabela da wiki como reserva |
| Constantes de hunt | `multHpSelvagem`, `multDanoSelvagem`, `ampliacaoHunt` e a tabela de tipos completa vêm no `welcome` | aplica as do `welcome` se chegarem; senão usa valores da wiki |
| Nível acima de 150 | arenas (ginásio e guerra) usam nível efetivo `150 × (n/150)^expoente` | não aplica (não afeta boss, mas afeta ginásio) |
| Duelo oficial 1×1 | 400 amostras com variação de dano e gerador determinístico; devolve chance de vitória | não existe |

**O que continua desconhecido:**

- HP, stats e penalidade real do boss: a luta de boss é calculada no servidor. A penalidade `3^déficit` vem da wiki e não aparece no código público.
- Não há nenhuma luta de boss gravada: 0 vitórias registradas e nenhuma mensagem `batalha` de boss entre as cerca de 108 mil capturadas. Há 2.460 ataques de hunt amostrados (dano, efetividade, STAB, HP do alvo), mas sem a ficha do atacante.

Conclusão: o modelo do legado acerta as cartas e erra no boss. O maior ganho de qualidade não está no otimizador, e sim em modelar o boss com o que é público e calibrar o resto com lutas reais.

## Escopo

1. **Paridade.** Portar para o `nucleo`, sem I/O, `sim.js`, `equipes-core.js`, `bolsa-equipe-selecao.js`, `bolsa-alvos.js`, `bolsa-lucro.js` e o cálculo de `equipeBolsa`/`equipePropria`. Rotas com o mesmo contrato JSON.
2. **Modelo fiel ao jogo**, ao lado do modelo legado (ver abaixo).
3. **Melhorias** de desempenho, qualidade da busca e funcionalidade.
4. **Estado no Postgres** no lugar dos arquivos JSON.

## O que o Nexus faz melhor

### 1. Dois modelos de combate, lado a lado

Um trait `ModeloCombate` com duas implementações:

- **`Legado`**: cópia exata do `sim.js` + `equipes-core.js`, inclusive o passo de 0,1 s e o acúmulo de float. É o que a comparação com o Node usa, e o que responde até o cutover.
- **`Jogo`**: as mesmas fórmulas das cartas, com o boss modelado pelo que é público (golpes padrão, lista e níveis do `welcome`, tabela de tipos e constantes do servidor) e os pontos desconhecidos (HP e stats do boss, penalidade) como parâmetros calibráveis.

As duas rodam juntas: a resposta mostra a do modelo ativo, e as métricas registram quanto elas divergem. A troca de modelo é uma configuração.

### 2. Calibração com lutas reais

- Gravar cada tentativa de boss: equipe (fichas completas), níveis, boss, resultado, duração e os eventos `batalha` (dano por golpe, HP do alvo). A tabela `fato_boss_tentativa` já existe e está vazia; a captura entra pela ingestão da fase 3, mas a tabela e o cálculo nascem aqui.
- Com algumas dezenas de lutas, ajustar os parâmetros do boss (HP, multiplicador de dano, penalidade) por mínimos quadrados sobre o dano observado e o tempo de luta.
- Validar a fórmula de dano das cartas com os ataques de hunt, assim que a amostra passar a guardar a ficha do atacante.
- Trocar o "score" por **chance de vitória**, com a variação de dano do jogo (0,85 a 1,0) e um gerador determinístico, como no duelo oficial: mesma entrada, mesma resposta.

### 3. Equipe na hora

- O duelo em Rust custa microssegundos. A matriz **oferta × boss** (o valor de cada carta à venda contra cada boss) é calculada em segundo plano, junto com o snapshot da bolsa.
- `GET /api/bolsa/equipe` passa a responder direto na maior parte das vezes. O `202` com progresso continua no contrato, para quando o cálculo ainda está rodando.
- O otimizador roda num pool `rayon` de tamanho fixo (`NEXUS_CPU_THREADS`), fora das threads do tokio; a API segue respondendo em menos de 50 ms.
- Um pedido igual a outro em andamento espera o mesmo resultado; um pedido que deixa de interessar (o painel trocou de boss) é cancelado.

### 4. Otimizador melhor e auditável

- As sementes e as buscas locais rodam em paralelo, e a busca em feixe pode ser mais larga no mesmo tempo.
- **Busca exata** por branch-and-bound. Com o multiplicador de força fixo, o dano da equipe é a soma dos danos das cartas, então dá para podar com limites superiores. Para cada nível de força possível, o problema vira uma mochila com no máximo 2 cartas por espécie.
- A resposta informa a **distância até o ótimo** ("a 2% do melhor possível") quando a busca exata termina no tempo limite, e a heurística vira só o ponto de partida.
- Resultado determinístico: mesma entrada, mesma equipe, em qualquer máquina.

### 5. Funcionalidades novas

- **Equipe híbrida:** usa os pokémon da conta e compra só as vagas que faltam, pelo menor custo. Hoje existe "comprar tudo" ou "só o que tenho".
- **Orçamento:** `orcamento=<gemas>` devolve a melhor equipe até esse valor.
- **Curva custo × poder da equipe:** a fronteira de Pareto de equipes completas, e não só de cartas soltas, para ver quanto poder cada gema a mais compra.
- **Plano de compra robusto:** ordem de compra e substituto de cada vaga se a oferta for vendida antes, mantendo a equipe acima do alvo.
- **Simulador no app:** o motor fica no `nucleo` sem I/O e compila para WebAssembly. O app desktop (fases A1/A3) roda o mesmo cálculo localmente, sem rede.

### 6. Estado no Postgres

| Arquivo do legado | Tabela |
|---|---|
| `equipes-estado.json` (bosses vivos, nível do treinador, coleção por conta) | `nexus.estado_equipes`, `nexus.colecao_pokemon` |
| `.bolsa-faltas.json` (compartilhado entre processos) | `nexus.falta_coleta`, com escrita atômica |
| `.bolsa-equipes-cache.json` | desnecessário: o resultado é recalculado em segundo plano |
| `equipe-sugestoes.jsonl` | `nexus.equipe_sugestao` (base da calibração e do scoring da fase 6) |
| `POKEIDLE-wiki.txt` (bosses da wiki) | `nexus.boss`, alimentada pela lista viva do jogo (relay); a wiki e o módulo público ficam como reserva |

## Testes

- **Paridade de fórmulas:** fixtures geradas pelo código JS do legado, só lendo (como o precificador na fase 1): stats, dano, duelo, `equipeIdeal`. O modelo `Legado` tem que bater exatamente.
- **Paridade do otimizador por qualidade:** o otimizador é heurístico, então a equipe pode ser outra. O dano da equipe do Nexus tem que ser ≥ 99% do Node, todas as restrições respeitadas, e o custo dentro de ±5% em 90% dos casos.
- **Propriedades (proptest):** nunca devolve equipe inválida (5 cartas distintas, no máximo 2 por espécie, teto de preço); subir nível, IV ou qualidade nunca diminui o dano; acrescentar uma carta melhor nunca piora a equipe; mesma entrada, mesma saída; a busca exata nunca perde para a heurística.
- **Desempenho:** benchmarks com `criterion` para o duelo, a matriz e o otimizador; teste de carga `k6` com p95 menor que o do Node e a API abaixo de 50 ms durante uma otimização.
- **Comparador:** o mesmo da fase 1, estendido para `equipe`, `equipe-propria`, `alvos-coleta` e `plano`, com a comparação por qualidade.

## Etapas

1. **Fórmulas e duelo** (`Legado`) com fixtures do JS. *Feito:* módulo `nucleo::combate` (tipos, espécies, combatente, duelo, trait `ModeloCombate` e o modelo `Legado`), idêntico bit a bit às fixtures.
2. **`equipeIdeal`, `equipeColecao` e `/api/equipes/*`.**
3. **Matriz oferta × boss e otimizador portado**, com paridade de qualidade; `/api/bolsa/equipe` e `equipe-propria`.
4. **`alvos-coleta`, lucro e faltas** no Postgres; o `/dicas` volta a aquecer o cálculo de equipe.
5. **Modelo `Jogo`** e a tabela de tentativas, com a calibração pronta para receber dados.
6. **Busca exata, equipe híbrida, orçamento e curva custo × poder.**
7. **Benchmarks, carga e comparador estendido.**

**Pronto quando:** dano das equipes dentro da tolerância do Node em todas as comparações; p95 menor que o do Node; API abaixo de 50 ms com o otimizador rodando; escrita de estado só atrás da flag até o cutover.

## Decisões em aberto

- **Quando trocar de modelo.** O modelo `Jogo` só substitui o `Legado` com lutas reais suficientes para calibrar. Até lá, ele roda em paralelo e só aparece nas métricas.
- **Ginásio e guerra.** O código público dá as regras de ginásio (nível efetivo, buffs do pódio, time de 5 do mesmo tipo). Não fazem parte do legado; podem virar uma rota nova depois da fase 2.
