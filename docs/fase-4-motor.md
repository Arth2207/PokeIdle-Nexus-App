# Fase 4: motor de automação

Plano da fase. Etapas 1 (levantamento), 2 (estado no Postgres) e 3 (rotas de leitura) feitas.

## O que existe hoje

O motor é o `src/motor.js` do `holodex-nexus` (~2.400 linhas). Ele roda de dois jeitos:

- **Electron** (`main.js`): abre as janelas do jogo, lê o HUD e executa comandos na página.
- **Servidor** (`servidor.js`, no pm2): o mesmo `motor.js` com o adaptador Playwright. Na VPS roda com `HDX_SEM_BROWSER=1`: sem navegador, motor e API ativos esperando um cliente externo.

Na VPS, o `/api/estado` responde **sem nenhuma conta conectada** e a última ação registrada tem cerca de 40 h. Ver "Perguntas em aberto".

### Laço principal

`motor()` roda a cada 30 s. Para cada conta, em sequência:

`motorPocao`, `motorCompra`, `motorHunt`, `motorVender`, `motorEquipe`, `motorBola`, `motorRodizio`, `motorShinyBolas`, `motorMedias`, `motorPedras`, `motorRadar`, `motorExtras`

Depois faz o backup diário e salva. Cada passo mistura regra e execução: a decisão sai de `D.config` e do estado lido do jogo, e a execução é um `await comando()` (`__hdxCmd`) ou `avaliar()` (`executeJavaScript`) no meio da função.

Outros laços: coleta das janelas a cada 3 s (`laco()`), debounce de gravação de 500 ms, conferência de estoque 15 s depois de uma compra (desliga a compra automática se o estoque não subiu).

### Estado

Não há Postgres no motor. Tudo está num único JSON:

| Arquivo | Tamanho | Conteúdo |
|---|---|---|
| `dados/holodex-dados.json` | ~470 KB | objeto `D` (abaixo) |
| `dados/holodex-dados.json.bak-AAAA-MM-DD` | 0,3–1,8 MB | cópia diária, no mesmo disco |
| `dados/contas.json` | <1 KB | 4 contas (`id`, `nome`, `cor`) |
| `dados/preferencias.json` | <1 KB | layout, tema, notificações |

Chaves de `D`:

- **Configuração por conta:** `config[nick]` com `alertas, compra, gemaPrecos, hunt, mercadoCompra, papel, pedras, pocao, radar, recarregar, retomar, rodizio, venderNpc, zerar`.
- **Histórico:** `acoes` (últimas 300: `nick, ok, t, texto, tipo`), `dia`, `horas`, `histT`, `medHist`, `saldo`, `carimbos`.
- **Builds:** `builds`.
- **Calibração e medidas:** `calib2`, `medidas`, `vida`, `linhasCache`.
- **Controle:** `pausa`, `bloqueio`, `alertas`, `mercadoAvisos`, `mercadoRejeitado`.
- **Acesso:** `acesso` (e-mails liberados no túnel). É dado sensível; o import não grava em log.

Só em memória (some ao reiniciar): o estado lido de cada janela (`ultimo`, `welcome`, caixa), a fila de `comando`/`avaliar` e o debounce pendente.

Riscos do formato atual: gravação sem arquivo temporário (uma queda no meio corrompe o JSON, e a leitura falha em silêncio e sobe com `D` vazio); sem lock; backup no mesmo disco.

### Dependências

- Nenhum `require` do mapper e nenhuma tabela Postgres.
- Proxy HTTP para o mapper (porta 8181) em `/api/bolsa/*`, `/api/equipes/*`, `/api/raros2`, `/api/shiny` (GET), `/api/ia/*` e `POST /api/bolsa/meus-itens`. Essas rotas já são do servidor e seguem nas fases delas; o motor só repassa.
- Quem chama o motor: as páginas de `web/` (servidas pelo próprio motor) e o relay, que lê `/api/estado` a cada 30 s e executa comandos da fila postando em `/api/config` e `/api/team`.

## Inventário das rotas

38 rotas servidas pelo motor. Classe: **D** = decisão (vai para o Nexus), **E** = execução (vai para o app, fase A2), **M** = mista (separar).

