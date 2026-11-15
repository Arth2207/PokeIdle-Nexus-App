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

/// Um item do inventário avaliado na moeda em que ele de fato gira.
#[derive(Clone, Debug, PartialEq)]
pub struct Avaliado {
    pub nome: String,
    pub qtd: f64,
    /// "gold" ou "orb".
    pub moeda: &'static str,
    pub unit: f64,
    pub valor: f64,
    pub acao: String,
    pub motivo: String,
    pub sprite: String,
}

/// Avalia os itens de `/api/bolsa/meus-itens` pelos tickers de `/api/bolsa/mercado`: cada item
/// na moeda com mais vendas em 30 dias, ao valor justo (`base`, ou o último preço). Sem ticker,
/// fica o valor que a API deu (em orbs). Somar tudo numa moeda só misturava gold e orbs.
pub fn avaliar(itens: &[Value], tickers: &[Value]) -> Vec<Avaliado> {
    itens
        .iter()
        .map(|x| {
            let nome = txt(x, "nome");
            let qtd = num(x, "quantidade").unwrap_or(0.0);
            let t = tickers
                .iter()
                .filter(|t| txt(t, "nome") == nome)
                .max_by(|a, b| {
                    num(a, "vol30d")
                        .unwrap_or(0.0)
                        .total_cmp(&num(b, "vol30d").unwrap_or(0.0))
                });
            let (moeda, unit, sprite) = match t {
                Some(t) => (
                    if txt(t, "moeda") == "gold" {
                        "gold"
                    } else {
                        "orb"
                    },
                    num(t, "base").or_else(|| num(t, "ultimo")).unwrap_or(0.0),
                    txt(t, "sprite"),
                ),
                None => (
                    "orb",
                    if qtd > 0.0 {
                        num(x, "valor").unwrap_or(0.0) / qtd
                    } else {
                        0.0
                    },
                    String::new(),
                ),
            };
            Avaliado {
                nome,
                qtd,
                moeda,
                unit,
                valor: unit * qtd,
                acao: txt(x, "acao"),
                motivo: txt(x, "vmot"),
                sprite,
            }
        })
        .collect()
}

/// (total em gold, total em orbs) do inventário avaliado.
pub fn totais(v: &[Avaliado]) -> (f64, f64) {
    v.iter().fold((0.0, 0.0), |(g, o), a| {
        if a.moeda == "gold" {
            (g + a.valor, o)
        } else {
            (g, o + a.valor)
        }
    })
}

fn aba_inventario(e: Estado, versao: RwSignal<u32>) -> impl IntoView {
    let i = recurso(e, versao, || Some("/api/bolsa/meus-itens".into()));
    let m = recurso(e, versao, || Some("/api/bolsa/mercado".into()));
    view! {
        {move || carregado(i.get(), move |v| carregado(m.get(), move |mk| {
            let mut av = avaliar(lista(&v, "itens"), lista(&mk, "tickers"));
            av.sort_by(|a, b| (b.moeda == "gold").cmp(&(a.moeda == "gold")).then(b.valor.total_cmp(&a.valor)));
            let (g, o) = totais(&av);
            let vender: Vec<&Avaliado> = av.iter().filter(|a| a.acao == "VENDER").collect();
            let (vg, vo) = vender.iter().fold((0.0, 0.0), |(g, o), a| if a.moeda == "gold" { (g + a.valor, o) } else { (g, o + a.valor) });
            let linhas: Vec<Vec<String>> = av.iter().map(|a| vec![
                a.nome.clone(), inteiro(a.qtd), if a.moeda == "gold" { "gold".into() } else { "orbs".into() },
                curto(a.unit), curto(a.valor), acao_pt(&a.acao), a.motivo.clone(),
            ]).collect();
            view! {
                <div class="kpis">
                    <Kpi titulo="Itens em gold" valor=curto(g) unidade="gold" ajuda="Itens que giram em gold, ao valor justo da Bolsa." />
                    <Kpi titulo="Itens em orbs" valor=curto(o) unidade="orbs" ajuda="Itens que giram em orbs, ao valor justo da Bolsa." />
                    <Kpi titulo="Para vender" valor=format!("{} gold · {} orbs", curto(vg), curto(vo)) comparacao=format!("{} itens com sinal de venda", vender.len()) />
                </div>
                <Tabela cab=vec!["Item", "Qtd.", "Moeda", "Valor justo (un.)", "Total", "Sinal", "Motivo"] linhas=linhas />
                <div class="dica">"Cada item é avaliado na moeda em que mais vende (vendas de 30 dias); gold e orbs nunca se somam."</div>
            }
        }))}
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
            "Pregão" => super::pregao::aba(e, versao, moeda).into_any(),
            "Oportunidades" => aba_oportunidades(e, versao).into_any(),
            _ => aba_inventario(e, versao).into_any(),
        }}
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    #[test]
    fn item_vai_na_moeda_que_mais_vende() {
        let itens = json!([{"nome": "Leaf Stone", "quantidade": 2, "valor": 1026}, {"nome": "Raro", "quantidade": 4, "valor": 40}]);
        let t = json!([
            {"nome": "Leaf Stone", "moeda": "gold", "base": 87000.0, "vol30d": 2359},
            {"nome": "Leaf Stone", "moeda": "orb", "base": 96.5, "vol30d": 18}]);
        let av = avaliar(itens.as_array().unwrap(), t.as_array().unwrap());
        assert_eq!((av[0].moeda, av[0].valor), ("gold", 174_000.0));
        assert_eq!((av[1].moeda, av[1].unit), ("orb", 10.0));
        assert_eq!(totais(&av), (174_000.0, 40.0));
    }
}
