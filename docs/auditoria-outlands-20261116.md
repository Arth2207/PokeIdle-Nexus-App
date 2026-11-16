# Auditoria de Outlands — 16/11/2026

Análise somente de leitura do aplicativo A2, motor, API, ETL/consultas, wiki local, anúncios fornecidos e dados recentes do Nexus. Nenhuma configuração de conta ou execução de hunt foi alterada.

## Evidência atual do jogo

O evento `welcome` mais recente consultado para uma conta em uso foi recebido em 16/11/2026 às 14:13:11 de Brasília. A origem é `nexus.evento.dados.payload`, não a tabela estática do motor nem uma inferência dos anúncios.

- `outlandTiers` contém 40 entradas.
- `chanceBossToken` é `0.0001`.
- `outlandDex` informa 58 espécies, ids 2001 a 2058.
- As quatro janelas consultadas exibiam o identificador interno 104.

Exemplos da tabela enviada pelo jogo:

| ID interno (`tier`) | Nome exibido | Nível exigido | Multiplicador informado |
| --- | --- | --- | --- |
| 101 | Outland 1 | 150 | 1 |
| 102 | Outland 2 | 500 | 1,1 |
| 103 | Outland 3 | 750 | 1,15 |
| 104 | Outland 4 | 1000 | 1,25 |
| 105 | Outland 5 | 1250 | 1,3 |
| 140 | Outland 40 | 65000 | 20 |

O valor 104 NÃO significa que há uma Outland número 104: é o ID da Outland 4. Os níveis de treinador consultados estavam entre 1033 e 1053; pela tabela recebida, a próxima liberação é no nível 1250. Isso não demonstra, sozinho, qual espécie rende mais.

A wiki local tem cópias de 08/11 e 11/11, com oito Outlands. O anúncio de agosto traz uma versão ainda diferente dos requisitos. Essas fontes precisam ter versão/data; não podem substituir silenciosamente a tabela atual do login.

## Achados e impacto

1. **Regras descartadas pelo coletor.** `crates/a2-nucleo/src/relay/contas/janela.rs::welcome_motor` preserva hunts/bolas/itens, mas não `outlandTiers`, `outlandDex` nem `chanceBossToken`. O evento bruto contém a informação; a janela normalizada usada pelo motor não. `publico` também não carrega esses campos.

2. **ID interno exibido como número da região.** A montagem da janela concatena `Outland {tier_outland}`. Isso produz `Furious Scizor · Outland 104 · nível 150`, embora o login forneça `rotulo: Outland 4`.

3. **Tabela de cálculo antiga e chance-base divergente.** `crates/nucleo/src/motor/regras/hunt.rs` fixa oito tiers e `bronzeToken = 0.0005`. O login atual informa 40 e `0.0001`. Quando a fórmula é aplicada, o componente de tokens parte de uma base cinco vezes maior. O multiplicador é inferido do nível do treinador pela tabela antiga, sem consultar a região efetivamente selecionada. Isso não mede um lucro real cinco vezes maior; é uma divergência na estimativa.

4. **Nome de apresentação usado como identidade de espécie.** `hunt_atual` mantém o sufixo `· Outland 104` no nome, e `melhores_hunts` procura esse nome completo na Pokédex. `Pokedex::por_nome` só ignora maiúsculas/minúsculas; não remove o sufixo. O plano retorna vazio se não reconhece a espécie atual. A identidade deve vir de slug/id, com região e rótulo separados.

A seleção de candidatas também impõe um piso de metade do menor nível entre treinador e lutador (`segura_para`, em `hunt.rs`). As 58 espécies de Outland na Pokédex atual consultada têm `h = 150`. Com um lutador de nível 1000 e treinador de nível 1000, o piso é 500 e essas candidatas são rejeitadas, apesar de a região ser acessível. O piso genérico de progressão de XP precisa ser separado das regras de acesso/rendimento de Outlands.

5. **Comparação mistura tiers e contextos.** `persistencia::cacada::hunts` agrupa eficiência por `hunt_slug` e drops por espécie, juntando contas e sessões. Os eventos normalizados de kill/drop não preservam tier/versão das regras. Isso mistura condições anteriores e posteriores à atualização, além de níveis, equipamentos e bônus diferentes. A medição precisa de conta, espécie, tier, versão, lutador, bônus e tempo observado.

6. **Receita apresentada como lucro.** O mapa do app calcula `Lucro/h = ouro/h + valor de drops em gold/h`, sem descontar poções/bolas/taxas. Não há métrica de gemas/h nessa tela. O motor tem um objetivo gemas, mas usa preços configurados manualmente (`gemaPrecos`) e o cálculo de raros não integra automaticamente preço de vendas, liquidez ou custos. O mercado já possui informações de liquidez; falta conectá-las a esse plano, sem tratar anúncio como venda realizada.

7. **Controles incompletos e objetivo atual diferente do desejado.** As contas em uso consultadas têm papel `farm`, que no motor prioriza `equilibrio`, e não `gemas`. A tela de automação só oferece controles genéricos para campos booleanos/numéricos; objetivo e papel são textos e não têm seletor nessa tela. Alterar para espólio também liga outras regras por efeito do papel; isso precisa ficar explícito antes de qualquer mudança de configuração.

8. **Sem gestão própria de Outlands no app.** Falta apresentar a tabela dinâmica de regiões, bloqueios, multiplicador atual, próxima liberação e comparação do rendimento por espécie/tier. Os parâmetros da decisão de troca de hunt carregam slug/nome/nível, sem escolha explícita de tier. Um controlador deve respeitar requisitos, preservar a escolha e confirmar no estado do jogo o resultado antes de registrar sucesso.

## Prioridade proposta

1. Preservar e versionar o catálogo e as chances do login; manter ID interno, nome exibido e espécie separados.
2. Corrigir reconhecimento da hunt e cálculo pelo tier efetivo; validar a diferença de chance-base antes de usar a previsão.
3. Guardar contexto dos kills/drops e separar dados de antes/depois da atualização; manter desconhecidos como desconhecidos.
4. Calcular gemas/h esperadas e realizadas separadamente, com valores de mercado, liquidez e custos/taxas na moeda correta.
5. Expor objetivo da conta e uma tela Outlands com regiões, bloqueios, drops, ritmo, previsão e motivo da recomendação.
6. Só então usar comparação automática entre Outlands, Safari/shiny e outras fontes de itens vendáveis. Sem interpretar valor de inventário como gemas já recebidas.

Os raros precisam ser apresentados como expectativa estatística. Uma sessão curta sem fragmentos não demonstra que a probabilidade mudou, e uma queda isolada não demonstra que aquele tier é superior.

## Limites

A auditoria identifica divergências entre fonte atual, normalização e cálculos. Não certifica todas as regras do servidor do PokeIdle, cujo backend não está neste repositório. O Nexus recebe dados do jogo, mas não controla os sorteios. Dados brutos de login e anúncios mais recentes têm precedência sobre cópias antigas da wiki para os campos que efetivamente publicam. Os demais campos precisam ser confirmados antes de substituir probabilidades ou automatizar escolhas.