| Método | Caminho | Faz | Classe |
|---|---|---|---|
| GET | `/api/estado` | snapshot de todas as contas: ações, planos, rodízio, resumos, caixa, config | M |
| POST | `/api/config` | mescla a config de uma conta | D |
| GET, POST | `/api/backup` | exporta ou importa `D` | D |
| GET, POST | `/api/builds` | lista, salva ou remove builds | D |
| GET | `/api/builds/sugerir` | sugere build para um boss | D |
| GET | `/api/builds/teorica` | build teórica | D |
| POST | `/api/builds/simular` | o que `aplicar` mandaria (dry-run) | D |
| GET | `/api/builds/prioridade` | ranking de bosses | D |
| POST | `/api/builds/aplicar` | aplica build no jogo | M |
| GET | `/api/equipe/melhor` | melhor time possível de uma conta | D |
| GET | `/api/equipe/melhor/todas` | idem, todas as contas | D |
| GET | `/api/raros` | plano de raros | D |
| GET | `/api/shiny` | plano de shiny (GET vai para o mapper) | D |
| POST | `/api/pausa` | pausa geral | D |
| GET | `/api/espolio` | espólio | D |
| GET | `/api/pedras/plano` | plano de pedras | D |
| GET | `/api/pedras/diag` | diagnóstico; com parâmetros, consulta o jogo | M |
| GET | `/api/vender/previa` | plano de guardar/vender, com hash | D |
| POST | `/api/vender/confirmar` | libera a venda automática | D |
| POST | `/api/vender/aplicar` | executa a venda (confere o hash) | M |
| GET | `/api/historico` | histórico, resumo do dia e da semana | D |
| GET | `/api/financeiro` | financeiro | D |
| GET | `/api/combate` | combate da conta | D |
| GET | `/api/cache-linhas` | tamanho do cache de linhas do HUD | D |
| POST | `/api/hunt` | troca de hunt manual | M |
| POST | `/api/auto-set` | ajuste pontual da automação do jogo | M |
| POST | `/api/mercado/favoritar` | favorita espécies no mercado do jogo | M |
| POST | `/api/amigos/conectar` | amizade entre as contas | M |
| GET | `/api/diagnostico` | saúde do coletor por conta | M |
| GET, POST | `/api/acesso` | e-mails liberados no túnel (só local) | D |
| POST | `/api/painel/recarregar` | recarrega o painel do jogo | E |
| POST | `/api/retomar` | clica em "continuar aqui" | E |
| POST | `/api/eco-reset` | script de economia na página | E |
| GET | `/api/versao`, `/api/dev/versao` | versão e hot-reload da interface | E |
| GET | `/api/dev/hunts` | depuração de hunts | E |

Contrato: quase tudo responde `{ok, ...}`; conta inexistente é 404 `{erro: "conta"}`; validações 400/409 com `erro`. Só o POST confere `Origin`; o acesso remoto exige os cabeçalhos do Cloudflare Access.

Defeitos encontrados (paridade mantém; correção atrás de `NEXUS_MELHORIAS` ou no cutover):

- O relay posta comandos `team.add`/`team.set` em `/api/team`, que o motor não serve: esses comandos falham.
- O comando `reload` manda `{recarregar: true}` para `/api/config`, e o handler ignora o campo.
- `web/vender.html` chama `/api/colecao` e `/api/precos`, que não existem.

## Fronteira com a A2

**Regra:** o Nexus decide e enfileira; o app executa e devolve o resultado.

- **Nexus (esta fase):** estado das contas, config, builds, histórico, financeiro e todas as regras (`plano*`, `melhorTimePossivel`, `priorizarBosses`, `planoVender`, hash da venda). Regra pura no `nucleo`, I/O no `nexus-api`.
- **App (A2):** leitura da janela (`ultimo`, `welcome`, caixa, HUD), `comando()`, `avaliar()`, scripts injetados, recarregar/retomar.
- **Rotas mistas:** viram duas metades. O Nexus calcula a lista de passos (o `builds/simular` já é essa metade) e grava em `nexus.comando`; o app executa e confirma. O `await comando()` dentro do handler vira fila assíncrona: a rota responde com os ids dos comandos, e o resultado chega pela confirmação.
- **Estado do jogo:** o laço do motor precisa do que a janela leu. No Nexus, ele chega por push do app (um evento `estado` no `/ws`, mesmo canal da fase 3). Até a A2 existir, a sombra lê o `/api/estado` do legado (só `GET`).

## Plano

### Etapa 2: estado no Postgres (feita)

Migration `20261116000001_motor.sql`, aditiva:

| Tabela | De |
|---|---|
| `motor_conta` | `contas.json` |
| `motor_config` (nick, seção, `jsonb`) | `config[nick]`, uma linha por seção. Conta sem seção vira uma linha com seção vazia |
| `motor_acao` (colunas `nick`, `t`, `tipo`, `ok` + registro inteiro em `jsonb`) | `acoes`; os campos variam com o tipo. `id` cresce no sentido do tempo |
| `motor_estado` (chave, `jsonb`) | as outras chaves de topo: `dia`, `saldo`, `horas`, `builds`, `calib2`, `pausa`... |
| `motor_importacao` | registro de cada importação (tipo, sha256 do arquivo, linhas) |

As chaves de `motor_estado` ganham tabela própria quando a regra da etapa 4 precisar consultá-las; até lá o formato ainda varia. `acesso` e `preferencias` ficam fora: são do túnel e da interface, não do motor.

```
nexus-api importar motor < holodex-dados.json
nexus-api importar motor-contas < contas.json
nexus-api comparar-motor motor < holodex-dados.json
nexus-api comparar-motor motor-contas < contas.json
```

