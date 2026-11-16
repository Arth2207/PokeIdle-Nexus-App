# Auditoria Nexus / PokeIdle — 16/11/2026

## Escopo e conclusão

Leitura do código, banco e serviços Nexus, anúncios fornecidos, wiki local e páginas públicas do PokeIdle.io. Consultas em produção somente de leitura; nenhuma configuração de conta foi alterada. Não há acesso ao backend privado do jogo nem a todo o Discord. Jogos homônimos em outros domínios foram excluídos.

O Nexus tem uma boa base de coleta e automação, mas ainda não há evidência suficiente para chamar sua escolha de hunt de ótima para gemas. As principais lacunas são fidelidade do modelo ao estado recebido, confirmação das ações, qualidade temporal dos dados e contabilidade. O ganho mais provável vem de usar melhor o que já é recebido, antes de adicionar novos mecanismos de decisão.

## Dados recebidos do jogo

Consulta de 16/11 aproximadamente 18:03 UTC, janela móvel de 24 horas:

| Evidência | Resultado | Interpretação |
|---|---|---|
| `welcome.payload.chanceShiny` | 0,000041666666666666665 | 1/24.000; o denominador correto ainda depende da mecânica descrita pelo jogo |
| `chanceBossToken` | 0,0001 | Base enviada pelo jogo, sem inferir bônus adicionais |
| `outlandTiers` | 40 | O catálogo antigo de oito tiers ficou desatualizado |
| `bossesJogaveis` | 27 entradas | A lista estática de 25 bosses exige revisão por identidade, requisitos e disponibilidade |
| `kill` / `capturado` | 89.684 / 575 eventos | Contagens coletadas; não provam horas de atividade ou elegibilidade no Top Catch |
| `ws.bruto` | 554.829 eventos | Discovery domina o volume; avaliar retenção e coleta temporária |
| `mercado.extrato` mais recente | 15/11 21:50 UTC | Não usar como retrato financeiro atualizado em 16/11 à tarde |
| `treino.resultado` mais recente | 15/11 21:50 UTC | Mostrar idade da amostra nos indicadores de treino |
| Comandos com resultado | 9.599 `sombra`, 59 `ok` | Cerca de 99,4% simulados. Sombra não é execução nem falha do jogo |

Contagens mudam continuamente. Eventos não equivalem necessariamente a transações únicas sem considerar deduplicação e cobertura. Não foi copiado payload de login, credencial ou informação de outros jogadores para este relatório.

## Regras públicas que mudam a decisão

