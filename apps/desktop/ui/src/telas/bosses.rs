//! Combate › Bosses (mockups 06 e 07): a grade com o selo de cada boss e, ao escolher um, as
//! duas equipes (ideal e a melhor da conta), a chance, o tempo de luta e por que perde.
//! Chance = cobertura do modelo de combate (dano total ÷ HP do boss), limitada a 100%.

use leptos::prelude::*;
use serde_json::Value;

use super::nick_ativo;
use crate::comp::{carregado, Abas, Kpi, Tabela, Veredito};
use crate::dados::{inteiro, lista, num, recurso, txt};
use crate::Estado;

const FILTROS: &[&str] = &["Todos", "Vence agora", "Com compras", "Fora de alcance"];

/// O selo de um boss da grade (`/api/equipes/resumo`).
pub fn selo(b: &Value) -> &'static str {
    let alcanca = b.get("alcanca").and_then(Value::as_bool).unwrap_or(false);
    if !alcanca {
        "Fora de alcance"
    } else if num(b, "minha").unwrap_or(0.0) >= 1.0 {
        "Vence agora"
    } else if num(b, "ideal").unwrap_or(0.0) >= 1.0 {
        "Com compras"
    } else {
        "Fora de alcance"
    }
}

fn pct(c: f64) -> String {
    format!("{:.0}%", c.min(1.0) * 100.0)
}

fn tipos(x: &Value) -> String {
    lista(x, "tipos")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join("/")
}

/// Tabela de uma equipe do plano: nome, nível, dano, quando cai e a fraqueza.
fn equipe(eq: &Value) -> Vec<Vec<String>> {
    lista(eq, "equipe")
        .iter()
        .map(|x| {
            vec![
                txt(x, "nome"),
                inteiro(num(x, "nivel").unwrap_or(0.0)),
                inteiro(num(x, "D").unwrap_or(0.0)),
                num(x, "tDie").map_or("—".into(), |t| format!("{t:.0} s")),
                x.pointer("/apanha/mult")
                    .and_then(Value::as_f64)
                    .map_or("—".into(), |m| {
                        format!(
                            "×{m} {}",
                            x.pointer("/apanha/tipo")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                        )
                    }),
            ]
        })
        .collect()
}

/// O pokémon que cai primeiro e por quê (o "por que perde" do modelo).
fn por_que(minha: &Value) -> Option<String> {
    let x = lista(minha, "equipe")
        .iter()
        .filter(|x| num(x, "tDie").is_some())
        .min_by(|a, b| {
            num(a, "tDie")
                .unwrap_or(0.0)
                .total_cmp(&num(b, "tDie").unwrap_or(0.0))
        })?;
    let fraco = x
        .pointer("/apanha/mult")
        .and_then(Value::as_f64)
        .filter(|m| *m > 1.0)
        .map(|m| {
            format!(
                ": apanha ×{m} de {}",
                x.pointer("/apanha/tipo")
                    .and_then(Value::as_str)
                    .unwrap_or("")
            )
        })
        .unwrap_or_default();
    Some(format!(
        "{} ({}) cai primeiro, em {:.0} s{fraco}.",
        txt(x, "nome"),
        tipos(x),
        num(x, "tDie").unwrap_or(0.0)
    ))
}

fn detalhe(p: Value) -> AnyView {
    let boss = p.get("boss").cloned().unwrap_or(Value::Null);
    let minha = p.get("minha").cloned().unwrap_or(Value::Null);
    let ideal = p.get("ideal").cloned().unwrap_or(Value::Null);
    let dps: f64 = lista(&minha, "equipe")
        .iter()
        .filter_map(|x| num(x, "dpsOut"))
        .sum();
    let tempo = num(&boss, "hp").filter(|_| dps > 0.0).map(|hp| hp / dps);
    let cm = num(&minha, "cobertura").unwrap_or(0.0);
    view! {
        <div class="secao">{format!("{} · nível {}", txt(&boss, "nome"), inteiro(num(&boss, "teamLevel").unwrap_or(0.0)))}</div>
        <div class="kpis">
            <Kpi titulo="Chance de vitória" valor=pct(cm) comparacao="meta 100%".to_owned() bom=Some(cm >= 1.0)
                ajuda="Dano total da melhor equipe da conta ÷ HP do boss (modelo de combate)." />
            <Kpi titulo="Tempo de luta" valor=tempo.map_or("—".into(), |t| format!("{}m{:02}s", (t / 60.0) as i64, (t % 60.0) as i64))
                ajuda="HP do boss ÷ dano por segundo somado da equipe." />
            <Kpi titulo="Força vs. ideal" valor=pct(num(&minha, "poderVsIdeal").unwrap_or(0.0)) />
            <Kpi titulo="Ideal" valor=pct(num(&ideal, "cobertura").unwrap_or(0.0)) ajuda="Chance da equipe ideal, que independe do que a conta tem." />
        </div>
        {por_que(&minha).filter(|_| cm < 1.0).map(|m| view! {
            <Veredito titulo="Por que perde".to_owned() motivo=m tom="alerta" />
        })}
        <div class="duas" style="margin-top:14px">
            <div class="caixa">
                <div class="caixa-t">{format!("Melhor disponível · {}", pct(cm))}</div>
                <Tabela cab=vec!["Pokémon", "Nível", "Dano", "Cai em", "Fraqueza"] linhas=equipe(&minha) />
            </div>
            <div class="caixa">
                <div class="caixa-t">{format!("Ideal · {}", pct(num(&ideal, "cobertura").unwrap_or(0.0)))}</div>
                <Tabela cab=vec!["Pokémon", "Nível", "Dano", "Cai em", "Fraqueza"] linhas=equipe(&ideal) />
            </div>
        </div>
        <div class="dica">{txt(&p, "nota")}</div>
    }
    .into_any()
}

