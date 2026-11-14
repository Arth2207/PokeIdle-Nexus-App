//! Visão geral: está tudo bem, e o que precisa de mim agora?

use leptos::prelude::*;
use serde_json::Value;

use super::{cabecalho, nick_ativo};
use crate::comp::{carregado, Kpi, Veredito};
use crate::dados::{curto, inteiro, lista, num, recurso, txt};
use crate::Estado;

/// O dia mais recente com dado no `/api/financeiro` e o registro de cada conta nesse dia.
fn ultimo_dia(f: &Value) -> (String, Vec<Value>) {
    let dia = lista(f, "contas")
        .iter()
        .filter_map(|c| lista(c, "semana").last().map(|d| txt(d, "dia")))
        .max()
        .unwrap_or_default();
    let regs = lista(f, "contas")
        .iter()
        .filter_map(|c| {
            lista(c, "semana")
                .iter()
                .rev()
                .find(|d| txt(d, "dia") == dia)
                .cloned()
        })
        .collect();
    (dia, regs)
}

#[component]
pub fn VisaoGeral(e: Estado) -> impl IntoView {
    let versao = RwSignal::new(0u32);
    let fin = recurso(e, versao, || Some("/api/financeiro".into()));
    let itens = recurso(e, versao, || Some("/api/bolsa/meus-itens".into()));
    let dicas = recurso(e, versao, || Some("/api/bolsa/dicas".into()));
    let pvp = recurso(e, versao, move || {
        nick_ativo(e).map(|n| format!("/api/pvp/sugestao?nick={n}"))
    });
    let rodando = move || {
        let c = e.cards.get();
        let ok = c.iter().filter(|x| x.status == "execucao").count();
        (ok, c.len())
    };
    view! {
        {cabecalho("Visão geral", "Está tudo bem, e o que precisa de mim agora?", versao)}
        <div class="kpis">
            {move || {
                let (ok, n) = rodando();
                let todas = n > 0 && ok == n;
                view! { <Kpi titulo="Contas rodando" valor=format!("{ok} de {n}")
                    ajuda="Janelas do jogo com captura saudável agora (coletor respondendo e WS ligado)."
                    comparacao=if n == 0 { "Conecte em Configurações".to_owned() } else if ok == n { "todas no ar".to_owned() } else { format!("{} com atenção", n - ok) }
                    bom=Some(todas) /> }
            }}
            {move || carregado(fin.get(), |f| {
                let (dia, dias) = ultimo_dia(&f);
                let hoje = txt(&f, "hoje");
                let quando = if dia == hoje { "hoje".to_owned() } else { format!("em {dia} (último dia com dado)") };
                let soma = |k: &str| dias.iter().map(|d| num(d, k).unwrap_or(0.0)).sum::<f64>();
                let lucro = soma("loot") - soma("compras") - soma("consumo");
                let kills = soma("kills");
                view! {
                    <Kpi titulo="Lucro do dia" valor=curto(lucro) unidade="gold"
                        ajuda="Loot da caçada − compras − consumo, somando as contas."
                        comparacao=format!("{quando}, {} contas", dias.len()) />
                    <Kpi titulo="Abates do dia" valor=inteiro(kills) unidade="kills"
                        ajuda="Abates do dia (UTC−3), somando as contas." comparacao=quando.clone() />
                }
            })}
            {move || carregado(itens.get(), |v| view! {
                <Kpi titulo="Inventário" valor=curto(num(&v, "total").unwrap_or(0.0)) unidade="orbs"
                    ajuda="Itens das contas avaliados pelo último preço da Bolsa."
                    comparacao=format!("{} itens", lista(&v, "itens").len()) />
            })}
        </div>
        <div class="secao">"Precisa de você"</div>
        <div class="vereditos">
            {move || carregado(dicas.get(), |d| {
                let op = lista(&d, "oportunidades").first().cloned();
                op.map(|o| view! {
                    <Veredito titulo=format!("Comprar {}", txt(&o, "nome"))
                        motivo=txt(&o, "motivo")
                        acao=format!("{} un. a {} {} cada", inteiro(num(&o, "quantidade").unwrap_or(1.0)), curto(num(&o, "unidade").unwrap_or(0.0)), txt(&o, "moeda")) />
                })
            })}
            {move || carregado(itens.get(), |v| {
                let melhor = lista(&v, "itens").iter()
                    .filter(|i| txt(i, "acao") == "VENDER")
                    .max_by(|a, b| num(a, "valor").unwrap_or(0.0).total_cmp(&num(b, "valor").unwrap_or(0.0)))
                    .cloned();
                melhor.map(|i| view! {
                    <Veredito titulo=format!("Vender {}", txt(&i, "nome")) motivo=txt(&i, "vmot")
                        confianca=num(&i, "vconf").unwrap_or(0.0)
                        acao=format!("~{} orbs", curto(num(&i, "valorAlvo").unwrap_or(0.0))) tom="bom" />
                })
            })}
            {move || match pvp.get() {
                Some(Some(Ok(p))) => {
                    let atual = p.pointer("/atual/casas").cloned().unwrap_or(Value::Null);
                    let nova = p.pointer("/sugestao/casas").cloned().unwrap_or(Value::Null);
                    (atual != nova && !nova.is_null()).then(|| view! {
                        <Veredito titulo="Formação de PvP nova".to_owned()
                            motivo=format!("Sugestão contra o meta de {} partidas.", num(&p["sugestao"], "partidas_no_meta").unwrap_or(0.0))
                            acao="Ver em Combate › PvP".to_owned() />
                    }).into_any()
                }
                _ => ().into_any(),
            }}
        </div>
        <div class="secao">"Contas"</div>
        <div class="grade-cards">
            {move || e.cards.get().into_iter().map(|c| view! {
                <div class="mini">
                    <div class="n">{c.conta.clone()}</div>
                    <div class="s">{c.hunt.clone().unwrap_or_else(|| "—".into())}</div>
                    <span class=format!("stat {}", c.status)>{c.status.clone()}</span>
                </div>
            }).collect_view()}
        </div>
        <crate::Diario reg=e.reg />
    }
}
