//! Seleção de equipes de boss: cópia do `bolsa-equipe-selecao.js` do legado.
//!
//! Regra do jogo: 5 vagas; o dano recebido pela equipe é multiplicado por
//! `3^(5 − Σ mín(1, nível/boss))`. Três perfis: Econômica (o mínimo que passa com ~105% da
//! meta), Equilibrada (melhor custo-benefício pagando o preço justo ou menos) e Máxima.
//!
//! A busca é heurística (sementes, trocas e feixe), como no legado. Diferença deliberada: o
//! legado desiste da subida do teto de nível depois de 3 s de relógio, o que faz o resultado
//! depender da máquina; aqui não há limite de tempo, e a mesma entrada dá sempre a mesma
//! saída.

use std::collections::HashSet;

use crate::js;

const PISO_NIVEL: f64 = 0.70;
const TETO_NIVEL: f64 = 2.5;
const SEM_HEROI: f64 = 0.50;
const ESTRESSE_CAUSADO: f64 = 0.85;
const ESTRESSE_RECEBIDO: f64 = 1.30;
const TETO_PASSO: f64 = 0.1;
const TETO_SEGURANCA: f64 = 40.0;
const TETO_AVISO: f64 = 5.0;
const FEIXE: usize = 3;
const RESERVAS: usize = 2;
const TEMPO_MAXIMO: f64 = 300.0;
const EPS: f64 = 1e-9;

/// Limites expostos na resposta (`limites`).
pub const LIMITE_PISO: f64 = PISO_NIVEL;
pub const LIMITE_TETO_NIVEL: f64 = TETO_NIVEL;
pub const LIMITE_HEROI: f64 = SEM_HEROI;
pub const LIMITE_ESTRESSE: (f64, f64) = (ESTRESSE_CAUSADO, ESTRESSE_RECEBIDO);

/// Uma carta candidata (oferta ou pokémon da conta).
#[derive(Debug, Clone, PartialEq)]
pub struct Candidato {
    pub id: String,
    /// Espécie (no máximo 2 por equipe).
    pub sp: i64,
    pub lvl: f64,
    /// Preço unitário (ou um custo relativo, na equipe própria).
    pub unit: f64,
    /// Valor justo estimado; `NaN` quando não há avaliação.
    pub justo: f64,
    pub hp: f64,
    pub dps_out: f64,
    pub dps_in: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Perfil {
    Eco,
    Eq,
    Max,
}

impl Perfil {
    pub fn nome(self) -> &'static str {
        match self {
            Perfil::Eco => "Econômica",
            Perfil::Eq => "Equilibrada",
            Perfil::Max => "Máxima",
        }
    }
    fn teto_mult(self) -> Option<f64> {
        match self {
            Perfil::Eco => Some(3.0),
            Perfil::Eq => Some(1.5),
            Perfil::Max => None,
        }
    }
}

/// O que torna uma equipe viável.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Regra {
    mult: Option<f64>,
    heroi: bool,
    estresse: bool,
}

/// Métricas de uma equipe (índices em `candidatos`).
#[derive(Debug, Clone, PartialEq)]
pub struct Equipe {
    pub pick: Vec<usize>,
    pub forca: f64,
    pub mult: f64,
    pub danos: Vec<f64>,
    pub d: f64,
    pub ds: f64,
    pub hero: f64,
    pub total: f64,
}

/// Opções do `selecionar`.
#[derive(Debug, Clone, PartialEq)]
pub struct Opcoes {
    pub n: usize,
    pub nivel_boss: f64,
    pub referencia: f64,
    pub teto_justo: bool,
    pub moeda_orb: bool,
    pub so_max: bool,
    pub so_eco: bool,
}

/// Restrições cumpridas pela equipe escolhida.
#[derive(Debug, Clone, PartialEq)]
pub struct Restricoes {
    pub piso: bool,
    pub teto_nivel: bool,
    pub mult: bool,
    pub heroi: bool,
    pub estresse: bool,
    pub relaxadas: Vec<&'static str>,
}

/// Uma reserva para uma vaga.
#[derive(Debug, Clone, PartialEq)]
pub struct Reserva {
    pub x: usize,
    pub margem: f64,
    pub total: f64,
}

/// Resultado de um perfil (o `pack` do legado).
#[derive(Debug, Clone, PartialEq)]
pub struct ResultadoPerfil {
    pub alvo: Option<f64>,
    pub indisponivel: bool,
    pub inviavel: bool,
    pub equipe: Option<Equipe>,
    pub poder: f64,
    pub margem_estresse: f64,
    pub abaixo: usize,
    pub pior_faltam: f64,
    pub t_sobrevive: Option<f64>,
    pub restr: Option<Restricoes>,
    pub teto_mult: Option<f64>,
    pub teto_usado: Option<f64>,
    /// Planos B e C.
    pub alternativas: Vec<ResultadoPerfil>,
    pub reservas: Vec<Vec<Reserva>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Selecao {
    pub eco: ResultadoPerfil,
    pub eq: ResultadoPerfil,
    pub max: ResultadoPerfil,
    pub avisos: Vec<String>,
    pub elegiveis: usize,
    pub excluidas_preco: usize,
    pub sem_avaliacao: usize,
    pub impossivel: bool,
}

pub const METODO: &str = "busca por trocas; mínimo global não garantido";

/// `Math.max` do JS: `NaN` contamina.
fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.min(b)
    }
}

/// Comparador de `sort` do JS a partir de uma diferença numérica: `NaN` vale como empate.
fn ordem(d: f64) -> std::cmp::Ordering {
    if d < 0.0 {
        std::cmp::Ordering::Less
    } else if d > 0.0 {
        std::cmp::Ordering::Greater
    } else {
        std::cmp::Ordering::Equal
    }
}

/// `a || b` para números (0 e `NaN` caem no segundo).
fn ou(a: f64, b: f64) -> f64 {
    if a == 0.0 || a.is_nan() {
        b
    } else {
        a
    }
}

/// Contexto da busca sobre um pool (o `montar` do legado).
struct Busca<'a> {
    c: &'a [Candidato],
    n: usize,
    l: f64,
    referencia: f64,
    pool: Vec<usize>,
    reg: Regra,
    seeds: Vec<Vec<usize>>,
    maximum: Option<Equipe>,
}

