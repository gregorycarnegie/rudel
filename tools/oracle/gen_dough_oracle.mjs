// gen_dough_oracle.mjs — the dough engine as the browser runs it.
//
//   cd tools/oracle && node gen_dough_oracle.mjs
//
// Rudel's `.dough()` compiles dough's C source natively (crates/rudel-dough).
// The browser runs the same source compiled to WebAssembly, with its own
// maths routines in place of the C library's. This renders a set of events
// through that WebAssembly build, `dough.wasm` from the `dough-synth` package
// Strudel pins, so `crates/rudel-dough/tests/wasm_golden.rs` can render the
// same events natively and measure how far apart the two are.
//
// The wasm is fetched from the npm registry and checked against a pinned hash,
// rather than installed: `npm install` here would prune the `@strudel`
// junctions the other generators need. No `npm install` is required.
//
// The engine is laid out once and reset between cases (`dough/reset`), as
// Rudel reuses its one engine, so state that a reset leaves alone (the noise
// generator's seed, registered files) carries from case to case identically on
// both sides. The test runs the cases in this order for that reason.

import { createHash } from 'node:crypto';
import { gunzipSync } from 'node:zlib';
import { writeJson } from './lib.mjs';

const VERSION = '0.4.1';
const TARBALL = `https://registry.npmjs.org/dough-synth/-/dough-synth-${VERSION}.tgz`;
const WASM_SHA256 = 'c1f4afab909a816779c76911a4a4f2420663db955d49f3702ca529a420938880';

// What dough.js hands the engine, and what rudel-dough hands it too.
const SAMPLE_RATE = 44100;
const BLOCK_SIZE = 128;
const CHANNELS = 2;
const [MAX_VOICES, MAX_ORBITS, MAX_DELAY_TIME, MAX_EVENTS, MAX_FILES] = [32, 1, 1, 64, 128];
const MAX_PCM = (1024 * 1024 * 196) / 4;

// A short decaying tone, as mono PCM, for the sample case.
const SAMPLE_PCM = Array.from({ length: 2000 }, (_, i) => Math.fround(Math.sin(i * 0.05) * (1 - i / 2000)));

// `frames` is rounded up to whole blocks. Messages without a `time` pair take
// effect at the first frame; a `time` is seconds on the engine's clock.
const CASES = [
  { id: 'sine', messages: ['dough/play/s/sine/freq/330/duration/0.04'] },
  {
    id: 'tri_adsr',
    messages: ['dough/play/s/tri/freq/220/attack/0.01/decay/0.02/sustain/0.5/release/0.02/duration/0.05'],
  },
  { id: 'saw_lpf', messages: ['dough/play/s/saw/freq/110/lpf/800/lpq/0.6/duration/0.08'] },
  { id: 'pulse_pw', messages: ['dough/play/s/pulse/freq/150/pw/0.3/duration/0.06'] },
  { id: 'zaw', messages: ['dough/play/s/zaw/freq/200/duration/0.04'] },
  { id: 'fm', messages: ['dough/play/s/sine/freq/220/fm/2/fmh/1.5/duration/0.08'] },
  { id: 'vibrato', messages: ['dough/play/s/saw/note/57/vib/6/vibmod/0.5/duration/0.08'] },
  { id: 'hpf_bpf', messages: ['dough/play/s/saw/freq/90/hpf/400/bpf/1200/bpq/2/duration/0.06'] },
  { id: 'crush_coarse_distort', messages: ['dough/play/s/tri/freq/140/crush/4/coarse/3/distort/0.5/duration/0.06'] },
  { id: 'pan_gain', messages: ['dough/play/s/sine/freq/500/pan/0.2/gain/0.5/duration/0.04'] },
  {
    id: 'chord',
    messages: [
      'dough/play/s/sine/note/60/duration/0.05',
      'dough/play/s/sine/note/64/duration/0.05',
      'dough/play/s/sine/note/67/duration/0.05',
    ],
  },
  {
    id: 'glide_in_group',
    messages: [
      'dough/play/s/saw/freq/220/group/1/voices/1/duration/0.1',
      'dough/play/s/saw/freq/330/group/1/voices/1/glide/0.02/time/0.03/duration/0.05',
    ],
  },
  {
    id: 'update_group',
    messages: [
      'dough/play/s/saw/freq/110/group/2/lpf/300/duration/0.08',
      'dough/update_group/group/2/lpf/3000/time/0.03',
    ],
  },
  { id: 'white_noise', messages: ['dough/play/s/white/duration/0.03'] },
  {
    id: 'delay',
    frames: 8192,
    messages: ['dough/play/s/sine/freq/440/duration/0.02/delay/0.6/delaytime/0.03/delayfeedback/0.5'],
  },
  { id: 'room', frames: 8192, messages: ['dough/play/s/saw/freq/180/duration/0.03/room/0.5'] },
  { id: 'sample', file: true, messages: ['dough/play/s/smp:0/file/0/speed/1.5/duration/0.05'] },
];
const DEFAULT_FRAMES = 4096;

