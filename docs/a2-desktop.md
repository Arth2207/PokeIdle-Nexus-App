# App desktop A2: desenho

Substitui o relay Node `holodex-relay.js` (ADR 0009). Parte a: núcleo e casca mínima.

## Peças

- `crates/a2-nucleo` (workspace principal, sem Tauri, testado na CI Linux):
  - `cliente`: WS `nexus.v1` com `Authorization: Bearer`; envia `retomar {desde}` ao conectar; reconecta com `backoff` (1 s a 30 s, jitter de ±20%). O estado (último `seq`, lote em voo, comandos concluídos) sobrevive à reconexão.
  - `sessao`: guarda o último `seq`; mensagem repetida ou antiga não é reprocessada nem reconfirmada com `ack`.
  - `eventos`: fila com dedup local; o lote em voo é reenviado idêntico (mesmo `seq_cliente`) até `evento.ack` com `ok`, sem duplicar (o servidor também deduplica por conteúdo). Usa `evento.lote` do `/ws`, que grava pela mesma função do `POST /ingest`.
  - `comandos`: trait `Executor` (o webview plugará). O `Despachante` executa cada id uma vez, devolve o mesmo resultado em reentrega (lease de 120 s, pelo menos uma vez), serializa por conta, respeita `expiraEm` e `comando.cancelado`, ignora contas fora do escopo. Resultado vai em `comando.resultado`. Modo `Sombra` confirma como `sombra` sem executar.
  - `token`: trait `ArmazemToken`; `ArquivoToken` grava em `<config do usuário>/nexus/token` com modo 0600 (Unix). Trocar por keyring depois.
- `apps/desktop`: casca Tauri 2, **fora** do workspace principal (`exclude` no `Cargo.toml` raiz), com workspace e `Cargo.lock` próprios. O lock não é gerado na VPS; o CI o gera. Comando `iniciar_nucleo` liga o núcleo em modo sombra.
- CI: job `desktop` em `windows-latest` gera ícones a partir de `app-icon.png`, roda `cargo tauri build` (NSIS e MSI) e publica o artefato `nexus-desktop-windows`.

## Parte b: janelas do jogo (`apps/desktop`, `crates/a2-nucleo::jogo`)

- **Uma janela por conta** (`jogo-<nick>`, rótulo reversível em `rotulo_da_conta`/`conta_do_rotulo`), com perfil próprio (`data_directory` em `<dados locais>/perfis/<rótulo>`): cookies e armazenamento separados. Cada perfil é um ambiente WebView2 à parte (mais RAM que janelas no mesmo perfil; é o preço do isolamento). Navegação restrita a `pokeidle.io`.
- **Coletor** em `apps/desktop/coletor/coletor.js` (JS de página, sem npm nem bundler, único JS do projeto, ADR 0001), via `include_str!` como initialization script (roda antes do JS do jogo). Faz: hook no `WebSocket` do jogo (guarda a conexão e repassa `welcome` e `batalha`), observador do `#hud-log` (linhas `[seq, ts, texto]`, enviadas a cada 2 s) e `window.__nexus.executar(id, obj)` (manda `obj` como JSON no WS do jogo, igual ao `__hdxCmd` legado, e devolve o resultado por IPC). Não faz requisição nenhuma.
- **Ponte coletor -> Rust:** IPC `coletor_evento`; a conta vem do rótulo da janela, nunca da página. `jogo::eventos_do_coletor` converte em eventos do `/ingest`: `welcome`, `kill` (de `batalha` e do HUD, com `seq`), `drop`, `capturado`, `bola`, `mercado_aviso`, `inventario`, `perfil`. Cada evento leva `_conta` e a fila (`FilaEventos`) agrupa o lote por conta (o campo sai antes de enviar). Um único `/ws` serve todas as contas.
- **Executor real** (`jogo::ExecutorJanelas` + `Avaliador` da casca): `eval` de `window.__nexus.executar(id, passo)` na janela da conta; o coletor responde por `coletor_resultado` (`Correio`, prazo de 15 s, resultado cortado em 200). Passos entre si com 150 ms. Mapeamento (`jogo::passos`): `motor.<t>` -> `{t, ...params}`; `hunt {slug}` -> `hunt.select`; `team.add`; `team.set {pokemonIds}` -> `team.active` + `team.move` por slot. `config` e `reload` (eram do motor local) e tipos desconhecidos voltam como erro.
- **Modo Eco:** no Windows, `additional_browser_args` do WebView2 com `--disable-background-timer-throttling --disable-renderer-backgrounding --disable-backgrounding-occluded-windows` (mais os argumentos padrão do Tauri, que o campo substitui). Em outras plataformas não há equivalente (o app é Windows). Vale por perfil: todas as janelas usam os mesmos argumentos.
- **Tracker** (`abrir_tracker`, rótulo `tracker`, script `coletor/tracker.js`): abre só `https://pokeidle.io/tracker`; o usuário navega à mão. O botão "Guardar esta página" (só clique com `isTrusted`) captura `innerText`, títulos e tabelas do que está renderizado e chama `tracker_captura`; um clique, uma captura. O app nunca abre, pagina, atualiza nem busca URL do tracker e nunca chama `/tracker/api/`. O Rust revalida (`url_tracker_valida`: só `/tracker...`, nunca `/tracker/api`, texto até 200 mil caracteres) e emite o evento `tracker.captura` (`k`, `ts`, `url`, `titulo`, `texto`, `estrutura`), assinado pela primeira conta da lista. **Pendência de servidor:** `persistencia::eventos::TIPOS` ainda não aceita `tracker.captura` (conta como erro até incluir; aditivo).
- **Segurança do IPC:** os comandos do app ficam atrás de permissões (`build.rs` com `AppManifest`); `capabilities/jogo.json` dá às páginas remotas (`https://pokeidle.io/*`, janelas `jogo-*` e `tracker`) só `coletor_evento`, `coletor_resultado` e `tracker_captura`, nenhum `core:*`.

