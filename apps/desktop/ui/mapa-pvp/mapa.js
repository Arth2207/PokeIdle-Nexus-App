const el = id => document.getElementById(id);
const mapa = el('mapa');
const ctx = mapa.getContext('2d');
const invoke = window.__TAURI__?.core?.invoke;
const dossie = new URLSearchParams(location.search).get('origem') === 'dossie';
if (dossie) {
  el('voltar').href = '/dossie.html';
  el('voltar').textContent = 'Voltar ao Dossiê';
} else if (invoke) {
  el('voltar').href = '../index.html#combate';
  el('voltar').textContent = 'Voltar ao Nexus Desktop';
}
const tipos = {
  NORMAL:['Normal','#b6becd'], FIRE:['Fogo','#ff826e'], WATER:['Água','#68aaff'],
  GRASS:['Planta','#82df89'], ELECTRIC:['Elétrico','#ffdb70'], ICE:['Gelo','#86e8f5'],
  FIGHTING:['Lutador','#ffa574'], POISON:['Veneno','#c990ff'], GROUND:['Terra','#d3a97c'],
  FLYING:['Voador','#8ebdff'], PSYCHIC:['Psíquico','#ff84b7'], BUG:['Inseto','#b8d466'],
  ROCK:['Pedra','#d7c796'], GHOST:['Fantasma','#b699de'], DRAGON:['Dragão','#a4a6ff'],
  DARK:['Sombrio','#b4a4a3'], STEEL:['Aço','#90d7d4'], FAIRY:['Fada','#ffa4e6'],
  UNKNOWN:['Sem tipo','#a7b7ce'],
};
const numero = new Intl.NumberFormat('pt-BR');
const decimal = new Intl.NumberFormat('pt-BR', {maximumFractionDigits:2});
const dataCaptura = new Intl.DateTimeFormat('pt-BR', {
  dateStyle:'short', timeStyle:'short', timeZone:'America/Sao_Paulo',
});
const normalizar = s => s.normalize('NFD').replace(/\p{Diacritic}/gu, '').toLowerCase();
const dex = new Map();
const filtros = new Set();
let meta, grupos = [], especies = [], setores = [], externos = [];
let fonte = 'catalogo', selecao = null, consulta = '';
let rankingLocal, assinaturaLocal, importado = false;
const temPresenca = () => fonte === 'tracker';
const formatarUso = uso => temPresenca() ? decimal.format(uso) + '%' : numero.format(uso);
let largura = 1, altura = 1, zoom = 1, dx = 0, dy = 0, gesto, quadro;
const volta = Math.PI * 2;
const polar = (angulo, r) => ({x: Math.cos(angulo) * r, y: Math.sin(angulo) * r});
// Formas Mega com ID próprio no catálogo de sprites da PokéAPI.
const formasMega = {
  3003: 10033, 3006: 10035, 9006: 10034, 3009: 10036, 3015: 10090, 3018: 10073, 3026: 10305,
  9026: 10304, 3036: 10278, 3065: 10037, 3071: 10279, 3080: 10071, 3094: 10038, 3115: 10039,
  3121: 10280, 3127: 10040, 3130: 10041, 3142: 10042, 3149: 10281, 3150: 10044, 9150: 10043,
  3154: 10282, 3160: 10283, 3181: 10045, 3208: 10072, 3212: 10046, 3214: 10047, 3227: 10284,
  3229: 10048, 3248: 10049, 3254: 10065, 3257: 10050, 3260: 10064, 3282: 10051, 3302: 10066,
  3303: 10052, 3306: 10053, 3308: 10054, 3310: 10055, 3319: 10070, 3323: 10087, 3334: 10067,
  3354: 10056, 3358: 10306, 3359: 10057, 3362: 10074, 3373: 10089, 3376: 10076, 3380: 10062,
  3381: 10063, 3384: 10079, 3398: 10308, 3428: 10088, 3445: 10058, 9445: 10309, 3448: 10059,
  9448: 10310, 3460: 10060, 3475: 10068, 3478: 10285, 3485: 10311, 3491: 10312, 3500: 10286,
  3530: 10287, 3531: 10069, 3545: 10288, 3560: 10289, 3604: 10290, 3609: 10291, 3623: 10313,
  3652: 10292, 3655: 10293, 3658: 10294, 3668: 10295, 3670: 10296, 3687: 10297, 3689: 10298,
  3691: 10299, 3701: 10300, 3718: 10301, 3719: 10075, 3740: 10315, 3768: 10316, 3780: 10302,
  3801: 10317, 3807: 10319, 3870: 10303, 3952: 10320, 3998: 10325,
};
const idBase = id => id <= 1025 ? id : id % 1000;
const sprite = id => 'https://raw.githubusercontent.com/PokeAPI/sprites/master/sprites/pokemon/' + id + '.png';

