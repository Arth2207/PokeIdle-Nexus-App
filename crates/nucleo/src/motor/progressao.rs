//! Meta de treinador: extrapolação do XP observado na hunt atual, sem inventar bônus futuros.
use serde_json::{json, Value};

use super::{jogo, regras::ContaViva};

pub const META: f64 = 3000.0;

pub fn xp_total(nivel: f64) -> f64 {
    (50.0 / 3.0 * (nivel.powi(3) - 6.0 * nivel.powi(2) + 17.0 * nivel - 12.0)).round()
}

pub fn prever(c: &ContaViva, agora: i64) -> Value {
    let nivel = c
        .ultimo
        .get("trLv")
        .and_then(|v| v.as_f64().or_else(|| jogo::num(Some(v))));
    let Some(nivel) = nivel.filter(|n| n.is_finite() && *n >= 1.0) else {
        return json!({"meta":META,"motivo":"Aguardando nível do treinador."});
    };
    // O relay novo envia XP acumulado. O anterior envia progresso textual dentro do nível.
    let xp = c.ultimo["xp"].as_f64().or_else(|| {
        c.ultimo["trXp"]
            .as_str()?
            .split('/')
            .next()
            .and_then(jogo::num_texto)
            .map(|x| xp_total(nivel) + x)
    });
    let Some(xp) =
        xp.filter(|x| x.is_finite() && *x >= xp_total(nivel) && *x < xp_total(nivel + 1.0))
    else {
        return json!({"meta":META,"nivel":nivel,"motivo":"Aguardando XP atual do treinador."});
    };
    let falta = (xp_total(META) - xp).max(0.0);
    let inicio = c.hunt_desde.max(c.sessao_inicio).max(agora - 20 * 60_000);
    let mut eventos: Vec<(i64, f64)> = c
        .linhas
        .iter()
        .filter(|(_, t, _)| *t >= inicio && *t <= agora)
        .filter_map(|(_, t, texto)| {
            let m = jogo::LINHA_ABATE.as_ref()?.captures(texto)?;
            Some((*t, jogo::num_texto(m.get(2)?.as_str())?))
        })
        .collect();
    eventos.sort_by_key(|e| e.0);
    eventos.dedup();
    let mut xp_medido = 0.0;
    let mut tempo_ms = 0;
    let mut intervalos = 0;
    // Intervalos longos são lacunas: não fingir que houve combate contínuo.
    for par in eventos.windows(2) {
        let dt = par[1].0 - par[0].0;
        if dt > 0 && dt <= 30_000 {
            tempo_ms += dt;
            xp_medido += par[1].1;
            intervalos += 1;
        }
    }
    let recente = eventos.last().is_some_and(|e| agora - e.0 <= 120_000);
    let taxa = (recente && intervalos >= 20 && tempo_ms >= 120_000 && xp_medido > 0.0)
        .then(|| xp_medido * 3_600_000.0 / tempo_ms as f64);
    let horas = if falta == 0.0 {
        Some(0.0)
    } else {
        taxa.map(|x| falta / x)
    };
    json!({"meta":META,"nivel":nivel,"xpAtual":xp,"xpFalta":falta,
        "progressoPct":(xp / xp_total(META) * 100.0).clamp(0.0,100.0),
        "xpH":taxa,"horas":horas,"chegadaEm":horas.map(|h|agora as f64+h*3_600_000.0),
        "amostraMin":tempo_ms as f64/60_000.0,"abates":eventos.len(),"atingida":falta==0.0,
        "motivo":if falta==0.0 {"Meta atingida."} else if taxa.is_none() {"Aguardando ao menos 2 minutos e 20 intervalos de abates recentes na mesma hunt."} else {"Estimativa mantendo o ritmo atual, jogando continuamente. Bônus, paradas e futuras hunts mudam a previsão."}})
}

#[cfg(test)]
mod testes {
    use super::*;
    fn conta() -> ContaViva {
        ContaViva {
            ultimo: json!({"trLv":"650","trXp":"100 / 100000000 xp"}),
            linhas: (0..31)
                .map(|i| {
                    (
                        i,
                        1_000_000 + i * 10_000,
                        "Silcoon derrotado! +1.000 xp treinador, +2.000 xp pokemon, +100 ouro"
                            .into(),
                    )
                })
                .collect(),
            ..Default::default()
        }
    }
    #[test]
    fn usa_xp_do_treinador_e_progresso_no_nivel() {
        let p = prever(&conta(), 1_300_000);
        assert_eq!(p["xpH"], 360000.0);
        assert_eq!(p["xpAtual"], xp_total(650.0) + 100.0);
        assert_eq!(
            p["horas"],
            (xp_total(3000.0) - xp_total(650.0) - 100.0) / 360000.0
        );
    }
    #[test]
    fn evita_dupla_contagem_do_xp_acumulado_e_respeita_a_meta() {
        let mut c = conta();
        c.ultimo["xp"] = json!(xp_total(650.0) + 500.0);
        assert_eq!(prever(&c, 1_300_000)["xpAtual"], xp_total(650.0) + 500.0);
        c.ultimo = json!({"trLv":3000,"xp":xp_total(3000.0)});
        assert_eq!(prever(&c, 1_300_000)["horas"], 0.0);
    }
    #[test]
    fn nao_inventa_previsao_sem_xp_com_lacunas_ou_apos_parar() {
        let mut c = conta();
        assert!(prever(&c, 1_600_000)["horas"].is_null());
        c.hunt_desde = 1_250_000;
        assert!(prever(&c, 1_300_000)["horas"].is_null());
        c.ultimo = json!({"trLv":650});
        assert!(prever(&c, 1_300_000)["xpAtual"].is_null());
        let mut c = conta();
        for (_, t, _) in &mut c.linhas {
            *t *= 4;
        }
        assert!(prever(&c, 5_200_000)["xpH"].is_null());
    }
}
