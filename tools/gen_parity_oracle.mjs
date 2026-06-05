// gen_parity_oracle.mjs — generate golden reference values for the rudel parity
// oracle (crates/rudel-core/tests/parity_oracle.rs).
//
// This deliberately *re-implements* Strudel's RNG and signal arithmetic inline,
// copied verbatim from strudel/packages/core/signal.mjs, so it has no npm
// dependencies and serves as an independent reference for the Rust port. JS
// bitwise operators act on int32, which the Rust port mirrors with wrapping i32.
//
//   node tools/gen_parity_oracle.mjs
//
// Paste the printed JSON into the GOLDEN constant of the Rust test.

// --- legacy RNG (signal.mjs) ------------------------------------------------
const __xorwise = (x) => {
  const a = (x << 13) ^ x;
  const b = (a >> 17) ^ a;
  return (b << 5) ^ b;
};
const __frac = (x) => x - Math.trunc(x);
const __timeToIntSeed = (x) => __xorwise(Math.trunc(__frac(x / 300) * 536870912));
const __intSeedToRand = (x) => (x % 536870912) / 536870912;
const timeToRand = (t) => Math.abs(__intSeedToRand(__timeToIntSeed(t)));

// --- perlin (signal.mjs _perlin, seed 0) ------------------------------------
const smootherStep = (x) => 6.0 * x ** 5 - 15.0 * x ** 4 + 10.0 * x ** 3;
const perlinAt = (t) => {
  const ta = Math.floor(t);
  const tb = ta + 1;
  const ra = timeToRand(ta);
  const rb = timeToRand(tb);
  return ra + smootherStep(t - ta) * (rb - ra);
};

// --- analytic signals (signal.mjs) ------------------------------------------
const saw = (t) => t - Math.floor(t);
const isaw = (t) => 1 - saw(t);
const sine = (t) => (Math.sin(2 * Math.PI * t) + 1) / 2;
const cosine = (t) => (Math.cos(2 * Math.PI * t) + 1) / 2;
const square = (t) => Math.floor(t * 2) % 2;

const N = 8;
const times = Array.from({ length: N }, (_, k) => k / N);

const out = {
  times,
  rand: times.map(timeToRand),
  perlin: times.map(perlinAt),
  saw: times.map(saw),
  isaw: times.map(isaw),
  sine: times.map(sine),
  cosine: times.map(cosine),
  square: times.map(square),
  // "0 1 .. 7".degradeBy(0.5): an event at onset k/8 survives when rand > 0.5.
  degrade_survivors: times.map((t, k) => (timeToRand(t) > 0.5 ? k : -1)).filter((k) => k >= 0),
};

console.log(JSON.stringify(out));
