//! Finanças: quanto estou ganhando de verdade, com o quê? Gold e orbs nunca se somam: o gold
//! vem do diário do jogo (caçada e loja) e do Mercado em gold; as orbs, só do Mercado.
//! Segue o seletor do topo: uma conta ou "Todas as contas".

use leptos::prelude::*;
use serde_json::Value;

use super::{cabecalho, nick_ativo, rotulo_ativo, todas};
use crate::comp::{carregado, Barras, Cascata, Kpi, Linha, Tabela};
use crate::dados::{curto, inteiro, lista, num, recurso, txt};
use crate::Estado;

/// Lucro de gold de um dia do diário: loot − compras na loja − consumo.
fn lucro_dia(d: &Value) -> f64 {
    num(d, "loot").unwrap_or(0.0)
        - num(d, "compras").unwrap_or(0.0)
        - num(d, "consumo").unwrap_or(0.0)
}

/// Soma de um campo nos últimos `n` dias de uma conta (e o mesmo nos `n` anteriores).
fn somar(c: &Value, k: &str, n: usize) -> (f64, f64) {
    let (mut agora, mut antes) = (0.0, 0.0);
    for (i, d) in lista(c, "semana").iter().rev().enumerate() {
        let x = if k == "lucro" {
            lucro_dia(d)
        } else {
            num(d, k).unwrap_or(0.0)
        };
        if i < n {
            agora += x;
        } else if i < 2 * n {
            antes += x;
        }
    }
    (agora, antes)
}

/// Totais do Mercado por moeda: (vendas gold, compras gold, vendas orbs, compras orbs).
/// Venda conta o `valor` (o que entrou, já sem a taxa); compra conta o `bruto` (o que saiu).
pub fn mercado_por_moeda(linhas: &[&Value]) -> (f64, f64, f64, f64) {
    linhas
        .iter()
        .fold((0.0, 0.0, 0.0, 0.0), |(vg, cg, vo, co), l| {
            let venda = txt(l, "lado") == "venda";
            let v = if venda {
                num(l, "valor")
            } else {
                num(l, "bruto").or_else(|| num(l, "valor"))
            }
            .unwrap_or(0.0);
            match (txt(l, "moeda") == "gold", venda) {
                (true, true) => (vg + v, cg, vo, co),
                (true, false) => (vg, cg + v, vo, co),
                (false, true) => (vg, cg, vo + v, co),
                (false, false) => (vg, cg, vo, co + v),
            }
        })
}

/// Data "dd/mm hh:mm" (UTC−3) de um instante em ms.
fn quando(t: f64) -> String {
    let s = (t / 1000.0) as i64 - 3 * 3600;
    let dia = s.div_euclid(86_400);
    let (h, m) = (s.rem_euclid(86_400) / 3600, s.rem_euclid(3600) / 60);
    let z = dia + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mes = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{d:02}/{mes:02} {h:02}:{m:02}")
}

