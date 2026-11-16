//! Contexto atual e medições de Outlands. Não atribui tier a eventos antigos.
use serde_json::{json, Value};
use sqlx::PgPool;

pub async fn contexto(db: &PgPool, conta: &str) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT jsonb_build_object('em',extract(epoch FROM j.em)*1000,'janela',j.janela,'hunts',j.login->'hunts',
          'regras',jsonb_build_object(
            'outlandTiers',coalesce(nullif(j.login->'outlandTiers','null'),w.p->'outlandTiers'),
            'chanceBossToken',coalesce(nullif(j.login->'chanceBossToken','null'),w.p->'chanceBossToken'),
            'outlandDex',coalesce(nullif(j.login->'outlandDex','null'),w.p->'outlandDex'),
            'outlandRegrasId',coalesce(nullif(j.login->'outlandRegrasId','null'),to_jsonb('welcome:'||extract(epoch FROM w.em)::text))))
         FROM nexus.motor_janela j LEFT JOIN LATERAL
           (SELECT e.em,e.dados->'payload' p FROM nexus.evento e
             WHERE e.tipo='welcome' AND e.conta IN (j.conta,lower(j.janela->>'nick'))
               AND e.em>now()-interval '30 days' ORDER BY e.em DESC LIMIT 1) w ON true
         WHERE j.conta=$1 OR lower(j.janela->>'nick')=$1 ORDER BY j.em DESC LIMIT 1")
        .bind(conta.trim().to_lowercase()).fetch_optional(db).await.map(|v| v.unwrap_or(Value::Null))
}

pub async fn medidas(db: &PgPool, conta: &str, regra: Option<&str>) -> Result<Value, sqlx::Error> {
    let linhas: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('tipo',tipo,'especie',coalesce(dados->>'nome',dados->>'alvo_especie'),
             'slug',dados#>>'{contexto,hunt_slug}','tier',dados#>>'{contexto,outland_tier}','regra',dados#>>'{contexto,regras_id}',
             'ativo',dados#>>'{contexto,ativo_id}','ativo_nivel',dados#>>'{contexto,ativo_nivel}',
             'loot',dados#>'{contexto,loot}','item_id',dados->'item_id','item',dados->>'item_nome',
             'qtd',sum(CASE WHEN dados->>'quantidade' ~ '^[0-9]+$' THEN (dados->>'quantidade')::numeric ELSE 1 END),
             'ouro',sum(CASE WHEN dados->>'ouro' ~ '^[0-9.]+$' THEN (dados->>'ouro')::numeric ELSE 0 END))
         FROM nexus.evento WHERE tipo IN ('kill','drop') AND em>now()-interval '24 hours'
           AND (conta=$1 OR conta_id=$1 OR conta=(SELECT lower(janela->>'nick') FROM nexus.motor_janela WHERE conta=$1))
         GROUP BY tipo,coalesce(dados->>'nome',dados->>'alvo_especie'),dados#>>'{contexto,hunt_slug}',dados#>>'{contexto,outland_tier}',
           dados#>>'{contexto,regras_id}',dados#>>'{contexto,ativo_id}',dados#>>'{contexto,ativo_nivel}',
           dados#>'{contexto,loot}',dados->'item_id',dados->>'item_nome'")
        .bind(conta.trim().to_lowercase()).fetch_all(db).await?;
    let desconhecidos: f64 = linhas
        .iter()
        .filter(|l| l["tipo"] == "kill" && l["tier"].is_null())
        .filter_map(|l| l["qtd"].as_f64())
        .sum();
    // Denominador por contexto: usa sinais de vida, incluindo intervalos sem drops.
    let periodos:Vec<Value>=sqlx::query_scalar(
        "SELECT jsonb_build_object('slug',dados#>>'{contexto,hunt_slug}','tier',dados#>>'{contexto,outland_tier}',
          'regra',dados#>>'{contexto,regras_id}','ativo',dados#>>'{contexto,ativo_id}',
          'ativo_nivel',dados#>>'{contexto,ativo_nivel}','loot',dados#>'{contexto,loot}',
          'horas',sum(segundos)/3600.0) FROM (
          SELECT dados,CASE WHEN lag(dados->'contexto') OVER (PARTITION BY conta ORDER BY em) = dados->'contexto'
            THEN least(30,greatest(0,extract(epoch FROM em-lag(em) OVER (PARTITION BY conta ORDER BY em)))) ELSE 0 END segundos
          FROM nexus.evento WHERE tipo='hunt.contexto' AND em>now()-interval '24 hours'
            AND (conta=$1 OR conta_id=$1 OR conta=(SELECT lower(janela->>'nick') FROM nexus.motor_janela WHERE conta=$1))
          ) s GROUP BY dados#>>'{contexto,hunt_slug}',dados#>>'{contexto,outland_tier}',dados#>>'{contexto,regras_id}',
            dados#>>'{contexto,ativo_id}',dados#>>'{contexto,ativo_nivel}',dados#>'{contexto,loot}'")
        .bind(conta.trim().to_lowercase()).fetch_all(db).await?;
    Ok(
        json!({"linhas":linhas,"periodos":periodos,"abates_sem_contexto":desconhecidos,"regra_atual":regra}),
    )
}

/// Mediana de preço unitário de vendas de tokens em gemas (não usa anúncios).
pub async fn preco_token(db: &PgPool, agora: i64) -> Result<Option<f64>, sqlx::Error> {
    sqlx::query_scalar("SELECT percentile_cont(0.5) WITHIN GROUP (ORDER BY preco::float8 / quantidade)
       FROM public.fato_venda_mercado WHERE em >= $1 AND em <= $2 AND moeda='orb' AND tipo <> 'pokemon'
         AND preco>0 AND quantidade>0 AND lower(descricao) LIKE '%bronze boss token%' HAVING count(*)>=3")
       .bind(agora-7*86_400_000).bind(agora).fetch_optional(db).await.map(|v:Option<Option<f64>>|v.flatten())
}
