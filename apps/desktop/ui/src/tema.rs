//! Tema "estufa solarpunk" (substitui o vidro): papel creme sólido sobre a paisagem do bioma,
//! armação verde-musgo no menu, títulos editoriais (Fraunces), acentos de folha e sol. À noite,
//! floresta escura com brilho bioluminescente. Vem por último no `<style>`: sobrepõe o resto.

pub const CSS: &str = r#"
:root {
  --papel:#FBF6EA; --cartao:#FFFDF7; --texto:#1E2B23; --texto-2:#4A5A4F; --texto-3:#7C8B80;
  --musgo:#2F6B45; --musgo-2:#24462F; --folha:#7BC043; --sol:#F4B740; --terra:#D4683F; --ceu:#4D9CC4;
  --neon:#A8D85A; --sobre-neon:#1E2B23; --melhora:#2E8B57; --piora:#D4683F;
  --vidro:rgba(251,246,234,.97); --vidro-borda:rgba(47,107,69,.16); --trilho:rgba(47,107,69,.12);
  --sombra:0 18px 40px rgba(44,62,40,.18); --raio:24px;
  --sombra-cartao:0 1px 0 rgba(47,107,69,.06), 0 10px 26px rgba(44,62,40,.08);
}
body.noite {
  --papel:#12201A; --cartao:#182A21; --texto:#E6EFE4; --texto-2:#AFC2B3; --texto-3:#81968A;
  --musgo:#5BE0B5; --musgo-2:#0E1A14; --neon:#5BE0B5; --sobre-neon:#0E1A14;
  --vidro:rgba(18,32,26,.96); --vidro-borda:rgba(160,220,170,.14); --trilho:rgba(160,220,170,.12);
  --sombra-cartao:0 1px 0 rgba(0,0,0,.2), 0 10px 26px rgba(0,0,0,.35);
}
/* Sem vidro: nada de desfoque nem transparência nos painéis. */
.vidro, .kpi, .caixa, .card, .regra, .corte, .hunt-card, .meta-card, .adv-card, .time-meta,
.oferta-pvp, .boss, .mini, .veredito, .elo-faixa, .tabuleiro-pvp {
  backdrop-filter:none !important; -webkit-backdrop-filter:none !important;
}
.vidro { background:var(--vidro); border:1px solid var(--vidro-borda); box-shadow:var(--sombra); }

