//! Componentes padronizados das telas (ver `docs/design/telas.md`): stat tile, barras
//! ordenadas, tabela densa, cartão de veredito, abas e estado de carregamento.

use leptos::prelude::*;
use serde_json::Value;

/// Ícone do KPI pelo assunto do título.
fn icone_kpi(titulo: &str) -> &'static str {
    let t = titulo.to_lowercase();
    [
        ("patrim", "financas"),
        ("lucro", "mercado"),
        ("ganho", "mercado"),
        ("ouro", "financas"),
        ("rendimento", "recarregar"),
        ("disponib", "monitor"),
        ("chance", "combate"),
        ("vence", "combate"),
        ("tempo", "aguardando"),
        ("força", "combate"),
        ("ideal", "foco"),
        ("abate", "cacada"),
        ("hunt", "cacada"),
        ("shiny", "folha"),
        ("custo", "alerta"),
        ("regra", "automacao"),
        ("comando", "automacao"),
        ("espécie", "bestiario"),
    ]
    .iter()
    .find(|(k, _)| t.contains(k))
    .map_or("grade", |(_, i)| i)
}

/// KPI: valor, unidade, comparação e a definição (no `title`).
#[component]
pub fn Kpi(
    titulo: &'static str,
    valor: String,
    #[prop(optional)] unidade: &'static str,
    #[prop(optional)] comparacao: Option<String>,
    #[prop(optional)] ajuda: &'static str,
    /// `Some(true)` melhora (verde), `Some(false)` piora (vermelho), `None` neutro.
    #[prop(default = None)]
    bom: Option<bool>,
) -> impl IntoView {
    let (classe, seta, estado) = match bom {
        Some(true) => ("comp melhora", "▲ ", "kpi bom"),
        Some(false) => ("comp piora", "▼ ", "kpi ruim"),
        None => ("comp", "", "kpi"),
    };
    view! {
        <div class=estado title=ajuda>
            <div class="kpi-t">
                <span class="kpi-ico">{crate::icone(icone_kpi(titulo))}</span>
                {titulo}{(!ajuda.is_empty()).then(|| view! { <span class="ajuda">"?"</span> })}
            </div>
            <div class="kpi-v">{valor}<span class="un">{unidade}</span></div>
            {comparacao.map(|c| view! { <div class=classe>{seta}{c}</div> })}
        </div>
    }
}

/// Barras horizontais ordenadas; a linha com `destaque` leva o acento, o resto fica em cinza.
#[component]
pub fn Barras(
    linhas: Vec<(String, f64, String)>,
    #[prop(optional)] destaque: Option<String>,
) -> impl IntoView {
    let max = linhas.iter().map(|l| l.1).fold(0.0_f64, f64::max).max(1e-9);
    view! {
        <div class="barras">
            {linhas.into_iter().map(|(nome, v, rotulo)| {
                let w = format!("width:{:.1}%", (v / max * 100.0).clamp(0.0, 100.0));
                let ativo = destaque.as_deref() == Some(nome.as_str());
                view! {
                    <div class="barra-l" class:ativo=ativo>
                        <span class="b-nome">{nome}</span>
                        <span class="b-trilho"><span class="b-val" style=w></span></span>
                        <span class="b-rot">{rotulo}</span>
                    </div>
                }
            }).collect_view()}
        </div>
    }
}

/// Colunas verticais (série curta no tempo): a última leva o acento; linha tracejada na média.
#[component]
pub fn Colunas(linhas: Vec<(String, f64, String)>) -> impl IntoView {
    let max = linhas.iter().map(|l| l.1).fold(0.0_f64, f64::max).max(1e-9);
    let media = if linhas.is_empty() {
        0.0
    } else {
        linhas.iter().map(|l| l.1).sum::<f64>() / linhas.len() as f64
    };
    let n = linhas.len();
    let topo_media = format!("bottom:{:.1}%", (media / max * 100.0).clamp(0.0, 100.0));
    view! {
        <div class="colunas">
            <div class="col-media" style=topo_media></div>
            {linhas.into_iter().enumerate().map(|(i, (nome, v, rot))| {
                let h = format!("height:{:.1}%", (v.max(0.0) / max * 100.0).clamp(0.0, 100.0));
                let ultima = i + 1 == n;
                view! {
                    <div class="col" class:hoje=ultima>
                        <span class="col-rot">{rot}</span>
                        <span class="col-trilho"><span class="col-val" style=h></span></span>
                        <span class="col-nome">{nome}</span>
                    </div>
                }
            }).collect_view()}
        </div>
    }
}