fn chave(c: &[Candidato], pick: &[usize]) -> String {
    let mut ids: Vec<&str> = pick.iter().map(|&i| c[i].id.as_str()).collect();
    ids.sort_unstable();
    ids.join(",")
}

impl<'a> Busca<'a> {
    fn metrics(&self, pick: &[usize]) -> Equipe {
        let c = self.c;
        let forca = pick
            .iter()
            .fold(0.0, |s, &i| s + js_min(1.0, c[i].lvl / self.l));
        let mult = js::pow(3.0, js_max(0.0, self.n as f64 - forca));
        let danos: Vec<f64> = pick.iter().map(|&i| dano(&c[i], mult)).collect();
        let d = danos.iter().fold(0.0, |a, b| a + b);
        let ds = pick.iter().fold(0.0, |s, &i| {
            let x = &c[i];
            let t = if x.dps_in > 0.0 {
                js_min(TEMPO_MAXIMO, x.hp / (x.dps_in * mult * ESTRESSE_RECEBIDO))
            } else {
                TEMPO_MAXIMO
            };
            s + x.dps_out * ESTRESSE_CAUSADO * t
        });
        let hero = if d > 0.0 {
            danos.iter().copied().fold(f64::NEG_INFINITY, js_max) / d
        } else {
            0.0
        };
        let total = pick.iter().fold(0.0, |s, &i| s + c[i].unit);
        Equipe {
            pick: pick.to_vec(),
            forca,
            mult,
            danos,
            d,
            ds,
            hero,
            total,
        }
    }

    fn viavel(&self, m: &Equipe) -> bool {
        self.reg.mult.is_none_or(|t| m.mult <= t + EPS)
            && (!self.reg.heroi || m.hero <= SEM_HEROI + EPS)
            && (!self.reg.estresse || m.ds >= self.referencia - EPS)
    }

    fn valid(&self, pick: &[usize]) -> bool {
        if pick.len() != self.n {
            return false;
        }
        let ids: HashSet<&str> = pick.iter().map(|&i| self.c[i].id.as_str()).collect();
        ids.len() == self.n
            && pick
                .iter()
                .all(|&i| conta_sp(self.c, pick, self.c[i].sp) <= 2)
    }

    fn choose(&self, rank: &[usize]) -> Vec<usize> {
        let mut pick = Vec::new();
        for &x in rank {
            if conta_sp(self.c, &pick, self.c[x].sp) >= 2 {
                continue;
            }
            pick.push(x);
            if pick.len() == self.n {
                break;
            }
        }
        pick
    }

    fn indi(&self, x: &Candidato) -> f64 {
        let mult = js::pow(3.0, 5.0 * js_max(0.0, 1.0 - js_min(1.0, x.lvl / self.l)));
        dano(x, mult)
    }

    fn add(&mut self, rank: &[usize]) {
        let p = self.choose(rank);
        if self.valid(&p) {
            let k = chave(self.c, &p);
            if !self.seeds.iter().any(|s| chave(self.c, s) == k) {
                self.seeds.push(p);
            }
        }
    }

    fn ordenado(&self, f: impl Fn(&Candidato) -> f64) -> Vec<usize> {
        let mut r = self.pool.clone();
        r.sort_by(|&a, &b| {
            let (a, b) = (&self.c[a], &self.c[b]);
            ordem(ou(f(b) - f(a), a.unit - b.unit))
        });
        r
    }

    fn montar(
        c: &'a [Candidato],
        n: usize,
        l: f64,
        referencia: f64,
        pool: Vec<usize>,
        reg: Regra,
    ) -> Self {
        let mut b = Busca {
            c,
            n,
            l,
            referencia,
            pool,
            reg,
            seeds: Vec::new(),
            maximum: None,
        };
        let base = |x: &Candidato| dano(x, 1.0);
        let rankings: Vec<Vec<usize>> = vec![
            b.ordenado(|x| -x.unit),
            b.ordenado(|x| b.indi(x)),
            b.ordenado(base),
            b.ordenado(|x| b.indi(x) / x.unit),
            b.ordenado(|x| base(x) / x.unit),
            b.ordenado(|x| js_min(1.0, x.lvl / l)),
        ];
        for r in &rankings {
            b.add(r);
        }
        let max_ratio = b
            .pool
            .iter()
            .fold(0.0, |m, &i| js_max(m, b.indi(&c[i]) / c[i].unit));
        for i in 1..=32 {
            let lim = max_ratio * 2.0 * js::pow(f64::from(i) / 32.0, 4.0);
            let r = b.ordenado(|x| b.indi(x) - lim * x.unit);
            b.add(&r);
        }
        let mut melhores: Vec<(f64, &Vec<usize>)> =
            b.seeds.iter().map(|s| (b.metrics(s).d, s)).collect();
        melhores.sort_by(|x, y| ordem(y.0 - x.0));
        let best = melhores.first().map(|(_, s)| (*s).clone());
        b.maximum = best.map(|s| b.improve(&s, None));
        b
    }

    fn improve(&self, seed: &[usize], target: Option<f64>) -> Equipe {
        let mut pick = seed.to_vec();
        let mut m = self.metrics(&pick);
        for _ in 0..12 {
            let mut best: Option<Equipe> = None;
            for i in 0..pick.len() {
                for &x in &self.pool {
                    if pick.iter().any(|&y| self.c[y].id == self.c[x].id) {
                        continue;
                    }
                    if target.is_some() && self.c[x].unit >= self.c[pick[i]].unit {
                        continue;
                    }
                    let mut trial = pick.clone();
                    trial[i] = x;
                    if !self.valid(&trial) {
                        continue;
                    }
                    let t = self.metrics(&trial);
                    let melhora = match target {
                        None => t.d > m.d + EPS,
                        Some(tg) => t.d + EPS >= tg && self.viavel(&t) && t.total < m.total - EPS,
                    };
                    if !melhora {
                        continue;
                    }
                    let troca = match &best {
                        None => true,
                        Some(b) => match target {
                            None => {
                                t.d > b.d + EPS || ((t.d - b.d).abs() < EPS && t.total < b.total)
                            }
                            Some(_) => t.total < b.total || (t.total == b.total && t.d > b.d),
                        },
                    };
                    if troca {
                        best = Some(t);
                    }
                }
            }
            match best {
                Some(b) => {
                    pick = b.pick.clone();
                    m = b;
                }
                None => break,
            }
        }
        m
    }

