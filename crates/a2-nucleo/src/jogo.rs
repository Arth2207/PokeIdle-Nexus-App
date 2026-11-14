//! Lógica pura da ponte com o jogo (sem Tauri): leitura do coletor, comando para chamada JS,
//! roteamento por conta e a captura do tracker. A casca `apps/desktop` só cola as peças.

use crate::comandos::Executor;
use contrato::ws::ComandoNovo;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;

/// Tamanho máximo do resultado de comando (`docs/websocket.md`).
pub const RESULTADO_MAX: usize = 200;
/// Pausa entre passos de um mesmo comando (o legado usava 120 a 150 ms).
const PAUSA_PASSOS: Duration = Duration::from_millis(150);
/// Limite do texto de uma captura do tracker.
pub const TRACKER_TEXTO_MAX: usize = 200_000;

/// Corta em `max` caracteres.
pub fn limitar(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

// ---------------------------------------------------------------- roteamento por conta

/// Rótulo da janela de uma conta: `jogo-` + nick normalizado, com todo caractere fora de
/// `[a-z0-9-]` como `_` e dois dígitos hex por byte UTF-8 (reversível; o rótulo do Tauri só
/// aceita alfanuméricos, `-`, `/`, `:` e `_`).
pub fn rotulo_da_conta(conta: &str) -> String {
    let mut s = String::from("jogo-");
    for b in conta.trim().to_lowercase().bytes() {
        if b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' {
            s.push(b as char);
        } else {
            s.push_str(&format!("_{b:02x}"));
        }
    }
    s
}

/// Inverso de [`rotulo_da_conta`]; `None` se o rótulo não é de janela de jogo.
pub fn conta_do_rotulo(rotulo: &str) -> Option<String> {
    let resto = rotulo.strip_prefix("jogo-")?;
    let b = resto.as_bytes();
    let mut bytes = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'_' {
            let h = resto.get(i + 1..i + 3)?;
            bytes.push(u8::from_str_radix(h, 16).ok()?);
            i += 3;
        } else {
            bytes.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(bytes).ok()
}

// ---------------------------------------------------------------- leitura do coletor

/// Número no formato do jogo: `.` separa milhar, `,` a fração (igual ao `num` do relay legado).
fn num_br(s: &str) -> f64 {
    s.replace('.', "").replace(',', ".").parse().unwrap_or(0.0)
}

fn campo<'a>(o: &'a Value, nomes: &[&str]) -> Option<&'a Value> {
    nomes
        .iter()
        .find_map(|n| o.get(*n))
        .filter(|v| !v.is_null())
}

fn copia(o: &Value, nomes: &[&str]) -> Value {
    campo(o, nomes).cloned().unwrap_or(Value::Null)
}

/// Linha do HUD de abate: `<nome> derrotado! +X xp treinador, +Y xp pokemon, +Z ouro...`.
fn linha_kill(t: &str) -> Option<(String, f64, f64, f64)> {
    let (nome, resto) = t.split_once(" derrotado! +")?;
    let (xt, resto) = resto.split_once(" xp treinador, +")?;
    let (xp, resto) = resto.split_once(" xp pokemon, +")?;
    let (ouro, _) = resto.split_once(" ouro")?;
    Some((nome.to_string(), num_br(xt), num_br(xp), num_br(ouro)))
}

/// Linha do HUD de captura: `Capturou <espécie> Nv.<n>! Qualidade: <q>...`.
fn linha_captura(t: &str) -> Option<(String, i64, f64)> {
    let resto = t.strip_prefix("Capturou ")?;
    let (especie, resto) = resto.split_once(" Nv.")?;
    let (nivel, resto) = resto.split_once("! Qualidade: ")?;
    let q: String = resto
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == ',')
        .collect();
    Some((especie.to_string(), nivel.parse().ok()?, num_br(&q)))
}