#[component]
pub fn Financas(e: Estado, periodo: RwSignal<String>) -> impl IntoView {
    let versao = RwSignal::new(0u32);
    let fin = recurso(e, versao, || Some("/api/financeiro".into()));
    let merc = recurso(e, versao, move || {
        let d = match periodo.get().as_str() {
            "Hoje" => 1,
            "Últimos 30 dias" => 30,
            _ => 7,
        };
        Some(format!("/api/financas/mercado?dias={d}"))
    });
    view! {
        {cabecalho("Finanças", "Quanto estou ganhando de verdade, com o quê?", versao)}
        {move || carregado(fin.get(), move |f| {
            let n = match periodo.get().as_str() {
                "Hoje" => 1,
                "Últimos 30 dias" => 30,
                _ => 7,
            };
            let tudo = todas(e);
            let nick = nick_ativo(e).unwrap_or_default();
            let rotulo = rotulo_ativo(e).unwrap_or_default();
            let contas: Vec<Value> = lista(&f, "contas").iter()
                .filter(|c| tudo || txt(c, "nick").eq_ignore_ascii_case(&nick))
                .cloned().collect();
            let mlinhas: Vec<Value> = match merc.get() {
                Some(Some(Ok(m))) => lista(&m, "linhas").iter()
                    .filter(|l| tudo || txt(l, "conta").eq_ignore_ascii_case(&rotulo))
                    .cloned().collect(),
                _ => Vec::new(),
            };
            let refs: Vec<&Value> = mlinhas.iter().collect();
            let (vg, cg, vo, co) = mercado_por_moeda(&refs);
            let soma = |k: &str| contas.iter().fold((0.0, 0.0), |a, c| { let x = somar(c, k, n); (a.0 + x.0, a.1 + x.1) });
            let (loot, loot_a) = soma("loot");
            let (loja, loja_a) = soma("compras");
            let (consumo, consumo_a) = soma("consumo");
            let lucro_gold = loot - loja - consumo + vg - cg;
            let lucro_a = loot_a - loja_a - consumo_a;
            let caixa: f64 = contas.iter().map(|c| num(c, "ouroAgora").unwrap_or(0.0)).sum();
            let comp = |a: f64, b: f64| if b.abs() > 0.0 { Some(format!("{:+.0}% vs período anterior", (a / b - 1.0) * 100.0)) } else { None };
            let cascata = vec![
                ("Caçada (loot)".to_owned(), loot),
                ("Vendas no Mercado".to_owned(), vg),
                ("Compras no Mercado".to_owned(), -cg),
                ("Loja do jogo".to_owned(), -loja),
                ("Consumo".to_owned(), -consumo),
            ];
            // Por conta: cada moeda na sua coluna.
            let nicks = e.nicks.get();
            let mut por_conta: Vec<(String, f64)> = Vec::new();
            let tabela: Vec<Vec<String>> = contas.iter()
                .map(|c| {
                    let nome = txt(c, "nick");
                    let rot = nicks.iter().find(|(_, n)| n.eq_ignore_ascii_case(&nome)).map_or(nome.to_lowercase(), |(r, _)| r.to_lowercase());
                    let dela: Vec<&Value> = mlinhas.iter().filter(|l| txt(l, "conta").eq_ignore_ascii_case(&rot)).collect();
                    let (vg, cg, vo, co) = mercado_por_moeda(&dela);
                    let caca = somar(c, "lucro", n).0;
                    por_conta.push((nome.clone(), caca + vg - cg));
                    vec![nome, curto(num(c, "ouroAgora").unwrap_or(0.0)), curto(caca), curto(vg - cg), curto(vo - co), inteiro(somar(c, "kills", n).0)]
                }).collect();
            por_conta.sort_by(|a, b| b.1.total_cmp(&a.1));
            let barras: Vec<(String, f64, String)> = por_conta.into_iter().map(|(n, v)| (n, v, format!("{} gold", curto(v)))).collect();
            // Ouro em caixa no tempo e calendário do lucro em gold.
            let mut dias: std::collections::BTreeMap<String, (f64, f64)> = Default::default();
            for c in &contas {
                for d in lista(c, "semana") {
                    let x = dias.entry(txt(d, "dia")).or_default();
                    x.0 += lucro_dia(d);
                    x.1 += d.pointer("/saldo/fim").and_then(Value::as_f64).unwrap_or(0.0);
                }
            }
            let saldos: Vec<(String, f64)> = dias.iter().filter(|d| d.1 .1 > 0.0).map(|(d, x)| (d.get(5..).unwrap_or(d).to_owned(), x.1)).collect();
            let max_lucro = dias.values().map(|x| x.0.abs()).fold(1.0_f64, f64::max);
            let cal: Vec<(String, f64)> = dias.iter().rev().take(28).map(|(d, x)| (d.get(8..).unwrap_or(d).to_owned(), x.0)).collect();
            let mut ord = mlinhas.clone();
            ord.sort_by(|a, b| num(b, "em").unwrap_or(0.0).total_cmp(&num(a, "em").unwrap_or(0.0)));
            let extrato: Vec<Vec<String>> = ord.iter().take(60).map(|l| {
                let venda = txt(l, "lado") == "venda";
                let v = if venda { num(l, "valor") } else { num(l, "bruto").or_else(|| num(l, "valor")) }.unwrap_or(0.0);
                vec![quando(num(l, "em").unwrap_or(0.0)), txt(l, "conta"), if venda { "Venda".into() } else { "Compra".into() },
                     txt(l, "descricao"), format!("{}{}", if venda { "+" } else { "−" }, curto(v)), if txt(l, "moeda") == "gold" { "gold".into() } else { "orbs".into() }]
            }).collect();
            let sem_mercado = mlinhas.is_empty();
            view! {
                <div class="secao">"Gold"</div>
                <div class="kpis">
                    <Kpi titulo="Lucro em gold" valor=curto(lucro_gold) unidade="gold" comparacao=comp(lucro_gold, lucro_a).unwrap_or_else(|| "sem período anterior".into()) bom=Some(lucro_gold >= lucro_a)
                        ajuda="Loot da caçada + vendas em gold no Mercado − compras em gold no Mercado − loja − consumo." />
                    <Kpi titulo="Caçada (loot)" valor=curto(loot) unidade="gold" comparacao=comp(loot, loot_a).unwrap_or_else(|| "sem período anterior".into()) />
                    <Kpi titulo="Custos em gold" valor=curto(loja + consumo + cg) unidade="gold" comparacao=format!("loja {} · consumo {} · mercado {}", curto(loja), curto(consumo), curto(cg)) />
                    <Kpi titulo="Ouro em caixa" valor=curto(caixa) unidade="gold" ajuda="Saldo de ouro agora." />
                </div>
                <div class="secao">"Orbs (Mercado)"</div>
                <div class="kpis">
                    <Kpi titulo="Saldo em orbs" valor=curto(vo - co) unidade="orbs" bom=Some(vo >= co) ajuda="Vendas em orbs (já sem a taxa) − compras em orbs no período." />
                    <Kpi titulo="Vendas em orbs" valor=curto(vo) unidade="orbs" />
                    <Kpi titulo="Compras em orbs" valor=curto(co) unidade="orbs" />
                </div>
                {sem_mercado.then(|| view! { <div class="dica">"Ainda sem extrato do Mercado no período: o app novo lê vendas e compras de cada conta a cada 30 min."</div> })}
                <div class="duas" style="margin-top:16px">
                    <div class="caixa"><div class="caixa-t">"De onde veio e para onde foi o gold"</div><Cascata passos=cascata /></div>
                    <div class="caixa"><div class="caixa-t">"Lucro em gold por conta"</div><Barras linhas=barras /></div>
                </div>
                <div class="secao">"Por conta"</div>
                <Tabela cab=vec!["Conta", "Ouro em caixa", "Caçada e loja (gold)", "Mercado (gold)", "Mercado (orbs)", "Abates"] linhas=tabela />
                <div class="duas" style="margin-top:16px">
                    <div class="caixa"><div class="caixa-t">"Ouro em caixa ao longo do tempo"</div><Linha pontos=saldos /></div>
                    <div class="caixa"><div class="caixa-t">"Calendário do lucro em gold"</div>
                        <div class="cal">{cal.into_iter().rev().map(|(d, l)| { let neg = l < 0.0; view! {
                            <div class:neg=neg style=format!("opacity:{:.2}", 0.15 + 0.85 * l.abs() / max_lucro) title=curto(l)>{d}</div>
                        }}).collect_view()}</div>
                    </div>
                </div>
                <div class="secao">"Extrato do Mercado"</div>
                <Tabela cab=vec!["Quando", "Conta", "Lado", "Item", "Valor", "Moeda"] linhas=extrato />
            }
        })}
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    #[test]
    fn mercado_separa_moedas_e_lados() {
        let l = json!([
            {"lado": "venda", "moeda": "orb", "valor": 76, "bruto": 90},
            {"lado": "venda", "moeda": "gold", "valor": 166_070, "bruto": 195_377},
            {"lado": "compra", "moeda": "gold", "valor": 372_866, "bruto": 438_666},
            {"lado": "compra", "moeda": "orb", "valor": 8}]);
        let r: Vec<&Value> = l.as_array().unwrap().iter().collect();
        assert_eq!(mercado_por_moeda(&r), (166_070.0, 438_666.0, 76.0, 8.0));
        assert_eq!(quando(1_794_600_183_291.0), "13/11 17:03");
    }
}
