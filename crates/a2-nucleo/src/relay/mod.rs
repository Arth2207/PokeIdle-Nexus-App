//! A2 como relay: fluxos que pedem dados ao jogo e entregam ao servidor (antes eram o motor e o
//! relay do Electron). Cada fluxo é um [`Fluxo`]; o [`Relay`] reveza os pedidos respeitando o
//! ritmo do jogo. Plano em `docs/a2-relay.md`.

pub mod mercado;

use serde_json::Value;

/// O que um fluxo tem pronto para o servidor.
#[derive(Debug, Clone, PartialEq)]
pub enum Entrega {
    /// Corpo de `POST /api/bolsa/ofertas` (`{ofertas: [...]}` no formato do relay antigo).
    Ofertas(Vec<Value>),
}

/// Um fluxo de dados do jogo para o servidor.
pub trait Fluxo: Send {
    fn nome(&self) -> &'static str;
    /// Mensagem do WS do jogo (o fluxo ignora o que não é dele).
    fn receber(&mut self, msg: &Value, agora: i64);
    /// Próximo pedido ao jogo (JSON para o `__hdxCmd`), se for a hora.
    fn pedir(&mut self, agora: i64) -> Option<Value>;
    /// O que já está pronto para o servidor (esvazia).
    fn entregar(&mut self) -> Vec<Entrega>;
    /// Espécies alvo do servidor (só o mercado usa).
    fn definir_alvos(&mut self, _alvos: &[mercado::pokemon::Alvo], _agora: i64) {}
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

    /// Relay com os fluxos padrão.
    pub fn padrao() -> Self {
        Self::novo(vec![Box::new(mercado::Mercado::novo())])
    }

    pub fn receber(&mut self, msg: &Value, agora: i64) {
        for f in &mut self.fluxos {
            f.receber(msg, agora);
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

    pub fn entregar(&mut self) -> Vec<Entrega> {
        self.fluxos.iter_mut().flat_map(|f| f.entregar()).collect()
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
        fn entregar(&mut self) -> Vec<Entrega> {
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