fn evento_batalha(ev: &Value, ts: i64, saida: &mut Vec<Value>) {
    match ev.get("k").and_then(Value::as_str) {
        Some("morte") => {
            let quem = ev.get("quem").and_then(Value::as_str);
            if quem.is_some_and(|q| q != "selvagem") {
                return;
            }
            let drops = ev.get("drops").cloned().unwrap_or_else(|| json!([]));
            saida.push(json!({
                "k": "kill", "ts": ts,
                "slot": copia(ev, &["slot"]),
                "nome": copia(ev, &["nome", "especie"]),
                "nivel": copia(ev, &["nivel", "level"]),
                "xp_treinador": copia(ev, &["xpTreinador", "xp_treinador"]),
                "xp_pokemon": copia(ev, &["xpPokemon", "xp_pokemon"]),
                "ouro": copia(ev, &["ouro"]),
                "ouro_loot_bonus": copia(ev, &["ouroLoot", "ouro_loot_bonus"]),
                "drops": drops,
                "hunt_slug": copia(ev, &["huntSlug", "hunt_slug"]),
                "shiny": ev.get("shiny").cloned().unwrap_or(json!(0)),
            }));
            for d in ev
                .get("drops")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                saida.push(json!({
                    "k": "drop", "ts": ts,
                    "slot": copia(ev, &["slot"]),
                    "alvo_especie": copia(ev, &["nome"]),
                    "item_id": copia(d, &["id", "item_id"]),
                    "item_nome": copia(d, &["nome", "name"]),
                    "quantidade": campo(d, &["qt", "quantidade"]).cloned().unwrap_or(json!(1)),
                }));
            }
        }
        Some("capturado") => {
            let p = ev.get("pokemon").cloned().unwrap_or(Value::Null);
            let iv_total = p
                .get("ivs")
                .and_then(Value::as_object)
                .map(|m| m.values().filter_map(Value::as_i64).sum::<i64>());
            saida.push(json!({
                "k": "capturado", "ts": ts,
                "id": copia(&p, &["id"]),
                "especie": copia(&p, &["nome", "especie"]),
                "nivel": copia(&p, &["level", "nivel"]),
                "ivs": copia(&p, &["ivs"]),
                "iv_total": iv_total,
                "quality": copia(&p, &["quality"]),
                "potencia": copia(&p, &["potencia"]),
                "shiny": p.get("shiny").cloned().unwrap_or(json!(0)),
                "bola": copia(ev, &["bola"]),
            }));
        }
        Some("bola") => saida.push(json!({
            "k": "bola", "ts": ts,
            "slot": copia(ev, &["slot"]),
            "ball_id": copia(ev, &["ballId", "ball_id"]),
            "sucesso": ev.get("sucesso").cloned().unwrap_or(json!(0)),
            "chance_pct": copia(ev, &["chance"]),
            "shiny": ev.get("shiny").cloned().unwrap_or(json!(0)),
            "alvo_especie": copia(ev, &["nome", "especie"]),
            "alvo_nivel": copia(ev, &["nivel", "level"]),
        })),
        _ => {}
    }
}

/// Mensagem do coletor -> eventos do `/ingest` (`k`, `ts`, ...). `ts` é o instante local (ms).
/// Formas aceitas (`t`): `welcome`, `batalha` (`ev[]` do WS do jogo), `hud` (`linhas`
/// `[seq, ts, texto]`, `hunt_slug`), `mercado` (`avisos[]`), `inventario`, `perfil`.
/// `tracker.captura` não passa aqui (ver [`evento_tracker`]). Desconhecido dá lista vazia.
/// Tamanho máximo (bytes) da mensagem bruta guardada pelo modo Descoberta.
pub const BRUTO_MAX: usize = 16 * 1024;

/// Chaves cujo valor nunca sai do app (sessão, login).
fn chave_secreta(k: &str) -> bool {
    let k = k.to_lowercase();
    [
        "token", "senha", "password", "passwd", "session", "sessao", "auth", "cookie", "jwt",
        "secret", "segredo", "apikey", "api_key",
    ]
    .iter()
    .any(|p| k.contains(p))
}

