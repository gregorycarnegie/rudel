// gen_core_oracle.mjs — dump golden transform outputs from Strudel core for the
// rudel parity oracle (crates/rudel-core/tests/transform_parity.rs).
//
//   cd tools/oracle && node gen_core_oracle.mjs
//
// Each entry is a labelled Pattern built with the real Strudel engine; the Rust
// test reconstructs the same pattern with rudel-core and compares hap-for-hap.

import { writeFileSync } from 'node:fs';
import { mini } from '@strudel/mini';
import { rev, s, note, randrun, zip, fast, jux, squeeze } from '@strudel/core';

// label -> Pattern (built from the same mini strings the Rust side uses).
const CASES = {
  rev: mini('0 1 2 3').rev(),
  fast2: mini('0 1 2 3').fast(2),
  slow2: mini('0 1 2 3').slow(2),
  ply2: mini('0 1 2').ply(2),
  iter4: mini('0 1 2 3').iter(4),
  palindrome: mini('0 1 2').palindrome(),
  every2_add10: mini('0 1 2 3').every(2, (x) => x.add(10)),
  off_add12: mini('0 1').off(0.25, (x) => x.add(12)),
  chop2: s(mini('bd')).chop(2),
  striate2: s(mini('bd sd')).striate(2),
  chunk4_add10: mini('0 1 2 3').chunk(4, (x) => x.add(10)),
  within_add10: mini('0 1 2 3').within(0, 0.5, (x) => x.add(10)),
  // struct/mask don't mini-parse string args (they reify a pure string), so the
  // boolean pattern must be passed as an explicit mini(...).
  struct: mini('0').struct(mini('1 0 1 0')),
  mask: mini('0 1 2 3').mask(mini('1 0')),
  jux_rev: note(mini('0 1')).jux(rev),
  add_const: mini('0 1 2').add(10),
  degrade: mini('0 1 2 3 4 5 6 7').degradeBy(0.5),
  superimpose: mini('0 1').superimpose((x) => x.add(7)),

  // alignment matrix: operator x alignment
  add_out: mini('0 1').add.out(mini('10 20 30')),
  add_mix: mini('0 1 2').add.mix(mini('10 20')),
  add_squeeze: mini('0 1').add.squeeze(mini('10 20')),
  add_squeezeout: mini('0 1').add.squeezeout(mini('10 20')),
  add_reset: mini('0 10 20 30').add.reset(mini('0 100')),
  add_restart: mini('0 10 20 30').add.restart(mini('0 100')),
  mul_out: mini('1 2').mul.out(mini('10 20 30')),
  set_squeeze: note(mini('0 1')).set.squeeze(s(mini('a b'))),
  set_out: note(mini('0 1')).set.out(s(mini('a b c'))),
  keep_out: note(mini('0 1')).keep.out(s(mini('a b c'))),
  add_poly: mini('0 1 2').add.poly(mini('10 20')),
  set_poly: note(mini('0 1 2')).set.poly(s(mini('a b'))),

  // random rearrangement (signal.mjs randrun/shuffle/scramble)
  randrun8: randrun(8),
  shuffle4: mini('0 1 2 3').shuffle(4),
  shuffle2: mini('a b c d').shuffle(2),
  scramble4: mini('0 1 2 3').scramble(4),

  // stepwise tour/zip (pattern.mjs)
  tour: mini('[c g]').tour(mini('e f'), mini('e f g'), mini('g f e c')),
  zip: zip(mini('e f'), mini('e f g'), mini('g [f e] a f4 c')),

  // the pick family (pick.mjs): join variants, clamping vs wrapping, list and
  // name lookups, function lookups (pickF), and standalone squeeze
  pick: mini('<0 1 2 3>').pick([mini('g a'), mini('e f'), mini('f g f g'), mini('g c d')]),
  pick_clamp: mini('0 1 5').pick([mini('a'), mini('b c')]),
  pickmod_wrap: mini('0 1 5').pickmod([mini('a'), mini('b c')]),
  pick_out: mini('0 1').pickOut([mini('a b c'), mini('d e')]),
  pickmod_out: mini('0 5').pickmodOut([mini('a b c'), mini('d e')]),
  pick_reset: mini('a [~ b]').pickReset({ a: mini('<0 1>'), b: mini('<2 3> 4') }),
  pick_restart: mini('a [~ b]').pickRestart({ a: mini('<0 1>'), b: mini('<2 3> 4') }),
  inhabit: mini('<0 1 [0 1]>').inhabit([mini('x y'), mini('z w v')]),
  inhabit_map: mini('a@2 [a b] a').inhabit({ a: mini('0 1 2'), b: mini('3 4') }),
  inhabitmod_wrap: mini('0 5').inhabitmod([mini('x y'), mini('z')]),
  squeeze: squeeze(mini('<0@2 [1!2] 2>'), [mini('g a'), mini('f g f g'), mini('g a c d')]),
  pickF: s(mini('bd [rim hh]')).pickF(mini('<0 1 2>'), [rev, jux(rev), fast(2)]),
  pickmodF: s(mini('bd [rim hh]')).pickmodF(mini('<0 1 5>'), [rev, jux(rev), fast(2)]),
};
const CYCLES = 4;

function fracStr(f) {
  return `${f.s < 0 ? '-' : ''}${f.n}/${f.d}`;
}
function normValue(v) {
  if (v === null || v === undefined) return null;
  if (Array.isArray(v)) return v.map(normValue);
  if (typeof v === 'object') {
    const o = {};
    for (const k of Object.keys(v).sort()) o[k] = normValue(v[k]);
    return o;
  }
  return v;
}
function dump(pat) {
  return pat.queryArc(0, CYCLES).map((h) => ({
    pb: fracStr(h.part.begin),
    pe: fracStr(h.part.end),
    wb: h.whole ? fracStr(h.whole.begin) : null,
    we: h.whole ? fracStr(h.whole.end) : null,
    v: normValue(h.value),
  }));
}

const out = {};
for (const [label, pat] of Object.entries(CASES)) {
  out[label] = dump(pat);
}
writeFileSync(new URL('./core_golden.json', import.meta.url), JSON.stringify(out, null, 1));
console.error('wrote core_golden.json');
