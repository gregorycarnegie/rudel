// Rank the functions in a samply profile by CPU time, with names resolved.
//
// samply saves raw addresses and symbolicates lazily in the browser, so reading
// profile.json.gz directly gives you nothing but `0x119373c`. This starts
// `samply load` headless and asks its /symbolicate/v5 endpoint for the names.
//
//   node hotspots.mjs [profile.json.gz] [--thread <substr>] [--module <substr>]
//                     [--top N] [--port P]
import { spawn } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { basename, dirname } from 'node:path';
import { gunzipSync } from 'node:zlib';

const opt = { top: 30, port: 3579 };
let file = 'profile.json.gz';
for (const argv = process.argv.slice(2); argv.length; ) {
  const a = argv.shift();
  if (a.startsWith('--')) opt[a.slice(2)] = argv.shift();
  else file = a;
}
const { thread: threadFilter, module: moduleFilter } = opt;
const top = Number(opt.top), port = Number(opt.port);

const raw = readFileSync(file);
const profile = JSON.parse(raw[0] === 0x1f && raw[1] === 0x8b ? gunzipSync(raw) : raw);
const libs = profile.libs ?? [];

// key = "<libIndex>:<relative address>"; -1 for frames with no module.
const self = new Map(), total = new Map(), threadCpu = new Map();
const keys = new Set();

for (const t of profile.threads) {
  const label = `${t.name} (${t.tid})`;
  if (threadFilter && !label.includes(threadFilter)) continue;
  const { frameTable: ft, funcTable: fn, stackTable: st, resourceTable: rt, samples: sm } = t;
  // frame -> key, resolved once per thread
  const frameKey = new Array(ft.length);
  for (let f = 0; f < ft.length; f++) {
    const res = fn.resource[ft.func[f]];
    const lib = res >= 0 ? rt.lib[res] : -1;
    frameKey[f] = `${lib}:${ft.address[f]}`;
    keys.add(frameKey[f]);
  }
  for (let i = 0; i < sm.length; i++) {
    // Sample count is meaningless here: the profile is full of idle samples
    // parked in NtWaitForMultipleObjects. CPU delta (µs) is the real weight.
    const cpu = sm.threadCPUDelta?.[i] ?? 0;
    if (!cpu) continue;
    threadCpu.set(label, (threadCpu.get(label) ?? 0) + cpu);
    let s = sm.stack[i];
    if (s == null) continue;
    self.set(frameKey[st.frame[s]], (self.get(frameKey[st.frame[s]]) ?? 0) + cpu);
    const seen = new Set();
    for (; s != null; s = st.prefix[s]) {
      const k = frameKey[st.frame[s]];
      if (seen.has(k)) continue; // recursion: count a frame once per stack
      seen.add(k);
      total.set(k, (total.get(k) ?? 0) + cpu);
    }
  }
}

// --- names, via a headless samply server ---------------------------------
// The profile records each PDB by bare filename, so samply looks for it in the
// cwd and finds nothing. Point it at the directory each module was loaded from.
const symbolDirs = [...new Set(libs.map((l) => dirname(l.path ?? '')))]
  .filter((d) => d && !/^[a-z]:[\\/]windows/i.test(d) && existsSync(d))
  .flatMap((d) => ['--symbol-dir', d]);
// shell: true would break on the space in "C:\Program Files\...", and some
// module paths in a Windows profile have one.
const srv = spawn(process.platform === 'win32' ? 'samply.exe' : 'samply',
  ['load', '-n', '--port', String(port), ...symbolDirs, file], { stdio: 'ignore' });
process.on('exit', () => srv.kill());

const root = `http://127.0.0.1:${port}`;
let token = null;
for (let i = 0; i < 100 && !token; i++) {
  try {
    const body = await (await fetch(root + '/')).text();
    token = body.match(/\/([0-9a-zA-Z]{16,})\/profile\.json/)?.[1] ?? null;
  } catch { /* not up yet */ }
  if (!token) await new Promise((r) => setTimeout(r, 200));
}
if (!token) throw new Error(`samply load did not come up on ${root}`);

const memoryMap = libs.map((l) => [l.debugName, l.breakpadId]);
const wanted = [...keys].filter((k) => !k.startsWith('-1:'));
const names = new Map();
for (let i = 0; i < wanted.length; i += 500) {
  const batch = wanted.slice(i, i + 500);
  const stack = batch.map((k) => k.split(':').map(Number));
  const res = await fetch(`${root}/${token}/symbolicate/v5`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ memoryMap, stacks: [stack] }),
  });
  const frames = (await res.json()).results?.[0]?.stacks?.[0] ?? [];
  frames.forEach((f, j) => { if (f?.function) names.set(batch[j], f); });
}
srv.kill();

// --- report ---------------------------------------------------------------
const modOf = (k) => libs[Number(k.split(':')[0])]?.name ?? '?';
const nameOf = (k) => {
  const f = names.get(k);
  if (!f) return `0x${Number(k.split(':')[1]).toString(16)}  [${modOf(k)}]`;
  return f.file ? `${f.function}  (${basename(f.file)}:${f.line})` : f.function;
};
const ms = (us) => (us / 1000).toFixed(0).padStart(8);
const grand = [...threadCpu.values()].reduce((a, b) => a + b, 0);
const pct = (us) => `${((100 * us) / grand).toFixed(1).padStart(5)}%`;

console.log(`${file} — ${(grand / 1e6).toFixed(1)} s of CPU across ${threadCpu.size} threads\n`);
console.log('CPU by thread');
for (const [t, us] of [...threadCpu].sort((a, b) => b[1] - a[1]).slice(0, 10)) {
  console.log(`  ${ms(us)} ms ${pct(us)}  ${t}`);
}

const show = (title, map) => {
  console.log(`\n${title}`);
  const rows = [...map].filter(([k]) => !moduleFilter || modOf(k).includes(moduleFilter))
    .sort((a, b) => b[1] - a[1]).slice(0, top);
  for (const [k, us] of rows) console.log(`  ${ms(us)} ms ${pct(us)}  ${nameOf(k)}`);
};
show(`Self time — top ${top}${moduleFilter ? ` in ${moduleFilter}` : ''}`, self);
show(`Total (inclusive) time — top ${top}${moduleFilter ? ` in ${moduleFilter}` : ''}`, total);
