// gen_examples_oracle.mjs — the Strudel documentation example corpus.
//
//   cd tools/oracle && node gen_examples_oracle.mjs
//
// Strudel's own `test/examples.test.mjs` walks the jsdoc `doc.json` and
// snapshots every `@example` block. `doc.json` is a build artefact and is not
// checked in, so — like `gen_reference_oracle.mjs` — this script reconstructs
// the same corpus by source-scanning the vendored packages for jsdoc blocks
// carrying an `@example`, naming each after its `@name` tag or the declaration
// that follows it (which is what jsdoc itself does).
//
// The emitted `examples_golden.json` is consumed by
// `crates/rudel-lang/tests/doc_examples.rs`, which evaluates each snippet with
// Rudel and queries a cycle. That is the "port or execute every runnable
// example from Strudel docs" item in FULL_STRUDEL.md: a snippet that stops
// evaluating is a regression, and a snippet Rudel cannot run has to be listed
// (with a reason) in `examples_allowlist.json`.

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { writeJson } from './lib.mjs';

const __dirname = dirname(fileURLToPath(import.meta.url));
const PACKAGES = resolve(__dirname, '../../strudel/packages');
const NL = String.fromCharCode(10);
const CR = String.fromCharCode(13);
const normalize = (text) => text.split(CR).join('');
function collectSources(dir) {
  const out = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    const st = statSync(full);
    if (st.isDirectory()) {
      if (entry === 'node_modules' || entry === 'dist' || entry === 'test') continue;
      out.push(...collectSources(full));
    } else if (/\.mjs$/.test(entry) && !/\.test\.mjs$/.test(entry)) {
      out.push(full);
    }
  }
  return out;
}

const packageOf = (file) => {
  const norm = file.replace(/\\/g, '/');
  const m = norm.match(/\/packages\/([^/]+)\//);
  return m ? m[1] : 'unknown';
};

/** Strip the leading ` * ` from a jsdoc body line. */
const stripStar = (line) => line.replace(/^\s*\*\s?/, '');

/**
 * Derive the documented name for a jsdoc block from the declaration that
 * follows it — jsdoc does the same when there is no explicit `@name`.
 */
function nameFromCode(after) {
  const line = after.split(NL).find((l) => l.trim().length) ?? '';
  return (
    line.match(/^\s*(?:export\s+)?(?:const|let|var|function|class)\s+([A-Za-z_$][\w$]*)/)?.[1] ??
    line.match(/^\s*(?:export\s+)?const\s*\{\s*([A-Za-z_$][\w$]*)/)?.[1] ??
    line.match(/^\s*(?:Pattern\.prototype\.)?([A-Za-z_$][\w$]*)\s*=/)?.[1] ??
    line.match(/^\s*register(?:Control)?\(\s*\[?\s*['"`]([^'"`]+)/)?.[1] ??
    null
  );
}

/**
 * Pull `{ name, examples[] }` out of every jsdoc block in `src`.
 * An `@example` runs until the next `@tag` or the end of the block, matching
 * how jsdoc itself terminates the tag.
 */
function parseBlocks(src) {
  const out = [];
  const re = /\/\*\*[\s\S]*?\*\//g;
  let m;
  while ((m = re.exec(src)) !== null) {
    const block = m[0];
    const lines = normalize(block).split(NL).map(stripStar);
    let name = null;
    const examples = [];
    let current = null;
    for (const line of lines) {
      const tag = line.match(/^\s*@(\w+)\s*(.*)$/);
      if (tag) {
        if (current !== null) {
          examples.push(current.join(NL).trim());
          current = null;
        }
        if (tag[1] === 'name') name = tag[2].trim();
        else if (tag[1] === 'example') current = tag[2].trim() ? [tag[2]] : [];
        continue;
      }
      // `*/` becomes a bare `/` once the leading star is stripped.
      if (['*/', '/', '/**'].includes(line.trim())) continue;
      if (current !== null) current.push(line);
    }
    if (current !== null) examples.push(current.join(NL).trim());
    const kept = examples.filter(Boolean);
    if (!kept.length) continue;
    name = name || nameFromCode(src.slice(m.index + block.length, m.index + block.length + 400));
    if (name) out.push({ name, examples: kept });
  }
  return out;
}

const cases = [];
const seen = new Set();
for (const file of collectSources(PACKAGES)) {
  const pkg = packageOf(file);
  for (const { name, examples } of parseBlocks(normalize(readFileSync(file, 'utf8')))) {
    examples.forEach((code, i) => {
      const id = `${name}#${i}`;
      // The same jsdoc block can appear in more than one build entry point;
      // keep the first occurrence so ids stay stable.
      if (seen.has(id)) return;
      seen.add(id);
      cases.push({ id, name, index: i, package: pkg, code });
    });
  }
}

cases.sort((a, b) => a.id.localeCompare(b.id));
writeJson('examples_golden.json', { cases }, 1);
console.log(`${cases.length} examples from ${new Set(cases.map((c) => c.package)).size} packages`);