fn redigir(v: &mut Value) {
    match v {
        Value::Object(m) => {
            for (k, x) in m.iter_mut() {
                if chave_secreta(k) {
                    *x = Value::String("<removido>".into());
                } else {
                    redigir(x);
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(redigir),
        _ => {}
    }
}

/// Mensagem do WS do jogo como veio (modo Descoberta): `dir` `in`/`out`, tipo (`t`), tamanho
/// e o conteúdo sem segredos. Acima de [`BRUTO_MAX`] guarda só o começo do texto.
fn evento_bruto(msg: &Value, ts: i64) -> Option<Value> {
    let texto = msg.get("dados")?.as_str()?;
    let dir = match msg.get("dir").and_then(Value::as_str) {
        Some("out") => "out",
        _ => "in",
    };
    let tam = texto.len();
    let (tipo, dados, truncado) = match serde_json::from_str::<Value>(texto) {
        Ok(mut v) => {
            let tipo = v.get("t").or_else(|| v.get("type")).cloned();
            redigir(&mut v);
            if v.to_string().len() <= BRUTO_MAX {
                (tipo, v, false)
            } else {
                (tipo, Value::String(corte(&v.to_string())), true)
            }
        }
        // Texto que não é JSON: não dá para redigir por chave, então só o tamanho.
        Err(_) => (None, Value::Null, true),
    };
    Some(
        json!({"k": "ws.bruto", "ts": ts, "dir": dir, "tipo": tipo, "tam": tam,
        "truncado": truncado, "dados": dados}),
    )
}

/// Primeiros [`BRUTO_MAX`] bytes, sem quebrar caractere.
fn corte(t: &str) -> String {
    let mut fim = BRUTO_MAX.min(t.len());
    while !t.is_char_boundary(fim) {
        fim -= 1;
    }
    t[..fim].to_owned()
}

pub fn eventos_do_coletor(msg: &Value, ts: i64) -> Vec<Value> {
    let mut s = Vec::new();
    match msg.get("t").and_then(Value::as_str) {
        Some("welcome") if msg.get("estado").is_some() => {
            s.push(json!({"k": "welcome", "ts": ts, "payload": msg}));
        }
        Some("batalha") => {
            for ev in msg
                .get("ev")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                evento_batalha(ev, ts, &mut s);
            }
        }
        Some("hud") => {
            let hunt = copia(msg, &["hunt_slug", "huntSlug"]);
            for l in msg
                .get("linhas")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let (seq, lts, texto) = (
                    l.get(0).cloned().unwrap_or(Value::Null),
                    l.get(1).and_then(Value::as_i64).unwrap_or(ts),
                    l.get(2).and_then(Value::as_str).unwrap_or(""),
                );
                if let Some((nome, xt, xp, ouro)) = linha_kill(texto) {
                    s.push(json!({"k": "kill", "ts": lts, "seq": seq, "nome": nome,
                        "xp_treinador": xt, "xp_pokemon": xp, "ouro": ouro, "hunt_slug": hunt}));
                } else if let Some((esp, nv, q)) = linha_captura(texto) {
                    s.push(
                        json!({"k": "capturado", "ts": lts, "seq": seq, "especie": esp,
                        "nivel": nv, "quality": q,
                        "shiny": i32::from(texto.to_lowercase().contains("shiny"))}),
                    );
                }
            }
        }
        Some("mercado") => {
            for av in msg
                .get("avisos")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let an = av.get("anuncio").cloned().unwrap_or(Value::Null);
                let item = campo(&an, &["itemId", "id"])
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                let preco = campo(&an, &["preco", "price"])
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                if item == 0 || preco == 0 {
                    continue;
                }
                s.push(json!({"k": "mercado_aviso",
                    "ts": av.get("t").and_then(Value::as_i64).unwrap_or(ts),
                    "nick_origem": copia(av, &["nick"]), "item_id": item,
                    "nome": copia(&an, &["nome", "name"]), "preco": preco,
                    "moeda": campo(&an, &["moeda"]).cloned().unwrap_or(json!("gold")),
                    "texto": copia(av, &["texto"])}));
            }
        }
        Some("bruto") => s.extend(evento_bruto(msg, ts)),
        Some("inventario") => {
            let (b, p, i) = (
                copia(msg, &["bolas"]),
                copia(msg, &["pocoes"]),
                copia(msg, &["itens"]),
            );
            if !(b.is_null() && p.is_null() && i.is_null()) {
                s.push(json!({"k": "inventario", "ts": ts, "bolas": b, "pocoes": p, "itens": i}));
            }
        }
        Some("perfil") => {
            s.push(
                json!({"k": "perfil", "ts": ts, "tr_lv": copia(msg, &["tr_lv"]),
                "lvl_time": copia(msg, &["lvl_time"])}),
            );
        }
        _ => {}
    }
    s
}

