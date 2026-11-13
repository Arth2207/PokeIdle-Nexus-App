//! Precificador do dossiê (etapa 6b): `montarPrecos()`, `GET /precificador/precos.json`,
//! `GET /api/dossie/calc/lite` e `POST /api/dossie/calc/preco`.
//!
//! O legado roda o motor JS do precificador (`pve-pricing.js`, `central.js`, `nota.js`)
//! num contexto VM. Aqui o cálculo vem do `nucleo::precificador` (o mesmo porte que a bolsa
//! usa); este arquivo só monta as entradas e as respostas como o `dossie-server.js`.
//!
//! - Catálogo: `nexus.especie` (sincronizado dos `especies*.js`), carregado uma vez, como o
//!   `carregarMotor()` do legado.
//! - `precos.json` base: o arquivo do legado, lido a cada remontagem (cache de 10 min).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::header;
use axum::response::{IntoResponse, Response};
use nucleo::js as njs;
use nucleo::precificador::cotacao::{Cotacoes, PrecoItemDia};
use nucleo::precificador::especie::{lendario, Central, Especie, Golpe};
use nucleo::precificador::ficha::Ficha;
use nucleo::precificador::motor::{descrever, LinhaMotor};
use nucleo::precificador::nota::{nota, Entrada};
use nucleo::precificador::vizinhos::{
    vies_padrao, vizinhos_detalhe, Alvo, Indice, PESOS_CONFIANCA,
};
use nucleo::precificador::{DIA_MS, MEGA, MEGA_SHINY, POR_ADICIONAL};
use regex::Regex;
use serde_json::{json, Map, Value};
use sqlx::PgPool;
use tokio::sync::Mutex;

use super::{enviar, js, jsonjs, ler_json, Dossie, Erro, Pedido};

const JANELA_DIAS: i64 = 30;
const PRECOS_TTL: Duration = Duration::from_secs(600);
const NOTA_MIN: f64 = 3.5;
const STATS: [&str; 6] = ["hp", "atk", "def", "spAtk", "spDef", "speed"];
const LEVEL_STEPS: [f64; 16] = [
    20.0, 40.0, 60.0, 80.0, 100.0, 150.0, 200.0, 300.0, 500.0, 700.0, 1000.0, 1500.0, 2000.0,
    3000.0, 5000.0, 10000.0,
];
/// Dias máximos para cotar um adicional na decomposição das vendas (`FAR_ITEM_DAYS`).
const DIAS_ITEM_LONGE: i64 = 5;

fn caminho_base() -> String {
    std::env::var("NEXUS_DOSSIE_PRECOS_BASE")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/opt/mapper/engine/data/precos.json".into())
}

/// Catálogo na ordem da fonte (o `Map` do legado itera na ordem de inserção).
pub struct Catalogo {
    ordem: Vec<Especie>,
    central: Central,
}

/// `precos.json` montado + o que o cálculo deriva dele (`rowsCalc`).
pub struct Precos {
    valor: Value,
    /// Corpo do `precos.json` (`JSON.stringify` compacto).
    texto: Arc<Vec<u8>>,
    linhas: Vec<LinhaMotor>,
    /// Índices por lado (não shiny, shiny). `None` com menos de 3 vendas.
    indices: [Option<Indice>; 2],
    cotacoes: Cotacoes,
}

/// Estado do precificador dentro do `Dossie`.
#[derive(Default)]
pub struct Cache {
    catalogo: Mutex<Option<Arc<Catalogo>>>,
    precos: Mutex<Option<(Instant, Arc<Precos>)>>,
}

async fn carregar_catalogo(db: &PgPool) -> Result<Catalogo, sqlx::Error> {
    type Linha = (
        i32,
        String,
        Vec<String>,
        Vec<i32>,
        Value,
        Vec<i32>,
        Option<i32>,
        Option<i32>,
        Option<i32>,
    );
    let linhas: Vec<Linha> = sqlx::query_as(
        "SELECT id, nome, tipos, base, golpes, evolucoes, mega_base, sprite_id, desempate
           FROM nexus.especie ORDER BY ordem",
    )
    .fetch_all(db)
    .await?;
    let ordem: Vec<Especie> = linhas
        .into_iter()
        .filter_map(
            |(id, nome, tipos, base, golpes, evolucoes, mega_base, sprite_id, desempate)| {
                let base: [f64; 6] = base
                    .into_iter()
                    .map(f64::from)
                    .collect::<Vec<_>>()
                    .try_into()
                    .ok()?;
                Some(Especie {
                    id,
                    nome,
                    tipos,
                    base,
                    golpes: serde_json::from_value::<Vec<Golpe>>(golpes).unwrap_or_default(),
                    evolucoes,
                    mega_base,
                    sprite_id,
                    desempate,
                })
            },
        )
        .collect();
    let central = Central::novo(ordem.clone());
    Ok(Catalogo { ordem, central })
}

impl Cache {
    /// `carregarMotor()`: uma vez por processo.
    pub async fn catalogo(&self, db: &PgPool) -> Result<Arc<Catalogo>, Erro> {
        let mut g = self.catalogo.lock().await;
        if let Some(c) = g.as_ref() {
            return Ok(Arc::clone(c));
        }
        let c = Arc::new(carregar_catalogo(db).await?);
        *g = Some(Arc::clone(&c));
        Ok(c)
    }

