//! Strudel's draw canvas, as far as the script side goes: the frame driver an
//! evaluation hands the app, and the drawing it returns (`bindings/canvas.js`).

use crate::bindings::arg_to_f64;
use crate::js::{self, Arg, Scope, SendFn};

/// One drawing operation, in canvas pixels, with any transform already
/// applied to its points.
#[derive(Clone, Debug, PartialEq)]
pub enum CanvasOp {
    /// Fill these polygons (`fill`, `fillRect`).
    Fill {
        color: String,
        alpha: f32,
        even_odd: bool,
        polygons: Vec<Vec<[f32; 2]>>,
    },
    /// Stroke these polylines, each closed or not (`stroke`, `strokeRect`).
    Stroke {
        color: String,
        alpha: f32,
        width: f32,
        cap: String,
        join: String,
        lines: Vec<(bool, Vec<[f32; 2]>)>,
    },
    /// Make this polygon transparent (`clearRect`).
    Clear { polygon: Vec<[f32; 2]> },
    /// `fillText`/`strokeText`, `y` on the baseline `baseline` names.
    Text {
        text: String,
        x: f32,
        y: f32,
        size: f32,
        color: String,
        alpha: f32,
        align: String,
        baseline: String,
    },
}

/// The canvas size the app last showed, in CSS pixels, packed `w << 32 | h`.
static SIZE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new((1280 << 32) | 720);

/// Tell scripts how big the canvas is, so one that reads `window.innerWidth`
/// as it is evaluated (before any frame has run) gets the real size.
pub fn set_size(width: u32, height: u32) {
    let packed = (u64::from(width.max(1)) << 32) | u64::from(height.max(1));
    SIZE.store(packed, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) fn size() -> (u32, u32) {
    let packed = SIZE.load(std::sync::atomic::Ordering::Relaxed);
    ((packed >> 32) as u32, packed as u32)
}

/// A mouse or key event over the editor, for the script's `document`
/// handlers, in canvas pixels. `kind` is the DOM event type (`mousemove`,
/// `mousedown`, `mouseup`, `click`, `keydown`, `keyup`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CanvasEvent {
    pub kind: &'static str,
    pub x: f32,
    pub y: f32,
    pub button: u8,
    /// `KeyboardEvent.key` and `.code`, for key events.
    pub key: String,
    pub code: String,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

impl CanvasEvent {
    fn to_arg(&self) -> Arg {
        Arg::List(vec![
            Arg::Str(self.kind.to_string()),
            Arg::Num(f64::from(self.x)),
            Arg::Num(f64::from(self.y)),
            Arg::Num(f64::from(self.button)),
            Arg::Str(self.key.clone()),
            Arg::Str(self.code.clone()),
            Arg::Bool(self.ctrl),
            Arg::Bool(self.shift),
            Arg::Bool(self.alt),
            Arg::Bool(self.meta),
        ])
    }
}

/// What draws on the canvas for one evaluation: its painters,
/// `requestAnimationFrame` callbacks and `animate`, run once a frame.
#[derive(Clone, Copy)]
pub struct CanvasDriver(SendFn);

impl CanvasDriver {
    pub(crate) fn new(frame: SendFn) -> Self {
        Self(frame)
    }

    /// Run one frame at transport time `cycle` and clock `ms`, on a canvas of
    /// `width` by `height` CSS pixels, after handing the script this frame's
    /// `events`, and return what it drew. `None` once the evaluation is gone.
    pub fn frame(
        &self,
        cycle: f64,
        ms: f64,
        width: u32,
        height: u32,
        events: &[CanvasEvent],
    ) -> Option<Vec<CanvasOp>> {
        // Made into script values on the JS thread: an `Arg` is not `Send`.
        let events = events.to_vec();
        self.0.run(move |f| {
            let mut args: Vec<Arg> = [cycle, ms, f64::from(width), f64::from(height)]
                .map(Arg::Num)
                .to_vec();
            args.push(Arg::List(events.iter().map(CanvasEvent::to_arg).collect()));
            // What was drawn outside a frame (as the script ran) is dropped.
            js::with_canvas(|c| c.ops.clear());
            let _ = js::call(f, args);
            js::with_canvas(|c| std::mem::take(&mut c.ops))
        })
    }
}

impl PartialEq for CanvasDriver {
    fn eq(&self, _: &Self) -> bool {
        // Each evaluation's driver is its own; none equals another.
        false
    }
}

impl std::fmt::Debug for CanvasDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CanvasDriver")
    }
}

