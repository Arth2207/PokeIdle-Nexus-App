//! Mercado › Pregão (mockup 09): a lista de ativos, o gráfico do escolhido (velas por dia,
//! média de 7 dias, valor justo e volume) e o veredito com os motivos.

use leptos::prelude::*;
use serde_json::Value;

use crate::comp::carregado;
use crate::dados::{curto, inteiro, lista, num, pct, recurso, txt};
use crate::Estado;

/// Vela de um dia: (dia "dd/mm", abertura, máxima, mínima, fechamento, vendas).
type Vela = (String, f64, f64, f64, f64, usize);

/// Vendas `[t, preço]` → velas por dia (UTC−3), em ordem.
pub fn velas(vendas: &[Value]) -> Vec<Vela> {
    let mut v: Vec<(i64, f64)> = vendas
        .iter()
        .filter_map(|x| Some((x.get(0)?.as_f64()? as i64, x.get(1)?.as_f64()?)))
        .filter(|(_, p)| p.is_finite() && *p > 0.0)
        .collect();
    v.sort_by_key(|x| x.0);
    let mut s: Vec<(i64, Vela)> = Vec::new();
    for (t, p) in v {
        let dia = (t - 3 * 3_600_000).div_euclid(86_400_000);
        match s.last_mut() {
            Some((d, c)) if *d == dia => {
                c.2 = c.2.max(p);
                c.3 = c.3.min(p);
                c.4 = p;
                c.5 += 1;
            }
            _ => {
                // Dia civil a partir dos dias desde 1970 (algoritmo de Howard Hinnant).
                let z = dia + 719_468;
                let era = z.div_euclid(146_097);
                let doe = z - era * 146_097;
                let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
                let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
                let mp = (5 * doy + 2) / 153;
                let d = doy - (153 * mp + 2) / 5 + 1;
                let m = if mp < 10 { mp + 3 } else { mp - 9 };
                s.push((dia, (format!("{d:02}/{m:02}"), p, p, p, p, 1)));
            }
        }
    }
    s.into_iter().map(|x| x.1).collect()
}

fn grafico(vs: &[Vela], justo: Option<f64>) -> impl IntoView {
    let (w, h, hv) = (640.0, 220.0, 50.0);
    let lo = vs.iter().map(|v| v.3).chain(justo).fold(f64::MAX, f64::min);
    let hi = vs.iter().map(|v| v.2).chain(justo).fold(f64::MIN, f64::max);
    let faixa = (hi - lo).max(hi * 0.02).max(1e-9);
    let y = |p: f64| 8.0 + (hi - p) / faixa * (h - 16.0);
    let n = vs.len().max(1) as f64;
    let larg = w / n;
    let vmax = vs.iter().map(|v| v.5).max().unwrap_or(1).max(1) as f64;
    let media: String = vs
        .iter()
        .enumerate()
        .map(|(i, _)| {
            let ini = i.saturating_sub(6);
            let m = vs[ini..=i].iter().map(|v| v.4).sum::<f64>() / (i - ini + 1) as f64;
            format!(
                "{}{:.1},{:.1}",
                if i == 0 { "M" } else { " L" },
                larg * (i as f64 + 0.5),
                y(m)
            )
        })
        .collect();
    view! {
        <svg class="velas" viewBox=format!("0 0 {w} {}", h + hv + 6.0)>
            {justo.map(|j| view! { <line class="justo" x1="0" x2=w y1=y(j) y2=y(j) /> })}
            {vs.iter().enumerate().map(|(i, v)| {
                let x = larg * (i as f64 + 0.5);
                let sobe = v.4 >= v.1;
                let (top, base) = (y(v.1.max(v.4)), y(v.1.min(v.4)));
                let alt = (base - top).max(1.5);
                let vol = v.5 as f64 / vmax * hv;
                view! {
                    <g class=if sobe { "sobe" } else { "desce" }>
                        <line x1=x x2=x y1=y(v.2) y2=y(v.3) />
                        <rect x=x - larg * 0.3 y=top width=larg * 0.6 height=alt />
                        <rect class="vol" x=x - larg * 0.3 y=h + 6.0 + hv - vol width=larg * 0.6 height=vol />
                    </g>
                }
            }).collect_view()}
            <path class="ma" d=media />
        </svg>
        <div class="linha-rot">{vs.iter().step_by((vs.len() / 7).max(1)).map(|v| view! { <span>{v.0.clone()}</span> }).collect_view()}</div>
    }
}

fn sinal(t: &Value) -> (&'static str, &'static str) {
    match txt(t, "sinal").as_str() {
        "COMPRAR" => ("COMPRAR", "bom"),
        "VENDER" => ("VENDER", "alerta"),
        "OBSERVAR" => ("OBSERVAR", ""),
        _ => ("MANTER", ""),
    }
}