function usarSprite(img, id) {
  const base = idBase(id);
  const forma = formasMega[id];
  if (forma) img.addEventListener('error', () => { img.src = sprite(base); }, {once:true});
  img.src = sprite(forma || base);
}
const sprites = new Map();

function imagem(id) {
  if (!sprites.has(id)) {
    const img = new Image();
    img.addEventListener('load', desenhar);
    usarSprite(img,id);
    sprites.set(id, img);
  }
  return sprites.get(id);
}

function carregarFonte(ranking, preservar = false) {
  const anterior = selecao;
  fonte = ranking ? 'ranking' : meta?.origem || (meta ? 'meta' : 'catalogo');
  grupos = ranking || (meta?.times || []).map((t, i) => ({
    nome: 'Equipe ' + (i + 1), detalhe: numero.format(t.vezes) + ' aparições',
    time: t.especies,
  }));
  const usos = new Map();
  const dados = new Map(dex);
  for (const sp of meta?.especies || []) {
    if (!dados.has(sp.especie)) dados.set(sp.especie, {d:sp.especie, n:sp.nome || 'Espécie ' + sp.especie, t:[]});
    if (!ranking) usos.set(sp.especie, sp.presenca ?? sp.usos);
  }
  for (const grupo of grupos) {
    grupo.ids = new Set(grupo.time);
    for (const id of grupo.ids) {
      if (!dados.has(id)) dados.set(id, {d:id, n:'Espécie ' + id, t:[]});
      if (ranking) usos.set(id, (usos.get(id) || 0) + 1);
    }
  }
  especies = [...dados.values()].map(sp => ({
    ...sp, uso:usos.get(sp.d) || 0, tipo:tipos[sp.t[0]] ? sp.t[0] : 'UNKNOWN',
  })).sort((a,b) => b.uso - a.uso || a.n.localeCompare(b.n, 'pt-BR'));
  selecao = null;
  if (preservar && anterior?.sp) {
    const sp = especies.find(item => item.d === anterior.sp.d);
    if (sp) selecao = {sp};
  } else if (preservar && anterior?.grupo) {
    const grupo = grupos.find(item => item.nome === anterior.grupo.nome);
    if (grupo) selecao = {grupo};
  }
  el('meta').hidden = !importado;
  el('meta').textContent = invoke ? 'Voltar à coleta do app' : dossie ? 'Voltar ao Tracker' : 'Voltar ao meta observado';
  el('status').textContent = ranking ? grupos.length + (importado ? ' jogadores do ranking importado' : ' jogadores do ranking coletado do jogo')
    : temPresenca() ? 'Presença no Tracker · captura de ' + dataCaptura.format(new Date(meta.em))
    : invoke ? 'Aguardando os times do ranking. Mantenha uma conta conectada ao jogo.'
    : dossie ? 'Pokédex do Dossiê. Nenhuma amostra de presença do Tracker disponível.'
    : meta ? 'Meta observado em ' + numero.format(meta.partidas) + ' partidas (14 dias)'
    : 'Pokédex disponível. O meta do PvP está indisponível.';
  el('titulo-usos').textContent = ranking ? 'Mais usados no ranking'
    : temPresenca() ? 'Maior presença no Tracker' : 'Mais usados nas partidas';
  el('titulo-grupos').textContent = ranking ? 'Jogadores do ranking' : 'Equipes mais frequentes';
  el('titulo-grupos').hidden = !grupos.length;
  atualizar();
}

