//! A2 como relay: fluxos que pedem dados ao jogo e entregam ao servidor (antes eram o motor e o
//! relay do Electron). Cada fluxo é um [`Fluxo`]; o [`Relay`] reveza os pedidos respeitando o
//! ritmo do jogo. Plano em `docs/a2-relay.md`.

pub mod contas;
pub mod cortes;
pub mod extrato;
pub mod mercado;
pub mod raid;
pub mod saida;
pub mod tempo;
pub mod treino;
pub mod vendas;

use serde_json::Value;

/// O que um fluxo tem pronto para o servidor.
#[derive(Debug, Clone, PartialEq)]
pub enum Entrega {
    /// Corpo de `POST /api/bolsa/ofertas` (`{ofertas: [...]}` no formato do relay antigo).
    Ofertas(Vec<Value>),
    /// Vendas do mercado (`POST /api/bolsa/vendas`; o servidor repassa ao Dossiê).
    Vendas(Vec<Value>),
    /// Equipe e coleção de uma conta (`POST /api/equipes/ingest`).
    Equipe(Value),
    /// Hunt atual das contas (`POST /api/hunt-sync`, `{eventos}`).
    Hunt(Vec<Value>),
    /// Inventário somado das contas (`POST /api/bolsa/meus-itens`).
    MeusItens(Value),
    /// Raros e histórico (`POST /api/raros/ingest`).
    Raros(Value),
    /// Resumo do dia de uma conta (eventos `diario` em `POST /ingest`).
    Diario { conta: String, eventos: Vec<Value> },
    /// Janela da conta para o motor do Nexus (`POST /api/motor/janela`).
    Janela { conta: String, corpo: Value },
    /// Eventos avulsos de uma conta (`POST /ingest`); um não substitui o outro na fila.
    Eventos { conta: String, eventos: Vec<Value> },
}

/// Um fluxo de dados do jogo para o servidor.
pub trait Fluxo: Send {
    fn nome(&self) -> &'static str;
    /// Mensagem do WS do jogo (o fluxo ignora o que não é dele).
    fn receber(&mut self, msg: &Value, agora: i64);
    /// Próximo pedido ao jogo (JSON para o `__hdxCmd`), se for a hora.
    fn pedir(&mut self, agora: i64) -> Option<Value>;
    /// O que já está pronto para o servidor (esvazia).
    fn entregar(&mut self, agora: i64) -> Vec<Entrega>;
    /// Espécies alvo do servidor (só o mercado usa).
    fn definir_alvos(&mut self, _alvos: &[mercado::pokemon::Alvo], _agora: i64) {}
    /// Mensagem de qualquer conta (`welcome`, `estado`, linhas do `hud`).
    fn receber_conta(&mut self, _conta: &str, _msg: &Value, _agora: i64) {}
    /// Estado para guardar em disco, se mudou.
    fn salvar(&mut self) -> Option<Value> {
        None
    }
    fn restaurar(&mut self, _v: &Value) {}
    /// Liga a leitura do ranking para o mapa e retorna a última amostra disponível.
    fn ranking_pvp(&mut self) -> Option<Value> {
        None
    }
    /// Rota de leitura no servidor (`GET`) de que o fluxo precisa agora, se precisar.
    fn consulta(&mut self, _agora: i64) -> Option<String> {
        None
    }
    /// Resposta da [`Fluxo::consulta`].
    fn resposta(&mut self, _rota: &str, _v: &Value, _agora: i64) {}
}

/// Intervalo mínimo entre dois pedidos ao jogo pela mesma conta (o jogo aceita 1/s).
pub const INTERVALO_PEDIDOS_MS: i64 = 2_000;

/// Reveza os fluxos: no máximo um pedido por [`INTERVALO_PEDIDOS_MS`], em rodízio.
pub struct Relay {
    fluxos: Vec<Box<dyn Fluxo>>,
    vez: usize,
    ultimo_pedido: i64,
}

