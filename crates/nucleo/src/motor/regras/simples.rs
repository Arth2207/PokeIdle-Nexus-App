//! Regras simples do motor (etapa 4): `motorRadar`, `motorMedias`, `motorBola` e
//! `motorExtras` do legado.
//!
//! Estado que só existe no legado: `conta.trocandoHunt` (marcado dentro de `trocarHunt`, que
//! aqui é só uma decisão) é tratado como `false`. `conta.sessaoTomada` é `ultimo.sessaoTomada`
//! (o legado grava `!!r.sessaoTomada` da própria leitura).

use serde_json::{json, Map, Value};

use super::{alerta, ContaViva, Decisao, EstadoLaco, Registro};
use crate::js::{self, json_num, mais, texto, verdadeiro};
use crate::motor::jogo::{casa, hunt_atual, num, slug_de, LINHA_ABATE};
use crate::motor::{caminho, config, leitura, num_ou_0, Relogio};

const MIN_MS: f64 = 60_000.0;

/// Acrescenta o `alerta()` do legado à lista, se ele de fato avisaria.
#[allow(clippy::too_many_arguments)]
fn avisar(
    d: &Value,
    laco: &mut EstadoLaco,
    r: &Relogio,
    saida: &mut Vec<Decisao>,
    nick: &str,
    (chave, titulo): (&str, &str),
    texto: String,
    minutos: i64,
) {
    let pedido = Decisao::Alerta {
        nick: nick.to_owned(),
        chave: chave.to_owned(),
        titulo: titulo.to_owned(),
        texto,
        minutos,
    };
    saida.extend(alerta(d, laco, r, pedido));
}

/// `motorRadar`: avisa (uma vez a cada 6 h por item) quando um raro que você tem está com o
/// preço de hoje bem acima da média de 7 dias.
pub fn radar(d: &Value, laco: &mut EstadoLaco, r: &Relogio, contas: &[ContaViva]) -> Vec<Decisao> {
    let mut saida = Vec::new();
    let plano = leitura::plano_raros(d, r, contas);
    if !verdadeiro(plano.get("ativo")) {
        return saida;
    }
    let Some(nick) = contas.iter().find(|c| !c.nick.is_empty()).map(|c| &c.nick) else {
        return saida;
    };
    let Some(Value::Array(itens)) = plano.get("itens") else {
        return saida;
    };
    for i in itens {
        let var = i.get("varPct").and_then(Value::as_f64);
        let (true, Some(var)) = (i.get("sinal") == Some(&json!("vender")), var) else {
            continue;
        };
        let texto_alerta = format!(
            "Preço de hoje {}{}% sobre a média de 7 dias (você tem {}). Vale vender.",
            if var > 0.0 { "+" } else { "" },
            js::numero(var),
            texto(i.get("qtd"))
        );
        avisar(
            d,
            laco,
            r,
            &mut saida,
            nick,
            (
                &format!("radar{}", texto(i.get("id"))),
                &format!("Radar: {} em alta", texto(i.get("nome"))),
            ),
            texto_alerta,
            360,
        );
    }
    saida
}

fn comando_medias(nick: &str, dias: i64) -> Decisao {
    Decisao::Comando {
        nick: nick.to_owned(),
        comando: json!({"t": "market.medias", "dias": dias}),
        registro: None,
    }
}

/// `motorMedias`: preço médio do Mercado (só consulta), a cada 30 min por conta.
/// O jogo responde `dias: 7` mesmo ao pedido de 1 dia (visto em 528 pedidos, 14/11), então
/// `medias1` nunca chega: exigir as duas, como o legado, repetia os pedidos a cada 5 min.
pub fn medias(_d: &Value, laco: &mut EstadoLaco, r: &Relogio, conta: &ContaViva) -> Vec<Decisao> {
    let agora = r.agora_ms as f64;
    let m = caminho(&conta.ultimo, &["hdx", "medias"]);
    if verdadeiro(m) && agora - mais(m.and_then(|x| x.get("t"))) < 30.0 * MIN_MS {
        return Vec::new();
    }
    let em = laco.marca(&conta.nick, "mediasEm");
    if em != 0.0 && agora - em < 5.0 * MIN_MS {
        return Vec::new();
    }
    laco.marcar(&conta.nick, "mediasEm", agora);
    vec![comando_medias(&conta.nick, 7)]
}

