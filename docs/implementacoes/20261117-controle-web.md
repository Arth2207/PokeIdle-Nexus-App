# Controle de contas pelo painel web

O painel web usa GET/POST `/api/relay/controle`, com a autenticação da borda. A configuração do aplicativo instalado permanece na versão desktop; o site não pede tokens nem invoca o Tauri.

O GET informa dados recentes, último pedido de modo e confirmação do cliente por conta. POST aceita somente `modo` (ativo/sombra) ou `hunt` (slug no catálogo recebido do jogo, dentro do nível do treinador). O nick enviado à fila é o nick exato recebido no jogo. Tokens limitados a uma conta não podem alterar outra.

O cliente atualizado reconhece `relay.modo` sem enviar esse comando ao jogo. A alteração afeta somente a conta destinatária e preserva expiração, escopo, serialização e deduplicação dos comandos. O estado remoto dura enquanto o núcleo está aberto; reiniciar restaura o modo local.

O cliente antigo confirma o pedido como `sombra` e não altera o modo. O site mostra atualização necessária e mantém a troca de hunt desabilitada. O painel não pode atualizar o executável instalado nem contornar esse limite. É necessária uma atualização única do cliente para executar comandos pelo site.

Validação: seis testes do despachante; teste da API com PostgreSQL descartável; compilação da API e da UI; teste Playwright com respostas simuladas para confirmação, troca de hunt e cliente antigo. O teste de navegador não equivale a uma execução no jogo.
