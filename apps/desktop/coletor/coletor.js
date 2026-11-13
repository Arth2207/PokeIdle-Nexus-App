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

  // --- WebSocket do jogo: guarda a conexão (para comandos) e repassa welcome/batalha.
  var OrigWS = window.WebSocket;
  var repassa = { welcome: 1, batalha: 1 };
  function Hook(url, protocolos) {
    var ws = protocolos ? new OrigWS(url, protocolos) : new OrigWS(url);
    if (typeof url === 'string' && url.indexOf('127.0.0.1') < 0 && url.indexOf('localhost') < 0) {
      window.__hdxWs = ws;
      ws.addEventListener('message', function (ev) {
        if (typeof ev.data !== 'string') return;
        try {
          var m = JSON.parse(ev.data);
          if (m && repassa[m.t]) enviar(m);
        } catch (e) {}
      });
    }
    return ws;
  }
  Object.assign(Hook, OrigWS);
  Hook.prototype = OrigWS.prototype;
  window.WebSocket = Hook;

  // --- Comando: manda JSON no WS do jogo (mesmo contrato do __hdxCmd legado).
  function cmd(o) {
    var s = window.__hdxWs;
    if (!s || s.readyState !== 1) return 'sem-conexao';
    try { s.send(JSON.stringify(o)); } catch (e) { return 'erro:' + String(e).slice(0, 80); }
    return 'ok';
  }
  window.__hdxCmd = cmd;
  window.__nexus = {
    executar: function (id, o) {
      var r;
      try { r = String(cmd(o)); } catch (e) { r = 'erro:' + String(e).slice(0, 80); }
      invocar('coletor_resultado', { id: id, resultado: r.slice(0, 200) });
    }
  };

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
})();