    /// Busca em feixe: equipes parciais pontuadas por dano útil por gema.
    fn feixe(&self) -> Vec<Vec<usize>> {
        let c = self.c;
        let mut pool = self.pool.clone();
        pool.sort_by(|&a, &b| ordem(ou(c[a].lvl - c[b].lvl, c[a].unit - c[b].unit)));
        let score = |q: &[usize]| {
            let forca = q
                .iter()
                .fold(0.0, |s, &i| s + js_min(1.0, c[i].lvl / self.l))
                + (self.n - q.len()) as f64;
            let mult = js::pow(3.0, js_max(0.0, self.n as f64 - forca));
            q.iter().fold(0.0, |s, &i| s + dano(&c[i], mult))
                / q.iter().fold(0.0, |s, &i| s + c[i].unit)
        };
        let mut beam: Vec<Vec<usize>> = vec![Vec::new()];
        for _ in 1..=self.n {
            let mut vistos = HashSet::new();
            let mut cand: Vec<(Vec<usize>, f64, f64)> = Vec::new();
            for p in &beam {
                for &x in &pool {
                    if p.iter().any(|&y| c[y].id == c[x].id) || conta_sp(c, p, c[x].sp) >= 2 {
                        continue;
                    }
                    let mut q = p.clone();
                    q.push(x);
                    if vistos.insert(chave(c, &q)) {
                        let s = score(&q);
                        let nv = q.iter().fold(0.0, |t, &i| t + c[i].lvl);
                        cand.push((q, s, nv));
                    }
                }
            }
            cand.sort_by(|a, b| ordem(ou(b.1 - a.1, a.2 - b.2)));
            beam = cand.into_iter().take(FEIXE).map(|(q, ..)| q).collect();
            if beam.is_empty() {
                break;
            }
        }
        beam.into_iter().filter(|q| q.len() == self.n).collect()
    }

    fn minimo_todos(&self, alvo: f64) -> Vec<Equipe> {
        let Some(maximum) = &self.maximum else {
            return Vec::new();
        };
        let target = alvo * self.referencia;
        let mut fontes: Vec<Equipe> = self
            .seeds
            .iter()
            .cloned()
            .chain(std::iter::once(maximum.pick.clone()))
            .chain(self.feixe())
            .map(|p| self.metrics(&p))
            .filter(|m| m.d + EPS >= target && self.viavel(m))
            .collect();
        fontes.sort_by(|a, b| ordem(a.total - b.total));
        fontes.truncate(9);
        let mut vistos = HashSet::new();
        let mut out = Vec::new();
        for f in fontes {
            let r = self.improve(&f.pick, Some(target));
            if vistos.insert(chave(self.c, &r.pick)) {
                out.push(r);
            }
        }
        out.sort_by(|a, b| ordem(ou(a.total - b.total, b.d - a.d)));
        out
    }

    fn reservas(&self, pick: &[usize], alvo: f64) -> Vec<Vec<Reserva>> {
        let c = self.c;
        pick.iter()
            .enumerate()
            .map(|(i, &tit)| {
                let mut out: Vec<(f64, Reserva)> = Vec::new();
                for &x in &self.pool {
                    if pick.iter().any(|&y| c[y].id == c[x].id) {
                        continue;
                    }
                    let mut t = pick.to_vec();
                    t[i] = x;
                    if !self.valid(&t) {
                        continue;
                    }
                    let m = self.metrics(&t);
                    if m.d + EPS < alvo * self.referencia || !self.viavel(&m) {
                        continue;
                    }
                    let d = js::ln(c[x].unit / c[tit].unit).abs()
                        + 0.5 * js::ln(c[x].lvl / c[tit].lvl).abs();
                    out.push((
                        d,
                        Reserva {
                            x,
                            margem: m.d / self.referencia,
                            total: m.total,
                        },
                    ));
                }
                out.sort_by(|a, b| ordem(a.0 - b.0));
                out.into_iter().take(RESERVAS).map(|(_, r)| r).collect()
            })
            .collect()
    }

    fn alternativos(&self, principal: &Equipe, alvo: f64) -> Vec<Equipe> {
        let mut vistos = HashSet::from([chave(self.c, &principal.pick)]);
        let mut out = Vec::new();
        for &tit in &principal.pick {
            let pool2: Vec<usize> = self
                .pool
                .iter()
                .copied()
                .filter(|&x| self.c[x].id != self.c[tit].id)
                .collect();
            if pool2.len() < self.n {
                continue;
            }
            let b = Busca::montar(self.c, self.n, self.l, self.referencia, pool2, self.reg);
            let Some(r) = b.minimo_todos(alvo).into_iter().next() else {
                continue;
            };
            if vistos.insert(chave(self.c, &r.pick)) {
                out.push(r);
            }
        }
        out.sort_by(|a, b| ordem(ou(a.total - b.total, b.d - a.d)));
        out.truncate(2);
        out
    }
}

fn conta_sp(c: &[Candidato], pick: &[usize], sp: i64) -> usize {
    pick.iter().filter(|&&i| c[i].sp == sp).count()
}

/// Dano que a carta entrega antes de cair, com o dano recebido × `mult`.
fn dano(x: &Candidato, mult: f64) -> f64 {
    x.dps_out
        * if x.dps_in > 0.0 {
            js_min(TEMPO_MAXIMO, x.hp / (x.dps_in * mult))
        } else {
            TEMPO_MAXIMO
        }
}

