//! Regras do motor: guardar ou vender capturas (`planoVender`, `executarVender_`,
//! `motorVender`) e refino com pedras (`motorPedras`). O plano das pedras fica em
//! `leitura::plano_pedras`.
//!
//! Dependências da resposta do jogo (em sombra assume-se `"ok"`, sem enviar nada):
//! - cada `comando(...)` que o legado espera com `await` vira um passo [`Decisao::Comando`]
//!   cujo [`Registro`] traz o texto do sucesso (`texto_ok`, vazio quando o legado só registra
//!   na falha) e o da falha (`{r}` é a resposta do jogo);
//! - `confirmar()` do `executarVender_` relê o plano até o jogo mostrar o estado novo (até 12
//!   s). Em sombra o estado não muda, então se assume que a releitura devolve o plano já com
//!   tudo movido: os de `guardar` estão na Coleção, os devolvidos ao depot entram na venda;
//! - `loc(novoId) === 'colecao'` (troca só com o novo já na Coleção) vale quando o novo está
//!   em `guardar`, que o laço acabou de mover;
//! - as pausas (`esperar(500)`) e o teto de tempo não aparecem: quem executa os passos decide.

use std::collections::{HashMap, HashSet};

use serde_json::{json, Map, Value};

use super::{Decisao, EstadoLaco, Registro};
use crate::js;
use crate::motor::leitura::{entradas_js, objeto_js, plano_pedras};
use crate::motor::veredito::{
    decidir, iv_arr, ordenar_como_js, pot_of, q_of, sha1_hex, substituicao, Acao, Especies, Local,
};
use crate::motor::{caminho, config, existe, Relogio};

/// `VENDER_VERSAO`: sobe quando a regra mudar, e obriga a confirmar de novo.
const VENDER_VERSAO: u32 = 2;

/// `Array.prototype.join` de um valor: `null` e `undefined` viram texto vazio.
fn junta(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        outro => js::texto(outro),
    }
}

/// `assinaturaVender(C)`.
pub fn assinatura_vender(c: &Value) -> String {
    let bool01 = |k: &str| u8::from(js::verdadeiro(c.get(k))).to_string();
    [
        VENDER_VERSAO.to_string(),
        junta(c.get("minIvTotal")),
        junta(c.get("minQualidade")),
        junta(c.get("minPotencia")),
        bool01("sempreGuardarShiny"),
        bool01("sempreGuardarDesejo"),
        bool01("substituirMelhor"),
        junta(c.get("maxPctDepot")),
        junta(c.get("maxPorRodada")),
    ]
    .join("|")
}

/// `a === b` do JS entre valores de um arquivo JSON (`undefined` é `None`).
fn igual_estrito(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(Value::Number(x)), Some(Value::Number(y))) => x.as_f64() == y.as_f64(),
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// Chave de um número num `Set` do JS (`NaN` é igual a `NaN`, `-0` a `0`).
fn chave_num(x: f64) -> u64 {
    if x.is_nan() {
        u64::MAX
    } else if x == 0.0 {
        0
    } else {
        x.to_bits()
    }
}

/// `modeloLocal(conta, pokes)`: onde cada pokémon está, pelo que o jogo manda (equipe = tem
/// `slot`; Coleção = o id está em `pokemonTravado`; o resto é depot).
struct ModeloLocal {
    motivo: Option<String>,
    travados: Option<HashSet<u64>>,
    ativo: Option<Value>,
    diag: Value,
}

impl ModeloLocal {
    fn de(conta: &super::ContaViva, pokes: &[Value]) -> Self {
        let pl = caminho(&conta.ultimo, &["io", "player"])
            .filter(|v| js::verdadeiro(Some(v)))
            .cloned()
            .unwrap_or_else(|| json!({}));
        let travados: Option<HashSet<u64>> = match pl.get("travados") {
            Some(Value::Array(a)) => Some(a.iter().map(|x| chave_num(js::mais(Some(x)))).collect()),
            _ => None,
        };
        let tem_slot = pokes.iter().any(|p| p.get("slot").is_some());
        let motivo = if travados.is_none() {
            Some("o jogo ainda não me mandou a lista da Coleção. Recarregue o painel do jogo uma vez (F5).".to_owned())
        } else if !tem_slot {
            Some("o jogo não me disse quem está na equipe. Recarregue o painel do jogo uma vez (F5).".to_owned())
        } else {
            None
        };
        let ativo = pl.get("activeId").cloned();
        let diag = json!({
            "colecao": travados.as_ref().map(HashSet::len),
            "ativo": ativo.clone().filter(|v| !v.is_null()),
            "camposDoPokemon": pl.get("chavesPoke").filter(|v| js::verdadeiro(Some(v))),
        });
        Self {
            motivo,
            travados,
            ativo,
            diag,
        }
    }

    fn local_de(&self, p: &Value) -> Local {
        if existe(p.get("slot")) || igual_estrito(p.get("id"), self.ativo.as_ref()) {
            Local::Equipe
        } else if self
            .travados
            .as_ref()
            .is_some_and(|t| t.contains(&chave_num(js::mais(p.get("id")))))
        {
            Local::Colecao
        } else {
            Local::Depot
        }
    }
}

