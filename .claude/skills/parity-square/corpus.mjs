// Fetch the public patterns people have shared on strudel.cc into a corpus
// directory, one `<id>.js` per pattern.
//
//   node .claude/skills/parity-square/corpus.mjs OUTDIR [--all]
//
// The database is the REPL's own: `code_v1` in the Supabase project named in
// `strudel/website/src/repl/util.mjs`, read with the anon key committed
// alongside it — the same request the site makes to load a shared link. Only
// rows marked public are taken unless `--all` is passed.
//
// SPDX-License-Identifier: AGPL-3.0-or-later
import { mkdirSync, writeFileSync } from 'node:fs';

const BASE = 'https://pidxdsxphlhzjnzmifth.supabase.co/rest/v1/code_v1';
const KEY =
  'eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6InBpZHhkc3hwaGxoempuem1pZnRoIiwicm9sZSI6ImFub24iLCJpYXQiOjE2NTYyMzA1NTYsImV4cCI6MTk3MTgwNjU1Nn0.bqlw7802fsWRnqU5BLYtmXk_k-D1VFmbkHMywWc15NM';

const out = process.argv[2];
if (!out) throw new Error('usage: corpus.mjs OUTDIR [--all]');
const publicOnly = !process.argv.includes('--all');
mkdirSync(out, { recursive: true });

// Paginate: the API caps a response at 1000 rows however large the limit.
const patterns = new Map();
for (let offset = 0; ; offset += 1000) {
  const query = [`select=id,code`, `limit=1000`, `offset=${offset}`, `order=id`];
  if (publicOnly) query.push('public=eq.true');
  const response = await fetch(`${BASE}?${query.join('&')}`, {
    headers: { apikey: KEY, Authorization: `Bearer ${KEY}` },
  });
  if (!response.ok) throw new Error(`${response.status} ${await response.text()}`);
  const rows = await response.json();
  for (const row of rows) {
    if (typeof row.code === 'string' && row.code.trim()) patterns.set(row.id, row.code);
  }
  process.stderr.write(`\rfetched ${patterns.size}`);
  if (rows.length < 1000) break;
}

for (const [id, code] of patterns) writeFileSync(`${out}/${id}.js`, code);
const wide = [...patterns.values()].filter((c) => /[^\x00-\x7f]/.test(c)).length;
console.log(`\n${patterns.size} patterns -> ${out} (${wide} contain non-ASCII)`);
