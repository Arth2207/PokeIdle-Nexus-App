//! Finanças: quanto estou ganhando de verdade, com o quê?

use leptos::prelude::*;
use serde_json::Value;

use super::cabecalho;
use crate::comp::{carregado, Barras, Kpi, Tabela};
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
                ("Loot da caçada".to_owned(), loot, format!("+{}", curto(loot))),
                ("Compras".to_owned(), compras, format!("−{}", curto(compras))),
                ("Consumo".to_owned(), consumo, format!("−{}", curto(consumo))),
                ("Lucro líquido".to_owned(), lucro.max(0.0), curto(lucro)),
            ];
            let mut por_conta: Vec<(String, f64, String)> = lista(&f, "contas").iter().map(|c| {
                let l: f64 = lista(c, "semana").iter().rev().take(n)
                    .map(|d| num(d, "loot").unwrap_or(0.0) - num(d, "compras").unwrap_or(0.0) - num(d, "consumo").unwrap_or(0.0)).sum();
                (txt(c, "nick"), l, curto(l))
            }).collect();
            por_conta.sort_by(|a, b| b.1.total_cmp(&a.1));
            let mut dias: std::collections::BTreeMap<String, (f64, f64)> = Default::default();
            for c in lista(&f, "contas") {
                for d in lista(c, "semana") {
                    let x = dias.entry(txt(d, "dia")).or_default();
                    x.0 += num(d, "loot").unwrap_or(0.0) - num(d, "compras").unwrap_or(0.0) - num(d, "consumo").unwrap_or(0.0);
                    x.1 += num(d, "kills").unwrap_or(0.0);
                }
            }
            let linhas: Vec<Vec<String>> = dias.iter().rev().map(|(d, (l, k))| vec![d.clone(), curto(*l), inteiro(*k)]).collect();
            view! {
                <div class="kpis">
                    <Kpi titulo="Lucro líquido" valor=curto(lucro) unidade="gold" comparacao=comp(lucro, lucro_a) bom=b_lucro
                        ajuda="Loot da caçada − compras − consumo no período, somando as contas." />
                    <Kpi titulo="Receitas" valor=curto(loot) unidade="gold" comparacao=comp(loot, loot_a) bom=b_loot />
                    <Kpi titulo="Custos" valor=curto(compras + consumo) unidade="gold" comparacao=comp(compras + consumo, compras_a + consumo_a) bom=b_custo />
                </div>
                <div class="duas">
                    <div><div class="secao">"De onde veio e para onde foi"</div><Barras linhas=cascata destaque="Lucro líquido".to_owned() /></div>
                    <div><div class="secao">"Lucro por conta"</div><Barras linhas=por_conta /></div>
                </div>
                <div class="secao">"Dia a dia (todas as contas)"</div>
                <Tabela cab=vec!["Dia", "Lucro", "Abates"] linhas=linhas />
            }
        })}
    }
}