pub fn aba(e: Estado, versao: RwSignal<u32>, moeda: RwSignal<String>) -> impl IntoView {
    let m = recurso(e, versao, || Some("/api/bolsa/mercado".into()));
    let busca = RwSignal::new(String::new());
    let tipo = RwSignal::new(String::from("todos"));
    let escolhido = RwSignal::new(String::new());
    let serie = recurso(e, versao, move || {
        let id = escolhido.get();
        (!id.is_empty()).then(|| format!("/api/bolsa/serie?id={id}"))
    });
    view! {
        {move || carregado(m.get(), move |v| {
            let q = busca.get().to_lowercase();
            let moeda_api = if moeda.get() == "Gold" { "gold" } else { "orb" };
            let tp = tipo.get();
            let mut t: Vec<Value> = lista(&v, "tickers").iter()
                .filter(|x| txt(x, "moeda") == moeda_api
                    && (tp == "todos" || txt(x, "tipo") == tp)
                    && (q.is_empty() || txt(x, "nome").to_lowercase().contains(&q)))
                .cloned().collect();
            t.sort_by(|a, b| num(b, "vscore").unwrap_or(0.0).abs().total_cmp(&num(a, "vscore").unwrap_or(0.0).abs()));
            if escolhido.get_untracked().is_empty() {
                if let Some(p) = t.first() {
                    escolhido.set(txt(p, "id"));
                }
            }
            let sel = t.iter().find(|x| txt(x, "id") == escolhido.get()).cloned().unwrap_or(Value::Null);
            let (s, tom) = sinal(&sel);
            let desc = num(&sel, "desconto").unwrap_or(0.0);
            let justo = num(&sel, "base");
            view! {
                <div class="pregao">
                    <div class="caixa lista-ativos">
                        <input placeholder="Buscar ativo…" prop:value=move || busca.get() on:input=move |ev| busca.set(event_target_value(&ev)) />
                        <select class="caixa" prop:value=move || tipo.get() on:change=move |ev| tipo.set(event_target_value(&ev))>
                            <option value="todos">"Todos os tipos"</option>
                            <option value="pokemon">"Pokémon"</option>
                            <option value="item">"Itens"</option>
                        </select>
                        <div class="ativos">
                            {t.iter().take(120).map(|x| {
                                let id = txt(x, "id");
                                let id2 = id.clone();
                                let var = num(x, "var24h").unwrap_or(0.0);
                                let cor = if var >= 0.0 { "melhora" } else { "piora" };
                                view! {
                                    <div class="ativo" class:sel=move || escolhido.get() == id2 on:click=move |_| escolhido.set(id.clone())>
                                        <img src=txt(x, "sprite") alt="" loading="lazy" />
                                        <span class="nome">{txt(x, "nome")}</span>
                                        <span>{curto(num(x, "ultimo").unwrap_or(0.0))}</span>
                                        <span class=cor>{pct(var)}</span>
                                    </div>
                                }
                            }).collect_view()}
                        </div>
                    </div>
                    <div class="caixa">
                        <div class="caixa-t">{txt(&sel, "nome")}</div>
                        {move || carregado(serie.get(), move |sr| {
                            let vs = velas(lista(&sr, "vendas"));
                            let vs = vs[vs.len().saturating_sub(21)..].to_vec();
                            let u = vs.last().cloned();
                            view! {
                                {u.map(|u| view! { <div class="ohlc">{format!("A {} · M {} · m {} · F {} · {} vendas", curto(u.1), curto(u.2), curto(u.3), curto(u.4), u.5)}</div> })}
                                {grafico(&vs, justo)}
                                <div class="dica">"Verde/vermelho: o dia subiu/caiu · linha: média de 7 dias · tracejado: valor justo · barras de baixo: vendas."</div>
                            }
                        })}
                    </div>
                    <div class="caixa">
                        <div class="caixa-t">"Veredito"</div>
                        <div class=format!("sinal {tom}")>{s}</div>
                        <div class="dica">{format!("confiança {:.0}% · liquidez {}", num(&sel, "confianca").unwrap_or(0.0), txt(&sel, "liquidez"))}</div>
                        <div class="pares">
                            <span>"Preço atual"</span><b>{curto(num(&sel, "ultimo").unwrap_or(0.0))}</b>
                            <span>"Valor justo"</span><b>{format!("{} ({})", curto(num(&sel, "base").unwrap_or(0.0)), pct(desc))}</b>
                            <span>"Alvo de compra"</span><b>{num(&sel, "alvoC").map_or("—".into(), curto)}</b>
                            <span>"Alvo de venda"</span><b>{num(&sel, "alvoV").map_or("—".into(), curto)}</b>
                            <span>"Menor oferta"</span><b>{num(&sel, "askMin").map_or("—".into(), |a| format!("{} · {} anúncios", curto(a), inteiro(num(&sel, "askAnuncios").unwrap_or(0.0))))}</b>
                            <span>"Vendas 24 h / 30 d"</span><b>{format!("{} / {}", inteiro(num(&sel, "vol24h").unwrap_or(0.0)), inteiro(num(&sel, "vol30d").unwrap_or(0.0)))}</b>
                        </div>
                        <div class="caixa-t" style="margin-top:12px">"Motivos"</div>
                        <div class="dica">{txt(&sel, "motivo")}</div>
                        {(!txt(&sel, "vmot").is_empty()).then(|| view! { <div class="dica">{txt(&sel, "vmot")}</div> })}
                    </div>
                </div>
            }
        })}
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    #[test]
    fn velas_por_dia_de_brasilia() {
        // 14/11 02:00 UTC é 13/11 em Brasília; 14/11 12:00 UTC é 14/11.
        let v = json!([
            [1_794_621_600_000_i64, 10.0],
            [1_794_657_600_000_i64, 12.0],
            [1_794_661_200_000_i64, 9.0],
            [1_794_664_800_000_i64, 11.0]
        ]);
        let vs = velas(v.as_array().map_or(&[][..], Vec::as_slice));
        assert_eq!(vs.len(), 2);
        assert_eq!(vs[0].0, "13/11");
        assert_eq!(
            (vs[1].1, vs[1].2, vs[1].3, vs[1].4, vs[1].5),
            (12.0, 12.0, 9.0, 11.0, 3)
        );
    }
}