impl Relay {
    pub fn novo(fluxos: Vec<Box<dyn Fluxo>>) -> Self {
        Self {
            fluxos,
            vez: 0,
            ultimo_pedido: i64::MIN / 2,
        }
    }

    /// Relay com os fluxos padrão. `oficial`: o app já é o relay oficial (manda também o
    /// inventário somado, os raros e as médias de preço).
    /// `dono`: a conta que faz os pedidos (o treino aprende com o time dela); `aprender`:
    /// testar formações na Área de Treinamento e aplicar a melhor (só com `oficial`).
    pub fn padrao(oficial: bool, dono: &str, aprender: bool) -> Self {
        Self::novo(vec![
            Box::new(mercado::Mercado::novo()),
            Box::new(vendas::Vendas::novo()),
            Box::new(contas::Contas::novo(oficial)),
            Box::new(raid::Raid::novo()),
            Box::new(treino::Treino::novo(dono, oficial && aprender)),
            Box::new(extrato::Extrato::novo(oficial)),
            Box::new(cortes::Cortes::novo(oficial)),
        ])
    }

    /// Resposta do jogo na conta dona.
    pub fn receber(&mut self, msg: &Value, agora: i64) {
        for f in &mut self.fluxos {
            f.receber(msg, agora);
        }
    }

    /// Mensagem de qualquer conta.
    pub fn receber_conta(&mut self, conta: &str, msg: &Value, agora: i64) {
        for f in &mut self.fluxos {
            f.receber_conta(conta, msg, agora);
        }
    }

    pub fn ranking_pvp(&mut self) -> Option<Value> {
        self.fluxos.iter_mut().find_map(|f| f.ranking_pvp())
    }

    /// Estados a guardar, por nome do fluxo.
    pub fn salvar(&mut self) -> Vec<(&'static str, Value)> {
        self.fluxos
            .iter_mut()
            .filter_map(|f| Some((f.nome(), f.salvar()?)))
            .collect()
    }

    /// Consultas ao servidor pedidas pelos fluxos: (nome do fluxo, rota).
    pub fn consultas(&mut self, agora: i64) -> Vec<(&'static str, String)> {
        self.fluxos
            .iter_mut()
            .filter_map(|f| Some((f.nome(), f.consulta(agora)?)))
            .collect()
    }

    pub fn responder(&mut self, nome: &str, rota: &str, v: &Value, agora: i64) {
        for f in self.fluxos.iter_mut().filter(|f| f.nome() == nome) {
            f.resposta(rota, v, agora);
        }
    }

    pub fn restaurar(&mut self, nome: &str, v: &Value) {
        for f in self.fluxos.iter_mut().filter(|f| f.nome() == nome) {
            f.restaurar(v);
        }
    }

    /// O próximo pedido ao jogo, se já passou o intervalo e algum fluxo quer pedir.
    pub fn pedir(&mut self, agora: i64) -> Option<Value> {
        if agora - self.ultimo_pedido < INTERVALO_PEDIDOS_MS || self.fluxos.is_empty() {
            return None;
        }
        let n = self.fluxos.len();
        for i in 0..n {
            let k = (self.vez + i) % n;
            if let Some(p) = self.fluxos[k].pedir(agora) {
                self.vez = (k + 1) % n;
                self.ultimo_pedido = agora;
                return Some(p);
            }
        }
        None
    }

    pub fn entregar(&mut self, agora: i64) -> Vec<Entrega> {
        self.fluxos
            .iter_mut()
            .flat_map(|f| f.entregar(agora))
            .collect()
    }

