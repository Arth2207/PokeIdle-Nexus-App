// Teste de carga das rotas de equipes (k6). Uso:
//   docker run --rm --network host -v $PWD/ferramentas/carga:/s -e BASE=http://127.0.0.1:8480 \
//     -e TOKEN=... -e RPS=2 -e DUR=30s -e OTIMIZADOR=0 grafana/k6:1.3.0 run /s/equipes.js
// BASE sem TOKEN mede o legado. OTIMIZADOR=1 mantém cálculos de equipe rodando em paralelo
// (orçamentos diferentes forçam cálculo novo) para medir a API com a CPU ocupada.
// O rate limit do Nexus é por token (10 req/s, rajada de 40): fique abaixo disso.
import http from 'k6/http';
import { check } from 'k6';

const BASE = __ENV.BASE;
const TOKEN = __ENV.TOKEN || '';
const RPS = Number(__ENV.RPS || 2);
const DUR = __ENV.DUR || '30s';
const BOSSES = ['articuno', 'moltres', 'zapdos', 'suicune', 'entei', 'raikou', 'groudon', 'kyogre'];

export const options = {
  scenarios: {
    leitura: { executor: 'constant-arrival-rate', rate: RPS, timeUnit: '1s', duration: DUR, preAllocatedVUs: 10, exec: 'leitura' },
    ...(__ENV.OTIMIZADOR === '1'
      ? { otimizador: { executor: 'constant-arrival-rate', rate: 1, timeUnit: '2s', duration: DUR, preAllocatedVUs: 4, exec: 'otimizador' } }
      : {}),
  },
  summaryTrendStats: ['avg', 'p(50)', 'p(95)', 'p(99)', 'max'],
  thresholds: __ENV.OTIMIZADOR === '1' ? { 'http_req_duration{grupo:leitura}': ['p(95)<50'] } : {},
};

const cab = TOKEN ? { headers: { Authorization: `Bearer ${TOKEN}` } } : {};

export function leitura() {
  const b = BOSSES[Math.floor(Math.random() * BOSSES.length)];
  const rotas = ['/api/equipes/bosses', `/api/equipes/plano?boss=${b}`, '/api/bolsa/alvos-coleta', '/api/bolsa/mercado'];
  const r = http.get(BASE + rotas[Math.floor(Math.random() * rotas.length)], { ...cab, tags: { grupo: 'leitura' } });
  check(r, { ok: (x) => x.status === 200 });
}

export function otimizador() {
  const b = BOSSES[Math.floor(Math.random() * BOSSES.length)];
  http.get(`${BASE}/api/bolsa/equipe?boss=${b}&moeda=gold&orcamento=${1000 + Math.floor(Math.random() * 1e6)}`, {
    ...cab,
    tags: { grupo: 'otimizador' },
  });
}
