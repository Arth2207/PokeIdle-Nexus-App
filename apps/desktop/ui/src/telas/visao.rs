//! Visão geral: está tudo bem, e o que precisa de mim agora? (mockup `01_visao_geral.png`)

use leptos::prelude::*;
use serde_json::Value;

use super::escaneamento::Escaneamento;
use super::{cabecalho, nick_ativo};
use crate::comp::{carregado, Colunas, Kpi, Veredito};
use crate::dados::{curto, inteiro, lista, no_app, num, recurso, txt};
use crate::Estado;

/// Lucro de um dia do `/api/financeiro`: loot − compras − consumo.
fn lucro(d: &Value) -> Option<f64> {
    d.is_object().then(|| {
        num(d, "loot").unwrap_or(0.0)
            - num(d, "compras").unwrap_or(0.0)
            - num(d, "consumo").unwrap_or(0.0)
    })
}

/// Soma de `hoje` ou `ontem` das contas (`None` se nenhuma tem o dia).
fn soma_dia(f: &Value, k: &str) -> Option<f64> {
    let v: Vec<f64> = lista(f, "contas")
        .iter()
        .filter_map(|c| lucro(c.get(k)?))
        .collect();
    (!v.is_empty()).then(|| v.iter().sum())
}

/// Lucro por dia somando as contas, dos últimos 7 dias com dado.
fn por_dia(f: &Value) -> Vec<(String, f64)> {
    let mut m = std::collections::BTreeMap::<String, f64>::new();
    for c in lista(f, "contas") {
        for d in lista(c, "semana") {
            *m.entry(txt(d, "dia")).or_default() += lucro(d).unwrap_or(0.0);
        }
    }
    let v: Vec<_> = m.into_iter().collect();
    v[v.len().saturating_sub(7)..].to_vec()
}

/// Horas desde a meia-noite de Brasília (UTC−3).
fn horas_do_dia(agora_ms: f64) -> f64 {
    let local = agora_ms - 3.0 * 3.6e6;
    (local.rem_euclid(86_400_000.0) / 3.6e6).max(0.25)
}

