# 0009. App desktop em Tauri 2 com interface em Leptos

- Status: aceito
- Data: 2026-11-11

## Contexto

O app do PC hoje é Electron com React. Ele faz três coisas:

1. abre o jogo num navegador embutido;
2. injeta o script que captura o WebSocket do jogo;
3. mostra o painel de controle.

Junto dele roda o `holodex-relay.js`, que envia os dados capturados ao servidor e executa os comandos da fila.

Com o backend em Rust, a ideia é levar o app para Rust também. A restrição é o item 1: o app precisa renderizar uma página web de terceiros. Frameworks de UI nativa (egui, iced) não fazem isso.

Opções avaliadas:

- **egui ou iced + extensão de navegador:** UI nativa, com a captura feita por uma extensão. Ficam duas peças separadas para instalar e sincronizar.
- **Dioxus desktop:** UI em Rust sobre webview, mas com suporte menos maduro para abrir um site externo e injetar scripts.
- **Tauri 2:** núcleo em Rust, usando o webview do sistema (WebView2 no Windows), com suporte nativo a várias janelas, a carregar URLs externas e a scripts de inicialização.

## Decisão

- **Tauri 2** como casca do app.
- **Núcleo em Rust**: cliente WebSocket do Nexus (protocolo em [websocket](../websocket.md)), execução de comandos no jogo, reconexão e armazenamento local do token. Substitui o `holodex-relay.js`.
- **Janela do jogo**: o PokeIdle num webview, com o coletor injetado como script de inicialização. O coletor continua em JavaScript porque roda dentro da página do jogo, a mesma exceção registrada na [ADR 0001](0001-rust-puro.md). Ele só captura e repassa ao núcleo, sem nenhuma lógica.
- **Interface** (painel, bolsa, equipes, automação) em **Leptos**, compilada para WebAssembly.
- A mesma interface Leptos também é publicada como site estático pelo Nginx da VPS, para acesso pelo navegador ou pelo celular via Tailscale.
- O app vive no mesmo repositório, em `apps/desktop`, e usa os tipos compartilhados do workspace (mensagens do WebSocket, DTOs da API), o que garante que cliente e servidor falam o mesmo contrato em tempo de compilação.

## Consequências

- Instalador em torno de 10 MB e consumo de RAM de dezenas de MB, contra cerca de 150 MB e centenas de MB do Electron.
- Uma linguagem só de ponta a ponta, com contrato da API checado pelo compilador nos dois lados.
- Uma interface para duas entregas: app e web.
- O ecossistema de UI em Rust é menos maduro que o do React (menos componentes prontos). Se a interface travar o projeto, ela pode ser trocada por React dentro do mesmo Tauri sem mexer no núcleo, então a decisão é reversível.
- O WebView2 do Windows é atualizado pelo sistema, fora do nosso controle. O coletor precisa ser testado contra as versões novas.
- O build do instalador Windows roda no CI (runner `windows-latest`), seguindo a [ADR 0008](0008-build-somente-no-ci.md).