/// Linha no tempo (SVG): os pontos e, se houver, a faixa esperada (média ± 20%) atrás.
#[component]
pub fn Linha(pontos: Vec<(String, f64)>, #[prop(optional)] media: Option<f64>) -> impl IntoView {
    let (w, h) = (600.0, 180.0);
    let max = pontos
        .iter()
        .map(|p| p.1)
        .chain(media.map(|m| m * 1.2))
        .fold(0.0_f64, f64::max)
        .max(1e-9);
    let n = pontos.len().max(2) as f64 - 1.0;
    let xy = |i: usize, v: f64| (i as f64 / n * w, h - v / max * (h - 10.0));
    let caminho: String = pontos
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let (x, y) = xy(i, p.1);
            format!("{}{x:.1},{y:.1}", if i == 0 { "M" } else { " L" })
        })
        .collect();
    let faixa = media.map(|m| {
        let (a, b) = (xy(0, m * 1.2).1, xy(0, m * 0.8).1);
        view! { <rect x="0" y=format!("{a:.1}") width=w height=format!("{:.1}", b - a) class="faixa" /> }
    });
    let rotulos: Vec<String> = pontos.iter().map(|p| p.0.clone()).collect();
    let passo = (rotulos.len() / 6).max(1);
    view! {
        <svg class="grafico-linha" viewBox=format!("0 0 {w} {h}") preserveAspectRatio="none">
            {faixa}
            <path d=caminho />
        </svg>
        <div class="linha-rot">
            {rotulos.into_iter().step_by(passo).map(|r| view! { <span>{r}</span> }).collect_view()}
        </div>
    }
}

/// Mapa de calor: linhas × 24 horas, a cor mais forte é o maior valor da tabela.
#[component]
pub fn Calor(linhas: Vec<(String, Vec<Option<f64>>)>) -> impl IntoView {
    let max = linhas
        .iter()
        .flat_map(|l| l.1.iter().flatten())
        .copied()
        .fold(0.0_f64, f64::max)
        .max(1e-9);
    view! {
        <div class="calor">
            <span></span>
            {(0..24).map(|h| view! { <span class="calor-h">{if h % 2 == 0 { format!("{h:02}") } else { String::new() }}</span> }).collect_view()}
            {linhas.into_iter().map(|(nome, v)| view! {
                <span class="calor-n">{nome}</span>
                {v.into_iter().map(|x| {
                    let o = x.map_or(0.0, |x| 0.12 + 0.88 * x / max);
                    view! { <span class="calor-c" style=format!("opacity:{o:.2}") title=x.map(|x| format!("{x:.0}")).unwrap_or_default()></span> }
                }).collect_view()}
            }).collect_view()}
        </div>
    }
}

/// Cascata (waterfall): cada passo soma ou subtrai do anterior; a última barra é o total.
#[component]
pub fn Cascata(passos: Vec<(String, f64)>) -> impl IntoView {
    let total: f64 = passos.iter().map(|p| p.1).sum();
    let mut acum = 0.0_f64;
    let mut barras: Vec<(String, f64, f64, &'static str)> = passos
        .iter()
        .map(|(n, d)| {
            let (de, ate) = (acum, acum + d);
            acum = ate;
            (n.clone(), de, ate, if *d >= 0.0 { "sobe" } else { "desce" })
        })
        .collect();
    barras.push(("Resultado".into(), 0.0, total, "total"));
    let max = barras
        .iter()
        .map(|b| b.1.max(b.2))
        .fold(0.0_f64, f64::max)
        .max(1e-9);
    let min = barras.iter().map(|b| b.1.min(b.2)).fold(0.0_f64, f64::min);
    let faixa = max - min;
    let n = barras.len() as f64;
    let (w, h) = (600.0, 200.0);
    let y = |v: f64| 12.0 + (max - v) / faixa * (h - 40.0);
    view! {
        <svg class="cascata" viewBox=format!("0 0 {w} {h}")>
            {barras.into_iter().enumerate().map(|(i, (nome, de, ate, classe))| {
                let x = w / n * i as f64 + w / n * 0.15;
                let (top, base) = (y(de.max(ate)), y(de.min(ate)));
                let rot = crate::dados::curto(ate - de);
                view! {
                    <g class=classe>
                        <rect x=x y=top width=w / n * 0.7 height=(base - top).max(1.5) rx="4" />
                        <text x=x + w / n * 0.35 y=top - 4.0 text-anchor="middle">{rot}</text>
                        <text class="rot" x=x + w / n * 0.35 y=h - 8.0 text-anchor="middle">{nome}</text>
                    </g>
                }
            }).collect_view()}
        </svg>
    }
}

/// Tabela densa com cabeçalho; a primeira coluna à esquerda, o resto alinhado à direita.
#[component]
pub fn Tabela(cab: Vec<&'static str>, linhas: Vec<Vec<String>>) -> impl IntoView {
    let vazia = linhas.is_empty();
    // Coluna numérica (todas as células parecem número) alinha à direita; texto, à esquerda.
    let numerica: Vec<bool> = (0..cab.len())
        .map(|i| {
            !linhas.is_empty()
                && linhas
                    .iter()
                    .all(|l| l.get(i).is_none_or(|c| parece_numero(c)))
        })
        .collect();
    let n2 = numerica.clone();
    view! {
        <div class="tabela-caixa">
            <table class="tabela">
                <thead><tr>{cab.into_iter().enumerate().map(|(i, c)| { let num = n2.get(i).copied().unwrap_or(false); view! { <th class:num=num>{c}</th> } }).collect_view()}</tr></thead>
                <tbody>
                    {linhas.into_iter().map(|l| { let numerica = numerica.clone(); view! {
                        <tr>{l.into_iter().enumerate().map(|(i, c)| { let num = numerica.get(i).copied().unwrap_or(false); view! { <td class:num=num>{c}</td> } }).collect_view()}</tr>
                    }}).collect_view()}
                </tbody>
            </table>
            {vazia.then(|| view! { <div class="vazio">"Sem dados no período."</div> })}
        </div>
    }
}

/// Célula que parece número ("1.234", "−5,2%", "+16", "12 mi", "—"): alinha à direita.
pub fn parece_numero(c: &str) -> bool {
    let t = c.trim();
    t.is_empty()
        || t == "—"
        || t.chars()
            .next()
            .is_some_and(|x| x.is_ascii_digit() || "+-−~≈#×".contains(x))
            && t.chars().filter(|x| x.is_alphabetic()).count() <= 5
}

/// Recomendação com motivo, confiança e (opcional) o que fazer.
#[component]
pub fn Veredito(
    titulo: String,
    motivo: String,
    #[prop(optional)] confianca: Option<f64>,
    #[prop(optional)] acao: Option<String>,
    #[prop(optional)] tom: &'static str,
) -> impl IntoView {
    view! {
        <div class=format!("veredito {tom}")>
            <div class="v-t">{titulo}</div>
            <div class="v-m">{motivo}</div>
            <div class="v-p">
                {confianca.map(|c| view! { <span>{format!("confiança {c:.0}%")}</span> })}
                {acao.map(|a| view! { <b>{a}</b> })}
            </div>
        </div>
    }
}

/// Abas simples.
#[component]
pub fn Abas(opcoes: &'static [&'static str], ativa: RwSignal<&'static str>) -> impl IntoView {
    view! {
        <div class="seg abas">
            {opcoes.iter().map(|o| {
                let o: &'static str = o;
                view! { <button class:ativo=move || ativa.get() == o on:click=move |_| ativa.set(o)>{o}</button> }
            }).collect_view()}
        </div>
    }
}

