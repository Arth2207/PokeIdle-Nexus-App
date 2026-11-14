//! Componentes padronizados das telas (ver `docs/design/telas.md`): stat tile, barras
//! ordenadas, tabela densa, cartão de veredito, abas e estado de carregamento.

use leptos::prelude::*;
use serde_json::Value;

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
    let classe = match bom {
        Some(true) => "comp melhora",
        Some(false) => "comp piora",
        None => "comp",
    };
    view! {
        <div class="kpi" title=ajuda>
            <div class="kpi-t">{titulo}{(!ajuda.is_empty()).then(|| view! { <span class="ajuda">"?"</span> })}</div>
            <div class="kpi-v">{valor}<span class="un">{unidade}</span></div>
            {comparacao.map(|c| view! { <div class=classe>{c}</div> })}
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

/// Tabela densa com cabeçalho; a primeira coluna à esquerda, o resto alinhado à direita.
#[component]
pub fn Tabela(cab: Vec<&'static str>, linhas: Vec<Vec<String>>) -> impl IntoView {
    let vazia = linhas.is_empty();
    view! {
        <div class="tabela-caixa">
            <table class="tabela">
                <thead><tr>{cab.into_iter().map(|c| view! { <th>{c}</th> }).collect_view()}</tr></thead>
                <tbody>
                    {linhas.into_iter().map(|l| view! {
                        <tr>{l.into_iter().map(|c| view! { <td>{c}</td> }).collect_view()}</tr>
                    }).collect_view()}
                </tbody>
            </table>
            {vazia.then(|| view! { <div class="vazio">"Sem dados no período."</div> })}
        </div>
    }
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
.kpi { background:rgba(255,255,255,.6); border:1px solid var(--vidro-borda); border-radius:16px; padding:12px 14px; }
.kpi-t { font-size:11px; letter-spacing:.12em; text-transform:uppercase; font-weight:700; color:var(--texto-3); display:flex; gap:6px; align-items:center; }
.ajuda { display:inline-flex; width:14px; height:14px; border-radius:50%; border:1px solid var(--texto-3); font-size:9px; align-items:center; justify-content:center; cursor:help; }
.kpi-v { font:700 26px "Space Grotesk", Inter, sans-serif; font-variant-numeric:tabular-nums; margin-top:4px; }
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
.tabela th { position:sticky; top:0; background:rgba(255,255,255,.92); text-align:right; font-size:11px; letter-spacing:.08em; text-transform:uppercase; color:var(--texto-3); padding:8px 10px; }
.tabela td { text-align:right; padding:7px 10px; border-bottom:1px solid var(--trilho); }
.tabela th:first-child, .tabela td:first-child { text-align:left; }
.veredito { background:rgba(255,255,255,.7); border:1px solid var(--vidro-borda); border-left:4px solid var(--neon); border-radius:14px; padding:12px 14px; }
.veredito.alerta { border-left-color:var(--piora); } .veredito.bom { border-left-color:var(--melhora); }
.v-t { font-weight:700; } .v-m { color:var(--texto-2); font-size:13px; margin-top:2px; }
.v-p { display:flex; gap:12px; font-size:12px; color:var(--texto-3); margin-top:6px; }
.vereditos { display:grid; grid-template-columns:repeat(auto-fit,minmax(240px,1fr)); gap:12px; margin-bottom:18px; }
.abas { display:inline-flex; margin-bottom:16px; }
.secao { margin:18px 0 8px; font:700 13px "Space Grotesk", Inter, sans-serif; letter-spacing:.1em; text-transform:uppercase; color:var(--texto-2); }
.filtros { display:flex; gap:8px; flex-wrap:wrap; margin-bottom:14px; }
.filtros input, .filtros select { width:auto; min-width:180px; }
.grade-cards { display:grid; grid-template-columns:repeat(auto-fill,minmax(180px,1fr)); gap:12px; }
.mini { background:rgba(255,255,255,.6); border:1px solid var(--vidro-borda); border-radius:14px; padding:10px; display:flex; flex-direction:column; align-items:center; gap:4px; text-align:center; cursor:pointer; }
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
.horas { display:grid; grid-template-columns:repeat(24,1fr); gap:2px; }
.horas div { height:26px; border-radius:4px; background:var(--melhora); }
.horas-rot { display:grid; grid-template-columns:repeat(24,1fr); font-size:10px; color:var(--texto-3); text-align:center; }
"#;
