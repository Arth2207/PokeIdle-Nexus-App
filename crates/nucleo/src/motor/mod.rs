//! Motor de automação (fase 4): as regras do `motor.js` do legado, sem I/O.
//!
//! `d` é o objeto `D` do `holodex-dados.json` (veja `persistencia::motor`) com as chaves na
//! ordem do arquivo. A ordem importa: o legado itera objetos na ordem de inserção, e é ela que
//! desempata, por exemplo, os drops do dia com a mesma quantidade.
//!
//! As contas "ao vivo" (janelas do jogo abertas, com `ultimo` e `welcome`) ainda não existem no
//! Nexus: chegam pela A2. Até lá, cada rota responde como o legado responde sem nenhuma conta
//! conectada, e a parte que depende do jogo fica para quando o estado chegar.

pub mod config;
pub mod jogo;
pub mod laco;
pub mod leitura;
pub mod progressao;
pub mod regras;
pub mod veredito;

use serde_json::Value;

use crate::js;

const DIA_MS: i64 = 86_400_000;
const HORA_MS: i64 = 3_600_000;
/// Maior instante que o `Date` do JS aceita (±8,64e15 ms).
const DATA_MAX_MS: f64 = 8.64e15;

/// Instante e fuso do computador onde o legado roda. `toLocaleDateString` e `getHours` usam o
/// fuso local: UTC na VPS, UTC−3 no PC. O Brasil não tem horário de verão desde 2019, então
/// um deslocamento fixo basta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Relogio {
    pub agora_ms: i64,
    pub fuso_ms: i64,
}

impl Relogio {
    /// `diaDe(t)`: `new Date(t).toLocaleDateString('sv-SE')`, a data `AAAA-MM-DD` local.
    pub fn dia(&self, t: i64) -> String {
        js::data_do_dia((t + self.fuso_ms).div_euclid(DIA_MS))
    }

    pub fn hoje(&self) -> String {
        self.dia(self.agora_ms)
    }

    /// `diaDe(Date.now() - 864e5)`.
    pub fn ontem(&self) -> String {
        self.dia(self.agora_ms - DIA_MS)
    }

    /// `new Date().getHours()`.
    pub fn hora(&self) -> i64 {
        (self.agora_ms + self.fuso_ms).rem_euclid(DIA_MS) / HORA_MS
    }

    /// `new Date(k + 'T00:00:00').getTime()`: meia-noite local do dia `k`.
    pub fn meia_noite(&self, k: &str) -> Option<i64> {
        js::dia_da_data(k).map(|d| d * DIA_MS - self.fuso_ms)
    }

    /// `diaDe(v)` de um valor do arquivo (`a.t`). Sem o campo vale o padrão do parâmetro
    /// (`Date.now()`, hoje); `new Date(null)` é o instante 0; texto ou objeto viram
    /// "Invalid Date", que não é igual a nenhum dia (`None`).
    pub fn dia_de(&self, v: Option<&Value>) -> Option<String> {
        match v {
            None => Some(self.hoje()),
            Some(Value::Null) => Some(self.dia(0)),
            Some(Value::Number(n)) => {
                let x = n.as_f64()?;
                // TimeClip: trunca para inteiro; fora da faixa é data inválida.
                (x.is_finite() && x.abs() <= DATA_MAX_MS).then(|| self.dia(x.trunc() as i64))
            }
            _ => None,
        }
    }

    /// `new Date(ano, mes + 1, 0).getDate() - agora.getDate()`: dias até o fim do mês local.
    pub fn dias_restantes_mes(&self) -> i64 {
        let hoje = self.hoje();
        let (Some(a), Some(m), Some(d)) = (
            hoje.get(0..4).and_then(|x| x.parse::<i64>().ok()),
            hoje.get(5..7).and_then(|x| x.parse::<i64>().ok()),
            hoje.get(8..10).and_then(|x| x.parse::<i64>().ok()),
        ) else {
            return 0;
        };
        let (pa, pm) = if m == 12 { (a + 1, 1) } else { (a, m + 1) };
        let ultimo = js::dia_da_data(&format!("{pa:04}-{pm:02}-01"))
            .and_then(|p| js::dia_da_data(&format!("{a:04}-{m:02}-01")).map(|i| p - i))
            .unwrap_or(31);
        ultimo - d
    }
}

/// `a != null` do JS: nem `undefined` (`None`) nem `null`.
pub(crate) fn existe(v: Option<&Value>) -> bool {
    !matches!(v, None | Some(Value::Null))
}

/// `v || padrao` do JS, devolvendo o próprio valor quando ele é verdadeiro.
pub(crate) fn ou<'a>(v: Option<&'a Value>, padrao: &'a Value) -> &'a Value {
    match v {
        Some(x) if js::verdadeiro(Some(x)) => x,
        _ => padrao,
    }
}

/// `+(v || 0)`: número do campo, com falso virando 0.
pub(crate) fn num_ou_0(v: Option<&Value>) -> f64 {
    if js::verdadeiro(v) {
        js::mais(v)
    } else {
        0.0
    }
}

/// `o?.[k]` encadeado: segue as chaves enquanto houver objeto.
pub(crate) fn caminho<'a>(v: &'a Value, chaves: &[&str]) -> Option<&'a Value> {
    chaves.iter().try_fold(v, |atual, k| match atual {
        Value::Object(o) => o.get(*k),
        _ => None,
    })
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    const SP: i64 = -3 * HORA_MS;

    #[test]
    fn relogio_no_fuso_local() {
        // 2026-11-12 01:30 UTC = 2026-11-11 22:30 em São Paulo.
        let t = 1_794_447_000_000;
        let utc = Relogio {
            agora_ms: t,
            fuso_ms: 0,
        };
        let sp = Relogio {
            agora_ms: t,
            fuso_ms: SP,
        };
        assert_eq!(utc.hoje(), "2026-11-12");
        assert_eq!(sp.hoje(), "2026-11-11");
        assert_eq!(sp.ontem(), "2026-11-10");
        assert_eq!(utc.hora(), 1);
        assert_eq!(sp.hora(), 22);
        assert_eq!(sp.meia_noite("2026-11-11"), Some(1_794_366_000_000));
        assert_eq!(utc.dias_restantes_mes(), 18);
        assert_eq!(sp.dias_restantes_mes(), 19);
    }

    #[test]
    fn dia_de_valores_do_arquivo() {
        let r = Relogio {
            agora_ms: 0,
            fuso_ms: 0,
        };
        assert_eq!(r.dia_de(Some(&json!(null))).as_deref(), Some("1970-01-01"));
        assert_eq!(r.dia_de(Some(&json!("x"))), None);
        assert_eq!(
            r.dia_de(None).as_deref(),
            Some("1970-01-01"),
            "undefined = hoje"
        );
        assert_eq!(r.dia_de(Some(&json!(9e15))), None);
        assert_eq!(
            r.dia_de(Some(&json!(86_400_000.7))).as_deref(),
            Some("1970-01-02")
        );
    }

    #[test]
    fn fim_de_fevereiro_e_dezembro() {
        let em = |data: &str| Relogio {
            agora_ms: js::dia_da_data(data).unwrap_or(0) * DIA_MS,
            fuso_ms: 0,
        };
        assert_eq!(em("2028-02-10").dias_restantes_mes(), 19);
        assert_eq!(em("2026-12-31").dias_restantes_mes(), 0);
    }
}
