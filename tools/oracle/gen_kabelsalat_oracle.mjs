// gen_kabelsalat_oracle.mjs — run kabelsalat patches through the real
// kabelsalat and dump both the compiled graph and the audio it produces, for
// the rudel parity tests.
//
//   cd tools/oracle && node gen_kabelsalat_oracle.mjs
//
// Reads `@kabelsalat/lib` out of the vendored Strudel checkout, which is what
// superdough itself loads (`strudel/packages/superdough/package.json` pins
// `^0.4.1`). The version lands in the golden and a test asserts it, so a bump
// is reviewable rather than silent.
//
// Two things come out per patch, because rudel splits the work across two
// crates that never meet:
//
//   graph    the topologically sorted node list — what rudel-lang's compiler
//            has to agree with (`crates/rudel-lang/src/tests/kabelsalat.rs`)
//   samples  the audio, sample for sample — what rudel-dsp's interpreter has
//            to agree with (`crates/rudel-dsp/tests/kabelsalat_golden.rs`)
//
// The audio is produced by replaying `GenericProcessor`'s own loop
// (strudel/packages/superdough/worklets.mjs): compile the graph to JavaScript,
// build one ugen instance per node, and call the generated function once per
// sample over the `r`/`o`/`s` register arrays. That is exactly what the browser
// runs, minus the AudioWorklet wrapper — which is the only reason this can be
// done in node at all.
//
// Patches using `noise`, `pink`, `brown` or `dust` are deliberately absent:
// they call `Math.random()` upstream, so there is nothing stable to pin.
// `lcgnoise` is here precisely because it is the one noise source that is
// deterministic on both sides.

import { readFileSync } from 'node:fs';
import { registerHooks } from 'node:module';
import { writeJson } from './lib.mjs';

const SAMPLE_RATE = 44100;
const FRAMES = 64;

// `ugens.js` reads a bare `sampleRate`, which is an AudioWorklet global. It has
// to exist before the module is imported, since `SawOsc` closes over it.
globalThis.sampleRate = SAMPLE_RATE;

// `SawOsc` starts at `Math.random()`, so an unpinned run is unreproducible even
// against itself. Rudel starts it at zero deliberately — a patch should sound
// the same twice — so the reference is pinned to the same choice. This is the
// only thing in the generator that departs from upstream as shipped, and it
// departs towards determinism rather than away from it.
Math.random = () => 0;

const LIB = new URL(
  '../../strudel/node_modules/@kabelsalat/lib/',
  import.meta.url,
);

const PINNED = JSON.parse(readFileSync(new URL('package.json', LIB), 'utf8')).version;

// `@kabelsalat/lib` imports `@kabelsalat/core` by bare specifier, and that
// package's `main` is a browser IIFE bundle — vite reads its `module` field, but
// node does not, so the import fails with "does not provide an export named
// 'Node'". Point the specifier at the ESM source instead. A resolve hook rather
// than an edit, because the vendored tree has to stay as Strudel ships it.
const CORE = new URL('../../strudel/node_modules/@kabelsalat/core/src/', import.meta.url);
registerHooks({
  resolve(spec, ctx, next) {
    if (spec === '@kabelsalat/core') {
      return next(new URL('index.js', CORE).href, ctx);
    }
    return next(spec, ctx);
  },
});

const kabel = await import(new URL('src/lib.js', LIB));
const ugens = await import(new URL('src/ugens.js', LIB));
const core = await import(new URL('index.js', CORE));
const { evaluate } = core;

// `evaluate` builds a function whose parameters are the scope's keys, so the
// scope has to name everything a patch can. `lib` re-exports most of `core` but
// not all of it — `n` and `poly` live only in core.
const SCOPE = { ...core, ...kabel };

const UGENS = new Map(Object.entries(ugens));

