// worklet_harness.mjs - run superdough's own AudioWorklet processors in Node.
//
// Reads strudel/packages/superdough/worklets.mjs at run time, so an oracle
// built on this exercises upstream's code as it stands, not a copy of it. The
// module's imports are stubbed (nothing used by the plain processors needs
// them), `export` is dropped, and the AudioWorklet globals are provided:
// `sampleRate`, a `currentTime` the harness advances, `AudioWorkletProcessor`
// and `registerProcessor`.
//
//   const { render } = loadWorklets(44100);
//   render('pulse-oscillator', { frequency: 220, pulsewidth: 0.5 }, 2048);
// SPDX-License-Identifier: AGPL-3.0-or-later

import { readFileSync } from 'node:fs';

const SOURCE = new URL('../../strudel/packages/superdough/worklets.mjs', import.meta.url);
const QUANTUM = 128;

export function loadWorklets(sampleRate) {
  const text = readFileSync(SOURCE, 'utf8')
    .replace(/^import .*$/gm, '')
    .replace(/^export /gm, '');
  const processors = new Map();
  const clock = { currentTime: 0 };
  const body = `
    const OLAProcessor = class {};
    const FFT = class {};
    const getDistortionAlgorithm = () => (x) => x;
    const ugens = {};
    ${text}
  `;
  // `currentTime` is read as a bare global inside the processors.
  const scope = {
    sampleRate,
    AudioWorkletProcessor: class {
      constructor() {
        this.port = { onmessage: null, postMessage() {} };
      }
    },
    registerProcessor: (name, cls) => processors.set(name, cls),
  };
  Object.defineProperty(scope, 'currentTime', { get: () => clock.currentTime });
  const names = Object.keys(scope);
  // eslint-disable-next-line no-new-func
  new Function(...names, `with (this) { ${body} }`).call(scope, ...names.map((n) => scope[n]));

  /**
   * Render `frames` samples of processor `name` from time 0, one 128-frame
   * quantum at a time. `params` holds constant values, or functions of the
   * frame index for a-rate automation. Returns channel 0.
   */
  function render(name, params, frames, options = {}) {
    const Processor = processors.get(name);
    if (!Processor) throw new Error(`no processor ${name}`);
    const processor = new Processor(options);
    const out = new Float32Array(frames);
    clock.currentTime = 0;
    for (let start = 0; start < frames; start += QUANTUM) {
      // Upstream's processors skip a block whose start is not after `begin`.
      clock.currentTime = (start + QUANTUM) / sampleRate;
      // Each declared param at its default, as an AudioWorkletNode provides.
      const defaults = Object.fromEntries(
        (Processor.parameterDescriptors ?? []).map((d) => [d.name, d.defaultValue ?? 0]),
      );
      const values = {};
      for (const [key, v] of Object.entries({ ...defaults, begin: 0, end: 1e9, ...params })) {
        values[key] =
          typeof v === 'function'
            ? Float32Array.from({ length: QUANTUM }, (_, i) => v(start + i))
            : Float32Array.of(v);
      }
      const channels = [new Float32Array(QUANTUM), new Float32Array(QUANTUM)];
      processor.process([[]], [channels], values);
      out.set(channels[0].subarray(0, Math.min(QUANTUM, frames - start)), start);
    }
    return Array.from(out);
  }
  return { render, processors };
}