/// Mostra o conteúdo quando o recurso chega, com aviso de carregando e de erro.
pub fn carregado<V: IntoView + 'static>(
    r: Option<Option<Result<Value, String>>>,
    f: impl FnOnce(Value) -> V,
) -> AnyView {
    match r {
        None => view! { <div class="vazio">"Carregando…"</div> }.into_any(),
        Some(None) => view! { <div class="vazio">"Escolha uma conta no topo."</div> }.into_any(),
        Some(Some(Err(e))) => view! { <div class="dica erro">{e}</div> }.into_any(),
        Some(Some(Ok(v))) => f(v).into_any(),
    }
}

pub const CSS: &str = r#"
.kpis { display:grid; grid-template-columns:repeat(auto-fit,minmax(170px,1fr)); gap:12px; margin-bottom:18px; }
.kpi { position:relative; overflow:hidden; background:linear-gradient(140deg, rgba(255,255,255,.66), rgba(255,255,255,.34)); backdrop-filter:blur(16px) saturate(1.3); -webkit-backdrop-filter:blur(16px) saturate(1.3); box-shadow:0 10px 30px rgba(22,24,28,.10), inset 0 1px 0 rgba(255,255,255,.85); border:1px solid var(--vidro-borda); border-radius:20px; padding:16px 18px 14px; transition:transform .18s, box-shadow .18s; }
.kpi:hover { transform:translateY(-2px); box-shadow:0 16px 40px rgba(22,24,28,.14), inset 0 1px 0 rgba(255,255,255,.85); }
.kpi::after { content:""; position:absolute; left:18px; right:18px; bottom:0; height:3px; border-radius:3px 3px 0 0; background:linear-gradient(90deg, var(--neon), transparent); opacity:.8; }
.kpi.bom::after { background:linear-gradient(90deg, var(--melhora), transparent); }
.kpi.ruim::after { background:linear-gradient(90deg, var(--piora), transparent); }
.kpi-ico { width:30px; height:30px; border-radius:50%; display:inline-flex; align-items:center; justify-content:center; background:rgba(200,255,46,.35); color:var(--texto); flex:none; }
.kpi-ico svg { width:17px; height:17px; }
.kpi .comp { display:inline-block; margin-top:6px; padding:2px 10px; border-radius:999px; background:var(--trilho); font-size:12px; font-weight:600; }
.kpi .comp.melhora { background:rgba(31,157,107,.14); }
.kpi .comp.piora { background:rgba(229,83,61,.14); }
.kpi-t { font-size:11px; letter-spacing:.12em; text-transform:uppercase; font-weight:700; color:var(--texto-3); display:flex; gap:6px; align-items:center; }
.ajuda { display:inline-flex; width:14px; height:14px; border-radius:50%; border:1px solid var(--texto-3); font-size:9px; align-items:center; justify-content:center; cursor:help; }
.kpi-v { font:700 32px "Space Grotesk", Inter, sans-serif; font-variant-numeric:tabular-nums; margin-top:4px; }
.kpi-v .un { font:500 13px Inter, sans-serif; color:var(--texto-2); margin-left:6px; }
.comp { font-size:12px; color:var(--texto-2); margin-top:2px; }
.comp.melhora { color:var(--melhora); } .comp.piora { color:var(--piora); }
.barras { display:flex; flex-direction:column; gap:6px; }
.barra-l { display:grid; grid-template-columns:minmax(120px,30%) 1fr auto; gap:10px; align-items:center; font-size:13px; }
.b-nome { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
.b-trilho { height:12px; border-radius:999px; background:var(--trilho); overflow:hidden; }
.b-val { display:block; height:100%; border-radius:999px; background:#9AA3AD; }
.barra-l.ativo .b-val { background:var(--melhora); }
.barra-l.ativo .b-nome { font-weight:700; }
.b-rot { font-variant-numeric:tabular-nums; color:var(--texto-2); min-width:70px; text-align:right; }
.tabela-caixa { overflow:auto; max-height:480px; border-radius:12px; }
.tabela { width:100%; border-collapse:collapse; font-size:13px; font-variant-numeric:tabular-nums; }
.tabela th { position:sticky; top:0; background:rgba(255,255,255,.92); text-align:left; white-space:nowrap; font-size:11px; letter-spacing:.08em; text-transform:uppercase; color:var(--texto-3); padding:8px 10px; }
.tabela td { text-align:left; padding:7px 10px; border-bottom:1px solid var(--trilho); }
.tabela th.num, .tabela td.num { text-align:right; }
.veredito { background:var(--cartao); backdrop-filter:blur(14px); -webkit-backdrop-filter:blur(14px); box-shadow:0 8px 24px rgba(22,24,28,.08); border:1px solid var(--vidro-borda); border-left:4px solid var(--neon); border-radius:14px; padding:12px 14px; }
.veredito.alerta { border-left-color:var(--piora); } .veredito.bom { border-left-color:var(--melhora); }
.v-t { font-weight:700; } .v-m { color:var(--texto-2); font-size:13px; margin-top:2px; }
.v-p { display:flex; gap:12px; font-size:12px; color:var(--texto-3); margin-top:6px; }
.vereditos { display:grid; grid-template-columns:repeat(auto-fit,minmax(240px,1fr)); gap:12px; margin-bottom:18px; }
.abas { display:inline-flex; margin-bottom:16px; }
.secao { margin:18px 0 8px; font:700 13px "Space Grotesk", Inter, sans-serif; letter-spacing:.1em; text-transform:uppercase; color:var(--texto-2); }
.filtros { display:flex; gap:8px; flex-wrap:wrap; margin-bottom:14px; }
.filtros input, .filtros select { width:auto; min-width:180px; }
.grade-cards { display:grid; grid-template-columns:repeat(auto-fill,minmax(180px,1fr)); gap:12px; }
.mini { background:var(--cartao); backdrop-filter:blur(14px); -webkit-backdrop-filter:blur(14px); box-shadow:0 8px 24px rgba(22,24,28,.08); border:1px solid var(--vidro-borda); border-radius:14px; padding:10px; display:flex; flex-direction:column; align-items:center; gap:4px; text-align:center; cursor:pointer; }
.mini img { width:84px; height:84px; object-fit:contain; filter:drop-shadow(0 6px 8px rgba(22,24,28,.25)); }
.mini .n { font-weight:700; } .mini .s { font-size:12px; color:var(--texto-2); }
.mini.sel { outline:2px solid var(--melhora); }
.selo { font-size:11px; font-weight:700; padding:2px 8px; border-radius:999px; background:var(--trilho); }
.selo.ok { background:rgba(31,157,107,.18); color:var(--melhora); } .selo.nao { background:rgba(229,83,61,.15); color:var(--piora); }
.tabuleiro { display:grid; grid-template-columns:repeat(5,44px); gap:4px; }
.casa { width:44px; height:44px; border-radius:10px; background:var(--trilho); display:flex; align-items:center; justify-content:center; font-size:10px; color:var(--texto-3); position:relative; }
.casa img { width:40px; height:40px; object-fit:contain; }
.casa.ocupada { background:rgba(200,255,46,.55); }
.duas { display:grid; grid-template-columns:1fr 1fr; gap:18px; }
@media (max-width: 1100px) { .duas { grid-template-columns:1fr; } }
.colunas { position:relative; display:flex; gap:10px; align-items:stretch; height:170px; padding-top:18px; }
.col { flex:1; display:flex; flex-direction:column; align-items:center; gap:4px; min-width:0; }
.col-trilho { flex:1; width:100%; display:flex; align-items:flex-end; }
.col-val { display:block; width:100%; border-radius:8px 8px 3px 3px; background:#9AA3AD; }
.col.hoje .col-val { background:var(--neon); }
.col-rot { font-size:11px; font-variant-numeric:tabular-nums; color:var(--texto-2); }
.col-nome { font-size:11px; color:var(--texto-3); }
.col-media { position:absolute; left:0; right:0; border-top:1px dashed var(--texto-3); margin-bottom:22px; pointer-events:none; }
.vg { display:grid; grid-template-columns:1.6fr 1fr; gap:16px; margin-bottom:16px; }
.vg:has(> .larga) { grid-template-columns:1fr; }
.faixa { display:grid; grid-template-columns:repeat(auto-fit, minmax(320px, 1fr)); gap:14px; }
.faixa /* Formas do jogo sobre a imagem da espécie base: brilho de cor própria. */
img.forma-furious { filter:drop-shadow(0 0 10px rgba(229,83,61,.9)) saturate(1.25) hue-rotate(-12deg); }
img.forma-ancient { filter:drop-shadow(0 0 10px rgba(214,158,46,.9)) sepia(.35); }
img.forma-brave { filter:drop-shadow(0 0 10px rgba(56,132,255,.9)) saturate(1.15); }
/* Janela por cima da tela (detalhe do boss) */
.janela-fundo { align-items:center; padding:24px; backdrop-filter:blur(4px); -webkit-backdrop-filter:blur(4px); }
.janela { position:relative; width:min(1100px, 96vw); max-height:90vh; overflow:auto; padding:24px 28px; animation:surgir .22s ease-out; }
.janela .fechar { position:sticky; top:0; float:right; z-index:5; }
@keyframes surgir { from { opacity:0; transform:translateY(12px) scale(.98); } to { opacity:1; transform:none; } }
/* Caçada › Mapa de hunts */
.hunts { display:grid; grid-template-columns:repeat(auto-fill, minmax(320px, 1fr)); gap:14px; margin-top:14px; }
.hunt-card { padding:14px 16px; border-radius:20px; background:var(--cartao); border:1px solid var(--vidro-borda); backdrop-filter:blur(14px); -webkit-backdrop-filter:blur(14px); display:flex; flex-direction:column; gap:10px; transition:transform .18s; }
.hunt-card:hover { transform:translateY(-2px); }
.hunt-card.atual { box-shadow:0 0 0 2px var(--neon), 0 10px 30px rgba(200,255,46,.25); }
.hunt-card.melhor { box-shadow:0 0 0 2px var(--melhora); }
.hunt-topo { display:flex; align-items:center; gap:10px; }
.hunt-topo img { width:56px; height:56px; object-fit:contain; }
.hunt-topo > div { flex:1; }
.hunt-n { font:700 16px "Space Grotesk", Inter, sans-serif; }
.hunt-nums { display:grid; grid-template-columns:repeat(5, 1fr); gap:6px; }
.hunt-nums div { display:flex; flex-direction:column; }
.hunt-nums b { font:700 15px "Space Grotesk", Inter, sans-serif; font-variant-numeric:tabular-nums; }
.hunt-nums span { font-size:10px; color:var(--texto-3); text-transform:uppercase; letter-spacing:.06em; }
.faixa-ouro { position:relative; height:8px; border-radius:999px; background:var(--trilho); }
.faixa-ouro .faixa { position:absolute; top:0; bottom:0; border-radius:999px; background:rgba(31,157,107,.45); }
.faixa-ouro .marca { position:absolute; top:-3px; width:3px; height:14px; border-radius:2px; background:var(--texto); transform:translateX(-1px); }
.hunt-drops { display:flex; flex-wrap:wrap; gap:4px; }
/* Mercado › Para o PvP */
.ofertas-pvp { display:grid; grid-template-columns:repeat(auto-fill, minmax(260px, 1fr)); gap:12px; margin-top:12px; }
.oferta-pvp { padding:12px 14px; border-radius:18px; background:var(--cartao); border:1px solid var(--vidro-borda); backdrop-filter:blur(14px); -webkit-backdrop-filter:blur(14px); display:flex; flex-direction:column; gap:6px; }
.op-topo { display:flex; gap:10px; align-items:center; }
.op-topo img { width:56px; height:56px; object-fit:contain; image-rendering:auto; }
.op-preco { font:700 16px "Space Grotesk", Inter, sans-serif; }
.op-preco span { font:500 12px Inter, sans-serif; }
/* Automação › Controles */
.regras { display:grid; grid-template-columns:repeat(auto-fill, minmax(300px, 1fr)); gap:14px; }
.regra { background:var(--cartao); backdrop-filter:blur(14px); -webkit-backdrop-filter:blur(14px); border:1px solid var(--vidro-borda); border-radius:18px; padding:14px 16px; transition:opacity .2s; }
.regra.desligada { opacity:.6; }
.regra-topo { display:flex; justify-content:space-between; gap:10px; align-items:flex-start; }
.regra-n { font:700 15px "Space Grotesk", Inter, sans-serif; }
.regra-op { display:flex; flex-wrap:wrap; gap:8px 14px; margin-top:10px; }
.regra-op .op { display:flex; align-items:center; gap:6px; font-size:12px; color:var(--texto-2); }
.regra-op .num { width:84px; padding:4px 10px; font-size:12px; }
.chave-btn { border:0; background:transparent; padding:0; cursor:pointer; }
.aviso-ok { background:rgba(31,157,107,.14); border-radius:12px; padding:8px 12px; margin-bottom:12px; font-size:13px; }
.grade-regras td { text-align:center; }
@keyframes pulsar { 0%,100% { box-shadow:0 0 0 4px rgba(255,255,255,.5), 0 0 22px var(--tier); } 50% { box-shadow:0 0 0 6px rgba(255,255,255,.65), 0 0 40px var(--tier); } }
/* PvP: tabuleiro do jogo (30 casas, 5 fileiras), elo, cortes, meta e treino */
.tabuleiro-pvp { position:relative; display:grid; grid-template-columns:repeat(8, 58px); grid-template-rows:repeat(5, 58px); gap:6px; justify-content:center; margin:8px auto; padding:26px 18px 14px; border-radius:22px; background:radial-gradient(ellipse at 50% 0%, rgba(200,255,46,.18), transparent 70%), var(--cartao); border:1px solid var(--vidro-borda); width:max-content; }
.tabuleiro-pvp.mini { grid-template-columns:repeat(8, 26px); grid-template-rows:repeat(5, 26px); gap:3px; padding:16px 8px 8px; border-radius:14px; }
.casa-pvp { position:relative; border-radius:12px; background:rgba(255,255,255,.28); border:1px dashed rgba(22,24,28,.15); display:flex; align-items:center; justify-content:center; }
.mini .casa-pvp { border-radius:6px; }
.casa-pvp .n { font-size:10px; color:var(--texto-3); }
.mini .casa-pvp .n { display:none; }
.casa-pvp.ocupada { background:rgba(200,255,46,.28); border:1px solid rgba(200,255,46,.9); box-shadow:0 0 14px rgba(200,255,46,.35); cursor:help; }
.casa-pvp img { width:120%; height:120%; object-fit:contain; filter:drop-shadow(0 4px 6px rgba(0,0,0,.25)); }
.casa-pvp .sigla { font-size:10px; font-weight:700; }
.casa-pvp .balao { display:none; position:absolute; bottom:calc(100% + 8px); left:50%; transform:translateX(-50%); width:220px; z-index:30; padding:10px 12px; border-radius:14px; background:rgba(22,24,28,.92); color:#fff; font-size:12px; line-height:1.4; box-shadow:0 12px 30px rgba(0,0,0,.35); pointer-events:none; }
.casa-pvp .balao b { display:block; margin-bottom:4px; color:var(--neon); }
.casa-pvp:hover .balao { display:block; }
.tabuleiro-pvp .frente { position:absolute; top:6px; left:0; right:0; text-align:center; font-size:10px; letter-spacing:.12em; text-transform:uppercase; color:var(--texto-3); }
.mini .frente { display:none; }
.formacao { display:flex; flex-direction:column; align-items:center; gap:6px; }
.formacoes { display:flex; flex-wrap:wrap; justify-content:center; gap:24px; }
.elo-faixa { display:flex; flex-wrap:wrap; gap:18px; align-items:center; padding:18px 22px; margin-bottom:16px; border-radius:22px; background:linear-gradient(120deg, color-mix(in srgb, var(--tier) 30%, transparent), var(--cartao)); border:1px solid var(--vidro-borda); backdrop-filter:blur(16px); -webkit-backdrop-filter:blur(16px); }
.elo-selo { width:96px; height:96px; border-radius:50%; display:flex; flex-direction:column; align-items:center; justify-content:center; background:radial-gradient(circle at 35% 30%, #fff8, var(--tier)); box-shadow:0 0 0 4px rgba(255,255,255,.5), 0 0 30px var(--tier); color:#16181C; font:800 13px "Space Grotesk", Inter, sans-serif; letter-spacing:.08em; animation:pulsar 3s ease-in-out infinite; }
.elo-selo b { font-size:22px; }
.elo-num { display:flex; flex-direction:column; min-width:96px; }
.elo-num b { font:700 30px "Space Grotesk", Inter, sans-serif; font-variant-numeric:tabular-nums; }
.elo-num span { font-size:11px; letter-spacing:.1em; text-transform:uppercase; color:var(--texto-3); }
.elo-num.bom b { color:var(--melhora); }
.elo-num.ruim b { color:var(--piora); }
.cortes { display:grid; grid-template-columns:repeat(auto-fit, minmax(240px, 1fr)); gap:14px; }
.corte { padding:16px 18px; border-radius:20px; background:var(--cartao); border:1px solid var(--vidro-borda); backdrop-filter:blur(14px); -webkit-backdrop-filter:blur(14px); display:flex; flex-direction:column; gap:6px; }
.corte-t { font:700 13px "Space Grotesk", Inter, sans-serif; letter-spacing:.12em; text-transform:uppercase; color:var(--texto-2); }
.corte-v { font:700 28px "Space Grotesk", Inter, sans-serif; }
.trilho-corte { height:10px; border-radius:999px; background:var(--trilho); overflow:hidden; }
.trilho-corte span { display:block; height:100%; border-radius:999px; background:linear-gradient(90deg, var(--melhora), var(--neon)); }
.corte-prev { font-weight:600; font-size:13px; }
.meta-grade { display:grid; grid-template-columns:repeat(auto-fill, minmax(260px, 1fr)); gap:12px; }
.meta-card { display:flex; gap:10px; align-items:center; padding:10px 12px; border-radius:18px; background:var(--cartao); border:1px solid var(--vidro-borda); }
.meta-card img { width:64px; height:64px; object-fit:contain; }
.meta-info { flex:1; display:flex; flex-direction:column; gap:4px; min-width:0; }
.barra-taxa { height:8px; border-radius:999px; background:var(--trilho); overflow:hidden; }
.barra-taxa span { display:block; height:100%; border-radius:999px; background:var(--piora); }
.barra-taxa.bom span { background:var(--melhora); }
.times-meta, .treino-adv { display:grid; grid-template-columns:repeat(auto-fill, minmax(280px, 1fr)); gap:12px; }
.time-meta, .adv-card { padding:12px 14px; border-radius:18px; background:var(--cartao); border:1px solid var(--vidro-borda); display:flex; flex-direction:column; gap:6px; }
.sprites { display:flex; gap:2px; }
.sprites img { width:40px; height:40px; object-fit:contain; }
.adv-topo { display:flex; justify-content:space-between; align-items:center; gap:8px; }
.scan { grid-template-columns:110px 1fr; }
.vg-base { display:grid; grid-template-columns:1.3fr 1fr; gap:16px; }
@media (max-width: 1100px) { .vg, .vg-base { grid-template-columns:1fr; } }
.caixa { background:linear-gradient(150deg, rgba(255,255,255,.62), rgba(255,255,255,.32)); backdrop-filter:blur(16px) saturate(1.3); -webkit-backdrop-filter:blur(16px) saturate(1.3); box-shadow:0 10px 30px rgba(22,24,28,.10), inset 0 1px 0 rgba(255,255,255,.85); border:1px solid var(--vidro-borda); border-radius:22px; padding:18px 20px; }
.caixa-t { font:700 13px "Space Grotesk", Inter, sans-serif; letter-spacing:.1em; text-transform:uppercase; color:var(--texto-2); margin-bottom:12px; display:flex; align-items:center; gap:8px; }
.caixa-t::before { content:""; width:8px; height:8px; border-radius:50%; background:var(--neon); box-shadow:0 0 0 3px rgba(200,255,46,.3); flex:none; }
.scan { display:grid; grid-template-columns:minmax(180px,1fr) 1.1fr; gap:14px; align-items:center; }
.scan-img { position:relative; display:flex; justify-content:center; }
.scan-img img { width:100%; max-width:260px; aspect-ratio:1; object-fit:contain; filter:drop-shadow(0 18px 18px rgba(22,24,28,.28)); }
.scan-img::after { content:""; position:absolute; bottom:4%; width:62%; height:12%; border-radius:50%; background:radial-gradient(rgba(200,255,46,.6), transparent 70%); z-index:-1; }
.scan-nome { font:700 30px "Space Grotesk", Inter, sans-serif; }
.scan-sub { font-size:12px; color:var(--texto-3); letter-spacing:.08em; }
.chips { display:flex; gap:6px; flex-wrap:wrap; margin:8px 0; }
.chip { font-size:12px; font-weight:600; padding:3px 10px; border-radius:999px; background:var(--trilho); }
.medidor { display:grid; grid-template-columns:28px 1fr auto; gap:8px; align-items:center; font-size:12px; margin-top:6px; }
.medidor .trilho { height:9px; border-radius:999px; background:var(--trilho); overflow:hidden; }
.medidor .val { display:block; height:100%; border-radius:999px; background:var(--melhora); }
.medidor .xp .val { background:var(--neon); }
.pilula-neon { display:inline-flex; gap:6px; align-items:center; margin-top:10px; padding:5px 12px; border-radius:999px; background:var(--neon); color:var(--sobre-neon); font-weight:600; font-size:13px; }
.acao-v { display:flex; gap:10px; align-items:center; }
.acao-v .veredito { flex:1; }
.diario-m { display:flex; flex-direction:column; gap:6px; font-size:13px; max-height:220px; overflow:auto; }
.diario-m div { display:grid; grid-template-columns:44px 90px 1fr; gap:8px; }
.diario-m .h { font-variant-numeric:tabular-nums; color:var(--texto-3); }
.diario-m .falha { color:var(--piora); }
.grade-bosses { display:grid; grid-template-columns:repeat(auto-fill,minmax(260px,1fr)); gap:12px; margin-bottom:12px; }
.boss { display:grid; grid-template-columns:96px 1fr; gap:10px; align-items:center; background:var(--cartao); backdrop-filter:blur(14px); -webkit-backdrop-filter:blur(14px); box-shadow:0 8px 24px rgba(22,24,28,.08); border:1px solid var(--vidro-borda); border-radius:16px; padding:10px; cursor:pointer; }
.boss img { width:96px; height:96px; object-fit:contain; filter:drop-shadow(0 8px 10px rgba(22,24,28,.25)); }
.boss .n { font-weight:700; } .boss .s { font-size:12px; color:var(--texto-2); margin-bottom:4px; }
.boss.sel { outline:2px solid var(--melhora); }
.pregao { display:grid; grid-template-columns:minmax(230px,1fr) 2fr minmax(240px,1fr); gap:14px; }
@media (max-width: 1100px) { .pregao { grid-template-columns:1fr; } }
.lista-ativos { display:flex; flex-direction:column; gap:8px; }
.ativos { max-height:470px; overflow:auto; display:flex; flex-direction:column; gap:2px; }
.ativo { display:grid; grid-template-columns:26px 1fr auto 56px; gap:6px; align-items:center; font-size:13px; padding:5px 6px; border-radius:10px; cursor:pointer; font-variant-numeric:tabular-nums; }
.ativo img { width:24px; height:24px; object-fit:contain; }
.ativo .nome { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
.ativo.sel { background:var(--neon); }
.melhora { color:var(--melhora); } .piora { color:var(--piora); }
.velas { width:100%; }
.velas line { stroke-width:1.2; } .velas .sobe { fill:var(--melhora); stroke:var(--melhora); } .velas .desce { fill:var(--piora); stroke:var(--piora); }
.velas .vol { opacity:.45; } .velas .ma { fill:none; stroke:#8BC34A; stroke-width:2; }
.velas .justo { stroke:var(--texto-2); stroke-dasharray:5 4; }
.ohlc { font-size:12px; color:var(--texto-2); font-variant-numeric:tabular-nums; margin-bottom:4px; }
.sinal { font:700 30px "Space Grotesk", Inter, sans-serif; } .sinal.bom { color:var(--melhora); } .sinal.alerta { color:var(--piora); }
.pares { display:grid; grid-template-columns:1fr auto; gap:6px 10px; font-size:13px; margin-top:10px; font-variant-numeric:tabular-nums; }
.cascata { width:100%; }
.cascata .sobe rect { fill:var(--melhora); } .cascata .desce rect { fill:var(--piora); } .cascata .total rect { fill:var(--neon); }
.cascata text { font-size:12px; fill:var(--texto); font-variant-numeric:tabular-nums; } .cascata .rot { fill:var(--texto-3); }
.cal { display:grid; grid-template-columns:repeat(7,1fr); gap:4px; }
.cal div { aspect-ratio:1.6; border-radius:6px; background:var(--melhora); display:flex; align-items:center; justify-content:center; font-size:11px; color:var(--texto); }
.cal div.neg { background:var(--piora); }
.pokedex { display:grid; grid-template-columns:repeat(auto-fill,minmax(48px,1fr)); gap:4px; margin-top:10px; }
.pokedex img { width:48px; height:48px; object-fit:contain; }
.pokedex img.falta { filter:brightness(0) opacity(.18); }
.grafico-linha { width:100%; height:180px; }
.grafico-linha path { fill:none; stroke:var(--melhora); stroke-width:2.5; vector-effect:non-scaling-stroke; }
.grafico-linha .faixa { fill:var(--trilho); }
.linha-rot { display:flex; justify-content:space-between; font-size:11px; color:var(--texto-3); }
.calor { display:grid; grid-template-columns:minmax(110px,auto) repeat(24,1fr); gap:2px; font-size:11px; align-items:center; }
.calor-h { color:var(--texto-3); text-align:center; }
.calor-n { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; padding-right:6px; }
.calor-c { height:18px; border-radius:3px; background:var(--melhora); }
.horas { display:grid; grid-template-columns:repeat(24,1fr); gap:2px; }
.horas div { height:26px; border-radius:4px; background:var(--melhora); }
.horas-rot { display:grid; grid-template-columns:repeat(24,1fr); font-size:10px; color:var(--texto-3); text-align:center; }
"#;

#[cfg(test)]
mod testes_tabela {
    use super::parece_numero;

    #[test]
    fn colunas_numericas_e_de_texto() {
        assert!(parece_numero("1.234"));
        assert!(parece_numero("−5,2%"));
        assert!(parece_numero("12,4 mi"));
        assert!(parece_numero("—"));
        assert!(!parece_numero("Metal Stone"));
        assert!(!parece_numero("Charizard chegou na meta"));
    }
}
