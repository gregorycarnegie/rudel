// strudel_diff.test.mjs — run a directory of patterns through *real* Strudel.
//
//   node ../../strudel/node_modules/vitest/vitest.mjs run \
//     --config tools/oracle/strudel_diff.config.mjs
//
// Emits `<OK|EMPTY|ERR>\t<id>\t<haps|message>` per pattern, which pairs with the
// same shape from rudel to say which failures are ours. Without a comparison
// like this an absolute pass rate over user-written patterns means nothing:
// plenty of them do not work in Strudel either.
//
// Environment: DIFF_CORPUS (directory of .js patterns), DIFF_OUT (tsv path),
// DIFF_FROM / DIFF_TO (index range, for sharding a long run).
//
// Setup is in the README's "Differential run" section — it needs more of the
// strudel workspace linked than the golden generators do.
//
// SPDX-License-Identifier: AGPL-3.0-or-later
import { it } from 'vitest';
import { evalScope } from '@strudel/core';
import { queryCode } from './test/runtime.mjs';
import { readdirSync, readFileSync, writeFileSync } from 'node:fs';

const CORPUS = process.env.DIFF_CORPUS;
const OUT = process.env.DIFF_OUT ?? 'strudel-diff.tsv';
const FROM = Number(process.env.DIFF_FROM ?? 0);
const TO = Number(process.env.DIFF_TO ?? 1e9);
const CYCLES = Number(process.env.DIFF_CYCLES ?? 8);

// Neutralise the browser, not the language: the REPL has these and node does
// not, and rudel likewise never fetches while evaluating. Anything left
// unstubbed is a real difference in what the two engines understand.
const noop = () => {};
const anoop = async () => {};
await evalScope({
  samples: anoop, initHydra: anoop, H: noop, hydra: noop, P5: noop, p5: noop,
  speak: noop, window: {}, document: {}, location: {},
  setGainCurve: noop, theme: noop, strudelMirror: {}, dough: noop, fetch: anoop,
});

it('runs the corpus through strudel', async () => {
  if (!CORPUS) throw new Error('set DIFF_CORPUS to a directory of .js patterns');
  const files = readdirSync(CORPUS).filter((f) => f.endsWith('.js')).sort();
  const lines = [];
  for (const f of files.slice(FROM, TO)) {
    const id = f.replace(/\.js$/, '');
    try {
      const haps = await queryCode(readFileSync(CORPUS + '/' + f, 'utf8'), CYCLES);
      lines.push(`${haps.length > 0 ? 'OK' : 'EMPTY'}\t${id}\t${haps.length}`);
    } catch (e) {
      const msg = String(e?.message ?? e).split('\n')[0].slice(0, 200);
      lines.push(`ERR\t${id}\t${msg}`);
    }
  }
  writeFileSync(OUT, lines.join('\n') + '\n');
  console.log('wrote', lines.length, 'results to', OUT);
});