// ---------------------------------------------------------------- tracker (manual)

/// A URL é uma página do tracker que o usuário está vendo? Só `https://pokeidle.io/tracker...`
/// e nunca `/tracker/api/` (bloqueado no robots.txt).
pub fn url_tracker_valida(url: &str) -> bool {
    let Some(resto) = url.strip_prefix("https://pokeidle.io") else {
        return false;
    };
    let caminho = resto.split(['?', '#']).next().unwrap_or("");
    let baixo = caminho.to_lowercase();
    (baixo == "/tracker" || baixo.starts_with("/tracker/")) && !baixo.starts_with("/tracker/api")
}

/// Evento `tracker.captura` (aditivo) do que o usuário viu ao clicar em "Guardar esta página".
/// `msg`: `{url, titulo, texto, estrutura?}`. `None` se a URL não é do tracker ou não há texto.
pub fn evento_tracker(msg: &Value, ts: i64) -> Option<Value> {
    let url = msg.get("url")?.as_str()?;
    let texto = msg.get("texto")?.as_str()?;
    if !url_tracker_valida(url) || texto.trim().is_empty() {
        return None;
    }
    let mut o = Map::new();
    o.insert("k".into(), json!("tracker.captura"));
    o.insert("ts".into(), json!(ts));
    o.insert("url".into(), json!(limitar(url, 500)));
    o.insert(
        "titulo".into(),
        json!(limitar(
            msg.get("titulo").and_then(Value::as_str).unwrap_or(""),
            300
        )),
    );
    o.insert("texto".into(), json!(limitar(texto, TRACKER_TEXTO_MAX)));
    if let Some(e) = msg
        .get("estrutura")
        .filter(|e| e.to_string().len() <= TRACKER_TEXTO_MAX)
    {
        o.insert("estrutura".into(), e.clone());
    }
    Some(Value::Object(o))
}

// ---------------------------------------------------------------- comando -> chamadas JS

fn com_t(t: &str, params: &Value) -> Value {
    let mut o = params.as_object().cloned().unwrap_or_default();
    o.insert("t".into(), json!(t));
    Value::Object(o)
}

/// Passos (objetos para `__hdxCmd`, que manda JSON no WS do jogo) de um comando do servidor.
///
/// - `motor.<t>`: o passo `{t, ...params}` (é o que o motor do Nexus enfileira);
/// - `hunt` `{slug}` -> `hunt.select`; `team.add`; `team.set` `{pokemonIds}` -> `team.active` +
///   `team.move` por slot (como o legado);
/// - `config`/`reload` (eram do motor local) e tipos desconhecidos dão `Err`.
pub fn passos(cmd: &ComandoNovo) -> Result<Vec<Value>, String> {
    let p = &cmd.params;
    match cmd.tipo.as_str() {
        t if t.starts_with("motor.") && t.len() > 6 => Ok(vec![com_t(&t[6..], p)]),
        "hunt" => {
            let slug = p
                .get("slug")
                .and_then(Value::as_str)
                .ok_or("params.slug ausente")?;
            Ok(vec![json!({"t": "hunt.select", "slug": slug})])
        }
        "team.add" => {
            let id = p.get("pokemonId").ok_or("params.pokemonId ausente")?;
            Ok(vec![json!({"t": "team.add", "pokemonId": id})])
        }
        "team.set" => {
            let ids = p
                .get("pokemonIds")
                .and_then(Value::as_array)
                .filter(|a| !a.is_empty() && a.len() <= 6)
                .ok_or("params.pokemonIds inválido")?;
            let mut v = vec![json!({"t": "team.active", "pokemonId": ids[0]})];
            for (i, id) in ids.iter().enumerate().skip(1) {
                v.push(json!({"t": "team.move", "pokemonId": id, "slot": i}));
            }
            Ok(v)
        }
        outro => Err(format!("tipo sem execução no app: {}", limitar(outro, 60))),
    }
}