fn ouro_texto(t: &str) -> f64 {
    t.chars()
        .filter(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or(0.0)
}

/// Hora local (UTC−3) de um instante em ms.
fn hora(t: f64) -> String {
    let s = ((t / 1000.0) as i64 - 3 * 3600).rem_euclid(86_400);
    format!("{:02}:{:02}", s / 3600, (s % 3600) / 60)
}

#[component]
pub fn VisaoGeral(e: Estado) -> impl IntoView {
    let versao = RwSignal::new(0u32);
    let fin = recurso(e, versao, || Some("/api/financeiro".into()));
    let itens = recurso(e, versao, || Some("/api/bolsa/meus-itens".into()));
    let dicas = recurso(e, versao, || Some("/api/bolsa/dicas".into()));
    let contas = recurso(e, versao, || Some("/api/contas".into()));
    let hist = recurso(e, versao, || Some("/api/historico".into()));
    let pvp = recurso(e, versao, move || {
        nick_ativo(e).map(|n| format!("/api/pvp/sugestao?nick={n}"))
    });
    let ir = move |p: &'static str| move |_| e.pagina.set(p);
    view! {
        {cabecalho("Visão geral", "Está tudo bem, e o que precisa de mim agora?", versao)}
        <div class="kpis">
            {move || carregado(itens.get(), |v| {
                let ouro: f64 = match contas.get() {
                    Some(Some(Ok(c))) => lista(&c, "contas").iter().map(|x| ouro_texto(&txt(x, "gold"))).sum(),
                    _ => 0.0,
                };
                view! {
                    <Kpi titulo="Patrimônio" valor=curto(num(&v, "total").unwrap_or(0.0)) unidade="orbs"
                        ajuda="Inventário das contas pelo valor da Bolsa; o ouro em caixa vai na linha de baixo."
                        comparacao=format!("+ {} gold em caixa · {} itens", curto(ouro), lista(&v, "itens").len()) />
                }
            })}
            {move || carregado(fin.get(), |f| {
                let hoje = soma_dia(&f, "hoje").unwrap_or(0.0);
                let ontem = soma_dia(&f, "ontem");
                let (comp, bom) = match ontem {
                    Some(o) if o > 0.0 => (format!("{:+.0}% vs. ontem", (hoje / o - 1.0) * 100.0), Some(hoje >= o)),
                    _ => ("sem dado de ontem".to_owned(), None),
                };
                let horas = horas_do_dia(num(&f, "agora").unwrap_or(0.0));
                let dias = por_dia(&f);
                let media = if dias.is_empty() { 0.0 } else { dias.iter().map(|d| d.1).sum::<f64>() / dias.len() as f64 / 24.0 };
                view! {
                    <Kpi titulo="Lucro de hoje" valor=curto(hoje) unidade="gold" comparacao=comp bom=bom
                        ajuda="Loot da caçada − compras − consumo, hoje (UTC−3), somando as contas." />
                    <Kpi titulo="Rendimento" valor=curto(hoje / horas) unidade="gold/h"
                        comparacao=format!("média de 7 dias {}", curto(media))
                        bom=Some(hoje / horas >= media)
                        ajuda="Lucro de hoje dividido pelas horas do dia até agora." />
                }
            })}
            {move || carregado(contas.get(), |c| {
                let l = lista(&c, "contas");
                let agora = js_sys::Date::now();
                let vivas = l.iter().filter(|x| agora - num(x, "em").unwrap_or(0.0) < 120_000.0).count();
                let pct = if l.is_empty() { 0.0 } else { vivas as f64 / l.len() as f64 * 100.0 };
                view! {
                    <Kpi titulo="Disponibilidade" valor=format!("{pct:.0}%")
                        comparacao=format!("{vivas} de {} contas mandando dados · meta 95%", l.len())
                        bom=Some(pct >= 95.0)
                        ajuda="Contas cuja janela chegou ao Nexus nos últimos 2 minutos." />
                }
            })}
        </div>
        <div class="vg">
            <div class="caixa">
                <div class="caixa-t">"Escaneamento de campo"</div>
                {move || carregado(contas.get(), |c| {
                    let alvo = nick_ativo(e);
                    let l = lista(&c, "contas");
                    let conta = l.iter().find(|x| Some(txt(x, "conta")) == alvo).or(l.first()).cloned();
                    conta.map(|x| view! { <Escaneamento conta=x /> })
                })}
            </div>
            <div class="caixa">
                <div class="caixa-t">"Precisa de você"</div>
                <div class="vereditos" style="grid-template-columns:1fr">
                    {move || carregado(dicas.get(), |d| {
                        lista(&d, "oportunidades").first().cloned().map(|o| view! {
                            <div class="acao-v">
                                <Veredito titulo=format!("Comprar {}", txt(&o, "nome")) motivo=txt(&o, "motivo")
                                    acao=format!("{} un. a {} {}", inteiro(num(&o, "quantidade").unwrap_or(1.0)), curto(num(&o, "unidade").unwrap_or(0.0)), txt(&o, "moeda")) />
                                <button class="btn prim" on:click=ir("Mercado")>"Comprar"</button>
                            </div>
                        })
                    })}
                    {move || carregado(itens.get(), |v| {
                        lista(&v, "itens").iter().filter(|i| txt(i, "acao") == "VENDER")
                            .max_by(|a, b| num(a, "valor").unwrap_or(0.0).total_cmp(&num(b, "valor").unwrap_or(0.0)))
                            .cloned()
                            .map(|i| view! {
                                <div class="acao-v">
                                    <Veredito titulo=format!("Vender {}", txt(&i, "nome")) motivo=txt(&i, "vmot")
                                        confianca=num(&i, "vconf").unwrap_or(0.0) tom="bom"
                                        acao=format!("~{} orbs", curto(num(&i, "valorAlvo").unwrap_or(0.0))) />
                                    <button class="btn" on:click=ir("Mercado")>"Vender"</button>
                                </div>
                            })
                    })}
                    {move || match pvp.get() {
                        Some(Some(Ok(p))) => {
                            let atual = p.pointer("/atual/casas").cloned().unwrap_or(Value::Null);
                            let nova = p.pointer("/sugestao/casas").cloned().unwrap_or(Value::Null);
                            (atual != nova && !nova.is_null()).then(|| view! {
                                <div class="acao-v">
                                    <Veredito titulo="Formação de PvP nova".to_owned()
                                        motivo=format!("Sugestão contra o meta de {} partidas.", num(&p["sugestao"], "partidas_no_meta").unwrap_or(0.0)) />
                                    <button class="btn" on:click=ir("Combate")>"Ver"</button>
                                </div>
                            }).into_any()
                        }
                        _ => ().into_any(),
                    }}
                </div>
            </div>
        </div>
        <div class="vg-base">
            <div class="caixa">
                <div class="caixa-t">"Lucro por dia · 7 dias"</div>
                {move || carregado(fin.get(), |f| {
                    let linhas = por_dia(&f).into_iter()
                        .map(|(d, v)| (d.get(5..).unwrap_or(&d).to_owned(), v, curto(v)))
                        .collect::<Vec<_>>();
                    view! { <Colunas linhas=linhas /> }
                })}
            </div>
            <div class="caixa">
                <div class="caixa-t">"Diário de eventos"</div>
                {move || carregado(hist.get(), |h| {
                    let acoes: Vec<Value> = lista(&h, "acoes").iter().take(12).cloned().collect();
                    view! {
                        <div class="diario-m">
                            {acoes.into_iter().map(|a| {
                                let ok = a.get("ok").and_then(Value::as_bool).unwrap_or(true);
                                view! {
                                    <div class:falha=!ok>
                                        <span class="h">{hora(num(&a, "t").unwrap_or(0.0))}</span>
                                        <b>{txt(&a, "nick")}</b>
                                        <span>{txt(&a, "texto")}</span>
                                    </div>
                                }
                            }).collect_view()}
                        </div>
                    }
                })}
            </div>
        </div>
        {no_app().then(|| view! { <crate::Diario reg=e.reg /> })}
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    #[test]
    fn contas_somam_por_dia_e_hoje() {
        let f = json!({"contas": [
            {"hoje": {"loot": 10.0, "compras": 2.0}, "ontem": null,
             "semana": [{"dia": "2026-11-13", "loot": 5.0}, {"dia": "2026-11-14", "loot": 10.0}]},
            {"hoje": {"loot": 4.0}, "semana": [{"dia": "2026-11-14", "loot": 4.0, "consumo": 1.0}]}]});
        assert_eq!(soma_dia(&f, "hoje"), Some(12.0));
        assert_eq!(soma_dia(&f, "ontem"), None);
        assert_eq!(
            por_dia(&f),
            vec![("2026-11-13".into(), 5.0), ("2026-11-14".into(), 13.0)]
        );
        assert_eq!(hora(1_794_683_459_014.0), "16:10");
        assert_eq!(ouro_texto("337.717.989"), 337_717_989.0);
    }
}
