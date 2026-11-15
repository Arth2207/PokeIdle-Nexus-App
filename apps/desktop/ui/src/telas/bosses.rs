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

/// Linha do tempo da luta no modelo: cada um dos 5 primeiros luta sozinho até cair
/// (`D` = dano por segundo × tempo até cair) e o próximo entra em seguida.
/// Devolve os pontos (segundo, HP do boss em %) e o segundo da vitória, se vence.
pub fn replay(eq: &Value, hp: f64) -> (Vec<(f64, f64)>, Option<f64>) {
    let mut pts = vec![(0.0, 100.0)];
    if hp <= 0.0 {
        return (pts, None);
    }
    let (mut t, mut resta) = (0.0, hp);
    for x in lista(eq, "equipe").iter().take(5) {
        let (d, td) = (num(x, "D").unwrap_or(0.0), num(x, "tDie").unwrap_or(0.0));
        if td <= 0.0 || d <= 0.0 {
            continue;
        }
        if d >= resta {
            let fim = t + td * resta / d;
            pts.push((fim, 0.0));
            return (pts, Some(fim));
        }
        t += td;
        resta -= d;
        pts.push((t, resta / hp * 100.0));
    }
    (pts, None)
}

fn minutos(t: f64) -> String {
    format!("{}m{:02}s", (t / 60.0) as i64, (t % 60.0) as i64)
}

/// HP do boss ao longo da luta (eixo x em segundos, proporcional ao tempo).
fn grafico_replay(eq: &Value, hp: f64) -> impl IntoView {
    let (pts, fim) = replay(eq, hp);
    let tmax = pts.last().map_or(1.0, |p| p.0).max(1.0);
    let (w, h) = (600.0, 140.0);
    let d: String = pts
        .iter()
        .enumerate()
        .map(|(i, (t, v))| {
            format!(
                "{}{:.1},{:.1}",
                if i == 0 { "M" } else { " L" },
                t / tmax * w,
                6.0 + (100.0 - v) / 100.0 * (h - 12.0)
            )
        })
        .collect();
    let nomes: Vec<String> = lista(eq, "equipe")
        .iter()
        .take(5)
        .map(|x| txt(x, "nome"))
        .collect();
    let fim_txt = fim.map_or_else(
        || {
            format!(
                "Não vence: o boss fica com {:.0}% do HP.",
                pts.last().map_or(100.0, |p| p.1)
            )
        },
        |f| format!("Vence em {}.", minutos(f)),
    );
    view! {
        <svg class="grafico-linha" viewBox=format!("0 0 {w} {h}")>
            <path class="linha-v" d=d fill="none" stroke="var(--neon)" stroke-width="2.5" />
        </svg>
        <div class="linha-rot"><span>"0s"</span><span>{minutos(tmax)}</span></div>
        <div class="dica">{format!("Ordem: {} · {fim_txt} Modelo analítico (sem sorte): cada um luta até cair e o próximo entra.", nomes.join(" → "))}</div>
    }
}

fn detalhe(p: Value) -> AnyView {
    let boss = p.get("boss").cloned().unwrap_or(Value::Null);
    let minha = p.get("minha").cloned().unwrap_or(Value::Null);
    let ideal = p.get("ideal").cloned().unwrap_or(Value::Null);
    let hp = num(&boss, "hp").unwrap_or(0.0);
    let tempo = replay(&minha, hp).1;
    let cm = num(&minha, "cobertura").unwrap_or(0.0);
    view! {
        <div class="secao">{format!("{} · nível {}", txt(&boss, "nome"), inteiro(num(&boss, "teamLevel").unwrap_or(0.0)))}</div>
        <div class="kpis">
            <Kpi titulo="Chance de vitória" valor=pct(cm) comparacao="meta 100%".to_owned() bom=Some(cm >= 1.0)
                ajuda="Dano total da melhor equipe da conta ÷ HP do boss (modelo de combate)." />
            <Kpi titulo="Tempo de luta" valor=tempo.map_or("não vence".into(), minutos)
                ajuda="Pela linha do tempo do modelo: cada um luta até cair e o próximo entra." />
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
        <div class="caixa" style="margin-top:14px">
            <div class="caixa-t">"Replay da luta · melhor disponível"</div>
            {grafico_replay(&minha, hp)}
        </div>
        <div class="dica">{txt(&p, "nota")}</div>
    }
    .into_any()
}