/// JS avaliado na janela: o coletor roda o passo e devolve o resultado por IPC com o `id`.
pub fn script_passo(id: u64, passo: &Value) -> String {
    format!("window.__nexus&&window.__nexus.executar({id},{passo})")
}

// ---------------------------------------------------------------- avaliação com resposta

/// Correio de respostas: registra um pedido, o coletor responde pelo `id` (IPC).
#[derive(Clone, Default)]
pub struct Correio(Arc<Mutex<CorreioInterno>>);

#[derive(Default)]
struct CorreioInterno {
    proximo: u64,
    espera: HashMap<u64, oneshot::Sender<String>>,
}

impl Correio {
    /// Novo pedido: `id` para o script e o receptor da resposta.
    pub fn pedir(&self) -> (u64, oneshot::Receiver<String>) {
        let (tx, rx) = oneshot::channel();
        let mut g = self.0.lock().unwrap_or_else(|e| e.into_inner());
        g.proximo += 1;
        let id = g.proximo;
        g.espera.insert(id, tx);
        (id, rx)
    }

    /// Resposta do coletor; `false` se o pedido não existe mais (expirou ou repetido).
    pub fn responder(&self, id: u64, resposta: String) -> bool {
        let tx = self
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .espera
            .remove(&id);
        tx.is_some_and(|t| t.send(resposta).is_ok())
    }

    /// Descarta o pedido (timeout).
    pub fn cancelar(&self, id: u64) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .espera
            .remove(&id);
    }
}

/// Quem roda um passo na janela de uma conta e devolve o que o coletor respondeu (a casca usa
/// [`Correio::pedir`] + [`script_passo`] + `eval` + espera com prazo).
pub trait Avaliador: Send + Sync + 'static {
    /// `Err` se a janela da conta não existe ou não respondeu.
    fn avaliar<'a>(
        &'a self,
        conta: &'a str,
        passo: Value,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'a>>;
}

/// [`Executor`] real: cada passo vira um script na janela da conta (via [`Avaliador`]).
pub struct ExecutorJanelas<A> {
    av: A,
}

impl<A: Avaliador> ExecutorJanelas<A> {
    pub fn novo(av: A) -> Self {
        Self { av }
    }
}

impl<A: Avaliador> Executor for ExecutorJanelas<A> {
    fn executar<'a>(
        &'a self,
        cmd: &'a ComandoNovo,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'a>> {
        Box::pin(async move {
            let lista = passos(cmd)?;
            let mut ultimo = String::from("ok");
            for (i, passo) in lista.iter().enumerate() {
                if i > 0 {
                    tokio::time::sleep(PAUSA_PASSOS).await;
                }
                let r = self
                    .av
                    .avaliar(&cmd.conta, passo.clone())
                    .await
                    .map_err(|e| limitar(&e, RESULTADO_MAX))?;
                if r != "ok" {
                    return Err(limitar(&r, RESULTADO_MAX));
                }
                ultimo = r;
            }
            Ok(limitar(&ultimo, RESULTADO_MAX))
        })
    }
}

#[cfg(test)]
mod testes_bruto {
    use super::*;

    #[test]
    fn bruto_remove_segredos_e_guarda_tipo() {
        let m = json!({"t": "bruto", "dir": "in",
            "dados": r#"{"t":"pvp.fim","sessionToken":"x","jogador":{"authKey":"y","nick":"a"}}"#});
        let e = eventos_do_coletor(&m, 5);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0]["k"], "ws.bruto");
        assert_eq!(e[0]["tipo"], "pvp.fim");
        assert_eq!(e[0]["dados"]["sessionToken"], "<removido>");
        assert_eq!(e[0]["dados"]["jogador"]["authKey"], "<removido>");
        assert_eq!(e[0]["dados"]["jogador"]["nick"], "a");
    }