/// `pack` do legado: o resultado de um perfil a partir da equipe escolhida.
fn empacotar(
    c: &[Candidato],
    l: f64,
    referencia: f64,
    r: Option<&Equipe>,
    alvo: Option<f64>,
    teto: Option<f64>,
    rel: Vec<&'static str>,
) -> ResultadoPerfil {
    let Some(r) = r else {
        return ResultadoPerfil {
            alvo,
            indisponivel: true,
            inviavel: true,
            equipe: None,
            poder: 0.0,
            margem_estresse: 0.0,
            abaixo: 0,
            pior_faltam: 0.0,
            t_sobrevive: None,
            restr: None,
            teto_mult: teto,
            teto_usado: None,
            alternativas: Vec::new(),
            reservas: Vec::new(),
        };
    };
    let ab: Vec<usize> = r.pick.iter().copied().filter(|&i| c[i].lvl < l).collect();
    let t_sob = r.pick.iter().fold(f64::INFINITY, |m, &i| {
        let x = &c[i];
        js_min(
            m,
            if x.dps_in > 0.0 {
                js_min(TEMPO_MAXIMO, x.hp / (x.dps_in * r.mult))
            } else {
                TEMPO_MAXIMO
            },
        )
    });
    ResultadoPerfil {
        alvo,
        indisponivel: false,
        inviavel: false,
        poder: r.d / referencia,
        margem_estresse: r.ds / referencia,
        abaixo: ab.len(),
        pior_faltam: if ab.is_empty() {
            0.0
        } else {
            ab.iter()
                .fold(f64::NEG_INFINITY, |m, &i| js_max(m, l - c[i].lvl))
        },
        t_sobrevive: Some(js::round(t_sob)),
        restr: Some(Restricoes {
            piso: r.pick.iter().all(|&i| c[i].lvl >= PISO_NIVEL * l),
            teto_nivel: r.pick.iter().all(|&i| c[i].lvl <= TETO_NIVEL * l),
            mult: teto.is_none_or(|t| r.mult <= t + EPS),
            heroi: r.hero <= SEM_HEROI + EPS,
            estresse: r.ds >= referencia - EPS,
            relaxadas: rel,
        }),
        teto_mult: teto,
        teto_usado: None,
        equipe: Some(r.clone()),
        alternativas: Vec::new(),
        reservas: Vec::new(),
    }
}

/// Seleção sem nenhuma equipe (nível do boss inválido).
fn selecao_vazia(c: &[Candidato]) -> Selecao {
    let vazio = || empacotar(c, 1.0, 1.0, None, None, None, Vec::new());
    Selecao {
        eco: vazio(),
        eq: vazio(),
        max: vazio(),
        avisos: vec!["Nível do boss inválido.".into()],
        elegiveis: 0,
        excluidas_preco: 0,
        sem_avaliacao: c.len(),
        impossivel: true,
    }
}

/// Resultado do `resolver` de um perfil.
struct Resolvido<'a> {
    sel: Equipe,
    outros: Vec<Equipe>,
    m: Busca<'a>,
    rel: Vec<&'static str>,
    teto_usado: Option<f64>,
    alvo: f64,
}

