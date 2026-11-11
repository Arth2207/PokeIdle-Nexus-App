'use strict';
/**
 * Gera as fixtures do modelo de combate `Legado` rodando o próprio código do legado.
 * Só lê arquivos; não grava nada fora da saída padrão.
 *
 * Uso: node ferramentas/fixtures/combate-legado.cjs <raiz do legado> > crates/nucleo/tests/fixtures/combate-legado.json
 */
const path = require('node:path');
const fs = require('node:fs');

const raiz = process.argv[2];
if (!raiz) { console.error('uso: combate-legado.cjs <raiz do legado>'); process.exit(2); }
const C = require(path.join(raiz, 'equipes-core.js'));
const S = C.S;

// Gerador determinístico: as fixtures saem iguais a cada execução.
let semente = 20261111;
const rnd = () => { semente = (Math.imul(semente, 1664525) + 1013904223) >>> 0; return semente / 4294967296; };
const sorteia = a => a[Math.floor(rnd() * a.length)];
const inteiro = (min, max) => min + Math.floor(rnd() * (max - min + 1));

const usadas = new Map();
const usa = sp => { if (sp) usadas.set(sp.d, sp); return sp; };

const QUALIDADES = [1, 1.1, 1.2, 1.3, 1.45, 1.5, 1.6, 1.75, 2];
const especieSorteada = () => usa(sorteia(C.DEX));
const ivsSorteados = () => Array.from({ length: 6 }, () => inteiro(0, 31));

function descreve(m) {
  return {
    st: m.st, hp: m.hp, cdmul: m.cdmul, gap: m.gap, inv: m.inv, leg: m.leg, types: m.types,
    moves: m.moves.map(x => [x.n, x.p, x.cd, x.cat, x.t]),
  };
}
function specSorteada() {
  const sp = especieSorteada();
  const spec = { d: sp.d, lvl: sorteia([inteiro(1, 100), inteiro(100, 1200), rnd() * 900 + 1]), q: sorteia([...QUALIDADES, 1 + Math.round(rnd() * 100) / 100]),
    iv: ivsSorteados(), pot: inteiro(1, 5), shiny: rnd() < 0.2, wild: rnd() < 0.15, legend: sorteia([null, null, true, false]) };
  return spec;
}
const monDeSpec = s => S.makeMon(C.BYD.get(s.d), s.lvl, { q: s.q, iv: s.iv, pot: s.pot, shiny: s.shiny, wild: s.wild, legend: s.legend });

// 1. Efetividade: cada tipo de golpe contra cada defesa de um ou dois tipos.
const efetividade = [];
for (const mt of S.TYPES) {
  for (let i = 0; i < S.TYPES.length; i++) {
    efetividade.push([mt, [S.TYPES[i]], S.effRaw(mt, [S.TYPES[i]]), S.effHunt(mt, [S.TYPES[i]])]);
    for (let j = i + 1; j < S.TYPES.length; j++) {
      const d = [S.TYPES[i], S.TYPES[j]];
      efetividade.push([mt, d, S.effRaw(mt, d), S.effHunt(mt, d)]);
    }
  }
}

// 2. Combatentes (stats, HP, recargas, golpes liberados pelo nível).
const combatentes = [];
for (let i = 0; i < 400; i++) { const spec = specSorteada(); combatentes.push({ spec, mon: descreve(monDeSpec(spec)) }); }

// 3. Dano de cada golpe e da investida entre dois combatentes.
const danos = [];
for (let i = 0; i < 300; i++) {
  const a = specSorteada(), b = specSorteada(), ma = monDeSpec(a), mb = monDeSpec(b);
  danos.push({ att: a, def: b,
    golpes: ma.moves.map(m => S.dmg(ma, mb, m.p, m.cat, m.t, ma.types.includes(m.t) ? 1.5 : 1)),
    investida: S.invDmg(ma, mb) });
}

// 4. Bosses: tabela da wiki e lista viva do jogo.
const estado = JSON.parse(fs.readFileSync(path.join(raiz, 'data/equipes-estado.json'), 'utf8'));
const vivos = (estado.bosses || []).map(b => ({ key: b.key, nome: b.nome, tipos: b.tipos, teamLevel: b.teamLevel, level: b.level, minTr: b.minTr }));
const listaBosses = [...C.listaBosses([]), ...C.listaBosses(vivos)];
const bosses = listaBosses.map(b => {
  usa(C.BYN.get(String(b.nome).toLowerCase()));
  const bm = C.bossMon(b);
  return { boss: { key: b.key, nome: b.nome, teamLevel: b.teamLevel, tipos: b.tipos }, mon: bm ? descreve(bm.mon) : null };
});

