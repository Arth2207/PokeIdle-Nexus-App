//! Tema "Mutirão Solar": cartaz de cordel que ganha vida. Papel kraft com azulejo, traço de
//! tinta grosso, cantos de corte de madeira, sombra de carimbo, abas de bandeirinha e a paleta
//! de feira (sol, urucum, bananeira, azulejo, hibisco). À noite, noite de São João.
//! Vem por último no `<style>`: sobrepõe o resto.

pub const CSS: &str = r#"
:root {
  --kraft:#E9DDC0; --papel:#F3EAD3; --cartao:#FBF4E2; --tinta:#17130F;
  --sol:#FFC21A; --urucum:#D8402A; --bananeira:#2E8B4E; --azulejo:#1D5FA8; --hibisco:#E0457B;
  --texto:#17130F; --texto-2:#4B3F33; --texto-3:#7A6A58;
  --neon:#FFC21A; --sobre-neon:#17130F; --melhora:#2E8B4E; --piora:#D8402A;
  --musgo:#2E8B4E; --folha:#2E8B4E;
  --vidro:#F3EAD3; --vidro-borda:#17130F; --trilho:rgba(23,19,15,.12);
  --sombra:5px 5px 0 #17130F; --sombra-cartao:4px 4px 0 #17130F;
  --corte:16px 22px 14px 24px / 22px 14px 24px 16px;
  --corte-2:22px 14px 20px 16px / 14px 22px 16px 20px;
}
body.noite {
  --kraft:#2A2117; --papel:#231B13; --cartao:#2E241A; --tinta:#0B0806;
  --texto:#F3EAD3; --texto-2:#D2C2A3; --texto-3:#A8977A; --vidro:#231B13;
  --vidro-borda:#0B0806; --trilho:rgba(243,234,211,.12); --neon:#FFC21A;
}
/* Fundo: kraft com azulejo (no lugar da foto do bioma). */
body, body.noite {
  background-color:var(--kraft) !important;
  background-image:url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='120' height='120'%3E%3Cg fill='none' stroke='%231D5FA8' stroke-width='2' opacity='.22'%3E%3Crect x='1' y='1' width='118' height='118'/%3E%3Ccircle cx='60' cy='60' r='26'/%3E%3Cpath d='M60 34 Q72 60 60 86 Q48 60 60 34Z M34 60 Q60 48 86 60 Q60 72 34 60Z'/%3E%3Cpath d='M1 1 Q20 20 1 40 M119 1 Q100 20 119 40 M1 119 Q20 100 1 80 M119 119 Q100 100 119 80'/%3E%3Ccircle cx='60' cy='60' r='5' fill='%231D5FA8'/%3E%3C/g%3E%3C/svg%3E") !important;
  background-size:120px 120px !important; background-attachment:fixed !important;
  font-family:"Bitter", Georgia, serif; color:var(--texto);
}
body.noite { background-blend-mode:normal !important; }
body.madrugada { filter:saturate(.85); }
body.manha .conteudo.vidro { background:linear-gradient(180deg, #FFF3D6, var(--papel) 240px); }
body.tarde .conteudo.vidro { background:linear-gradient(180deg, #FBD9A8, var(--papel) 260px); }
.atmosfera, .nevoa { display:none !important; }

/* Painéis em papel com traço de tinta e sombra de carimbo; cantos de corte de madeira. */
.vidro, .kpi, .caixa, .card, .regra, .corte, .hunt-card, .meta-card, .adv-card, .time-meta,
.oferta-pvp, .boss, .mini, .veredito, .elo-faixa, .tabuleiro-pvp, .tabela-caixa {
  backdrop-filter:none !important; -webkit-backdrop-filter:none !important;
}
.vidro { background:var(--vidro); border:3px solid var(--tinta); box-shadow:var(--sombra); }
.conteudo.vidro { background:var(--papel); border-radius:26px; position:relative; }
.conteudo.vidro::before { display:none; }
body.noite .conteudo.vidro { background:var(--papel); }
/* À noite a faixa vira linogravura em negativo: traço claro sobre o papel escuro. */
body.noite .faixa-mutirao { --tinta:#D9CBA8; }
body.noite .faixa-mutirao .camada.tinta [class*="c-"] { fill:#2A2117; }

/* Menu: preto de tinta, item ativo em sol. */
.barra.vidro { background:var(--tinta); color:#E9DDC0; border-radius:var(--corte); overflow:hidden; position:relative; }
.barra .logo { font-family:"Rubik Dirt", "Alfa Slab One", serif; font-size:30px; color:var(--sol); letter-spacing:.02em; }
.barra .logo svg { color:var(--bananeira); }
.barra .grupo { color:rgba(233,221,192,.55); font-family:"Alfa Slab One", serif; font-weight:400; letter-spacing:.14em; }
.barra .item { color:#E9DDC0; border-radius:12px 18px 10px 16px; font-family:"Bitter", serif; font-weight:600; }
.barra .item:hover { background:rgba(233,221,192,.1); }
.barra .item.ativo { background:var(--sol); color:var(--tinta); box-shadow:3px 3px 0 var(--urucum); animation:none; }
.barra::after { content:""; position:absolute; left:0; right:0; bottom:0; height:120px; pointer-events:none; opacity:.9;
  background:url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='224' height='120'%3E%3Cg stroke='%2317130F' stroke-width='2'%3E%3Cpath d='M0 120 V70 Q30 40 60 70 Q90 30 120 64 Q150 36 180 66 Q205 46 224 62 V120Z' fill='%232E8B4E'/%3E%3Ccircle cx='40' cy='60' r='9' fill='%23FFC21A'/%3E%3Ccircle cx='150' cy='52' r='7' fill='%23E0457B'/%3E%3Cpath d='M100 120 V84 h24 v36' fill='%23D8402A'/%3E%3C/g%3E%3C/svg%3E") bottom/224px 120px no-repeat; }
@media (max-height: 820px) { .barra::after { display:none; } }
.barra .rodape-conta { background:var(--papel); color:var(--texto); border:2px solid var(--kraft); border-radius:var(--corte-2); position:relative; z-index:1; }

/* Topo: pílulas de papel carimbadas. */
.pilula.vidro { background:var(--cartao); border:2.5px solid var(--tinta); box-shadow:3px 3px 0 var(--tinta); border-radius:999px; }
.pilula.vidro:active, button.btn:active { transform:translate(2px, 2px); box-shadow:1px 1px 0 var(--tinta); }

/* Tipografia de cartaz. */
.resumo h2 { font-family:"Rubik Dirt", "Alfa Slab One", serif; font-weight:400; font-size:44px; line-height:1; text-transform:none; letter-spacing:.01em; color:var(--tinta); }
body.noite .resumo h2 { color:var(--sol); }
.pergunta { font-family:"Bitter", serif; font-style:italic; color:var(--texto-2); font-size:16px; }
.secao { font-family:"Alfa Slab One", serif; font-weight:400; text-transform:uppercase; letter-spacing:.08em; font-size:16px; color:var(--tinta); margin:28px 0 12px; display:flex; align-items:center; gap:12px; }
body.noite .secao { color:var(--texto); }
.secao::after { content:""; flex:1; height:10px; background:radial-gradient(circle at 5px 0, transparent 4px, var(--tinta) 4.5px, var(--tinta) 5.5px, transparent 6px) 0 0/10px 10px repeat-x; opacity:.7; }
.caixa-t { font-family:"Alfa Slab One", serif; font-weight:400; text-transform:uppercase; letter-spacing:.06em; font-size:14px; color:var(--tinta); display:flex; align-items:center; gap:8px; }
body.noite .caixa-t { color:var(--texto); }
.caixa-t::before { content:""; width:16px; height:16px; border-radius:50%; background:var(--sol); border:2px solid var(--tinta); box-shadow:none; transform:none;
  -webkit-mask:none; }
.kpi-t { font-family:"Alfa Slab One", serif; font-weight:400; color:var(--texto-2); }
.kpi-v, .elo-num b, .corte-v, .hunt-nums b, .op-preco { font-family:"Space Grotesk", "Bitter", sans-serif; }
.dica { color:var(--texto-3); }

/* Cartões: papel, traço grosso, sombra de carimbo. */
.kpi, .caixa, .card, .regra, .corte, .hunt-card, .meta-card, .adv-card, .time-meta, .oferta-pvp, .boss, .mini, .elo-faixa {
  background:var(--cartao); border:2.5px solid var(--tinta); box-shadow:var(--sombra-cartao); border-radius:var(--corte); min-width:0; overflow-wrap:anywhere;
}
.kpi:nth-child(even), .hunt-card:nth-child(even), .boss:nth-child(even), .meta-card:nth-child(even) { border-radius:var(--corte-2); }
body.noite .kpi, body.noite .caixa { background:var(--cartao); box-shadow:4px 4px 0 #000; }
.kpi::after { left:0; right:auto; top:0; bottom:0; width:8px; height:auto; border-radius:0; background:var(--sol); border-right:2.5px solid var(--tinta); opacity:1; }
.kpi.bom::after { background:var(--bananeira); }
.kpi.ruim::after { background:var(--urucum); }
.kpi { padding-left:24px; }
.kpi-ico { background:var(--sol); border:2px solid var(--tinta); color:var(--tinta); }
.kpi .comp { background:transparent; border:1.5px dashed var(--texto-3); }
.kpi .comp.melhora { border-color:var(--bananeira); background:transparent; }
.kpi .comp.piora { border-color:var(--urucum); background:transparent; }
.kpi:hover, .hunt-card:hover, .boss:hover, .meta-card:hover, .oferta-pvp:hover { transform:translate(-2px, -2px); box-shadow:6px 6px 0 var(--tinta); }
.caixa { padding:20px 22px; position:relative; overflow:hidden; }
.caixa::before { display:none; }
.veredito { background:var(--cartao); border:2.5px solid var(--tinta); border-left:10px solid var(--bananeira); box-shadow:3px 3px 0 var(--tinta); border-radius:var(--corte-2); }
.veredito.alerta, .veredito[class*="alerta"] { border-left-color:var(--urucum); }

/* Abas como bandeirinhas de São João. */
.seg { background:transparent; border:0; padding:0 0 10px; gap:4px; flex-wrap:wrap; }
.seg button { border:2.5px solid var(--tinta); background:var(--cartao); color:var(--texto); border-radius:0; clip-path:polygon(0 0, 100% 0, 100% 78%, 50% 100%, 0 78%); padding:8px 16px 16px; font-family:"Alfa Slab One", serif; font-weight:400; letter-spacing:.04em; }
.seg button:nth-child(5n+1).ativo { background:var(--urucum); color:#fff; }
.seg button:nth-child(5n+2).ativo { background:var(--sol); color:var(--tinta); }
.seg button:nth-child(5n+3).ativo { background:var(--bananeira); color:#fff; }
.seg button:nth-child(5n+4).ativo { background:var(--azulejo); color:#fff; }
.seg button:nth-child(5n).ativo { background:var(--hibisco); color:#fff; }
.seg button:not(.ativo):hover { background:var(--kraft); }
body.noite .seg button.ativo:nth-child(5n+2) { color:var(--tinta); }

/* Botões carimbados e campos. */
button.btn { background:var(--cartao); border:2.5px solid var(--tinta); color:var(--tinta); box-shadow:3px 3px 0 var(--tinta); font-family:"Bitter", serif; font-weight:700; border-radius:14px 20px 12px 18px; }
button.btn:hover { background:var(--kraft); }
button.btn.prim, body.noite button.btn.prim { background:var(--urucum); border-color:var(--tinta); color:#fff; }
body.noite button.btn { background:var(--cartao); color:var(--texto); }
input, select.caixa, body.noite input, body.noite select.caixa { background:var(--cartao); border:2.5px solid var(--tinta); color:var(--texto); border-radius:14px 20px 12px 18px; }
:focus-visible { outline:3px solid var(--azulejo); outline-offset:3px; }

/* Tabelas como tabela de feira. */
.tabela-caixa { border:2.5px solid var(--tinta); border-radius:var(--corte-2); background:var(--cartao); box-shadow:3px 3px 0 var(--tinta); }
.tabela th, body.noite .tabela th { background:var(--tinta); color:#E9DDC0; font-family:"Alfa Slab One", serif; font-weight:400; letter-spacing:.06em; }
.tabela td { border-bottom:1.5px dashed rgba(23,19,15,.25); }
.tabela tbody tr:nth-child(even) td { background:rgba(255,194,26,.06); }
.tabela tbody tr:hover td { background:rgba(255,194,26,.22); }

/* Gráficos e medidores como blocos de xilogravura. */
.col-val { background:repeating-linear-gradient(45deg, var(--tinta) 0 2px, var(--cartao) 2px 6px); border:2px solid var(--tinta); border-radius:6px 6px 2px 2px; }
.col.hoje .col-val { background:var(--sol); }
.trilho, .trilho-corte, .barra-taxa, .faixa-ouro, .b-trilho { background:var(--kraft); border:1.5px solid var(--tinta); }
.trilho .val, .barra-l.ativo .b-val { background:var(--bananeira); }
.b-val { background:var(--azulejo); }
.trilho-corte span { background:repeating-linear-gradient(90deg, var(--urucum) 0 12px, var(--sol) 12px 24px); }
.barra-taxa span { background:var(--urucum); }
.barra-taxa.bom span { background:var(--bananeira); }
.pilula-neon { background:var(--sol); color:var(--tinta); border:2px solid var(--tinta); }
.chip { background:var(--kraft); border:1.5px solid var(--tinta); color:var(--texto); }
.selo { border:1.5px solid var(--tinta); }
.selo.ok { background:var(--bananeira); color:#fff; }
.selo.nao { background:var(--urucum); color:#fff; }

/* PvP e treino. */
.tabuleiro-pvp { background:var(--cartao); border:2.5px solid var(--tinta); box-shadow:3px 3px 0 var(--tinta); }
.casa-pvp { background:var(--kraft); border:1.5px dashed var(--tinta); }
.casa-pvp.ocupada { background:var(--sol); border:2px solid var(--tinta); box-shadow:2px 2px 0 var(--tinta); }
.casa-pvp .balao { background:var(--tinta); color:#E9DDC0; border:2px solid var(--sol); }
.casa-pvp .balao b { color:var(--sol); }
.tabuleiro-pvp.mini { width:100%; max-width:260px; grid-template-columns:repeat(8, minmax(0, 1fr)); grid-template-rows:none; grid-auto-rows:auto; }
.tabuleiro-pvp.mini .casa-pvp { aspect-ratio:1; }
.sprites, .adv-topo { flex-wrap:wrap; }
.elo-faixa { background:var(--cartao); }
.elo-selo { border:3px solid var(--tinta); }
.janela.vidro { background:var(--papel); border-radius:var(--corte); }
.aviso-ok { background:var(--cartao); border:2px solid var(--bananeira); }
.hunt-card.atual { box-shadow:4px 4px 0 var(--sol), 4px 4px 0 2.5px var(--tinta); }
.hunt-card.melhor { box-shadow:4px 4px 0 var(--bananeira); }

/* Vazio: o pessoal descansando na rede. */
.vazio { padding:96px 12px 18px; text-align:center; color:var(--texto-2); font-style:italic;
  background:url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='200' height='86'%3E%3Cg stroke='%2317130F' stroke-width='3' stroke-linecap='round' fill='none'%3E%3Cpath d='M20 84 V10 M180 84 V10'/%3E%3Cpath d='M20 22 Q100 74 180 22' stroke-width='2'/%3E%3Cpath d='M30 30 Q100 86 170 30 Q100 60 30 30Z' fill='%23D8402A'/%3E%3Ccircle cx='76' cy='40' r='8' fill='%23E9DDC0'/%3E%3Cpath d='M84 44 Q110 56 140 42' stroke-width='5' stroke='%231D5FA8'/%3E%3Cpath d='M8 10 h24 M168 10 h24'/%3E%3C/g%3E%3C/svg%3E") center 8px/200px 86px no-repeat; }

/* Título que brota: letras nascem do chão, com uma rama crescendo embaixo. */
.brota { display:inline-block; position:relative; }
.brota .letra { display:inline-block; animation:brotar .55s cubic-bezier(.2,1.4,.4,1) both; animation-delay:calc(var(--i) * 32ms); transform-origin:50% 100%; }
@keyframes brotar { from { opacity:0; transform:translateY(40%) scale(.2) rotate(-8deg); } to { opacity:1; transform:none; } }
.brota svg.rama { position:absolute; left:0; bottom:-14px; width:100%; height:18px; overflow:visible; }
.brota svg.rama path { fill:none; stroke:var(--bananeira); stroke-width:3; stroke-linecap:round; stroke-dasharray:600; stroke-dashoffset:600; animation:rama 1.1s .25s ease-out forwards; }
.brota svg.rama .folhinha { fill:var(--bananeira); stroke:var(--tinta); stroke-width:1.5; stroke-dasharray:none; stroke-dashoffset:0; transform-box:fill-box; transform-origin:0 100%; animation:folhinha .5s ease-out both; animation-delay:calc(.6s + var(--i) * 90ms); }
@keyframes rama { to { stroke-dashoffset:0; } }
@keyframes folhinha { from { transform:scale(0); } to { transform:scale(1); } }
body.leve .brota .letra, body.leve .brota svg.rama path, body.leve .brota svg.rama .folhinha { animation:none !important; opacity:1; stroke-dashoffset:0; transform:none; }
"#;
