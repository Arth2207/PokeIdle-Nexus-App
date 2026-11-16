# Safari e Top Catch semanal

Em **Automação → Controles**, selecione a conta e ligue **Safari · trocar após 1 captura**. O padrão é desligado. O aplicativo precisa estar no modo Ativo e o motor precisa estar autorizado a executar comandos.

O Safari consulta o histórico semanal do Nexus. Enquanto a espécie atual ainda falta, permanece na hunt. Quando a primeira captura elegível dessa espécie chega ao histórico, sorteia uma hunt de outra espécie que ainda não foi registrada na semana. Também sai de uma hunt já completada ou fora da estimativa do evento ao ligar a opção.

Os destinos vêm do login do jogo e precisam estar até o nível do Pokémon ativo. A escolha sorteia espécies, evitando favorecer uma espécie com várias hunts. Exclui destinos de Outlands, bosses, megas e espécies sem categoria Pokébola na Pokédex. Repetidas não liberam novo progresso. Sem destino elegível disponível, registra a situação e aguarda novos destinos ou o reset. A troca por rendimento não compete com Safari enquanto ele estiver ligado.

A verificação ocorre no ciclo do motor (aproximadamente 30 segundos), depois da ingestão da captura; não é uma troca instantânea. As trocas usam a fila existente. Aguarda 60 segundos antes de repetir uma solicitação sem confirmação no estado da hunt. O histórico no banco evita repetir espécies após reiniciar o motor. Se o histórico ou o catálogo não estiverem disponíveis, o Safari aguarda. Não muda bolas, não compra itens e não ativa recompensas.

## Semana e elegibilidade estimadas

A semana começa **domingo às 00h de Brasília** e termina no domingo seguinte às 00h, independentemente do fuso do servidor. Capturas anteriores ao reset não bloqueiam a nova semana.

O Nexus ainda não recebe o placar oficial do Top Catch. A estimativa considera espécies da categoria `pokebola` da Pokédex importada, exclui os ids de Outlands (2000–2999), e descarta eventos identificados como Beast Ball. Quando o evento tem `speciesId`, esse identificador tem prioridade; caso contrário, usa o nome da espécie. Espécies sem classificação ficam separadas e não entram no progresso.

Essa classificação conservadora não comprova a elegibilidade de cada bola no servidor. O histórico pode estar incompleto, principalmente se houve capturas com o Nexus desconectado. O painel identifica os valores como estimativa local e não afirma pagamentos de recompensas.

## Painel

Em **Caçada → Safari**, a conta selecionada mostra:

- Espécies únicas elegíveis, ritmo de espécies novas e projeção até o reset.
- Total de capturas, incluindo repetidas e capturas fora da estimativa.
- Próximo marco, espécies restantes, prazo estimado e os nove prêmios anunciados.
- Novas espécies por dia (domingo a sábado), capturas por espécie e últimas capturas.

Os marcos são 10, 25, 50, 100, 150, 200, 300, 400 e 600 espécies. Os prêmios seguem o anúncio fornecido em 16/11/2026. O jogo paga os prêmios automaticamente; o Nexus não confirma recebimento nem resgata/ativa bônus.

O ritmo usa as primeiras capturas elegíveis de cada espécie na semana que ocorreram nas últimas 24 horas. O denominador é o tempo desde o primeiro evento observado da conta na semana, limitado a 24 horas e incluindo pausas. Com menos de uma hora observada, a projeção fica indisponível. A previsão soma as espécies já registradas ao ritmo multiplicado pelas horas restantes, limitada ao catálogo elegível conhecido. Níveis, pausas e dificuldade das espécies restantes podem reduzir o resultado. **Atualizar** relê os dados.

Eventos de captura com o mesmo id de Pokémon contam uma vez; uma linha do HUD da mesma espécie a até três segundos de uma captura com id é tratada como outra fonte daquela captura. Uma segunda captura real da mesma espécie aparece no total, mas não aumenta o Top Catch.

## Atualização e validação

A migração `20261124000001_motor_safari.sql` adiciona o controle desligado às contas existentes. É necessário atualizar API e aplicativo e aplicar a migração para disponibilizar o recurso instalado. O painel usa `GET /api/cacada/capturas?conta=rotulo`, respeitando tokens restritos à própria conta. O motor carrega o progresso do banco; ignora qualquer progresso incluído na janela enviada pelo cliente.

Validação da consulta em PostgreSQL 16 descartável, sem TCP nem banco de produção:

```sh
python3 ferramentas/fixtures/testar-capturas.py
```

Requer PostgreSQL 16 local, usuário postgres e execução como root. A fixture cobre o limite exato do reset, isolamento de conta, deduplicação, espécies únicas, categorias excluídas, nome/id de espécie, projeção limitada e dados insuficientes. Os testes de `nucleo` cobrem os marcos e as decisões do Safari.
