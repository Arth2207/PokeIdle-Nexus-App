//! Tema "Diário de expedição" (prévia): o painel é o caderno de campo de um explorador aberto
//! sobre o wallpaper do bioma. Páginas pautadas presas com fita, fichas escritas à mão, pokémon
//! em polaroid, carimbo da tela, gráficos a lápis (traço tremido de verdade), capa de couro no
//! menu e marca-página na tela ativa. Só vale com `body.tema-diario` (ligado em `/previa/`).

/// Filtro SVG do traço a lápis (os gráficos usam `filter:url(#rabisco)`).
pub const FILTRO: &str = r#"<svg width="0" height="0" style="position:absolute" aria-hidden="true"><defs>
<filter id="rabisco"><feTurbulence type="fractalNoise" baseFrequency="0.03" numOctaves="2" seed="7" result="r"/><feDisplacementMap in="SourceGraphic" in2="r" scale="2.6"/></filter>
</defs></svg>"#;

pub const CSS: &str = r#"
body.tema-diario {
  --papel:#FBF7EE; --papel-2:#F4EEDF; --tinta:#2B2A33; --grafite:#4A4A52; --pauta:rgba(70,120,190,.16);
  --margem:rgba(214,64,64,.35); --fita-1:rgba(244,183,64,.78); --fita-2:rgba(116,190,160,.78);
  --fita-3:rgba(231,120,140,.75); --fita-4:rgba(120,160,220,.78); --carimbo:#B8352E;
  --couro:#5A3A26; --couro-2:#3E2718;
  --texto:#2B2A33; --texto-2:#55525C; --texto-3:#7D7884;
  --neon:#F4B740; --sobre-neon:#2B2A33; --melhora:#2F7D4F; --piora:#B8352E;
  --vidro-borda:rgba(43,42,51,.14); --trilho:rgba(43,42,51,.10);
}
/* O wallpaper fica inteiro: sem o painel grande, as páginas ficam soltas na paisagem. */
body.tema-diario .conteudo.vidro { background:transparent !important; border:0 !important; box-shadow:none !important; padding:18px 22px 40px; }
body.tema-diario .conteudo.vidro::before { display:none; }