function nome(id) {
  return especies.find(sp => sp.d === id)?.n || 'Espécie ' + id;
}

function item(container, titulo, legenda, escolha, total, id) {
  const botao = document.createElement('button');
  botao.className = 'item';
  botao.setAttribute('aria-pressed', String(escolha.sp ? selecao?.sp === escolha.sp : selecao?.grupo === escolha.grupo));
  if (id) {
    const img = document.createElement('img');
    img.alt = '';
    img.loading = 'lazy';
    usarSprite(img,id);
    botao.append(img);
  }
  const texto = document.createElement('span');
  const principal = document.createElement('strong');
  const subtitulo = document.createElement('small');
  principal.textContent = titulo;
  subtitulo.textContent = legenda;
  texto.append(principal, subtitulo);
  botao.append(texto);
  if (total) {
    const numeroEl = document.createElement('span');
    numeroEl.className = 'contador';
    numeroEl.textContent = formatarUso(total);
    botao.append(numeroEl);
  }
  botao.onclick = () => {
    const indice = Array.from(container.children).indexOf(botao);
    selecionar(escolha);
    container.children[indice]?.focus();
  };
  container.append(botao);
}

function selecionar(escolha) {
  selecao = escolha && (escolha.sp ? selecao?.sp !== escolha.sp : selecao?.grupo !== escolha.grupo) ? escolha : null;
  atualizar();
}

function atualizar() {
  consulta = normalizar(el('busca').value.trim());
  const jogadores = grupos.filter(g => consulta && normalizar(g.nome).includes(consulta));
  const visiveis = especies.filter(sp =>
    (!filtros.size || sp.t.some(t => filtros.has(t))) &&
    (!el('usadas').checked || sp.uso) &&
    (!consulta || normalizar(sp.n).includes(consulta) || jogadores.some(g => g.ids.has(sp.d))));
  posicionar(visiveis);
  const lista = el('especies');
  lista.replaceChildren();
  (consulta ? visiveis : visiveis.filter(sp => sp.uso)).slice(0, consulta ? 50 : 15).forEach(sp =>
    item(lista, sp.n, sp.t.map(t => tipos[t]?.[0]).filter(Boolean).join(' / ') || 'Sem tipo', {sp}, sp.uso, sp.d));
  const equipes = el('grupos');
  equipes.replaceChildren();
  externos.filter(g => !consulta || jogadores.includes(g.grupo) || g.ligacoes.length)
    .forEach(({grupo}) => item(equipes, grupo.nome, grupo.detalhe, {grupo}));
  const detalhe = el('selecao');
  detalhe.hidden = !selecao;
  detalhe.replaceChildren();
  if (selecao) {
    const titulo = document.createElement('h2');
    const info = document.createElement('p');
    titulo.textContent = selecao.sp?.n || selecao.grupo.nome;
    info.textContent = selecao.grupo ? selecao.grupo.time.map(nome).join(' · ')
      : formatarUso(selecao.sp.uso) + (fonte === 'ranking' ? ' jogadores usam esta espécie'
        : temPresenca() ? ' de presença no Tracker' : ' usos nas partidas');
    detalhe.append(titulo, info);
  }
  desenhar();
}

