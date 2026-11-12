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

### Etapa 4: regras no `nucleo` (feita)

Uma função pura por `motorX` em `nucleo::motor::regras`: conta ao vivo (`ContaViva`) + `D` + relógio → lista de `Decisao`. O estado que o legado guarda entre ciclos (antiflood dos alertas, marcas de tempo por conta, última hunt) fica em `EstadoLaco`, para a regra continuar pura. `nucleo::motor::laco::ciclo` roda as doze na ordem do legado: poção, compra, hunt, vender, equipe, bola, rodízio, shiny, médias, pedras, radar, extras.

- **Estado da janela.** O `/api/estado` só resume o `welcome`. O motor do PC ganhou duas rotas só de leitura: `GET /api/nexus/dados` (o `D` inteiro, sem `acesso`) e `GET /api/nexus/janela` (`ultimo`, `welcome`, `pub`, `huntKey`, linhas do HUD, estado da venda). São a fonte da sombra até a A2.
- **Foto do início do ciclo.** As regras do legado mudam o estado da conta durante o ciclo (`cons`, `ouroMax`, `huntDesde`). Para o Nexus decidir sobre o mesmo estado, o motor do PC tira uma foto de cada conta logo depois da checagem de pausa (`nexusFoto`) e a janela devolve `ciclo` (instante e estado do laço) e `antes` por conta. Sem ela, a sombra via o estado já alterado e dava falso acerto.
- **Versão de referência.** O motor do PC e a cópia do servidor tinham divergido; foram unidos (merge de três vias, um conflito resolvido a favor do servidor). As regras são portadas da versão unida.
- **Fixtures.** Capturas do mesmo instante de `nexus/dados`, `nexus/janela` e `estado`, com quatro contas. Todos os planos (`planoCompra`, `planoHunt`, `planoVender` com hash, pedras, rodízio, equipe, builds, combate, raros) batem com o que o legado calculou. Ficam fora do repositório (dados das contas); os testes que as usam ficam `#[ignore]`.
- As funções `motorX` têm testes de unidade; a prova delas é a sombra (etapa 5).

### Etapa 5: laço em sombra e escrita atrás de flag

`nexus-api paridade-continua` roda também o laço do motor quando `NEXUS_MOTOR_SOMBRA_URL` aponta para o motor do legado (`NEXUS_MOTOR_SOMBRA_S`, padrão 30 s). A cada ciclo:

1. lê `GET /api/nexus/dados` e `GET /api/nexus/janela` do legado e grava as ações novas;
2. só segue se o legado fechou um ciclo novo (`ciclo.t`); usa esse instante como `agora`, tira do `D` as ações do próprio ciclo e pega o antiflood e a última hunt do estado do laço do legado;
3. acumula as linhas do HUD de cada conta (a rota manda as últimas 500; o espelho acha a sobreposição e acrescenta o resto, como o vetor do legado, que só cresce);
4. roda as regras (`laco::ciclo`, com `catch_unwind`: um pânico perde o ciclo, não o laço; nada com `D.pausa`);
5. grava as decisões em `nexus.motor_decisao` (com as ações que o legado gravaria). As ações do legado ficam em `nexus.motor_acao_legado` (o legado guarda só 300; aqui acumulam sem repetir, chave com `md5` do texto). Poda de hora em hora: 14 dias de decisões, 30 de ações.

`nexus-api motor-sombra-relatorio [--desde <horas>]` confronta as duas: uma decisão bate com uma ação da mesma conta e tipo, com o mesmo texto (ou o começo do de falha, que traz a resposta do jogo), a até dois ciclos mais um minuto. Só entram os tipos que o Nexus já decide.

Com `NEXUS_ESCRITA_MOTOR=true`, os comandos do ciclo vão também para a fila da fase 3 (`nexus.comando`, origem `motor`, tipo `motor.<t>`, validade de dois ciclos, chave de idempotência `motor:<ciclo>:<i>`), para o app executar. Fica desligada até o cutover; ligada, só nos testes de integração.

**O que a sombra ainda não reproduz:** o que acontece depois da resposta do jogo dentro da própria regra (ex.: a conferência de estoque 15 s depois da compra, que desliga a compra automática). Em sombra o Nexus assume resposta `ok`; a consequência do lado do legado chega no `D` do ciclo seguinte, e é ela que o Nexus passa a ver.

### Etapa 6: plano de cutover

Tudo nasce desligado; cada passo volta atrás desligando a flag e reiniciando o container.

**Sombra (já possível):**

1. Motor do PC com as rotas `/api/nexus/*` (versão unida) e alcançável pela Tailscale.
2. `NEXUS_MOTOR_SOMBRA_URL` no `nexus-paridade`, com `NEXUS_MOTOR_SOMBRA_FUSO_MIN=-180` (o `NEXUS_MOTOR_FUSO_MIN` segue o do comparador).
3. `motor-sombra-relatorio` por 7 dias de uso normal: o critério é nenhuma decisão só de um lado, fora as explicadas (primeira hora depois de subir, ações feitas na mão).
4. Comparador `motor.*` contra o PC, com o estado importado de `/api/nexus/dados`.

**Cutover (decisão do dono do projeto, numa janela só, depois da A2):**

1. O app da A2 conectado no `/ws` executa os comandos `motor.*` e manda o estado da janela (substitui a rota `/api/nexus/janela`).
2. Importar o `D` do PC uma última vez; o estado do motor passa a ser só do Nexus (`nexus.motor_*`), e as rotas de escrita do motor (config, builds, pausa, venda) passam a gravar lá.
3. Ligar `NEXUS_ESCRITA_MOTOR` e, na mesma janela, desligar o laço do motor do legado no PC, para nunca haver dois executores.
4. Ligar `NEXUS_MELHORIAS` para o que foi guardado atrás dela.

**Volta atrás:** desligar `NEXUS_ESCRITA_MOTOR` e religar o laço do legado; o `D` do legado continua de pé até a fase de desligamento.

## Perguntas em aberto

Respondidas: o motor ativo é o Electron do PC, alcançável pela Tailscale (só leitura). A sombra lê o `D` dele pela rota `/api/nexus/dados`; o `GET /api/backup` não serve (traz só parte do `D`). `motorExtras` e `motorRadar` foram portados na etapa 4.
