// gen_stepwise_oracle.mjs — the stepwise surface, with Strudel's own haps.
//
//   cd tools/oracle && node gen_stepwise_oracle.mjs
//
// https://strudel.cc/learn/stepwise/ is the one documentation page whose
// functions are all *about* a pattern's step count rather than its cycle, and a
// step count is metadata: a wrong one is silent, not loud. `expand` on its own
// sounds identical however much it expands — the page says so — so an example
// can look fine, run fine, and be wrong, right up until a `stepcat` or a `pace`
// reads the number back out.
//
// That is why this oracle exists rather than hand-written expectations: every
// case is a source string evaluated by the real Strudel engine, and
// `crates/rudel-lang/tests/stepwise_parity.rs` evaluates *the same string*
// through Rudel and compares hap for hap. The corpus is the page's own examples
// plus every `@example` from the functions it documents.
//
// Sources are written with explicit `mini(...)` rather than bare string
// literals, so the same text is valid in both languages: Strudel's REPL rewrites
// a string literal into mini-notation with its transpiler (not installed here),
// and Rudel does it in its preprocessor.

import { mini } from '@strudel/mini';
import * as core from '@strudel/core';
import { fracStr, normValue, writeJson } from './lib.mjs';

core.setStringParser(mini);

// --- from https://strudel.cc/learn/stepwise/ ---------------------------------
const PAGE = {
  // "when you `fastcat` two patterns together, the cycles will be squashed into
  // half a cycle each" — versus stepcat, which distributes their steps evenly.
  fastcat_two: 'fastcat(mini("bd hh hh"), mini("bd hh hh cp hh")).sound()',
  stepcat_two: 'stepcat(mini("bd hh hh"), mini("bd hh hh cp hh")).sound()',

  // "steps are counted according to the 'top level' in mini-notation": five
  // events but four steps, unless `^` marks a different metrical level.
  steps_toplevel: 'stepcat(mini("a [b c] d e"), mini("x")).sound()',
  steps_marked: 'stepcat(mini("a [^b c] d e"), mini("x")).sound()',

  // "these two examples of `expand` sound exactly the same despite being
  // expanded by different amounts" — the step count changed, nothing else.
  expand2_alone: 'mini("c a f e").expand(2).note().sound("folkharp")',
  expand4_alone: 'mini("c a f e").expand(4).note().sound("folkharp")',

  // "You will hear a difference however, once you use another stepwise function"
  expand2_stepcat: 'stepcat(mini("c a f e").expand(2), mini("g d")).note().sound("folkharp")',
  expand4_stepcat: 'stepcat(mini("c a f e").expand(4), mini("g d")).note().sound("folkharp")',

  // "The first example has ten steps, and the second example has 18 steps, but
  // are then both played a rate of 8 steps per cycle."
  expand2_paced: 'stepcat(mini("c a f e").expand(2), mini("g d")).note().sound("folkharp").pace(8)',
  expand4_paced: 'stepcat(mini("c a f e").expand(4), mini("g d")).note().sound("folkharp").pace(8)',

  // "The argument to `expand` can also be patterned, and will be treated in a
  // stepwise fashion" — the expanded versions are stepcatted together.
  expand_patterned: 'note(mini("c a f e")).sound("folkharp").expand(mini("3 2 1 1 2 3"))',
  expand_patterned_paced: 'note(mini("c a f e")).sound("folkharp").expand(mini("3 2 1 1 2 3")).pace(8)',
};