/// `selecionar(candidatos, opções)`.
#[allow(clippy::too_many_lines)]
pub fn selecionar(c: &[Candidato], op: &Opcoes) -> Selecao {
    let (n, l, referencia) = (op.n, op.nivel_boss, op.referencia);
    // Cartas com número não finito ficam de fora: NaN nas chaves de ordenação quebraria o
    // `sort` (e não há equipe útil com elas).
    if !(l.is_finite() && l > 0.0) {
        return selecao_vazia(c);
    }
    let pool_todos: Vec<usize> = (0..c.len())
        .filter(|&i| {
            [c[i].lvl, c[i].hp, c[i].dps_out, c[i].dps_in]
                .iter()
                .all(|v| v.is_finite())
        })
        .filter(|&i| c[i].unit.is_finite() && c[i].unit > 0.0)
        .collect();
    let pool_justo: Vec<usize> = pool_todos
        .iter()
        .copied()
        .filter(|&i| {
            let x = &c[i];
            !op.teto_justo
                || !op.moeda_orb
                || (x.justo.is_finite() && x.justo > 0.0 && x.unit <= x.justo)
        })
        .collect();
    let mut avisos: Vec<String> = Vec::new();
    let lvl_max = c.iter().fold(0.0, |m, x| js_max(m, ou(x.lvl, 0.0)));
    let m0 = (pool_todos.len() >= n)
        .then(|| Busca::montar(c, n, l, referencia, pool_todos.clone(), Regra::default()));
    let impossivel = m0
        .as_ref()
        .and_then(|m| m.maximum.as_ref())
        .is_none_or(|mx| mx.d < 1.05 * referencia);

    let resolver = |pool_base: &[usize], alvo: f64, perfil: Perfil| -> Option<Resolvido<'_>> {
        if impossivel {
            return None;
        }
        let teto = perfil.teto_mult();
        let base: Vec<usize> = pool_base
            .iter()
            .copied()
            .filter(|&i| c[i].lvl >= PISO_NIVEL * l)
            .collect();
        if base.len() < n {
            return None;
        }
        let t_max = TETO_SEGURANCA.min(js_max(TETO_NIVEL, lvl_max / l));
        let mut t = TETO_NIVEL;
        loop {
            let pool: Vec<usize> = base
                .iter()
                .copied()
                .filter(|&i| c[i].lvl <= t * l + 1e-6)
                .collect();
            if pool.len() >= n {
                let m = Busca::montar(
                    c,
                    n,
                    l,
                    referencia,
                    pool,
                    Regra {
                        mult: teto,
                        heroi: true,
                        estresse: true,
                    },
                );
                let sel = m.minimo_todos(alvo);
                if let Some(primeira) = sel.first().cloned() {
                    let mut outros: Vec<Equipe> = sel.iter().skip(1).take(2).cloned().collect();
                    outros.extend(m.alternativos(&primeira, alvo));
                    let mut vistos = HashSet::new();
                    outros.retain(|o| vistos.insert(chave(c, &o.pick)));
                    outros.truncate(2);
                    return Some(Resolvido {
                        sel: primeira,
                        outros,
                        m,
                        rel: Vec::new(),
                        teto_usado: Some(t),
                        alvo,
                    });
                }
            }
            if t >= t_max - EPS {
                break;
            }
            let prox = base
                .iter()
                .filter(|&&i| c[i].lvl > t * l + 1e-6)
                .fold(f64::INFINITY, |m, &i| js_min(m, c[i].lvl / l));
            if !prox.is_finite() {
                break;
            }
            t = t_max.min(js_max(
                js::round((t + TETO_PASSO) * 10.0) / 10.0,
                (prox * 10.0 - EPS).ceil() / 10.0,
            ));
        }
        for (estresse, heroi, rel) in [
            (false, true, vec!["estresse"]),
            (false, false, vec!["estresse", "herói"]),
        ] {
            let m = Busca::montar(
                c,
                n,
                l,
                referencia,
                base.clone(),
                Regra {
                    mult: teto,
                    heroi,
                    estresse,
                },
            );
            let sel = m.minimo_todos(alvo);
            if let Some(primeira) = sel.first().cloned() {
                let mut outros: Vec<Equipe> = sel.iter().skip(1).take(2).cloned().collect();
                outros.extend(m.alternativos(&primeira, alvo));
                outros.truncate(2);
                return Some(Resolvido {
                    sel: primeira,
                    outros,
                    m,
                    rel,
                    teto_usado: None,
                    alvo,
                });
            }
        }
        None
    };

    let mut e = if op.so_max {
        None
    } else {
        resolver(&pool_todos, 1.05, Perfil::Eco)
    };
    let margem_eco = e.as_ref().map_or(1.05, |r| r.sel.d / referencia);
    let mut eq_alvo = js_max(1.3, margem_eco * 1.1);
    let mut q = if op.so_max || op.so_eco {
        None
    } else {
        resolver(&pool_justo, eq_alvo, Perfil::Eq)
    };
    if q.is_none() && !op.so_max && !op.so_eco {
        eq_alvo = 1.05;
        q = resolver(&pool_justo, 1.05, Perfil::Eq);
    }
    // A econômica nunca custa mais que a equilibrada (o legado copia os campos da equilibrada
    // para a econômica, mas mantém o alvo dela).
    if let (Some(er), Some(qr)) = (e.as_mut(), q.as_ref()) {
        if qr.sel.total < er.sel.total - EPS {
            er.sel = qr.sel.clone();
            er.outros = qr.outros.clone();
            er.m = Busca {
                c,
                n,
                l,
                referencia,
                pool: qr.m.pool.clone(),
                reg: qr.m.reg,
                seeds: Vec::new(),
                maximum: None,
            };
            er.rel = qr.rel.clone();
            er.teto_usado = qr.teto_usado;
        }
    }
    let eco_sel = e.as_ref().map(|r| &r.sel);
    let eq_sel = q.as_ref().map(|r| &r.sel);
    if let Some(eco) = eco_sel {
        if eco.d / referencia > 1.15 {
            avisos.push(format!(
                "Econômica: a opção mais barata que cumpre as regras já entrega {}% da meta (acima de 115%): não há equipe mais barata com margem menor.",
                js::numero(js::round(eco.d / referencia * 100.0))
            ));
        }
    }
    if let (Some(a), Some(b)) = (eq_sel, eco_sel) {
        if chave(c, &a.pick) == chave(c, &b.pick) {
            avisos.push("Boss fraco para as ofertas disponíveis: a equipe econômica já cumpre a margem da equilibrada, por isso as duas coincidem.".into());
        }
    }
    for (p, r) in [(Perfil::Eco, &e), (Perfil::Eq, &q)] {
        if let Some(r) = r.as_ref().filter(|r| !r.rel.is_empty()) {
            avisos.push(format!(
                "{}: nenhuma equipe cumpriu todas as regras de segurança; relaxei {}.",
                p.nome(),
                r.rel.join(", ")
            ));
        }
    }
    let mm = (pool_justo.len() >= n)
        .then(|| Busca::montar(c, n, l, referencia, pool_justo.clone(), Regra::default()));
    let max_eq = mm.as_ref().and_then(|m| m.maximum.clone());

    let pack =
        |r: Option<&Equipe>, alvo: Option<f64>, teto: Option<f64>, rel: Vec<&'static str>| {
            empacotar(c, l, referencia, r, alvo, teto, rel)
        };

    let mut res_eco = pack(
        eco_sel,
        Some(1.05),
        Perfil::Eco.teto_mult(),
        e.as_ref().map(|r| r.rel.clone()).unwrap_or_default(),
    );
    let mut res_eq = pack(
        eq_sel,
        Some(eq_alvo),
        Perfil::Eq.teto_mult(),
        q.as_ref().map(|r| r.rel.clone()).unwrap_or_default(),
    );
    let mut res_max = pack(max_eq.as_ref(), None, None, Vec::new());
    if !res_eq.indisponivel && res_eq.poder > res_max.poder {
        if let (Some(mm), Some(eqp)) = (&mm, &res_eq.equipe) {
            let melhorada = mm.improve(&eqp.pick, None);
            res_max = pack(Some(&melhorada), None, None, Vec::new());
        }
    }
    for (p, r, res) in [
        (Perfil::Eco, &e, &mut res_eco),
        (Perfil::Eq, &q, &mut res_eq),
    ] {
        let Some(r) = r else { continue };
        if res.indisponivel {
            continue;
        }
        res.teto_usado = r.teto_usado;
        if let Some(restr) = res.restr.as_mut() {
            let teto = r.teto_usado.unwrap_or(f64::INFINITY);
            restr.teto_nivel = r.sel.pick.iter().all(|&i| c[i].lvl <= teto * l + 1e-6);
        }
        match r.teto_usado {
            Some(t) if t > TETO_NIVEL + EPS => avisos.push(format!(
                "{}: teto de nível subiu para {}×{}",
                p.nome(),
                js::to_fixed(t, 1),
                if t > TETO_AVISO {
                    " — mercado sem opções na faixa."
                } else {
                    "."
                }
            )),
            Some(_) => {}
            None => avisos.push(format!(
                "{}: sem teto de nível (subida do teto esgotada).",
                p.nome()
            )),
        }
        res.alternativas = r
            .outros
            .iter()
            .map(|o| pack(Some(o), Some(r.alvo), p.teto_mult(), r.rel.clone()))
            .collect();
        // A busca guardada pode ser a da equilibrada (quando ela substituiu a econômica).
        let busca = Busca {
            c,
            n,
            l,
            referencia,
            pool: r.m.pool.clone(),
            reg: r.m.reg,
            seeds: Vec::new(),
            maximum: None,
        };
        res.reservas = busca.reservas(&r.sel.pick, r.alvo);
    }
    if impossivel && !op.so_max {
        avisos.push("Boss impossível com as ofertas atuais: nem a melhor equipe possível por dano chega a 105% da meta.".into());
    }
    if res_eco.indisponivel && res_eq.indisponivel {
        avisos.push(format!(
            "Nenhuma equipe das ofertas atuais alcança a margem mínima de 105% contra este boss (nível {}), com {n} Pokémon: faltam ofertas de nível mais alto.",
            js::numero(l)
        ));
    }
    if !res_max.indisponivel && res_max.poder < 1.0 {
        res_max.inviavel = true;
    }
    for (p, r) in [(Perfil::Eco, &res_eco), (Perfil::Eq, &res_eq)] {
        if r.indisponivel || r.abaixo == 0 {
            continue;
        }
        let mult = r.equipe.as_ref().map_or(1.0, |e| e.mult);
        avisos.push(format!(
            "{}: {} slot(s) abaixo do nível do boss (pior: faltam {} níveis), dano recebido ×{}; margem de {}% (sob estresse {}%) já considera essa penalidade (estimativa, sem validação em luta).",
            p.nome(),
            r.abaixo,
            js::numero(r.pior_faltam),
            js::to_fixed(mult, 1),
            js::numero(js::round(r.poder * 100.0)),
            js::numero(js::round(r.margem_estresse * 100.0))
        ));
    }
    Selecao {
        eco: res_eco,
        eq: res_eq,
        max: res_max,
        avisos,
        elegiveis: pool_justo.len(),
        excluidas_preco: c
            .iter()
            .filter(|x| x.justo.is_finite() && x.unit > x.justo)
            .count(),
        sem_avaliacao: c.iter().filter(|x| !x.justo.is_finite()).count(),
        impossivel,
    }
}

