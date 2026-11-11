'use strict';
/**
 * Fixtures de Math.log, Math.exp e Math.pow do Node, para conferir os portes do `nucleo::js`.
 * Uso: node ferramentas/fixtures/js-math.cjs > crates/nucleo/tests/fixtures/js-math.json
 */
let semente = 7;
const rnd = () => { semente = (Math.imul(semente, 1664525) + 1013904223) >>> 0; return semente / 4294967296; };
const sorteia = a => a[Math.floor(rnd() * a.length)];
const ln = [], exp = [];
for (let i = 0; i < 20000; i++) {
  const x = sorteia([rnd() * 10, rnd() * 1e6, rnd() * 1e-3, 1 + (rnd() - 0.5) * 1e-6, Math.exp(rnd() * 1400 - 700), rnd() * 3e5, 1e-310 * rnd(), 0, -1, 1, 2, 3, 5]);
  ln.push([x, Math.log(x)]);
  const y = sorteia([rnd() * 40 - 20, rnd() * 1500 - 750, (rnd() - 0.5) * 1e-9, rnd() * 2 - 1, 1, 0, -745.2, 709.8]);
  exp.push([y, Math.exp(y)]);
}
// JSON grava Infinity/NaN como null.
process.stdout.write(JSON.stringify({ ln, exp }));
