//! Combate › PvP e Treinamento. O tabuleiro é o do jogo (`shared/pvp-tabuleiro.mjs`): 30 casas
//! em 5 fileiras de larguras diferentes; a fileira 0 é a da frente (a mais perto do inimigo).

use leptos::prelude::*;
use serde_json::Value;

use super::nick_ativo;
use crate::comp::{carregado, Kpi};
use crate::dados::{curto, inteiro, lista, num, recurso, sprite, txt};
use crate::Estado;

/// Fileiras do tabuleiro: (primeira coluna, última coluna) na grade de 8 colunas (cx 11–18).
const FILEIRAS: [(i64, i64); 5] = [(12, 17), (11, 18), (11, 18), (13, 16), (13, 16)];

/// Casa do jogo (0–29) → (fileira, coluna 1–8 na grade).
pub fn posicao(casa: i64) -> Option<(i64, i64)> {
    let mut n = 0;
    for (f, (de, ate)) in FILEIRAS.iter().enumerate() {
        let largura = ate - de + 1;
        if casa < n + largura {
            return (casa >= n).then(|| (f as i64, de + (casa - n) - 10));
        }
        n += largura;
    }
    None
}

/// Uma peça no tabuleiro: casa, espécie (para a imagem), nome e o texto do balão.
#[derive(Clone)]
pub struct Peca {
    pub casa: i64,
    pub especie: i64,
    pub nome: String,
    pub balao: String,
}

/// O tabuleiro do jogo com as peças; passar o mouse mostra o balão de cada uma.
pub fn tabuleiro(pecas: Vec<Peca>, mini: bool) -> impl IntoView {
    let casas: Vec<(i64, i64, i64)> = (0..30)
        .filter_map(|c| posicao(c).map(|(f, col)| (c, f, col)))
        .collect();
    view! {
        <div class="tabuleiro-pvp" class:mini=mini>
            {casas.into_iter().map(|(c, f, col)| {
                let p = pecas.iter().find(|p| p.casa == c).cloned();
                let estilo = format!("grid-row:{};grid-column:{}", f + 1, col);
                let ocupada = p.is_some();
                view! {
                    <div class="casa-pvp" class:ocupada=ocupada style=estilo>
                        {match p {
                            Some(p) => view! {
                                {(p.especie > 0).then(|| view! { <img src=sprite(p.especie, false) alt=p.nome.clone() /> })}
                                {(p.especie <= 0).then(|| view! { <span class="sigla">{p.nome.chars().take(3).collect::<String>()}</span> })}
                                {(!mini).then(|| view! { <div class="balao"><b>{p.nome.clone()}</b><span>{p.balao.clone()}</span></div> })}
                            }.into_any(),
                            None => view! { <span class="n">{c}</span> }.into_any(),
                        }}
                    </div>
                }
            }).collect_view()}
            <div class="frente">"▲ inimigo"</div>
        </div>
    }
}

/// Pontos esperados por partida: taxa de vitória × ganho médio + taxa de derrota × perda média.
pub fn por_partida(vitorias: f64, derrotas: f64, ganho: f64, perda: f64) -> Option<f64> {
    let n = vitorias + derrotas;
    (n > 0.0).then(|| (vitorias * ganho + derrotas * perda) / n)
}

/// Previsão para chegar a `alvo` pontos: (partidas, horas). `None` se não sobe no ritmo atual.
pub fn previsao(pontos: f64, alvo: f64, por_partida: f64, partidas_h: f64) -> Option<(f64, f64)> {
    let falta = alvo - pontos;
    if falta <= 0.0 {
        return Some((0.0, 0.0));
    }
    if por_partida <= 0.0 || partidas_h <= 0.0 {
        return None;
    }
    let partidas = (falta / por_partida).ceil();
    Some((partidas, partidas / partidas_h))
}

fn duracao(h: f64) -> String {
    if h < 1.0 {
        format!("{:.0} min", (h * 60.0).max(1.0))
    } else if h < 48.0 {
        format!("{h:.1} h")
    } else {
        format!("{:.1} dias", h / 24.0)
    }
}

/// "dd/mm hh:mm" (UTC−3) daqui a `h` horas.
fn chegada(h: f64) -> String {
    let t = js_sys::Date::now() + h * 3.6e6;
    let s = (t / 1000.0) as i64 - 3 * 3600;
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64((s * 1000) as f64));
    format!(
        "{:02}/{:02} {:02}:{:02}",
        d.get_utc_date(),
        d.get_utc_month() + 1,
        d.get_utc_hours(),
        d.get_utc_minutes()
    )
}