// ---------- seleção exata ----------

/// Equipe de menor custo com dano ≥ `alvo_d` que cumpre as regras de `b.reg` (busca
/// exata por branch-and-bound). As cartas vão em ordem crescente de preço: um ramo é podado
/// quando nem as próximas cartas mais baratas cabem abaixo do melhor custo, ou quando nem o
/// teto de dano alcança a meta.
fn minimo_custo(b: &Busca<'_>, alvo_d: f64, limite_nos: u64) -> (Option<Equipe>, bool) {
    let c = b.c;
    let n = b.n;
    let mut ordem = b.pool.clone();
    ordem.sort_by(|&x, &y| c[x].unit.total_cmp(&c[y].unit).then(c[x].id.cmp(&c[y].id)));
    // Piso de custo: soma das `r` próximas cartas mais baratas (contíguas, pela ordem).
    let mut acumulado = vec![0.0; ordem.len() + 1];
    for (k, &i) in ordem.iter().enumerate() {
        acumulado[k + 1] = acumulado[k] + c[i].unit;
    }
    // Teto de dano: os `r` maiores `dano(·, 1)` entre as cartas a partir da posição `k`.
    let mut topo_suf = vec![vec![0.0; n + 1]; ordem.len() + 1];
    let mut maiores: Vec<f64> = Vec::with_capacity(n + 1);
    for k in (0..ordem.len()).rev() {
        let v = dano(&c[ordem[k]], 1.0);
        let pos = maiores.iter().position(|&x| v > x).unwrap_or(maiores.len());
        maiores.insert(pos, v);
        maiores.truncate(n);
        let mut soma = 0.0;
        for (r, alvo) in topo_suf[k].iter_mut().enumerate().skip(1) {
            soma += maiores.get(r - 1).copied().unwrap_or(0.0);
            *alvo = soma;
        }
    }

    struct Ctx<'a, 'b> {
        b: &'a Busca<'b>,
        ordem: Vec<usize>,
        acumulado: Vec<f64>,
        topo_suf: Vec<Vec<f64>>,
        alvo: f64,
        nos: u64,
        limite: u64,
        esgotado: bool,
        melhor: Option<Equipe>,
    }
    fn dfs(x: &mut Ctx<'_, '_>, k0: usize, pick: &mut Vec<usize>, gasto: f64, forca: f64) {
        let b = x.b;
        let r = b.n - pick.len();
        if r == 0 {
            let m = b.metrics(pick);
            // Mesmo custo: fica a de mais dano, como na heurística.
            let melhora = x.melhor.as_ref().is_none_or(|e| {
                m.total < e.total - EPS || ((m.total - e.total).abs() <= EPS && m.d > e.d + EPS)
            });
            if melhora && b.valid(pick) && m.d + EPS >= x.alvo && b.viavel(&m) {
                x.melhor = Some(m);
            }
            return;
        }
        let m_min = js::pow(3.0, (b.n as f64 - forca - r as f64).max(0.0));
        let parcial: f64 = pick.iter().map(|&i| dano(&b.c[i], m_min)).sum();
        for k in k0..x.ordem.len() {
            if x.ordem.len() - k < r || parcial + x.topo_suf[k][r] + EPS < x.alvo {
                break;
            }
            let piso = x.acumulado[k + r] - x.acumulado[k];
            if x.melhor
                .as_ref()
                .is_some_and(|e| gasto + piso > e.total + EPS)
            {
                break;
            }
            if x.nos >= x.limite {
                x.esgotado = true;
                break;
            }
            let i = x.ordem[k];
            let ci = &b.c[i];
            if pick.iter().any(|&y| b.c[y].id == ci.id) || conta_sp(b.c, pick, ci.sp) >= 2 {
                continue;
            }
            x.nos += 1;
            pick.push(i);
            dfs(
                x,
                k + 1,
                pick,
                gasto + ci.unit,
                forca + js_min(1.0, ci.lvl / b.l),
            );
            pick.pop();
        }
    }
    let mut x = Ctx {
        b,
        ordem,
        acumulado,
        topo_suf,
        alvo: alvo_d,
        nos: 0,
        limite: limite_nos,
        esgotado: false,
        melhor: None,
    };
    let mut pick = Vec::with_capacity(b.n);
    dfs(&mut x, 0, &mut pick, 0.0, 0.0);
    (x.melhor, x.esgotado)
}

