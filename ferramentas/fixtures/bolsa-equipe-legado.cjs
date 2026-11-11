'use strict';
/**
 * Fixtures das peças puras da equipe da bolsa: tabela de bosses da wiki, listas de alvos por
 * perfil e lucro por boss, rodando o código do legado. Só lê.
 *
 * Uso: node ferramentas/fixtures/bolsa-equipe-legado.cjs <raiz do legado> > crates/nucleo/tests/fixtures/bolsa-equipe-legado.json
 */
const path = require('node:path');
const fs = require('node:fs');
const raiz = process.argv[2];
if (!raiz) { console.error('uso: bolsa-equipe-legado.cjs <raiz do legado>'); process.exit(2); }
const C = require(path.join(raiz, 'equipes-core.js'));
const WIKI = require(path.join(raiz, 'bolsa-wiki-bosses.js'));
const ALVOS = require(path.join(raiz, 'bolsa-alvos.js'));
const LUCRO = require(path.join(raiz, 'bolsa-lucro.js'));

let semente = 777;
const rnd = () => { semente = (Math.imul(semente, 1664525) + 1013904223) >>> 0; return semente / 4294967296; };
const sorteia = a => a[Math.floor(rnd() * a.length)];

const wikiTexto = fs.readFileSync(path.join(raiz, 'data/POKEIDLE-wiki.txt'), 'utf8');
const wiki = WIKI.parse(wikiTexto);
const secao = wikiTexto.slice(wikiTexto.indexOf('## Bosses'), wikiTexto.indexOf('\n## ', wikiTexto.indexOf('## Bosses') + 5));

const bosses = C.listaBosses([]);
const listas = [];
for (const boss of bosses.slice(0, 10)) {
  const precos = new Map();
  for (const sp of C.DEX) if (rnd() < 0.4) precos.set(sp.d, Math.round(50 + rnd() * 5000));
  const extras = { maxima: 0, equilibrada: sorteia([0, 5, 10]), economica: sorteia([0, 5, 20]) };
  listas.push({ boss: { key: boss.key, nome: boss.nome, teamLevel: boss.teamLevel, tipos: boss.tipos }, precos: [...precos], extras, saida: ALVOS.listasDoBoss(boss, precos, extras) });
}

const lucros = [];
for (let i = 0; i < 60; i++) {
  const w = sorteia(wiki), precos = new Map();
  const slots = Array.from({ length: sorteia([0, 3, 5]) }, () => ({ dpsOut: rnd() * 500, justo: sorteia([undefined, 0, rnd() * 9000]), preco: sorteia([undefined, rnd() * 9000]) }));
  const perfil = { indisponivel: rnd() < 0.1, slots, total: sorteia([undefined, rnd() * 50000]), poder: rnd() * 3, margemEstresse: sorteia([null, undefined, rnd() * 2, 0]) };
  const boss = { drops: sorteia([w.drops, null, [{ itemId: 1, nome: 'X', chance: 1 }]]), entrada: sorteia([w.entrada, null]) };
  const tab = {}; for (const d of [...(w.drops || []), w.entrada]) if (rnd() < 0.7) tab[d.nome] = { preco: rnd() < 0.2 ? 0 : rnd() * 3000, fonte: sorteia(['mediana 7d', 'último', 'menor anúncio']) };
  const pm = (id, nome) => tab[nome] || { preco: null };
  const bossHp = sorteia([0, Math.round(rnd() * 1e7)]);
  lucros.push({ perfil, bossHp, boss, tabela: tab, saida: LUCRO.lucroPerfil(perfil, bossHp, boss, pm) });
}
// Alvos de coleta com o estado real (nicks trocados só na memória deste processo).
const EST = require(path.join(raiz, 'equipes-estado.js'));
const E = EST.get(), contas = {};
Object.values(E.contas).forEach((c, i) => { contas[`conta-${i + 1}`] = c; });
E.contas = contas;
const alvos = [];
for (let i = 0; i < 4; i++) {
  const precos = new Map();
  for (const sp of C.DEX) if (rnd() < 0.5) precos.set(sp.d, Math.round(50 + rnd() * 5000));
  const lista = C.listaBosses(E.bosses);
  const faltas = i === 0 ? [] : lista.slice(0, 3).map(b => ({ boss: b.key, perfil: sorteia(['equilibrada', 'economica']), extras: sorteia([5, 10, 20]) }));
  const max = sorteia([null, '10', '60', 'x']);
  const saida = ALVOS.alvosColeta({ nick: null, max, precos, versaoPrecos: i, faltas, faltasSig: String(i) });
  alvos.push({ max, precos: [...precos], faltas, saida });
}
const estado = { bosses: E.bosses, bossesEm: E.bossesEm, contas: Object.fromEntries(Object.entries(contas).map(([k, c]) => [k, { trLv: c.trLv }])) };
process.stdout.write(JSON.stringify({ secao, wiki, listas, lucros, estado, alvos }));
