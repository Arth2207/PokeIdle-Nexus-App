'use strict';
/**
 * Fixtures de /api/equipes/bosses e /api/equipes/plano, chamando as rotas do próprio legado
 * em memória. Só lê arquivos: o estado é alterado só na memória deste processo (o legado só
 * grava o arquivo no ingest, que não é chamado). Os nicks reais são trocados por nomes fixos.
 *
 * Uso: node ferramentas/fixtures/equipes-legado.cjs <raiz do legado> <saída>
 * Grava <saída>/equipes-legado.json e <saída>/pokedex.json.
 *
 * Cada cenário roda num processo separado: o cache do `equipeIdeal` do legado não leva a
 * fonte da lista na chave, e um cenário contaminaria o outro.
 */
const path = require('node:path');
const fs = require('node:fs');

const [raiz, saida] = process.argv.slice(2);
if (!raiz || !saida) { console.error('uso: equipes-legado.cjs <raiz do legado> <saída>'); process.exit(2); }
const EST = require(path.join(raiz, 'equipes-estado.js'));
const C = require(path.join(raiz, 'equipes-core.js'));

let semente = 99;
const rnd = () => { semente = (Math.imul(semente, 1664525) + 1013904223) >>> 0; return semente / 4294967296; };

const E = EST.get();
const reais = Object.values(E.contas);
const contas = {};
reais.forEach((c, i) => { contas[`conta-${i + 1}`] = JSON.parse(JSON.stringify(c)); });
// Conta sintética: nível baixo, alguns bosses vencidos e parte da coleção real.
const colecao = reais.flatMap(c => c.pokemons || []);
contas['conta-teste'] = { trLv: 700, vencidos: ['articuno', 'Moltres'], em: 1794000000000,
  pokemons: colecao.filter(() => rnd() < 0.3).map(p => ({ ...p, level: Math.max(1, Math.round((p.level || 1) * rnd())) })) };
contas['conta-vazia'] = { em: 1794000000001 };

async function chamar(rota, query) {
  let status = 200, resposta;
  const res = {};
  const send = (_, corpo, st) => { resposta = corpo; if (st) status = st; };
  const { rotas: r } = require(path.join(raiz, 'equipes.js'))(null, send, null);
  await r[`GET ${rota}`]({ url: `${rota}?${new URLSearchParams(query)}` }, res);
  return { rota, query, status, resposta: JSON.parse(JSON.stringify(resposta)) };
}

async function cenario(variante) {
  {
    const bossesOrig = E.bosses;
    E.contas = contas;
    if (variante === 'wiki') E.bosses = [];
    const lista = C.listaBosses(E.bosses);
    const chamadas = [];
    for (const nick of [undefined, 'conta-1', 'conta-teste', 'conta-vazia', 'inexistente']) {
      const q = nick === undefined ? {} : { nick };
      chamadas.push(await chamar('/api/equipes/bosses', q));
      chamadas.push(await chamar('/api/equipes/plano', q));
    }
    for (const b of lista) {
      for (const perfil of ['realista', 'ideal']) {
        const nick = rnd() < 0.5 ? 'conta-1' : 'conta-teste';
        const boss = rnd() < 0.2 ? b.nome.toUpperCase() : b.key;
        chamadas.push(await chamar('/api/equipes/plano', { nick, boss, perfil }));
      }
    }
    chamadas.push(await chamar('/api/equipes/plano', { boss: 'missingno' }));
    const r = { variante, estado: { bosses: E.bosses, bossesEm: E.bossesEm, contas }, chamadas };
    E.bosses = bossesOrig;
    return r;
  }
}

const variante = process.argv[4];
if (variante) {
  cenario(variante).then(r => process.stdout.write(JSON.stringify(r)));
} else {
  const { execFileSync } = require('node:child_process');
  const cenarios = ['jogo', 'wiki'].map(v =>
    JSON.parse(execFileSync(process.execPath, [__filename, raiz, saida, v], { maxBuffer: 1 << 28 })));
  fs.writeFileSync(path.join(saida, 'equipes-legado.json'), JSON.stringify({ cenarios }));
  fs.writeFileSync(path.join(saida, 'pokedex.json'), JSON.stringify(C.DEX));
}
