// gen_mini_oracle.mjs — dump golden mini-notation expansions from Strudel's real
// parser (@strudel/mini) for the rudel parity oracle.
//
//   cd tools/oracle && npm install && node gen_mini_oracle.mjs
//
// Resolution: tools/oracle/node_modules has fraction.js plus junctions
// @strudel/core and @strudel/mini -> strudel/packages/{core,mini}.
//
// Emits JSON: { "<pattern>": [ {pb,pe,wb,we,v}, ... ], ... } where pb/pe are the
// part begin/end and wb/we the whole begin/end, each as a reduced "num/den"
// string (or null for a continuous whole), and v the normalised value.

import { writeFileSync } from 'node:fs';
import { mini } from '@strudel/mini';

// Patterns exercising the core mini-notation grammar. Alternation/slowcat need
// several cycles, so everything is queried over cycles 0..CYCLES.
const PATTERNS = [
  '0 1 2 3',
  'bd sd hh',
  '0 [1 2] 3',
  '0*2 1',
  '0/2',
  '0!3 1',
  '0@3 1',
  '[0,1,2]',
  '0 ~ 2',
  '<0 1 2>',
  '0 1 . 2 3 4',
  'bd(3,8)',
  '0 .. 3',
  '[0 1]*2 2',
  '0 <1 2> 3',
  'bd:3 sd:5',
  '{0 1 2}%4',
  '{0 1, 2 3 4}',
  'bd(3,8,2)',
  '0 1*<2 3>',
  '<0 [1 2]> 3',
  '0 1!2 2',
  '[0 1 2 3]/2',
  '0 . 1 2 . 3 4 5',
  '0 1 2 3?',
  'bd*4?0.7',
  '[0,4,7]*2',
  'c4 e4 g4',
  '0 1@2 3',
];
const CYCLES = 3;

function fracStr(f) {
  // Fraction.js: s = sign (1/-1), n = numerator (>=0), d = denominator (>0).
  const sign = f.s < 0 ? '-' : '';
  return `${sign}${f.n}/${f.d}`;
}

function spanStr(span) {
  return span ? { b: fracStr(span.begin), e: fracStr(span.end) } : null;
}

function normValue(v) {
  if (v === null || v === undefined) return null;
  if (Array.isArray(v)) return v.map(normValue);
  if (typeof v === 'object') {
    const o = {};
    for (const k of Object.keys(v).sort()) o[k] = normValue(v[k]);
    return o;
  }
  return v; // number | string | boolean
}

function dump(code) {
  const pat = mini(code);
  const haps = pat.queryArc(0, CYCLES);
  const rows = haps.map((h) => {
    const whole = spanStr(h.whole);
    return {
      pb: fracStr(h.part.begin),
      pe: fracStr(h.part.end),
      wb: whole ? whole.b : null,
      we: whole ? whole.e : null,
      v: normValue(h.value),
    };
  });
  // Stable sort: by part begin, then end, then value JSON.
  rows.sort((a, b) => {
    const k = (r) => `${r.pb}|${r.pe}|${JSON.stringify(r.v)}`;
    return k(a) < k(b) ? -1 : k(a) > k(b) ? 1 : 0;
  });
  return rows;
}

const out = {};
for (const code of PATTERNS) {
  out[code] = dump(code);
}
// Write directly to file: importing @strudel/core prints a banner to stdout.
writeFileSync(new URL('./mini_golden.json', import.meta.url), JSON.stringify(out, null, 1));
console.error('wrote mini_golden.json');
