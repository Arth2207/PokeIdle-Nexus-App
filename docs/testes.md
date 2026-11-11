# Testes

## Pirâmide

| Nível | Ferramenta | O que cobre |
|---|---|---|
| Unidade | `cargo test` | Cálculos do `nucleo`: indicadores, combate, otimizador, lucro, estatísticas |
| Propriedade | `proptest` | Casos de borda gerados automaticamente |
| Integração | `testcontainers` | Repositórios e rotas contra um Postgres efêmero com TimescaleDB |
| Paridade | ferramenta própria (`tests/paridade`) | Nexus contra Node, com requisições reais |
| Carga | `k6` | Latência e throughput nas rotas pesadas |

## Unidade e propriedade

O `nucleo` não faz I/O, então os testes de unidade são rápidos e determinísticos. Exemplos de propriedades verificadas com `proptest`:

- O otimizador nunca devolve uma equipe que viola as restrições pedidas (nível, moeda, orçamento, tamanho).
- Repetir o otimizador com a mesma semente dá o mesmo resultado.
- Um duelo entre A e B com os lados trocados tem o resultado espelhado.
- Os indicadores nunca produzem `NaN` nem infinito, mesmo com séries vazias, de um ponto só ou com preços repetidos.
- A média móvel de uma série constante é a própria constante.

## Integração

Cada teste sobe um Postgres com TimescaleDB descartável, aplica todas as migrations e roda contra ele. Isso cobre:

- **Migrations:** `sqlx migrate run` funciona num banco vazio.
- **Idempotência:** ingerir o mesmo lote duas vezes deixa o banco no mesmo estado.
- **ETL:** processar o mesmo período duas vezes dá o mesmo resultado.
- **Concorrência:** dois consumidores da fila de comandos nunca pegam o mesmo comando.
- **Readiness:** com o banco parado, `/health/ready` responde 503; quando o banco volta, responde 200 de novo sem reiniciar o processo.
- **WebSocket:** conexão, push, reconexão com `retomar` e desconexão por backpressure.

## Paridade

É o teste que decide se uma rota pode ir para produção.

### Fixtures

Requisições reais capturadas do tráfego do Node em produção, sem alterar o Node. Cada fixture guarda a requisição, a resposta do Node e o estado do banco usado (um dump das tabelas envolvidas, ou um intervalo de tempo).

### Comparação normalizada

As duas respostas são normalizadas antes de comparar:

- chaves de objeto ordenadas;
- floats comparados com tolerância relativa de `1e-6`, ou outra configurada por campo;
- campos voláteis ignorados: `geradoEm`, `ts`, ids gerados e `progresso`;
- listas cuja ordem não importa são ordenadas por uma chave estável antes de comparar.

O resultado é um relatório por rota com a porcentagem de fixtures idênticas e o diff das que divergiram.

### Otimizador

O otimizador é heurístico: duas execuções corretas podem achar equipes diferentes com a mesma qualidade. Por isso a paridade dele é por **qualidade**, e não por igualdade:

- o score da equipe do Nexus tem que ser maior ou igual a `score_node × 0,99`;
- todas as restrições têm que ser respeitadas;
- em pelo menos 90% das fixtures, o custo tem que estar dentro de ±5% do custo do Node.

## Carga

Com `k6`, nas duas rotas pesadas (`/api/bolsa/equipe` e `/api/equipes/plano`), primeiro contra o Node e depois contra o Nexus, na mesma máquina e com os mesmos dados.

São medidos p50, p95, p99 e throughput máximo antes de a latência degradar. O resultado entra neste documento como a justificativa objetiva da migração.

| Rota | Node p50 | Node p95 | Nexus p50 | Nexus p95 |
|---|---|---|---|---|
| `/api/bolsa/equipe` | — | — | — | — |
| `/api/equipes/plano` | — | — | — | — |

Também é medido se as rotas leves continuam rápidas enquanto o otimizador está rodando, o que valida o isolamento entre tokio e rayon.
