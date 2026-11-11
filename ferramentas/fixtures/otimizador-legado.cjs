'use strict';
/**
 * Fixtures do otimizador (`selecionar` do bolsa-equipe-selecao.js). Monta pools de cartas
 * avaliadas contra bosses reais pelo motor do legado e grava a entrada e a saída. Só lê.
 *
 * Uso: node ferramentas/fixtures/otimizador-legado.cjs <raiz do legado> > crates/nucleo/tests/fixtures/otimizador-legado.json
 */
const path = require('node:path');
const raiz = process.argv[2];
if (!raiz) { console.error('uso: otimizador-legado.cjs <raiz do legado>'); process.exit(2); }
const C = require(path.join(raiz, 'equipes-core.js'));
const SEL = require(path.join(raiz, 'bolsa-equipe-selecao.js'));

let semente = 4242;
const rnd = () => { semente = (Math.imul(semente, 1664525) + 1013904223) >>> 0; return semente / 4294967296; };
const sorteia = a => a[Math.floor(rnd() * a.length)];
const comprar = C.DEX.filter(sp => (sp.c === 'pokebola' || !sp.c) && !C.S.isLegend(sp));
const bosses = C.listaBosses([]).slice(0, 12);

function pool(boss, tam) {
  const foe = C.bossMon(boss).mon, out = [];
  for (let i = 0; out.length < tam && i < tam * 4; i++) {
    const sp = sorteia(comprar), lvl = Math.max(1, Math.round(boss.teamLevel * (0.4 + rnd() * 3)));
    const me = C.S.makeMon(sp, lvl, { q: 1 + Math.round(rnd() * 8) / 10, iv: Array.from({ length: 6 }, () => Math.floor(rnd() * 32)), pot: 1 + Math.floor(rnd() * 5) });
    const r = C.avaliar(me, foe, 1);
    if (!(r.D > 0)) continue;
    const unit = Math.round(lvl * (5 + rnd() * 40) * (rnd() < 0.05 ? 0 : 1));
    const justo = rnd() < 0.2 ? undefined : Math.round(unit * (0.6 + rnd() * 0.9));
    out.push({ id: String(100000 + out.length * 7 + Math.floor(rnd() * 5)), sp: sp.d, lvl, unit, justo, hp: me.hp, dpsOut: r.dpsOut, dpsIn: r.dpsIn });
  }
  return out;
}
const ids = p => p.map(x => x.id);
function projeta(r) {
  const perfil = p => ({ indisponivel: p.indisponivel, inviavel: p.inviavel, ids: ids(p.pick), total: p.total, poder: p.poder, forca: p.forca, mult: p.mult,
    danos: p.danos, D: p.D, margemEstresse: p.margemEstresse, heroiShare: p.heroiShare, abaixo: p.abaixo ?? 0, piorFaltam: p.piorFaltam ?? 0,
    tSobrevive: p.tSobrevive ?? null, restr: p.restr, tetoUsado: p.tetoUsado ?? null,
    alternativas: (p.alternativas || []).map(a => ({ plano: a.plano, ids: ids(a.pick), total: a.total, poder: a.poder, mult: a.mult, margemEstresse: a.margemEstresse })),
    reservas: (p.reservas || []).map(l => l.map(c => ({ id: c.x.id, margem: c.margem, total: c.total }))) });
  return { perfis: { eco: perfil(r.perfis.eco), eq: perfil(r.perfis.eq), max: perfil(r.perfis.max) }, avisos: r.avisos,
    elegiveis: r.elegiveis, excluidasPreco: r.excluidasPreco, semAvaliacao: r.semAvaliacao };
}

const casos = [];
for (let i = 0; i < 80; i++) {
  const boss = sorteia(bosses), cands = pool(boss, sorteia([4, 10, 25, 60, 150]));
  const ref = Math.max(1, C.equipeIdeal(boss, { perfil: 'realista', n: 5 })?.soma || 1) * sorteia([0.15, 0.3, 0.5, 1]);
  const opts = sorteia([{}, { moeda: 'gold' }, { soEco: true, tetoJusto: false }, { soMax: true }]);
  const op = { n: 5, nivelBoss: boss.teamLevel, referencia: ref, ...opts };
  const t0 = Date.now(), r = SEL.selecionar(cands, op), ms = Date.now() - t0;
  casos.push({ candidatos: cands, opcoes: op, saida: projeta(r), ms });
}
process.stdout.write(JSON.stringify({ casos }));
