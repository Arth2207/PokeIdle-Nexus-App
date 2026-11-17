//! Seleção por rendimento: catálogo vivo, XP publicado pelo jogo e amostras comparáveis.
//! A previsão ordena testes; só a observação aprova uma troca.
use super::*;

pub(super) fn contexto(c: &ContaViva) -> Value {
    let p = &c.ultimo["io"]["active"];
    let ficha: Map<String, Value> = [
        "id",
        "speciesId",
        "ivs",
        "quality",
        "potencia",
        "shiny",
        "refino",
        "refinoTotal",
        "tmFisico",
        "tmElemental",
        "tmAoe",
        "heldItemId",
    ]
    .into_iter()
    .map(|k| (k.into(), p.get(k).cloned().unwrap_or(Value::Null)))
    .collect();
    let bonus = |k: &str| {
        c.ultimo["bonusXp"][k]
            .as_f64()
            .map(|v| (v * 10000.0).round() / 10000.0)
    };
    json!({"versao":3, "ativo": ficha, "dia":c.ultimo["dia"],
        "bonus":{"tr":bonus("tr"),"pk":bonus("pk")}, "movimento":c.ultimo["movimento"], "tier":c.ultimo["outlandTier"],
        "regras":c.welcome.get("outlandRegrasId").or_else(|| c.welcome.get("nexusRegrasId")),
        "ballIds":c.ultimo.pointer("/hdx/eu/automation/ballIds"),
        "potionIds":c.ultimo.pointer("/hdx/eu/automation/potionIds")})
}

/// `shared/sell-value.mjs::xpDoNivel`, versão atual do jogo.
pub(super) fn xp_base(n: f64) -> f64 {
    crate::motor::jogo::xp_base(n)
}

/// Ajusta o tempo entre alvos sem congelar recargas. Não finge calibrar se o combate
/// simulado já é mais lento que o observado.
fn pausa_calibrada(
    me: &Combatente,
    foe: &Combatente,
    observado: f64,
    k: &Constantes,
) -> Option<f64> {
    if !observado.is_finite() || observado <= 0.0 {
        return None;
    }
    let sem_pausa = luta(me, foe, 600.0, k).kph;
    if observado > sem_pausa * 1.02 {
        return None;
    }
    let (mut lo, mut hi) = (0.0, 120.0);
    for _ in 0..14 {
        let meio = (lo + hi) / 2.0;
        if luta_com_pausa(me, foe, 600.0, k, meio).kph > observado {
            lo = meio;
        } else {
            hi = meio;
        }
    }
    Some((lo + hi) / 2.0)
}

fn amostra_valida(m: &Value, contexto: &Value, nivel: f64, agora: f64) -> bool {
    m["contexto"] == *contexto
        && nm(m, "kills") >= 20.0
        && nm(m, "span") >= 120_000.0
        && (0.0..=30.0 * 60e3).contains(&(agora - nm(m, "t")))
        && (nm(m, "lvl") - nivel).abs() <= nivel * 0.05
}

pub(super) fn valor_atual(w: &Janela, obj: &str, pr: &Precos, g: &Gemas, out: bool) -> Option<f64> {
    if obj == "gemas" {
        let sp = EspValor {
            d: out.then_some(2001),
            capturas_por_kill: (w.kills >= 20).then(|| w.capturas as f64 / w.kills as f64),
        };
        let v = valor_gemas_h(w.kph, &sp, g);
        return v.is_finite().then_some(v);
    }
    valor_janela(w, obj, pr, Some(g.ch_shiny)).filter(|v| v.is_finite())
}