/* Capa de couro costurada no menu, fita marca-página na tela ativa. */
body.tema-diario .barra.vidro { background:radial-gradient(ellipse at 30% 20%, #6E4930, var(--couro) 55%, var(--couro-2)) !important; border:0 !important; border-radius:18px 10px 10px 18px; box-shadow:inset 0 0 0 7px rgba(0,0,0,.08), inset 0 0 0 9px rgba(244,224,190,.35), 6px 10px 26px rgba(0,0,0,.35) !important; color:#F4E6CC; position:relative; }
body.tema-diario .barra.vidro::before { content:""; position:absolute; inset:12px; border:2px dashed rgba(244,224,190,.45); border-radius:12px 6px 6px 12px; pointer-events:none; }
body.tema-diario .barra::after { display:none; }
body.tema-diario .barra .logo { font-family:"Caveat", cursive; font-weight:700; font-size:40px; color:#F7E7C4; letter-spacing:0; }
body.tema-diario .barra .logo svg { color:#9FD27A; }
body.tema-diario .barra .grupo { font-family:"Special Elite", monospace; color:rgba(244,230,204,.6); letter-spacing:.12em; }
body.tema-diario .barra .item { color:#F4E6CC; font-family:"Caveat", cursive; font-size:22px; font-weight:500; border-radius:0; position:relative; padding:4px 14px; }
body.tema-diario .barra .item:hover { background:rgba(255,255,255,.07); }
body.tema-diario .barra .item.ativo { background:#C4413A; color:#FFF6E6; box-shadow:2px 3px 0 rgba(0,0,0,.25); clip-path:polygon(0 0, 100% 0, 92% 50%, 100% 100%, 0 100%); animation:none; margin-right:-26px; padding-right:40px; }
body.tema-diario .barra .rodape-conta { background:#F4E6CC; color:var(--tinta); border:0; border-radius:3px; transform:rotate(-1.5deg); box-shadow:2px 4px 10px rgba(0,0,0,.3); font-family:"Special Elite", monospace; }

/* Topo: etiquetas de papel amarradas por barbante. */
body.tema-diario .pilula.vidro { background:var(--papel) !important; border:0 !important; border-radius:4px 14px 14px 4px !important; box-shadow:0 4px 10px rgba(0,0,0,.22) !important; position:relative; padding-left:28px; font-family:"Special Elite", monospace; }
body.tema-diario .pilula.vidro::before { content:""; position:absolute; left:10px; top:50%; width:8px; height:8px; margin-top:-4px; border-radius:50%; background:#C9BFA8; box-shadow:inset 0 0 0 2px rgba(0,0,0,.25); }
body.tema-diario .pilula.vidro select { font-family:"Special Elite", monospace; }

/* Título escrito à mão com rabisco embaixo; carimbo da tela no canto. */
body.tema-diario .resumo { position:relative; background:var(--papel); padding:14px 22px 12px; border-radius:2px; box-shadow:0 10px 24px rgba(0,0,0,.25); transform:rotate(-.6deg); margin-bottom:18px; }
body.tema-diario .resumo::before { content:""; position:absolute; top:-12px; left:40px; width:120px; height:26px; background:var(--fita-1); transform:rotate(-4deg); box-shadow:0 1px 2px rgba(0,0,0,.12); }
body.tema-diario .resumo h2 { font-family:"Caveat", cursive; font-weight:700; font-size:54px; letter-spacing:0; text-transform:none; line-height:1; margin:0; color:var(--tinta);
  background:url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='300' height='14'%3E%3Cpath d='M2 9 C40 3 70 12 110 7 S190 2 230 8 S280 10 298 5' fill='none' stroke='%23C4413A' stroke-width='3' stroke-linecap='round'/%3E%3C/svg%3E") left bottom/100% 12px no-repeat; padding-bottom:10px; }
body.tema-diario .carimbo { display:inline-flex; position:absolute; right:150px; top:10px; transform:rotate(-8deg); border:3px double var(--carimbo); color:var(--carimbo); font-family:"Special Elite", monospace; font-size:12px; letter-spacing:.14em; text-transform:uppercase; padding:6px 12px; border-radius:6px; opacity:.78; mix-blend-mode:multiply; }
body.tema-diario .pergunta { font-family:"Caveat", cursive; font-size:24px; font-style:normal; color:#F8F2E2; text-shadow:0 2px 6px rgba(0,0,0,.6); margin:0 0 20px 8px; }

/* Páginas: papel pautado, fita no topo, cada uma um pouco torta. */
body.tema-diario .caixa, body.tema-diario .regra, body.tema-diario .corte, body.tema-diario .hunt-card, body.tema-diario .adv-card, body.tema-diario .time-meta, body.tema-diario .oferta-pvp, body.tema-diario .meta-card, body.tema-diario .elo-faixa, body.tema-diario .tabela-caixa {
  background:linear-gradient(90deg, transparent 38px, var(--margem) 38px, var(--margem) 40px, transparent 40px), repeating-linear-gradient(180deg, var(--papel) 0 27px, var(--pauta) 27px 28px) !important;
  border:0 !important; border-radius:2px !important; box-shadow:0 10px 22px rgba(0,0,0,.28) !important; backdrop-filter:none !important; -webkit-backdrop-filter:none !important; position:relative; overflow:visible !important;
}
body.tema-diario .caixa { padding:22px 22px 18px 52px; }
body.tema-diario .caixa::before, body.tema-diario .corte::before, body.tema-diario .hunt-card::before, body.tema-diario .adv-card::before, body.tema-diario .oferta-pvp::before {
  content:""; position:absolute; top:-11px; left:50%; width:110px; height:24px; margin-left:-55px; background:var(--fita-2); transform:rotate(2deg); box-shadow:0 1px 2px rgba(0,0,0,.15); opacity:1; border-radius:0;
  -webkit-mask:linear-gradient(90deg, transparent 0 3px, #000 3px calc(100% - 3px), transparent calc(100% - 3px)); }
body.tema-diario .caixa:nth-of-type(3n+1)::before { background:var(--fita-1); transform:rotate(-3deg); }
body.tema-diario .caixa:nth-of-type(3n+2)::before { background:var(--fita-3); transform:rotate(1deg); }
body.tema-diario .caixa:nth-of-type(3n)::before { background:var(--fita-4); transform:rotate(-1deg); }
body.tema-diario .caixa:nth-of-type(odd) { transform:rotate(-.35deg); }
body.tema-diario .caixa:nth-of-type(even) { transform:rotate(.4deg); }
body.tema-diario .caixa-t { font-family:"Special Elite", monospace; font-weight:400; text-transform:uppercase; letter-spacing:.1em; font-size:13px; color:var(--tinta); }
body.tema-diario .caixa-t::before { content:"✎"; width:auto; height:auto; background:none; box-shadow:none; transform:none; border-radius:0; color:var(--carimbo); font-size:15px; }
body.tema-diario .secao { font-family:"Caveat", cursive; font-weight:700; font-size:30px; text-transform:none; letter-spacing:0; color:#FFF8E6; text-shadow:0 2px 8px rgba(0,0,0,.65); margin:28px 0 10px 6px; }
body.tema-diario .secao::after { display:none; }
body.tema-diario .dica { font-family:"Caveat", cursive; font-size:18px; color:var(--texto-2); }
body.tema-diario .conteudo > .dica, body.tema-diario .conteudo > div > .dica { color:#FBF4E2; text-shadow:0 1px 4px rgba(0,0,0,.7); }

/* Fichas: o número escrito à mão, presas por um clipe. */
body.tema-diario .kpi { background:var(--papel) !important; border:0 !important; border-radius:2px !important; box-shadow:0 8px 18px rgba(0,0,0,.26) !important; padding:18px 18px 14px !important; position:relative; overflow:visible !important; }
body.tema-diario .kpi::after { content:""; position:absolute; top:-14px; right:22px; left:auto; bottom:auto; width:14px; height:40px; border:3px solid #9AA0A8; border-radius:8px; background:transparent; opacity:1; }
body.tema-diario .kpi:nth-child(odd) { transform:rotate(-1deg); }
body.tema-diario .kpi:nth-child(even) { transform:rotate(.8deg); }
body.tema-diario .kpi:hover { transform:rotate(0) translateY(-3px); box-shadow:0 14px 26px rgba(0,0,0,.3) !important; }
body.tema-diario .kpi-t { font-family:"Special Elite", monospace; font-weight:400; letter-spacing:.12em; color:var(--texto-2); }
body.tema-diario .kpi-ico { display:none; }
body.tema-diario .kpi-v { font-family:"Caveat", cursive; font-weight:700; font-size:50px; line-height:1; color:var(--tinta); letter-spacing:0; }
body.tema-diario .kpi-v .un { font-family:"Caveat", cursive; font-size:22px; }
body.tema-diario .kpi .comp { font-family:"Caveat", cursive; font-size:19px; background:transparent !important; border:0 !important; padding:0; }
body.tema-diario .kpi .comp.melhora { color:var(--melhora); text-decoration:underline wavy rgba(47,125,79,.5); }
body.tema-diario .kpi .comp.piora { color:var(--piora); text-decoration:underline wavy rgba(184,53,46,.5); }

/* Pokémon em polaroid: endireita e levanta ao passar o mouse. */
body.tema-diario .scan-img, body.tema-diario .boss img, body.tema-diario .meta-card img, body.tema-diario .hunt-topo img, body.tema-diario .op-topo img, body.tema-diario .mini img {
  background:#fff; padding:8px 8px 26px; box-shadow:0 6px 14px rgba(0,0,0,.28); transform:rotate(-3deg); transition:transform .25s, box-shadow .25s; border-radius:1px; }
body.tema-diario .scan-img:hover, body.tema-diario .boss:hover img, body.tema-diario .meta-card:hover img, body.tema-diario .hunt-card:hover .hunt-topo img, body.tema-diario .oferta-pvp:hover .op-topo img { transform:rotate(0) scale(1.05); box-shadow:0 12px 24px rgba(0,0,0,.35); }
body.tema-diario .boss { background:var(--papel) !important; border:0 !important; border-radius:2px !important; box-shadow:0 8px 18px rgba(0,0,0,.26) !important; }
body.tema-diario .boss:nth-child(3n) { transform:rotate(.6deg); }
body.tema-diario .boss:nth-child(3n+1) { transform:rotate(-.5deg); }
body.tema-diario .boss .n, body.tema-diario .scan-nome, body.tema-diario .hunt-n, body.tema-diario .regra-n { font-family:"Caveat", cursive; font-weight:700; font-size:26px; }

/* Abas como divisórias de caderno. */
body.tema-diario .seg { background:transparent !important; border:0 !important; padding:0; gap:2px; align-items:flex-end; }
body.tema-diario .seg button { background:#E9DFC8; color:var(--tinta); border-radius:10px 10px 0 0; padding:8px 16px 6px; font-family:"Special Elite", monospace; letter-spacing:.06em; box-shadow:0 -2px 6px rgba(0,0,0,.15); }
body.tema-diario .seg button:nth-child(4n+1).ativo { background:#F4C95D; }
body.tema-diario .seg button:nth-child(4n+2).ativo { background:#9FD0B8; }
body.tema-diario .seg button:nth-child(4n+3).ativo { background:#F0A3B2; }
body.tema-diario .seg button:nth-child(4n).ativo { background:#A9C3EE; }
body.tema-diario .seg button.ativo { color:var(--tinta); padding-bottom:12px; transform:translateY(2px); }

/* Botões e campos de papel; tabela pautada com cabeçalho datilografado. */
body.tema-diario button.btn { background:var(--papel); border:0; box-shadow:0 3px 8px rgba(0,0,0,.25); font-family:"Special Elite", monospace; color:var(--tinta); border-radius:3px; }
body.tema-diario button.btn.prim { background:#C4413A; color:#FFF6E6; }
body.tema-diario input, body.tema-diario select.caixa { background:var(--papel); border:0; border-bottom:2px solid var(--grafite); border-radius:0; font-family:"Caveat", cursive; font-size:20px; }
body.tema-diario .tabela th { background:transparent !important; color:var(--grafite); font-family:"Special Elite", monospace; border-bottom:2px solid var(--grafite); }
body.tema-diario .tabela td { border-bottom:0; }
body.tema-diario .tabela tbody tr:hover td { background:rgba(244,201,93,.25); }
body.tema-diario .tabela-caixa { padding:6px 4px 6px 44px; }

/* Gráficos a lápis: traço tremido e barras hachuradas. */
body.tema-diario .grafico-linha, body.tema-diario svg.velas, body.tema-diario .colunas, body.tema-diario .barras, body.tema-diario .faixa-ouro, body.tema-diario .trilho { filter:url(#rabisco); }
body.tema-diario .col-val { background:repeating-linear-gradient(-45deg, var(--grafite) 0 2px, transparent 2px 6px) !important; border:2px solid var(--grafite); border-radius:3px 3px 0 0; }
body.tema-diario .col.hoje .col-val { background:repeating-linear-gradient(-45deg, #C4413A 0 3px, rgba(196,65,58,.25) 3px 7px) !important; border-color:#C4413A; }
body.tema-diario .b-val, body.tema-diario .trilho .val { background:repeating-linear-gradient(-45deg, var(--grafite) 0 2px, rgba(74,74,82,.2) 2px 6px) !important; }
body.tema-diario .barra-l.ativo .b-val { background:repeating-linear-gradient(-45deg, #C4413A 0 2px, rgba(196,65,58,.25) 2px 6px) !important; }
body.tema-diario .grafico-linha path, body.tema-diario .grafico-linha polyline { stroke:var(--grafite); }
body.tema-diario .chip, body.tema-diario .selo { font-family:"Special Elite", monospace; background:transparent; border:1.5px solid var(--grafite); border-radius:3px; color:var(--tinta); }
body.tema-diario .selo.ok { border-color:var(--melhora); color:var(--melhora); }
body.tema-diario .selo.nao { border-color:var(--piora); color:var(--piora); }
body.tema-diario .pilula-neon { background:#F4C95D; color:var(--tinta); font-family:"Caveat", cursive; font-size:20px; border-radius:2px; transform:rotate(-1deg); }
body.tema-diario .veredito { background:#FFF4B8 !important; border:0 !important; border-radius:1px !important; box-shadow:0 6px 14px rgba(0,0,0,.22) !important; transform:rotate(-.8deg); font-family:"Caveat", cursive; font-size:19px; }
body.tema-diario .veredito b, body.tema-diario .veredito strong { font-family:"Caveat", cursive; font-size:23px; }

/* Ao trocar de tela, a página folheia. */
body.tema-diario .conteudo > * { animation:folhear .45s cubic-bezier(.2,.8,.2,1) both; transform-origin:0 50%; }
@keyframes folhear { from { opacity:0; transform:perspective(900px) rotateY(-14deg) translateX(-12px); } to { opacity:1; transform:none; } }

/* Noite: lamparina (páginas amarelando, menos brilho). */
body.tema-diario.noite { --papel:#E9DCC0; --texto:#2B2A33; --texto-2:#4C4854; --texto-3:#6E6876; }
body.tema-diario.noite .conteudo .kpi, body.tema-diario.noite .caixa { filter:brightness(.93) sepia(.15); }
body.tema-diario.noite .tabela th { color:var(--grafite) !important; }
body.tema-diario .vazio { font-family:"Caveat", cursive; font-size:22px; }
/* Texto direto sobre o wallpaper sempre em papel (o céu claro apagava o branco). */
body.tema-diario .secao { display:flex; width:fit-content; background:var(--papel); color:var(--tinta); text-shadow:none; padding:2px 16px 0; transform:rotate(-1.2deg); box-shadow:0 4px 10px rgba(0,0,0,.25); font-size:28px; margin:26px 0 12px 4px; position:relative; }
body.tema-diario .secao::before { content:""; position:absolute; top:-8px; left:-10px; width:40px; height:16px; background:var(--fita-4); transform:rotate(-30deg); }
body.tema-diario .pergunta { display:table; background:#FFF4B8; color:var(--tinta); text-shadow:none; padding:4px 14px; transform:rotate(.7deg); box-shadow:0 4px 10px rgba(0,0,0,.22); font-size:22px; margin:0 0 22px 18px; }
body.tema-diario .conteudo > .dica, body.tema-diario .conteudo > div > .dica, body.tema-diario .conteudo > * > .dica:only-child { display:inline-block; background:rgba(251,247,238,.95); color:var(--texto-2); text-shadow:none; padding:3px 12px; border-radius:2px; box-shadow:0 3px 8px rgba(0,0,0,.18); }
body.tema-diario .barra .item { white-space:nowrap; font-size:21px; }
body.tema-diario .barra .item.ativo { padding-right:34px; margin-right:-22px; }
body.tema-diario .veredito div, body.tema-diario .veredito span { font-size:17px; line-height:1.3; }
"#;