    pub fn definir_alvos(&mut self, alvos: &[mercado::pokemon::Alvo], agora: i64) {
        for f in &mut self.fluxos {
            f.definir_alvos(alvos, agora);
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    struct Sempre(&'static str);
    impl Fluxo for Sempre {
        fn nome(&self) -> &'static str {
            self.0
        }
        fn receber(&mut self, _: &Value, _: i64) {}
        fn pedir(&mut self, _: i64) -> Option<Value> {
            Some(json!({"t": self.0}))
        }
        fn entregar(&mut self, _: i64) -> Vec<Entrega> {
            Vec::new()
        }
    }

    #[test]
    fn reveza_e_respeita_o_intervalo() {
        let mut r = Relay::novo(vec![Box::new(Sempre("a")), Box::new(Sempre("b"))]);
        assert_eq!(r.pedir(0), Some(json!({"t": "a"})));
        assert_eq!(r.pedir(1_000), None);
        assert_eq!(r.pedir(2_000), Some(json!({"t": "b"})));
        assert_eq!(r.pedir(4_000), Some(json!({"t": "a"})));
    }
}

/// Base HTTP do servidor a partir do endereço do WebSocket (`wss://h/ws` → `https://h`).
pub fn base_http(ws: &str) -> Option<String> {
    let (esquema, resto) = if let Some(r) = ws.strip_prefix("wss://") {
        ("https://", r)
    } else {
        ("http://", ws.strip_prefix("ws://")?)
    };
    let host = resto.split('/').next().filter(|h| !h.is_empty())?;
    Some(format!("{esquema}{host}"))
}

impl Entrega {
    /// Rota e corpo do `POST` no servidor.
    pub fn requisicao(&self) -> (&'static str, Value) {
        match self {
            Entrega::Ofertas(o) => ("/api/bolsa/ofertas", serde_json::json!({ "ofertas": o })),
            Entrega::Vendas(v) => ("/api/bolsa/vendas", serde_json::json!({ "vendas": v })),
            Entrega::Equipe(b) => ("/api/equipes/ingest", b.clone()),
            Entrega::Hunt(e) => ("/api/hunt-sync", serde_json::json!({ "eventos": e })),
            Entrega::MeusItens(b) => ("/api/bolsa/meus-itens", b.clone()),
            Entrega::Raros(b) => ("/api/raros/ingest", b.clone()),
            Entrega::Janela { conta, corpo } => (
                "/api/motor/janela",
                serde_json::json!({ "conta": conta, "janela": corpo }),
            ),
            Entrega::Diario { conta, eventos } | Entrega::Eventos { conta, eventos } => (
                "/ingest",
                serde_json::json!({ "conta_id": conta, "nick": conta, "eventos": eventos }),
            ),
        }
    }

    /// Resumo para o diário do app.
    pub fn resumo(&self) -> String {
        match self {
            Entrega::Ofertas(o) => format!("Bolsa: {} ofertas do mercado enviadas", o.len()),
            Entrega::Vendas(v) => format!("Dossiê: {} vendas do mercado enviadas", v.len()),
            Entrega::Equipe(b) => format!(
                "Equipes: {} pokémon de {} enviados",
                b["pokemons"].as_array().map_or(0, Vec::len),
                b["nick"].as_str().unwrap_or("?")
            ),
            Entrega::Hunt(e) => format!("Hunt: {} conta(s) atualizada(s)", e.len()),
            Entrega::MeusItens(b) => format!(
                "Meus itens: {} itens enviados",
                b["itens"].as_object().map_or(0, serde_json::Map::len)
            ),
            Entrega::Raros(b) => format!(
                "Raros: {} itens enviados",
                b["raros"]["itens"].as_array().map_or(0, Vec::len)
            ),
            Entrega::Diario { conta, eventos } => {
                format!("Diário: {} dia(s) de {conta} enviados", eventos.len())
            }
            Entrega::Janela { conta, .. } => format!("Motor: janela de {conta} enviada"),
            Entrega::Eventos { conta, eventos } => {
                format!("Extrato: {} evento(s) de {conta} enviados", eventos.len())
            }
        }
    }
}

#[cfg(test)]
mod testes_base {
    use super::*;

    #[test]
    fn base_http_do_ws() {
        assert_eq!(base_http("wss://a.b/ws").as_deref(), Some("https://a.b"));
        assert_eq!(base_http("ws://h:81/ws").as_deref(), Some("http://h:81"));
        assert_eq!(base_http("https://x"), None);
    }
}