/// Laboratório: monta até 6 pokémon da coleção à mão e vê a chance contra o boss escolhido.
fn laboratorio(e: Estado, versao: RwSignal<u32>, boss: RwSignal<String>) -> impl IntoView {
    let col = recurso(e, versao, move || {
        nick_ativo(e).map(|n| format!("/api/pvp/colecao?nick={n}"))
    });
    let ids = RwSignal::new(Vec::<(String, String)>::new());
    let q = RwSignal::new(String::new());
    let lab = recurso(e, versao, move || {
        let n = nick_ativo(e)?;
        let l = ids.get();
        let b = boss.get();
        (!l.is_empty() && !b.is_empty()).then(|| {
            let v: Vec<String> = l.into_iter().map(|x| x.0).collect();
            format!("/api/equipes/lab?nick={n}&boss={b}&ids={}", v.join(","))
        })
    });
    view! {
        <div class="caixa" style="margin-top:14px">
            <div class="caixa-t">"Laboratório · monte a equipe"</div>
            <div class="chips">
                {move || ids.get().into_iter().map(|(id, nome)| view! {
                    <span class="chip" title="Tirar" style="cursor:pointer" on:click=move |_| ids.update(|l| l.retain(|x| x.0 != id))>{format!("{nome} ✕")}</span>
                }).collect_view()}
            </div>
            <input class="caixa" placeholder="Buscar na coleção…" prop:value=move || q.get() on:input=move |ev| q.set(event_target_value(&ev)) />
            {move || carregado(col.get(), move |c| {
                let f = q.get().to_lowercase();
                let mut l: Vec<Value> = lista(&c, "pokemons").iter()
                    .filter(|x| f.is_empty() || txt(x, "nome").to_lowercase().contains(&f))
                    .cloned().collect();
                l.sort_by(|a, b| num(b, "level").unwrap_or(0.0).total_cmp(&num(a, "level").unwrap_or(0.0)));
                view! {
                    <div class="chips">
                        {l.into_iter().take(30).map(|x| {
                            let id = match x.get("id") { Some(Value::String(t)) => t.clone(), Some(v) => v.to_string(), None => String::new() };
                            let nome = txt(&x, "nome");
                            let rot = format!("{nome} · nv {}", inteiro(num(&x, "level").unwrap_or(0.0)));
                            view! {
                                <span class="chip" style="cursor:pointer" on:click=move |_| ids.update(|v| {
                                    if v.len() < 6 && !v.iter().any(|y| y.0 == id) { v.push((id.clone(), nome.clone())); }
                                })>{rot}</span>
                            }
                        }).collect_view()}
                    </div>
                }
            })}
            {move || (!ids.get().is_empty()).then(|| carregado(lab.get(), |r| {
                let hp = r.pointer("/boss/hp").and_then(Value::as_f64).unwrap_or(0.0);
                let c = num(&r, "cobertura").unwrap_or(0.0);
                view! {
                    <div class="kpis">
                        <Kpi titulo="Chance (modelo)" valor=pct(c) bom=Some(c >= 1.0) ajuda="Dano total da equipe montada ÷ HP do boss." />
                        <Kpi titulo="Tempo de luta" valor=replay(&r, hp).1.map_or("não vence".into(), minutos) />
                    </div>
                    <Tabela cab=vec!["Pokémon", "Nível", "Dano", "Cai em", "Fraqueza"] linhas=equipe(&r) />
                    {grafico_replay(&r, hp)}
                    <div class="dica">"Espécie repetida conta uma vez; a ordem é a do maior dano."</div>
                }
            }))}
        </div>
    }
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
        {move || (!escolhido.get().is_empty()).then(|| view! {
            <div class="sobre janela-fundo" on:click=move |_| escolhido.set(String::new())>
                <div class="janela vidro" on:click=|ev| ev.stop_propagation()>
                    <button class="btn fechar" title="Fechar" on:click=move |_| escolhido.set(String::new())>"✕"</button>
                    {move || carregado(plano.get(), detalhe)}
                    {laboratorio(e, versao, escolhido)}
                </div>
            </div>
        })}
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
        // A tira 60 de 100 em 30 s; B tira o resto: 40 de 80 → metade dos 20 s.
        let eq = json!({"equipe": [{"D": 60, "tDie": 30}, {"D": 80, "tDie": 20}]});
        let (pts, fim) = replay(&eq, 100.0);
        assert_eq!(fim, Some(40.0));
        assert_eq!(pts, vec![(0.0, 100.0), (30.0, 40.0), (40.0, 0.0)]);
        assert_eq!(
            replay(&json!({"equipe": [{"D": 10, "tDie": 5}]}), 100.0).1,
            None
        );
    }
}