fn cor_tier(t: &str) -> &'static str {
    match t {
        "bronze" => "#B87333",
        "prata" => "#A7B1BC",
        "ouro" => "#E3B341",
        "platina" => "#5FD3C6",
        "diamante" => "#6FA8FF",
        "mestre" => "#B07CFF",
        "challenger" | "desafiante" => "#FF6B6B",
        _ => "#9AA3AD",
    }
}

/// Um corte do ranking (top N): quanto falta e quando chega no ritmo atual.
fn cartao_corte(
    rotulo: &'static str,
    pontos: f64,
    agora: Option<f64>,
    anterior: Option<f64>,
    pp: Option<f64>,
    ph: f64,
) -> impl IntoView {
    // Meta: o corte do fim da semana passada (onde o corte termina); sem ele, o de agora.
    let alvo = anterior.or(agora);
    let pct = alvo.map_or(0.0, |a| (pontos / a.max(1.0) * 100.0).clamp(0.0, 100.0));
    let prev = alvo.and_then(|a| previsao(pontos, a, pp.unwrap_or(0.0), ph));
    let falta = alvo.map(|a| (a - pontos).max(0.0));
    let largura = format!("width:{pct:.1}%");
    view! {
        <div class="corte">
            <div class="corte-t">{rotulo}</div>
            <div class="corte-v">{alvo.map_or("—".into(), |a| format!("{} pts", inteiro(a)))}</div>
            <div class="trilho-corte"><span style=largura></span></div>
            <div class="dica">{match falta { Some(f) if f <= 0.0 => "Você já está acima deste corte.".to_owned(), Some(f) => format!("Faltam {} pontos", inteiro(f)), None => "Corte ainda não lido.".into() }}</div>
            <div class="corte-prev">{match (falta, prev) {
                (Some(f), _) if f <= 0.0 => "✓".to_owned(),
                (Some(_), Some((p, h))) => format!("≈ {} partidas · {} · chega {}", inteiro(p), duracao(h), chegada(h)),
                (Some(_), None) => "No ritmo atual não sobe: precisa ganhar mais do que perde.".into(),
                _ => String::new(),
            }}</div>
            <div class="dica">{format!("corte agora: {} · fim da semana passada: {}", agora.map_or("—".into(), inteiro), anterior.map_or("—".into(), inteiro))}</div>
        </div>
    }
}

