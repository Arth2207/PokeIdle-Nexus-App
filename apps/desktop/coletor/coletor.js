// Coletor do PokeIdle (JS de página, sem build). Injetado no início de cada janela de conta
// (initialization script do Tauri). Lê o que o jogo já mostra (WS do próprio cliente e HUD) e
// manda ao Rust por IPC; executa comandos pedidos pelo Rust. Não faz nenhuma requisição.
(function () {
  'use strict';
  if (window.top !== window || window.__nexus) return;

  function invocar(cmd, args) {
    try {
      var i = window.__TAURI_INTERNALS__;
      if (i && i.invoke) return i.invoke(cmd, args);
    } catch (e) {}
    return null;
  }
  function enviar(msg) { invocar('coletor_evento', { msg: msg }); }

  // --- WebSocket do jogo (mesmo gancho do legado): subclasse preserva WebSocket.OPEN etc.;
  // o send() também captura conexão criada antes do gancho. Só a conexão do próprio jogo.
  var OrigWS = window.WebSocket;
  var repassa = { welcome: 1, batalha: 1, estado: 1, pvp: 1, treino: 1, 'shiny.captura': 1, raidFichas: 1, pvpTabela: 1, perfil: 1, pvpMensal: 1 };
  var abasMercado = { vitrine: 1, item: 1, itens: 1, historicoGlobal: 1, medias: 1, historico: 1, historicoCompras: 1 };
  function doJogo(s) { try { return new URL(s.url).host === location.host; } catch (e) { return false; } }
  function ouvir(ws) {
    if (ws.__nexusOuvindo) return;
    ws.__nexusOuvindo = 1;
    window.__hdxWs = ws;
    ws.addEventListener('message', function (ev) {
      if (typeof ev.data !== 'string') return;
      vistoWs = Date.now();
      if (bruto) enviar({ t: 'bruto', dir: 'in', dados: ev.data.slice(0, 3145728) });
      try {
        var m = JSON.parse(ev.data);
        if (m && repassa[m.t]) enviar(m);
        // Respostas do mercado: a varredura do app (relay) depende delas.
        else if (m && m.t === 'market' && abasMercado[m.aba]) enviar({ t: 'mkt', m: m });
      } catch (e) {}
    });
  }
  var vistoWs = 0;
  // Modo Descoberta: repassa cada mensagem do WS do jogo (o app tira segredos e limita).
  var bruto = false;
  var envioOrig = OrigWS.prototype.send;
  OrigWS.prototype.send = function (d) {
    if (doJogo(this)) {
      ouvir(this);
      if (bruto && typeof d === 'string') enviar({ t: 'bruto', dir: 'out', dados: d.slice(0, 3145728) });
    }
    return envioOrig.apply(this, arguments);
  };
  try {
    window.WebSocket = class extends OrigWS {
      constructor(a, b) {
        super(a, b);
        if (doJogo(this)) ouvir(this);
      }
    };
  } catch (e) {}

  // --- HTTP do jogo (só no modo Descoberta): algumas telas (ex.: perfil de outro jogador)
  // vêm por fetch/XHR, não pelo WS. Só o próprio site, só JSON, sem a query e sem login.
  var naoHttp = /auth|login|logout|token|sess|senha|password|cadastro|registr/i;
  function caminhoJogo(u) {
    try {
      var x = new URL(u, location.href);
      if (x.host !== location.host || naoHttp.test(x.pathname)) return null;
      return x.pathname;
    } catch (e) { return null; }
  }
  function http(metodo, url, tipo, texto) {
    var p = caminhoJogo(url);
    if (!bruto || p === null || typeof texto !== 'string' || String(tipo).indexOf('json') < 0) return;
    var corpo;
    try { corpo = JSON.parse(texto.slice(0, 3000000)); } catch (e) { return; }
    var m = String(metodo || 'GET').toUpperCase();
    enviar({ t: 'bruto', dir: 'in', dados: JSON.stringify({ t: 'http ' + m + ' ' + p, corpo: corpo }) });
  }
  var fetchOrig = window.fetch;
  if (fetchOrig) {
    window.fetch = function (entrada, init) {
      var pr = fetchOrig.apply(this, arguments);
      if (bruto) {
        var url = typeof entrada === 'string' ? entrada : (entrada && entrada.url) || '';
        var met = (init && init.method) || (entrada && entrada.method) || 'GET';
        pr.then(function (r) {
          try {
            var ct = r.headers.get('content-type') || '';
            if (ct.indexOf('json') >= 0) r.clone().text().then(function (t) { http(met, url, ct, t); }, function () {});
          } catch (e) {}
        }, function () {});
      }
      return pr;
    };
  }
  var xAbrir = XMLHttpRequest.prototype.open, xEnviar = XMLHttpRequest.prototype.send;
  XMLHttpRequest.prototype.open = function (m, u) { this.__nx = [m, u]; return xAbrir.apply(this, arguments); };
  XMLHttpRequest.prototype.send = function () {
    var x = this;
    if (bruto && x.__nx) {
      x.addEventListener('load', function () {
        try { http(x.__nx[0], x.__nx[1], x.getResponseHeader('content-type') || '', x.responseText); } catch (e) {}
      });
    }
    return xEnviar.apply(this, arguments);
  };

  // --- Comando: manda JSON no WS do jogo (mesmo contrato do __hdxCmd legado).
  function cmd(o) {
    var s = window.__hdxWs;
    if (!s || s.readyState !== 1) return 'sem-conexao';
    try { s.send(JSON.stringify(o)); } catch (e) { return 'erro:' + String(e).slice(0, 80); }
    return 'ok';
  }
  window.__hdxCmd = cmd;
  window.__nexus = {
    bruto: function (v) { bruto = !!v; },
    executar: function (id, o) {
      var r;
      try { r = String(cmd(o)); } catch (e) { r = 'erro:' + String(e).slice(0, 80); }
      invocar('coletor_resultado', { id: id, resultado: r.slice(0, 200) });
    }
  };

  try {
    var pc = invocar('coletor_config', {});
    if (pc && pc.then) pc.then(function (r) { bruto = !!(r && r.bruto); }, function () {});
  } catch (e) {}

  // --- HUD: linhas novas do log (kill/captura são interpretadas no Rust).
  var seq = 0, buf = [];
  function observar() {
    var log = document.getElementById('hud-log');
    if (!log) { setTimeout(observar, 1500); return; }
    var vistos = new WeakSet(Array.prototype.slice.call(log.children));
    new MutationObserver(function (ms) {
      ms.forEach(function (m) {
        m.addedNodes.forEach(function (n) {
          if (n.nodeType !== 1 || vistos.has(n)) return;
          vistos.add(n);
          var t = (n.textContent || '').trim();
          if (t) buf.push([++seq, Date.now(), t]);
        });
      });
      if (buf.length > 500) buf.splice(0, buf.length - 500);
    }).observe(log, { childList: true });
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', observar);
  else observar();
  setInterval(function () {
    if (buf.length) enviar({ t: 'hud', linhas: buf.splice(0, buf.length) });
  }, 2000);

  // --- Sinal de vida (fica no app, não vai ao Nexus): conexão do jogo aberta e mensagem recente.
  function texto(id) { var e = document.getElementById(id); return e ? (e.textContent || '').trim() : ''; }
  setInterval(function () {
    var s = window.__hdxWs;
    enviar({ t: 'vivo', ws: !!(s && s.readyState === 1), msg_ms: vistoWs,
      hunt: (texto('hud-hunt') || texto('eco-onde')).slice(0, 80) });
  }, 15000);
})();
