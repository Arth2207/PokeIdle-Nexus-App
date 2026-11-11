# Testes

## Pirâmide

| Nível | Ferramenta | O que cobre |
|---|---|---|
| Unidade | `cargo test` | Cálculos do `nucleo`: indicadores, combate, otimizador, lucro, estatísticas |
| Propriedade | `proptest` | Casos de borda gerados automaticamente |
| Integração | `testcontainers` | Repositórios e rotas contra um Postgres efêmero com TimescaleDB |
| Paridade | comparador no `nexus-api` (`nexus-api paridade`) | Nexus contra Node, nas mesmas consultas e no mesmo instante |
| Carga | `k6` | Latência e throughput nas rotas pesadas |

## Unidade e propriedade

O `nucleo` não faz I/O, então os testes de unidade são rápidos e determinísticos. Exemplos de propriedades verificadas com `proptest`:

- O otimizador nunca devolve uma equipe que viola as restrições pedidas (nível, moeda, orçamento, tamanho).
- Repetir o otimizador com a mesma semente dá o mesmo resultado.
- Um duelo entre A e B com os lados trocados tem o resultado espelhado.
- Os indicadores nunca produzem `NaN` nem infinito, mesmo com séries vazias, de um ponto só ou com preços repetidos.
- A média móvel de uma série constante é a própria constante.

## Integração

Os testes sobem um Postgres com TimescaleDB descartável (`timescale/timescaledb-ha:pg16`, com testcontainers), aplicam a baseline das tabelas do legado e as migrations do Nexus, e rodam contra ele. Precisam de Docker.

**Baseline do legado.** As tabelas do schema `public` que o Nexus lê pertencem ao Node e já existem em produção. Para os testes, a estrutura delas fica em `migrations-teste/` (gerada com `pg_dump --schema-only`). Essa pasta é aplicada como SQL simples, só pelos testes. O binário embute apenas `migrations/` (`sqlx::migrate!`), então a baseline nunca roda em produção.

Hoje os testes de integração cobrem a bolsa (`crates/persistencia/tests/integracao.rs`): ingestão idempotente, coleta completa restrita ao próprio escopo, resumo de itens sem duplicar, limpeza de ofertas vencidas, inventário e a leitura completa até um ticker montado. A lista abaixo é o objetivo para as próximas fases:

- **Migrations:** `sqlx migrate run` funciona num banco vazio.
- **Idempotência:** ingerir o mesmo lote duas vezes deixa o banco no mesmo estado.
- **ETL:** processar o mesmo período duas vezes dá o mesmo resultado.
- **Concorrência:** dois consumidores da fila de comandos nunca pegam o mesmo comando.
- **Readiness:** com o banco parado, `/health/ready` responde 503; quando o banco volta, responde 200 de novo sem reiniciar o processo.
- **WebSocket:** conexão, push, reconexão com `retomar` e desconexão por backpressure.

## Paridade

É o teste que decide se uma rota pode ir para produção.

### Como as respostas são obtidas

O comparador chama o Node com GETs comuns e monta a resposta do Nexus para os mesmos parâmetros, no mesmo instante em que o Node montou a dele. O motivo de não usar tráfego capturado está em [migração](migracao.md#como-as-respostas-são-comparadas).

Para partes que não aparecem nas rotas o tempo todo, há conferências dedicadas. O precificador, por exemplo, foi validado avaliando as mesmas 200 fichas reais nos dois lados (`nexus-api avaliar-fichas`).

### Comparação normalizada

As duas respostas são normalizadas antes de comparar:

- chaves de objeto ordenadas;
- floats comparados com tolerância relativa de `1e-6`, ou outra configurada por campo (`spd` usa `1e-4`, porque depende do instante exato em que o Node analisou);
- campos ausentes e `null` são equivalentes (o `JSON.stringify` omite `undefined`);
- campos voláteis ignorados: `geradoEm`, `ultimaCaptura`;
- listas cuja ordem não importa são ordenadas por uma chave estável antes de comparar.

O resultado é um relatório por rota com a porcentagem de fixtures idênticas e o diff das que divergiram.

### Fórmulas de combate

O modelo de combate `Legado` é comparado com fixtures geradas pelo próprio código do legado: `ferramentas/fixtures/combate-legado.cjs` carrega o motor JS (só leitura) e grava em `crates/nucleo/tests/fixtures/combate-legado.json` a efetividade de tipos, os atributos de combatentes sorteados, o dano de cada golpe, os bosses, fichas reais e sintéticas, duelos contra os bosses e a penalidade de equipe. Os floats têm que sair **idênticos bit a bit**.

Para isso o `nucleo` usa `js::pow`, um porte do `Math.pow` do V8 do Node 20 (o `pow` do fdlibm com os ajustes do V8). O `powf` da libc e o crate `libm` erram o último bit em cerca de 1 caso em 10, e o resultado ainda mudaria entre a glibc da VPS e a musl da imagem. O gerador também grava 3.000 potências para conferir esse porte. Pelo mesmo motivo, `Math.log` e `Math.exp` têm portes em `js::ln` e `js::exp` (o `ln` da libc difere do V8 em cerca de 8% dos casos), conferidos com 20.000 valores de `ferramentas/fixtures/js-math.cjs`. Toda fórmula copiada do legado usa as versões do `js`, inclusive o precificador e as estatísticas da bolsa.

Para regerar: `node ferramentas/fixtures/combate-legado.cjs <raiz do legado> > crates/nucleo/tests/fixtures/combate-legado.json`. O gerador usa uma semente fixa, então a saída só muda se o legado mudar.

As rotas de equipes seguem a mesma ideia: `ferramentas/fixtures/equipes-legado.cjs` chama em memória as próprias rotas `/api/equipes/bosses` e `/plano` do legado (com os nicks trocados) para a lista viva de bosses e para a tabela da wiki, e o teste exige o mesmo JSON, número a número. Cada cenário roda num processo separado, porque o cache do `equipeIdeal` do legado não leva a fonte da lista na chave.

O otimizador também tem fixtures do próprio legado (`ferramentas/fixtures/otimizador-legado.cjs`): pools de cartas avaliadas contra bosses reais, passados ao `selecionar` do JS. Com a mesma entrada, a porta em Rust escolhe as mesmas equipes, reservas e planos B e C, com os mesmos números. As peças puras da equipe da bolsa (wiki de bosses, listas por perfil, lucro e alvos de coleta) seguem o mesmo esquema (`bolsa-equipe-legado.cjs`).

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
