//! Espera entre tentativas de reconexão: 1 s, 2 s, 4 s... até 30 s, com jitter de ±20%.

use std::time::Duration;

const BASE_MS: u64 = 1_000;
const TETO_MS: u64 = 30_000;

#[derive(Debug, Clone)]
pub struct Backoff {
    tentativa: u32,
    estado: u64,
}

impl Backoff {
    /// `semente` alimenta o jitter (gerador congruencial: sem dependência de `rand`).
    pub fn novo(semente: u64) -> Self {
        Self {
            tentativa: 0,
            estado: semente | 1,
        }
    }

    /// Chamar quando a conexão ficou de pé.
    pub fn zerar(&mut self) {
        self.tentativa = 0;
    }

    pub fn proximo(&mut self) -> Duration {
        let base = (BASE_MS << self.tentativa.min(10)).min(TETO_MS);
        self.tentativa = self.tentativa.saturating_add(1);
        self.estado = self
            .estado
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        // 0..=400 permille => fator 0,8..=1,2
        let permille = 800 + (self.estado >> 33) % 401;
        Duration::from_millis(base * permille / 1000)
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn cresce_e_respeita_teto_com_jitter() {
        let mut b = Backoff::novo(7);
        let esperas: Vec<u64> = (0..12).map(|_| b.proximo().as_millis() as u64).collect();
        assert!((800..=1200).contains(&esperas[0]));
        assert!((1600..=2400).contains(&esperas[1]));
        assert!((3200..=4800).contains(&esperas[2]));
        assert!(esperas.iter().all(|&e| e <= 36_000));
        assert!((24_000..=36_000).contains(&esperas[11]));
    }

    #[test]
    fn zerar_volta_ao_inicio() {
        let mut b = Backoff::novo(1);
        for _ in 0..5 {
            b.proximo();
        }
        b.zerar();
        assert!(b.proximo().as_millis() <= 1200);
    }
}
