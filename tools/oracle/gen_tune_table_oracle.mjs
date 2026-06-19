// gen_tune_table_oracle.mjs — exhaustive tune.js scale-table parity golden.
//
//   cd tools/oracle && node gen_tune_table_oracle.mjs
//
// For every scale in tune.js's archive, dump the runtime note values
// `tune.note(0..length)` with tonic 1 (i.e. the per-degree ratios, including the
// octave). `crates/rudel-core/tests/tune_table_parity.rs` rebuilds each with
// rudel's `tune(name)` and compares within tolerance, verifying both the
// generated `tune_table.rs` data and rudel's ratio derivation against tune.js
// for the whole archive.

import { writeFileSync } from 'node:fs';
import Tune from '@strudel/xen/tunejs.js';

// All archive scale names. `search('')` matches every key in tune.js's private
// TuningList (empty substring matches all).
const names = new Tune().search('');

const out = {};
for (const name of names) {
  const t = new Tune();
  if (!t.isValidScale(name)) continue;
  t.loadScale(name);
  t.tonicize(1);
  const length = t.scale.length;
  if (!length) continue;
  // degrees 0..length inclusive: 0..length-1 are the ratios, length is the octave.
  const ratios = [];
  for (let i = 0; i <= length; i++) ratios.push(t.note(i));
  out[name] = ratios;
}

writeFileSync(new URL('./tune_table_golden.json', import.meta.url), JSON.stringify(out));
console.error(`wrote tune_table_golden.json (${Object.keys(out).length} scales)`);