/// `motorBola`: a bola automática fica sempre ligada (partes do app desligam por um instante
/// na troca de hunt; se o app reiniciar no meio, ela ficaria desligada para sempre).
pub fn bola(_d: &Value, laco: &mut EstadoLaco, r: &Relogio, conta: &ContaViva) -> Vec<Decisao> {
    let agora = r.agora_ms as f64;
    let a = caminho(&conta.ultimo, &["hdx", "eu", "automation"]);
    // `conta.trocandoHunt` é estado do legado: tratado como false.
    let Some(Value::Object(a)) = a.filter(|x| verdadeiro(Some(x))) else {
        return Vec::new();
    };
    if verdadeiro(caminho(&conta.ultimo, &["sessaoTomada"])) {
        return Vec::new();
    }
    let sem_bolas = match a.get("ballIds") {
        Some(Value::Array(l)) => l.is_empty(),
        Some(Value::String(s)) => s.is_empty(),
        _ => true,
    };
    if verdadeiro(a.get("autoBallSemParar")) || sem_bolas {
        return Vec::new();
    }
    let ultima = laco.marca(&conta.nick, "bolaReligadaEm");
    if ultima != 0.0 && agora - ultima < MIN_MS {
        return Vec::new();
    }
    laco.marcar(&conta.nick, "bolaReligadaEm", agora);
    // `{ t: 'auto.set', ...a, autoBallSemParar: true }`: a chave existente mantém a posição.
    let mut cmd = Map::new();
    cmd.insert("t".into(), json!("auto.set"));
    for (k, v) in a {
        cmd.insert(k.clone(), v.clone());
    }
    cmd.insert("autoBallSemParar".into(), Value::Bool(true));
    // Em sombra assume resposta "ok". No legado, `r == null` (sem resposta) zera
    // `bolaReligadaEm` para tentar de novo no próximo ciclo.
    vec![Decisao::Comando {
        nick: conta.nick.clone(),
        comando: Value::Object(cmd),
        registro: Some(Registro {
            custo: None,
            tipo: "bola".into(),
            texto_ok: "A bola automática estava desligada: religuei (captura tudo que der).".into(),
            texto_falha: "Não consegui religar a bola automática ({r}).".into(),
        }),
    }]
}

/// `ligadas(arr)`: itens com `on` verdadeiro (só lista; outro valor vira vazio).
fn ligadas(v: Option<&Value>) -> Vec<&Value> {
    match v {
        Some(Value::Array(a)) => a.iter().filter(|b| verdadeiro(b.get("on"))).collect(),
        _ => Vec::new(),
    }
}

fn com_estoque(v: Option<&Value>) -> Vec<&Value> {
    ligadas(v)
        .into_iter()
        .filter(|b| num(b.get("q")).is_some_and(|q| q > 0.0))
        .collect()
}

