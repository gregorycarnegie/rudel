// gen_mini_oracle.mjs — dump golden mini-notation expansions from Strudel's real
// parser (@strudel/mini) for the rudel parity oracle.
//
//   cd tools/oracle && npm install && node gen_mini_oracle.mjs
//
// Resolution: tools/oracle/node_modules has fraction.js plus junctions
// @strudel/core and @strudel/mini -> strudel/packages/{core,mini}.
//
// Emits JSON: { "<pattern>": { "steps": "num/den"|null, "locs": [[a,b],...],
// "haps": [ {pb,pe,wb,we,v,l}, ... ] } } where pb/pe are the part begin/end
// and wb/we the whole begin/end, each as a reduced "num/den" string (or null
// for a continuous whole), v the normalised value, l the hap's sorted source
// locations, steps the pattern's `_steps`, and locs the sorted leaf locations.
// Strudel's locations are relative to the quoted code, so 1 is subtracted to
// make them offsets into the bare pattern string (matching rudel's parse).

import { writeFileSync } from 'node:fs';
import { mini, getLeafLocations } from '@strudel/mini';
import Fraction from '@strudel/core/fraction.mjs';

// Patterns exercising the mini-notation grammar, including the cases from
// strudel/packages/mini/test/mini.test.mjs and tokenizer edge cases.
// Patterns using ? or | rely on rudel's rand matching Strudel's PRNG.
const PATTERNS = [
  // basics
  '0 1 2 3',
  'bd sd hh',
  'a',
  '~',
  '0 [1 2] 3',
  'c3 [d3 [e3 f3]]',
  '0 ~ 2',
  'a - b [- c]',
  // fast/slow
  '0*2 1',
  '0/2',
  'a*3 b',
  '[a*<3 5>]*2',
  '[a a a]/3 b',
  '[a a a a a a a a]/[2 4]',
  '[c3 d3]/2',
  'c3*2',
  '[c3 d3]*2',
  'a/<1 2>',
  'a*2.5 b',
  '0 1*<2 3>',
  '[0 1]*2 2',
  '[0 1 2 3]/2',
  // alternation
  '<a b>',
  '<0 1 2>',
  '0 <1 2> 3',
  '<0 [1 2]> 3',
  '<a!2 b> c',
  '<a@2 b> c',
  // stacks and choice
  '[0,1,2]',
  '[c3,e3,g3] f3',
  '[0,4,7]*2',
  'a | b | c',
  '[a|b] [a|b]',
  'a | [b | c]',
  // weights and replication
  '0@3 1',
  'a@2 b@3',
  '0!3 1',
  '0 1!2 2',
  'a ! ! b',
  'a @ b @ @',
  'a _ b _ _',
  '[<a b c>]!3 d',
  '[<a b c>]!! d',
  // euclid
  'bd(3,8)',
  'bd(3,8,2)',
  'bd(3, 8) sd(5 , 8)',
  '[a(<3 5>, <8 16>)]*2',
  'a(3,8,<0 2>)',
  'x(2,5) x(3,4)',
  'x(7,16)',
  // ranges
  '0 .. 3',
  '4 .. 0',
  '[<0 1> .. <2 4>]*2',
  // dots/feet
  '0 1 . 2 3 4',
  '0 . 1 2 . 3 4 5',
  'a . b c . [d e f . g h]',
  // tails
  'bd:3 sd:5',
  'a:b c:d:[e:f] g',
  'c:maj7 g:7',
  // polymeter
  '{0 1 2}%4',
  '{0 1, 2 3 4}',
  '{a b, c d e}*3',
  '{a b, c [d e] f}*3',
  '{a b c, d e}*2',
  '{a b, c d e}%3',
  '{a b, c d e}%5',
  '{a b c}%<2 3>',
  '{a@2 b, c}%2',
  // degradation (PRNG + per-occurrence seed parity)
  '0 1 2 3?',
  'bd*4?0.7',
  'a? b? c?',
  'a?0.8 b?0.2',
  'a?.8 b',
  'a!2? b',
  // ^ steps marker
  'a [^b c]',
  '[^b c]!3',
  '[a b c] [d [e f]]',
  '^[a b c] [d [e f]]',
  '[a b c] [d [^e f]]',
  '[a b c] [^d [e f]]',
  '[^a b c] [^d [e f]]',
  '[^a b c] [d [^e f]]',
  '[^a b c d e]',
  // tokenizer edge cases
  'c4 e4 g4',
  '3M 5P -2M',
  '1e3 2',
  '.5 1.',
  '0x10 0',
  'bd.cp x',
  'a~b c',
  '-3 -x',
  'bd#2 x_y',
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

const sortPairs = (locs) => locs.sort((x, y) => x[0] - y[0] || x[1] - y[1]);

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
      l: sortPairs((h.context.locations || []).map(({ start, end }) => [start - 1, end - 1])),
    };
  });
  // Stable sort: by part begin, then end, then value JSON.
  rows.sort((a, b) => {
    const k = (r) => `${r.pb}|${r.pe}|${JSON.stringify(r.v)}`;
    return k(a) < k(b) ? -1 : k(a) > k(b) ? 1 : 0;
  });
  const steps = pat._steps === undefined ? null : fracStr(Fraction(pat._steps));
  const locs = sortPairs(getLeafLocations(`"${code}"`).map(([a, b]) => [a - 1, b - 1]));
  return { steps, locs, haps: rows };
}

const out = {};
for (const code of PATTERNS) {
  out[code] = dump(code);
}
// Write directly to file: importing @strudel/core prints a banner to stdout.
writeFileSync(new URL('./mini_golden.json', import.meta.url), JSON.stringify(out, null, 1));
console.error('wrote mini_golden.json');
