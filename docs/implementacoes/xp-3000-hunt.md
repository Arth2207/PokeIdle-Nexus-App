# Treinador 3000 e troca de hunt

## Escopo e decisões

Checkout isolado `/opt/nexus-xp-hunt`, branch `fix/xp-3000-hunt`, a partir da versão em produção `666fe3a`. As alterações não relacionadas do checkout `/opt/nexus-rs` foram preservadas. Seleção v2 previamente preparada foi incorporada apenas com suas dependências de combate/janela e proteção de Safari pago.

- `GET /api/progressao?conta=` usa nível e XP atuais do treinador; aceita o progresso textual do relay antigo sem somar XP acumulado duas vezes. Meta fixa 3000. Mede XP de treinador em até 20 minutos na hunt atual; exige 20 intervalos e dois minutos, descarta lacunas >30 s e leituras sem abates recentes. A previsão assume caçada contínua no ritmo atual; não promete hunts ou bônus futuros.
- Caçada abre na aba Meta 3000, com XP restante, XP/h, duração e data estimada em Brasília.
- A troca automática usa nível/slug do catálogo vivo, curva de XP do jogo e combate com atributos recebidos. Sem ganho conservador acima da margem, mantém a hunt. Desconto de 15% para simulações; coleta pelo menos dois minutos/20 abates antes de testar; confirma por cinco minutos e retorna se houver piora. Objetivos XP/equilíbrio não descem para menos da metade do nível da hunt atual sem uma medição comparável recente da candidata.
- Objetivo explícito na UI: XP, equilíbrio, lucro, gemas ou shiny. Campos internos ficam ocultos. O cartão mostra o estado efetivo do perfil; clicar no liga/desliga ganha prioridade sobre o papel farm (`ativoManual`). Objetivo salvo mantém prioridade via `objetivoManual`. Margem e risco aparecem em porcentagem na UI (15 e 40, em vez de 0,15 e 0,40). Risco antigo inválido como 4 recebe o padrão seguro de 40% no estado efetivo.

## Validação

- Núcleo: 132 testes aprovados, seis fixtures externas ignoradas. Inclui Parasect 50 versus hunt 650, XP de treinador versus Pokémon, XP textual/acumulado, nível 3000, amostra insuficiente/lacunas/parada e prioridade do botão sobre farm.
- Clippy de todos os targets do workspace e da UI wasm sem avisos. A consulta real de gravação dos controles passou em transação com rollback, incluindo os dois campos de prioridade manual.
- Replay privado das quatro contas: mantém Silcoon 650 e Camerupt 700 porque as alternativas não superam a margem. Arquivos privados em `/root/pokeidle-projecao/xp-hunt-20261117`.
- Build com um job, sem LTO, para reduzir pressão de memória da VPS. Publicação e smoke tests ainda pendentes enquanto este registro é preparado.

## Limites

A instalação do PC continua compatível; bônus não enviados por versões antigas do relay não podem ser distinguidos na assinatura de contexto. O XP/h é observado, e a previsão se atualiza com as próximas amostras. Reavaliar hunts futuras pode antecipar a meta; interrupções ou expiração de bônus podem atrasá-la. Não alteramos objetivo nem ligamos/desligamos automações de contas reais na validação.
