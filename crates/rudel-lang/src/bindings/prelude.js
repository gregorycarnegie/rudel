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

  // Numbered REPL slots (repl.mjs): `pat.d1`/`pat.p1` are getters that
  // register the pattern as slot 1; `pat.q1` is silence.
  for (let i = 1; i < 10; ++i) {
    for (const name of [`d${i}`, `p${i}`]) {
      Object.defineProperty(P, name, {
        get() {
          return this.p(i);
        },
        configurable: true,
      });
    }
    def(P, `q${i}`, silence);
  }

  // In the browser REPL `window` is the global object, and tunes use it as a
  // namespace shared between blocks (`window.spag = …`).
  def(globalThis, 'window', globalThis);
  // A tune that sees `window` may schedule frames. There is no canvas to
  // paint, so a requested frame never comes.
  def(globalThis, 'requestAnimationFrame', () => 0);
  def(globalThis, 'cancelAnimationFrame', () => {});

  // `initHydra()` (@strudel/hydra) loads hydra-synth, which puts its sources
  // and outputs (`osc`, `src`, `o0`, `render`, …) in global scope, over
  // Strudel's own `osc`, `noise` and `shape`, as upstream. Until then they
  // are on `Hydra`.
  // hydra's external sources `s0`..`s3`, indices 4..7 past the four outputs.
  // An image loads (`initImage`); there is no camera, video or screen
  // capture, so those `init*` do nothing and the source reads as empty.
  const sourceIndex = (s) => (s !== null && typeof s === 'object' && 'index' in s ? s.index : s);
  const hydraSrc = Hydra.src;
  Hydra.src = (s, ...rest) => hydraSrc(sourceIndex(s), ...rest);
  for (let i = 0; i < 4; i++) {
    const none = function () {
      return this;
    };
    Hydra[`s${i}`] = { index: 4 + i, init: none, initCam: none, initVideo: none,
      initScreen: none, initStream: none, clear: none,
      initImage(url = '') {
        Hydra._image(i, url);
        return this;
      },
    };
  }
  const EASINGS = ['linear', 'easeInQuad', 'easeOutQuad', 'easeInOutQuad', 'easeInCubic',
    'easeOutCubic', 'easeInOutCubic', 'easeInQuart', 'easeOutQuart', 'easeInOutQuart',
    'easeInQuint', 'easeOutQuint', 'easeInOutQuint', 'sin'];
  // hydra's array sequencing (hydra-synth `lib/array-utils.js`): an array
  // argument steps through its values, and these set how.
  const arrayUtils = {
    fast(speed = 1) {
      this._speed = speed;
      return this;
    },
    smooth(smooth = 1) {
      this._smooth = smooth;
      return this;
    },
    ease(ease = 'linear') {
      // Upstream stores the function itself; a custom one is linear here.
      if (typeof ease === 'function' || EASINGS.includes(ease)) {
        this._smooth = 1;
        this._ease = typeof ease === 'function' ? 'linear' : ease;
      }
      return this;
    },
    offset(offset = 0.5) {
      this._offset = offset % 1.0;
      return this;
    },
    fit(low = 0, high = 1) {
      const lowest = Math.min(...this);
      const highest = Math.max(...this);
      const arr = this.map((n) => ((n - lowest) * (high - low)) / (highest - lowest) + low);
      arr._speed = this._speed;
      arr._smooth = this._smooth;
      arr._ease = this._ease;
      return arr;
    },
  };
  // hydra's canvas size and pointer, fixed here: there is no canvas.
  Object.assign(Hydra, { width: 1920, height: 1080, mouse: { x: 0, y: 0 } });
  // `a`, hydra's audio analyser (hydra-synth `lib/audio.js`), with
  // `detectAudio: true`. Upstream it listens to the microphone through Meyda;
  // here it hears rudel's own output, as the same 24-band loudness
  // (`Hydra._loudness`), and folds it into bins exactly as upstream does.
  const audioAnalyser = () => {
    const each = (key) =>
      function (value) {
        this[key] = value;
        this.settings = this.settings.map((s) => ({ ...s, [key]: value }));
      };
    const a = {
      vol: 0,
      scale: 10,
      max: 15,
      cutoff: 2,
      smooth: 0.4,
      frame: -1,
      beat: { holdFrames: 20, threshold: 40, _cutoff: 0, decay: 0.98, _framesSinceBeat: 0 },
      onBeat() {},
      show() {},
      hide() {},
      setMax(max) {
        this.max = max;
      },
      setCutoff: each('cutoff'),
      setSmooth: each('smooth'),
      setScale: each('scale'),
      setBins(numBins) {
        this.bins = Array(numBins).fill(0);
        this.prevBins = Array(numBins).fill(0);
        this.fft = Array(numBins).fill(0);
        this.settings = Array(numBins)
          .fill(0)
          .map(() => ({ cutoff: this.cutoff, scale: this.scale, smooth: this.smooth }));
        this.bins.forEach((_, index) => {
          globalThis['a' + index] = (scale = 1, offset = 0) => () => a.fft[index] * scale + offset;
        });
      },
      detectBeat(level) {
        const beat = this.beat;
        if (level > beat._cutoff && level > beat.threshold) {
          this.onBeat();
          beat._cutoff = level * 1.2;
          beat._framesSinceBeat = 0;
        } else if (beat._framesSinceBeat <= beat.holdFrames) {
          beat._framesSinceBeat++;
        } else {
          beat._cutoff = Math.max(beat._cutoff * beat.decay, beat.threshold);
        }
      },
      // Once per frame, however many function arguments read it.
      tick() {
        const { frame, specific } = Hydra._loudness();
        if (frame === this.frame || specific.length === 0) return;
        this.frame = frame;
        this.vol = specific.reduce((x, y) => x + y, 0);
        this.detectBeat(this.vol);
        const spacing = Math.floor(specific.length / this.bins.length);
        this.prevBins = this.bins.slice(0);
        this.bins = this.bins
          .map((_, i) => specific.slice(i * spacing, (i + 1) * spacing).reduce((x, y) => x + y, 0))
          .map((bin, i) => bin * (1.0 - this.settings[i].smooth) + this.prevBins[i] * this.settings[i].smooth);
        this.fft = this.bins.map((bin, i) =>
          Math.max(0, (bin - this.settings[i].cutoff) / this.settings[i].scale),
        );
      },
    };
    a.setBins(4);
    return a;
  };
  // An array or function argument changes every frame upstream. The natives
  // cannot see an array's `_speed` and friends, nor keep a function, so they
  // get each spelled out; the chain then reads it per frame (`hydra/params.rs`).
  // A function also sees hydra's `time`, which upstream sets every frame.
  const hydraArg = (v) => {
    if (Array.isArray(v)) {
      return { hydraSeq: Array.from(v), speed: v._speed, smooth: v._smooth, ease: v._ease,
        offset: v._offset };
    }
    if (typeof v === 'function') {
      return {
        hydraFn: (props) => {
          if (typeof globalThis.time === 'number') globalThis.time = props.time;
          globalThis.a?.tick?.();
          return v(props);
        },
      };
    }
    return v;
  };
  const wrapArgs = (object) => {
    for (const name of Object.getOwnPropertyNames(object)) {
      const f = object[name];
      if (typeof f !== 'function' || name === 'constructor') continue;
      object[name] = function (...args) {
        return f.apply(this, args.map(hydraArg));
      };
    }
  };
  wrapArgs(Hydra);
  wrapArgs(Object.getPrototypeOf(Hydra.osc()));
  // `H(pattern)` (@strudel/hydra): a pattern's value, per frame.
  def(globalThis, 'H', (pattern) => reify(pattern));
  def(globalThis, 'initHydra', async (options = {}) => {
    for (const name of Object.getOwnPropertyNames(Hydra)) globalThis[name] = Hydra[name];
    // hydra's clock, which a function argument reads.
    globalThis.time = 0;
    for (const [name, f] of Object.entries(arrayUtils)) def(Array.prototype, name, f);
    if (options.detectAudio) globalThis.a = audioAnalyser();
  });

  // `createParam(names)` (controls.mjs): a control made at runtime. Returns
  // the factory and sets the method; a list of names spreads a list value
  // over them, and an object's `.value` fills the first.
  const createParam = (names) => {
    const isMulti = Array.isArray(names);
    names = isMulti ? names : [names];
    const name = names[0];
    const withVal = (xs) => {
      let bag;
      if (typeof xs === 'object' && xs.value !== undefined) {
        bag = { ...xs };
        xs = xs.value;
        delete bag.value;
      }
      if (isMulti && Array.isArray(xs)) {
        const result = bag || {};
        xs.forEach((x, i) => {
          if (i < names.length) result[names[i]] = x;
        });
        return result;
      }
      if (bag) {
        bag[name] = xs;
        return bag;
      }
      return { [name]: xs };
    };
    const func = function (value, pat) {
      if (!pat) return reify(value).withValue(withVal);
      if (typeof value === 'undefined') return pat.fmap(withVal);
      return pat.set(reify(value).withValue(withVal));
    };
    def(P, name, function (value) {
      return func(value, this);
    });
    return func;
  };
  def(globalThis, 'createParam', createParam);
  def(globalThis, 'createParams', (...names) =>
    names.reduce((acc, name) => Object.assign(acc, { [name]: createParam(name) }), {}),
  );

  // draw.mjs / animate.mjs paint a browser canvas every animation frame.
  // There is none here, so they keep upstream's return values and draw
  // nothing: `draw` and `onPaint` return the pattern, `animate` silence.
  def(P, 'draw', function () {
    return this;
  });
  def(P, 'onPaint', function () {
    return this;
  });
  def(P, 'animate', () => silence);

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