pub fn aba_pvp(e: Estado, versao: RwSignal<u32>) -> impl IntoView {
    let painel = recurso(e, versao, move || {
        nick_ativo(e).map(|n| format!("/api/pvp/painel?nick={n}"))
    });
    let sug = recurso(e, versao, move || {
        nick_ativo(e).map(|n| format!("/api/pvp/sugestao?nick={n}"))
    });
    view! {
        {move || carregado(painel.get(), move |p| {
            let a = p.get("atual").cloned().unwrap_or(Value::Null);
            let s = p.get("semana").cloned().unwrap_or(Value::Null);
            let tier = txt(&a, "tier");
            let pontos = num(&a, "pontos").unwrap_or(0.0);
            let (v, d) = (num(&s, "vitorias").unwrap_or(0.0), num(&s, "derrotas").unwrap_or(0.0));
            let wr = if v + d > 0.0 { v / (v + d) } else { 0.0 };
            let pp = por_partida(v, d, num(&s, "ganho").unwrap_or(0.0), num(&s, "perda").unwrap_or(0.0));
            let ph = num(&s, "ultimas6h").unwrap_or(0.0) / 6.0;
            let corte = |q: &str, pos: &str| p.pointer(&format!("/cortes/{q}/cortes/{pos}")).and_then(Value::as_f64);
            let estilo_tier = format!("--tier:{}", cor_tier(&tier));
            view! {
                <div class="elo-faixa" style=estilo_tier>
                    <div class="elo-selo"><span>{tier.to_uppercase()}</span><b>{txt(&a, "divisao")}</b></div>
                    <div class="elo-num"><b>{inteiro(pontos)}</b><span>"pontos"</span></div>
                    <div class="elo-num"><b>{num(&a, "posicao").map_or("—".into(), |x| format!("#{}", inteiro(x)))}</b><span>"posição"</span></div>
                    <div class="elo-num bom"><b>{inteiro(v)}</b><span>"vitórias na semana"</span></div>
                    <div class="elo-num ruim"><b>{inteiro(d)}</b><span>"derrotas na semana"</span></div>
                    <div class="elo-num"><b>{format!("{:.0}%", wr * 100.0)}</b><span>"taxa de vitória"</span></div>
                </div>
                <div class="kpis">
                    <Kpi titulo="Ganho por vitória" valor=num(&s, "ganho").map_or("—".into(), |g| format!("+{g:.1}")) unidade="pts" />
                    <Kpi titulo="Perda por derrota" valor=num(&s, "perda").map_or("—".into(), |g| format!("{g:.1}")) unidade="pts" />
                    <Kpi titulo="Saldo por partida" valor=pp.map_or("—".into(), |x| format!("{x:+.1}")) unidade="pts" bom=pp.map(|x| x > 0.0)
                        ajuda="Taxa de vitória × ganho médio + taxa de derrota × perda média (semana)." />
                    <Kpi titulo="Ritmo" valor=format!("{ph:.1}") unidade="partidas/h" ajuda="Partidas nas últimas 6 h ÷ 6." />
                </div>
                <div class="secao">"Rumo ao topo"</div>
                <div class="cortes">
                    {cartao_corte("Top 500", pontos, corte("atual", "500"), corte("anterior", "500"), pp, ph)}
                    {cartao_corte("Top 100", pontos, corte("atual", "100"), corte("anterior", "100"), pp, ph)}
                    {cartao_corte("Top 10", pontos, corte("atual", "10"), corte("anterior", "10"), pp, ph)}
                </div>
                <div class="dica">"Meta de cada corte: os pontos dessa posição no fim da semana passada (o ranking zera todo domingo). Previsão pelo saldo por partida e pelo ritmo de agora."</div>
            }
        })}
        {move || carregado(sug.get(), move |p| {
            let s = p.get("sugestao").cloned().unwrap_or(Value::Null);
            let faixa = num(&s, "faixa");
            let meta = recurso(e, versao, move || Some(match faixa {
                Some(f) => format!("/api/pvp/meta?faixa={f}"),
                None => "/api/pvp/meta".into(),
            }));
            let pecas: Vec<Peca> = lista(&s, "time").iter().map(|x| Peca {
                casa: num(x, "casa").unwrap_or(-1.0) as i64,
                especie: num(x, "especie").unwrap_or(0.0) as i64,
                nome: txt(x, "nome"),
                balao: format!("Nv {} · alcance {} · {}", inteiro(num(x, "nivel").unwrap_or(0.0)), inteiro(num(x, "alcance").unwrap_or(0.0)), txt(x, "motivo")),
            }).collect();
            view! {
                <div class="secao">"Formação sugerida"</div>
                <div class="formacao">
                    {tabuleiro(pecas, false)}
                    <div class="dica" style="text-align:center">{format!("Passe o mouse em cada pokémon para ver o porquê · medido em {} partidas do meta", inteiro(num(&s, "partidas_no_meta").unwrap_or(0.0)))}</div>
                </div>
                <div class="secao">"Meta da faixa"</div>
                {move || carregado(meta.get(), |m| {
                    let especies: Vec<Value> = lista(&m, "especies").iter().filter(|x| num(x, "usos").unwrap_or(0.0) >= 20.0).take(12).cloned().collect();
                    let times: Vec<Value> = lista(&m, "times").iter().take(6).cloned().collect();
                    view! {
                        <div class="meta-grade">
                            {especies.into_iter().map(|x| {
                                let taxa = num(&x, "taxa").unwrap_or(0.0);
                                let casa = x.get("casas").and_then(Value::as_array).and_then(|c| c.first()).and_then(|c| c.get(0)).and_then(Value::as_i64);
                                let w = format!("width:{:.0}%", taxa * 100.0);
                                let cor = if taxa >= 0.5 { "bom" } else { "ruim" };
                                view! {
                                    <div class="meta-card">
                                        <img src=sprite(super::escaneamento::especie_da_imagem(num(&x, "especie").unwrap_or(0.0) as i64), false) alt="" loading="lazy" />
                                        <div class="meta-info">
                                            <b>{txt(&x, "nome")}</b>
                                            <div class=format!("barra-taxa {cor}")><span style=w></span></div>
                                            <span class="dica">{format!("{:.0}% vitória · {} usos · alcance {} · casa {}", taxa * 100.0, inteiro(num(&x, "usos").unwrap_or(0.0)), inteiro(num(&x, "alcance").unwrap_or(0.0)), casa.map_or("—".into(), |c| c.to_string()))}</span>
                                        </div>
                                    </div>
                                }
                            }).collect_view()}
                        </div>
                        <div class="secao">"Times que aparecem na faixa"</div>
                        <div class="times-meta">
                            {times.into_iter().map(|t| view! {
                                <div class="time-meta">
                                    <div class="sprites">{lista(&t, "especies").iter().filter_map(Value::as_i64).map(|sp| view! { <img src=sprite(super::escaneamento::especie_da_imagem(sp), false) alt="" loading="lazy" /> }).collect_view()}</div>
                                    <span class="dica">{format!("{} vezes · {:.0}% vitória", inteiro(num(&t, "vezes").unwrap_or(0.0)), num(&t, "taxa").unwrap_or(0.0) * 100.0)}</span>
                                </div>
                            }).collect_view()}
                        </div>
                    }
                })}
            }
        })}
    }
}

