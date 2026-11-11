# Prompts dos fundos (biomas)

Prompts para gerar, no ChatGPT, as 13 paisagens de fundo do app (ver *Biomas* em [telas.md](telas.md)), cada uma em versão diurna e noturna.

## Como usar

1. Numa conversa nova, cole o **lote diurno**. Ele gera as 13 paisagens de dia, uma por imagem.
2. Depois, para cada paisagem, use o **prompt noturno**. Ele edita a imagem diurna, o que mantém a mesma composição.
3. Salve cada arquivo com o nome indicado (`bioma-<nome>-dia.png` e `bioma-<nome>-noite.png`).

As imagens geradas **não** entram no repositório público. Elas vão para a pasta de assets do app, que o build empacota.

## Lote diurno

```
Vou pedir 13 imagens. Gere UMA imagem por vez, em sequência, sem juntar paisagens na mesma imagem. Depois de cada uma, siga para a próxima.

ESTILO (igual em todas): wallpaper horizontal 16:9 na maior resolução possível. Pintura digital de cenário de filme de animação, limpa, luminosa e detalhada, com cores vivas porém naturais. Estética solarpunk: natureza exuberante com, no máximo, um toque discreto de tecnologia sustentável ao longe (uma turbina eólica, uma passarela de vidro coberta de plantas, um domo de estufa), nunca em primeiro plano. Luz de manhã vinda de cima à esquerda em todas as paisagens. Profundidade atmosférica: primeiro plano nítido nas bordas, plano médio rico, fundo suave e enevoado.

COMPOSIÇÃO (é o fundo de um app; painéis de vidro cobrem o centro):
- Os elementos mais interessantes ficam no terço superior (céu, horizonte, montanhas, copas) e nas bordas esquerda, direita e inferior (vegetação, pedras, água).
- O centro da imagem é calmo, com pouco contraste e poucos detalhes, sem nenhum objeto importante.
- A imagem precisa continuar bonita se for cortada nas laterais.

PROIBIDO: pessoas, animais, criaturas ou monstros de qualquer tipo; texto, logos, interface; estilo anime de personagens; neon; céu dramático escuro; partículas.

IMAGEM 1 · RIACHO E LAGO (arquivo bioma-riacho-dia): riacho cristalino serpenteando entre pedras arredondadas e musgo, desaguando num lago calmo de água turquesa; pequenas cachoeiras à esquerda; floresta nas margens; montanhas suaves ao fundo refletidas na água.

IMAGEM 2 · CAMPOS E PENHASCOS (bioma-campos-dia): planície ampla de capim alto ondulando ao vento, flores silvestres brancas e amarelas, trilha de terra; penhascos e falésias ao fundo com nuvens altas e céu aberto; algumas árvores isoladas.

IMAGEM 3 · FLORESTA E BOSQUE (bioma-floresta-dia): floresta densa de árvores altas com raios de sol atravessando as copas, samambaias e flores no chão, troncos cobertos de musgo nas bordas, clareira de luz suave no centro.

IMAGEM 4 · FLORESTA SOMBRIA (bioma-sombria-dia): floresta antiga de árvores retorcidas e raízes expostas, névoa baixa, cogumelos e líquens, luz filtrada e esverdeada; misteriosa porém clara o bastante para ser lida de dia, nunca escura ou assustadora.

IMAGEM 5 · CLAREIRA MÍSTICA (bioma-mistica-dia): clareira circular cercada de árvores claras, flores em tons lilás e rosa, pedras antigas com runas desgastadas formando um círculo, cristais rosados brotando do chão nas bordas, luz suave e perolada.

IMAGEM 6 · VULCÃO (bioma-vulcao-dia): encosta vulcânica com rochas escuras e rios de lava ao longe, vegetação resistente em primeiro plano, fumaça clara subindo do cume, céu limpo; luminoso, sem clima de catástrofe.

IMAGEM 7 · PLANÍCIE TEMPESTUOSA (bioma-tempestade-dia): planície aberta com grama alta, nuvens carregadas ao longe com raios distantes, mas sol forte no primeiro plano; postes de energia e turbinas eólicas modernas no horizonte.

IMAGEM 8 · MONTANHA E CÂNION (bioma-montanha-dia): cânion de rochas avermelhadas e estratificadas, rio estreito no fundo do vale, arbustos e árvores secas nas bordas, montanhas altas ao fundo; luz quente da manhã.

IMAGEM 9 · PÂNTANO (bioma-pantano-dia): pântano de águas paradas esverdeadas com vitórias-régias, árvores de raízes aéreas, juncos altos nas bordas, névoa leve sobre a água; atmosfera úmida mas luminosa.

IMAGEM 10 · TEMPLO NA MONTANHA (bioma-templo-dia): templo de pedra e madeira no alto de uma montanha, escadarias entre pinheiros, bandeirolas ao vento, nuvens abaixo do nível do templo, picos ao fundo.

IMAGEM 11 · MONTANHA NEVADA (bioma-neve-dia): vale de montanha coberto de neve, pinheiros nevados nas bordas, lago congelado no centro, picos altos e céu azul limpo, neve brilhando ao sol.

IMAGEM 12 · PICO E RUÍNAS ANTIGAS (bioma-ruinas-dia): ruínas de uma civilização antiga em pedra e metal envelhecido no topo de picos altos, pontes de pedra entre os picos, vegetação tomando as ruínas, céu amplo com nuvens altas.

IMAGEM 13 · OUTLAND (bioma-outland-dia): outro mundo: ilhas de rocha flutuando no céu ligadas por cachoeiras que caem no vazio, vegetação de cores incomuns (folhas azul-esverdeadas e lilás), dois sóis pálidos no céu; estranho e maravilhoso, ainda luminoso e calmo.
```

## Prompt noturno (um por paisagem)

Na mesma conversa, depois de gerar as diurnas, mande uma vez para cada paisagem, trocando o número:

```
Pegue a IMAGEM N desta conversa e gere a versão noturna dela, mantendo EXATAMENTE a mesma composição: os mesmos elementos, nas mesmas posições e no mesmo enquadramento. Mude só a hora do dia:
- céu azul-profundo com estrelas e uma lua grande e suave, refletida na água quando houver água;
- luz geral fria e azulada vinda da lua, de cima à esquerda;
- plantas, cogumelos e flores com bioluminescência suave em verde-limão e ciano, espalhada com moderação, principalmente nas bordas;
- alguns vagalumes discretos;
- se houver construções, janelas acesas em luz quente.
Atmosfera calma e mágica, não sombria: a paisagem continua legível. Centro da imagem continua calmo e com pouco contraste. Sem pessoas, criaturas, texto ou interface.
```

Nomes dos arquivos noturnos: `bioma-<nome>-noite.png` (por exemplo, `bioma-riacho-noite.png`).

## Ajustes comuns

- Ficou detalhado demais no centro: "deixe o centro mais vazio e suave, os painéis do app vão cobrir essa área".
- Apareceu algum animal: "remova todos os animais e criaturas".
- A versão noturna mudou a composição: "mantenha a imagem original e só escureça e mude a iluminação para noite".
