# Sistema legado

Retrato do sistema em Node.js antes da migração, levantado em novembro de 2026. Serve de referência para a paridade e para não esquecer nenhuma responsabilidade no caminho.

## Processos

| Processo | Gerenciado por | Função |
|---|---|---|
| `mapper` | pm2 | API principal: bolsa, equipes, raros, fila de comandos, ingestão e controle do coletor |
| `holodex-nexus` | pm2 | Motor de automação das contas e painel web. Repassa as rotas analíticas para o `mapper` |
| `scoring` | pm2 | Decide a recomendação de hunt a partir de uma view SQL. Também grava decisões e parâmetros |
| `etl-continuo` | pm2 | Normaliza eventos de kill e sessão em tabelas fato |
| `etl-eficiencia` | pm2 (parado) | Recalcula a eficiência por hunt |
| `ai-observer` | pm2 (parado) | Grava observações da IA |
| `dossie` | systemd | Site do dossiê; grava as vendas do mercado |
| crons | crontab | Status do pm2 a cada minuto, limpeza diária |

O `holodex-nexus` foi pensado para controlar navegadores com Playwright, mas em produção roda com os navegadores desligados (`HDX_SEM_BROWSER=1`). A automação acontece no PC, e o servidor só guarda o estado e decide.

## Fluxo de dados

1. O app do PC abre o jogo e injeta um script que escuta o WebSocket do jogo.
2. O relay do PC envia o que foi capturado ao servidor: ofertas, vendas, kills, raros, equipes.
3. O `mapper` grava os dados brutos no SQLite (captura) e no Postgres (mercado).
4. Os ETLs transformam os eventos em tabelas fato no Postgres.
5. Bolsa, scoring e otimizador leem essas tabelas e produzem recomendações.
6. O relay consulta a fila de comandos periodicamente e executa no jogo o que encontra lá.

## Gargalos conhecidos

- **Motor de combate** (`equipes-core.js`, `sim.js`): simula duelos de 400 ticks sobre mais de mil espécies. Cache LRU de 400 entradas.
- **Otimizador de equipe** (`bolsa-equipe-selecao.js`): beam search + busca local, limitado a 3 s, rodando num único worker thread. É heurístico: não garante o ótimo global.

## Problemas encontrados

Itens que a reescrita corrige:

- Não há log de requisições. Medir a carga exigiu capturar pacotes.
- Credenciais do banco escritas no código-fonte de alguns scripts.
- A API principal escuta em todas as interfaces e confia em IP de origem como autenticação.
- Há um servidor de arquivos estáticos exposto publicamente.
- O estado fica dividido entre Postgres, um SQLite de cerca de 600 MB e vários arquivos JSON.
- Vários processos escrevem na mesma tabela de comandos sem uma coordenação explícita.
- Código morto e cópias de backup misturados com o código ativo.

## Carga observada

Numa amostra de 60 segundos, a API principal recebeu 6 conexões, vindas do PC e de processos locais. Ou seja, cerca de 0,1 req/s e um único cliente externo. A reescrita não tem a ver com volume, e sim com latência dos cálculos pesados, confiabilidade e manutenção.