// 5. Fichas reais da coleção (sem identificadores) e variações sintéticas.
const fichasReais = Object.values(estado.contas || {}).flatMap(c => c.pokemons || [])
  .map(({ id, slot, ...f }) => f).slice(0, 80);
const sinteticas = [];
for (let i = 0; i < 60; i++) {
  const sp = especieSorteada();
  const f = { speciesId: sorteia([sp.d, String(sp.d), undefined]), nome: sorteia([sp.n, undefined]), level: sorteia([inteiro(1, 1200), String(inteiro(1, 500)), 0, undefined]),
    quality: sorteia([1.3, '1.5', 0, null, undefined]), potencia: sorteia([1, 2.6, '4', 7, 0, undefined]), shiny: sorteia([true, false, 1, 0, 'true', undefined]) };
  if (rnd() < 0.7) f.ivs = { hp: inteiro(0, 31), atk: inteiro(0, 31), def: inteiro(0, 31), spAtk: inteiro(0, 31), spDef: sorteia([inteiro(0, 31), undefined]), spd: inteiro(0, 31), speed: inteiro(0, 31) };
  if (rnd() < 0.3) { f.stats = { hp: inteiro(0, 9000), atk: inteiro(1, 9000), def: inteiro(1, 9000), spAtk: inteiro(1, 9000), spDef: inteiro(1, 9000), speed: inteiro(1, 9000) }; f.maxHp = sorteia([inteiro(1, 99999), 0, undefined]); }
  sinteticas.push(JSON.parse(JSON.stringify(f)));
}
const fichas = [...fichasReais, ...sinteticas].map(f => {
  const c = C.monDeFicha(f); usa(c && c.sp);
  return { ficha: f, especie: c ? c.sp.d : null, mon: c ? descreve(c.mon) : null };
});

// 6. Duelos e avaliação contra bosses (teóricos, sorteados e fichas).
const bossesOk = listaBosses.filter(b => C.bossMon(b));
const duelos = [];
for (let i = 0; i < 400; i++) {
  const b = sorteia(bossesOk), foe = C.bossMon(b).mon;
  const tipo = sorteia(['teorico', 'spec', 'ficha']);
  let me, entrada;
  if (tipo === 'teorico') {
    const sp = especieSorteada(), perfil = sorteia(['ideal', 'realista']), lvl = sorteia([b.teamLevel, inteiro(1, b.teamLevel)]);
    me = S.makeMon(sp, lvl, perfil === 'ideal' ? { q: 1.5, pot: 5, iv: Array(6).fill(31) } : { q: 1.3, pot: 3, iv: Array(6).fill(31) });
    entrada = { tipo, d: sp.d, lvl, perfil };
  } else if (tipo === 'spec') {
    const spec = specSorteada(); me = monDeSpec(spec); entrada = { tipo, spec };
  } else {
    const f = sorteia(fichas.filter(x => x.mon)).ficha; me = C.monDeFicha(f).mon; entrada = { tipo, ficha: f };
  }
  const mult = sorteia([1, C.multIndividual(me.lvl, b.teamLevel), Math.pow(3, rnd() * 5)]);
  const r = C.avaliar(me, foe, mult);
  duelos.push({ boss: { key: b.key, nome: b.nome, teamLevel: b.teamLevel, tipos: b.tipos }, me: entrada, mult,
    saida: { D: r.D, dpsOut: r.dpsOut, dpsIn: r.dpsIn, tDie: r.tDie, bate: r.bate, apanha: r.apanha, imune: r.imune } });
}

// 7. Penalidade de equipe.
const penalidades = [];
for (let i = 0; i < 200; i++) {
  const team = sorteia(listaBosses).teamLevel, niveis = Array.from({ length: inteiro(0, 7) }, () => sorteia([inteiro(1, team * 1.5), team]));
  const p = C.multPenalidade(niveis, team), lv = inteiro(1, team * 1.2);
  penalidades.push({ niveis, team, forca: p.forca, mult: p.mult, lvl: lv, individual: C.multIndividual(lv, team) });
}

// 8. Math.pow (o Rust porta o pow do V8; a libc erra o último bit às vezes).
const potencias = [];
for (let i = 0; i < 3000; i++) {
  const x = sorteia([3, sorteia(QUALIDADES), rnd() * 4, rnd() * 1e6, 1 / (1 + rnd() * 1e3), -inteiro(1, 9), 0, 1e-310]);
  const y = sorteia([rnd() * 5, sorteia([0.95, 0.8]), inteiro(-5, 40), rnd() * 600 - 300, 0.5, 2, -1, -rnd() * 1100]);
  potencias.push([x, y, Math.pow(x, y)]);
}

const especies = [...usadas.values()].sort((a, b) => a.d - b.d);
process.stdout.write(JSON.stringify({ especies, efetividade, combatentes, danos, bosses, fichas, duelos, penalidades, potencias }));