function posicionar(visiveis) {
  const porTipo = new Map();
  for (const tipo of Object.keys(tipos)) {
    const membros = visiveis.filter(sp => sp.tipo === tipo);
    if (membros.length) porTipo.set(tipo, membros);
  }
  let angulo = -Math.PI / 2;
  const pontos = new Map();
  setores = [];
  for (const [tipo, membros] of porTipo) {
    const abertura = volta * (0.42 / porTipo.size + 0.58 * membros.length / visiveis.length);
    const setor = {tipo, inicio:angulo, fim:angulo + abertura, pontos:[]};
    const colunas = Math.max(1, Math.ceil(Math.sqrt(membros.length * abertura * 1.35)));
    const faixas = Math.ceil(membros.length / colunas);
    membros.forEach((sp, i) => {
      const faixa = Math.floor(i / colunas);
      const n = Math.min(colunas, membros.length - faixa * colunas);
      const a = angulo + abertura * ((i % colunas + .5) / n);
      const ponto = {...polar(a, 75 + 174 * (faixa + .25) / Math.max(1, faixas)), sp};
      pontos.set(sp.d, ponto);
      setor.pontos.push(ponto);
    });
    setores.push(setor);
    angulo += abertura;
  }
  externos = grupos.map((grupo, i) => {
    const a = volta * i / grupos.length - Math.PI / 2;
    return {...polar(a, 318), a, grupo, ligacoes:[...grupo.ids].map(id => pontos.get(id)).filter(Boolean)};
  });
}

function desenhar() {
  if (quadro) return;
  quadro = requestAnimationFrame(() => {
    quadro = null;
    renderizar();
  });
}

function transformar() {
  const escala = Math.min(largura, altura) / 760 * zoom;
  return {escala, x:largura / 2 + dx, y:altura / 2 + dy};
}

function circulo(x, y, raio, cor) {
  ctx.beginPath();
  ctx.arc(x,y,raio,0,volta);
  ctx.fillStyle = cor;
  ctx.fill();
}

function rotulo(texto, x, y, cor, tamanho, alinhamento = 'center') {
  ctx.font = '600 ' + tamanho + 'px system-ui';
  ctx.textAlign = alinhamento;
  ctx.lineWidth = 3;
  ctx.strokeStyle = '#0a101b';
  ctx.strokeText(texto,x,y);
  ctx.fillStyle = cor;
  ctx.fillText(texto,x,y);
}

function renderizar() {
  const {escala, x, y} = transformar();
  ctx.setTransform(mapa.width / largura,0,0,mapa.height / altura,0,0);
  ctx.clearRect(0,0,largura,altura);
  ctx.translate(x,y);
  ctx.scale(escala,escala);
  for (const setor of setores) {
    const [rotuloTipo, cor] = tipos[setor.tipo];
    const intervalo = Math.min(.012,(setor.fim - setor.inicio) / 5);
    ctx.beginPath();
    ctx.arc(0,0,257,setor.inicio + intervalo,setor.fim - intervalo);
    ctx.arc(0,0,63,setor.fim - intervalo,setor.inicio + intervalo,true);
    ctx.closePath();
    ctx.fillStyle = cor + '1c';
    ctx.strokeStyle = cor + '60';
    ctx.lineWidth = 1;
    ctx.fill();
    ctx.stroke();
    const label = polar((setor.inicio + setor.fim) / 2, 274);
    if (setor.fim - setor.inicio > .13 || zoom > 1.5) rotulo(rotuloTipo,label.x,label.y,cor,12);
  }
  for (const ext of externos) {
    for (const p of ext.ligacoes) {
      const ativa = selecao?.grupo === ext.grupo || selecao?.sp === p.sp;
      if (!ativa && !el('linhas').checked) continue;
      ctx.beginPath();
      ctx.moveTo(ext.x,ext.y);
      ctx.lineTo(p.x,p.y);
      ctx.strokeStyle = ativa ? '#ff8dcb' : selecao ? '#54769322' : '#6fa9c659';
      ctx.lineWidth = ativa ? 2.8 : 1;
      ctx.setLineDash(ativa ? [7,4] : []);
      ctx.stroke();
    }
  }
  ctx.setLineDash([]);
  desenharEspecies(escala,x,y);
  const resumo = setores.reduce((total, s) => total + s.pontos.length, 0);
  rotulo('POKÉDEX',0,-2,'#edf4ff',14);
  rotulo(numero.format(resumo) + ' espécies',0,16,'#a7b7ce',11);
  for (const ext of externos) {
    const ativo = selecao?.grupo === ext.grupo || selecao?.sp && ext.grupo.ids.has(selecao.sp.d);
    circulo(ext.x,ext.y,ativo ? 9 : 6,ativo ? '#ff8dcb' : '#73e5d4');
    if (ativo || grupos.length <= 12 || zoom > 1.3) {
      const label = polar(ext.a,332);
      rotulo(ext.grupo.nome.slice(0,26),label.x,label.y + (Math.abs(ext.x) < 35 ? 4 : 0),
        ativo ? '#ffd2ed' : '#badbdc',11, Math.abs(ext.x) < 35 ? 'center' : ext.x > 0 ? 'left' : 'right');
    }
  }
}