/// `motorExtras`: alertas (shiny, desmaio, sessão, hunt parada, suprimentos), retomar a
/// última hunt, recarregar o painel sem lista de pokemon ou sem dados do jogo.
pub fn extras(d: &Value, laco: &mut EstadoLaco, r: &Relogio, conta: &ContaViva) -> Vec<Decisao> {
    let mut saida = Vec::new();
    let nick = conta.nick.as_str();
    let u = &conta.ultimo;
    if !verdadeiro(Some(u)) {
        return saida;
    }
    let c = config::efetiva(d, nick);
    let agora = r.agora_ms;
    let agora_f = agora as f64;
    let desde = agora - 60_000;
    for (_, t, x) in &conta.linhas {
        if *t < desde {
            continue;
        }
        let baixo = x.to_lowercase();
        if baixo.contains("shiny") {
            avisar(
                d,
                laco,
                r,
                &mut saida,
                nick,
                ("shiny", "Shiny"),
                x.clone(),
                1,
            );
        }
        if baixo.contains("desmai") {
            avisar(
                d,
                laco,
                r,
                &mut saida,
                nick,
                ("desmaio", "Desmaio"),
                x.clone(),
                5,
            );
        }
    }
    if verdadeiro(u.get("sessaoTomada")) {
        let t = "A conta foi aberta em outro lugar.".to_owned();
        avisar(
            d,
            laco,
            r,
            &mut saida,
            nick,
            ("sessao", "Sessão caiu"),
            t,
            15,
        );
    }
    let h = hunt_atual(conta);
    if let Some((nome, lvl)) = &h {
        let slug = slug_de(conta, nome, *lvl).map_or(Value::Null, Value::String);
        let nivel = lvl.map_or(Value::Null, json_num);
        laco.ultima_hunt.insert(
            nick.to_owned(),
            json!({"nome": nome, "lvl": nivel, "slug": slug, "t": agora}),
        );
    }
    let ultimo_abate = conta.linhas.iter().rev().find(|l| casa(&LINHA_ABATE, &l.2));
    let parado = ultimo_abate.map(|l| (agora - l.1) as f64 / MIN_MS);
    if let (Some((nome, _)), Some(p)) = (&h, parado) {
        if p > 10.0 {
            let t = format!("Sem abates há {} min em {nome}.", js::numero(js::round(p)));
            avisar(
                d,
                laco,
                r,
                &mut saida,
                nick,
                ("parada", "Hunt parada"),
                t,
                30,
            );
        }
    }
    // bola e poção em uso (ligadas e com estoque); avisa também quando todas as ligadas acabaram
    let bola = com_estoque(u.get("bolas")).pop();
    let pocao = com_estoque(u.get("pocoes")).into_iter().next();
    let sem_bola = !ligadas(u.get("bolas")).is_empty() && bola.is_none();
    let sem_pocao = !ligadas(u.get("pocoes")).is_empty() && pocao.is_none();
    let abaixo = |b: &Value, lim: f64| num(b.get("q")).is_some_and(|q| q < lim);
    let linha = |b: &Value| format!("{}: {}", texto(b.get("nome")), texto(b.get("q")));
    let mut falta: Vec<String> = Vec::new();
    if sem_bola {
        falta.push("as bolas ligadas acabaram".into());
    } else if let Some(b) = bola.filter(|b| abaixo(b, 100.0)) {
        falta.push(linha(b));
    }
    if sem_pocao {
        falta.push("as poções ligadas acabaram".into());
    } else if let Some(p) = pocao.filter(|p| abaixo(p, 30.0)) {
        falta.push(linha(p));
    }
    if !falta.is_empty() {
        let t = falta.join(" · ");
        avisar(
            d,
            laco,
            r,
            &mut saida,
            nick,
            ("suprimentos", "Suprimentos baixos"),
            t,
            60,
        );
    }
    // retomar: fora de hunt por X minutos depois de estar numa → volta para a última
    let retomar_ativo = verdadeiro(caminho(&c, &["retomar", "ativo"]));
    if retomar_ativo && h.is_none() && verdadeiro(Some(&conta.welcome)) {
        let minutos = caminho(&c, &["retomar", "minutos"]);
        if let Some(lh) = laco.ultima_hunt.get(nick).cloned() {
            let passou = agora_f - mais(lh.get("t"));
            if verdadeiro(lh.get("slug"))
                && passou > mais(minutos) * MIN_MS
                && passou < 6.0 * 3.6e6
                && laco.marca(nick, "retomado") == 0.0
            {
                let nome = texto(lh.get("nome"));
                let nivel = lh.get("lvl").cloned().unwrap_or(Value::Null);
                let ate = format!(
                    "Voltei para {nome} Nv {} depois de {} min fora da hunt",
                    texto(Some(&nivel)),
                    texto(minutos)
                );
                // Em sombra assume resposta "ok" (o texto do legado traz `(${r})`).
                laco.marcar(nick, "retomado", agora_f);
                saida.push(Decisao::TrocarHunt {
                    nick: nick.to_owned(),
                    slug: texto(lh.get("slug")),
                    nome,
                    nivel,
                    registro: Some(Registro {
                        custo: None,
                        tipo: "retomar".into(),
                        texto_ok: format!("{ate} (ok)."),
                        texto_falha: format!("{ate} ({{r}})."),
                    }),
                });
            }
        }
    }
    if h.is_some() {
        laco.marcar(nick, "retomado", 0.0);
    }
    // sem a lista de pokemon (conta recém-aberta): recarrega sozinho, até 2 vezes, a cada 2 min
    if verdadeiro(u.get("hdx")) && !verdadeiro(caminho(u, &["hdx", "pokemons"])) {
        let ja = laco.marca(nick, "semListaDesde");
        laco.marcar(nick, "semListaDesde", if ja != 0.0 { ja } else { agora_f });
    } else {
        laco.marcar(nick, "semListaDesde", 0.0);
    }
    let sem_lista = laco.marca(nick, "semListaDesde");
    if sem_lista != 0.0
        && agora_f - sem_lista > 45e3
        && laco.marca(nick, "recargasLista") < 2.0
        && agora_f - laco.marca(nick, "recargaListaEm") > 120e3
    {
        let n = laco.marca(nick, "recargasLista") + 1.0;
        laco.marcar(nick, "recargasLista", n);
        laco.marcar(nick, "recargaListaEm", agora_f);
        saida.push(Decisao::RecarregarPainel {
            nick: nick.to_owned(),
        });
        saida.push(Decisao::Registro {
            nick: nick.to_owned(),
            tipo: "recarregar".into(),
            texto: format!(
                "Ainda não tinha a lista de pokemon desta conta: recarreguei o painel do jogo ({} de 2).",
                js::numero(n)
            ),
            ok: true,
        });
    }
    if laco.marca(nick, "semListaDesde") == 0.0 {
        laco.marcar(nick, "recargasLista", 0.0);
    }
    // recarregar: painel sem nenhum dado novo do jogo por X minutos
    let io = num_ou_0(caminho(u, &["io", "receivedAt"]));
    let minutos = caminho(&c, &["recarregar", "minutos"]);
    if verdadeiro(caminho(&c, &["recarregar", "ativo"]))
        && io != 0.0
        && agora_f - io > mais(minutos) * MIN_MS
        && agora_f - laco.marca(nick, "recarregado") > 20.0 * MIN_MS
    {
        laco.marcar(nick, "recarregado", agora_f);
        saida.push(Decisao::RecarregarPainel {
            nick: nick.to_owned(),
        });
        saida.push(Decisao::Registro {
            nick: nick.to_owned(),
            tipo: "recarregar".into(),
            texto: format!(
                "Painel sem dados do jogo há {} min: recarreguei.",
                texto(minutos)
            ),
            ok: true,
        });
    }
    saida
}