/// What `canvas.js`'s 2D context has built and drawn, one per script context.
/// Kept natively, so a path's points never pass through the script: a script
/// call per point, and the op arrays read back each frame, were most of what
/// a frame cost.
pub(crate) struct Recorder {
    /// Maps user space to canvas pixels: `[a, b, c, d, e, f]`.
    t: [f64; 6],
    /// The transforms `save` kept; the script keeps the rest of the state.
    saved: Vec<[f64; 6]>,
    path: Vec<Subpath>,
    /// What has been drawn since the frame began.
    pub(crate) ops: Vec<CanvasOp>,
}

const IDENTITY: [f64; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

impl Default for Recorder {
    fn default() -> Self {
        Self {
            t: IDENTITY,
            saved: Vec::new(),
            path: Vec::new(),
            ops: Vec::new(),
        }
    }
}

/// Canvas-pixel points, and where the subpath is in user space: its last
/// point, which a curve starts from, and its first, which closing returns to.
struct Subpath {
    points: Vec<[f32; 2]>,
    closed: bool,
    user: [f64; 2],
    first: [f64; 2],
}

/// How many straight pieces a curve becomes, by its length on screen.
fn pieces(length: f64) -> usize {
    if length.is_nan() {
        return 8;
    }
    (length / 4.0).ceil().clamp(8.0, 256.0) as usize
}

impl Recorder {
    fn map(&self, x: f64, y: f64) -> [f32; 2] {
        let [a, b, c, d, e, f] = self.t;
        [finite(a * x + c * y + e), finite(b * x + d * y + f)]
    }

    /// How much the transform scales lengths, for line widths and font sizes.
    fn unit(&self) -> f64 {
        let [a, b, c, d, ..] = self.t;
        let unit = (a * d - b * c).abs().sqrt();
        if unit > 0.0 { unit } else { 1.0 }
    }

    fn transform(&mut self, [a, b, c, d, e, f]: [f64; 6]) {
        let [ta, tb, tc, td, te, tf] = self.t;
        self.t = [
            ta * a + tc * b,
            tb * a + td * b,
            ta * c + tc * d,
            tb * c + td * d,
            ta * e + tc * f + te,
            tb * e + td * f + tf,
        ];
    }

    fn move_to(&mut self, x: f64, y: f64) {
        let points = vec![self.map(x, y)];
        self.path.push(Subpath {
            points,
            closed: false,
            user: [x, y],
            first: [x, y],
        });
    }

    fn line_to(&mut self, x: f64, y: f64) {
        let p = self.map(x, y);
        match self.path.last_mut() {
            Some(sub) => {
                sub.points.push(p);
                sub.user = [x, y];
            }
            None => self.move_to(x, y),
        }
    }

    fn close_path(&mut self) {
        let Some(sub) = self.path.last_mut() else {
            return;
        };
        sub.closed = true;
        // The next segment starts where this subpath did.
        let (first, start) = (sub.first, sub.points[0]);
        self.path.push(Subpath {
            points: vec![start],
            closed: false,
            user: first,
            first,
        });
    }

    fn rect(&mut self, [x, y, w, h]: [f64; 4]) {
        self.move_to(x, y);
        self.line_to(x + w, y);
        self.line_to(x + w, y + h);
        self.line_to(x, y + h);
        self.close_path();
    }

    fn ellipse(&mut self, [x, y, rx, ry, rotation, start, end]: [f64; 7], ccw: bool) {
        let tau = std::f64::consts::TAU;
        let mut sweep = end - start;
        if !ccw && sweep < 0.0 {
            sweep = sweep % tau + tau;
        }
        if ccw && sweep > 0.0 {
            sweep = sweep % tau - tau;
        }
        if (end - start).abs() >= tau {
            sweep = if ccw { -tau } else { tau };
        }
        let n = pieces(sweep.abs() * rx.max(ry) * self.unit());
        let (sin, cos) = rotation.sin_cos();
        for i in 0..=n {
            let (sa, ca) = (start + sweep * i as f64 / n as f64).sin_cos();
            let (ex, ey) = (rx * ca, ry * sa);
            self.line_to(x + ex * cos - ey * sin, y + ex * sin + ey * cos);
        }
    }

    fn bezier(&mut self, [c1x, c1y, c2x, c2y, x, y]: [f64; 6]) {
        let [x0, y0] = match self.path.last() {
            Some(sub) => sub.user,
            None => {
                self.move_to(x, y);
                [x, y]
            }
        };
        let n = pieces((x - x0).hypot(y - y0) * self.unit());
        for i in 1..=n {
            let s = i as f64 / n as f64;
            let u = 1.0 - s;
            let (a, b, c, d) = (u * u * u, 3.0 * u * u * s, 3.0 * u * s * s, s * s * s);
            self.line_to(
                a * x0 + b * c1x + c * c2x + d * x,
                a * y0 + b * c1y + c * c2y + d * y,
            );
        }
    }

    /// The same curve as a cubic, its control points two thirds of the way
    /// from each end to (cx, cy).
    fn quadratic(&mut self, [cx, cy, x, y]: [f64; 4]) {
        let [x0, y0] = self.path.last().map_or([x, y], |sub| sub.user);
        let k = 2.0 / 3.0;
        self.bezier([
            x0 + k * (cx - x0),
            y0 + k * (cy - y0),
            x + k * (cx - x),
            y + k * (cy - y),
            x,
            y,
        ]);
    }

    fn corners(&self, [x, y, w, h]: [f64; 4]) -> Vec<[f32; 2]> {
        vec![
            self.map(x, y),
            self.map(x + w, y),
            self.map(x + w, y + h),
            self.map(x, y + h),
        ]
    }

    fn fill(&mut self, color: String, alpha: f32, even_odd: bool) {
        let polygons = self
            .path
            .iter()
            .filter(|sub| sub.points.len() >= 3)
            .map(|sub| sub.points.clone())
            .collect();
        self.ops.push(CanvasOp::Fill {
            color,
            alpha,
            even_odd,
            polygons,
        });
    }

    fn stroke(&mut self, style: Stroke, lines: Vec<(bool, Vec<[f32; 2]>)>) {
        let Stroke {
            color,
            alpha,
            width,
            cap,
            join,
        } = style;
        self.ops.push(CanvasOp::Stroke {
            color,
            alpha,
            width: finite(width * self.unit()),
            cap,
            join,
            lines,
        });
    }

    fn path_lines(&self) -> Vec<(bool, Vec<[f32; 2]>)> {
        self.path
            .iter()
            .filter(|sub| sub.points.len() >= 2)
            .map(|sub| (sub.closed, sub.points.clone()))
            .collect()
    }
}

/// How a stroke is drawn, as the script's context says: `[color, alpha,
/// lineWidth, lineCap, lineJoin]` from argument `at` on.
struct Stroke {
    color: String,
    alpha: f32,
    width: f64,
    cap: String,
    join: String,
}

impl Stroke {
    fn read(a: &[Arg], at: usize) -> Self {
        Stroke {
            color: text(a.get(at)),
            alpha: finite(num(a, at + 1)),
            width: num(a, at + 2),
            cap: text(a.get(at + 3)),
            join: text(a.get(at + 4)),
        }
    }
}

/// A number for the app, which draws nothing sensible from NaN or infinity.
fn finite(v: f64) -> f32 {
    if v.is_finite() { v as f32 } else { 0.0 }
}

fn num(a: &[Arg], i: usize) -> f64 {
    arg_to_f64(a.get(i).unwrap_or(js::NULL))
}

fn nums<const N: usize>(a: &[Arg]) -> [f64; N] {
    std::array::from_fn(|i| num(a, i))
}

fn truthy(arg: Option<&Arg>) -> bool {
    match arg {
        Some(Arg::Bool(b)) => *b,
        Some(Arg::Num(n)) => *n != 0.0 && !n.is_nan(),
        Some(Arg::Str(s)) => !s.is_empty(),
        Some(Arg::Null) | None => false,
        Some(_) => true,
    }
}

fn text(arg: Option<&Arg>) -> String {
    match arg {
        Some(Arg::Str(s)) => s.clone(),
        _ => String::new(),
    }
}

/// The context's natives, on `__canvas`. The path and transform methods are
/// the canvas API's own, which `canvas.js` puts on its context as they are;
/// the drawing ones (`…With`) take the style the script's context holds.
pub(crate) fn register(prelude: &Scope) {
    let ns = prelude.namespace("__canvas");
    let on = |name: &str, f: fn(&mut Recorder, &[Arg])| {
        ns.func(name, move |a| {
            js::with_canvas(|c| f(c, a));
            Ok(Arg::Null)
        });
    };
    on("beginPath", |c, _| c.path.clear());
    on("moveTo", |c, a| {
        let [x, y] = nums(a);
        c.move_to(x, y);
    });
    on("lineTo", |c, a| {
        let [x, y] = nums(a);
        c.line_to(x, y);
    });
    on("closePath", |c, _| c.close_path());
    on("rect", |c, a| c.rect(nums(a)));
    on("roundRect", |c, a| c.rect(nums(a)));
    on("ellipse", |c, a| c.ellipse(nums(a), truthy(a.get(7))));
    on("arc", |c, a| {
        let [x, y, r, start, end] = nums(a);
        c.ellipse([x, y, r, r, 0.0, start, end], truthy(a.get(5)));
    });
    on("arcTo", |c, a| {
        let [x1, y1, x2, y2] = nums(a);
        c.line_to(x1, y1);
        c.line_to(x2, y2);
    });
    on("bezierCurveTo", |c, a| c.bezier(nums(a)));
    on("quadraticCurveTo", |c, a| c.quadratic(nums(a)));
    // --- transforms
    on("setTransform", |c, a| {
        c.t = match a.first() {
            Some(m @ Arg::Map(_)) => {
                ["a", "b", "c", "d", "e", "f"].map(|k| arg_to_f64(m.get(k).unwrap_or(js::NULL)))
            }
            _ => nums(a),
        };
    });
    on("resetTransform", |c, _| c.t = IDENTITY);
    on("transform", |c, a| c.transform(nums(a)));
    on("translate", |c, a| {
        let [x, y] = nums(a);
        c.transform([1.0, 0.0, 0.0, 1.0, x, y]);
    });
    on("scale", |c, a| {
        let x = num(a, 0);
        let y = if a.get(1).is_none_or(Arg::is_null) {
            x
        } else {
            num(a, 1)
        };
        c.transform([x, 0.0, 0.0, y, 0.0, 0.0]);
    });
    on("rotate", |c, a| {
        let (sin, cos) = num(a, 0).sin_cos();
        c.transform([cos, sin, -sin, cos, 0.0, 0.0]);
    });
    ns.func("getTransform", |_| {
        let t = js::with_canvas(|c| c.t);
        let keys = ["a", "b", "c", "d", "e", "f"];
        Ok(Arg::Map(
            keys.iter()
                .zip(t)
                .map(|(k, v)| (k.to_string(), Arg::Num(v)))
                .collect(),
        ))
    });
    on("save", |c, _| c.saved.push(c.t));
    on("restore", |c, _| {
        if let Some(t) = c.saved.pop() {
            c.t = t;
        }
    });
    on("reset", |c, _| {
        c.t = IDENTITY;
        c.saved.clear();
        c.path.clear();
    });
    // --- drawing
    on("clearRect", |c, a| {
        let polygon = c.corners(nums(a));
        c.ops.push(CanvasOp::Clear { polygon });
    });
    // (color, alpha, evenOdd)
    on("fillWith", |c, a| {
        c.fill(text(a.first()), finite(num(a, 1)), truthy(a.get(2)));
    });
    // (color, alpha, lineWidth, lineCap, lineJoin)
    on("strokeWith", |c, a| {
        let lines = c.path_lines();
        c.stroke(Stroke::read(a, 0), lines);
    });
    // (x, y, w, h, color, alpha)
    on("fillRectWith", |c, a| {
        let polygons = vec![c.corners(nums(a))];
        c.ops.push(CanvasOp::Fill {
            color: text(a.get(4)),
            alpha: finite(num(a, 5)),
            even_odd: false,
            polygons,
        });
    });
    // (x, y, w, h, color, alpha, lineWidth, lineCap, lineJoin)
    on("strokeRectWith", |c, a| {
        let lines = vec![(true, c.corners(nums(a)))];
        c.stroke(Stroke::read(a, 4), lines);
    });
    // (text, x, y, size, color, alpha, align, baseline), size before the
    // transform scales it.
    on("textWith", |c, a| {
        let [x, y] = c.map(num(a, 1), num(a, 2));
        let size = finite(num(a, 3) * c.unit());
        c.ops.push(CanvasOp::Text {
            text: text(a.first()),
            x,
            y,
            size,
            color: text(a.get(4)),
            alpha: finite(num(a, 5)),
            align: text(a.get(6)),
            baseline: text(a.get(7)),
        });
    });
}
