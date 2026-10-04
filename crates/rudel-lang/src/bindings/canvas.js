// Strudel's draw canvas (@strudel/draw `draw.mjs`, `animate.mjs`) for rudel.
//
// Upstream a full-screen <canvas> sits behind the code, and `.draw(fn)`,
// `.onPaint(painter)`, `getDrawContext()` and `requestAnimationFrame` paint
// on it. Here the 2D context is a stand-in that records what is drawn: paths
// and transforms are flattened to polygons as they are built, so the app only
// replays fills, strokes, clears and text onto a canvas of its own, which,
// like a browser's, keeps its pixels until something clears them.
//
// The app calls `__drawFrame` once per frame: it runs the animation-frame
// callbacks and painters and returns that frame's drawing.
// SPDX-License-Identifier: AGPL-3.0-or-later
(() => {
  const def = (object, name, value) =>
    Object.defineProperty(object, name, { value, writable: true, configurable: true });

  let ops = [];
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

  // How many straight pieces a curve becomes, by its size on screen.
  const pieces = (length) => Math.max(8, Math.min(256, Math.ceil(length / 4)));

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
    t: [1, 0, 0, 1, 0, 0],
  });

  // A gradient or pattern fill has no counterpart; its first colour stands in.
  const style = (s) => (s && typeof s === 'object' ? s.color ?? '#000000' : String(s));

  class Context2D {
    constructor() {
      Object.assign(this, defaults());
      this.canvas = canvas;
      this.stack = [];
      this.path = [];
    }
    // --- state
    save() {
      const { stack, path, canvas: _c, ...state } = this;
      this.stack.push({ ...state, t: [...this.t] });
    }
    restore() {
      const state = this.stack.pop();
      if (state) Object.assign(this, state);
    }
    reset() {
      Object.assign(this, defaults());
      this.stack = [];
      this.path = [];
      this.clearRect(0, 0, size.width, size.height);
    }
    // --- transforms: `t` maps user space to canvas pixels
    setTransform(a, b, c, d, e, f) {
      if (a && typeof a === 'object') ({ a, b, c, d, e, f } = a);
      this.t = [a, b, c, d, e, f].map(Number);
    }
    resetTransform() {
      this.t = [1, 0, 0, 1, 0, 0];
    }
    getTransform() {
      const [a, b, c, d, e, f] = this.t;
      return { a, b, c, d, e, f };
    }
    // Index access, not destructuring or spread, on every per-point path:
    // boa runs those through the iterator protocol, which was most of a frame.
    transform(a, b, c, d, e, f) {
      const t = this.t;
      this.t = [
        t[0] * a + t[2] * b, t[1] * a + t[3] * b, t[0] * c + t[2] * d, t[1] * c + t[3] * d,
        t[0] * e + t[2] * f + t[4], t[1] * e + t[3] * f + t[5],
      ];
    }
    translate(x, y) {
      this.transform(1, 0, 0, 1, x, y);
    }
    scale(x, y = x) {
      this.transform(x, 0, 0, y, 0, 0);
    }
    rotate(a) {
      this.transform(Math.cos(a), Math.sin(a), -Math.sin(a), Math.cos(a), 0, 0);
    }
    point(x, y) {
      const t = this.t;
      return [t[0] * x + t[2] * y + t[4], t[1] * x + t[3] * y + t[5]];
    }
    // Append (x, y), mapped to canvas pixels, to a flat point list.
    push(points, x, y) {
      const t = this.t;
      points.push(t[0] * x + t[2] * y + t[4], t[1] * x + t[3] * y + t[5]);
    }
    // How much the transform scales lengths, for line widths and font sizes.
    get unit() {
      const t = this.t;
      return Math.sqrt(Math.abs(t[0] * t[3] - t[1] * t[2])) || 1;
    }
    // --- paths: subpaths of canvas-pixel points
    beginPath() {
      this.path = [];
    }
    current() {
      return this.path[this.path.length - 1];
    }
    moveTo(x, y) {
      this.path.push({ points: this.point(x, y), closed: false, user: [x, y], first: [x, y] });
    }
    lineTo(x, y) {
      const sub = this.current();
      if (!sub) return this.moveTo(x, y);
      this.push(sub.points, x, y);
      sub.user = [x, y];
    }
    closePath() {
      const sub = this.current();
      if (!sub) return;
      sub.closed = true;
      // The next segment starts where this subpath did.
      this.path.push({ points: sub.points.slice(0, 2), closed: false, user: sub.first, first: sub.first });
    }
    rect(x, y, w, h) {
      this.moveTo(x, y);
      this.lineTo(x + w, y);
      this.lineTo(x + w, y + h);
      this.lineTo(x, y + h);
      this.closePath();
    }
    roundRect(x, y, w, h) {
      this.rect(x, y, w, h);
    }
    ellipse(x, y, rx, ry, rotation, start, end, ccw = false) {
      let sweep = end - start;
      const tau = 2 * Math.PI;
      if (!ccw && sweep < 0) sweep = (sweep % tau) + tau;
      if (ccw && sweep > 0) sweep = (sweep % tau) - tau;
      if (Math.abs(end - start) >= tau) sweep = ccw ? -tau : tau;
      const n = pieces(Math.abs(sweep) * Math.max(rx, ry) * this.unit);
      const cos = Math.cos(rotation);
      const sin = Math.sin(rotation);
      const user = (a) => {
        const ex = rx * Math.cos(a);
        const ey = ry * Math.sin(a);
        return [x + ex * cos - ey * sin, y + ex * sin + ey * cos];
      };
      if (!this.current()) this.path.push({ points: [], closed: false, user: null, first: user(start) });
      this.extend(__ellipse(this.t, n, x, y, rx, ry, rotation, start, sweep), user(start + sweep));
    }
    // Append canvas-pixel points to the current subpath, which now ends at
    // `user`. The curves are flattened natively (`__ellipse`, `__bezier`): a
    // script call per point was most of what a frame of curves cost.
    extend(points, user) {
      const sub = this.current();
      sub.points = sub.points.concat(points);
      sub.user = user;
    }
    arc(x, y, r, start, end, ccw = false) {
      this.ellipse(x, y, r, r, 0, start, end, ccw);
    }
    arcTo(x1, y1, x2, y2) {
      this.lineTo(x1, y1);
      this.lineTo(x2, y2);
    }
    bezierCurveTo(c1x, c1y, c2x, c2y, x, y) {
      const sub = this.current();
      const from = sub ? sub.user : [x, y];
      const n = pieces(Math.hypot(x - from[0], y - from[1]) * this.unit);
      if (!sub) this.moveTo(x, y);
      this.extend(__bezier(this.t, n, from[0], from[1], c1x, c1y, c2x, c2y, x, y), [x, y]);
    }
    // The same curve as a cubic, its control points two thirds of the way
    // from each end to (cx, cy).
    quadraticCurveTo(cx, cy, x, y) {
      const from = this.current()?.user ?? [x, y];
      const k = 2 / 3;
      this.bezierCurveTo(
        from[0] + k * (cx - from[0]), from[1] + k * (cy - from[1]),
        x + k * (cx - x), y + k * (cy - y),
        x, y,
      );
    }
    subpaths(stroke) {
      return this.path
        .filter((sub) => sub.points.length >= (stroke ? 4 : 6))
        .map((sub) => (stroke ? [sub.closed, sub.points] : sub.points));
    }
    // --- drawing
    fill(rule) {
      if (typeof rule === 'object') rule = arguments[1];
      ops.push(['fill', style(this.fillStyle), this.globalAlpha, rule === 'evenodd', this.subpaths(false)]);
    }
    stroke() {
      ops.push([
        'stroke',
        style(this.strokeStyle),
        this.globalAlpha,
        this.lineWidth * this.unit,
        this.lineCap,
        this.lineJoin,
        this.subpaths(true),
      ]);
    }
    corners(x, y, w, h) {
      const points = [];
      this.push(points, x, y);
      this.push(points, x + w, y);
      this.push(points, x + w, y + h);
      this.push(points, x, y + h);
      return points;
    }
    fillRect(x, y, w, h) {
      ops.push(['fill', style(this.fillStyle), this.globalAlpha, false, [this.corners(x, y, w, h)]]);
    }
    strokeRect(x, y, w, h) {
      const s = [true, this.corners(x, y, w, h)];
      ops.push(['stroke', style(this.strokeStyle), this.globalAlpha, this.lineWidth * this.unit, this.lineCap, this.lineJoin, [s]]);
    }
    clearRect(x, y, w, h) {
      ops.push(['clear', this.corners(x, y, w, h)]);
    }
    fontSize() {
      const m = /(\d*\.?\d+)px/.exec(this.font);
      return (m ? Number(m[1]) : 10) * this.unit;
    }
    text(text, x, y, fill) {
      const [px, py] = this.point(x, y);
      const color = style(fill ? this.fillStyle : this.strokeStyle);
      ops.push(['text', String(text), px, py, this.fontSize(), color, this.globalAlpha, this.textAlign, this.textBaseline]);
    }
    fillText(text, x, y) {
      this.text(text, x, y, true);
    }
    strokeText(text, x, y) {
      this.text(text, x, y, false);
    }
    measureText(text) {
      const size = this.fontSize() / this.unit;
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
  const drawable = (hap) =>
    Object.assign(hap, {
      hasOnset() {
        return !!hap.whole && n(hap.whole.begin) === n(hap.part.begin);
      },
      get endClipped() {
        return n((hap.whole ?? hap.part).end);
      },
      isInFuture(t) {
        return n(hap.whole?.begin ?? hap.part.begin) > t;
      },
      isInNearPast(margin, t) {
        return n((hap.whole ?? hap.part).end) >= t - margin;
      },
      isActive(t) {
        const span = hap.whole ?? hap.part;
        return n(span.begin) <= t && n(span.end) >= t;
      },
    });
  const query = (pattern, begin, end) => pattern.queryArc(begin, end).map(drawable);

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
    ops = [];
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
    return ops;
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