    /// `precosAtual()`: remonta se passou de 10 min. Falha na montagem mantém o anterior; sem
    /// nenhum, usa o `precos.json` base cru (como o legado).
    pub async fn atual(&self, db: &PgPool, cat: &Arc<Catalogo>) -> Result<Arc<Precos>, Erro> {
        let mut g = self.precos.lock().await;
        if let Some((quando, p)) = g.as_ref() {
            if quando.elapsed() <= PRECOS_TTL {
                return Ok(Arc::clone(p));
            }
        }
        match montar(db, cat).await {
            Ok(p) => {
                let p = Arc::new(p);
                *g = Some((Instant::now(), Arc::clone(&p)));
                Ok(p)
            }
            Err(e) => {
                tracing::warn!(erro = %e.msg, "dossiê: montarPrecos falhou");
                if let Some((_, p)) = g.as_ref() {
                    return Ok(Arc::clone(p));
                }
                let bruto = ler_base().await?;
                let p = Arc::new(derivar(
                    serde_json::from_slice(&bruto).map_err(|e| Erro::novo(500, e.to_string()))?,
                    Some(bruto),
                ));
                *g = Some((Instant::now(), Arc::clone(&p)));
                Ok(p)
            }
        }
    }
}

async fn ler_base() -> Result<Vec<u8>, Erro> {
    let arq = caminho_base();
    tokio::fs::read(&arq).await.map_err(|e| {
        Erro::novo(
            500,
            if e.kind() == std::io::ErrorKind::NotFound {
                format!("ENOENT: no such file or directory, open '{arq}'")
            } else {
                e.to_string()
            },
        )
    })
}

/// Uma venda do banco (`fato_venda_mercado`), como o node-pg entrega.
struct VendaBanco {
    em: f64,
    tipo: Option<String>,
    moeda: Option<String>,
    preco: f64,
    descricao: Option<String>,
    species_id: Option<i32>,
    item_id: Option<i32>,
    quantidade: Option<i32>,
    ficha: Option<Value>,
}

fn agora_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// `new Date(ms).toISOString()`; `None` se o instante for inválido (o JS lança RangeError).
fn iso(ms: f64) -> Option<String> {
    if !ms.is_finite() {
        return None;
    }
    chrono::DateTime::from_timestamp_millis(ms as i64)
        .map(|d| d.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
}

/// `Date.parse(texto)` para o formato ISO que o legado grava.
fn data_ms(v: Option<&Value>) -> f64 {
    v.and_then(Value::as_str)
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map_or(f64::NAN, |d| d.timestamp_millis() as f64)
}

/// `mediana(xs)` do legado.
fn mediana(xs: &[f64]) -> f64 {
    let mut s = xs.to_vec();
    s.sort_by(f64::total_cmp);
    let m = s.len() >> 1;
    if s.len() % 2 == 1 {
        s[m]
    } else {
        (s[m - 1] + s[m]) / 2.0
    }
}

/// Item da junção (`[a,b].join('|')`): `null`/`undefined` viram vazio.
fn parte_chave(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        outro => js::texto(outro),
    }
}

/// `unicas(vs)`: tira vendas repetidas pela chave (at, price, speciesId, level, pve, nota, shiny).
fn unicas(vs: impl IntoIterator<Item = Value>) -> Vec<Value> {
    let mut vistos = HashSet::new();
    vs.into_iter()
        .filter(|v| {
            let k = ["at", "price", "speciesId", "level", "pve", "nota", "shiny"]
                .iter()
                .map(|c| parte_chave(v.get(*c)))
                .collect::<Vec<_>>()
                .join("|");
            vistos.insert(k)
        })
        .collect()
}

static NV: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r" Nv [0-9]+[^\n\r\u{2028}\u{2029}]*$").ok());

/// Venda de pokémon pronta para o `prepare` (o objeto `vendas[i]` do `montarPrecos`).
struct VendaPokemon {
    moeda: Option<String>,
    especie: Option<Value>,
    nome: String,
    em: f64,
    preco: f64,
    ficha: Value,
}