    #[test]
    fn bruto_grande_e_cortado_e_texto_nao_json_vai_sem_conteudo() {
        let grande = format!(r#"{{"t":"x","v":"{}"}}"#, "é".repeat(BRUTO_MAX));
        let e = eventos_do_coletor(&json!({"t": "bruto", "dados": grande}), 1);
        assert_eq!(e[0]["truncado"], true);
        assert!(e[0]["dados"].as_str().is_some_and(|d| d.len() <= BRUTO_MAX));
        let e = eventos_do_coletor(&json!({"t": "bruto", "dados": "token=abc"}), 1);
        assert_eq!(e[0]["dados"], Value::Null);
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use std::sync::Mutex as M;

    fn cmd(tipo: &str, params: Value) -> ComandoNovo {
        ComandoNovo {
            id: 1,
            conta: "ash".into(),
            tipo: tipo.into(),
            params,
            prioridade: 0,
            expira_em: None,
        }
    }

    #[test]
    fn rotulo_ida_e_volta() {
        for n in ["Ash", "a b", "ção_x", "x-1"] {
            let r = rotulo_da_conta(n);
            assert!(r
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
            assert_eq!(conta_do_rotulo(&r), Some(n.trim().to_lowercase()));
        }
        assert_eq!(conta_do_rotulo("tracker"), None);
        assert_eq!(conta_do_rotulo("jogo-_zz"), None);
    }

    #[test]
    fn hud_vira_kill_e_captura() {
        let m = json!({"t":"hud","hunt_slug":"x","linhas":[
            [7, 100, "Rattata derrotado! +1.234 xp treinador, +56 xp pokemon, +10 ouro (bônus)"],
            [8, 101, "Capturou Pidgey Nv.12! Qualidade: 45,5% shiny"],
            [9, 102, "ruído"]]});
        let e = eventos_do_coletor(&m, 5);
        assert_eq!(e.len(), 2);
        assert_eq!(e[0]["k"], "kill");
        assert_eq!(e[0]["xp_treinador"], 1234.0);
        assert_eq!(e[0]["seq"], 7);
        assert_eq!(e[0]["hunt_slug"], "x");
        assert_eq!(e[1]["k"], "capturado");
        assert_eq!(e[1]["quality"], 45.5);
        assert_eq!(e[1]["shiny"], 1);
    }

    #[test]
    fn batalha_gera_kill_drop_captura_bola() {
        let m = json!({"t":"batalha","ev":[
            {"k":"morte","quem":"selvagem","nome":"A","drops":[{"id":3,"nome":"pena","qt":2}]},
            {"k":"morte","quem":"aliado","nome":"B"},
            {"k":"capturado","pokemon":{"id":9,"nome":"C","ivs":{"a":1,"b":2}},"bola":"great"},
            {"k":"bola","ballId":4,"sucesso":1}]});
        let e = eventos_do_coletor(&m, 1);
        let ks: Vec<&str> = e.iter().filter_map(|v| v["k"].as_str()).collect();
        assert_eq!(ks, ["kill", "drop", "capturado", "bola"]);
        assert_eq!(e[1]["quantidade"], 2);
        assert_eq!(e[2]["iv_total"], 3);
    }

    #[test]
    fn mercado_exige_item_e_preco() {
        let m = json!({"t":"mercado","avisos":[
            {"t":9,"nick":"x","anuncio":{"itemId":5,"preco":100,"nome":"n"}},
            {"anuncio":{"itemId":6}}]});
        let e = eventos_do_coletor(&m, 1);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0]["moeda"], "gold");
        assert!(eventos_do_coletor(&json!({"t":"?"}), 1).is_empty());
        assert!(eventos_do_coletor(&json!({"t":"inventario"}), 1).is_empty());
    }

    #[test]
    fn tracker_so_pagina_visivel_do_tracker() {
        assert!(url_tracker_valida("https://pokeidle.io/tracker"));
        assert!(url_tracker_valida(
            "https://pokeidle.io/tracker/pokemon/1?x=2"
        ));
        assert!(!url_tracker_valida("https://pokeidle.io/tracker/api/x"));
        assert!(!url_tracker_valida("https://pokeidle.io/tracker/API/x"));
        assert!(!url_tracker_valida("https://pokeidle.io/trackers"));
        assert!(!url_tracker_valida("http://pokeidle.io/tracker"));
        assert!(!url_tracker_valida("https://pokeidle.io.evil.com/tracker"));
        let ok = json!({"url":"https://pokeidle.io/tracker","titulo":"t","texto":"abc"});
        assert_eq!(
            evento_tracker(&ok, 3).map(|v| v["k"].clone()),
            Some(json!("tracker.captura"))
        );
        assert!(evento_tracker(
            &json!({"url":"https://pokeidle.io/tracker/api/a","texto":"x"}),
            1
        )
        .is_none());
        assert!(evento_tracker(
            &json!({"url":"https://pokeidle.io/tracker","texto":"  "}),
            1
        )
        .is_none());
    }

    #[test]
    fn mapeia_comandos() {
        let p = passos(&cmd(
            "motor.shop.buy",
            json!({"kind":"ball","id":2,"qty":5}),
        ))
        .unwrap_or_default();
        assert_eq!(
            p,
            vec![json!({"t":"shop.buy","kind":"ball","id":2,"qty":5})]
        );
        assert_eq!(
            passos(&cmd("hunt", json!({"slug":"x"}))),
            Ok(vec![json!({"t":"hunt.select","slug":"x"})])
        );
        let t = passos(&cmd("team.set", json!({"pokemonIds":[1,2,3]}))).unwrap_or_default();
        assert_eq!(t.len(), 3);
        assert_eq!(t[2], json!({"t":"team.move","pokemonId":3,"slot":2}));
        assert!(passos(&cmd("team.set", json!({"pokemonIds":[]}))).is_err());
        assert!(passos(&cmd("hunt", json!({}))).is_err());
        assert!(passos(&cmd("config", json!({}))).is_err());
        assert!(passos(&cmd("motor.", json!({}))).is_err());
        assert!(script_passo(3, &json!({"t":"a"})).contains("executar(3,{\"t\":\"a\"})"));
    }

    #[tokio::test]
    async fn correio_entrega_uma_vez() {
        let c = Correio::default();
        let (id, rx) = c.pedir();
        assert!(c.responder(id, "ok".into()));
        assert!(!c.responder(id, "ok".into()));
        assert_eq!(rx.await.ok(), Some("ok".into()));
        let (id2, _rx) = c.pedir();
        c.cancelar(id2);
        assert!(!c.responder(id2, "x".into()));
    }

    struct Falso(M<Vec<(String, String)>>, &'static str);
    impl Avaliador for Falso {
        fn avaliar<'a>(
            &'a self,
            conta: &'a str,
            passo: Value,
        ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'a>> {
            Box::pin(async move {
                if let Ok(mut g) = self.0.lock() {
                    g.push((conta.to_string(), passo.to_string()));
                }
                Ok(self.1.to_string())
            })
        }
    }

    #[tokio::test(start_paused = true)]
    async fn executor_roda_passos_na_conta_e_limita_erro() {
        let ex = ExecutorJanelas::novo(Falso(M::default(), "ok"));
        let r = ex
            .executar(&cmd("team.set", json!({"pokemonIds":[1,2]})))
            .await;
        assert_eq!(r, Ok("ok".into()));
        assert_eq!(ex.av.0.lock().map(|g| g.len()).unwrap_or(0), 2);
        let longo: &'static str = Box::leak("x".repeat(500).into_boxed_str());
        let ex = ExecutorJanelas::novo(Falso(M::default(), longo));
        let r = ex.executar(&cmd("hunt", json!({"slug":"a"}))).await;
        assert_eq!(r.err().map(|e| e.chars().count()), Some(RESULTADO_MAX));
    }
}