pub fn aba(e: Estado, versao: RwSignal<u32>) -> impl IntoView {
    let filtro = RwSignal::new("Todos");
    let resumo = recurso(e, versao, move || {
        nick_ativo(e).map(|n| format!("/api/equipes/resumo?nick={n}"))
    });
    let escolhido = RwSignal::new(String::new());
    let plano = recurso(e, versao, move || {
        let b = escolhido.get();
        let n = nick_ativo(e)?;
        (!b.is_empty()).then(|| format!("/api/equipes/plano?nick={n}&boss={b}"))
    });
    view! {
        <Abas opcoes=FILTROS ativa=filtro />
        {move || carregado(resumo.get(), move |v| {
            let todos = lista(&v, "bosses").to_vec();
            let conta = |s: &str| todos.iter().filter(|b| selo(b) == s).count();
            let feitos = todos.iter().filter(|b| b.get("feito").and_then(Value::as_bool) == Some(true)).count();
            let f = filtro.get();
            let vis: Vec<Value> = todos.iter().filter(|b| f == "Todos" || selo(b) == f).cloned().collect();
            view! {
                <div class="kpis">
                    <Kpi titulo="Vence agora" valor=format!("{} de {}", conta("Vence agora"), todos.len()) />
                    <Kpi titulo="Vence com compras" valor=conta("Com compras").to_string() />
                    <Kpi titulo="Fora de alcance" valor=conta("Fora de alcance").to_string() />
                    <Kpi titulo="Já vencidos" valor=feitos.to_string() />
                </div>
                <div class="grade-bosses">
                    {vis.into_iter().map(|b| {
                        let key = txt(&b, "key");
                        let k2 = key.clone();
                        let s = selo(&b);
                        let c = num(&b, "minha").unwrap_or(0.0);
                        let cor = if c >= 1.0 { "var(--melhora)" } else if c >= 0.7 { "var(--neon)" } else { "var(--piora)" };
                        view! {
                            <div class="boss" class:sel=move || escolhido.get() == k2 on:click=move |_| escolhido.set(key.clone())>
                                <img src=txt(&b, "sprite") alt="" loading="lazy" />
                                <div>
                                    <div class="n">{txt(&b, "nome")}</div>
                                    <div class="s">{format!("Nv. {} · {}", inteiro(num(&b, "teamLevel").unwrap_or(0.0)), tipos(&b))}</div>
                                    <span class=format!("selo {}", match s { "Vence agora" => "ok", "Com compras" => "", _ => "nao" })>{s}</span>
                                    <div class="medidor"><b></b><span class="trilho"><span class="val" style=format!("width:{:.0}%;background:{cor}", c.min(1.0) * 100.0)></span></span><span>{pct(c)}</span></div>
                                </div>
                            </div>
                        }
                    }).collect_view()}
                </div>
            }
        })}
        {move || (!escolhido.get().is_empty()).then(|| carregado(plano.get(), detalhe))}
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    #[test]
    fn selos_e_por_que_perde() {
        assert_eq!(selo(&json!({"alcanca": true, "minha": 1.2})), "Vence agora");
        assert_eq!(
            selo(&json!({"alcanca": true, "minha": 0.5, "ideal": 1.3})),
            "Com compras"
        );
        assert_eq!(
            selo(&json!({"alcanca": false, "minha": 2.0})),
            "Fora de alcance"
        );
        let m = json!({"equipe": [
            {"nome": "A", "tipos": ["FIRE"], "tDie": 30, "apanha": {"mult": 2.0, "tipo": "WATER"}},
            {"nome": "B", "tipos": ["ROCK"], "tDie": 7, "apanha": {"mult": 4.0, "tipo": "WATER"}}]});
        assert_eq!(
            por_que(&m).as_deref(),
            Some("B (ROCK) cai primeiro, em 7 s: apanha ×4 de WATER.")
        );
    }
}
