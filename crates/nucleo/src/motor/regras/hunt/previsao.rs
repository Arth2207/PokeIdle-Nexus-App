//! Uma única previsão para dossiê, aplicativo e painel; usa o mesmo simulador do motor.
use super::*;

pub fn prever(
    d: &Value,
    r: &Relogio,
    c: &ContaViva,
    ctx: &ContextoHunt,
    ficha: Option<&Value>,
) -> Result<Value, String> {
    if !["tr", "pk"].iter().all(|k| {
        c.ultimo["bonusXp"][k]
            .as_f64()
            .is_some_and(|v| v.is_finite() && v > 0.0)
    }) {
        return Err("Aguardando os bônus atuais ou um novo abate da conta para não misturar boosts antigos.".into());
    }
    let mut dados = d.clone();
    dados["config"][&c.nick]["hunt"]["objetivo"] = json!("xp");
    dados["config"][&c.nick]["hunt"]["objetivoManual"] = json!(true);
    let original = plano_hunt(&dados, r, c, ctx);
    let mut conta = c.clone();
    if let Some(f) = ficha {
        let id = f["speciesId"].as_i64().ok_or("Espécie da ficha ausente.")?;
        let sp = ctx
            .dex
            .especies()
            .iter()
            .find(|s| s.id == id)
            .ok_or("Espécie não encontrada no catálogo.")?;
        let lv = f["level"]
            .as_f64()
            .filter(|v| v.is_finite() && *v >= 1.0 && *v <= 1_000_000.0)
            .ok_or("Nível da ficha inválido.")?;
        let q = f["quality"]
            .as_f64()
            .filter(|v| (0.8..=1.8).contains(v))
            .ok_or("Qualidade da ficha inválida.")?;
        let pot = f["potencia"]
            .as_u64()
            .filter(|v| (1..=5).contains(v))
            .ok_or("Potência da ficha inválida.")?;
        if !["hp", "atk", "def", "spAtk", "spDef", "speed"]
            .iter()
            .all(|k| {
                f["ivs"][k]
                    .as_f64()
                    .is_some_and(|v| (1.0..=32.0).contains(&v))
            })
        {
            return Err(
                "A ficha precisa dos seis IVs individuais para uma previsão comparável.".into(),
            );
        }
        let pausa=original.pointer("/calibracao/pausaSegundos").and_then(Value::as_f64).ok_or("Ainda não há calibração confiável do tempo entre alvos desta conta. Aguarde uma amostra estável.")?;
        let mut f = f.clone();
        let atual = &c.ultimo["io"]["active"];
        let mesma = ["speciesId", "quality", "potencia", "shiny", "ivs"]
            .iter()
            .all(|k| f.get(*k) == atual.get(*k));
        if mesma {
            for k in ["tmElemental", "tmFisico", "tmAoe", "heldItemId"] {
                if f.get(k).is_none() {
                    if let Some(v) = atual.get(k) {
                        f[k] = v.clone();
                    }
                }
            }
        }
        conta.ultimo["io"]["active"] = f;
        conta.ultimo["ativo"] = json!({"nome":sp.nome,"lv":format!("Nv {lv}"),"q":format!("Q {}",q.to_string().replace('.',",")),"pot":format!("P{pot}")});
        conta.ultimo["pausaEntreAlvos"] = json!(pausa);
        conta.ultimo["bonusXp"] =
            json!({"tr":original["multXpObservado"],"pk":original["multXpPokemonObservado"]});
        conta.linhas.clear();
    }
    let mut plano = if ficha.is_some() {
        plano_hunt(&dados, r, &conta, ctx)
    } else {
        original.clone()
    };
    let atual = if ficha.is_none() {
        json!({"nome":original["hunt"]["nome"],"slug":c.ultimo["huntSlug"],"nivel":original["hunt"]["lvl"],
          "xpH":original["xpH"],"xpPokemonH":original["xpPokemonH"],"kph":original["kph"],
          "pior":original["piorAtual"],"fonte":"medido (bônus atual)","atual":true})
    } else {
        let hc = config::efetiva(&dados, &c.nick);
        let base = melhores_hunts(&dados, &conta, ctx, &hc["hunt"])
            .ok_or("Não foi possível montar o combate da ficha.")?;
        let (me, foe) = base.par_atual.ok_or("Hunt atual fora do catálogo.")?;
        let pausa = conta.ultimo["pausaEntreAlvos"].as_f64().unwrap_or(0.0);
        let luta = luta_com_pausa(&me, &foe, 600.0, ctx.k, pausa);
        let xp = selecao::xp_base(base.h_lvl);
        json!({"nome":base.h_nome,"slug":c.ultimo["huntSlug"],"nivel":base.h_lvl,
          "xpH":original["multXpObservado"].as_f64().map(|m|xp*m*luta.kph),
          "xpPokemonH":original["multXpPokemonObservado"].as_f64().map(|m|xp*m*luta.kph),
          "kph":luta.kph,"pior":luta.pior,"fonte":"estimado","atual":true})
    };
    let mut ranking = plano["candidatos"].as_array().cloned().unwrap_or_default();
    if atual["xpH"].is_number() {
        ranking.push(atual);
    }
    ranking.sort_by(|a, b| {
        b["xpH"]
            .as_f64()
            .unwrap_or(0.0)
            .total_cmp(&a["xpH"].as_f64().unwrap_or(0.0))
    });
    let limite = config::efetiva(&dados, &c.nick)["hunt"]["maxPiorPct"]
        .as_f64()
        .unwrap_or(0.4);
    plano["melhorAgora"] = ranking
        .iter()
        .find(|h| h["pior"].as_f64().is_some_and(|p| p <= limite))
        .cloned()
        .unwrap_or(Value::Null);
    plano["ranking"] = json!(ranking);
    plano["conta"] = json!(c.nick);
    plano["nivelTreinador"] = c.ultimo["trLv"].clone();
    plano["dia"] = c.ultimo["dia"].clone();
    plano["bonusAtual"] = c.ultimo["bonusXp"].clone();
    plano["movimento"] = c.ultimo["movimento"].clone();
    plano["ficha"] = conta.ultimo["io"]["active"].clone();
    plano["ritmoAtual"] = json!({"xpTreinadorH":original["xpH"],"xpPokemonH":original["xpPokemonH"],"kph":original["kph"]});
    plano["calibracao"] = original["calibracao"].clone();
    plano["previsaoDaFicha"] = json!(ficha.is_some());
    Ok(plano)
}
