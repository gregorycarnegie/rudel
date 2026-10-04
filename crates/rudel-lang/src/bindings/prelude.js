// The part of the prelude upstream writes in JavaScript over its methods,
// kept in JavaScript: what a native binding would only restate. Runs once per
// engine, after every native binding is in place. The numbered slots, the
// alignment getters and the `_name` fallback are native (`prelude.rs`).
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

  // In the browser REPL `window` is the global object, and tunes use it as a
  // namespace shared between blocks (`window.spag = …`).
  def(globalThis, 'window', globalThis);

  // `initHydra()` (@strudel/hydra) loads hydra-synth, which puts its sources
  // and outputs (`osc`, `src`, `o0`, `render`, …) in global scope, over
  // Strudel's own `osc`, `noise` and `shape`, as upstream. Until then they
  // are on `Hydra`.
  // hydra's external sources `s0`..`s3`, indices 4..7 past the four outputs:
  // a picture (`initImage`), a webcam (`initCam(n)`, recorded as `camera:n`),
  // a video file (`initVideo`, as `video:url`) or the screen (`initScreen`, as
  // `screen:`). `initStream` (a peer's stream) has no counterpart here.
  const sourceIndex = (s) => (s !== null && typeof s === 'object' && 'index' in s ? s.index : s);
  const hydraSrc = Hydra.src;
  Hydra.src = (s, ...rest) => hydraSrc(sourceIndex(s), ...rest);
  for (let i = 0; i < 4; i++) {
    const none = function () {
      return this;
    };
    Hydra[`s${i}`] = { index: 4 + i, init: none,
      initVideo(url = '') {
        Hydra._image(i, url, 'video');
        return this;
      },
      initCam(index = 0) {
        Hydra._image(i, 'camera:' + (Math.floor(Number(index)) || 0));
        return this;
      },
      initScreen() {
        Hydra._image(i, '', 'screen');
        return this;
      },
      initStream: none, clear: none,
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
    Hydra._feed(!!options.feedStrudel);
  });

  // `ref(accessor)` (pattern.mjs): a pattern of whatever `accessor()` returns
  // as each cycle is queried, for values the script changes as it runs.
  def(globalThis, 'ref', (accessor) =>
    pure(1)
      .withValue(() => reify(accessor()))
      .innerJoin(),
  );

  // `onTrigger(fn, dominant)` (pattern.mjs): `fn(hap, time)` as each event
  // plays, fired from the app's frame loop like `onTriggerTime`. Upstream a
  // dominant trigger (the default) also silences the event's sound; here the
  // sound plays regardless.
  def(P, 'onTrigger', function (fn) {
    return this.onTriggerTime((hap) => fn(hap, globalThis.getTime?.() ?? 0));
  });
  // The website's pattern settings (website/src/settings.mjs): `.theme(
  // "<githubDark nord>")` switches the editor's theme as the events play.
  // Each event carries its value (a hook registered while the pattern is
  // queried would come too late to be kept), and one hook reports it.
  for (const key of ['theme', 'fontFamily', 'fontSize']) {
    const carried = `__${key}`;
    const set = function (value) {
      const values = reify(value).fmap((v) => ({ [carried]: Array.isArray(v) ? v.join(' ') : String(v) }));
      return this.set(values).onTrigger((hap) => __setting(key, hap.value[carried]), false);
    };
    def(P, key, set);
    def(globalThis, key, curry(2, (value, pat) => set.call(reify(pat), value)));
  }

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

  // `tokenizeNote('c#4')` (core/util.mjs): a note name's letter, accidentals
  // and octave.
  def(globalThis, 'tokenizeNote', (note) => {
    if (typeof note !== 'string') return [];
    const [pc, acc = '', oct] = note.match(/^([a-gA-G])([#bsf]*)(-?[0-9]*)$/)?.slice(1) || [];
    if (!pc) return [];
    return [pc, acc, oct ? Number(oct) : undefined];
  });
})();