// The patches. Each is kabelsalat source, evaluated in a scope holding the
// library — the same way `SalatRepl` does it.
const PATCHES = {
  // The shapes a patch is actually built out of.
  sine: 'sine(4800).mul(.5).out()',
  saw: 'saw(1200).mul(.5).out()',
  tri: 'tri(1200).out()',
  pulse: 'pulse(1200, .25).out()',
  zaw: 'zaw(1200).out()',
  impulse: 'impulse(400).out()',
  clock: 'clock(120).out()',
  clockdiv: 'clock(240).clockdiv(4).out()',

  // Envelopes and the triggers that drive them.
  adsr: 'impulse(200).adsr(.001, .002, .5, .003).out()',
  perc: 'impulse(200).perc(.005).out()',
  ad: 'impulse(200).ad(.001, .004).out()',
  seq: 'impulse(400).seq(110, 220, 330).out()',
  hold: 'saw(600).hold(impulse(400)).out()',

  // Filters.
  filter: 'saw(1200).lpf(.3, .2).out()',
  bpf: 'saw(1200).bpf(.4, .1).out()',
  hpf: 'saw(1200).hpf(.3).out()',
  qf: 'saw(1200).qf(0, 900, 3, 1).out()',
  qlpf: 'saw(1200).qlpf(900, 8).out()',

  // Shaping.
  distort: 'sine(1200).distort(.7).out()',
  fold: 'sine(1200).fold(1.5).out()',
  clip: 'sine(1200).clip(-.4, .4).out()',
  lag: 'impulse(400).lag(.01).out()',
  slew: 'impulse(400).slew(500, 900).out()',
  slide: 'impulse(400).slide(.4).out()',

  // Arithmetic, which compiles to inline expressions rather than ugens.
  math: 'sine(1200).mul(2).add(.1).clamp(-.5, .5).out()',
  range: 'sine(600).range(.2, .8).out()',
  remap: 'sine(600).remap(-1, 1, 0, 10).out()',
  midinote: 'n(69).midinote().div(4410).out()',
  logic: 'greater(sine(600), 0).out()',
  argmin: 'argmin(saw(600), saw(900), saw(1500)).div(4).out()',
  pick: 'n(1).pick(saw(600), tri(600), pulse(600)).out()',

  // The one noise source that is deterministic upstream too.
  lcgnoise: 'rng(1, 0).out()',
  // No `bytebeat`, `floatbeat` or `raw` case: all three are broken upstream.
  // Their compile step emits `const ${name} = ...` where `name` is the
  // register expression `r[3]`, so the generated source is `const r[3] = ...`
  // — a SyntaxError that takes the whole graph down, not just the node. Rudel
  // runs them (rudel-dsp's `Coded`, on the evaluator the bytebeat synth
  // already carries), so there is nothing to pin them against until upstream
  // emits an assignment there instead of a declaration.

  // Multichannel: the expansion, and folding it back down.
  poly: 'sine([1200, 1800]).out()',
  mixmono: 'sine([1200, 1800]).mix().out()',
  mixstereo: 'sine([600, 1200, 1800]).mix(2).out()',
  pan: 'sine(1200).pan(.5).out()',

  // Feedback: the whole reason the graph may be cyclic.
  feedback: 'impulse(200).add(x => x.delay(.001).mul(.7)).out()',
  delay: 'impulse(200).delay(.0005).out()',
};

/// Replay `GenericProcessor` for one compiled graph.
function render(node) {
  const { src, ugens: schema, registers } = node.compile({ lang: 'js' });
  const nodes = schema.map((ugen, i) => {
    const cls = UGENS.get(ugen.type);
    if (!cls) {
      throw new Error(`no ugen class for "${ugen.type}"`);
    }
    return new cls(i, ugen, SAMPLE_RATE, () => {});
  });
  const genSample = new Function(
    'time',
    'nodes',
    'input',
    'r',
    'o',
    's',
    `o.fill(0); // reset outputs\n${src}`,
  );
  const r = new Array(registers).fill(0);
  const o = new Array(16).fill(0);
  const s = new Array(16).fill(0);
  const left = [];
  const right = [];
  let playPos = 0;
  for (let i = 0; i < FRAMES; i++) {
    genSample(playPos, nodes, 0, r, o, s);
    left.push(o[0]);
    right.push(o[1]);
    playPos += 1 / SAMPLE_RATE;
  }
  return { left, right };
}

/// The topologically sorted node list, in the order the compiler assigns
/// registers — upstream's `topoSort`, re-run here so the golden records the
/// order rather than inferring it from the generated source.
function graph(node) {
  const sorted = [];
  const seen = new Set();
  (function dfs(n) {
    if (typeof n !== 'object' || seen.has(n)) return;
    seen.add(n);
    for (const i in n.ins) dfs(n.ins[i]);
    sorted.push(n);
  })(node);
  // `evaluate` wraps the outputs in an `exit` node and gives each channel an
  // `output` node with a constant for its index. Rudel keeps its outputs in a
  // side table, so those three kinds of node exist only on this side; dropping
  // them leaves the graph proper, which is what the two compilers can be held
  // to. A constant is dropped only when nothing but an `output` reads it.
  const isPlumbing = (n) => n.type === 'exit' || n.type === 'output';
  const readByGraph = new Set();
  for (const n of sorted) {
    if (isPlumbing(n)) continue;
    for (const child of n.ins) readByGraph.add(child);
  }
  const outputChannels = new Set(
    sorted.filter((n) => n.type === 'output').flatMap((n) => n.ins.slice(1)),
  );
  const kept = sorted.filter(
    (n) => !isPlumbing(n) && !(outputChannels.has(n) && !readByGraph.has(n)),
  );
  return kept.map((n) => ({
    type: n.type,
    value: n.value === undefined ? null : n.value,
    ins: n.ins.map((child) => kept.indexOf(child)),
  }));
}

const cases = [];
for (const [name, source] of Object.entries(PATCHES)) {
  // `evaluate` collects whatever the source sent to `out()` and wraps it in an
  // `exit` node, which is the root the compiler is handed.
  const node = evaluate(source, { ...SCOPE });
  cases.push({ name, source, graph: graph(node), ...render(node) });
}

writeJson('kabelsalat_golden.json', {
  version: PINNED,
  sampleRate: SAMPLE_RATE,
  frames: FRAMES,
  cases,
});
console.log(`kabelsalat ${PINNED}: ${cases.length} patches, ${FRAMES} frames each`);
