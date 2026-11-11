# 0001. Reescrever todo o backend em Rust

- Status: aceito
- Data: 2026-11-11

## Contexto

O backend atual é Node.js, dividido em vários processos. Dois trechos são pesados em CPU:

- o motor de combate, que simula duelos de 400 ticks contra mais de mil espécies;
- o otimizador de equipe (beam search + busca local), que já encosta no limite de 3 s.

Os dois rodam num único worker thread, em série.

A análise do código mostrou que não há dependência numérica ou científica: toda a matemática é aritmética escrita à mão. Os dados são pequenos (1279 espécies, séries de cerca de 30 pontos, pools de dezenas a centenas de candidatos). Bibliotecas como numpy, BLAS ou um solver externo só adicionariam overhead.

Consideramos três caminhos: manter Node e otimizar só os gargalos, reescrever só os gargalos em Rust e chamá-los do Node (via napi), ou reescrever tudo.

## Decisão

Reescrever todo o backend em Rust, sem nenhum processo Node no servidor.

A única exceção é o script injetado na página do jogo para capturar o WebSocket dele. Esse código roda no navegador, e por isso continua em JavaScript.

## Consequências

- Ganho esperado de 10x a 50x nos cálculos, mais o paralelismo com `rayon` que hoje não existe.
- Uma linguagem só no servidor, sem fronteira FFI para manter.
- Tipagem forte e erros explícitos reduzem uma classe inteira de falhas que hoje só aparecem em produção.
- Custo: curva de aprendizado de Rust e um prazo de meses. Isso é mitigado com a migração por fases e com o Node continuando como rede de segurança até o fim.
- Se um dia for preciso ótimo garantido na montagem de equipe, o caminho é modelar como programação inteira (ILP/CP-SAT). Também dá para fazer isso em Rust (`good_lp`), mas está fora do escopo atual.