A importação substitui tudo numa transação e valida o arquivo antes de tocar no banco. A comparação é exata (números pelo valor, como no JS; inteiros sem passar por `f64`) e para na primeira diferença. Com o arquivo real da VPS, num banco descartável: 70 seções de config, 300 ações, 16 chaves de estado e 4 contas, e arquivo e banco iguais.

Em produção, o estado importado hoje é o do arquivo da VPS, provisório: é o que o motor da VPS lê, então serve de referência para o comparador. O motor ativo é o do PC; quando a cópia periódica do arquivo dele existir, a importação passa a usá-la (ela substitui tudo).

**Ordem das chaves.** O legado itera objetos na ordem de inserção, e isso decide resultados (empates nos drops do dia, a ordem das contas no financeiro; no arquivo real há empates). O `jsonb` reordena as chaves, e o `serde_json` padrão também. Por isso o workspace usa o `serde_json` com `preserve_order`, e a migration `20261117000001_motor_ordem.sql` (aditiva) acrescenta `texto json` (o tipo `json` guarda o texto como veio) e `ordem`. O import grava os dois; a leitura monta o objeto em Rust, na ordem do arquivo. O `comparar-motor` confere valores e ordem.

### Etapa 3: rotas de leitura (feita)

18 rotas `GET` no caminho e com o contrato do legado: `builds`, `builds/prioridade`, `builds/sugerir`, `builds/teorica`, `equipe/melhor`, `equipe/melhor/todas`, `raros`, `espolio`, `pedras/plano`, `pedras/diag`, `vender/previa`, `historico`, `financeiro`, `combate`, `cache-linhas`, `diagnostico`, `estado`, `backup`. Regra em `nucleo::motor` (`config`: `CONFIG_PADRAO`, `cfgBase`, `cfg`, a carga do `D`; `leitura`: as rotas), I/O em `nexus-api` (`motor::responder`, `rotas/motor.rs`).

- **Sem conta ao vivo.** As janelas do jogo (`ultimo`, `welcome`) só existem no app. Até a A2 mandar esse estado, o Nexus responde como o legado sem nenhuma conta conectada: rotas com `nick` dão 404 `{erro: "conta"}`, e as que agregam contas usam só o `D`. O financeiro e o histórico já saem completos (dia, saldo, semana, comparação com ontem); o que depende da janela (ouro agora, preços de poção e bola, ritmo) entra com a A2.
- **Relógio.** O legado usa data e hora locais. `NEXUS_MOTOR_FUSO_MIN`: -180 (PC, padrão) ou 0 (VPS).
- **Token preso a conta.** Rotas com `nick` só respondem sobre a própria conta; as que juntam todas as contas respondem 403.
- **Erro.** Onde o JS lançaria `TypeError` (campo ausente lido como lista), 500 `{erro: "erro interno"}`, como o legado.
- **Comparador `motor.*`.** `NEXUS_PARIDADE_MOTOR_URL` liga a comparação no `paridade-continua`; `nexus-api paridade-motor <url>` roda uma vez. Cada rota sem `nick` e, as por conta, com cada nick do estado. Comparação exata (status e corpo), sem tolerância: o Nexus calcula no `agora` que o legado devolveu. Contra o motor da VPS, num banco descartável com o arquivo dela: 58 de 58 iguais.

### Etapa 4: regras no `nucleo`

Uma função pura por `motorX`: recebe estado da conta + config + relógio e devolve uma lista de decisões (`enum` com os passos). Fixtures: snapshots reais do `/api/estado` e do `holodex-dados.json`, com o resultado do legado gravado. Conceitos novos: máquina de estados com `enum`, agendador com `tokio::time::interval`, estado compartilhado com `ArcSwap`.

### Etapa 5: escrita e laço

Rotas de escrita e o laço de 30 s atrás de `NEXUS_ESCRITA_MOTOR` (desligada). Com a flag desligada, o laço roda em **sombra**: calcula as decisões e grava em `nexus.motor_decisao`, sem enfileirar comando. O comparador confronta essas decisões com as `acoes` do legado no mesmo período.

### Etapa 6: cutover

Na etapa 6, depois da paridade.

## Perguntas em aberto

Respondidas: o motor ativo é o Electron do PC (perguntas 1 e 2). Falta liberar a porta dele na Tailscale para a VPS e combinar a cópia do arquivo. O `GET /api/backup` não serve para isso: traz só parte do `D` (sem `saldo`, `horas`, `carimbos`).

1. **Onde o motor roda de verdade hoje?** Na VPS ele está sem navegador e sem contas. Se o motor ativo é o Electron do PC, o `holodex-dados.json` de referência está lá, e a sombra precisa de uma cópia periódica desse arquivo e de acesso ao `/api/estado` dele (pelo Tailscale, só leitura).
2. **Importar qual arquivo:** o da VPS (parado há ~40 h) ou o do PC?
3. **`motorExtras` e `motorRadar`** não foram detalhados no levantamento; entram na etapa 4.