pub(super) fn plano(d: &Value, r: &Relogio, c: &ContaViva, ctx: &ContextoHunt) -> Plano {
    let ag = r.agora_ms as f64;
    let cfg = config::efetiva(d, &c.nick);
    let hc = &cfg["hunt"];
    let obj = hc["objetivo"]
        .as_str()
        .and_then(objetivo)
        .unwrap_or(OBJETIVOS[0]);
    let h = hunt_atual(c);
    let inicio = ctx.hunt_desde.max(c.sessao_inicio as f64) + 15_000.0;
    let ms = (ag - inicio).clamp(0.0, 20.0 * 60e3);
    let mut w = janela(c, ms, ag, ag);
    let xp_observado = w.xp_h;
    let xp_pk_observado = w.xp_pk_h;
    let nivel_h = h.as_ref().and_then(|(_, n)| *n);
    for (tipo, pk) in [("tr", false), ("pk", true)] {
        if let Some((nivel, mult)) =
            nivel_h.zip(c.ultimo["bonusXp"][tipo].as_f64().filter(|m| *m > 0.0))
        {
            let por_kill = (xp_base(nivel) * mult).round();
            if pk {
                w.xp_pk_kill = Some(por_kill);
                w.xp_pk_h = w.kph.map(|k| k * por_kill);
            } else {
                w.xp_kill = Some(por_kill);
                w.xp_h = w.kph.map(|k| k * por_kill);
            }
        }
    }
    let medida_ok = w.kills >= 20 && w.span >= 120_000.0 && w.desm == 0;
    let ult = acao_hunt(d, &c.nick);
    let intervalo = hc["intervaloMin"].as_f64().unwrap_or(10.0).max(1.0) * 60e3;
    let espera = ult.map_or(0.0, |u| (nm(u, "t") + intervalo - ag).max(0.0));
    let contexto = contexto(c);
    let mut precos_gemas = cfg["gemaPrecos"].clone();
    if let Some(preco) = c.welcome["nexusTokenPreco"].as_f64().filter(|v| *v > 0.0) {
        precos_gemas["bronzeToken"] = json!(preco);
    }
    let g = Gemas {
        precos: precos_gemas,
        ch_shiny: c.welcome["chanceShiny"]
            .as_f64()
            .filter(|v| *v > 0.0)
            .unwrap_or(1.0 / 24000.0),
        out_mult: crate::outlands::atual(&c.welcome, &c.ultimo).map(|t| t.mult),
        token_chance: crate::outlands::chance_token(&c.welcome),
    };
    let out_atual = h
        .as_ref()
        .and_then(|(n, _)| ctx.dex.por_nome(n))
        .is_some_and(|s| eh_outland(Some(s.id)));
    let atual = valor_atual(&w, obj.0, &ctx.precos, &g, out_atual);
    let lucro = lucro_janela(&w, &ctx.precos);
    let mut o = json!({"versao":3,"ativo":ver(hc,"ativo"),"objetivo":obj.0,"objNome":obj.1,"un":obj.2,
        "hunt":h.as_ref().map(|(n,l)|json!({"nome":n,"lvl":l})),"kills":w.kills,"killsMin":20,
        "janelaMin":ms/60e3,"coberturaMin":w.span/60e3,"medidaOk":medida_ok,"xpH":w.xp_h,"xpPokemonH":w.xp_pk_h,"xpHObservado":xp_observado,"xpPokemonHObservado":xp_pk_observado,
        "comparacaoXp":"Todas as hunts são comparadas com os bônus atuais da conta.",
        "kph":w.kph,"lucroH":lucro,"atual":atual,"espera":espera,"explorar":null,"degraus":[],
        "candidatos":[],"melhor":null,"capturas":w.capturas,"gemasParcial":true,
        "calibracao":{"fator":1.0,"modelo":"segundos por abate"},"ultimaTroca":ult,
        "notaModelo":"Estimativa de XP sustentado. Inclui tempo sem abates da própria conta e recargas durante caminhada/espera. Espécies ainda não medidas precisam de teste real; não há garantia de ser o máximo global."});
    for k in ["margem", "intervaloMin", "maxPiorPct"] {
        o[k] = hc[k].clone();
    }
    let mut teste = None;
    if let Some(u) = ult.filter(|u| ver(u, "teste")) {
        let passou = ag - nm(u, "t");
        let depois = janela(c, (passou - 30e3).clamp(0.0, 15.0 * 60e3), ag, ag);
        let objeto = u["obj"].as_str().and_then(objetivo).unwrap_or(OBJETIVOS[0]);
        let valor = valor_atual(&depois, objeto.0, &ctx.precos, &g, out_atual);
        let antes = u.get("antesVal").filter(|v| v.is_number()).cloned();
        o["killsDepois"] = json!(depois.kills);
        o["teste"] = json!({"t":u["t"],"de":u["de"],"para":u["para"],"passou":passou,
            "dur":TESTE_MIN*60e3+30e3,"antes":antes,"depois":valor,"un":objeto.2,"obj":objeto.0,
            "killsDepois":depois.kills,"lucroDepois":lucro_janela(&depois,&ctx.precos)});
        teste = Some(TesteInfo {
            passou,
            dur: TESTE_MIN * 60e3 + 30e3,
            antes,
            depois: valor,
            lucro_depois: lucro_janela(&depois, &ctx.precos),
            obj: objeto.0.into(),
            un: objeto.2.into(),
        });
    }
    let mut p = Plano {
        o: o.as_object().cloned().unwrap_or_default(),
        obj: obj.0,
        base: None,
        best: None,
        w,
        medida_ok,
        atual,
        enc: 1.0,
        explorar: None,
        lucro_h: lucro,
        espera,
        alvo: None,
        teste,
    };
    let motivo_vazio = if h.is_none() {
        "Fora de hunt."
    } else {
        "Aguardando catálogo de hunts e ficha do Pokémon enviados pelo jogo."
    };
    p.o.insert("motivo".into(), json!(motivo_vazio));
    let Some(base) = melhores_hunts(d, c, ctx, hc) else {
        return p;
    };
    let Some(cur) = base.cur else {
        return p;
    };
    p.base = Some(PlanoBase {
        h_nome: base.h_nome.clone(),
        h_lvl: base.h_lvl,
        at_sp: base.at.sp.nome.clone(),
        at_lvl: base.at.lvl,
    });
    p.o.insert("piorAtual".into(), json!(cur.pior));
    p.o.insert("analisadas".into(), json!(base.alts.len()));
    let observado = p.w.kph.unwrap_or(0.0);
    let mult_xp = c
        .ultimo
        .pointer("/bonusXp/tr")
        .and_then(Value::as_f64)
        .filter(|v| *v > 0.0)
        .or_else(|| {
            p.w.xp_kill
                .filter(|v| *v > 0.0)
                .map(|x| x / xp_base(base.h_lvl))
        });
    p.o.insert("multXpObservado".into(), json!(mult_xp));
    let mult_pk = c
        .ultimo
        .pointer("/bonusXp/pk")
        .and_then(Value::as_f64)
        .filter(|v| *v > 0.0)
        .or_else(|| {
            p.w.xp_pk_kill
                .filter(|v| *v > 0.0)
                .map(|x| x / xp_base(base.h_lvl))
        });
    p.o.insert("multXpPokemonObservado".into(), json!(mult_pk));
    let pausa = base
        .par_atual
        .as_ref()
        .and_then(|(me, foe)| pausa_calibrada(me, foe, observado, ctx.k))
        .or_else(|| {
            c.ultimo["pausaEntreAlvos"]
                .as_f64()
                .filter(|v| v.is_finite() && *v >= 0.0 && *v <= 120.0)
        });
    p.o.insert("calibracao".into(),json!({"modelo":"combate com recargas durante o tempo entre alvos",
      "pausaSegundos":pausa,"kphObservado":observado,"kphCombateContinuo":cur.kph,
      "fonte":if observado>0.0 && pausa.is_some(){"amostra da própria conta, incluindo caminhada e espera"}else if pausa.is_some(){"tempo entre alvos informado para a simulação"}else{"sem calibração confiável"}}));
    p.o.insert("xpPokemonH".into(), json!(p.w.xp_pk_h));
    let mut exploracoes = Vec::new();
    let mut candidatos = Vec::new();
    let mut resumos = Vec::new();
    let max_pior = hc["maxPiorPct"].as_f64().unwrap_or(0.4).clamp(0.01, 0.95);
    for a in &base.alts {
        if a.nome == base.h_nome && a.h == base.h_lvl {
            continue;
        }
        if ctx.limite_rodizio.is_some_and(|l| a.h > l + 15.0) || a.r.pior > max_pior {
            continue;
        }
        if d["bloqueio"][format!("{}|{}", c.nick, a.nome)]
            .as_f64()
            .is_some_and(|t| t > ag)
        {
            continue;
        }
        let key = format!("{}|{}", a.nome, nv(a.h));
        let m = &d["medidas"][&c.nick][key];
        let medido = amostra_valida(m, &contexto, base.at.lvl, ag);
        // Uma queda grande de nível para upar exige rendimento já medido no mesmo contexto.
        // Não enviar uma conta em hunt 650 para nível 50 com base só em simulação.
        if matches!(obj.0, "xp" | "equilibrio") && a.h < base.h_lvl * 0.5 && !medido {
            continue;
        }
        let kph = if medido {
            nm(m, "kph")
        } else {
            pausa
                .map(|seg| luta_com_pausa(&a.par.0, &a.par.1, 600.0, ctx.k, seg).kph)
                .unwrap_or(0.0)
        };
        let xp_h = if medido {
            m["xpH"].as_f64()
        } else {
            mult_xp.map(|b| (xp_base(a.h) * b).round() * kph)
        };
        let xp_pk_h = if medido {
            m["xpPokemonH"].as_f64().or_else(|| {
                xp_h.zip(mult_xp)
                    .zip(mult_pk)
                    .map(|((tr, btr), bpk)| tr / btr * bpk)
            })
        } else {
            mult_pk.map(|b| (xp_base(a.h) * b).round() * kph)
        };
        let capturas = medido.then(|| m["capturasPorKill"].as_f64()).flatten();
        let sp = EspValor {
            d: Some(a.sp.id),
            capturas_por_kill: capturas,
        };
        // Gold escala pela curva atual; consumos medidos continuam sendo a âncora.
        let gold_ratio = p.w.gold_kill.unwrap_or(0.0) / (12000.0 * (base.h_lvl / 150.0).powf(0.6));
        let gold = 12000.0 * (a.h / 150.0).powf(0.6) * gold_ratio;
        let dmg_cur = cur.tkill.unwrap_or(0.0) * cur.dps_in;
        let dmg_alt = a.r.tkill.unwrap_or(0.0) * a.r.dps_in;
        let pot_kill = if dmg_cur > 0.0 {
            p.w.pot_kill.unwrap_or(0.0) * dmg_alt / dmg_cur
        } else {
            p.w.pot_kill.unwrap_or(0.0)
        };
        let pot_h = pot_kill * kph;
        let lucro_h = gold * kph
            - pot_h * ctx.precos.pot
            - p.w.bolas_kill.unwrap_or(0.0) * kph * ctx.precos.ball;
        let prev = Previsao {
            kph,
            xp_h,
            lucro_h,
            pot_h,
        };
        let bruto = valor_hunt(obj.0, &prev, &sp, &g);
        // Desconto explícito para incerteza de uma hunt ainda não medida.
        let val = bruto.map(|v| {
            if medido {
                v
            } else if v > 0.0 {
                v * 0.85
            } else {
                v / 0.85
            }
        });
        if !val.is_some_and(f64::is_finite)
            || kph <= 0.0
            || (obj.0 == "equilibrio" && (!lucro_h.is_finite() || lucro_h < 0.0))
        {
            continue;
        }
        let cand = Cand {
            nome: a.nome.clone(),
            slug: a.slug.clone(),
            nivel: a.h,
            p: prev,
            val,
        };
        if !medido
            && bruto
                .zip(p.atual)
                .is_some_and(|(prev, atual)| prev > atual * 1.03)
        {
            exploracoes.push((bruto.unwrap_or(0.0), cand.clone()));
        }
        resumos.push((
            cand.clone(),
            json!({"nome":cand.nome,"slug":cand.slug,"nivel":a.h,
            "val":val,"prevBruto":bruto,"xpH":xp_h,"xpPokemonH":xp_pk_h,"xpKillTreinador":mult_xp.map(|b|(xp_base(a.h)*b).round()),
            "xpKillPokemon":mult_pk.map(|b|(xp_base(a.h)*b).round()),"pausaSegundos":pausa,"lucroH":lucro_h,"kph":kph,"pior":a.r.pior,
            "fonte":if medido {"medido"} else {"estimado"}}),
        ));
        candidatos.push(cand);
    }
    candidatos.sort_by(|a, b| b.val.unwrap_or(0.0).total_cmp(&a.val.unwrap_or(0.0)));
    resumos.sort_by(|a, b| b.0.val.unwrap_or(0.0).total_cmp(&a.0.val.unwrap_or(0.0)));
    exploracoes.sort_by(|a, b| b.0.total_cmp(&a.0));
    p.explorar = exploracoes.into_iter().next().map(|(_, c)| c);
    p.o.insert(
        "explorar".into(),
        p.explorar
            .as_ref()
            .map(|c| json!({"nome":c.nome,"slug":c.slug,"nivel":c.nivel,"xpH":c.p.xp_h}))
            .unwrap_or(Value::Null),
    );
    p.o.insert("seguras".into(), json!(candidatos.len()));
    p.o.insert(
        "candidatos".into(),
        Value::Array(resumos.iter().map(|r| r.1.clone()).collect()),
    );
    p.o.insert(
        "melhor".into(),
        resumos.first().map(|r| r.1.clone()).unwrap_or(Value::Null),
    );
    p.best = candidatos.into_iter().next();
    p.alvo = p
        .atual
        .map(|a| a + a.abs() * hc["margem"].as_f64().unwrap_or(0.15).clamp(0.05, 1.0));
    p.o.insert("alvo".into(), json!(p.alvo));
    let motivo = if !ver(hc, "ativo") {
        "Desligada: somente análise.".into()
    } else if p.teste.is_some() {
        "Testando a troca; aguardo ao menos 5 minutos e 20 abates comparáveis.".into()
    } else if p.w.desm > 0 {
        "A amostra teve desmaio ou troca por desmaio; aguardo uma medição estável antes de escolher outra hunt.".into()
    } else if !medida_ok {
        format!(
            "Medindo a hunt atual: {} de 20 abates; mínimo de 2 minutos, sem misturar contextos.",
            p.w.kills
        )
    } else if espera > 0.0 {
        format!(
            "Mantendo a hunt: faltam {} min para reavaliar a troca.",
            (espera / 60e3).ceil()
        )
    } else if hc["explorar"] != false
        && p.explorar.is_some()
        && p.best
            .as_ref()
            .is_none_or(|b| b.val.unwrap_or(0.0) < p.alvo.unwrap_or(f64::INFINITY))
    {
        p.explorar.as_ref().map(|e|format!("Vou testar {} Nv {}: a previsão sugere mais XP sustentado; só mantenho após comparar os abates reais.",e.nome,nv(e.nivel))).unwrap_or_default()
    } else if let Some(b) = p.best.as_ref() {
        if p.alvo.is_some_and(|al| b.val.unwrap_or(0.0) >= al) {
            format!(
                "Vou testar {} Nv {}: {} {} previstos, contra {} medidos.",
                b.nome,
                nv(b.nivel),
                fmt_br(b.val.unwrap_or(0.0)),
                obj.2,
                fmt_br(p.atual.unwrap_or(0.0))
            )
        } else {
            format!(
                "Mantendo a hunt: {} não supera a margem de melhora.",
                b.nome
            )
        }
    } else {
        "Mantendo a hunt: nenhuma alternativa segura tem rendimento suficiente ou dados comparáveis.".into()
    };
    p.o.insert("motivo".into(), json!(motivo));
    p
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod testes {
    use super::*;
    fn fixture() -> (Pokedex, ContaViva, Value, Relogio) {
        let dex=Pokedex::de_valor(json!([
            {"d":6,"n":"Hero","t":["FIRE"],"s":[80,80,80,120,80,100],"h":80,"c":"pokebola","m":[["Flame",100,1,"E","FIRE"]]},
            {"d":10,"n":"Atual","t":["NORMAL"],"s":[50,20,50,20,50,20],"h":40,"c":"pokebola","m":[["Tackle",10,10,"F","NORMAL"]]},
            {"d":11,"n":"Nova","t":["NORMAL"],"s":[10,10,10,10,10,10],"h":99000,"c":"pokebola","m":[["Tackle",10,10,"F","NORMAL"]]}
        ])).expect("dex de teste");
        let r = Relogio {
            agora_ms: 1_800_000_000_000,
            fuso_ms: 0,
        };
        let c = ContaViva {
            nick: "Ana".into(),
            hunt_desde: r.agora_ms - 1_200_000,
            ultimo: json!({"huntSlug":"atual","hunt":"Atual · nível 500","trLv":"1000",
                "ativo":{"nome":"Hero","lv":"Nv 1000","q":"1,6","pot":"P2"},
                "io":{"active":{"id":1,"speciesId":6,"level":1000,"quality":1.6,"potencia":2}},
                "bonusXp":{"pk":4.0,"tr":4.0}}),
            welcome: json!({"hunts":[{"nome":"Atual","slug":"atual","nivel":500,"especies":[{"pokeId":10}]},
                {"nome":"Nova","slug":"nova_1000","nivel":1000,"especies":[{"pokeId":11}]},
                {"nome":"Nova","slug":"nova_1250","nivel":1250,"especies":[{"pokeId":11}]}]}),
            linhas: (0..60)
                .map(|i| {
                    (
                        i,
                        r.agora_ms - 600_000 + i * 10_000,
                        "Atual derrotado! +1.000 xp treinador, +1.000 xp pokemon, +100 ouro".into(),
                    )
                })
                .collect(),
            ..Default::default()
        };
        let d = config::carregar(
            &json!({"config":{"Ana":{"hunt":{"ativo":true,"objetivo":"xp","margem":0.15,"explorar":true,"maxPiorPct":0.4}}}}),
        );
        (dex, c, d, r)
    }
    fn contexto_teste<'a>(dex: &'a Pokedex, k: &'a Constantes, c: &ContaViva) -> ContextoHunt<'a> {
        ContextoHunt {
            dex,
            k,
            precos: Precos::default(),
            limite_rodizio: None,
            equipe_no_comando: false,
            rodizio_volta: false,
            hunt_desde: c.hunt_desde as f64,
        }
    }
    #[test]
    fn usa_nivel_e_slug_do_catalogo_vivo_e_reavalia_ao_liberar_hunt() {
        let (dex, mut c, d, r) = fixture();
        let k = Constantes::wiki();
        let p = plano(&d, &r, &c, &contexto_teste(&dex, &k, &c));
        assert!(p.medida_ok);
        assert_eq!(
            p.best.as_ref().map(|b| b.slug.as_str()),
            Some("nova_1000"),
            "{}",
            Value::Object(p.o.clone())
        );
        assert_eq!(p.o["analisadas"], 2); // inclui a atual, não a Nova 1250 bloqueada pelo treinador
        c.ultimo["trLv"] = json!(1250);
        let p = plano(&d, &r, &c, &contexto_teste(&dex, &k, &c));
        assert_eq!(p.o["analisadas"], 3);
        assert!(p.o["candidatos"]
            .as_array()
            .expect("ranking")
            .iter()
            .any(|v| v["slug"] == "nova_1250"));
    }
    #[test]
    fn nao_explora_sem_melhora_nem_inventa_hunts_ausentes() {
        let (dex, mut c, d, r) = fixture();
        let k = Constantes::wiki();
        c.welcome["hunts"]
            .as_array_mut()
            .expect("catálogo")
            .truncate(1);
        let p = plano(&d, &r, &c, &contexto_teste(&dex, &k, &c));
        assert!(p.best.is_none());
        assert!(p.explorar.is_none());
        c.welcome["hunts"] = json!([]);
        assert_eq!(
            plano(&d, &r, &c, &contexto_teste(&dex, &k, &c)).o["analisadas"],
            0
        );
    }
    #[test]
    fn xp_publicado_bate_com_abate_real_de_silcoon() {
        assert_eq!((xp_base(650.0) * 4.625775).round(), 390434.0);
        assert_eq!(xp_base(50.0), 1508.0);
        assert!((xp_base(1000.0) / xp_base(500.0) - 2_f64.powf(1.25)).abs() < 0.0001);
    }
    #[test]
    fn parasect_50_nao_supera_hunt_650_so_por_matar_rapido() {
        let (dex, mut c, d, r) = fixture();
        c.ultimo["hunt"] = json!("Atual · nível 650");
        c.welcome["hunts"] = json!([
            {"nome":"Atual","slug":"atual","nivel":650,"especies":[{"pokeId":10}]},
            {"nome":"Parasect","slug":"parasect_50","nivel":50,"especies":[{"pokeId":11}]}
        ]);
        let k = Constantes::wiki();
        let p = plano(&d, &r, &c, &contexto_teste(&dex, &k, &c));
        assert!(p.medida_ok);
        assert!(p.o["candidatos"]
            .as_array()
            .is_some_and(|a| a.iter().all(|h| h["slug"] != "parasect_50")));
        assert!(p
            .best
            .as_ref()
            .is_none_or(|b| b.val.unwrap_or(0.0) < p.alvo.unwrap_or(f64::INFINITY)));
    }
    #[test]
    fn caminhada_usa_tempo_real_e_recargas_nao_congelam() {
        let (dex, c, d, _r) = fixture();
        let k = Constantes::wiki();
        let base = melhores_hunts(
            &d,
            &c,
            &contexto_teste(&dex, &k, &c),
            &d["config"]["Ana"]["hunt"],
        )
        .expect("base");
        let (me, foe) = base.par_atual.expect("alvo");
        let observado = luta_com_pausa(&me, &foe, 600.0, &k, 3.0).kph;
        let pausa = pausa_calibrada(&me, &foe, observado, &k).expect("calibração");
        let refeito = luta_com_pausa(&me, &foe, 600.0, &k, pausa).kph;
        assert!((refeito - observado).abs() <= 6.0);
        assert!(observado < luta(&me, &foe, 600.0, &k).kph);
        assert!(pausa_calibrada(&me, &foe, 100000.0, &k).is_none());
    }
    #[test]
    fn nao_reutiliza_medida_com_bonus_lutador_ou_amostra_incompativeis() {
        let (_, mut c, _, r) = fixture();
        let ctx = contexto(&c);
        let mut m = json!({"contexto":ctx,"kills":50,"span":300000,"t":r.agora_ms,"lvl":1000});
        assert!(amostra_valida(&m, &ctx, 1000.0, r.agora_ms as f64));
        c.ultimo["bonusXp"]["tr"] = json!(2.0);
        assert!(!amostra_valida(
            &m,
            &contexto(&c),
            1000.0,
            r.agora_ms as f64
        ));
        assert!(!amostra_valida(&m, &ctx, 1300.0, r.agora_ms as f64));
        m["span"] = json!(30000);
        assert!(!amostra_valida(&m, &ctx, 1000.0, r.agora_ms as f64));
    }
    #[test]
    fn previsao_da_ficha_usa_motor_comum_e_nao_muda_a_conta() {
        let (dex, mut c, d, r) = fixture();
        let k = Constantes::wiki();
        c.ultimo["io"]["active"]["ivs"] =
            json!({"hp":27,"atk":27,"def":27,"spAtk":27,"spDef":27,"speed":27});
        c.ultimo["io"]["active"]["shiny"] = json!(false);
        let antes = c.ultimo.clone();
        let f = c.ultimo["io"]["active"].clone();
        let p = super::super::previsao::prever(&d, &r, &c, &contexto_teste(&dex, &k, &c), Some(&f))
            .expect("ficha completa");
        assert!(!p["ranking"].as_array().expect("ranking").is_empty());
        assert_eq!(p["ficha"]["ivs"]["hp"], 27);
        assert_eq!(antes, c.ultimo);
        let mut invalida = f;
        invalida["quality"] = json!(16.0);
        assert!(super::super::previsao::prever(
            &d,
            &r,
            &c,
            &contexto_teste(&dex, &k, &c),
            Some(&invalida)
        )
        .is_err());
    }

    #[test]
    fn compara_com_bonus_atual_sem_misturar_xp_historico() {
        let (dex, mut c, d, r) = fixture();
        let k = Constantes::wiki();
        c.ultimo["bonusXp"]["tr"] = json!(4.0);
        c.ultimo["bonusXp"]["pk"] = json!(2.0);
        let a = plano(&d, &r, &c, &contexto_teste(&dex, &k, &c));
        for (_, _, texto) in &mut c.linhas {
            *texto = texto.replace("+1.000 xp treinador", "+2.000 xp treinador");
        }
        let b = plano(&d, &r, &c, &contexto_teste(&dex, &k, &c));
        assert_eq!(a.o["xpH"], b.o["xpH"]);
        assert_ne!(a.o["xpHObservado"], b.o["xpHObservado"]);
        assert_eq!(a.o["candidatos"], b.o["candidatos"]);
        let p = super::super::previsao::prever(&d, &r, &c, &contexto_teste(&dex, &k, &c), None)
            .expect("previsão");
        assert!(p["ranking"]
            .as_array()
            .expect("ranking")
            .iter()
            .any(|h| h["atual"] == true));
        let candidato = &p["candidatos"][0];
        assert!(
            (candidato["xpH"].as_f64().unwrap_or(0.0)
                - 2.0 * candidato["xpPokemonH"].as_f64().unwrap_or(0.0))
            .abs()
                < 1000.0
        );
    }

    #[test]
    fn xp_hp_e_um_nivel_a_mais_nao_invalidam_contexto_mas_equipamento_sim() {
        let (_, mut c, _, _) = fixture();
        let antes = contexto(&c);
        c.ultimo["io"]["active"]["level"] = json!(1001);
        c.ultimo["io"]["active"]["xp"] = json!(100000);
        c.ultimo["io"]["active"]["hp"] = json!(10);
        assert_eq!(antes, contexto(&c));
        c.ultimo["io"]["active"]["tmAoe"] = json!(1);
        assert_ne!(antes, contexto(&c));
    }
    #[test]
    fn desligada_nao_reverte_teste_e_primeiro_ciclo_nao_troca_sem_medicao() {
        let (dex, c, mut d, r) = fixture();
        let k = Constantes::wiki();
        let ctx = contexto_teste(&dex, &k, &c);
        d["config"]["Ana"]["hunt"]["ativo"] = json!(false);
        d["acoes"] = json!([{"nick":"Ana","tipo":"hunt","ok":true,"teste":true,"vigiar":true,"t":r.agora_ms-600000,"de":"Nova","para":"Atual"}]);
        let mut laco = EstadoLaco::default();
        assert!(motor_hunt_completo(&d, &mut laco, &r, &c, &ctx)
            .decisoes
            .is_empty());
        d["config"]["Ana"]["hunt"]["ativo"] = json!(true);
        d["acoes"] = json!([]);
        assert!(
            motor_hunt_completo(&d, &mut EstadoLaco::default(), &r, &c, &ctx)
                .decisoes
                .is_empty()
        );
    }
    #[test]
    fn amostra_valida_inicia_teste_e_mudanca_de_bonus_nao_reverte_as_cegas() {
        let (dex, mut c, mut d, r) = fixture();
        let k = Constantes::wiki();
        let mut laco = EstadoLaco::default();
        let ctx = contexto_teste(&dex, &k, &c);
        motor_hunt_completo(&d, &mut laco, &r, &c, &ctx);
        laco.marcar("Ana", "huntContextoDesde", (r.agora_ms - 600000) as f64);
        let saida = motor_hunt_completo(&d, &mut laco, &r, &c, &ctx);
        assert!(saida
            .decisoes
            .iter()
            .any(|v| matches!(v,Decisao::TrocarHunt{slug,..}if slug=="nova_1000")));
        let acao = saida
            .gravacoes
            .iter()
            .find(|g| g.alvo == "acaoNova")
            .expect("teste persistido")
            .dados
            .clone();
        let mut acao = acao;
        acao["nick"] = json!("Ana");
        acao["tipo"] = json!("hunt");
        acao["ok"] = json!(true);
        d["acoes"] = json!([acao]);
        c.ultimo["bonusXp"]["tr"] = json!(2.0);
        let saida = motor_hunt_completo(&d, &mut laco, &r, &c, &contexto_teste(&dex, &k, &c));
        assert!(!saida
            .decisoes
            .iter()
            .any(|v| matches!(v, Decisao::TrocarHunt { .. })));
        assert!(saida
            .gravacoes
            .iter()
            .any(|g| g.alvo == "acao" && g.dados["teste"] == false));
    }
}
