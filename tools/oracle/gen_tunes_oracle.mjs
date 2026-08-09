// gen_tunes_oracle.mjs — the full-song corpus.
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
// The emitted `tunes_golden.json` is consumed by
// `crates/rudel-lang/tests/tunes.rs`, which evaluates each tune and queries
// several cycles. Anything Rudel cannot run has to be named (with a reason) in
// `tunes_allowlist.json`.

import * as siteTunes from '../../strudel/website/src/repl/tunes.mjs';
import * as testTunes from '../../strudel/test/testtunes.mjs';
import { writeJson } from './lib.mjs';

const cases = [];
for (const [source, mod] of [
  ['examples', siteTunes],
  ['testtunes', testTunes],
]) {
  for (const [name, code] of Object.entries(mod)) {
    if (typeof code !== 'string') continue;
    cases.push({ id: `${source}/${name}`, source, code });
  }
}
cases.sort((a, b) => a.id.localeCompare(b.id));

writeJson('tunes_golden.json', { cases }, 1);
console.log(`wrote ${cases.length} tunes`);