#[cfg(test)]
mod testes {
    use super::*;

    fn rel(agora_ms: i64) -> Relogio {
        Relogio {
            agora_ms,
            fuso_ms: 0,
        }
    }

    fn conta(ultimo: Value) -> ContaViva {
        ContaViva {
            id: "c1".into(),
            nick: "Ana".into(),
            ultimo,
            ..Default::default()
        }
    }

    fn dados() -> Value {
        config::carregar(&json!({}))
    }

    #[test]
    fn medias_pede_7_dias_e_espera_5_min() {
        let d = dados();
        let mut laco = EstadoLaco::default();
        let c = conta(json!({"hdx": {}}));
        let v = medias(&d, &mut laco, &rel(10_000_000), &c);
        assert_eq!(v.len(), 1);
        assert!(medias(&d, &mut laco, &rel(10_000_000 + 299_000), &c).is_empty());
        assert_eq!(
            medias(&d, &mut laco, &rel(10_000_000 + 300_000), &c).len(),
            1
        );
        // Só a de 7 dias fresca já basta (o jogo nunca manda a de 1 dia).
        let fresca = conta(json!({"hdx": {"medias": {"t": 9_999_000}}}));
        let mut outro = EstadoLaco::default();
        assert!(medias(&d, &mut outro, &rel(10_000_000), &fresca).is_empty());
    }

