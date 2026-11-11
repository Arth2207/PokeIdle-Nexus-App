//! Identificador de ticker, no mesmo formato de texto do legado:
//!
//! - pokémon: `p:<especie>:<shiny 0|1>:<moeda>`, ex. `p:205:0:orb`
//! - item com id: `i:<item_id>:<moeda>`, ex. `i:59194:gold`
//! - item sem id no catálogo: `i:n:<nome em minúsculas>:<moeda>`

use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Moeda {
    Orb,
    Gold,
}

impl Moeda {
    pub fn como_str(self) -> &'static str {
        match self {
            Moeda::Orb => "orb",
            Moeda::Gold => "gold",
        }
    }
}

impl FromStr for Moeda {
    type Err = ErroTicker;

    fn from_str(texto: &str) -> Result<Self, Self::Err> {
        match texto {
            "orb" => Ok(Moeda::Orb),
            "gold" => Ok(Moeda::Gold),
            _ => Err(ErroTicker),
        }
    }
}

impl fmt::Display for Moeda {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.como_str())
    }
}

/// Como o item é identificado: pelo id do catálogo ou, na falta dele, pelo nome.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ChaveItem {
    Id(i32),
    Nome(String),
}

/// `Hash` + `Eq` permitem usar o ticker como chave de `HashMap`, como uma string em um
/// `Map` do JS. Aqui a chave é tipada: não dá para confundir um id de item com uma espécie.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TickerId {
    Pokemon {
        especie: i32,
        shiny: bool,
        moeda: Moeda,
    },
    Item {
        item: ChaveItem,
        moeda: Moeda,
    },
}

impl fmt::Display for TickerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TickerId::Pokemon {
                especie,
                shiny,
                moeda,
            } => write!(f, "p:{especie}:{}:{moeda}", u8::from(*shiny)),
            TickerId::Item {
                item: ChaveItem::Id(id),
                moeda,
            } => write!(f, "i:{id}:{moeda}"),
            TickerId::Item {
                item: ChaveItem::Nome(nome),
                moeda,
            } => write!(f, "i:n:{nome}:{moeda}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("id de ticker inválido")]
pub struct ErroTicker;

impl FromStr for TickerId {
    type Err = ErroTicker;

    fn from_str(texto: &str) -> Result<Self, Self::Err> {
        // A moeda é sempre o último campo. `rsplit_once` corta no último ':', então um
        // nome de item com ':' no meio continua inteiro.
        let (resto, moeda) = texto.rsplit_once(':').ok_or(ErroTicker)?;
        let moeda = moeda.parse()?;

        if let Some(nome) = resto.strip_prefix("i:n:") {
            return Ok(TickerId::Item {
                item: ChaveItem::Nome(nome.to_owned()),
                moeda,
            });
        }
        if let Some(id) = resto.strip_prefix("i:") {
            let id = id.parse().map_err(|_| ErroTicker)?;
            return Ok(TickerId::Item {
                item: ChaveItem::Id(id),
                moeda,
            });
        }
        let (especie, shiny) = resto
            .strip_prefix("p:")
            .and_then(|r| r.split_once(':'))
            .ok_or(ErroTicker)?;
        let shiny = match shiny {
            "0" => false,
            "1" => true,
            _ => return Err(ErroTicker),
        };
        Ok(TickerId::Pokemon {
            especie: especie.parse().map_err(|_| ErroTicker)?,
            shiny,
            moeda,
        })
    }
}

#[cfg(test)]
mod testes {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn formatos_do_legado() {
        let casos = [
            "p:205:0:orb",
            "p:6:1:gold",
            "i:59194:gold",
            "i:n:rare candy:orb",
        ];
        for texto in casos {
            let id: TickerId = texto.parse().unwrap();
            assert_eq!(id.to_string(), texto);
        }
    }

    #[test]
    fn rejeita_lixo() {
        for texto in ["", "x", "p:1:2:orb", "p:1:0:usd", "i:abc:orb", "q:1:orb"] {
            assert!(texto.parse::<TickerId>().is_err(), "{texto}");
        }
    }

    fn moeda() -> impl Strategy<Value = Moeda> {
        prop_oneof![Just(Moeda::Orb), Just(Moeda::Gold)]
    }

    fn ticker() -> impl Strategy<Value = TickerId> {
        prop_oneof![
            (any::<i32>(), any::<bool>(), moeda()).prop_map(|(especie, shiny, moeda)| {
                TickerId::Pokemon {
                    especie,
                    shiny,
                    moeda,
                }
            }),
            (any::<i32>(), moeda()).prop_map(|(id, moeda)| TickerId::Item {
                item: ChaveItem::Id(id),
                moeda,
            }),
            (".*", moeda()).prop_map(|(nome, moeda)| TickerId::Item {
                item: ChaveItem::Nome(nome),
                moeda,
            }),
        ]
    }

    // proptest gera centenas de entradas aleatórias e, se achar uma que falha, encolhe até
    // o menor exemplo que ainda falha. Parecido com fast-check no JS.
    proptest! {
        #[test]
        fn texto_ida_e_volta(id in ticker()) {
            prop_assert_eq!(id.to_string().parse::<TickerId>(), Ok(id));
        }

        #[test]
        fn parse_nunca_entra_em_panico(texto in ".*") {
            let _ = texto.parse::<TickerId>();
        }
    }
}