/// `selecionar` com busca exata nos três perfis (modo `NEXUS_MELHORIAS`): a Econômica e a
/// Equilibrada são a equipe mais barata que cumpre a meta e as regras; a Máxima é a de maior
/// dano. Mesmas regras, mesma ordem de relaxamento e mesmo formato de saída do legado, mas
/// com resultado ótimo e sem depender do relógio.
#[allow(clippy::too_many_lines, clippy::type_complexity)]
pub fn selecionar_exato(c: &[Candidato], op: &Opcoes, limite_nos: u64) -> Selecao {
    let (n, l, referencia) = (op.n, op.nivel_boss, op.referencia);
    // Cartas com número não finito ficam de fora: NaN nas chaves de ordenação quebraria o
    // `sort` (e não há equipe útil com elas).
    if !(l.is_finite() && l > 0.0) {
        return selecao_vazia(c);
    }
    let pool_todos: Vec<usize> = (0..c.len())
        .filter(|&i| {
            [c[i].lvl, c[i].hp, c[i].dps_out, c[i].dps_in]
                .iter()
                .all(|v| v.is_finite())
        })
        .filter(|&i| c[i].unit.is_finite() && c[i].unit > 0.0)
        .collect();
    let pool_justo: Vec<usize> = pool_todos
        .iter()
        .copied()
        .filter(|&i| {
            !op.teto_justo
                || !op.moeda_orb
                || (c[i].justo.is_finite() && c[i].justo > 0.0 && c[i].unit <= c[i].justo)
        })
        .collect();
    let busca = |pool: Vec<usize>, reg: Regra| Busca {
        c,
        n,
        l,
        referencia,
        pool,
        reg,
        seeds: Vec::new(),
        maximum: None,
    };
    let melhor_d = |pool: &[usize]| -> Option<Equipe> {
        let sub: Vec<Candidato> = pool.iter().map(|&i| c[i].clone()).collect();
        let r = crate::busca_exata::melhor_equipe(&sub, n, l, None, None, limite_nos)?;
        let pick: Vec<usize> = r.pick.iter().map(|&j| pool[j]).collect();
        Some(busca(pool.to_vec(), Regra::default()).metrics(&pick))
    };
    let impossivel = melhor_d(&pool_todos).is_none_or(|m| m.d < 1.05 * referencia);
    let mut avisos: Vec<String> = Vec::new();

    // Escada de relaxamento: teto de nível 2,5×, sem teto, sem estresse, sem herói.
    let resolver = |pool_base: &[usize],
                    alvo: f64,
                    perfil: Perfil|
     -> Option<(Equipe, Busca<'_>, Vec<&'static str>, Option<f64>)> {
        if impossivel {
            return None;
        }
        let teto = perfil.teto_mult();
        let base: Vec<usize> = pool_base
            .iter()
            .copied()
            .filter(|&i| c[i].lvl >= PISO_NIVEL * l)
            .collect();
        let no_teto: Vec<usize> = base
            .iter()
            .copied()
            .filter(|&i| c[i].lvl <= TETO_NIVEL * l + 1e-6)
            .collect();
        let completa = Regra {
            mult: teto,
            heroi: true,
            estresse: true,
        };
        let tentar = |pool: Vec<usize>, reg: Regra| {
            let b = busca(pool, reg);
            let (e, esgotado) = minimo_custo(&b, alvo * referencia, limite_nos);
            (e, esgotado, b)
        };
        // Teto de nível: 2,5×; se não fecha, o menor teto (pelos níveis do pool) que fecha,
        // até 40× ou o maior nível. Mais cartas só aumentam o que é viável, então dá para
        // achar o menor teto por busca binária.
        let lvl_max = base.iter().fold(0.0, |m, &i| js_max(m, c[i].lvl));
        let t_max = TETO_SEGURANCA.min(js_max(TETO_NIVEL, lvl_max / l));
        let ate = |t: f64| -> Vec<usize> {
            base.iter()
                .copied()
                .filter(|&i| c[i].lvl <= t * l + 1e-6)
                .collect()
        };
        if no_teto.len() >= n {
            let (e, esgotado, b) = tentar(no_teto.clone(), completa);
            if let Some(e) = e {
                return Some((e, b, vec![], Some(TETO_NIVEL)));
            }
            if esgotado {
                return None;
            }
        }
        let mut tetos: Vec<f64> = base
            .iter()
            .map(|&i| (c[i].lvl / l * 10.0 - EPS).ceil() / 10.0)
            .filter(|t| *t > TETO_NIVEL && *t <= t_max + EPS)
            .collect();
        tetos.sort_by(f64::total_cmp);
        tetos.dedup();
        if let Some(&maior) = tetos.last() {
            let (e, esgotado, _) = tentar(ate(maior), completa);
            if esgotado {
                return None;
            }
            if e.is_some() {
                let (mut lo, mut hi) = (0usize, tetos.len() - 1);
                while lo < hi {
                    let meio = (lo + hi) / 2;
                    let pool = ate(tetos[meio]);
                    if pool.len() >= n && tentar(pool, completa).0.is_some() {
                        hi = meio;
                    } else {
                        lo = meio + 1;
                    }
                }
                let (e, _, b) = tentar(ate(tetos[lo]), completa);
                if let Some(e) = e {
                    return Some((e, b, vec![], Some(tetos[lo])));
                }
            }
        }
        // Sem teto de nível, relaxando estresse e depois herói.
        for (reg, rel) in [
            (completa, vec![]),
            (
                Regra {
                    mult: teto,
                    heroi: true,
                    estresse: false,
                },
                vec!["estresse"],
            ),
            (
                Regra {
                    mult: teto,
                    heroi: false,
                    estresse: false,
                },
                vec!["estresse", "herói"],
            ),
        ] {
            if base.len() < n {
                break;
            }
            let (e, esgotado, b) = tentar(base.clone(), reg);
            if let Some(e) = e {
                return Some((e, b, rel, None));
            }
            if esgotado {
                return None;
            }
        }
        None
    };
    let eco = if op.so_max {
        None
    } else {
        resolver(&pool_todos, 1.05, Perfil::Eco)
    };
    let margem_eco = eco.as_ref().map_or(1.05, |r| r.0.d / referencia);
    let mut eq_alvo = js_max(1.3, margem_eco * 1.1);
    let mut eq = if op.so_max || op.so_eco {
        None
    } else {
        resolver(&pool_justo, eq_alvo, Perfil::Eq)
    };
    if eq.is_none() && !op.so_max && !op.so_eco {
        eq_alvo = 1.05;
        eq = resolver(&pool_justo, 1.05, Perfil::Eq);
    }
    let maxima = if pool_justo.len() >= n {
        melhor_d(&pool_justo)
    } else {
        None
    };

    let montar = |r: &Option<(Equipe, Busca<'_>, Vec<&'static str>, Option<f64>)>,
                  perfil: Perfil,
                  alvo: f64,
                  avisos: &mut Vec<String>|
     -> ResultadoPerfil {
        let Some((e, b, rel, teto_usado)) = r else {
            return empacotar(
                c,
                l,
                referencia,
                None,
                Some(alvo),
                perfil.teto_mult(),
                Vec::new(),
            );
        };
        let mut p = empacotar(
            c,
            l,
            referencia,
            Some(e),
            Some(alvo),
            perfil.teto_mult(),
            rel.clone(),
        );
        p.teto_usado = *teto_usado;
        if let Some(restr) = p.restr.as_mut() {
            let t = teto_usado.unwrap_or(f64::INFINITY);
            restr.teto_nivel = e.pick.iter().all(|&i| c[i].lvl <= t * l + 1e-6);
        }
        if !rel.is_empty() {
            avisos.push(format!(
                "{}: nenhuma equipe cumpriu todas as regras de segurança; relaxei {}.",
                perfil.nome(),
                rel.join(", ")
            ));
        }
        match teto_usado {
            None => avisos.push(format!(
                "{}: sem teto de nível (nenhuma equipe viável até {}× o nível do boss).",
                perfil.nome(),
                js::numero(TETO_SEGURANCA)
            )),
            Some(t) if *t > TETO_NIVEL + EPS => avisos.push(format!(
                "{}: teto de nível subiu para {}×{}",
                perfil.nome(),
                js::to_fixed(*t, 1),
                if *t > TETO_AVISO {
                    " — mercado sem opções na faixa."
                } else {
                    "."
                }
            )),
            Some(_) => {}
        }
        // Planos B e C: a melhor equipe sem cada titular, as duas mais baratas e distintas.
        let mut vistos = HashSet::from([chave(c, &e.pick)]);
        let mut alt: Vec<Equipe> = e
            .pick
            .iter()
            .filter_map(|&tit| {
                let pool: Vec<usize> = b
                    .pool
                    .iter()
                    .copied()
                    .filter(|&x| c[x].id != c[tit].id)
                    .collect();
                minimo_custo(&busca(pool, b.reg), alvo * referencia, limite_nos).0
            })
            .filter(|x| vistos.insert(chave(c, &x.pick)))
            .collect();
        alt.sort_by(|a, z| ordem(ou(a.total - z.total, z.d - a.d)));
        p.alternativas = alt
            .iter()
            .take(2)
            .map(|x| {
                empacotar(
                    c,
                    l,
                    referencia,
                    Some(x),
                    Some(alvo),
                    perfil.teto_mult(),
                    rel.clone(),
                )
            })
            .collect();
        p.reservas = b.reservas(&e.pick, alvo);
        p
    };
    let res_eco = montar(&eco, Perfil::Eco, 1.05, &mut avisos);
    let res_eq = montar(&eq, Perfil::Eq, eq_alvo, &mut avisos);
    let mut res_max = empacotar(c, l, referencia, maxima.as_ref(), None, None, Vec::new());
    if !res_max.indisponivel && res_max.poder < 1.0 {
        res_max.inviavel = true;
    }
    if let (Some(a), Some(z)) = (&eco, &eq) {
        if chave(c, &a.0.pick) == chave(c, &z.0.pick) {
            avisos.push("Boss fraco para as ofertas disponíveis: a equipe econômica já cumpre a margem da equilibrada, por isso as duas coincidem.".into());
        }
    }
    if impossivel && !op.so_max {
        avisos.push("Boss impossível com as ofertas atuais: nem a melhor equipe possível por dano chega a 105% da meta.".into());
    }
    if res_eco.indisponivel && res_eq.indisponivel {
        avisos.push(format!(
            "Nenhuma equipe das ofertas atuais alcança a margem mínima de 105% contra este boss (nível {}), com {n} Pokémon: faltam ofertas de nível mais alto.",
            js::numero(l)
        ));
    }
    Selecao {
        eco: res_eco,
        eq: res_eq,
        max: res_max,
        avisos,
        elegiveis: pool_justo.len(),
        excluidas_preco: c
            .iter()
            .filter(|x| x.justo.is_finite() && x.unit > x.justo)
            .count(),
        sem_avaliacao: c.iter().filter(|x| !x.justo.is_finite()).count(),
        impossivel,
    }
}

