// Janela "Tracker": o usuário navega À MÃO. Este script só acrescenta o botão "Guardar esta
// página": um clique do usuário = uma captura do que está renderizado. Não abre, pagina,
// atualiza nem busca URL nenhuma (e nunca toca em /tracker/api/).
(function () {
  'use strict';
  if (window.top !== window || window.__nexusTracker) return;
  window.__nexusTracker = true;

  function estrutura() {
    var tabelas = [];
    document.querySelectorAll('table').forEach(function (t) {
      if (tabelas.length >= 20) return;
      var linhas = [];
      t.querySelectorAll('tr').forEach(function (tr) {
        if (linhas.length >= 500) return;
        var c = [];
        tr.querySelectorAll('th,td').forEach(function (td) { c.push((td.innerText || '').trim()); });
        linhas.push(c);
      });
      tabelas.push(linhas);
    });
    var titulos = [];
    document.querySelectorAll('h1,h2,h3').forEach(function (h) {
      if (titulos.length < 100) titulos.push((h.innerText || '').trim());
    });
    return { titulos: titulos, tabelas: tabelas };
  }

  function montar() {
    if (document.getElementById('nexus-guardar')) return;
    var b = document.createElement('button');
    b.id = 'nexus-guardar';
    b.type = 'button';
    b.textContent = 'Guardar esta página';
    b.style.cssText = 'position:fixed;right:16px;bottom:16px;z-index:2147483647;padding:10px 14px;' +
      'font:600 14px system-ui;background:#1d6fdc;color:#fff;border:0;border-radius:8px;cursor:pointer';
    b.addEventListener('click', function (ev) {
      if (!ev.isTrusted) return; // só clique real do usuário
      var i = window.__TAURI_INTERNALS__;
      if (!i || !i.invoke) return;
      b.style.visibility = 'hidden'; // o botão não entra na captura
      var captura = {
        url: location.href,
        titulo: document.title,
        texto: document.body ? document.body.innerText : '',
        estrutura: estrutura()
      };
      b.style.visibility = 'visible';
      i.invoke('tracker_captura', { captura: captura }).then(function () {
        b.textContent = 'Guardado';
      }, function () {
        b.textContent = 'Falhou';
      });
      setTimeout(function () { b.textContent = 'Guardar esta página'; }, 2000);
    });
    (document.body || document.documentElement).appendChild(b);
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', montar);
  else montar();
})();