### Metas e como medir (Windows)

Meta com 4 contas abertas em jogo: **até 1,4 GB de RAM e 25% de CPU**. Não dá para medir na VPS. Medir no Windows, com o app em modo ativo e as 4 janelas rodando 30 min (uma delas minimizada, outra coberta):

1. RAM: Gerenciador de Tarefas, aba Detalhes, somar `nexus-desktop.exe` e todos os `msedgewebview2.exe` filhos do app (agrupar por árvore de processos; ou `Get-Process msedgewebview2,nexus-desktop | Measure-Object WorkingSet64 -Sum`, ou melhor, `PrivateMemorySize64`).
2. CPU: `typeperf "\Process(msedgewebview2*)\% Processor Time" "\Process(nexus-desktop)\% Processor Time" -si 5 -sc 360` e dividir pela quantidade de núcleos (a meta é o total do app sobre a máquina). Alternativa: Monitor de Desempenho.
3. Conferir o Eco: com a janela minimizada o contador de kills no HUD deve continuar no mesmo ritmo (sem throttling).
4. Se estourar a RAM: reduzir o tamanho da janela, fechar a janela do Tracker e comparar com um perfil compartilhado (um ambiente WebView2 só) para ver quanto custa o isolamento.

## Pendente

- **Visual (pedido do dono, futuro):** cards de vidro mais transparentes, deixando a paisagem aparecer através deles, como no mockup.
- Tela: fundo pelo bioma da hunt da conta ativa e tema noturno; seletores de período e moeda, ⌘K, alertas e Modo desempenho (hoje só visuais); limite de 15 fps nas janelas fora de foco (hoje só texto); demais telas da navegação (etapa 10).
- Coletor: `hunt_slug` e HP mínimo por conta (cards mostram "—"); `inventario`, `mercado` e `perfil` (o parser em `jogo` já aceita; falta ler do DOM/estado); expor informativos (`raro.novo` etc.) e `sessao.ressincronizar` à UI.
- Infra do app: keyring no lugar do arquivo de token; commitar o `Cargo.lock` do desktop e da UI e usar `--locked`; ícone definitivo.

## Tela (Leptos)

A tela do app é um crate Rust próprio, `apps/desktop/ui` (Leptos CSR, alvo `wasm32-unknown-unknown`), buildado com [trunk](https://trunkrs.dev) 0.22.1. Não há JavaScript escrito à mão; a ponte com o Tauri é feita por `wasm_bindgen` (`window.__TAURI__`). Os fundos de bioma ficam em `apps/desktop/assets/biomas` (WebP) e o trunk os copia para o `dist`.

Para buildar:

```
rustup target add wasm32-unknown-unknown
cd apps/desktop/ui && trunk build --release   # gera apps/desktop/ui/dist
cd .. && cargo tauri build                    # o beforeBuildCommand já roda o trunk
```

O `tauri.conf.json` aponta `frontendDist` para `ui/dist` e roda `trunk build --release` na pasta `ui` antes do build.