/// O `item(p, acao, motivo, ivT)` do `planoVender`.
fn item(
    p: &Value,
    esp: &Especies,
    acao: &str,
    motivo: &str,
    iv_t: Option<f64>,
) -> Map<String, Value> {
    let nome = esp.por_id(p.get("speciesId")).map_or_else(
        || format!("#{}", js::texto(p.get("speciesId"))),
        |s| s.nome.clone(),
    );
    let mut o = Map::new();
    if let Some(id) = p.get("id") {
        o.insert("id".into(), id.clone());
    }
    o.insert("nome".into(), json!(nome));
    o.insert("lvl".into(), js::json_num(js::mais(p.get("level"))));
    o.insert("q".into(), q_of(p).map_or(Value::Null, js::json_num));
    o.insert("ivT".into(), iv_t.map_or(Value::Null, js::json_num));
    o.insert("pot".into(), js::json_num(pot_of(p)));
    o.insert("shiny".into(), json!(js::verdadeiro(p.get("shiny"))));
    o.insert("acao".into(), json!(acao));
    o.insert("motivo".into(), json!(motivo));
    o
}

/// `planoVender(conta)`: o que seria guardado, vendido e substituído, sem enviar nada ao
/// jogo. `desejos` é `listaDesejos(conta)` (`None` quando falhou ou ainda não carregou);
/// `substituindo` é o `conta.substituindo` do legado (só existe durante a execução).
pub fn plano_vender(
    d: &Value,
    r: &Relogio,
    conta: &super::ContaViva,
    esp: &Especies,
    desejos: Option<&[i64]>,
    substituindo: Option<&HashSet<i64>>,
) -> Value {
    let efetiva = config::efetiva(d, &conta.nick);
    let vazio = json!({});
    let c = efetiva.get("venderNpc").unwrap_or(&vazio);
    let pokes_v = caminho(&conta.ultimo, &["hdx", "pokemons"]).filter(|v| js::verdadeiro(Some(v)));
    let pokes: &[Value] = match pokes_v {
        Some(Value::Array(a)) => a,
        _ => &[],
    };
    let assinatura = assinatura_vender(c);
    let mut out = Map::new();
    out.insert("nick".into(), json!(conta.nick));
    out.insert("quando".into(), json!(r.agora_ms));
    out.insert("assinatura".into(), json!(assinatura));
    out.insert("total".into(), json!(pokes.len()));
    for k in ["guardar", "vender", "substituir"] {
        out.insert(k.into(), json!([]));
    }
    out.insert("jaColecao".into(), json!(0));
    out.insert("equipe".into(), json!(0));
    out.insert("bloqueio".into(), Value::Null);
    out.insert("avisos".into(), json!([]));
    out.insert("hash".into(), json!(""));
    out.insert("limite".into(), json!(false));
    if pokes.is_empty() {
        // conta nova tem 1 pokemon (o inicial): isso já é uma lista válida
        out.insert("bloqueio".into(), json!("Ainda não recebi a lista de pokemons do jogo. O app recarrega o painel sozinho; se continuar assim, abra a seção Jogo."));
        return Value::Object(out);
    }
    let modelo = ModeloLocal::de(conta, pokes);
    out.insert("diag".into(), modelo.diag.clone());
    if let Some(m) = &modelo.motivo {
        out.insert(
            "bloqueio".into(),
            json!(format!("Não vou mexer em nada porque {m}")),
        );
        return Value::Object(out);
    }
    let mut avisos: Vec<Value> = Vec::new();
    let mut desejadas: Option<HashSet<i64>> = None;
    if js::verdadeiro(c.get("sempreGuardarDesejo")) {
        desejadas = desejos.map(|l| l.iter().copied().collect());
        if desejadas.as_ref().is_none_or(HashSet::is_empty) {
            desejadas = None;
            avisos.push(json!("A lista de desejos dos bosses ainda não carregou: por enquanto não há proteção extra para espécie desejada (o melhor de cada espécie continua indo para a Coleção)."));
        }
    }
    // `grupos`: um `Map` por `+speciesId`, na ordem de inserção
    let mut grupos: Vec<Vec<&Value>> = Vec::new();
    let mut indice: HashMap<u64, usize> = HashMap::new();
    for p in pokes {
        let k = chave_num(js::mais(p.get("speciesId")));
        let i = *indice.entry(k).or_insert_with(|| {
            grupos.push(Vec::new());
            grupos.len() - 1
        });
        grupos[i].push(p);
    }
    let (mut guardar, mut vender, mut substituir) = (Vec::new(), Vec::new(), Vec::new());
    let (mut ja_colecao, mut equipe) = (0usize, 0usize);
    for p in pokes {
        match modelo.local_de(p) {
            Local::Equipe => equipe += 1,
            Local::Colecao => ja_colecao += 1,
            Local::Depot => {
                // quem acabou de ser devolvido ao depot numa troca vai para a venda, mesmo
                // passando nos mínimos
                let trocado = substituindo.is_some_and(|s| {
                    p.get("id")
                        .and_then(Value::as_f64)
                        .is_some_and(|f| f.fract() == 0.0 && s.contains(&(f as i64)))
                });
                let (acao, motivo, iv_t) = if trocado {
                    (
                        Acao::Vender,
                        "trocado por um melhor da mesma espécie".to_owned(),
                        None,
                    )
                } else {
                    let k = chave_num(js::mais(p.get("speciesId")));
                    let grupo = indice
                        .get(&k)
                        .and_then(|i| grupos.get(*i))
                        .map_or(&[][..], Vec::as_slice);
                    let v = decidir(p, grupo, c, desejadas.as_ref(), esp);
                    (v.acao, v.motivo, v.iv_t)
                };
                let nome_acao = if acao == Acao::Vender {
                    "vender"
                } else {
                    "guardar"
                };
                let it = Value::Object(item(p, esp, nome_acao, &motivo, iv_t));
                if acao == Acao::Vender {
                    vender.push(it)
                } else {
                    guardar.push(it)
                }
            }
        }
    }
    // um de cada espécie: o melhor entra na Coleção e o antigo sai e vai para a venda
    if js::verdadeiro(c.get("substituirMelhor")) {
        let local = |p: &Value| modelo.local_de(p);
        for g in &grupos {
            let Some(t) = substituicao(g, &local, esp) else {
                continue;
            };
            let (velho, novo) = (g[t.velho], g[t.novo]);
            let iv_t = iv_arr(velho.get("ivs")).map_or(0.0, |a| a.iter().fold(0.0, |s, n| s + n));
            let mut it = item(velho, esp, "substituir", "", Some(iv_t));
            if let Some(id) = novo.get("id") {
                it.insert("novoId".into(), id.clone());
            }
            it.insert("pct".into(), js::json_num(t.pct));
            // `+(quality ?? 0)`: sem o `qualidade` do `qOf`
            let q = js::mais(Some(
                novo.get("quality")
                    .filter(|v| !v.is_null())
                    .unwrap_or(&json!(0)),
            ));
            it.insert(
                "motivo".into(),
                json!(format!(
                    "sai da Coleção porque o Nv {} (qualidade {}) é {}% mais forte e entra no lugar dele",
                    js::numero(js::mais(novo.get("level"))),
                    crate::motor::veredito::local_br(q, 3),
                    crate::motor::veredito::local_br(t.pct, 3),
                )),
            );
            substituir.push(Value::Object(it));
        }
    }
    let no_depot = (guardar.len() + vender.len()) as f64;
    let a_vender = (vender.len() + substituir.len()) as f64;
    let limite = a_vender > js::mais(c.get("maxPorRodada"))
        || (a_vender >= 10.0
            && a_vender / (no_depot + substituir.len() as f64) * 100.0
                > js::mais(c.get("maxPctDepot")));
    let mut ids: Vec<Option<&Value>> = vender
        .iter()
        .chain(substituir.iter())
        .map(|x| x.get("id"))
        .collect();
    ordenar_como_js(&mut ids, |a, b| js::mais(*a) - js::mais(*b));
    let ids = ids.into_iter().map(junta).collect::<Vec<_>>().join(",");
    let hash: String = sha1_hex(&format!("{assinatura}|{ids}"))
        .chars()
        .take(16)
        .collect();
    out.insert("guardar".into(), Value::Array(guardar));
    out.insert("vender".into(), Value::Array(vender));
    out.insert("substituir".into(), Value::Array(substituir));
    out.insert("jaColecao".into(), json!(ja_colecao));
    out.insert("equipe".into(), json!(equipe));
    out.insert("avisos".into(), Value::Array(avisos));
    out.insert("hash".into(), json!(hash));
    out.insert("limite".into(), json!(limite));
    Value::Object(out)
}

