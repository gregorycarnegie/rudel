//! Strudel's draw canvas, as far as the script side goes: the frame driver an
//! evaluation hands the app, and the drawing it returns (`bindings/canvas.js`).

use crate::js::{self, Arg, SendFn};

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

/// What draws on the canvas for one evaluation: its painters,
/// `requestAnimationFrame` callbacks and `animate`, run once a frame.
#[derive(Clone, Copy)]
pub struct CanvasDriver(SendFn);

impl CanvasDriver {
    pub(crate) fn new(frame: SendFn) -> Self {
        Self(frame)
    }

    /// Run one frame at transport time `cycle` and clock `ms`, on a canvas of
    /// `width` by `height` CSS pixels, and return what it drew. `None` once
    /// the evaluation is gone.
    pub fn frame(&self, cycle: f64, ms: f64, width: u32, height: u32) -> Option<Vec<CanvasOp>> {
        self.0.run(move |f| {
            let args = [cycle, ms, f64::from(width), f64::from(height)].map(Arg::Num);
            match js::call(f, args.to_vec()) {
                Ok(Arg::List(ops)) => ops.iter().filter_map(op).collect(),
                _ => Vec::new(),
            }
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

fn num(arg: Option<&Arg>) -> f32 {
    match arg {
        Some(Arg::Num(n)) if n.is_finite() => *n as f32,
        _ => 0.0,
    }
}

fn text(arg: Option<&Arg>) -> String {
    match arg {
        Some(Arg::Str(s)) => s.clone(),
        _ => String::new(),
    }
}

/// A flat `[x0, y0, x1, y1, …]` list as points.
fn points(arg: Option<&Arg>) -> Vec<[f32; 2]> {
    let Some(Arg::List(flat)) = arg else {
        return Vec::new();
    };
    flat.as_chunks::<2>()
        .0
        .iter()
        .map(|[x, y]| [num(Some(x)), num(Some(y))])
        .collect()
}

fn list(arg: Option<&Arg>) -> &[Arg] {
    match arg {
        Some(Arg::List(items)) => items,
        _ => &[],
    }
}

/// One op as `canvas.js` records it: `[kind, …fields]`.
fn op(arg: &Arg) -> Option<CanvasOp> {
    let Arg::List(f) = arg else {
        return None;
    };
    Some(match text(f.first()).as_str() {
        "fill" => CanvasOp::Fill {
            color: text(f.get(1)),
            alpha: num(f.get(2)),
            even_odd: matches!(f.get(3), Some(Arg::Bool(true))),
            polygons: list(f.get(4)).iter().map(|p| points(Some(p))).collect(),
        },
        "stroke" => CanvasOp::Stroke {
            color: text(f.get(1)),
            alpha: num(f.get(2)),
            width: num(f.get(3)),
            cap: text(f.get(4)),
            join: text(f.get(5)),
            lines: list(f.get(6))
                .iter()
                .map(|line| {
                    let line = list(Some(line));
                    (
                        matches!(line.first(), Some(Arg::Bool(true))),
                        points(line.get(1)),
                    )
                })
                .collect(),
        },
        "clear" => CanvasOp::Clear {
            polygon: points(f.get(1)),
        },
        "text" => CanvasOp::Text {
            text: text(f.get(1)),
            x: num(f.get(2)),
            y: num(f.get(3)),
            size: num(f.get(4)),
            color: text(f.get(5)),
            alpha: num(f.get(6)),
            align: text(f.get(7)),
            baseline: text(f.get(8)),
        },
        _ => return None,
    })
}
