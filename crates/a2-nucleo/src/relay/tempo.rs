//! Datas no fuso do jogador (UTC−3, como o motor do legado no PC).

/// Fuso do dia: UTC−3.
pub const FUSO_MS: i64 = -3 * 3_600_000;
const DIA_MS: i64 = 86_400_000;

/// Ano, mês e dia (civil) a partir de dias desde 1970-01-01.
fn civil(dias: i64) -> (i64, u32, u32) {
    let z = dias + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let a = yoe + era * 400 + i64::from(m <= 2);
    (a, m, d)
}

fn local(ms: i64) -> (i64, u32, u32) {
    civil((ms + FUSO_MS).div_euclid(DIA_MS))
}

/// `AAAA-MM-DD` do instante (o `toLocaleDateString('sv-SE')` do legado).
pub fn dia_de(ms: i64) -> String {
    let (a, m, d) = local(ms);
    format!("{a:04}-{m:02}-{d:02}")
}

/// Dias que faltam para acabar o mês (sem contar hoje).
pub fn dias_restantes_no_mes(ms: i64) -> u32 {
    let (a, m, d) = local(ms);
    let bissexto = (a % 4 == 0 && a % 100 != 0) || a % 400 == 0;
    let total = match m {
        2 if bissexto => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    total - d
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn dia_no_fuso_do_jogador() {
        // 2026-11-14 02:00 UTC ainda é dia 13 em UTC−3.
        assert_eq!(dia_de(1_794_621_600_000), "2026-11-13");
        assert_eq!(dia_de(1_794_632_400_000), "2026-11-14");
        assert_eq!(dia_de(0), "1969-12-31");
        assert_eq!(dias_restantes_no_mes(1_794_632_400_000), 16);
        // 29/02/2024 12:00 UTC.
        assert_eq!(dia_de(1_709_208_000_000), "2024-02-29");
        assert_eq!(dias_restantes_no_mes(1_709_208_000_000), 0);
    }
}