/// O que `executarVender_` devolveu (menos o `texto` de erro).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResumoVender {
    pub ok: bool,
    pub movidos: usize,
    pub trocados: usize,
    pub vendidos: usize,
    /// `parcial`: sobrou lote para o próximo ciclo.
    pub parcial: bool,
}

fn lista<'a>(plano: &'a Value, k: &str) -> &'a [Value] {
    match plano.get(k) {
        Some(Value::Array(a)) => a,
        _ => &[],
    }
}

/// `arr.slice(0, fim)`: o tamanho do pedaço (`undefined` é o fim da lista).
fn fatia_ate(len: usize, fim: Option<&Value>) -> usize {
    let Some(v) = fim else {
        return len;
    };
    let n = js::mais(Some(v));
    let n = if n.is_nan() { 0.0 } else { n.trunc() };
    if n < 0.0 {
        (len as f64 + n).max(0.0) as usize
    } else {
        n.min(len as f64) as usize
    }
}

/// `String(it.lvl)` do item do plano (o `NaN` do JS vira `null` no JSON do plano).
fn texto_lvl(it: &Value) -> String {
    match it.get("lvl") {
        None | Some(Value::Null) => "NaN".to_owned(),
        outro => js::texto(outro),
    }
}

fn mover(nick: &str, it: &Value, para: &str) -> Decisao {
    let mut cmd = Map::new();
    cmd.insert("t".into(), json!("colecao.mover"));
    if let Some(id) = it.get("id") {
        cmd.insert("pokemonId".into(), id.clone());
    }
    cmd.insert("para".into(), json!(para));
    let onde = if para == "colecao" {
        "para a Coleção"
    } else {
        "de volta ao depot"
    };
    Decisao::Comando {
        nick: nick.to_owned(),
        comando: Value::Object(cmd),
        registro: Some(Registro {
            tipo: "depot".into(),
            // o legado só registra quando o jogo recusa
            texto_ok: String::new(),
            texto_falha: format!(
                "Parei: o jogo não aceitou mover {} Nv {} ({onde}) ({{r}}). Nada foi vendido.",
                it.get("nome").and_then(Value::as_str).unwrap_or(""),
                texto_lvl(it),
            ),
        }),
    }
}