// --- the `@example` of every function the page documents ---------------------
// Same code as the doc-example corpus, with bare string receivers spelled
// `mini(...)`. Comments upstream uses to state the equivalent spelling are kept
// as a second case wherever they name one, since "the same as X" is exactly the
// claim worth pinning.
const FUNCTIONS = {
  pace: 'sound(mini("bd sd cp")).pace(4)',
  pace_equiv: 'sound(mini("{bd sd cp}%4"))',

  stepcat_mini: 'stepcat(mini("bd sd cp"), mini("hh hh")).sound()',
  stepcat_mini_equiv: 'sound(mini("bd sd cp hh hh"))',
  stepcat_weighted: 'stepcat([3, mini("e3")], [1, mini("g3")]).note()',
  stepcat_weighted_equiv: 'note(mini("e3@3 g3"))',

  stepalt: 'stepalt([mini("bd cp"), mini("mt")], mini("bd")).sound()',
  stepalt_equiv: 'sound(mini("bd cp bd mt bd"))',

  expand: 'sound(mini("tha dhi thom nam")).bank("mridangam").expand(mini("3 2 1 1 2 3")).pace(8)',
  contract: 'sound(mini("tha dhi thom nam")).bank("mridangam").contract(mini("3 2 1 1 2 3")).pace(8)',

  extend: 'stepcat(sound(mini("bd bd - cp")).extend(2), sound(mini("bd - sd -"))).pace(8)',

  take_2: 'mini("bd cp ht mt").take(mini("2")).sound()',
  take_2_equiv: 'sound(mini("bd cp"))',
  take_123: 'mini("bd cp ht mt").take(mini("1 2 3")).sound()',
  take_123_equiv: 'sound(mini("bd bd cp bd cp ht"))',
  take_neg: 'mini("bd cp ht mt").take(mini("-1 -2 -3")).sound()',
  take_neg_equiv: 'sound(mini("mt ht mt cp ht mt"))',

  drop_1: 'mini("tha dhi thom nam").drop(mini("1")).sound().bank("mridangam")',
  drop_neg1: 'mini("tha dhi thom nam").drop(mini("-1")).sound().bank("mridangam")',
  drop_run: 'mini("tha dhi thom nam").drop(mini("0 1 2 3")).sound().bank("mridangam")',
  drop_run_neg: 'mini("tha dhi thom nam").drop(mini("0 -1 -2 -3")).sound().bank("mridangam")',

  polymeter: 'polymeter(mini("c eb g"), mini("c2 g2")).note()',
  polymeter_equiv: 'note(mini("{c eb g, c2 g2}%6"))',

  shrink_1: 'mini("tha dhi thom nam").shrink(mini("1")).sound().bank("mridangam")',
  shrink_neg1: 'mini("tha dhi thom nam").shrink(mini("-1")).sound().bank("mridangam")',
  shrink_alt: 'mini("tha dhi thom nam").shrink(mini("1 -1")).sound().bank("mridangam").pace(4)',
  shrink_run: 'note(mini("0 1 2 3 4 5 6 7")).sound("folkharp").shrink(mini("1 -1")).pace(8)',

  grow_1: 'mini("tha dhi thom nam").grow(mini("1")).sound().bank("mridangam")',
  grow_neg1: 'mini("tha dhi thom nam").grow(mini("-1")).sound().bank("mridangam")',
  grow_alt: 'mini("tha dhi thom nam").grow(mini("1 -1")).sound().bank("mridangam").pace(4)',
  grow_run: 'note(mini("0 1 2 3 4 5 6 7")).sound("folkharp").grow(mini("1 -1")).pace(8)',

  tour: 'mini("[c g]").tour(mini("e f"), mini("e f g"), mini("g f e c")).note().sound("folkharp").pace(8)',
  zip: 'zip(mini("e f"), mini("e f g"), mini("g [f e] a f4 c")).note().sound("folkharp").pace(8)',
};

const CASES = { ...PAGE, ...FUNCTIONS };
const CYCLES = 4;

const scope = { mini, ...core };

const names = Object.keys(scope);
const values = names.map((n) => scope[n]);

function evaluate(code) {
  // eslint-disable-next-line no-new-func
  return new Function(...names, `return (${code});`)(...values);
}

// `{wb, b, e, we, v}`, the shape `tunes_golden.json` uses, so both corpora are
// compared by the one set of helpers in `tests/common`. An analog hap has no
// whole; upstream's `show` prints the part for it, so the part is what is
// recorded.
function dump(pat) {
  return pat.queryArc(0, CYCLES).map((h) => ({
    wb: fracStr(h.whole ? h.whole.begin : h.part.begin),
    b: fracStr(h.part.begin),
    e: fracStr(h.part.end),
    we: fracStr(h.whole ? h.whole.end : h.part.end),
    v: normValue(h.value),
  }));
}

const cases = [];
for (const [id, code] of Object.entries(CASES)) {
  let haps;
  try {
    haps = dump(evaluate(code));
  } catch (err) {
    console.error(`! ${id}: ${err.message}`);
    continue;
  }
  cases.push({ id, code, haps });
}

writeJson('stepwise_golden.json', { cycles: CYCLES, cases }, 1);
console.error(
  `wrote ${cases.length}/${Object.keys(CASES).length} stepwise cases ` +
    `(${cases.reduce((n, c) => n + c.haps.length, 0)} haps)`,
);
