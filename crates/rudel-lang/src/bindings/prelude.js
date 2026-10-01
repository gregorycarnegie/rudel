// The part of the prelude upstream writes in JavaScript over its methods,
// kept in JavaScript: what a native binding would only restate. Runs once per
// engine, after every native binding is in place.
// SPDX-License-Identifier: AGPL-3.0-or-later
(() => {
  const P = Pattern.prototype;
  // As the natives are defined: writable, configurable, not enumerable.
  const def = (object, name, value) =>
    Object.defineProperty(object, name, { value, writable: true, configurable: true });
  const curry = (arity, f) => {
    const c = (...a) => (a.length >= arity ? f(...a) : (...b) => c(...a, ...b));
    return c;
  };

  // `bind` and kin (pattern.mjs): map each value to a pattern, and flatten
  // the result the way the name says.
  for (const [bind, join] of [
    ['bind', 'join'],
    ['innerBind', 'innerJoin'],
    ['outerBind', 'outerJoin'],
    ['squeezeBind', 'squeezeJoin'],
    ['stepBind', 'stepJoin'],
    ['polyBind', 'polyJoin'],
  ]) {
    def(P, bind, function (func) {
      return this.fmap(func)[join]();
    });
    def(globalThis, bind, curry(2, (func, pat) => reify(pat)[bind](func)));
  }

  // `shrinklist(amount, pat)`: the list of views `shrink` concatenates.
  for (const name of ['shrinklist', 's_taperlist']) {
    def(globalThis, name, (amount, pat) => reify(pat)[name](amount));
  }

  // `seqPLoop([start, stop, pat], …)`: like `arrange`, but by start and stop
  // cycle, so sections may overlap. A two-element part starts where the
  // previous one stopped.
  def(globalThis, 'seqPLoop', (...parts) => {
    let total = Fraction(0);
    for (const part of parts) {
      if (part.length == 2) part.unshift(total);
      total = part[1];
    }
    return stack(
      ...parts.map(([start, stop, pat]) =>
        pure(reify(pat)).compress(Fraction(start).div(total), Fraction(stop).div(total)),
      ),
    )
      .slow(total)
      .innerJoin();
  });

  // Alignment getters (pattern.mjs `_setupAlignments`): `pat.add` is a
  // function that adds, and `pat.add.out` one that adds with the other
  // pattern's structure. A getter, so `room(1).keep.out` is a function bound
  // to its pattern even when passed on uncalled. Each cell of the matrix is a
  // native method (`add_out`); `mod` is bound as `modulo`.
  const ALIGNMENTS = {
    in: '',
    out: '_out',
    mix: '_mix',
    squeeze: '_squeeze',
    squeezein: '_squeeze',
    squeezeIn: '_squeeze',
    squeezeout: '_squeezeout',
    squeezeOut: '_squeezeout',
    reset: '_reset',
    restart: '_restart',
    poly: '_poly',
  };
  const OPS = { add: 'add', sub: 'sub', mul: 'mul', div: 'div', set: 'set', keep: 'keep',
    keepif: 'keepif', mod: 'modulo', modulo: 'modulo', pow: 'pow' };
  for (const [op, bound] of Object.entries(OPS)) {
    // Read before the getter replaces the plain method.
    const cells = Object.entries(ALIGNMENTS).map(([how, suffix]) => [how, P[bound + suffix]]);
    const plain = P[bound];
    Object.defineProperty(P, op, {
      configurable: true,
      get() {
        const pat = this;
        const wrapper = (...args) => plain.apply(pat, args);
        for (const [how, method] of cells) {
          wrapper[how] = (...args) => method.apply(pat, args);
        }
        return wrapper;
      },
    });
  }

  // `pat._fast(2)`: every method `register` makes also has an unpatterned
  // twin under a leading underscore. Given plain arguments the patterned one
  // does the same thing, so any `_name` nothing else defines is `name`.
  // `_steps` is not a method upstream but the step count, which scripts read.
  Object.setPrototypeOf(
    P,
    new Proxy(Object.getPrototypeOf(P), {
      get(target, key, receiver) {
        if (typeof key === 'string' && key[0] === '_' && key[1] !== '_' && key !== '_steps') {
          const method = receiver[key.slice(1)];
          if (typeof method === 'function') return method;
        }
        return Reflect.get(target, key, receiver);
      },
    }),
  );

  // `tokenizeNote('c#4')` (core/util.mjs): a note name's letter, accidentals
  // and octave.
  def(globalThis, 'tokenizeNote', (note) => {
    if (typeof note !== 'string') return [];
    const [pc, acc = '', oct] = note.match(/^([a-gA-G])([#bsf]*)(-?[0-9]*)$/)?.slice(1) || [];
    if (!pc) return [];
    return [pc, acc, oct ? Number(oct) : undefined];
  });
})();