/* Armação da estufa: o menu em verde-musgo. */
.barra.vidro { background:linear-gradient(180deg, var(--musgo-2), #1B3A26); border:0; color:#F3EEDC; box-shadow:var(--sombra); position:relative; overflow:hidden; }
body.noite .barra.vidro { background:linear-gradient(180deg, #0E1A14, #0A140F); box-shadow:0 0 0 1px rgba(91,224,181,.12), var(--sombra); }
.barra .logo { color:#F3EEDC; font-family:"Fraunces", Georgia, serif; font-weight:700; letter-spacing:-.01em; }
.barra .logo svg { color:var(--folha); }
.barra .grupo { color:rgba(243,238,220,.55); }
.barra .item { color:#F3EEDC; }
.barra .item:hover { background:rgba(255,255,255,.08); }
.barra .item.ativo { background:var(--neon); color:var(--sobre-neon); }
.barra::after { content:""; position:absolute; left:0; right:0; bottom:0; height:180px; pointer-events:none; opacity:.22;
  background:url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='240' height='180'%3E%3Cg fill='none' stroke='%23A8D85A' stroke-width='2' stroke-linecap='round'%3E%3Cpath d='M20 180 C30 120 70 110 60 40'/%3E%3Cpath d='M60 40 c-20 -6 -30 -22 -24 -36 c18 4 28 18 24 36z' fill='%23A8D85A' fill-opacity='.5'/%3E%3Cpath d='M45 110 c-22 2 -36 -10 -38 -24 c18 -2 34 8 38 24z' fill='%23A8D85A' fill-opacity='.4'/%3E%3Cpath d='M200 180 C190 130 150 120 170 60'/%3E%3Cpath d='M170 60 c18 -10 24 -26 16 -38 c-16 6 -24 22 -16 38z' fill='%23A8D85A' fill-opacity='.5'/%3E%3Cpath d='M182 130 c20 -4 32 -18 32 -32 c-18 2 -30 14 -32 32z' fill='%23A8D85A' fill-opacity='.4'/%3E%3C/g%3E%3C/svg%3E") bottom center/240px 180px no-repeat; }
.barra .rodape-conta { background:rgba(255,255,255,.07); border-color:rgba(255,255,255,.1); color:#F3EEDC; position:relative; z-index:1; }

/* Barra do topo e conteúdo em papel. */
.pilula.vidro { background:var(--cartao); box-shadow:var(--sombra-cartao); }
.pilula.vidro select, button.pilula { color:var(--texto); }
.conteudo.vidro { background:var(--papel); border:1px solid var(--vidro-borda); box-shadow:var(--sombra); border-radius:28px; position:relative; }
.conteudo.vidro::before { content:""; position:sticky; top:-24px; display:block; height:4px; margin:-24px -28px 20px; border-radius:28px 28px 0 0; background:linear-gradient(90deg, var(--musgo), var(--folha) 45%, var(--sol)); z-index:2; }

/* Tipografia: títulos editoriais, números tabulares. */
h2, .resumo h2 { font-family:"Fraunces", Georgia, serif; font-weight:700; font-size:30px; letter-spacing:-.015em; text-transform:none; margin-bottom:6px; }
.pergunta { font-style:italic; color:var(--texto-2); font-size:15px; margin:0 0 20px; }
.secao { font-family:"Fraunces", Georgia, serif; text-transform:none; letter-spacing:0; font-size:19px; font-weight:600; color:var(--texto); margin:26px 0 12px; display:flex; align-items:center; gap:10px; }
.secao::after { content:""; flex:1; height:1px; background:linear-gradient(90deg, var(--vidro-borda), transparent); }
.caixa-t { font-family:"Fraunces", Georgia, serif; text-transform:none; letter-spacing:0; font-size:16px; font-weight:600; color:var(--texto); }
.caixa-t::before { width:14px; height:14px; border-radius:0 70% 0 70%; background:linear-gradient(135deg, var(--folha), var(--musgo)); box-shadow:none; transform:rotate(-10deg); }

/* Cartões: papel claro, borda musgo e uma linha de folha a sol no topo. */
.kpi, .caixa, .card, .regra, .corte, .hunt-card, .meta-card, .adv-card, .time-meta, .oferta-pvp, .boss, .mini, .elo-faixa {
  background:var(--cartao); border:1px solid var(--vidro-borda); box-shadow:var(--sombra-cartao); min-width:0; overflow-wrap:anywhere;
}
body.noite .kpi, body.noite .caixa { background:var(--cartao); box-shadow:var(--sombra-cartao); }
.kpi { border-radius:20px; padding:16px 18px 14px; }
.kpi::after { left:0; right:auto; top:0; bottom:0; width:4px; height:auto; border-radius:20px 0 0 20px; background:var(--folha); opacity:1; }
.kpi.bom::after { background:var(--melhora); }
.kpi.ruim::after { background:var(--piora); }
.kpi-ico { background:color-mix(in srgb, var(--folha) 22%, transparent); color:var(--musgo); }
.kpi-v { font-family:"Space Grotesk", Inter, sans-serif; letter-spacing:-.02em; }
.kpi .comp { background:color-mix(in srgb, var(--musgo) 8%, transparent); }
.kpi:hover, .hunt-card:hover, .boss:hover, .meta-card:hover, .oferta-pvp:hover { transform:translateY(-2px); box-shadow:0 1px 0 rgba(47,107,69,.08), 0 16px 34px rgba(44,62,40,.14); }
.caixa { border-radius:22px; padding:20px 22px; position:relative; overflow:hidden; }
.caixa::before { content:""; position:absolute; top:0; left:22px; right:22px; height:3px; border-radius:0 0 3px 3px; background:linear-gradient(90deg, var(--folha), var(--sol)); opacity:.7; }
.veredito { background:color-mix(in srgb, var(--folha) 10%, var(--cartao)); border-left-color:var(--folha); }
.veredito.alerta, .veredito[class*="alerta"] { border-left-color:var(--terra); background:color-mix(in srgb, var(--terra) 9%, var(--cartao)); }

/* Abas, botões e campos. */
.seg { background:color-mix(in srgb, var(--musgo) 7%, var(--cartao)); border-color:var(--vidro-borda); }
.seg button.ativo { background:var(--musgo); color:#F7F3E6; }
body.noite .seg button.ativo { color:#0E1A14; }
button.btn { background:var(--cartao); border-color:var(--vidro-borda); color:var(--texto); }
button.btn:hover { border-color:var(--musgo); }
button.btn.prim { background:var(--musgo); border-color:var(--musgo); color:#F7F3E6; }
body.noite button.btn.prim { background:var(--musgo); border-color:var(--musgo); color:#0E1A14; }
input, select.caixa { background:var(--cartao); border-color:var(--vidro-borda); color:var(--texto); }
:focus-visible { outline:2px solid var(--sol); outline-offset:2px; }

/* Tabelas: papel, cabeçalho musgo, linha destacada ao passar o mouse. */
.tabela th { background:color-mix(in srgb, var(--musgo) 9%, var(--cartao)); color:var(--texto-2); }
.tabela tbody tr:nth-child(even) td { background:color-mix(in srgb, var(--musgo) 3%, transparent); }
.tabela tbody tr:hover td { background:color-mix(in srgb, var(--folha) 14%, transparent); }

/* Gráficos e medidores nas cores do tema. */
.col.hoje .col-val, .trilho .val, .barra-l.ativo .b-val { background:linear-gradient(180deg, var(--folha), var(--musgo)); }
.col-val { background:color-mix(in srgb, var(--musgo) 22%, var(--cartao)); }
.trilho-corte span { background:linear-gradient(90deg, var(--musgo), var(--folha), var(--sol)); }
.pilula-neon { background:var(--sol); color:#1E2B23; }
.selo.ok { background:color-mix(in srgb, var(--melhora) 16%, transparent); color:var(--melhora); }
.selo.nao { background:color-mix(in srgb, var(--terra) 16%, transparent); color:var(--terra); }

/* PvP e treino: o tabuleiro em papel; o pequeno encolhe para caber no cartão. */
.tabuleiro-pvp { background:radial-gradient(ellipse at 50% 0%, color-mix(in srgb, var(--folha) 22%, transparent), transparent 70%), var(--cartao); }
.casa-pvp { background:color-mix(in srgb, var(--musgo) 5%, var(--cartao)); border-color:var(--vidro-borda); }
.casa-pvp.ocupada { background:color-mix(in srgb, var(--folha) 26%, var(--cartao)); border-color:var(--folha); box-shadow:0 0 12px color-mix(in srgb, var(--folha) 40%, transparent); }
.tabuleiro-pvp.mini { width:100%; max-width:260px; grid-template-columns:repeat(8, minmax(0, 1fr)); grid-template-rows:none; grid-auto-rows:auto; }
.tabuleiro-pvp.mini .casa-pvp { aspect-ratio:1; }
.sprites { flex-wrap:wrap; }
.adv-topo { flex-wrap:wrap; }
.adv-card, .time-meta { overflow:hidden; }
.elo-faixa { background:linear-gradient(110deg, color-mix(in srgb, var(--tier) 26%, var(--cartao)), var(--cartao) 60%); }
.janela.vidro { background:var(--papel); }
.aviso-ok { background:color-mix(in srgb, var(--melhora) 14%, var(--cartao)); }

/* A paisagem continua viva em volta: nevoa mais leve. */
.nevoa { background:linear-gradient(180deg, rgba(255,250,235,.10), rgba(255,250,235,0)); }
body.noite .nevoa { background:linear-gradient(180deg, rgba(8,16,12,.35), rgba(8,16,12,.15)); }
"#;
