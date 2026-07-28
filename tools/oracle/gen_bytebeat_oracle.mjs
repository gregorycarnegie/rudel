// gen_bytebeat_oracle.mjs — golden for the bytebeat expression evaluator.
//
//   cd tools/oracle && node gen_bytebeat_oracle.mjs
//
// Upstream compiles a bytebeat with `new Function(...)`, i.e. it runs the
// expression as real JavaScript (see `getByteBeatFunc` in
// strudel/packages/superdough/worklets.mjs). Rudel has no JS engine, so
// `crates/rudel-dsp/src/bytebeat.rs` carries its own parser/evaluator; this
// golden pins it against V8 for every built-in beat plus a set of expressions
// covering the operator surface (precedence, ToInt32 coercion, shift masking,
// ternaries, short-circuiting, Math calls).
//
// Each case records the raw expression value and `value & 255` (the byte the
// worklet actually turns into a sample) over a spread of `t`.

import { writeJson } from './lib.mjs';

// synth.mjs `registerSound('bytebeat')` defaultBeats, verbatim.
const defaultBeats = [
  '(t%255 >= t/255%255)*255',
  '(t*(t*8%60 <= 300)|(-t)*(t*4%512 < 256))+t/400',
  't',
  't*(t >> 10^t)',
  't&128',
  't&t>>8',
  '((t%255+t%128+t%64+t%32+t%16+t%127.8+t%64.8+t%32.8+t%16.8)/3)',
  '((t%64+t%63.8+t%64.15+t%64.35+t%63.5)/1.25)',
  '(t&(t>>7)-t)',
  '(sin(t*PI/128)*127+127)',
  '((t^t/2+t+64*(sin((t*PI/64)+(t*PI/32768))+64))%128*2)',
  '((t^t/2+t+64*(cos >> 0))%127.85*2)',
  '((t^t/2+t+64)%128*2)',
  '(((t * .25)^(t * .25)/100+(t * .25))%128)*2',
  '((t^t/2+t+64)%7 * 24)',
];

// Extra expressions exercising corners the built-in beats do not reach.
const extra = [
  '1+2*3',
  '(1+2)*3',
  '1+1>>1',
  '1|2^3&1',
  't&255',
  '1<<33',
  '-8>>1',
  '-1>>>28',
  '~t',
  't|0',
  't>0?t%64:255',
  '(t&1)&&(t>>3)',
  '(t&1)||(t>>3)',
  't%7==3',
  't%7!=3',
  'floor(t/64)*8',
  'int(t/64)*8',
  'abs(sin(t/128))*255',
  'pow(2,t%8)',
  'min(t%64,32)+max(t%16,4)',
  'sqrt(t)*4',
  '0xff&t',
  '1e2+t',
  't*.25',
  '(t>>4|t>>8)*(t>>16)',
  'round(t/100)',
];

// getByteBeatFunc, verbatim from worklets.mjs (minus the `chyx` helpers, which
// none of the built-in beats use).
let mathParams, byteBeatHelperFuncs;
function getByteBeatFunc(codetext) {
  if (mathParams == null) {
    mathParams = Object.getOwnPropertyNames(Math);
    byteBeatHelperFuncs = mathParams.map((k) => Math[k]);
    mathParams.push('int', 'window');
    byteBeatHelperFuncs.push(Math.floor, globalThis);
  }
  return new Function(...mathParams, 't', `return 0,\n${codetext || 0};`).bind(
    globalThis,
    ...byteBeatHelperFuncs,
  );
}

// A spread of `t` values: small integers, the fractional values the real
// `local_t = 256/sampleRate * freq * t` produces, and large ones that exercise
// the int32 wrap.
const TS = [];
for (let i = 0; i < 24; i++) TS.push(i);
for (let i = 0; i < 24; i++) TS.push(i * 37.3);
for (let i = 0; i < 12; i++) TS.push(i * 2553.7 + 0.5);
TS.push(4294967296 + 5, 2147483647, -12345.6);

const cases = {};
for (const [i, src] of defaultBeats.entries()) {
  cases[`default_${i}`] = evalCase(src);
}
for (const src of extra) {
  cases[`expr_${src}`] = evalCase(src);
}

function evalCase(src) {
  const f = getByteBeatFunc(src);
  const values = [];
  const bytes = [];
  for (const t of TS) {
    const v = f(t);
    // JSON has no NaN/Infinity; record them as null and skip on the Rust side.
    values.push(Number.isFinite(v) ? v : null);
    bytes.push(v & 255);
  }
  return { src, values, bytes };
}

writeJson('bytebeat_golden.json', { ts: TS, cases });
