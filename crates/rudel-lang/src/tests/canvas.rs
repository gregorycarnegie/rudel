use crate::canvas::{CanvasDriver, CanvasOp};

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
    let ops = d.frame(0.5, 0.0, 800, 600).expect("a frame");
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
    let first = d.frame(0.0, 16.0, 100, 100).expect("a frame");
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
    let second = d.frame(0.1, 32.0, 100, 100).expect("a frame");
    let CanvasOp::Fill { polygons, .. } = &second[1] else {
        panic!("{second:?}")
    };
    assert_eq!(polygons[0][2], [2.0, 2.0], "n is 2 on the second frame");
}

#[test]
fn animate_draws_its_shapes_over_a_smearing_clear() {
    let (d, _alive) =
        driver("x(0.5).y(0.5).w(0.1).h(0.1).s('rect').fill('red').animate({ smear: 0.5 })");
    let ops = d.frame(0.0, 1000.0, 1000, 500).expect("a frame");
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
