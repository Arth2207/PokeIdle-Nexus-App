//! Contexto recente recebido do jogo, inclusive clientes anteriores que reduziam a ficha.
use crate::eventos::Evento;
use serde_json::{json, Value};
use sqlx::PgPool;

/// Cache por campo e timestamp. Reenvios e lotes fora de ordem não restauram bônus antigos.
pub async fn registrar(db: &PgPool, conta: &str, eventos: &[Evento]) -> Result<(), sqlx::Error> {
    for campo in ["bonusXp", "bicicleta", "diaSazonal"] {
        let e = eventos
            .iter()
            .filter(|e| {
                e.tipo == "ws.bruto"
                    && e.dados
                        .pointer("/dados/estado")
                        .is_some_and(|s| s.get(campo).is_some())
            })
            .max_by_key(|e| e.em_ms);
        let Some(e) = e else { continue };
        let valor = json!({"em":e.em_ms,"valor":e.dados["dados"]["estado"][campo]});
        sqlx::query(
            "INSERT INTO nexus.parametro(chave,valor) VALUES($1,$2)
          ON CONFLICT(chave) DO UPDATE SET valor=EXCLUDED.valor,atualizado_em=now()
          WHERE coalesce((nexus.parametro.valor->>'em')::bigint,0)<(EXCLUDED.valor->>'em')::bigint",
        )
        .bind(format!("cacada.sinais.{}.{campo}", conta.to_lowercase()))
        .bind(valor)
        .execute(db)
        .await?;
    }
    Ok(())
}