fn registro_depot(nick: &str, texto: String, ok: bool) -> Decisao {
    Decisao::Registro {
        nick: nick.to_owned(),
        tipo: "depot".into(),
        texto,
        ok,
    }
}

/// `executarVender_(conta, plano, {vender})` como lista de passos. `c` é o `venderNpc`.
pub fn executar_vender(
    nick: &str,
    c: &Value,
    plano: &Value,
    vender: bool,
) -> (Vec<Decisao>, ResumoVender) {
    let mut passos = Vec::new();
    let mut res = ResumoVender::default();
    if js::verdadeiro(plano.get("bloqueio")) {
        return (passos, res);
    }
    let guardar = lista(plano, "guardar");
    let a_vender = lista(plano, "vender");
    let substituir = lista(plano, "substituir");
    let n_lote = fatia_ate(guardar.len(), c.get("maxMoverRodada"));
    for it in &guardar[..n_lote] {
        passos.push(mover(nick, it, "colecao"));
        res.movidos += 1;
    }
    let movidos = res.movidos;
    if guardar.len() > n_lote {
        passos.push(registro_depot(
            nick,
            format!(
                "Guardei {movidos} na Coleção; faltam {}. Continuo no próximo ciclo. Nada foi vendido ainda.",
                guardar.len() - n_lote
            ),
            true,
        ));
        res.ok = true;
        res.parcial = true;
        return (passos, res);
    }
    let tem_troca = vender && !substituir.is_empty();
    if !vender || (a_vender.is_empty() && !tem_troca) {
        passos.push(registro_depot(
            nick,
            format!(
                "{movidos} guardados na Coleção. {}",
                if vender {
                    "Nada para vender."
                } else {
                    "Venda desligada: nada foi vendido."
                }
            ),
            true,
        ));
        res.ok = true;
        return (passos, res);
    }
    // `confirmar()`: em sombra, o plano relido já mostra tudo movido (veja o topo do arquivo)
    let mut ids_venda: Vec<Option<&Value>> = a_vender.iter().map(|x| x.get("id")).collect();
    if tem_troca {
        for t in substituir {
            let novo_na_colecao = guardar
                .iter()
                .any(|g| igual_estrito(g.get("id"), t.get("novoId")));
            if !novo_na_colecao {
                continue; // só troca com o novo confirmado na Coleção
            }
            passos.push(mover(nick, t, "depot"));
            res.trocados += 1;
            ids_venda.push(t.get("id"));
        }
    }
    if ids_venda.is_empty() {
        passos.push(registro_depot(
            nick,
            format!("{movidos} guardados na Coleção. Nada para vender."),
            true,
        ));
        res.ok = true;
        return (passos, res);
    }
    // `Math.max(...ids.map(x => +x.id || 0))`
    let ate_id = ids_venda.iter().fold(f64::NEG_INFINITY, |m, id| {
        let v = js::mais(*id);
        let v = if v == 0.0 || v.is_nan() { 0.0 } else { v };
        m.max(v)
    });
    let trocados = if res.trocados > 0 {
        format!(", {} trocados por um melhor", res.trocados)
    } else {
        String::new()
    };
    let texto = format!(
        "{movidos} guardados na Coleção{trocados}; pedi ao NPC para vender {} (até o id {}).",
        ids_venda.len(),
        js::numero(ate_id)
    );
    passos.push(Decisao::Comando {
        nick: nick.to_owned(),
        comando: json!({"t": "shop.sellAllPokemons", "ateId": js::json_num(ate_id)}),
        registro: Some(Registro {
            tipo: "depot".into(),
            texto_ok: texto.clone(),
            texto_falha: texto,
        }),
    });
    res.ok = true;
    res.vendidos = ids_venda.len();
    (passos, res)
}

/// Marca numérica estável de um texto (`conta.venderLog` guarda um texto no legado e o
/// `EstadoLaco` só guarda números): FNV-1a de 64 bits reduzido a 52 bits, sempre diferente de 0.
fn marca_de_texto(t: &str) -> f64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in t.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    ((h & 0x000f_ffff_ffff_ffff) | 1) as f64
}

/// `unico(chave, texto, ok)`: registra só quando a chave mudou desde o último aviso.
fn unico(
    laco: &mut EstadoLaco,
    out: &mut Vec<Decisao>,
    nick: &str,
    chave: &str,
    texto: String,
    ok: bool,
) {
    let h = marca_de_texto(chave);
    if laco.marca(nick, "venderLog") != h {
        laco.marcar(nick, "venderLog", h);
        out.push(registro_depot(nick, texto, ok));
    }
}

