//! Finanças: quanto estou ganhando de verdade, com o quê?

use leptos::prelude::*;
use serde_json::Value;

use super::cabecalho;
use crate::comp::{carregado, Barras, Cascata, Kpi, Linha, Tabela};
use crate::dados::{curto, inteiro, lista, num, recurso, txt};
use crate::Estado;

/// Soma de um campo nos últimos `n` dias de cada conta (e o mesmo nos `n` anteriores).
fn somar(f: &Value, k: &str, n: usize) -> (f64, f64) {
    let (mut agora, mut antes) = (0.0, 0.0);
    for c in lista(f, "contas") {
        let s = lista(c, "semana");
        for (i, d) in s.iter().rev().enumerate() {
            let x = num(d, k).unwrap_or(0.0);
            if i < n {
                agora += x;
            } else if i < 2 * n {
                antes += x;
            }
        }
    }
    (agora, antes)
}

#[component]
pub fn Financas(e: Estado, periodo: RwSignal<String>) -> impl IntoView {
    let versao = RwSignal::new(0u32);
    let fin = recurso(e, versao, || Some("/api/financeiro".into()));
    let hist = recurso(e, versao, || Some("/api/historico".into()));
    view! {
        {cabecalho("Finanças", "Quanto estou ganhando de verdade, com o quê?", versao)}
        {move || carregado(fin.get(), move |f| {
            let n = match periodo.get().as_str() {
                "Hoje" => 1,
                "Últimos 30 dias" => 30,
                _ => 7,
            };
            let (loot, loot_a) = somar(&f, "loot", n);
            let (compras, compras_a) = somar(&f, "compras", n);
            let (consumo, consumo_a) = somar(&f, "consumo", n);
            let lucro = loot - compras - consumo;
            let lucro_a = loot_a - compras_a - consumo_a;
            let tem_antes = loot_a + compras_a + consumo_a > 0.0;
            let (b_lucro, b_loot, b_custo) = (
                tem_antes.then_some(lucro >= lucro_a),
                tem_antes.then_some(loot >= loot_a),
                tem_antes.then_some(compras + consumo <= compras_a + consumo_a),
            );
            let comp = |a: f64, b: f64| if b.abs() > 0.0 { format!("{:+.0}% vs período anterior", (a / b - 1.0) * 100.0) } else { "sem período anterior".into() };
            let cascata = vec![
                ("Loot da caçada".to_owned(), loot),
                ("Compras".to_owned(), -compras),
                ("Consumo".to_owned(), -consumo),
            ];
            let mut por_conta: Vec<(String, f64, String)> = lista(&f, "contas").iter().map(|c| {
                let l: f64 = lista(c, "semana").iter().rev().take(n)
                    .map(|d| num(d, "loot").unwrap_or(0.0) - num(d, "compras").unwrap_or(0.0) - num(d, "consumo").unwrap_or(0.0)).sum();
                (txt(c, "nick"), l, curto(l))
            }).collect();
            por_conta.sort_by(|a, b| b.1.total_cmp(&a.1));
            // dia → (lucro, abates, ouro no fim do dia somando as contas).
            let mut dias: std::collections::BTreeMap<String, (f64, f64, f64)> = Default::default();
            for c in lista(&f, "contas") {
                for d in lista(c, "semana") {
                    let x = dias.entry(txt(d, "dia")).or_default();
                    x.0 += num(d, "loot").unwrap_or(0.0) - num(d, "compras").unwrap_or(0.0) - num(d, "consumo").unwrap_or(0.0);
                    x.1 += num(d, "kills").unwrap_or(0.0);
                    x.2 += d.pointer("/saldo/fim").and_then(Value::as_f64).unwrap_or(0.0);
                }
            }
            let saldos: Vec<(String, f64)> = dias.iter().filter(|d| d.1 .2 > 0.0).map(|(d, x)| (d.get(5..).unwrap_or(d).to_owned(), x.2)).collect();
            let (ouro_ini, ouro_fim) = {
                let ult: Vec<f64> = saldos.iter().rev().take(n).map(|s| s.1).collect();
                (ult.last().copied().unwrap_or(0.0), ult.first().copied().unwrap_or(0.0))
            };
            let max_lucro = dias.values().map(|x| x.0.abs()).fold(1.0_f64, f64::max);
            let cal: Vec<(String, f64)> = dias.iter().rev().take(28).map(|(d, x)| (d.get(8..).unwrap_or(d).to_owned(), x.0)).collect();
            let extrato: Vec<Vec<String>> = hist.get().and_then(|h| h.and_then(Result::ok)).map(|h| {
                lista(&h, "acoes").iter()
                    .filter(|a| matches!(txt(a, "tipo").as_str(), "compra" | "venda" | "depot" | "pocao"))
                    .take(30)
                    .map(|a| vec![txt(a, "nick"), txt(a, "tipo"), txt(a, "texto")])
                    .collect()
            }).unwrap_or_default();
            let linhas: Vec<Vec<String>> = dias.iter().rev().map(|(d, (l, k, o))| vec![d.clone(), curto(*l), inteiro(*k), curto(*o)]).collect();
            view! {
                <div class="kpis">
                    <Kpi titulo="Lucro líquido" valor=curto(lucro) unidade="gold" comparacao=comp(lucro, lucro_a) bom=b_lucro
                        ajuda="Loot da caçada − compras − consumo no período, somando as contas." />
                    <Kpi titulo="Receitas" valor=curto(loot) unidade="gold" comparacao=comp(loot, loot_a) bom=b_loot />
                    <Kpi titulo="Custos" valor=curto(compras + consumo) unidade="gold" comparacao=comp(compras + consumo, compras_a + consumo_a) bom=b_custo />
                    <Kpi titulo="Ouro em caixa" valor=curto(ouro_fim) unidade="gold"
                        comparacao=format!("início do período {}", curto(ouro_ini)) bom=Some(ouro_fim >= ouro_ini)
                        ajuda="Saldo de ouro das contas no fim do último dia do período, contra o do primeiro." />
                </div>
                <div class="duas">
                    <div class="caixa"><div class="caixa-t">"De onde veio e para onde foi"</div><Cascata passos=cascata /></div>
                    <div class="caixa"><div class="caixa-t">"Lucro por conta"</div><Barras linhas=por_conta /></div>
                </div>
                <div class="duas" style="margin-top:16px">
                    <div class="caixa"><div class="caixa-t">"Ouro em caixa ao longo do tempo"</div><Linha pontos=saldos /></div>
                    <div class="caixa"><div class="caixa-t">"Calendário do lucro"</div>
                        <div class="cal">{cal.into_iter().rev().map(|(d, l)| { let neg = l < 0.0; view! {
                            <div class:neg=neg style=format!("opacity:{:.2}", 0.15 + 0.85 * l.abs() / max_lucro) title=curto(l)>{d}</div>
                        }}).collect_view()}</div>
                    </div>
                </div>
                <div class="secao">"Dia a dia (todas as contas)"</div>
                <Tabela cab=vec!["Dia", "Lucro", "Abates", "Ouro no fim"] linhas=linhas />
                <div class="secao">"Extrato (compras, vendas, coleção e poções do motor)"</div>
                <Tabela cab=vec!["Conta", "Tipo", "O que houve"] linhas=extrato />
            }
        })}
    }
}
