// Strudel's draw canvas (@strudel/draw `draw.mjs`, `animate.mjs`) for rudel.
//
// Upstream a full-screen <canvas> sits behind the code, and `.draw(fn)`,
// `.onPaint(painter)`, `getDrawContext()` and `requestAnimationFrame` paint
// on it. Here the 2D context is a stand-in that records what is drawn: paths
// and transforms are flattened to polygons as they are built (natively, in
// `crate::canvas::Recorder`), so the app only replays fills, strokes, clears
// and text onto a canvas of its own, which, like a browser's, keeps its
// pixels until something clears them.
//
// The app calls `__drawFrame` once per frame: it runs the animation-frame
// callbacks and painters, and the app takes what they drew from the recorder.
// SPDX-License-Identifier: AGPL-3.0-or-later
(() => {
  const def = (object, name, value) =>
    Object.defineProperty(object, name, { value, writable: true, configurable: true });

  let used = false;
  let now = 0;
  // The canvas size in CSS pixels: set by each frame, and before the first
  // the size the app last showed (a script reads `innerWidth` as it runs).
  const shown = __canvasSize();
  const size = { width: shown[0], height: shown[1] };
  const canvas = {
    get width() {
      return size.width;
    },
    set width(_) {},
    get height() {
      return size.height;
    },
    set height(_) {},
    get clientWidth() {
      return size.width;
    },
    get clientHeight() {
      return size.height;
    },
    style: {},
    getContext: () => context,
    addEventListener() {},
    removeEventListener() {},
    getBoundingClientRect: () => ({ left: 0, top: 0, width: size.width, height: size.height }),
  };
  for (const [name, read] of [
    ['innerWidth', () => size.width],
    ['innerHeight', () => size.height],
    ['devicePixelRatio', () => 1],
  ]) {
    Object.defineProperty(globalThis, name, { get: read, configurable: true });
  }

  // `document`, as far as tunes reach for it: the page size, and mouse and
  // key events over the editor (the app sends them each frame), through
  // `document.onmousemove = …` or `addEventListener`. Elements they create or
  // look up are inert.
  const listeners = new Map();
  const handlers = {};
  const element = () => ({
    style: {},
    dataset: {},
    classList: { add() {}, remove() {}, toggle() {}, contains: () => false },
    appendChild: (child) => child,
    removeChild: (child) => child,
    remove() {},
    setAttribute() {},
    getAttribute: () => null,
    addEventListener() {},
    removeEventListener() {},
    getContext: () => context,
    get clientWidth() {
      return size.width;
    },
    get clientHeight() {
      return size.height;
    },
  });
  const body = element();
  const page = {
    body,
    documentElement: body,
    head: element(),
    cookie: '',
    createElement: (tag) => (String(tag).toLowerCase() === 'canvas' ? canvas : element()),
    createElementNS: () => element(),
    getElementById: () => null,
    querySelector: () => null,
    querySelectorAll: () => [],
    getElementsByTagName: () => [],
    addEventListener(type, fn) {
      used = true;
      if (!listeners.has(type)) listeners.set(type, []);
      listeners.get(type).push(fn);
    },
    removeEventListener(type, fn) {
      const fns = listeners.get(type) ?? [];
      if (fns.includes(fn)) fns.splice(fns.indexOf(fn), 1);
    },
  };
  for (const type of ['mousemove', 'mousedown', 'mouseup', 'click', 'keydown', 'keyup']) {
    Object.defineProperty(page, 'on' + type, {
      get: () => handlers[type] ?? null,
      set(fn) {
        used = true;
        handlers[type] = fn;
      },
    });
  }
  def(globalThis, 'document', page);
  // `window.addEventListener('keydown', …)` hears the same events.
  def(globalThis, 'addEventListener', page.addEventListener);
  def(globalThis, 'removeEventListener', page.removeEventListener);
  // hydra's `mouse` follows the pointer, so `initHydra` turns the frame on.
  const initHydra = globalThis.initHydra;
  def(globalThis, 'initHydra', async (options) => {
    used = true;
    return initHydra(options);
  });
  const dispatch = ([type, x, y, button, key, code, ctrl, shift, alt, meta]) => {
    const event = {
      type,
      clientX: x,
      clientY: y,
      pageX: x,
      pageY: y,
      offsetX: x,
      offsetY: y,
      button,
      key,
      code,
      ctrlKey: ctrl,
      shiftKey: shift,
      altKey: alt,
      metaKey: meta,
      repeat: false,
      target: body,
      preventDefault() {},
      stopPropagation() {},
    };
    if (type === 'mousemove') Object.assign(Hydra.mouse, { x, y });
    for (const fn of [handlers[type], ...(listeners.get(type) ?? [])]) {
      if (typeof fn === 'function') attempt(`document.on${type}`, () => fn(event));
    }
  };

  // The path, the transform and what is drawn are kept natively (`__canvas`,
  // `crate::canvas::Recorder`): built point by point in script, a frame of
  // curves cost several times what it does now. The context here holds the
  // style, which scripts set as properties.
  const native = __canvas;
  const defaults = () => ({
    fillStyle: '#000000',
    strokeStyle: '#000000',
    lineWidth: 1,
    lineCap: 'butt',
    lineJoin: 'miter',
    globalAlpha: 1,
    font: '10px sans-serif',
    textAlign: 'start',
    textBaseline: 'alphabetic',
    globalCompositeOperation: 'source-over',
  });

  // A gradient or pattern fill has no counterpart; its first colour stands in.
  const style = (s) => (s && typeof s === 'object' ? s.color ?? '#000000' : String(s));

  class Context2D {
    constructor() {
      Object.assign(this, defaults());
      this.canvas = canvas;
      this.stack = [];
    }
    // --- state: the style here, the transform natively
    save() {
      const { stack, canvas: _c, ...state } = this;
      this.stack.push(state);
      native.save();
    }
    restore() {
      const state = this.stack.pop();
      if (state) Object.assign(this, state);
      native.restore();
    }
    reset() {
      Object.assign(this, defaults());
      this.stack = [];
      native.reset();
      this.clearRect(0, 0, size.width, size.height);
    }
    // --- drawing, with the style the context holds
    fill(rule) {
      if (typeof rule === 'object') rule = arguments[1];
      native.fillWith(style(this.fillStyle), this.globalAlpha, rule === 'evenodd');
    }
    stroke() {
      native.strokeWith(style(this.strokeStyle), this.globalAlpha, this.lineWidth, this.lineCap, this.lineJoin);
    }
    fillRect(x, y, w, h) {
      native.fillRectWith(x, y, w, h, style(this.fillStyle), this.globalAlpha);
    }
    strokeRect(x, y, w, h) {
      native.strokeRectWith(x, y, w, h, style(this.strokeStyle), this.globalAlpha, this.lineWidth, this.lineCap, this.lineJoin);
    }
    // The font's size in user space.
    fontSize() {
      const m = /(\d*\.?\d+)px/.exec(this.font);
      return m ? Number(m[1]) : 10;
    }
    fillText(text, x, y) {
      native.textWith(String(text), x, y, this.fontSize(), style(this.fillStyle), this.globalAlpha, this.textAlign, this.textBaseline);
    }
    strokeText(text, x, y) {
      native.textWith(String(text), x, y, this.fontSize(), style(this.strokeStyle), this.globalAlpha, this.textAlign, this.textBaseline);
    }
    measureText(text) {
      const size = this.fontSize();
      return { width: String(text).length * size * 0.55, actualBoundingBoxAscent: size * 0.8, actualBoundingBoxDescent: size * 0.2 };
    }
    // --- what has no counterpart: accepted, and drawn as nothing
    drawImage() {}
    putImageData() {}
    getImageData(x, y, w, h) {
      return { width: w, height: h, data: new Uint8ClampedArray(Math.max(0, w * h * 4)) };
    }
    createImageData(w, h) {
      return this.getImageData(0, 0, w, h);
    }
    createLinearGradient() {
      return { color: undefined, addColorStop(_, c) { this.color ??= c; } };
    }
    createRadialGradient() {
      return this.createLinearGradient();
    }
    createConicGradient() {
      return this.createLinearGradient();
    }
    createPattern() {
      return { color: '#000000' };
    }
    setLineDash() {}
    getLineDash() {
      return [];
    }
    clip() {}
    isPointInPath() {
      return false;
    }
  }
  for (const name of ['beginPath', 'moveTo', 'lineTo', 'closePath', 'rect', 'roundRect', 'ellipse', 'arc',
    'arcTo', 'bezierCurveTo', 'quadraticCurveTo', 'setTransform', 'resetTransform', 'getTransform',
    'transform', 'translate', 'scale', 'rotate', 'clearRect']) {
    def(Context2D.prototype, name, native[name]);
  }
  const context = new Context2D();

  def(globalThis, 'getDrawContext', () => {
    used = true;
    return context;
  });
  def(globalThis, 'getTime', () => now);

  // Animation frames: called on the next frame, with its time in milliseconds.
  let frames = new Map();
  let nextFrame = 1;
  def(globalThis, 'requestAnimationFrame', (callback) => {
    used = true;
    frames.set(nextFrame, callback);
    return nextFrame++;
  });
  def(globalThis, 'cancelAnimationFrame', (id) => frames.delete(id));

  // A hap as upstream's draw code reads one.
  const n = (x) => Number(x?.valueOf?.() ?? x);
  // On a prototype the queried haps share: closures made per hap per frame
  // were most of what a frame's query cost.
  const drawable = {
    hasOnset() {
      return !!this.whole && n(this.whole.begin) === n(this.part.begin);
    },
    get endClipped() {
      return n((this.whole ?? this.part).end);
    },
    isInFuture(t) {
      return n(this.whole?.begin ?? this.part.begin) > t;
    },
    isInNearPast(margin, t) {
      return n((this.whole ?? this.part).end) >= t - margin;
    },
    isActive(t) {
      const span = this.whole ?? this.part;
      return n(span.begin) <= t && n(span.end) >= t;
    },
  };
  const query = (pattern, begin, end) => {
    const haps = pattern.queryArc(begin, end);
    for (let i = 0; i < haps.length; i++) Object.setPrototypeOf(haps[i], drawable);
    return haps;
  };

  // `.draw(fn, {lookbehind, lookahead})` (draw.mjs): `fn(haps, time, t,
  // pattern)` every frame, with the haps seen recently.
  const painters = [];
  def(Pattern.prototype, 'draw', function (fn, options = {}) {
    used = true;
    const lookbehind = Math.abs(options.lookbehind ?? 0);
    const lookahead = options.lookahead ?? 0;
    painters.push({ pattern: this, fn, lookbehind, lookahead, memory: null, last: null });
    return this;
  });
  // `.onPaint(painter)`: `painter(ctx, time, haps, drawTime)` every frame,
  // with the haps from two cycles back to two ahead. Upstream passes the haps
  // of the whole running pattern; here, of the pattern it was attached to.
  const onPaints = [];
  def(Pattern.prototype, 'onPaint', function (painter) {
    used = true;
    onPaints.push({ pattern: this, painter });
    return this;
  });

  // An error in one painter is said once, not every frame.
  const said = new Set();
  const attempt = (what, run) => {
    try {
      run();
    } catch (e) {
      const message = `${what}: ${e?.message ?? e}`;
      if (!said.has(message)) {
        said.add(message);
        console.log(message);
      }
    }
  };

  globalThis.__drawUsed = () => used;
  globalThis.__drawFrame = (time, ms, width, height, events = []) => {
    size.width = Math.max(1, Math.round(width));
    size.height = Math.max(1, Math.round(height));
    now = time;
    for (const event of events) dispatch(event);
    const due = frames;
    frames = new Map();
    for (const callback of due.values()) attempt('requestAnimationFrame', () => callback(ms));
    for (const p of painters) {
      attempt('draw', () => {
        const t = time + p.lookahead;
        if (p.memory === null) {
          p.memory = query(p.pattern, time, t).filter((h) => h.hasOnset());
        }
        p.memory = p.memory.filter((h) => h.isInNearPast(p.lookbehind, time));
        const begin = Math.max(p.last ?? t, t - 1 / 10);
        if (t > begin) p.memory.push(...query(p.pattern, begin, t).filter((h) => h.hasOnset()));
        p.last = t;
        p.fn(p.memory, time, t, p.pattern);
      });
    }
    for (const p of onPaints) {
      attempt('onPaint', () => p.painter(context, time, query(p.pattern, time - 2, time + 2), [-2, 2]));
    }
  };

  // `animate` (animate.mjs), ported: shapes from the `x`/`y`/`w`/`h`/`angle`/
  // `r`/`fill` controls, drawn every frame over a translucent clear that
  // leaves a smear.
  def(Pattern.prototype, 'animate', function ({ callback, smear = 0.5 } = {}) {
    globalThis.frame && cancelAnimationFrame(globalThis.frame);
    const ctx = getDrawContext();
    const { clientWidth: ww, clientHeight: wh } = ctx.canvas;
    let smearPart = smear === 0 ? '99' : Number((1 - smear) * 100).toFixed(0);
    smearPart = smearPart.length === 1 ? `0${smearPart}` : smearPart;
    const clearColor = `#200010${smearPart}`;
    const render = (t) => {
      t = Math.round(t);
      const frame = this.slow(1000).queryArc(t, t);
      ctx.fillStyle = clearColor;
      ctx.fillRect(0, 0, ww, wh);
      frame.forEach((f) => {
        let { x, y, w, h, s, r, angle = 0, fill = 'darkseagreen' } = f.value;
        w *= ww;
        h *= wh;
        if (r !== undefined && angle !== undefined) {
          const radians = angle * 2 * Math.PI;
          const [cx, cy] = [(ww - w) / 2, (wh - h) / 2];
          x = cx + Math.cos(radians) * r * cx;
          y = cy + Math.sin(radians) * r * cy;
        } else {
          x *= ww - w;
          y *= wh - h;
        }
        const val = { ...f.value, x, y, w, h };
        ctx.fillStyle = fill;
        if (s === 'rect') {
          ctx.fillRect(x, y, w, h);
        } else if (s === 'ellipse') {
          ctx.beginPath();
          ctx.ellipse(x + w / 2, y + h / 2, w / 2, h / 2, 0, 0, 2 * Math.PI);
          ctx.fill();
        }
        callback && callback(ctx, val, f);
      });
      globalThis.frame = requestAnimationFrame(render);
    };
    globalThis.frame = requestAnimationFrame(render);
    return silence;
  });
})();
