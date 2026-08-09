// gen_tunes_oracle.mjs — the full-song corpus, with Strudel's own expected haps.
//
//   cd tools/oracle && node gen_tunes_oracle.mjs
//
// The doc-example corpus (gen_examples_oracle.mjs) is thousands of one-liners.
// This is the other end of the scale: whole tunes, tens of lines each, as a
// live coder actually types them — `website/src/repl/tunes.mjs` is the corpus
// behind https://strudel.cc/examples, and `test/testtunes.mjs` is upstream's
// own fixture set of community songs. Both are plain string exports, so there
// is nothing to extract: import them and dump the sources.
//
// For the website tunes there is also an *oracle*. Upstream's own
// `tunes.test.mjs` snapshots `queryCode(tune, testCycles[key] ?? 1)` — every
// hap of every tune, with its spans and every control that reaches the synth —
// and commits the result. Re-running that here would mean a working Strudel
// runtime (transpiler + superdough + a Web Audio shim); parsing the committed
// snapshot instead needs nothing, and is upstream's stated expectation rather
// than our re-derivation of it.
//
// The emitted `tunes_golden.json` is consumed by
// `crates/rudel-lang/tests/tunes.rs`: every tune must run, and every tune
// carrying `haps` must reproduce them. Exceptions are named with a reason in
// `tunes_allowlist.json` (does not run) and `tunes_parity_allowlist.json`
// (runs, but does not yet match).

import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as siteTunes from '../../strudel/website/src/repl/tunes.mjs';
import * as testTunes from '../../strudel/test/testtunes.mjs';
import { writeJson } from './lib.mjs';

const __dirname = dirname(fileURLToPath(import.meta.url));
const STRUDEL = resolve(__dirname, '../../strudel');
const read = (p) => readFileSync(resolve(STRUDEL, p), 'utf8').split('\r').join('');

// --- upstream's per-tune query length ---------------------------------------
// `testCycles` in test/runtime.mjs; tunes absent from it are queried for one
// cycle, which is what `tunes.test.mjs` passes.
function readTestCycles() {
  const body = /export const testCycles = \{([\s\S]*?)\n\};/.exec(read('test/runtime.mjs'));
  const out = {};
  for (const [, name, n] of body[1].matchAll(/(\w+):\s*(\d+)/g)) out[name] = Number(n);
  return out;
}

// --- upstream's expected haps ------------------------------------------------
// One snapshot entry per tune, each line a `Hap.show(true)`:
//
//   [ -3/2 ⇜ (0/1 → 1/4) ⇝ 1/2 | gain:0.3 note:A4 s:flbass n:0 clip:1 ]
//
// `⇜`/`⇝` mark a whole extending past the part on that side, and the parens
// appear whenever the two differ at all. `show(true)` renders the value as
// `JSON.stringify(...)` with quotes and commas stripped, so a control map comes
// out as space-separated `key:value` — unambiguous as long as no value contains
// a space, which is asserted below rather than assumed.
const SPAN = /^(?:(\S+) ⇜ )?\(?(\S+) → (\S+?)\)?(?: ⇝ (\S+))?$/;

function parseValue(text) {
  if (text === 'true') return true;
  if (text === 'false') return false;
  const n = Number(text);
  return text !== '' && Number.isFinite(n) ? n : text;
}

/** One `show(true)` line -> `{wb, b, e, we, v}`, or null if it is not a plain
 *  span with a map of scalars (which the caller reports rather than guesses). */
function parseHap(line) {
  const bar = line.indexOf(' | ');
  if (!line.startsWith('[ ') || !line.endsWith(' ]') || bar < 0) return null;
  const span = SPAN.exec(line.slice(2, bar));
  if (!span) return null;
  const [, wb, b, e, we] = span;
  const body = line.slice(bar + 3, -2).trim();
  const v = {};
  if (body !== '') {
    for (const pair of body.split(' ')) {
      const colon = pair.indexOf(':');
      if (colon <= 0) return null; // a bare value, or a value with a space in it
      v[pair.slice(0, colon)] = parseValue(pair.slice(colon + 1));
    }
  }
  return { wb: wb ?? b, b, e, we: we ?? e, v };
}

function readSnapshot() {
  const snap = read('test/__snapshots__/tunes.test.mjs.snap');
  const out = {};
  const entry = /exports\[`renders tunes > tune: (\w+) 1`\] = `\n\[\n([\s\S]*?)\n\]\n`;/g;
  for (const [, name, body] of snap.matchAll(entry)) {
    const lines = body
      .split('\n')
      .map((l) => l.trim().replace(/,$/, ''))
      .filter(Boolean)
      .map((l) => l.replace(/^"|"$/g, ''));
    const haps = lines.map(parseHap);
    const bad = haps.findIndex((h) => h === null);
    if (bad >= 0) throw new Error(`${name}: cannot parse hap ${bad}: ${lines[bad]}`);
    out[name] = haps;
  }
  return out;
}

const cycles = readTestCycles();
const expected = readSnapshot();

const cases = [];
for (const [source, mod] of [
  ['examples', siteTunes],
  ['testtunes', testTunes],
]) {
  for (const [name, code] of Object.entries(mod)) {
    if (typeof code !== 'string') continue;
    const entry = { id: `${source}/${name}`, source, code, cycles: cycles[name] ?? 1 };
    // The snapshot covers the website tunes, which is what tunes.test.mjs
    // imports; the fixtures in test/testtunes.mjs have no upstream expectation.
    if (source === 'examples' && expected[name]) entry.haps = expected[name];
    cases.push(entry);
  }
}
cases.sort((a, b) => a.id.localeCompare(b.id));

writeJson('tunes_golden.json', { cases }, 1);
const withHaps = cases.filter((c) => c.haps);
console.log(
  `wrote ${cases.length} tunes, ${withHaps.length} with expected haps ` +
    `(${withHaps.reduce((n, c) => n + c.haps.length, 0)} in total)`,
);
