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

## Pendente (parte b)

Webview do jogo e `Executor` real; UI Leptos; keyring; ligar a leitura do jogo ao canal de eventos; expor informativos (`raro.novo` etc.) e `sessao.ressincronizar` (recarga por HTTP) à UI; TLS é `rustls` com raízes webpki; commitar o `Cargo.lock` do desktop e passar a usar `--locked`; ícone definitivo.