/// `motorVender(conta)`. `desejos` é `listaDesejos(conta)`. O que o legado grava no objeto da
/// conta além das marcas (`venderStatus`, `venderUltimo`, `venderBusy`) não vira decisão:
/// `venderStatus` é o resumo do plano e `venderUltimo` o resumo da execução.
pub fn motor_vender(
    d: &Value,
    laco: &mut EstadoLaco,
    r: &Relogio,
    conta: &super::ContaViva,
    esp: &Especies,
    desejos: Option<&[i64]>,
) -> Vec<Decisao> {
    let efetiva = config::efetiva(d, &conta.nick);
    let vazio = json!({});
    let c = efetiva.get("venderNpc").unwrap_or(&vazio);
    if !js::verdadeiro(c.get("ativo")) || conta.vender_ocupado {
        return Vec::new();
    }
    let nick = conta.nick.as_str();
    let intervalo_ms = js::mais(c.get("intervaloMin")) * 60e3;
    let em = laco.marca(nick, "venderEm");
    if em != 0.0 && (r.agora_ms as f64) - em < intervalo_ms {
        return Vec::new();
    }
    laco.marcar(nick, "venderEm", r.agora_ms as f64);
    let plano = plano_vender(d, r, conta, esp, desejos, None);
    let mut out = Vec::new();
    let n = |k: &str| plano.get(k).and_then(Value::as_array).map_or(0, Vec::len);
    let (n_guardar, n_vender, n_subst) = (n("guardar"), n("vender"), n("substituir"));
    let assinatura = plano
        .get("assinatura")
        .and_then(Value::as_str)
        .unwrap_or("");
    if let Some(b) = plano
        .get("bloqueio")
        .and_then(Value::as_str)
        .filter(|b| !b.is_empty())
    {
        unico(laco, &mut out, nick, &format!("b|{b}"), b.to_owned(), false);
        return out;
    }
    if n_guardar == 0 && n_vender == 0 && n_subst == 0 {
        return out;
    }
    if c.get("confirmado") != Some(&Value::String(assinatura.to_owned())) {
        let chave = format!("s|{assinatura}|{n_guardar}|{n_vender}");
        let texto = format!("Simulação: {n_guardar} para guardar, {n_vender} para vender e {n_subst} trocas por um melhor. Nada foi enviado ao jogo. Confira a lista em Capturas > Guardar ou vender e confirme para liberar.");
        unico(laco, &mut out, nick, &chave, texto, true);
        return out;
    }
    let limite = js::verdadeiro(plano.get("limite"));
    let vender = js::verdadeiro(c.get("vender")) && !limite;
    if js::verdadeiro(c.get("vender")) && limite {
        let hash = plano.get("hash").and_then(Value::as_str).unwrap_or("");
        let texto = format!(
            "Parei antes de vender: {} pokemons passam do limite de segurança ({}% ou {}). Só guardei. Veja a lista em Capturas e aplique na mão.",
            n_vender + n_subst,
            js::texto(c.get("maxPctDepot")),
            js::texto(c.get("maxPorRodada")),
        );
        unico(laco, &mut out, nick, &format!("l|{hash}"), texto, false);
    }
    let (passos, resumo) = executar_vender(nick, c, &plano, vender);
    out.extend(passos);
    if resumo.parcial {
        // o resto do lote em 1 minuto
        laco.marcar(nick, "venderEm", r.agora_ms as f64 - (intervalo_ms - 60e3));
    }
    out
}