function desenharEspecies(escala, centroX, centroY) {
  const pontos = [];
  for (const setor of setores) {
    for (const [indice,p] of setor.pontos.entries()) {
      const x = centroX + p.x * escala;
      const y = centroY + p.y * escala;
      if (x < 0 || x > largura || y < 0 || y > altura) continue;
      const ativo = selecao?.sp === p.sp || selecao?.grupo?.ids.has(p.sp.d);
      pontos.push({p, x, y, ativo: Boolean(ativo), miniatura: indice < 3 || consulta || zoom >= 1.5});
    }
  }
  pontos.sort((a,b) => Number(b.ativo) - Number(a.ativo) || b.p.sp.uso - a.p.sp.uso);
  const ocupados = [];
  const spritesVisiveis = [];
  const pontosSimples = [];
  const cabe = (x,y,w,h) => x >= 0 && y >= 0 && x+w <= largura && y+h <= altura
    && !ocupados.some(r => x < r.x+r.w && x+w > r.x && y < r.y+r.h && y+h > r.y);

  for (const marca of pontos) {
    const {p,x,y,ativo,miniatura} = marca;
    const lado = ativo ? 26 : p.sp.uso ? 21 : 17;
    const espaco = lado + (ativo ? 8 : 10);
    if ((!ativo && (!miniatura || spritesVisiveis.length >= 90))
        || !cabe(x-espaco/2,y-espaco/2,espaco,espaco)) {
      pontosSimples.push(marca);
      continue;
    }
    ocupados.push({x:x-espaco/2,y:y-espaco/2,w:espaco,h:espaco});
    spritesVisiveis.push({...marca,lado});
  }

  for (const {p,x,y,ativo} of pontosSimples) {
    const raio = ativo ? 3.5 : p.sp.uso ? 2.5 : 1.35;
    if (!cabe(x-raio,y-raio,raio*2,raio*2)) continue;
    const cor = ativo ? '#ffe19a' : tipos[p.sp.tipo][1] + (selecao ? '38' : p.sp.uso ? 'a0' : '68');
    circulo(p.x,p.y,raio/escala,cor);
  }
  for (const {p,lado,ativo} of spritesVisiveis) {
    const img = imagem(p.sp.d);
    if (img.complete && img.naturalWidth) {
      const medida = lado / escala;
      ctx.globalAlpha = selecao && !ativo ? .38 : .95;
      ctx.drawImage(img,p.x-medida/2,p.y-medida/2,medida,medida);
      ctx.globalAlpha = 1;
    } else {
      circulo(p.x,p.y,(ativo ? 4 : 2.5)/escala,ativo ? '#ffe19a' : tipos[p.sp.tipo][1]+'a0');
    }
  }

  // Rótulos também disputam espaço em pixels para continuarem legíveis ao ampliar.
  ctx.save();
  ctx.setTransform(mapa.width/largura,0,0,mapa.height/altura,0,0);
  ctx.font = '600 12px system-ui';
  for (const {p,x,y,ativo,lado} of spritesVisiveis) {
    if (!ativo) continue;
    const nome = p.sp.n;
    const w = ctx.measureText(nome).width + 8;
    const h = 18;
    const margem = lado/2 + 5;
    const tentativas = [
      [x+margem,y-h/2], [x-margem-w,y-h/2],
      [x-w/2,y+margem], [x-w/2,y-margem-h],
    ];
    const local = tentativas.find(([lx,ly]) => cabe(lx,ly,w,h));
    if (!local) continue;
    const [lx,ly] = local;
    ocupados.push({x:lx,y:ly,w,h});
    rotulo(nome,lx+4,ly+13,'#ffe19a',12,'left');
  }
  ctx.restore();
}