/// `P.prepare(central, vendas, itens)`: linhas + motivos de descarte (na ordem em que
/// apareceram, como as chaves do objeto `skipped`).
fn preparar(
    central: &Central,
    vendas: &[VendaPokemon],
    cotacoes: &Cotacoes,
) -> (Vec<Value>, Vec<(&'static str, u64)>) {
    let mut linhas = Vec::new();
    let mut descartes: Vec<(&'static str, u64)> = Vec::new();
    let mut pular = |motivo: &'static str| match descartes.iter_mut().find(|(m, _)| *m == motivo) {
        Some((_, n)) => *n += 1,
        None => descartes.push((motivo, 1)),
    };
    for s in vendas {
        if s.moeda.as_deref() != Some("orb") {
            pular("coins");
            continue;
        }
        // `find(s.speciesId ?? s.ficha?.speciesId, s.name)`: id pelo `Number(id)`, senão nome.
        let id_bruto = js::ou(s.especie.as_ref(), s.ficha.get("speciesId"));
        let id = Some(js::numero(id_bruto))
            .filter(|n| n.fract() == 0.0 && n.abs() < 2e9)
            .map(|n| n as i32)
            .filter(|n| central.especie(*n).is_some());
        let Some(especie) = id.or_else(|| central.achar(None, Some(&s.nome))) else {
            pular("espécie desconhecida");
            continue;
        };
        let ficha = Ficha::de_json(&s.ficha);
        let Some(d) = descrever(central, &ficha, especie) else {
            pular("ficha incompleta");
            continue;
        };
        let em = s.em as i64;
        let mut soma = 0.0;
        let mut longe = false;
        let mut faltou = false;
        for a in &d.adicionais {
            match cotacoes.perto(a.item_id, em, DIAS_ITEM_LONGE) {
                Some(q) => {
                    soma += q.preco * POR_ADICIONAL;
                    longe |= q.longe;
                }
                None => {
                    faltou = true;
                    break;
                }
            }
        }
        if faltou {
            pular("sem cotação dos Adicionais");
            continue;
        }
        let base = s.preco - soma;
        // `!(base > 0)`: NaN também reprova.
        if base.is_nan() || base <= 0.0 {
            pular("Adicionais acima do preço");
            continue;
        }
        // `readAt` = `at` nas vendas do banco: a leitura nunca é tardia, só `far` conta.
        let incerta = longe;
        let nota = s
            .ficha
            .get("nota")
            .and_then(Value::as_f64)
            .filter(|n| n.is_finite());
        linhas.push(json!({
            "at": s.em,
            "price": s.preco,
            "base": base,
            "addons": soma,
            "pve": d.pve,
            "family": d.familia,
            "familyName": d.familia_nome,
            "shiny": d.shiny,
            "level": d.nivel,
            "speciesId": especie,
            "name": s.nome,
            "uncertain": incerta,
            "nota": nota,
            "mega": d.adicionais.iter().any(|a| a.item_id == MEGA || a.item_id == MEGA_SHINY),
            "refined": ficha.refinada,
        }));
    }
    (linhas, descartes)
}

/// Cotações (`itemQuotes`) a partir da lista `itens` do `precos.json`.
fn cotacoes_de(itens: &[Value]) -> Cotacoes {
    Cotacoes::novo(itens.iter().filter_map(|i| {
        let preco = i.get("price").and_then(Value::as_f64)?;
        (i.get("currency").and_then(Value::as_str) == Some("orb") && preco > 0.0).then_some(())?;
        Some(PrecoItemDia {
            dia: njs::dia_da_data(i.get("day")?.as_str()?)?,
            item_id: i32::try_from(i.get("itemId")?.as_i64()?).ok()?,
            preco,
        })
    }))
}

/// `montarPrecos()`.
async fn montar(db: &PgPool, cat: &Arc<Catalogo>) -> Result<Precos, Erro> {
    let bruto = ler_base().await?;
    let desde = (agora_ms() - JANELA_DIAS * DIA_MS) as f64;
    type Linha = (
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i32>,
        Option<i32>,
        Option<i32>,
        Option<Value>,
    );
    // Mesma consulta do legado (sem ORDER BY): a ordem física decide os empates.
    let linhas: Vec<Linha> = sqlx::query_as(
        "SELECT em::text, tipo, moeda, preco::text, descricao, species_id, item_id, quantidade, ficha
           FROM public.fato_venda_mercado WHERE em >= $1",
    )
    .bind(desde as i64)
    .fetch_all(db)
    .await?;
    let vendas_banco: Vec<VendaBanco> = linhas
        .into_iter()
        .map(
            |(em, tipo, moeda, preco, descricao, species_id, item_id, quantidade, ficha)| {
                VendaBanco {
                    em: js::numero_str(&em),
                    tipo,
                    moeda,
                    preco: preco.as_deref().map_or(0.0, js::numero_str),
                    descricao,
                    species_id,
                    item_id,
                    quantidade,
                    ficha,
                }
            },
        )
        .collect();
    let cat = Arc::clone(cat);
    tokio::task::spawn_blocking(move || {
        let base: Value =
            serde_json::from_slice(&bruto).map_err(|e| Erro::novo(500, e.to_string()))?;
        let valor = montar_valor(base, &vendas_banco, &cat.central, desde)?;
        Ok(derivar(valor, None))
    })
    .await
    .map_err(|e| Erro::novo(500, e.to_string()))?
}

fn montar_valor(
    mut base: Value,
    rows: &[VendaBanco],
    central: &Central,
    desde: f64,
) -> Result<Value, Erro> {
    let obj = base
        .as_object_mut()
        .ok_or_else(|| Erro::novo(500, "precos.json base não é objeto"))?;
    let vendas_base: Vec<Value> = obj
        .get("vendas")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if rows.is_empty() {
        obj.insert("vendas".into(), Value::Array(unicas(vendas_base)));
        return Ok(base);
    }
    // Cotação diária (mediana do preço unitário em gemas) dos itens vendidos.
    let mut por_dia: Vec<(String, String, i64, Vec<f64>)> = Vec::new();
    for r in rows {
        let Some(item) = r.item_id.filter(|i| *i != 0) else {
            continue;
        };
        if r.tipo.as_deref() == Some("pokemon") || r.moeda.as_deref() != Some("orb") {
            continue;
        }
        let dia = njs::data_sp(r.em as i64);
        let chave = format!("{dia}|{item}");
        let qtd = f64::from(r.quantidade.filter(|q| *q != 0).unwrap_or(1));
        let unit = r.preco / qtd.max(1.0);
        match por_dia.iter_mut().find(|(k, ..)| *k == chave) {
            Some((.., v)) => v.push(unit),
            None => por_dia.push((chave, dia, i64::from(item), vec![unit])),
        }
    }
    let chaves: HashSet<&str> = por_dia.iter().map(|(k, ..)| k.as_str()).collect();
    let mut itens: Vec<Value> = obj
        .get("itens")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter(|i| {
                    let k = format!("{}|{}", js::texto(i.get("day")), js::texto(i.get("itemId")));
                    !chaves.contains(k.as_str())
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    itens.extend(por_dia.iter().map(|(_, dia, item, v)| {
        json!({ "day": dia, "itemId": item, "currency": "orb", "price": mediana(v) })
    }));

    let vendas: Vec<VendaPokemon> = rows
        .iter()
        .filter(|r| r.tipo.as_deref() == Some("pokemon"))
        .filter_map(|r| {
            let ficha = r.ficha.clone().filter(|f| js::verdade(Some(f)))?;
            let nome_ficha = ficha.get("nome");
            let nome = if js::verdade(nome_ficha) {
                js::texto(nome_ficha)
            } else {
                let d = r.descricao.clone().unwrap_or_default();
                NV.as_ref()
                    .map_or(d.clone(), |re| re.replacen(&d, 1, "").into_owned())
            };
            Some(VendaPokemon {
                moeda: r.moeda.clone(),
                especie: match r.species_id {
                    Some(id) => Some(Value::from(id)),
                    None => ficha.get("speciesId").cloned(),
                },
                nome,
                em: r.em,
                preco: r.preco,
                ficha,
            })
        })
        .collect();
    if vendas.is_empty() {
        obj.insert("itens".into(), Value::Array(itens));
        return Ok(base);
    }
    let cotacoes = cotacoes_de(&itens);
    let (novas, descartes) = preparar(central, &vendas, &cotacoes);
    let corte = vendas.iter().fold(f64::INFINITY, |m, v| m.min(v.em));
    let antigas = unicas(vendas_base.into_iter().filter(|v| {
        let at = js::numero(v.get("at"));
        at >= desde && at < corte
    }));
    let (n_novas, n_antigas) = (novas.len(), antigas.len());
    let mut todas: Vec<Value> = antigas.into_iter().chain(novas).collect();
    // `sort((a, b) => a.at - b.at)`: estável, como o do JS.
    todas.sort_by(|a, b| js::numero(a.get("at")).total_cmp(&js::numero(b.get("at"))));
    let ultima = todas
        .last()
        .map_or_else(|| data_ms(obj.get("geradoEm")), |v| js::numero(v.get("at")));
    let gerado = iso(ultima).ok_or_else(|| Erro::novo(500, "Invalid time value"))?;
    let desde_iso = iso(corte).ok_or_else(|| Erro::novo(500, "Invalid time value"))?;
    let descartadas: Map<String, Value> = descartes
        .into_iter()
        .map(|(m, n)| (m.to_owned(), Value::from(n)))
        .collect();
    obj.insert("geradoEm".into(), Value::from(gerado));
    obj.insert("vendas".into(), Value::Array(todas));
    obj.insert("itens".into(), Value::Array(itens));
    obj.insert(
        "coleta".into(),
        json!({ "novas": n_novas, "antigas": n_antigas, "descartadas": descartadas, "desde": desde_iso }),
    );
    Ok(base)
}

/// Venda do `precos.json` → linha do motor (`PokeIdleCentral.linhas`: `{...v, logBase}`).
/// Venda sem `shiny` booleano não entra em nenhum dos lados (`r.shiny === shiny`).
fn linha_de(v: &Value) -> Option<LinhaMotor> {
    let num = |c: &str| js::numero(v.get(c));
    let shiny = v.get("shiny").and_then(Value::as_bool)?;
    let base = num("base");
    Some(LinhaMotor {
        em: num("at") as i64,
        preco: num("price"),
        base,
        log_base: njs::ln(base),
        adicionais: num("addons"),
        mega: js::verdade(v.get("mega")),
        pve: num("pve"),
        familia: num("family") as i32,
        familia_nome: js::texto(v.get("familyName")),
        shiny,
        nivel: num("level"),
        especie: num("speciesId") as i32,
        nome: v.get("name").and_then(Value::as_str).map(str::to_owned),
        nota: v
            .get("nota")
            .and_then(Value::as_f64)
            .filter(|n| n.is_finite()),
        incerta: v.get("uncertain") == Some(&Value::Bool(true)),
        refinada: js::verdade(v.get("refined")),
    })
}

/// Deriva do `precos.json` o que o cálculo usa.
fn derivar(valor: Value, texto: Option<Vec<u8>>) -> Precos {
    let vendas = valor
        .get("vendas")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let linhas: Vec<LinhaMotor> = vendas.iter().filter_map(linha_de).collect();
    let itens = valor
        .get("itens")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let cotacoes = cotacoes_de(itens);
    let indices = [Indice::novo(&linhas, false), Indice::novo(&linhas, true)];
    let texto = texto.unwrap_or_else(|| jsonjs::compacto(&valor).into_bytes());
    Precos {
        valor,
        texto: Arc::new(texto),
        linhas,
        indices,
        cotacoes,
    }
}

/// `GET /precificador/precos.json` (só privado).
pub async fn rota_precos(d: &Dossie) -> Result<Response, Erro> {
    let cat = d.precos.catalogo(&d.db).await?;
    let p = d.precos.atual(&d.db, &cat).await?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/json; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        Body::from(p.texto.as_ref().clone()),
    )
        .into_response())
}

/// `central.spriteId(id)`.
fn sprite_id(cat: &Catalogo, base_nome: &HashMap<String, i32>, e: &Especie) -> Option<i32> {
    if let Some(s) = e.sprite_id {
        return Some(s);
    }
    let id = e.id;
    if id <= 1025 {
        return Some(id);
    }
    if id >= 13000 {
        return Some(id % 1000);
    }
    if (3000..4000).contains(&id) {
        return Some(id - 3000);
    }
    let _ = cat;
    // `porNome`: maior trecho do nome que é nome de uma espécie com sprite (≤ 1025).
    let n = njs::normalizar_nome(&e.nome).replace('-', " ");
    let w: Vec<&str> = n.split_whitespace().collect();
    for i in 0..w.len() {
        for j in (i + 1..=w.len()).rev() {
            if let Some(r) = base_nome.get(&w[i..j].join(" ")) {
                return Some(*r);
            }
        }
    }
    None
}

/// `GET /api/dossie/calc/lite`
pub async fn rota_lite(d: &Dossie, p: &Pedido) -> Result<Response, Erro> {
    if !d.limites.ok(&p.ip, 60) {
        return Ok(enviar(&json!({ "erro": "muitas requisições" }), 429));
    }
    let cat = d.precos.catalogo(&d.db).await?;
    let pr = d.precos.atual(&d.db, &cat).await?;
    let mut base_nome: HashMap<String, i32> = HashMap::new();
    for e in cat.ordem.iter().filter(|e| e.id <= 1025) {
        base_nome
            .entry(njs::normalizar_nome(&e.nome))
            .or_insert(e.id);
    }
    let ids: HashSet<i32> = cat.ordem.iter().map(|e| e.id).collect();
    let mut megas: Vec<i32> = Vec::new();
    for e in &cat.ordem {
        if let Some(b) = e.base_da_mega().filter(|b| ids.contains(b)) {
            if !megas.contains(&b) {
                megas.push(b);
            }
        }
    }
    let species: Vec<Value> = cat
        .ordem
        .iter()
        .map(|e| {
            json!([
                e.id,
                e.nome,
                e.evolucoes,
                e.mega_base,
                sprite_id(&cat, &base_nome, e),
                e.tipos
            ])
        })
        .collect();
    let v = &pr.valor;
    let mut status = Map::new();
    // Campo ausente some do JSON (undefined), como no legado.
    if let Some(g) = v.get("geradoEm") {
        status.insert("geradoEm".into(), g.clone());
    }
    if let Some(j) = v.get("janelaDias") {
        status.insert("janelaDias".into(), j.clone());
    }
    let n_vendas = v
        .get("vendas")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    status.insert("nVendas".into(), Value::from(n_vendas));
    let teste = v
        .get("teste")
        .filter(|t| js::verdade(Some(t)))
        .cloned()
        .unwrap_or(Value::Null);
    let conferencia = js::campo(v.get("confianca"), "conferencia")
        .filter(|c| js::verdade(Some(c)))
        .cloned()
        .unwrap_or(Value::Null);
    Ok(enviar(
        &json!({
            "species": species,
            "megas": megas,
            "status": status,
            "teste": teste,
            "confianca": { "conferencia": conferencia },
        }),
        200,
    ))
}

/// `limparFicha(f)`.
struct FichaLimpa {
    nome: String,
    nivel: Option<f64>,
    potencia: Option<f64>,
    qualidade: Option<f64>,
    nota: Option<f64>,
    shiny: bool,
    ivs: [f64; 6],
    refinada: bool,
}

/// `NUM(v)`: vazio ou não finito → `undefined`.
fn num(v: Option<&Value>) -> Option<f64> {
    match v {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.is_empty() => None,
        outro => Some(js::numero(outro)).filter(|x| x.is_finite()),
    }
}

/// `Object.keys(o)`: chaves de índice inteiro primeiro (crescente), depois as outras na
/// ordem de inserção.
fn chaves_js(o: &Map<String, Value>) -> Vec<&String> {
    let indice = |k: &str| {
        (k == "0"
            || (!k.starts_with('0') && !k.is_empty() && k.bytes().all(|b| b.is_ascii_digit())))
        .then(|| k.parse::<u64>().ok())
        .flatten()
        .filter(|n| *n < u64::from(u32::MAX))
    };
    let mut ints: Vec<(&String, u64)> =
        o.keys().filter_map(|k| indice(k).map(|n| (k, n))).collect();
    ints.sort_by_key(|(_, n)| *n);
    ints.into_iter()
        .map(|(k, _)| k)
        .chain(o.keys().filter(|k| indice(k).is_none()))
        .collect()
}

fn limpar_ficha(f: Option<&Value>) -> FichaLimpa {
    let f = f.filter(|v| js::verdade(Some(v)) && (v.is_object() || v.is_array()));
    let ivs_obj = js::campo(f, "ivs");
    let ivs = STATS.map(|k| {
        let r = njs::round(js::numero(js::campo(ivs_obj, k)));
        if r.is_nan() || r == 0.0 {
            0.0
        } else {
            r
        }
    });
    let nome = if js::verdade(js::campo(f, "nome")) {
        js::fatia_utf16(&js::texto(js::campo(f, "nome")), 80)
    } else {
        String::new()
    };
    let refino = js::campo(f, "refinoActual").filter(|v| js::verdade(Some(v)));
    let refinada = match refino {
        Some(Value::Object(o)) => !chaves_js(o).is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        _ => false,
    };
    FichaLimpa {
        nome,
        nivel: num(js::campo(f, "level")),
        potencia: num(js::campo(f, "potencia")),
        qualidade: num(js::campo(f, "quality")),
        nota: num(js::campo(f, "nota")),
        shiny: js::campo(f, "shiny") == Some(&Value::Bool(true)),
        ivs,
        refinada,
    }
}

fn entrada(s: &FichaLimpa) -> Option<Entrada> {
    // Potência fracionária vira NaN no legado (sem nota).
    let potencia = s.potencia.filter(|p| p.fract() == 0.0)?;
    Some(Entrada {
        ivs: s.ivs.map(|v| v as i64),
        qualidade: s.qualidade.unwrap_or(f64::NAN),
        potencia: potencia as i64,
        shiny: s.shiny,
    })
}

/// `notaDe(m, sheet)`.
fn nota_de(cat: &Catalogo, s: &FichaLimpa) -> Option<f64> {
    if let Some(n) = s.nota {
        return Some(n);
    }
    let id = cat.central.achar(None, Some(&s.nome))?;
    nota(&cat.central, id, &entrada(s)?).filter(|v| v.is_finite())
}

/// `toLocaleString('pt-BR', { maximumFractionDigits: 2 })` para notas (0 a 10).
fn fmt_nota(v: f64) -> String {
    let t = njs::to_fixed(v, 2);
    let t = if t.contains('.') {
        t.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        t
    };
    t.replace('.', ",")
}

/// `travaNota(m, sheet)`: mensagem quando a nota fica abaixo de 3,5 até na última evolução.
fn trava_nota(cat: &Catalogo, s: &FichaLimpa) -> Option<String> {
    let id = cat.central.achar(None, Some(&s.nome))?;
    if s.shiny || s.potencia == Some(5.0) || lendario(id) {
        return None;
    }
    let agora = nota_de(cat, s).filter(|v| v.is_finite())?;
    if agora >= NOTA_MIN {
        return None;
    }
    let finais = cat.central.finais(id);
    let ent = entrada(s);
    let melhor = finais.iter().fold(agora, |m, f| {
        let n = ent
            .as_ref()
            .and_then(|e| nota(&cat.central, f.id, e))
            .unwrap_or(-1.0);
        m.max(n)
    });
    if melhor >= NOTA_MIN {
        return None;
    }
    let extra = if finais.iter().any(|f| f.id != id) {
        format!(
            " e nem na última evolução chega a 3,5 (no máximo {})",
            fmt_nota(melhor)
        )
    } else {
        String::new()
    };
    Some(format!(
        "Nota {}: abaixo de 3,5{extra}. O precificador não calcula preço para Pokémon com nota abaixo de 3,5.",
        fmt_nota(agora)
    ))
}

/// `chainCap(central, id)`: teto de nível pela posição na cadeia de evolução.
fn teto_cadeia(cat: &Catalogo, id: i32) -> f64 {
    let ids: HashSet<i32> = cat.ordem.iter().map(|e| e.id).collect();
    let mut up: HashMap<i32, i32> = cat.ordem.iter().map(|e| (e.id, 0)).collect();
    let mut down = up.clone();
    for _ in 0..8 {
        let mut mudou = false;
        for e in &cat.ordem {
            for &k in &e.evolucoes {
                if k == e.id || !ids.contains(&k) {
                    continue;
                }
                let (ui, uk) = (up[&e.id], up[&k]);
                if ui + 1 > uk {
                    up.insert(k, ui + 1);
                    mudou = true;
                }
                let (dk, di) = (down[&k], down[&e.id]);
                if dk + 1 > di {
                    down.insert(e.id, dk + 1);
                    mudou = true;
                }
            }
        }
        if !mudou {
            break;
        }
    }
    let estagio = up.get(&id).copied().unwrap_or(0);
    let total = (estagio + down.get(&id).copied().unwrap_or(0) + 1).min(4);
    let escada: &[f64] = match total {
        1 => &[100.0],
        2 => &[40.0, 100.0],
        3 => &[20.0, 40.0, 100.0],
        _ => &[20.0, 40.0, 70.0, 100.0],
    };
    escada[(estagio.max(0) as usize).min(escada.len() - 1)]
}

/// `dropLevel(central, rows, speciesId)`.
fn nivel_drop(cat: &Catalogo, linhas: &[LinhaMotor], especie: i32) -> f64 {
    let base = cat
        .central
        .especie(especie)
        .and_then(Especie::base_da_mega)
        .filter(|b| cat.central.especie(*b).is_some());
    let id = base.unwrap_or(especie);
    let mut conta: Vec<(f64, usize)> = Vec::new();
    for r in linhas.iter().filter(|r| r.especie == id) {
        match conta.iter_mut().find(|(l, _)| *l == r.nivel) {
            Some((_, n)) => *n += 1,
            None => conta.push((r.nivel, 1)),
        }
    }
    let total: usize = conta.iter().map(|(_, n)| n).sum();
    let mut ordenado = conta.clone();
    ordenado.sort_by_key(|x| std::cmp::Reverse(x.1));
    match ordenado.first() {
        Some((nivel, n)) if *n >= 5 && *n as f64 / total as f64 >= 0.3 => *nivel,
        _ => teto_cadeia(cat, id),
    }
}

/// `priceSheet(central, sheet, {..., details: true})`. `Err` = mensagem do `throw`.
#[allow(clippy::too_many_lines)]
fn preco_ficha(
    cat: &Catalogo,
    pr: &Precos,
    s: &FichaLimpa,
    nota_preco: Option<f64>,
    z_move: bool,
    aoe: bool,
    itens_atuais: &HashMap<String, f64>,
) -> Result<Value, String> {
    let central = &cat.central;
    let especie = central
        .achar(None, Some(&s.nome))
        .ok_or_else(|| format!("Espécie não reconhecida: {}.", js::fatia_utf16(&s.nome, 60)))?;
    let ivs: Option<[i32; 6]> = s
        .ivs
        .iter()
        .map(|v| i32::try_from(*v as i64).ok())
        .collect::<Option<Vec<_>>>()
        .and_then(|v| v.try_into().ok());
    let ficha = Ficha {
        especie: Some(especie),
        nome: Some(s.nome.clone()),
        ivs,
        qualidade: s.qualidade,
        potencia: s
            .potencia
            .filter(|p| p.fract() == 0.0 && p.abs() < 1e9)
            .map(|p| p as i32),
        nivel: s.nivel,
        shiny: s.shiny,
        shiny_frouxo: s.shiny,
        tm_elemental: z_move.then(|| "NORMAL".to_owned()),
        tm_aoe: aoe,
        ..Ficha::default()
    };
    let alvo_d = descrever(central, &ficha, especie)
        .ok_or_else(|| "Não foi possível calcular o PvE desta ficha.".to_owned())?;
    let agora_v = data_ms(pr.valor.get("geradoEm"));
    let agora = agora_v as i64;
    let cot = &pr.cotacoes;
    let partes: Vec<Value> = alvo_d
        .adicionais
        .iter()
        .map(|a| {
            let atual = itens_atuais
                .get(&a.item_id.to_string())
                .copied()
                .unwrap_or(f64::NAN);
            let cotacao = cot.preco(a.item_id, agora);
            let unidade = if atual > 0.0 { Some(atual) } else { cotacao };
            json!({
                "id": a.item_id,
                "label": a.rotulo,
                "quantity": POR_ADICIONAL,
                "unit": unidade,
                "quote": cotacao,
                "total": unidade.map(|u| njs::round(u * POR_ADICIONAL)),
            })
        })
        .collect();
    let conhecidos = partes.iter().fold(0.0, |s, p| {
        s + p["total"].as_f64().filter(|t| *t != 0.0).unwrap_or(0.0)
    });
    let faltam: Vec<Value> = partes
        .iter()
        .filter(|p| p["total"].is_null())
        .map(|p| p["label"].clone())
        .collect();
    let e = central.especie(especie);
    let tipos: Vec<String> = e.map(|e| e.tipos.clone()).unwrap_or_default();
    let mut r = json!({
        "name": e.map(|e| e.nome.clone()),
        "speciesId": especie,
        "types": tipos,
        "family": alvo_d.familia_nome,
        "familyId": alvo_d.familia,
        "shiny": alvo_d.shiny,
        "pve": alvo_d.pve,
        "level": alvo_d.nivel,
        "refined": s.refinada,
        "parts": partes,
        "missing": faltam,
    });
    let nota_t = nota_preco.filter(|n| n.is_finite());
    let pesos = pr
        .valor
        .get("confianca")
        .and_then(|c| c.get("pesos"))
        .and_then(Value::as_array)
        .and_then(|a| {
            a.iter()
                .map(Value::as_f64)
                .collect::<Option<Vec<f64>>>()
                .and_then(|v| <[f64; 5]>::try_from(v).ok())
        })
        .unwrap_or(PESOS_CONFIANCA);
    let vies = js::campo(pr.valor.get("correcao"), "bias")
        .and_then(Value::as_f64)
        .filter(|b| b.is_finite())
        .unwrap_or_else(vies_padrao);
    let indice = pr.indices[usize::from(alvo_d.shiny)].as_ref();
    let alvo = Alvo {
        pve: alvo_d.pve,
        nivel: alvo_d.nivel,
        nota: nota_t,
        familia: alvo_d.familia,
        familia_nome: alvo_d.familia_nome.clone(),
        em: agora,
    };
    let det = indice.and_then(|ix| {
        vizinhos_detalhe(
            &alvo,
            ix,
            conhecidos,
            |id| central.tipos(id).to_vec(),
            |shiny, em| {
                cot.preco(if shiny { MEGA_SHINY } else { MEGA }, em)
                    .unwrap_or(0.0)
            },
            &pesos,
            vies,
        )
        .map(|d| (ix, d))
    });
    let Some((ix, det)) = det else {
        if let Some(o) = r.as_object_mut() {
            o.insert("available".into(), Value::Bool(false));
        }
        return Ok(r);
    };
    let fit = &ix.ajuste;
    let ln = njs::ln;
    let alvo_fit = fit.avaliar(ln(alvo_d.pve), ln(alvo_d.nivel), nota_t);
    let vendas_familia: Vec<Value> = pr
        .linhas
        .iter()
        .filter(|l| l.familia == alvo_d.familia && l.shiny == alvo_d.shiny && l.base > 0.0)
        .map(|l| {
            json!({
                "at": l.em,
                "base": l.base,
                "adj": njs::exp(alvo_fit + l.log_base - fit.avaliar(ln(l.pve), ln(l.nivel), l.nota) - vies),
            })
        })
        .collect();
    let comps: Vec<Value> = det
        .influencia
        .iter()
        .filter_map(|&(i, share, perto)| {
            let l = ix.linha(i)?;
            Some(json!({
                "share": share,
                "closeness": njs::round(perto * 1000.0) / 1000.0,
                "at": l.em,
                "price": l.preco,
                "base": njs::round(l.base),
                "level": l.nivel,
                "pve": njs::round(l.pve),
                "nota": l.nota,
                "speciesId": l.especie,
                "name": l.nome,
                "shiny": l.shiny,
                "mega": l.mega,
                "uncertain": l.incerta,
                "refined": l.refinada,
                "types": central.tipos(l.especie),
            }))
        })
        .collect();
    let drop = nivel_drop(cat, &pr.linhas, especie);
    let topo = pr.linhas.iter().fold(alvo_d.nivel, |m, l| {
        if l.shiny == alvo_d.shiny && l.nivel > m {
            l.nivel
        } else {
            m
        }
    });
    let de = alvo_d.nivel.min(drop);
    let mut niveis: Vec<f64> = Vec::new();
    for l in LEVEL_STEPS
        .iter()
        .copied()
        .filter(|l| *l > de && *l <= topo)
        .chain([de, alvo_d.nivel])
    {
        if !niveis.contains(&l) {
            niveis.push(l);
        }
    }
    niveis.sort_by(f64::total_cmp);
    let aqui = fit.nivel(ln(alvo_d.nivel));
    let est = &det.estimativa;
    let por_nivel: Vec<Value> = niveis
        .iter()
        .map(|&nivel| {
            let k = njs::exp(fit.nivel(ln(nivel)) - aqui);
            let p = |x: f64| njs::round(x * k + conhecidos).max(1.0);
            json!({
                "level": nivel,
                "low": p(est.low - conhecidos),
                "median": p(det.base),
                "high": p(est.high - conhecidos),
                "current": nivel == alvo_d.nivel,
                "drop": nivel == drop,
            })
        })
        .collect();
    let [prox, certeza, consist, ampl] = det.partes;
    let extra = json!({
        "available": true,
        "low": est.low,
        "median": est.median,
        "high": est.high,
        "byLevel": por_nivel,
        "dropLevel": drop,
        "familySales": vendas_familia,
        "base": njs::round(det.base),
        "weight": ampl,
        "familyCount": est.familia_total,
        "marketCount": est.mercado_total,
        "uncertain": { "family": det.incertas_familia, "market": det.incertas_mercado },
        "confidenceParts": { "proximidade": prox, "certeza": certeza, "consistencia": consist, "ampliacao": ampl },
        "confidence": {
            "chance": est.confianca.chance,
            "percent": est.confianca.percent,
            "lowest": est.confianca.lowest,
        },
        "comps": comps,
    });
    if let (Some(o), Value::Object(x)) = (r.as_object_mut(), extra) {
        o.extend(x);
    }
    Ok(r)
}

/// `POST /api/dossie/calc/preco`
pub async fn rota_preco(d: &Dossie, p: &Pedido, corpo: Body) -> Result<Response, Erro> {
    if !d.limites.ok(&p.ip, 60) {
        return Ok(enviar(
            &json!({ "erro": "muitas requisições, tenta de novo em 1 min" }),
            429,
        ));
    }
    let b = ler_json(corpo).await?;
    if b.is_null() {
        return Err(Erro::novo(
            500,
            "Cannot read properties of null (reading 'sheet')",
        ));
    }
    let s = limpar_ficha(b.get("sheet"));
    if s.nome.is_empty() {
        return Ok(enviar(
            &json!({ "erro": "Não encontrei a espécie no texto." }),
            400,
        ));
    }
    let cat = d.precos.catalogo(&d.db).await?;
    let pr = d.precos.atual(&d.db, &cat).await?;
    if let Some(lock) = trava_nota(&cat, &s) {
        return Ok(enviar(&json!({ "lock": lock }), 200));
    }
    let nota_preco = nota_de(&cat, &s);
    let mut itens: HashMap<String, f64> = HashMap::new();
    if let Some(o) = b.get("items").and_then(Value::as_object) {
        for k in chaves_js(o).into_iter().take(20) {
            let v = js::numero(o.get(k));
            if v > 0.0 {
                itens.insert(k.clone(), v);
            }
        }
    }
    let z_move = js::verdade(b.get("zmove"));
    let aoe = js::verdade(b.get("aoe"));
    let cat2 = Arc::clone(&cat);
    let pr2 = Arc::clone(&pr);
    let r = tokio::task::spawn_blocking(move || {
        preco_ficha(&cat2, &pr2, &s, nota_preco, z_move, aoe, &itens)
    })
    .await
    .map_err(|e| Erro::novo(500, e.to_string()))?;
    match r {
        Ok(r) => Ok(enviar(&json!({ "r": r, "notaPreco": nota_preco }), 200)),
        Err(msg) => Ok(enviar(&json!({ "erro": msg }), 422)),
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn unicas_pela_chave_do_legado() {
        let v = vec![
            json!({"at": 1, "price": 2, "speciesId": 3, "level": 4, "pve": 5, "nota": null, "shiny": false}),
            json!({"at": 1, "price": 2, "speciesId": 3, "level": 4, "pve": 5, "shiny": false}),
            json!({"at": 1, "price": 2, "speciesId": 3, "level": 4, "pve": 5, "nota": 1, "shiny": false}),
        ];
        assert_eq!(unicas(v).len(), 2);
    }

    #[test]
    fn mediana_par_e_impar() {
        assert_eq!(mediana(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(mediana(&[4.0, 1.0, 2.0, 3.0]), 2.5);
    }

    #[test]
    fn chaves_inteiras_primeiro() {
        let o = json!({"b": 1, "10": 2, "2": 3, "a": 4, "01": 5});
        let k: Vec<&String> = chaves_js(o.as_object().unwrap());
        assert_eq!(k, ["2", "10", "b", "a", "01"]);
    }

    #[test]
    fn nota_formatada_pt_br() {
        assert_eq!(fmt_nota(3.0), "3");
        assert_eq!(fmt_nota(2.45), "2,45");
        assert_eq!(fmt_nota(3.125), "3,13");
        assert_eq!(fmt_nota(3.1), "3,1");
    }

    #[test]
    fn iso_como_o_js() {
        assert_eq!(iso(0.0).as_deref(), Some("1970-01-01T00:00:00.000Z"));
        assert_eq!(iso(f64::NAN), None);
    }

    #[test]
    fn nome_sem_nivel() {
        let re = NV.as_ref().unwrap();
        assert_eq!(re.replacen("Pikachu Nv 50 shiny", 1, ""), "Pikachu");
        assert_eq!(re.replacen("Pikachu", 1, ""), "Pikachu");
    }
}