/// `package/dough.wasm` out of the gzipped tarball.
async function fetchWasm() {
  const res = await fetch(TARBALL);
  if (!res.ok) throw new Error(`fetching ${TARBALL}: ${res.status}`);
  const tar = gunzipSync(Buffer.from(await res.arrayBuffer()));
  for (let at = 0; at + 512 <= tar.length; ) {
    const name = tar.toString('latin1', at, at + 100).replace(/\0.*$/s, '');
    if (!name) break;
    const size = parseInt(tar.toString('latin1', at + 124, at + 136).trim(), 8) || 0;
    if (name === 'package/dough.wasm') {
      const wasm = tar.subarray(at + 512, at + 512 + size);
      const hash = createHash('sha256').update(wasm).digest('hex');
      if (hash !== WASM_SHA256) throw new Error(`dough.wasm hash ${hash}, expected ${WASM_SHA256}`);
      return wasm;
    }
    at += 512 + Math.ceil(size / 512) * 512;
  }
  throw new Error('no package/dough.wasm in the tarball');
}

const memory = new WebAssembly.Memory({ initial: 3200, maximum: 3200, shared: true });
const text = (pointer) => {
  const bytes = new Uint8Array(memory.buffer);
  let end = pointer;
  while (bytes[end] !== 0) end++;
  return Buffer.from(bytes.slice(pointer, end)).toString('latin1');
};
let mirror;
const { instance } = await WebAssembly.instantiate(await fetchWasm(), {
  env: {
    memory,
    js_panic: (pointer) => {
      throw new Error(`dough panicked: ${text(pointer)}`);
    },
    // At startup the engine reports where its buffers live, as `?key=pointer&…`.
    js_init: (pointer) => {
      mirror = Object.fromEntries(
        text(pointer)
          .slice(1)
          .split('&')
          .filter(Boolean)
          .map((pair) => pair.split('='))
          .map(([key, at]) => [key, Number(at)]),
      );
    },
  },
});
const dough = instance.exports;
dough.dough_init(SAMPLE_RATE, MAX_VOICES, MAX_ORBITS, MAX_DELAY_TIME, MAX_PCM, MAX_EVENTS, MAX_FILES);

/// dough.js's `scheduleEvent`: the message, NUL-terminated, into `event_input`.
function evaluate(message) {
  const bytes = Buffer.from(`${message}\0`, 'latin1');
  new Uint8Array(memory.buffer, mirror.event_input, bytes.length).set(bytes);
  dough.evaluate_input();
}

/// dough.js's server-side `load_file`: write the PCM where the engine wants
/// it, then register it in place.
function registerFile(index, pcm) {
  const head = dough.dough_fits_pcm(index, pcm.length);
  if (head < 0) throw new Error('no room for the sample');
  new Float32Array(memory.buffer, mirror.pcm + head * 4, pcm.length).set(pcm);
  if (dough.dough_register_pcm(index, mirror.pcm + head * 4, pcm.length, 1, 65.46) !== 0) {
    throw new Error('registering the sample failed');
  }
}

const f32 = (x) => Number(Math.fround(x).toPrecision(9));
const cases = CASES.map((c, i) => {
  if (i > 0) evaluate('dough/reset');
  if (c.file) registerFile(0, SAMPLE_PCM);
  c.messages.forEach(evaluate);
  const blocks = Math.ceil((c.frames ?? DEFAULT_FRAMES) / BLOCK_SIZE);
  const left = [];
  const right = [];
  for (let b = 0; b < blocks; b++) {
    dough.dsp();
    const out = new Float32Array(memory.buffer, mirror.output, BLOCK_SIZE * CHANNELS);
    for (let f = 0; f < BLOCK_SIZE; f++) {
      left.push(f32(out[f * 2]));
      right.push(f32(out[f * 2 + 1]));
    }
  }
  const peak = Math.max(...left.map(Math.abs), ...right.map(Math.abs));
  if (peak === 0) throw new Error(`case ${c.id} is silent`);
  return { id: c.id, messages: c.messages, file: c.file ?? false, left, right };
});

writeJson(
  'dough_golden.json',
  { version: VERSION, sampleRate: SAMPLE_RATE, blockSize: BLOCK_SIZE, samplePcm: SAMPLE_PCM.map(f32), cases },
  0,
);
console.error(`wrote dough_golden.json (${cases.length} cases from dough-synth ${VERSION})`);