function medir() {
  const area = mapa.getBoundingClientRect();
  largura = Math.max(1, area.width);
  altura = Math.max(1, area.height);
  const dpr = Math.min(2, devicePixelRatio || 1);
  mapa.width = Math.round(largura * dpr);
  mapa.height = Math.round(altura * dpr);
  desenhar();
}

function ampliar(fator, x = largura / 2, y = altura / 2) {
  const antes = zoom;
  zoom = Math.max(.65, Math.min(6, zoom * fator));
  dx += (x - largura / 2 - dx) * (1 - zoom / antes);
  dy += (y - altura / 2 - dy) * (1 - zoom / antes);
  desenhar();
}

function enquadrar() {
  zoom = 1;
  dx = dy = 0;
  desenhar();
}

mapa.addEventListener('pointerdown', e => {
  if (e.button !== 0 || gesto) return;
  mapa.setPointerCapture(e.pointerId);
  gesto = {id:e.pointerId, x:e.offsetX, y:e.offsetY, dx, dy, moveu:false};
});
mapa.addEventListener('pointermove', e => {
  if (gesto?.id !== e.pointerId) return;
  const x = e.offsetX - gesto.x;
  const y = e.offsetY - gesto.y;
  if (!gesto.moveu && Math.hypot(x,y) < 5) return;
  gesto.moveu = true;
  dx = gesto.dx + x;
  dy = gesto.dy + y;
  desenhar();
});
mapa.addEventListener('pointerup', e => {
  if (gesto?.id !== e.pointerId) return;
  const clique = !gesto.moveu;
  gesto = null;
  if (!clique) return;
  const {escala, x, y} = transformar();
  const mx = (e.offsetX - x) / escala;
  const my = (e.offsetY - y) / escala;
  let escolha = null;
  let menor = Math.max(8, 8 / escala);
  for (const p of [...externos, ...setores.flatMap(s => s.pontos)]) {
    const dist = Math.hypot(p.x - mx,p.y - my);
    if (dist < menor) {
      menor = dist;
      escolha = p.grupo ? {grupo:p.grupo} : {sp:p.sp};
    }
  }
  selecionar(escolha);
});
mapa.addEventListener('lostpointercapture', () => gesto = null);
mapa.addEventListener('wheel', e => {
  if (!e.ctrlKey && !e.metaKey) return;
  e.preventDefault();
  ampliar(e.deltaY < 0 ? 1.15 : 1/1.15,e.offsetX,e.offsetY);
},{passive:false});
new ResizeObserver(medir).observe(mapa);

function montarFiltros() {
  for (const [tipo,[nome,cor]] of Object.entries(tipos)) {
    if (tipo === 'UNKNOWN') continue;
    const b = document.createElement('button');
    b.textContent = nome;
    b.style.setProperty('--tipo',cor);
    b.setAttribute('aria-pressed','false');
    b.onclick = () => {
      if (!filtros.delete(tipo)) filtros.add(tipo);
      b.setAttribute('aria-pressed',String(filtros.has(tipo)));
      atualizar();
    };
    el('tipos').append(b);
  }
}
montarFiltros();
el('menos').onclick = () => ampliar(1/1.25);
el('mais').onclick = () => ampliar(1.25);
el('ajustar').onclick = enquadrar;
el('busca').oninput = atualizar;
el('usadas').onchange = atualizar;
el('linhas').onchange = desenhar;
document.addEventListener('keydown', e => {
  if (e.key === 'Escape' && selecao) selecionar(null);
});