pub async fn enriquecer(db: &PgPool, j: &mut Value, agora: i64) -> Result<(), sqlx::Error> {
    let nick = j["nick"].as_str().unwrap_or("").to_owned();
    let conta = j["id"].as_str().unwrap_or(&nick).to_lowercase();
    let ativo = j
        .pointer("/ultimo/io/active/id")
        .cloned()
        .unwrap_or(Value::Null);
    // Coleção já recebida pelo relay, sem nova requisição ao jogo. Exige mesmo nível e frescor.
    let ficha:Option<Value>=sqlx::query_scalar("SELECT jsonb_path_query_first(dados,'$.pokemons[*] ? (@.id == $id)',jsonb_build_object('id',$2::jsonb))
       FROM nexus.equipes_conta WHERE lower(nick)=lower($1) AND coalesce((dados->>'em')::bigint,0)>$3 LIMIT 1")
       .bind(&nick).bind(&ativo).bind(agora-120_000).fetch_optional(db).await?.flatten();
    if let Some(p) = ficha.filter(|p| p["level"] == j["ultimo"]["io"]["active"]["level"]) {
        if let (Some(a), Some(p)) = (j["ultimo"]["io"]["active"].as_object_mut(), p.as_object()) {
            for k in [
                "nome",
                "stats",
                "maxHp",
                "refino",
                "refinoTotal",
                "tmFisico",
                "tmElemental",
                "tmAoe",
                "heldItemId",
            ] {
                if let Some(v) = p.get(k) {
                    a.insert(k.into(), v.clone());
                }
            }
        }
    }
    for campo in ["bonusXp", "bicicleta", "diaSazonal"] {
        let v: Option<Value> =
            sqlx::query_scalar("SELECT valor FROM nexus.parametro WHERE chave=$1")
                .bind(format!("cacada.sinais.{conta}.{campo}"))
                .fetch_optional(db)
                .await?;
        let Some(v) = v else { continue };
        // Bônus expiram; não usar uma observação antiga como se ainda estivesse valendo.
        if campo == "bonusXp" && agora - v["em"].as_i64().unwrap_or(0) > 120_000 {
            continue;
        }
        let mais_novo = v["em"].as_i64().unwrap_or(0) >= j["ultimo"]["t"].as_i64().unwrap_or(0);
        match campo {
            "bonusXp" if mais_novo || !j["ultimo"]["bonusXp"].is_object() => {
                j["ultimo"]["bonusXp"] = v["valor"].clone()
            }
            "bicicleta" if mais_novo || !j["ultimo"]["movimento"].is_object() => {
                j["ultimo"]["movimento"] = json!({"bicicletaEquipada":v["valor"]["equipada"],"raridade":v["valor"]["equipadaRar"]})
            }
            "diaSazonal" if mais_novo || !j["ultimo"]["dia"].is_string() => {
                j["ultimo"]["dia"] = v["valor"]["tipo"].clone()
            }
            _ => {}
        }
    }
    bonus_dos_abates(j, agora);
    Ok(())
}

pub struct Entradas {
    pub janela: Value,
    pub dados: Value,
    pub dex: Value,
    pub agora: i64,
}
impl Entradas {
    pub fn calcular(self, ficha: Option<Value>) -> Result<Value, String> {
        use nucleo::{
            combate::especie::Pokedex,
            motor::{
                config,
                regras::{
                    hunt::{self, ContextoHunt, Precos},
                    ContaViva,
                },
                Relogio,
            },
        };
        let c = ContaViva::de_json(&self.janela).ok_or("Conta sem dados atuais.")?;
        let dex = Pokedex::de_valor(self.dex).map_err(|e| e.to_string())?;
        let k = hunt::constantes_da_conta(&c);
        let ctx = ContextoHunt {
            dex: &dex,
            k: &k,
            precos: Precos::default(),
            limite_rodizio: None,
            equipe_no_comando: false,
            rodizio_volta: false,
            hunt_desde: c.hunt_desde as f64,
        };
        hunt::previsao::prever(
            &config::carregar(&self.dados),
            &Relogio {
                agora_ms: self.agora,
                fuso_ms: -3 * 3_600_000,
            },
            &c,
            &ctx,
            ficha.as_ref(),
        )
    }
}

/// Somente leituras; o cálculo não grava regras nem manda comando ao jogo.
pub async fn entradas(
    db: &PgPool,
    nick: &str,
    agora: i64,
) -> Result<Option<Entradas>, sqlx::Error> {
    let row:Option<(Value,Option<Value>)>=sqlx::query_as("SELECT janela,login FROM nexus.motor_janela WHERE em>now()-interval '90 seconds' AND (lower(conta)=lower($1) OR lower(janela->>'nick')=lower($1)) LIMIT 1")
        .bind(nick).fetch_optional(db).await?;
    let Some((mut janela, login)) = row else {
        return Ok(None);
    };
    janela["welcome"] = login.unwrap_or(Value::Null);
    enriquecer(db, &mut janela, agora).await?;
    if let Some(laco) = crate::motor::carregar_laco(db).await? {
        if let Some(marcas) = laco["marcas"].as_array() {
            if let Some(m) = marcas
                .iter()
                .find(|m| m[0] == janela["nick"] && m[1] == "huntContextoDesde")
            {
                if let Some(t) = m[2].as_f64() {
                    let desde = janela["huntDesde"].as_f64().unwrap_or(0.0).max(t);
                    janela["huntDesde"] = json!(desde as i64);
                    if janela["antes"].is_object() {
                        janela["antes"]["huntDesde"] = json!(desde as i64);
                    }
                }
            }
        }
    }
    let dados = crate::motor::carregar_dados(db)
        .await
        .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    let dex = crate::equipes::carregar_pokedex(db).await?;
    Ok(Some(Entradas {
        janela,
        dados,
        dex,
        agora,
    }))
}

#[cfg(test)]
mod testes {
    use super::*;
    use testcontainers::{
        core::{IntoContainerPort, WaitFor},
        runners::AsyncRunner,
        GenericImage, ImageExt,
    };
    #[tokio::test]
    async fn sinais_nao_regridem_e_stats_exigem_mesmo_nivel(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let pg = GenericImage::new("postgres", "16.4-alpine")
            .with_exposed_port(5432.tcp())
            .with_wait_for(WaitFor::message_on_stderr(
                "database system is ready to accept connections",
            ))
            .with_env_var("POSTGRES_PASSWORD", "teste")
            .start()
            .await?;
        let porta = pg.get_host_port_ipv4(5432).await?;
        let db = PgPool::connect(&format!(
            "postgres://postgres:teste@127.0.0.1:{porta}/postgres"
        ))
        .await?;
        sqlx::raw_sql("CREATE SCHEMA nexus;CREATE TABLE nexus.parametro(chave text PRIMARY KEY,valor jsonb,atualizado_em timestamptz DEFAULT now());CREATE TABLE nexus.equipes_conta(nick text,dados jsonb);")
            .execute(&db).await?;
        let evento = |t, m| Evento {
            tipo: "ws.bruto".into(),
            em_ms: t,
            chave: vec![],
            sem_ts: false,
            dados: json!({"dados":{"t":"estado","estado":{"bonusXp":{"tr":m,"pk":m},"bicicleta":{"equipada":null}}}}),
        };
        registrar(&db, "sikien", &[evento(100, 2.0)]).await?;
        registrar(&db, "sikien", &[evento(50, 4.0)]).await?;
        let ficha = json!({"id":1,"level":100,"stats":{"hp":10,"atk":20,"def":30,"spAtk":40,"spDef":50,"speed":60},"maxHp":120});
        sqlx::query("INSERT INTO nexus.equipes_conta VALUES('Sikien',$1)")
            .bind(json!({"em":190,"pokemons":[ficha]}))
            .execute(&db)
            .await?;
        let mut j =
            json!({"id":"sikien","nick":"Sikien","ultimo":{"io":{"active":{"id":1,"level":100}}}});
        enriquecer(&db, &mut j, 200).await?;
        assert_eq!(j["ultimo"]["bonusXp"]["tr"], 2.0);
        assert_eq!(j["ultimo"]["io"]["active"]["maxHp"], 120);
        assert_eq!(j["ultimo"]["movimento"]["bicicletaEquipada"], Value::Null);
        let mut outro =
            json!({"id":"sikien","nick":"Sikien","ultimo":{"io":{"active":{"id":1,"level":101}}}});
        enriquecer(&db, &mut outro, 200).await?;
        assert!(outro["ultimo"]["io"]["active"]["stats"].is_null());
        outro["ultimo"]["t"] = json!(300);
        outro["ultimo"]["bonusXp"] = json!({"tr":3.0,"pk":3.0});
        outro["ultimo"]["movimento"] = json!({"bicicletaEquipada":9});
        enriquecer(&db, &mut outro, 400).await?;
        assert_eq!(outro["ultimo"]["bonusXp"]["tr"], 3.0);
        assert_eq!(outro["ultimo"]["movimento"]["bicicletaEquipada"], 9);
        Ok(())
    }
}

/// Clientes antigos não mandavam os bônus fora da Descoberta. A recompensa do último
/// abate permite recuperar o multiplicador efetivo sem misturar a média de bônus antigos.
fn bonus_dos_abates(j: &mut Value, agora: i64) {
    use nucleo::motor::{jogo, regras::ContaViva};
    let Some(c) = ContaViva::de_json(j) else {
        return;
    };
    let Some((nome, Some(nivel))) = jogo::hunt_atual(&c) else {
        return;
    };
    let mut ultima: Option<(f64, f64, i64)> = None;
    let mut primeiro_novo = 0;
    let mut corte = None;
    for (_, t, texto) in c.linhas.iter().rev() {
        let Some(m) = jogo::LINHA_ABATE.as_ref().and_then(|r| r.captures(texto)) else {
            continue;
        };
        if m.get(1).map(|m| m.as_str()) != Some(nome.as_str()) {
            continue;
        }
        let xp = |i| {
            m.get(i)
                .and_then(|m| jogo::num_texto(m.as_str()))
                .unwrap_or(0.0)
        };
        let (tr, pk) = (xp(2), xp(3));
        if let Some((utr, upk, _)) = ultima {
            if tr != utr || pk != upk {
                corte = Some(primeiro_novo);
                break;
            }
        } else {
            ultima = Some((tr, pk, *t));
        }
        primeiro_novo = *t;
    }
    if let Some(t) = corte {
        let desde = j["huntDesde"].as_i64().unwrap_or(0).max(t);
        j["huntDesde"] = json!(desde);
        if j["antes"].is_object() {
            j["antes"]["huntDesde"] = json!(desde);
        }
    }
    if !j["ultimo"]["bonusXp"].is_object() {
        if let Some((tr, pk, _t)) = ultima.filter(|(_, _, t)| (0..=90_000).contains(&(agora - *t)))
        {
            let base = jogo::xp_base(nivel);
            if base > 0.0 && tr > 0.0 && pk > 0.0 {
                j["ultimo"]["bonusXp"] = json!({"tr":tr/base,"pk":pk/base});
                j["ultimo"]["bonusXpFonte"] = json!("recompensa do último abate");
            }
        }
    }
}

#[cfg(test)]
mod testes_bonus_hud {
    use super::*;
    #[test]
    fn ultimo_abate_recupera_bonus_e_corta_a_amostra_na_expiracao() {
        let mut j = json!({"id":"sikien","nick":"Sikien","huntDesde":0,
            "ultimo":{"hunt":"Silcoon · nível 650"},"linhas":[
            [1,0,"Silcoon derrotado! +390.434 xp treinador, +390.434 xp pokemon, +100 ouro"],
            [2,10000,"Silcoon derrotado! +146.230 xp treinador, +146.230 xp pokemon, +100 ouro"],
            [3,20000,"Silcoon derrotado! +146.230 xp treinador, +146.230 xp pokemon, +100 ouro"]]});
        bonus_dos_abates(&mut j, 21000);
        assert!((j["ultimo"]["bonusXp"]["tr"].as_f64().unwrap_or(0.0) - 1.7325).abs() < 0.00001);
        assert_eq!(j["huntDesde"], 10000);
        j["ultimo"]["bonusXp"] = json!({"tr":2.0,"pk":3.0});
        bonus_dos_abates(&mut j, 21000);
        assert_eq!(j["ultimo"]["bonusXp"]["tr"], 2.0);
        j["ultimo"]["bonusXp"] = Value::Null;
        bonus_dos_abates(&mut j, 120001);
        assert!(j["ultimo"]["bonusXp"].is_null());
    }
}