/// Assinatura de um time numa luta de treino: "nome@casa" ordenado.
fn assinatura(lado: &Value) -> String {
    let mut v: Vec<String> = lista(lado, "pokemons")
        .iter()
        .map(|p| format!("{}@{}", txt(p, "nome"), txt(p, "casa")))
        .collect();
    v.sort();
    v.join(" ")
}

fn lado(r: &Value, n: f64) -> Value {
    lista(r, "placar")
        .iter()
        .find(|l| num(l, "lado") == Some(n))
        .cloned()
        .unwrap_or(Value::Null)
}

pub fn aba_treino(e: Estado, versao: RwSignal<u32>) -> impl IntoView {
    let t = recurso(e, versao, move || {
        nick_ativo(e).map(|n| format!("/api/pvp/treinos?nick={n}"))
    });
    let m = recurso(e, versao, || Some("/api/pvp/meta".into()));
    view! {
        {move || carregado(t.get(), move |v| {
            // Nome → espécie (para as imagens), pelo meta.
            let dex: std::collections::HashMap<String, i64> = match m.get() {
                Some(Some(Ok(mm))) => lista(&mm, "especies").iter().filter_map(|x| Some((txt(x, "nome"), super::escaneamento::especie_da_imagem(num(x, "especie")? as i64)))).collect(),
                _ => Default::default(),
            };
            let pecas = |l: &Value| -> Vec<Peca> {
                lista(l, "pokemons").iter().map(|p| Peca {
                    casa: num(p, "casa").unwrap_or(-1.0) as i64,
                    especie: dex.get(&txt(p, "nome")).copied().unwrap_or(0),
                    nome: txt(p, "nome"),
                    balao: format!("dano {} · abates {}", curto(num(p, "dano").unwrap_or(0.0)), inteiro(num(p, "abates").unwrap_or(0.0))),
                }).collect()
            };
            let lutas: Vec<Value> = lista(&v, "treinos").iter().map(|x| x.get("resultado").cloned().unwrap_or(Value::Null)).collect();
            let venci = |r: &Value| r.get("venceuA").and_then(Value::as_bool) == Some(false);
            let vit = lutas.iter().filter(|r| venci(r)).count();
            // Por adversário: lutas, vitórias e a formação que venceu (a mais recente).
            let mut adv: Vec<(String, Value, usize, usize, Option<Value>)> = Vec::new();
            for r in &lutas {
                let a = lado(r, 1.0);
                let nomes: String = { let mut n: Vec<String> = lista(&a, "pokemons").iter().map(|p| txt(p, "nome")).collect(); n.sort(); n.join(", ") };
                let i = match adv.iter().position(|x| x.0 == nomes) { Some(i) => i, None => { adv.push((nomes, a.clone(), 0, 0, None)); adv.len() - 1 } };
                adv[i].2 += 1;
                if venci(r) {
                    adv[i].3 += 1;
                    if adv[i].4.is_none() { adv[i].4 = Some(lado(r, 2.0)); }
                }
            }
            // Formações testadas: vitórias e dano médio.
            let mut form: Vec<(String, Value, usize, usize, f64)> = Vec::new();
            for r in &lutas {
                let meu = lado(r, 2.0);
                let k = assinatura(&meu);
                let i = match form.iter().position(|x| x.0 == k) { Some(i) => i, None => { form.push((k, meu.clone(), 0, 0, 0.0)); form.len() - 1 } };
                form[i].2 += 1;
                form[i].4 += num(&meu, "dano").unwrap_or(0.0);
                if venci(r) { form[i].3 += 1; }
            }
            form.sort_by(|a, b| (b.3 as f64 / b.2 as f64).total_cmp(&(a.3 as f64 / a.2 as f64)).then(b.2.cmp(&a.2)));
            let total = lutas.len();
            view! {
                <div class="kpis">
                    <Kpi titulo="Lutas de treino" valor=inteiro(total as f64) unidade="recentes" />
                    <Kpi titulo="Vitórias" valor=inteiro(vit as f64) comparacao=if total == 0 { "—".into() } else { format!("{:.0}% das lutas", vit as f64 * 100.0 / total as f64) } bom=(total > 0).then(|| vit * 2 >= total) />
                    <Kpi titulo="Adversários testados" valor=inteiro(adv.len() as f64) ajuda="Times do top 10 e do meta acima da sua faixa." />
                    <Kpi titulo="Formações testadas" valor=inteiro(form.len() as f64) />
                </div>
                <div class="secao">"Contra quem e o que funcionou"</div>
                <div class="treino-adv">
                    {adv.into_iter().map(|(nomes, a, n, w, venceu)| {
                        let selo = if w * 2 >= n { "selo ok" } else { "selo nao" };
                        view! {
                            <div class="adv-card">
                                <div class="adv-topo">
                                    <div class="sprites">{lista(&a, "pokemons").iter().map(|p| match dex.get(&txt(p, "nome")).copied().filter(|sp| *sp > 0) {
                                        Some(sp) => view! { <img src=sprite(sp, false) alt="" title=txt(p, "nome") loading="lazy" /> }.into_any(),
                                        None => view! { <span class="chip">{txt(p, "nome")}</span> }.into_any(),
                                    }).collect_view()}</div>
                                    <span class=selo>{format!("{w}/{n} vitórias")}</span>
                                </div>
                                <div class="dica">{nomes}</div>
                                {match venceu {
                                    Some(meu) => view! { <div class="dica">"Formação que venceu:"</div>{tabuleiro(pecas(&meu), true)} }.into_any(),
                                    None => view! { <div class="dica">"Ainda sem vitória contra este time."</div> }.into_any(),
                                }}
                            </div>
                        }
                    }).collect_view()}
                </div>
                <div class="secao">"Formações que mais venceram"</div>
                <div class="treino-adv">
                    {form.into_iter().take(6).map(|(_, meu, n, w, dano)| view! {
                        <div class="adv-card">
                            <div class="adv-topo"><b>{format!("{:.0}% de vitória", w as f64 * 100.0 / n.max(1) as f64)}</b><span class="dica">{format!("{n} lutas · dano médio {}", curto(dano / n.max(1) as f64))}</span></div>
                            {tabuleiro(pecas(&meu), true)}
                        </div>
                    }).collect_view()}
                </div>
                <div class="dica">"O treino roda sozinho no modo Ativo com “Aprender PvP”: uma luta a cada 5 min, contra times reais do top 10 e do meta acima."</div>
            }
        })}
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn casas_do_tabuleiro_do_jogo() {
        assert_eq!(posicao(0), Some((0, 2)));
        assert_eq!(posicao(5), Some((0, 7)));
        assert_eq!(posicao(6), Some((1, 1)));
        assert_eq!(posicao(21), Some((2, 8)));
        assert_eq!(posicao(22), Some((3, 3)));
        assert_eq!(posicao(29), Some((4, 6)));
        assert_eq!(posicao(30), None);
    }

    #[test]
    fn previsao_de_chegada() {
        // 60% de vitória, +16 / −10: 5,6 pts por partida.
        let pp = por_partida(6.0, 4.0, 16.0, -10.0).unwrap_or(0.0);
        assert!((pp - 5.6).abs() < 1e-9);
        assert_eq!(previsao(100.0, 156.0, pp, 2.0), Some((10.0, 5.0)));
        assert_eq!(previsao(100.0, 50.0, pp, 2.0), Some((0.0, 0.0)));
        assert_eq!(previsao(100.0, 156.0, -1.0, 2.0), None);
    }
}