function lerRanking(dados) {
  const times = dados?.ranking?.times ?? dados?.times;
  if (!times || typeof times !== 'object' || Array.isArray(times)) {
    throw new Error('O arquivo não contém os times do ranking do Nexus.');
  }
  const jogadores = [];
  for (const [nome, registro] of Object.entries(times)) {
    if (!nome.trim() || !Array.isArray(registro?.time)) continue;
    const time = registro.time.map(p => p?.speciesId).filter(id => Number.isSafeInteger(id) && id > 0);
    if (!time.length) continue;
    jogadores.push({
      nome, time, pontos:Number.isFinite(registro.pontos) ? registro.pontos : null,
      posicao:Number.isSafeInteger(registro.posicao) && registro.posicao > 0 ? registro.posicao : null,
    });
  }
  if (!jogadores.length) throw new Error('Nenhum jogador com equipe foi encontrado no arquivo.');
  return jogadores.sort((a,b) => (a.posicao ?? Infinity) - (b.posicao ?? Infinity)
    || (b.pontos ?? -Infinity) - (a.pontos ?? -Infinity) || a.nome.localeCompare(b.nome,'pt-BR'))
    .map(j => ({...j, detalhe:[
      j.posicao && '#' + j.posicao,
      j.pontos !== null && numero.format(j.pontos) + ' pontos',
    ].filter(Boolean).join(' · ') || 'Sem pontuação'}));
}

el('ranking').disabled = true;
el('ranking').onchange = async () => {
  const arquivo = el('ranking').files[0];
  if (!arquivo) return;
  el('ranking').disabled = true;
  try {
    if (arquivo.size > 10_000_000) throw new Error('O arquivo deve ter até 10 MB.');
    let dados;
    try {
      dados = JSON.parse(await arquivo.text());
    } catch {
      throw new Error('Não foi possível ler o JSON do ranking.');
    }
    const ranking = lerRanking(dados);
    importado = true;
    carregarFonte(ranking);
    el('erro').hidden = true;
  } catch (erro) {
    el('erro').textContent = erro.message;
    el('erro').hidden = false;
  } finally {
    el('ranking').value = '';
    el('ranking').disabled = false;
  }
};
el('meta').onclick = () => {
  el('erro').hidden = true;
  importado = false;
  carregarFonte(rankingLocal);
  el('ranking').focus();
};

async function json(url) {
  const r = await fetch(url,{signal:AbortSignal.timeout(12000)});
  if (!r.ok) throw new Error('HTTP ' + r.status);
  return r.json();
}

async function atualizarRankingLocal() {
  try {
    const resposta = await invoke('ranking_pvp');
    const assinatura = JSON.stringify(resposta?.times);
    if (assinatura === assinaturaLocal) return;
    assinaturaLocal = assinatura;
    rankingLocal = Object.keys(resposta?.times || {}).length ? lerRanking(resposta) : undefined;
    if (!importado) carregarFonte(rankingLocal, true);
  } catch {
    // O mapa web também pode ser aberto em uma versão do app anterior à coleta automática.
  }
}

function carregarMeta() {
  if (dossie) {
    return json('/api/dossie/pvp/presenca').then(dados => dados.especies?.length && Number.isFinite(Date.parse(dados.em))
      ? {...dados, origem:'tracker', times:[]} : null);
  }
  if (!invoke) return json('../api/pvp/meta');
  const url = localStorage.getItem('nexus.url');
  return url ? invoke('api_get', {url, rota:'/api/pvp/meta'}) : null;
}

Promise.allSettled([
  json(dossie ? '/api/dossie/calc/catalogo' : 'catalogo.json'),
  Promise.resolve().then(carregarMeta),
]).then(([catalogo, dados]) => {
  if (catalogo.status === 'fulfilled') {
    for (const sp of catalogo.value) dex.set(sp.d,sp);
  } else {
    el('erro').textContent = 'Não foi possível carregar o catálogo completo da Pokédex.';
    el('erro').hidden = false;
  }
  if (dados.status === 'fulfilled' && Array.isArray(dados.value?.especies) && Array.isArray(dados.value?.times)) {
    meta = dados.value;
  }
  carregarFonte();
  el('ranking').disabled = false;
  if (invoke) {
    const consultar = () => {
      if (document.visibilityState === 'visible') void atualizarRankingLocal();
    };
    consultar();
    setInterval(consultar, 12000);
  }
});
