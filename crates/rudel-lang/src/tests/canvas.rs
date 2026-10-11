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

fn near(got: [f32; 2], want: [f32; 2]) {
    assert!(
        (got[0] - want[0]).abs() < 1e-3 && (got[1] - want[1]).abs() < 1e-3,
        "{got:?} against {want:?}"
    );
}

/// The ops of the first frame of a script, drawn on 1280 x 720.
fn first_frame(src: &str) -> Vec<CanvasOp> {
    let (d, _alive) = driver(src);
    d.frame(1.0, 16.0, 1280, 720, &[]).expect("a frame")
}

fn said(op: &CanvasOp) -> &str {
    match op {
        CanvasOp::Text { text, .. } => text,
        other => panic!("not text: {other:?}"),
    }
}

fn polygon(op: &CanvasOp) -> &[[f32; 2]] {
    match op {
        CanvasOp::Fill { polygons, .. } => &polygons[0],
        other => panic!("not a fill: {other:?}"),
    }
}

#[test]
fn a_matrix_object_sets_the_transform_and_rotate_turns_it() {
    let ops = first_frame(
        "requestAnimationFrame(() => {\n  const c = getDrawContext()\n  \
         c.setTransform({ a: 2, b: 0, c: 0, d: 2, e: 10, f: 20 }); c.fillRect(0, 0, 1, 1)\n  \
         c.resetTransform(); c.rotate(Math.PI / 2); c.fillRect(0, 0, 2, 1)\n  \
         c.resetTransform(); c.scale(3); c.font = '10px sans'; c.fillText('a', 1, 1)\n})\ns(\"bd\")",
    );
    assert_eq!(
        polygon(&ops[0]),
        [[10.0, 20.0], [12.0, 20.0], [12.0, 22.0], [10.0, 22.0]]
    );
    // A quarter turn clockwise (y down): x runs down, y runs left.
    let turned = polygon(&ops[1]);
    for (got, want) in turned
        .iter()
        .zip([[0.0, 0.0], [0.0, 2.0], [-1.0, 2.0], [-1.0, 0.0]])
    {
        near(*got, want);
    }
    let CanvasOp::Text { x, y, size, .. } = &ops[2] else {
        panic!("{ops:?}");
    };
    assert_eq!((*x, *y, *size), (3.0, 3.0, 30.0));
}

#[test]
fn styles_fill_rules_and_the_methods_that_draw_nothing() {
    let ops = first_frame(
        "requestAnimationFrame(() => {\n  const c = getDrawContext()\n  \
         const g = c.createLinearGradient(0, 0, 1, 1); g.addColorStop(0, 'blue')\n  \
         c.fillStyle = g; c.fillRect(0, 0, 1, 1)\n  \
         c.drawImage(); c.putImageData(); c.setLineDash([2]); c.clip()\n  \
         c.fillStyle = 'red'; c.beginPath(); c.rect(0, 0, 4, 4)\n  \
         c.fill(null, 'evenodd'); c.fill('evenodd'); c.fill({}, 'nonzero')\n  \
         c.fillText([c.fill.length, c.createImageData.length, c.fillText.length].join(), 0, 0)\n})\ns(\"bd\")",
    );
    let fills: Vec<(&str, bool)> = ops
        .iter()
        .filter_map(|op| match op {
            CanvasOp::Fill {
                color, even_odd, ..
            } => Some((color.as_str(), *even_odd)),
            _ => None,
        })
        .collect();
    assert_eq!(
        fills,
        [
            ("blue", false),
            ("red", true),
            ("red", true),
            ("red", false)
        ]
    );
    // The declared arities scripts can read.
    assert_eq!(said(ops.last().unwrap()), "1,2,3");
}

#[test]
fn frame_ids_are_distinct_and_events_can_be_prevented() {
    let (d, _alive) = driver(
        "let seen = ''\nrequestAnimationFrame(() => { seen += 'a' })\n\
         const b = requestAnimationFrame(() => { seen += 'b' })\ncancelAnimationFrame(b)\n\
         document.onmousedown = e => { e.preventDefault(); e.stopPropagation(); seen += 'd' }\n\
         requestAnimationFrame(() => getDrawContext().fillText(seen, 0, 0))\ns(\"bd\")",
    );
    let down = CanvasEvent {
        kind: "mousedown",
        ..CanvasEvent::default()
    };
    let ops = d.frame(0.0, 0.0, 100, 100, &[down]).expect("a frame");
    assert_eq!(said(&ops[0]), "da");
}

#[test]
fn on_paint_sees_two_cycles_either_side_and_haps_answer_where_they_are() {
    let ops = first_frame(
        "s(\"bd\").onPaint((ctx, time, haps) => {\n  \
         const h = haps.find(h => Number(h.whole.begin) === 0)\n  \
         ctx.fillText([haps.length, h.isInFuture(0), h.isInFuture(-0.1), h.isActive(1.1), \
         h.isActive(0.5)].join(), 0, 0)\n})",
    );
    assert_eq!(said(&ops[0]), "4,false,true,false,true");
}

#[test]
fn animate_places_a_shape_by_radius_and_angle() {
    // From the middle of the room left by a 128 x 72 shape, (576, 324), half
    // way out at an eighth of a turn.
    let ops = first_frame(
        "pure({ r: 0.5, angle: 0.125, w: 0.1, h: 0.1, s: 'rect', fill: 'red' }).animate({ smear: 0 })",
    );
    let (s, c) = std::f32::consts::FRAC_PI_4.sin_cos();
    near(
        polygon(&ops[1])[0],
        [576.0 + c * 0.5 * 576.0, 324.0 + s * 0.5 * 324.0],
    );
}

#[test]
fn animate_draws_an_ellipse_inside_its_box() {
    // A 128 x 144 box at (576, 288): the ellipse's right end first, all the
    // way round back to it.
    let ops =
        first_frame("x(0.5).y(0.5).w(0.1).h(0.2).s('ellipse').fill('red').animate({ smear: 0 })");
    let points = polygon(&ops[1]);
    near(points[0], [704.0, 360.0]);
    near(*points.last().unwrap(), [704.0, 360.0]);
    let fold = |f: fn(f32, f32) -> f32, axis: usize, start: f32| {
        points.iter().map(|p| p[axis]).fold(start, f)
    };
    assert!((fold(f32::min, 0, f32::MAX) - 576.0).abs() < 0.1);
    assert!((fold(f32::max, 1, f32::MIN) - 432.0).abs() < 0.1);
}

#[test]
fn a_driver_is_equal_to_no_other() {
    let (d, _alive) = driver("requestAnimationFrame(() => {})\ns(\"bd\")");
    assert_ne!(d, d);
    assert_eq!(format!("{d:?}"), "CanvasDriver");
}
