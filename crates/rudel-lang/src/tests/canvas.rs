use crate::canvas::{CanvasDriver, CanvasEvent, CanvasOp};

/// The canvas driver, and the pattern that keeps its script alive.
fn driver(src: &str) -> (CanvasDriver, rudel_core::Pattern) {
    let result = crate::eval_result(src).expect("eval");
    (
        result.meta.canvas.expect("the script draws"),
        result.pattern,
    )
}

#[test]
fn a_script_that_draws_nothing_has_no_canvas() {
    assert!(
        crate::eval_result("s(\"bd\")")
            .unwrap()
            .meta
            .canvas
            .is_none()
    );
}

#[test]
fn draw_hands_its_function_the_haps_and_the_time() {
    // 17330 "draw example" by froos, as shared on strudel.cc.
    let (d, _alive) = driver(
        "s(\"bd -, hh*4\")\n.draw((haps, t) => {\n  const ctx = getDrawContext()\n  \
         ctx.clearRect(0,0,ctx.canvas.width,ctx.canvas.height);\n  ctx.font = '160px sans';\n  \
         ctx.fillStyle = 'tomato'\n  ctx.fillText(t.toFixed(2) + ', ' + haps.length, 430, ctx.canvas.height-50);\n},{})",
    );
    let ops = d.frame(0.5, 0.0, 800, 600, &[]).expect("a frame");
    assert_eq!(
        ops[0],
        CanvasOp::Clear {
            polygon: vec![[0.0, 0.0], [800.0, 0.0], [800.0, 600.0], [0.0, 600.0]]
        }
    );
    let CanvasOp::Text {
        text,
        x,
        y,
        size,
        color,
        ..
    } = &ops[1]
    else {
        panic!("{ops:?}");
    };
    // The first frame's memory: the onsets at its time (lookahead 0), which
    // at 0.5 is one hh.
    assert_eq!(
        (text.as_str(), *x, *y, *size, color.as_str()),
        ("0.50, 1", 430.0, 550.0, 160.0, "tomato")
    );
}

#[test]
fn animation_frames_run_once_each_and_paths_are_transformed() {
    let (d, _alive) = driver(
        "const ctx = getDrawContext()\nlet n = 0\nfunction render(t) {\n  n++\n  \
         ctx.save(); ctx.translate(10, 20); ctx.scale(2)\n  ctx.beginPath(); ctx.moveTo(0, 0); ctx.lineTo(5, 0)\n  \
         ctx.lineWidth = 3; ctx.strokeStyle = 'red'; ctx.stroke(); ctx.restore()\n  \
         ctx.fillStyle = '#00ff00'; ctx.fillRect(0, 0, n, n)\n  requestAnimationFrame(render)\n}\n\
         requestAnimationFrame(render)\ns(\"bd\")",
    );
    let first = d.frame(0.0, 16.0, 100, 100, &[]).expect("a frame");
    assert_eq!(
        first[0],
        CanvasOp::Stroke {
            color: "red".into(),
            alpha: 1.0,
            width: 6.0,
            cap: "butt".into(),
            join: "miter".into(),
            lines: vec![(false, vec![[10.0, 20.0], [20.0, 20.0]])],
        }
    );
    // `restore` undid the transform; the callback re-armed itself once.
    let second = d.frame(0.1, 32.0, 100, 100, &[]).expect("a frame");
    let CanvasOp::Fill { polygons, .. } = &second[1] else {
        panic!("{second:?}")
    };
    assert_eq!(polygons[0][2], [2.0, 2.0], "n is 2 on the second frame");
}

#[test]
fn animate_draws_its_shapes_over_a_smearing_clear() {
    let (d, _alive) =
        driver("x(0.5).y(0.5).w(0.1).h(0.1).s('rect').fill('red').animate({ smear: 0.5 })");
    let ops = d.frame(0.0, 1000.0, 1000, 500, &[]).expect("a frame");
    let colors: Vec<&str> = ops
        .iter()
        .map(|op| match op {
            CanvasOp::Fill { color, .. } => color.as_str(),
            _ => "?",
        })
        .collect();
    assert_eq!(colors, ["#20001050", "red"]);
    let CanvasOp::Fill { polygons, .. } = &ops[1] else {
        unreachable!()
    };
    // Sized when `animate` is called, as upstream: before any frame, the
    // default 1280x720. x 0.5 of (1280 - w 128) = 576; y 0.5 of (720 - 72).
    assert_eq!(polygons[0][0], [576.0, 324.0]);
}

#[test]
fn document_handlers_hear_the_events_the_app_sends() {
    // 8030 "Techno mouse" by Enelg, cut down: `document.onmousemove` steers a
    // value that `ref` reads as the pattern is queried.
    let (d, pattern) = driver(
        "let _x = 0
document.onmousemove = (e) => { _x = e.clientX / document.body.clientWidth * 10 }
         let _ctrl = 0
document.addEventListener('keydown', (e) => { _ctrl = e.ctrlKey ? 1 : 0 })
         n(ref(() => _x + _ctrl * 100))",
    );
    let n_at = |pattern: &rudel_core::Pattern| {
        let haps = pattern.query_arc(rudel_core::Frac::zero(), rudel_core::Frac::one());
        match &haps[0].value {
            rudel_core::Value::Map(m) => m.get("n").and_then(rudel_core::Value::as_f64),
            other => other.as_f64(),
        }
    };
    assert_eq!(n_at(&pattern), Some(0.0));
    let moved = CanvasEvent {
        kind: "mousemove",
        x: 50.0,
        y: 10.0,
        ..CanvasEvent::default()
    };
    d.frame(0.0, 0.0, 100, 100, &[moved]).expect("a frame");
    assert_eq!(n_at(&pattern), Some(5.0));
    let ctrl = CanvasEvent {
        kind: "keydown",
        key: "Control".into(),
        code: "ControlLeft".into(),
        ctrl: true,
        ..CanvasEvent::default()
    };
    d.frame(0.0, 0.0, 100, 100, &[ctrl]).expect("a frame");
    assert_eq!(n_at(&pattern), Some(105.0));
}

#[test]
fn curves_are_flattened_through_the_transform() {
    let (d, _alive) = driver(
        "requestAnimationFrame(() => {\n  const ctx = getDrawContext()\n  ctx.translate(100, 0)\n  \
         ctx.beginPath()\n  ctx.arc(0, 0, 10, 0, Math.PI)\n  ctx.quadraticCurveTo(0, 20, 20, 0)\n  \
         ctx.closePath()\n  ctx.stroke()\n})\ns(\"bd\")",
    );
    let ops = d.frame(0.0, 0.0, 800, 600, &[]).expect("a frame");
    let [CanvasOp::Stroke { lines, .. }] = ops.as_slice() else {
        panic!("{ops:?}");
    };
    let [(true, points)] = lines.as_slice() else {
        panic!("{lines:?}");
    };
    // Eight pieces of half a circle (y down), then eight of the curve.
    assert_eq!(points.len(), 17);
    let near = |i: usize, [x, y]: [f32; 2]| {
        let [px, py] = points[i];
        assert!(
            (px - x).abs() < 1e-3 && (py - y).abs() < 1e-3,
            "{i}: {:?}",
            points[i]
        );
    };
    near(0, [110.0, 0.0]);
    near(4, [100.0, 10.0]);
    near(8, [90.0, 0.0]);
    // Halfway along the quadratic from (-10, 0) by (0, 20) to (20, 0).
    near(12, [102.5, 10.0]);
    near(16, [120.0, 0.0]);
}