/// Chance de a equipe passar da meta de dano, com a variação de dano do jogo (cada golpe
/// sai entre 85% e 100% do dano cheio; o modelo usa a média, 92,5%). Amostras com o gerador
/// determinístico do duelo oficial (LCG 1664525/1013904223): mesma entrada, mesma chance.
pub fn chance_vitoria(
    c: &[Candidato],
    pick: &[usize],
    n: usize,
    nivel_boss: f64,
    referencia: f64,
    amostras: u32,
) -> f64 {
    if pick.is_empty() || amostras == 0 {
        return 0.0;
    }
    let forca = pick
        .iter()
        .fold(0.0, |s, &i| s + js_min(1.0, c[i].lvl / nivel_boss));
    let mult = js::pow(3.0, js_max(0.0, n as f64 - forca));
    let mut semente: u32 = 0x5EED;
    let mut variacao = || {
        semente = semente.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (0.85 + 0.15 * (f64::from(semente) / 4_294_967_296.0)) / 0.925
    };
    let mut vitorias = 0u32;
    for _ in 0..amostras {
        let d: f64 = pick
            .iter()
            .map(|&i| {
                let x = &c[i];
                let (causado, recebido) = (variacao(), variacao());
                let t = if x.dps_in > 0.0 {
                    js_min(TEMPO_MAXIMO, x.hp / (x.dps_in * mult * recebido))
                } else {
                    TEMPO_MAXIMO
                };
                x.dps_out * causado * t
            })
            .sum();
        if d >= referencia {
            vitorias += 1;
        }
    }
    f64::from(vitorias) / f64::from(amostras)
}