Segundo os [termos oficiais](https://pokeidle.io/terms), shiny é sorteado por lançamento de bola; Outlands não têm shiny. Sair ou trocar de área durante o Safari pode consumir o tempo restante do ingresso. Drops de boss não recebem o bônus de loot de hunts. Gemas usam taxas com arredondamento; anúncio não significa venda. Essas regras exigem separar modalidades e contabilizar custos corretamente. A [página inicial](https://pokeidle.io/) tem informações que divergem entre seus próprios trechos e do catálogo recebido: é uma referência de apresentação, insuficiente como tabela técnica atualizada.

Para cada campo: usar valor explícito e atual do jogo, registrar origem/data/versão; confrontar com anúncio recente e regra oficial. Não inferir probabilidades ausentes de uma sessão curta nem considerar o cliente capaz de alterar sorteios.

## Prioridades

### P0 — limites por conta e proteção de operações

- `crates/nexus-api/src/rotas/janela.rs::alterar_config` aceita nick sem verificar `Cliente.conta` no próprio handler. Outras rotas já fazem essa verificação. Revisar middleware e aplicar autorização uniforme por conta em leitura e escrita antes de distribuir tokens restritos. Não foi tentada alteração cruzada em produção.
- Criar proteção explícita de modalidade para Safari pago: impedir que uma regra genérica saia/troque de área sem intenção clara. O nome Safari usado pelo rodízio Top Catch também pode confundir duas funções diferentes.
- Diferenciar comando enviado, simulado, aceito e efeito confirmado. A confirmação de troca de hunt foi acrescentada ao trabalho pendente; revisar os demais comandos pelo estado observado, especialmente compras, vendas e movimentações.

### P1 — recomendação realmente fiel ao jogo

- Em `crates/nucleo/src/motor/regras/hunt.rs`, `ativo_de` e a construção de `Opcoes` não transportam shiny para o combatente. O modelo também precisa confrontar TMs, itens, refinamentos e bônus com o estado real. Recomendações baseadas em atributos incompletos podem escolher a hunt errada.
- Substituir catálogos fixos pelos dados recebidos quando disponíveis. Em `regras/equipe.rs`, `BOSSES_NIVEL` tem 25 registros e `CONTAS_BOSS` fixa uma conta. As 27 entradas recebidas não provam sozinhas 27 bosses distintos; comparar IDs antes de migrar.
- Usar o tier efetivamente selecionado, não o maior inferido pelo nível. A auditoria específica de Outlands documenta ID 104 = Outland 4, piso de nível inadequado e chance antiga de token.
- Calibrar tempo de luta/deslocamento com medições por contexto. Comparar gemas líquidas realizadas e expectativa com confiança, sem chamar uma queda rara isolada de melhoria comprovada.
- Revisar a previsão de shiny usando lançamentos, sucesso de captura e bônus na etapa correta; não usar kills como se fossem capturas.

### P1 — contabilidade e transparência no aplicativo

- Mostrar receita realizada, estoque estimado e expectativa separadamente. Usar preço de venda e liquidez, taxas exatas por modalidade e arredondamento por lote; manter Coins e gemas separados.
- Exibir idade/intervalo de cobertura das fontes. Extrato antigo deve aparecer como desatualizado, nunca como zero de vendas recente.
- Mostrar modo sombra de forma destacada e contadores separados de execução. Hoje a grande maioria das decisões não produz ações reais.
- Conferir interação de papel da conta e objetivo: o papel farm aplica equilíbrio. O seletor com prioridade manual foi implementado no trabalho pendente, sem trocar configurações das contas automaticamente.
- Projeções semanais devem usar tempo efetivamente observado, reset em Brasília e elegibilidade das capturas. São projeções locais, não placar oficial.

### P2 — operação, recursos e manutenção

- Fazer coleta bruta temporária com expiração, retenção curta e contadores agregados. A amostra anterior do mesmo dia somava aproximadamente 418 MB de payload bruto em 24h; isso não é o tamanho físico da tabela. Evitar manter Discovery ligado permanentemente sem necessidade.
- Registrar motivo da recomendação, versão de regras, contexto e resultado confirmado. Permitir comparar antes/depois sem misturar tier, equipamentos ou bônus.
- Unificar janelas de repetição: há proteção global de três minutos e tentativas locais com prazos menores. A interface deve refletir o prazo efetivo.
- Manter testes de migração com banco e fixtures de protocolo atualizadas. O CI bloqueou corretamente a publicação por `DISTINCT` sobre coluna JSON; a correção seleciona os nicks distintos antes de construir o JSON.
- No Windows, avaliar armazenamento do token no cofre do sistema; o arquivo local não tem a mesma proteção de permissões Unix.

## O que já está bem encaminhado

Separação entre domínio, persistência e API; ingestão com deduplicação; fila com leases e comandos serializados por conta; modo sombra; testes; deploy com healthcheck e reversão. Essas peças tornam as melhorias viáveis sem reescrever o projeto. Não é necessário reativar a IA aposentada pelo dono em 13/11 para corrigir as lacunas encontradas.

## Estado da publicação e limites

Safari/Top Catch/Outlands foram preparados e enviados, mas a primeira execução do CI falhou em migração antes do deploy. A correção está em validação; código preparado não equivale a versão publicada. Verificar Actions e saúde da versão efetiva antes de anunciar conclusão.

Este relatório propõe melhorias; não certifica lucro, probabilidades ocultas nem regras não transmitidas pelo jogo. O levantamento não alterou hunts, compras, vendas ou flags de automação. Ver também [auditoria de Outlands](auditoria-outlands-20261116.md) para a evidência detalhada anterior às correções.