    #[test]
    fn bola_religa_e_mantem_posicao_da_chave() {
        let d = dados();
        let mut laco = EstadoLaco::default();
        let c = conta(json!({"hdx": {"eu": {"automation":
            {"autoBallSemParar": false, "ballIds": [1], "x": 2}}}}));
        let v = bola(&d, &mut laco, &rel(1_000_000), &c);
        let Some(Decisao::Comando {
            comando, registro, ..
        }) = v.first()
        else {
            panic!("sem comando");
        };
        assert_eq!(
            comando.to_string(),
            r#"{"t":"auto.set","autoBallSemParar":true,"ballIds":[1],"x":2}"#
        );
        assert_eq!(registro.as_ref().map(|x| x.tipo.as_str()), Some("bola"));
        assert!(bola(&d, &mut laco, &rel(1_059_000), &c).is_empty());
        assert_eq!(bola(&d, &mut laco, &rel(1_060_000), &c).len(), 1);
        let ligada = conta(json!({"hdx": {"eu": {"automation":
            {"autoBallSemParar": true, "ballIds": [1]}}}}));
        assert!(bola(&d, &mut EstadoLaco::default(), &rel(1), &ligada).is_empty());
        let kick = conta(json!({"sessaoTomada": true, "hdx": {"eu": {"automation":
            {"ballIds": [1]}}}}));
        assert!(bola(&d, &mut EstadoLaco::default(), &rel(1), &kick).is_empty());
    }

    #[test]
    fn extras_suprimentos_e_parada() {
        let d = dados();
        let mut laco = EstadoLaco::default();
        let agora = 100_000_000;
        let mut c = conta(json!({
            "hunt": "Rattata · nível 5", "hdx": {"pokemons": [1]},
            "bolas": [{"on": true, "q": 50, "nome": "Great"}],
            "pocoes": [{"on": true, "q": 0, "nome": "P"}]}));
        c.linhas = vec![
            (
                1,
                agora - 11 * 60_000,
                "Rattata derrotado! +1 xp treinador, +2 xp pokemon, +3 ouro".into(),
            ),
            (2, agora - 1000, "Um Shiny apareceu".into()),
        ];
        let v = extras(&d, &mut laco, &rel(agora), &c);
        let textos: Vec<String> = v
            .iter()
            .filter_map(|x| match x {
                Decisao::Alerta { chave, texto, .. } => Some(format!("{chave}={texto}")),
                _ => None,
            })
            .collect();
        assert_eq!(
            textos,
            [
                "shiny=Um Shiny apareceu",
                "parada=Sem abates há 11 min em Rattata.",
                "suprimentos=Great: 50 · as poções ligadas acabaram"
            ]
        );
        assert!(laco.ultima_hunt.contains_key("Ana"));
    }

    #[test]
    fn extras_recarrega_sem_lista_ate_duas_vezes() {
        let d = dados();
        let mut laco = EstadoLaco::default();
        let c = conta(json!({"hdx": {}}));
        let t0 = 100_000_000;
        assert!(extras(&d, &mut laco, &rel(t0), &c).is_empty());
        assert!(extras(&d, &mut laco, &rel(t0 + 45_000), &c).is_empty());
        let v = extras(&d, &mut laco, &rel(t0 + 45_001), &c);
        assert_eq!(v[0], Decisao::RecarregarPainel { nick: "Ana".into() });
        assert!(extras(&d, &mut laco, &rel(t0 + 100_000), &c).is_empty());
        assert_eq!(extras(&d, &mut laco, &rel(t0 + 170_000), &c).len(), 2);
        assert!(extras(&d, &mut laco, &rel(t0 + 400_000), &c).is_empty());
    }

