// Testa o coletor real numa página simulada, sem conexão nem comandos para o jogo.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');
const chamadas = [], timers = new Map(), intervalos = [];
let proximo = 0;
class WS {
  constructor(url) { this.url = url; this.readyState = 1; this.listeners = {}; this.enviados = []; }
  addEventListener(t, f) { (this.listeners[t] ||= []).push(f); }
  send(d) { this.enviados.push(JSON.parse(d)); }
  receber(m) { for (const f of this.listeners.message || []) f({ data: JSON.stringify(m) }); }
}
class XHR { open() {} send() {} }
const window = { WebSocket: WS, __TAURI_INTERNALS__: { invoke(c, a) { chamadas.push([c, a]); return null; } } };
window.top = window;
const sandbox = {
  window, document: { readyState: 'loading', addEventListener() {}, getElementById() { return null; } },
  location: { host: 'pokeidle.io', href: 'https://pokeidle.io/app' }, URL, XMLHttpRequest: XHR,
  setTimeout(f) { const id = ++proximo; timers.set(id, f); return id; },
  clearTimeout(id) { timers.delete(id); }, setInterval(f, ms) { intervalos.push([f, ms]); },
};
vm.runInNewContext(fs.readFileSync(path.join(__dirname, '../../apps/desktop/coletor/coletor.js'), 'utf8'), sandbox);
const ws = new window.WebSocket('wss://pokeidle.io/ws');
ws.receber({ t: 'welcome', hunts: [{slug:'furious_scizor',area:'outland'},{slug:'ancient_pupitar',area:'outland'}], outlandTiers: [{ tier: 104, numero: 4, nivel: 1000, mult: 1.25, rotulo: 'Outland 4' }], chanceBossToken: 0.0001,
  estado: { huntSlug: 'furious_scizor', outlandTier: 104, activeId: 7, pokemons: [{ id: 7, level: 1000 }] } });
ws.receber({ t: 'batalha', ev: [] });
const batalha = chamadas.find(([c, a]) => c === 'coletor_evento' && a.msg.t === 'batalha')[1].msg;
assert.equal(batalha.contexto.outland_tier, 104);
assert.equal(batalha.contexto.ativo_nivel, 1000);
assert.match(batalha.contexto.regras_id, /^outland:/);
window.__nexus.executar(1, { t: 'hunt.select', slug: 'ancient_pupitar' });
assert.equal(chamadas.filter(([c]) => c === 'coletor_resultado').length, 0, 'enviar no WS não confirma a troca');
ws.receber({ t: 'estado', estado: { huntSlug: 'ancient_pupitar' } });
assert.equal(chamadas.find(([c,a]) => c === 'coletor_resultado' && a.id === 1)[1].resultado, 'ok');
window.__nexus.executar(2, { t: 'hunt.select', slug: 'furious_scizor' });
ws.receber({t:'estado',estado:{huntSlug:'furious_scizor',outlandTier:101}});
assert.equal(chamadas.some(([c,a])=>c==='coletor_resultado'&&a.id===2),false,'tier alterado não confirma sucesso');
for (const f of [...timers.values()]) f();
assert.match(chamadas.find(([c,a]) => c === 'coletor_resultado' && a.id === 2)[1].resultado, /^erro:/);
intervalos.find(([,ms]) => ms === 15000)[0]();
assert.equal(chamadas.find(([c,a]) => c === 'coletor_evento' && a.msg.t === 'hunt.contexto')[1].msg.contexto.outland_tier, 101);
assert.equal(ws.enviados.length, 2, 'o coletor não envia pedidos extras ao jogo');
console.log('OK: contexto, catálogo, preservação de tier, confirmação de hunt pelo estado e timeout.');
