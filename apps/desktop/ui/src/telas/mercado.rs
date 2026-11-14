//! Mercado: o que compro, o que vendo e o que guardo?

use leptos::prelude::*;
use serde_json::Value;

use super::cabecalho;
use crate::comp::{carregado, Abas, Kpi, Tabela, Veredito};
use crate::dados::{curto, inteiro, lista, num, pct, recurso, txt};
use crate::Estado;

const ABAS: &[&str] = &["Pregão", "Oportunidades", "Inventário"];

fn acao_pt(a: &str) -> String {
    match a {
        "COMPRAR" => "comprar",
        "VENDER" => "vender",
        "AGUARDAR_PARA_COMPRAR" => "esperar para comprar",
        "AGUARDAR_PARA_VENDER" => "esperar para vender",
        "MANTER" => "manter",
        "OBSERVAR" => "observar",
        _ => a,
    }
    .to_owned()
}

fn aba_pregao(e: Estado, versao: RwSignal<u32>, moeda: RwSignal<String>) -> impl IntoView {
    let m = recurso(e, versao, || Some("/api/bolsa/mercado".into()));
    let busca = RwSignal::new(String::new());
    view! {
        <div class="filtros">
            <input placeholder="Buscar espécie ou item" prop:value=move || busca.get()
                on:input=move |ev| busca.set(event_target_value(&ev)) />
        </div>
        {move || carregado(m.get(), move |v| {
            let r = v.get("resumo").cloned().unwrap_or(Value::Null);
            let q = busca.get().to_lowercase();
            let moeda_api = if moeda.get() == "Gold" { "gold" } else { "orb" };
            let mut t: Vec<Value> = lista(&v, "tickers").iter()
                .filter(|x| txt(x, "moeda") == moeda_api && (q.is_empty() || txt(x, "nome").to_lowercase().contains(&q)))
                .cloned().collect();
            t.sort_by(|a, b| num(b, "vscore").unwrap_or(0.0).abs().total_cmp(&num(a, "vscore").unwrap_or(0.0).abs()));
            let linhas: Vec<Vec<String>> = t.iter().take(150).map(|x| vec![
                format!("{}{}", txt(x, "nome"), if x.get("shiny").and_then(Value::as_bool) == Some(true) { " ✦" } else { "" }),
                curto(num(x, "ultimo").unwrap_or(0.0)),
                curto(num(x, "base").unwrap_or(0.0)),
                num(x, "var24h").map_or("—".into(), pct),
                num(x, "var7d").map_or("—".into(), pct),
                inteiro(num(x, "vol24h").unwrap_or(0.0)),
                txt(x, "liquidez"),
                acao_pt(&txt(x, "acao")),
                format!("{:.0}%", num(x, "vconf").unwrap_or(0.0)),
            ]).collect();
            view! {
                <div class="kpis">
                    <Kpi titulo="Comprar" valor=inteiro(num(&r, "comprar").unwrap_or(0.0)) unidade="ativos"
                        ajuda="Ativos com preço bem abaixo da base (média de 30 dias ajustada pela tendência)." />
                    <Kpi titulo="Vender" valor=inteiro(num(&r, "vender").unwrap_or(0.0)) unidade="ativos" />
                    <Kpi titulo="Observar" valor=inteiro(num(&r, "observar").unwrap_or(0.0)) unidade="ativos" />
                    <Kpi titulo="Vendas na janela" valor=inteiro(num(&r, "vendas").unwrap_or(0.0))
                        comparacao=format!("{} dias", inteiro(num(&v, "janelaDias").unwrap_or(0.0))) />
                </div>
                <Tabela cab=vec!["Ativo", "Último", "Base", "24h", "7d", "Vol. 24h", "Liquidez", "Sinal", "Confiança"] linhas=linhas />
            }
        })}
    }
}

fn aba_oportunidades(e: Estado, versao: RwSignal<u32>) -> impl IntoView {
    let d = recurso(e, versao, || Some("/api/bolsa/dicas".into()));
    view! {
        {move || carregado(d.get(), |v| {
            let ops = lista(&v, "oportunidades");
            let top: Vec<Value> = ops.iter().take(3).cloned().collect();
            let linhas: Vec<Vec<String>> = ops.iter().map(|o| vec![
                txt(o, "nome"), txt(o, "moeda"), curto(num(o, "unidade").unwrap_or(0.0)),
                curto(num(o, "justo").unwrap_or(0.0)), num(o, "desconto").map_or("—".into(), pct),
                inteiro(num(o, "quantidade").unwrap_or(0.0)), txt(o, "motivo"),
            ]).collect();
            view! {
                <div class="vereditos">
                    {top.into_iter().map(|o| view! {
                        <Veredito titulo=format!("Comprar {}", txt(&o, "nome")) motivo=txt(&o, "motivo")
                            acao=format!("{} {} cada", curto(num(&o, "unidade").unwrap_or(0.0)), txt(&o, "moeda")) tom="bom" />
                    }).collect_view()}
                </div>
                <div class="dica">{format!("{} ofertas ativas analisadas.", inteiro(num(&v, "ofertasAtivas").unwrap_or(0.0)))}</div>
                <Tabela cab=vec!["Oferta", "Moeda", "Unidade", "Justo", "Desconto", "Qtd.", "Motivo"] linhas=linhas />
            }
        })}
    }
}

fn aba_inventario(e: Estado, versao: RwSignal<u32>) -> impl IntoView {
    let i = recurso(e, versao, || Some("/api/bolsa/meus-itens".into()));
    view! {
        {move || carregado(i.get(), |v| {
            let itens = lista(&v, "itens");
            let vender: f64 = itens.iter().filter(|x| txt(x, "acao") == "VENDER").map(|x| num(x, "valorAlvo").unwrap_or(0.0)).sum();
            let linhas: Vec<Vec<String>> = itens.iter().map(|x| vec![
                txt(x, "nome"), inteiro(num(x, "quantidade").unwrap_or(0.0)), curto(num(x, "ultimo").unwrap_or(0.0)),
                curto(num(x, "valor").unwrap_or(0.0)), acao_pt(&txt(x, "acao")), txt(x, "vmot"),
            ]).collect();
            view! {
                <div class="kpis">
                    <Kpi titulo="Valor do inventário" valor=curto(num(&v, "total").unwrap_or(0.0)) unidade=txt_static(&v)
                        ajuda="Itens das contas a último preço da Bolsa." />
                    <Kpi titulo="Para vender" valor=curto(vender) unidade="no alvo"
                        ajuda="Soma do preço-alvo dos itens com sinal de venda." />
                </div>
                <Tabela cab=vec!["Item", "Qtd.", "Preço", "Valor", "Sinal", "Motivo"] linhas=linhas />
            }
        })}
    }
}

fn txt_static(v: &Value) -> &'static str {
    if txt(v, "moeda") == "gold" {
        "gold"
    } else {
        "orbs"
    }
}

#[component]
pub fn Mercado(e: Estado, moeda: RwSignal<String>) -> impl IntoView {
    let versao = RwSignal::new(0u32);
    let aba = RwSignal::new("Pregão");
    view! {
        {cabecalho("Mercado", "O que compro, o que vendo e o que guardo?", versao)}
        <Abas opcoes=ABAS ativa=aba />
        {move || match aba.get() {
            "Pregão" => aba_pregao(e, versao, moeda).into_any(),
            "Oportunidades" => aba_oportunidades(e, versao).into_any(),
            _ => aba_inventario(e, versao).into_any(),
        }}
    }
}