    #[test]
    fn extras_retoma_a_hunt() {
        let d = config::carregar(
            &json!({"config": {"Ana": {"retomar": {"ativo": true, "minutos": 5}}}}),
        );
        let mut laco = EstadoLaco::default();
        let t0 = 100_000_000;
        let mut c = conta(json!({"hunt": "Rattata · nível 5", "hdx": {"pokemons": [1]}}));
        c.welcome = json!({"hunts": [{"nome": "Rattata", "nivel": 5, "slug": "rat-5"}]});
        extras(&d, &mut laco, &rel(t0), &c);
        c.ultimo = json!({"hdx": {"pokemons": [1]}});
        assert!(extras(&d, &mut laco, &rel(t0 + 299_000), &c).is_empty());
        let v = extras(&d, &mut laco, &rel(t0 + 301_000), &c);
        let Some(Decisao::TrocarHunt { slug, registro, .. }) = v.first() else {
            panic!("sem troca");
        };
        assert_eq!(slug, "rat-5");
        assert_eq!(
            registro.as_ref().map(|x| x.texto_ok.as_str()),
            Some("Voltei para Rattata Nv 5 depois de 5 min fora da hunt (ok).")
        );
        assert!(extras(&d, &mut laco, &rel(t0 + 302_000), &c).is_empty());
    }

    #[test]
    fn radar_avisa_item_em_alta() {
        let d = config::carregar(&json!({}));
        let mut laco = EstadoLaco::default();
        let mut c = conta(json!({"hdx": {
            "eu": {"items": {"60001": 2, "5": 9}},
            "medias": {"itens": {"60001": {"orb": {"media": 100}}}},
            "medias1": {"itens": {"60001": {"orb": {"media": 130}}}}}}));
        c.welcome = json!({"itens": [{"id": 60001, "nome": "Raro X"}]});
        let p = leitura::plano_raros(&d, &rel(1_000_000_000), std::slice::from_ref(&c));
        assert_eq!(p["itens"][0]["varPct"], json!(30));
        assert_eq!(p["itens"][0]["sinal"], json!("vender"));
        let v = radar(&d, &mut laco, &rel(1_000_000_000), std::slice::from_ref(&c));
        assert_eq!(
            v,
            [Decisao::Alerta {
                nick: "Ana".into(),
                chave: "radar60001".into(),
                titulo: "Radar: Raro X em alta".into(),
                texto: "Preço de hoje +30% sobre a média de 7 dias (você tem 2). Vale vender."
                    .into(),
                minutos: 360
            }]
        );
        assert!(radar(&d, &mut laco, &rel(1_000_001_000), &[c]).is_empty());
    }

    /// Compara `plano_raros` com o `GET /api/raros` capturado do legado.
    /// `NEXUS_FIXTURES_MOTOR` aponta para a pasta com nexus-janela.json, nexus-dados.json e raros.json.
    #[test]
    #[ignore]
    fn raros_com_contas_igual_ao_legado() {
        let Ok(dir) = std::env::var("NEXUS_FIXTURES_MOTOR") else {
            return;
        };
        let ler = |n: &str| -> Value {
            let t = std::fs::read_to_string(format!("{dir}/{n}")).unwrap_or_default();
            serde_json::from_str(&t).unwrap_or(Value::Null)
        };
        let janela = ler("nexus-janela.json");
        let contas: Vec<ContaViva> = janela["contas"]
            .as_array()
            .map(|a| a.iter().filter_map(ContaViva::de_json).collect())
            .unwrap_or_default();
        let d = config::carregar(&ler("nexus-dados.json"));
        let r = Relogio {
            agora_ms: janela["agora"].as_i64().unwrap_or(0),
            fuso_ms: -3 * 3_600_000,
        };
        let legado = ler("raros.json");
        let novo = leitura::plano_raros(&d, &r, &contas);
        assert!(!contas.is_empty() && legado.is_object());
        assert_eq!(novo, legado);
    }
}