/// `motorPedras(conta)`: refina com as pedras que o plano reservou para a equipe da conta.
/// `contas` são todas as contas conectadas (o plano divide as pedras entre elas).
/// Em sombra os `uso` de uma linha não descontam do estoque lido para as seguintes, e se
/// assume que o jogo aceita cada refino (o legado pararia no primeiro que falhar).
pub fn motor_pedras(
    d: &Value,
    laco: &mut EstadoLaco,
    r: &Relogio,
    conta: &super::ContaViva,
    contas: &[super::ContaViva],
    esp: &Especies,
) -> Vec<Decisao> {
    let efetiva = config::efetiva(d, &conta.nick);
    let vazio = json!({});
    let c = efetiva.get("pedras").unwrap_or(&vazio);
    if !js::verdadeiro(c.get("ativo")) {
        return Vec::new();
    }
    let nick = conta.nick.as_str();
    let em = laco.marca(nick, "pedrasEm");
    if em != 0.0 && (r.agora_ms as f64) - em < js::mais(c.get("intervaloMin")) * 60e3 {
        return Vec::new();
    }
    laco.marcar(nick, "pedrasEm", r.agora_ms as f64);
    let plano = plano_pedras(contas, esp);
    let reserva = {
        let x = js::mais(c.get("reserva"));
        if x == 0.0 || x.is_nan() {
            0.0
        } else {
            x
        }
    };
    let itens = caminho(&conta.ultimo, &["hdx", "eu", "items"]).filter(|v| js::verdadeiro(Some(v)));
    let mut out = Vec::new();
    let equipe = plano
        .get("equipe")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    for l in equipe.iter().filter(|x| {
        x.get("nick").and_then(Value::as_str) == Some(nick)
            && x.get("situacao").and_then(Value::as_str) == Some("pode")
    }) {
        let mut uso: Vec<(String, Value)> = Vec::new();
        let mut ok = true;
        if let Some(Value::Object(u)) = l.get("uso") {
            for (i, q) in entradas_js(u) {
                let resta = js::mais(itens.and_then(|m| m.get(i.as_str()))) - reserva;
                if resta < js::mais(Some(q)) {
                    ok = false;
                }
                uso.push((i.clone(), q.clone()));
            }
        }
        if !ok {
            continue;
        }
        let stat = l.get("stat").and_then(Value::as_str).unwrap_or("atk");
        let mut cmd = Map::new();
        cmd.insert("t".into(), json!("pokemon.refinar"));
        if let Some(id) = l.get("id") {
            cmd.insert("pokemonId".into(), id.clone());
        }
        cmd.insert("stat".into(), json!(stat));
        cmd.insert("uso".into(), objeto_js(uso));
        let texto = format!(
            "Refino {} de {}: +{} por {} pedras.",
            if stat == "atk" { "ATK" } else { "SpATK" },
            l.get("nome").and_then(Value::as_str).unwrap_or(""),
            js::numero(js::mais(l.get("nivelRefino")) + 1.0),
            js::texto(l.get("custo")),
        );
        out.push(Decisao::Comando {
            nick: nick.to_owned(),
            comando: Value::Object(cmd),
            registro: Some(Registro {
                tipo: "refino".into(),
                texto_ok: texto.clone(),
                texto_falha: texto,
            }),
        });
    }
    out
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::combate::especie::Pokedex;
    use crate::motor::regras::ContaViva;

    const REL: Relogio = Relogio {
        agora_ms: 1_794_493_463_355,
        fuso_ms: -3 * 3_600_000,
    };

    fn dex() -> Pokedex {
        Pokedex::de_valor(json!([
            {"d": 1, "n": "Bulbasaur", "t": ["GRASS"], "s": [45, 49, 49, 65, 65, 45], "h": 20,
             "p": 3000, "c": "pokebola", "b": [0.3, 0.6, 0.9, 1.2, 2.4],
             "m": [["Razor Leaf", 55, 19.4, "F", "GRASS"]]},
            {"d": 4, "n": "Charmander", "t": ["FIRE"], "s": [39, 52, 43, 60, 50, 65], "h": 20,
             "p": 3000, "c": "pokebola", "b": [0.3, 0.6, 0.9, 1.2, 2.4],
             "m": [["Ember", 40, 19.4, "E", "FIRE"]]}
        ]))
        .unwrap()
    }

    fn poke(id: i64, esp: i64, q: f64, iv: i64, slot: Option<i64>) -> Value {
        let mut p = json!({"id": id, "speciesId": esp, "level": 100, "quality": q,
                           "ivs": [iv, iv, iv, iv, iv, iv], "potencia": 1});
        if let Some(s) = slot {
            p["slot"] = json!(s);
        }
        p
    }

    fn conta(pokes: Vec<Value>, travados: Value) -> ContaViva {
        ContaViva {
            nick: "Ana".into(),
            ultimo: json!({"hdx": {"pokemons": pokes, "eu": {"items": {}}},
                           "io": {"player": {"travados": travados, "activeId": 1}}}),
            ..Default::default()
        }
    }

    fn dados(v_npc: Value) -> Value {
        config::carregar(&json!({"config": {"Ana": {"venderNpc": v_npc}}}))
    }

    #[test]
    fn assinatura_e_bloqueios() {
        let d = dados(json!({}));
        let c = config::efetiva(&d, "Ana");
        let a = assinatura_vender(&c["venderNpc"]);
        assert_eq!(a.split('|').count(), 9);
        assert!(a.starts_with("2|"));
        let dx = dex();
        let esp = Especies::sem_precos(&dx);
        let sem = ContaViva {
            nick: "Ana".into(),
            ..Default::default()
        };
        let p = plano_vender(&d, &REL, &sem, &esp, None, None);
        assert_eq!(p["total"], 0);
        assert!(p["bloqueio"]
            .as_str()
            .unwrap()
            .starts_with("Ainda não recebi"));
        assert!(p.get("diag").is_none());
        let sem_trav = conta(vec![poke(1, 1, 1.0, 10, Some(0))], Value::Null);
        let p = plano_vender(&d, &REL, &sem_trav, &esp, None, None);
        assert!(p["bloqueio"].as_str().unwrap().contains("lista da Coleção"));
        assert_eq!(p["diag"]["colecao"], Value::Null);
    }

    #[test]
    fn plano_guarda_vende_e_troca() {
        let d = dados(json!({"substituirMelhor": true}));
        let dx = dex();
        let esp = Especies::sem_precos(&dx);
        // 1 equipe; 2 Coleção (fraco); 3 depot melhor (entra na Coleção); 4 depot fraco; 5 de outra espécie
        let c = conta(
            vec![
                poke(1, 4, 1.0, 16, Some(0)),
                poke(2, 1, 1.0, 10, None),
                poke(3, 1, 1.3, 25, None),
                poke(4, 1, 1.0, 5, None),
                poke(5, 4, 1.0, 16, None),
            ],
            json!([2]),
        );
        let p = plano_vender(&d, &REL, &c, &esp, Some(&[]), None);
        assert_eq!(p["equipe"], 1);
        assert_eq!(p["jaColecao"], 1);
        let ids = |k: &str| {
            p[k].as_array()
                .unwrap()
                .iter()
                .map(|x| x["id"].as_i64().unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids("guardar"), [3, 5]);
        assert_eq!(ids("vender"), [4]);
        assert_eq!(ids("substituir"), [2]);
        assert_eq!(p["substituir"][0]["novoId"], 3);
        assert!(p["substituir"][0]["motivo"]
            .as_str()
            .unwrap()
            .starts_with("sai da Coleção porque o Nv 100 (qualidade 1,3) é "));
        assert_eq!(p["avisos"].as_array().unwrap().len(), 1);
        let h = sha1_hex(&format!("{}|2,4", p["assinatura"].as_str().unwrap()));
        assert_eq!(p["hash"], h[..16]);
        // quem foi devolvido ao depot vai direto para a venda
        let sub: HashSet<i64> = [3].into_iter().collect();
        let p2 = plano_vender(&d, &REL, &c, &esp, None, Some(&sub));
        assert_eq!(p2["vender"].as_array().unwrap().len(), 2);
        assert_eq!(
            p2["vender"][0]["motivo"],
            "trocado por um melhor da mesma espécie"
        );
    }

    #[test]
    fn passos_da_execucao() {
        let d = dados(json!({"substituirMelhor": true, "maxMoverRodada": 30}));
        let dx = dex();
        let esp = Especies::sem_precos(&dx);
        let c = conta(
            vec![
                poke(1, 4, 1.0, 16, Some(0)),
                poke(2, 1, 1.0, 10, None),
                poke(3, 1, 1.3, 25, None),
                poke(4, 1, 1.0, 5, None),
            ],
            json!([2]),
        );
        let plano = plano_vender(&d, &REL, &c, &esp, Some(&[]), None);
        let cfg = config::efetiva(&d, "Ana");
        let (passos, res) = executar_vender("Ana", &cfg["venderNpc"], &plano, true);
        let cmds: Vec<&Value> = passos
            .iter()
            .filter_map(|p| {
                if let Decisao::Comando { comando, .. } = p {
                    Some(comando)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            cmds[0],
            &json!({"t": "colecao.mover", "pokemonId": 3, "para": "colecao"})
        );
        assert_eq!(
            cmds[1],
            &json!({"t": "colecao.mover", "pokemonId": 2, "para": "depot"})
        );
        assert_eq!(cmds[2], &json!({"t": "shop.sellAllPokemons", "ateId": 4}));
        assert_eq!(
            res,
            ResumoVender {
                ok: true,
                movidos: 1,
                trocados: 1,
                vendidos: 2,
                parcial: false
            }
        );
        let Some(Decisao::Comando {
            registro: Some(rg), ..
        }) = passos.last()
        else {
            panic!("sem comando final");
        };
        assert_eq!(rg.texto_ok, "1 guardados na Coleção, 1 trocados por um melhor; pedi ao NPC para vender 2 (até o id 4).");
        // lote parcial
        let cfg2 = json!({"maxMoverRodada": 0});
        let (p2, r2) = executar_vender("Ana", &cfg2, &plano, true);
        assert!(
            r2.parcial
                && matches!(&p2[0], Decisao::Registro { texto, .. } if texto.starts_with("Guardei 0 na Coleção; faltam 1."))
        );
        // venda desligada
        let (p3, _) = executar_vender("Ana", &json!({}), &plano, false);
        assert!(
            matches!(p3.last(), Some(Decisao::Registro { texto, .. }) if texto == "1 guardados na Coleção. Venda desligada: nada foi vendido.")
        );
    }

    #[test]
    fn motor_vender_simula_ate_confirmar() {
        let d = dados(json!({"ativo": true}));
        let dx = dex();
        let esp = Especies::sem_precos(&dx);
        let c = conta(
            vec![poke(1, 4, 1.0, 16, Some(0)), poke(5, 4, 1.0, 16, None)],
            json!([]),
        );
        let mut laco = EstadoLaco::default();
        let v = motor_vender(&d, &mut laco, &REL, &c, &esp, Some(&[]));
        assert!(
            matches!(&v[..], [Decisao::Registro { texto, ok: true, .. }] if texto.starts_with("Simulação: 1 para guardar, 0 para vender e 0 trocas"))
        );
        // dentro do intervalo, nada
        assert!(motor_vender(&d, &mut laco, &REL, &c, &esp, Some(&[])).is_empty());
        // confirmado: guarda
        let plano = plano_vender(&d, &REL, &c, &esp, Some(&[]), None);
        let d2 = dados(json!({"v": 3, "ativo": true, "confirmado": plano["assinatura"]}));
        let mut laco2 = EstadoLaco::default();
        let v = motor_vender(&d2, &mut laco2, &REL, &c, &esp, Some(&[]));
        assert!(
            matches!(&v[0], Decisao::Comando { comando, .. } if comando["t"] == "colecao.mover")
        );
    }

    #[test]
    fn fatia_como_slice() {
        assert_eq!(fatia_ate(5, None), 5);
        assert_eq!(fatia_ate(5, Some(&json!(2))), 2);
        assert_eq!(fatia_ate(5, Some(&json!(-2))), 3);
        assert_eq!(fatia_ate(5, Some(&json!(null))), 0);
        assert_eq!(fatia_ate(5, Some(&json!(99))), 5);
    }

    #[test]
    fn pedras_pode_refinar() {
        let d = dados(json!({}));
        let dx = dex();
        let esp = Especies::sem_precos(&dx);
        let mut c = conta(vec![poke(1, 1, 1.0, 16, Some(0))], json!([]));
        c.welcome = json!({"pedrasEvolucao": {"NORMAL": {"itemId": 209, "nome": "Sun Stone"},
                                              "GRASS": {"itemId": 76, "nome": "Leaf Stone"}}});
        c.ultimo["hdx"]["eu"]["items"] = json!({"209": 100, "76": 600});
        let d2 = config::carregar(&json!({"config": {"Ana": {"pedras": {"ativo": true}}}}));
        let _ = d;
        let mut laco = EstadoLaco::default();
        let contas = [c.clone()];
        let v = motor_pedras(&d2, &mut laco, &REL, &c, &contas, &esp);
        assert_eq!(v.len(), 1);
        let Decisao::Comando {
            comando, registro, ..
        } = &v[0]
        else {
            panic!("esperava comando");
        };
        assert_eq!(
            comando,
            &json!({"t": "pokemon.refinar", "pokemonId": 1, "stat": "atk", "uso": {"76": 500}})
        );
        assert_eq!(
            registro.as_ref().map(|r| r.texto_ok.as_str()),
            Some("Refino ATK de Bulbasaur: +1 por 500 pedras.")
        );
        assert!(motor_pedras(&d2, &mut laco, &REL, &c, &contas, &esp).is_empty());
    }

    /// Compara com as respostas capturadas do legado. Rode com
    /// `NEXUS_FIXTURES_MOTOR=/root/fixtures-motor-pc/<data> cargo test ... -- --ignored`.
    #[test]
    #[ignore = "precisa das fixtures do legado (NEXUS_FIXTURES_MOTOR)"]
    fn fixtures_do_legado() {
        let Ok(dir) = std::env::var("NEXUS_FIXTURES_MOTOR") else {
            return;
        };
        let le = |nome: &str| -> Value {
            let t = std::fs::read_to_string(format!("{dir}/{nome}")).unwrap();
            serde_json::from_str(&t).unwrap()
        };
        let bruta: Value = {
            let local = format!("{dir}/pokedex.json");
            let t = std::fs::read_to_string(&local)
                .or_else(|_| std::fs::read_to_string("/opt/holodex-nexus/web/pokedex.json"))
                .unwrap();
            serde_json::from_str(&t).unwrap()
        };
        let dx = Pokedex::de_valor(bruta.clone()).unwrap();
        let precos = Especies::precos_de_json(&bruta);
        let esp = Especies::com_precos(&dx, &precos);
        let d = config::carregar(&le("nexus-dados.json"));
        let janela = le("nexus-janela.json");
        let agora = janela["agora"].as_i64().unwrap();
        let contas: Vec<ContaViva> = janela["contas"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(ContaViva::de_json)
            .collect();
        let mut erros = Vec::new();
        for c in &contas {
            let previa = le(&format!("vender-previa-{}.json", c.nick));
            // `quando` do legado é o instante da prévia; o resto depende só do estado
            let r = Relogio {
                agora_ms: previa["quando"].as_i64().unwrap_or(agora),
                fuso_ms: -3 * 3_600_000,
            };
            // a lista de desejos do legado: as espécies que ele protegeu por desejo
            let desejos: Vec<i64> = ["guardar", "vender"]
                .iter()
                .flat_map(|k| previa[*k].as_array().cloned().unwrap_or_default())
                .filter(|it| {
                    it["motivo"]
                        .as_str()
                        .is_some_and(|m| m.contains("lista de desejos dos bosses"))
                })
                .filter_map(|it| {
                    let id = it["id"].clone();
                    c.ultimo["hdx"]["pokemons"]
                        .as_array()?
                        .iter()
                        .find(|p| p["id"] == id)?["speciesId"]
                        .as_i64()
                })
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            // sem aviso no legado, a lista existia; se ninguém foi salvo por ela, um id qualquer
            let desejos: Vec<i64> = if desejos.is_empty() {
                vec![-1]
            } else {
                desejos
            };
            let desejos_opt = if previa["avisos"].as_array().is_some_and(|a| !a.is_empty()) {
                None
            } else {
                Some(&desejos[..])
            };
            let plano = plano_vender(&d, &r, c, &esp, desejos_opt, None);
            for k in [
                "nick",
                "assinatura",
                "total",
                "guardar",
                "vender",
                "substituir",
                "jaColecao",
                "equipe",
                "bloqueio",
                "avisos",
                "hash",
                "limite",
                "diag",
            ] {
                if plano.get(k) != previa.get(k) {
                    erros.push(format!("{} / {k}", c.nick));
                }
            }
            eprintln!(
                "{}: guardar={} vender={} substituir={} hash={}",
                c.nick,
                plano["guardar"].as_array().map_or(0, Vec::len),
                plano["vender"].as_array().map_or(0, Vec::len),
                plano["substituir"].as_array().map_or(0, Vec::len),
                plano["hash"]
            );
        }
        let pedras_legado = std::fs::read_to_string(format!("{dir}/pedras-plano.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok());
        if let Some(p) = pedras_legado {
            let meu = plano_pedras(&contas, &esp);
            if meu != p {
                erros.push("pedras/plano".into());
            }
            eprintln!("pedras/plano comparado");
        }
        assert!(erros.is_empty(), "divergências: {erros:?}");
    }
}
