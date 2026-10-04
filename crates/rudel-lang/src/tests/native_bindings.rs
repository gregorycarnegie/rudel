//! Migration checks for the native prelude and canvas, including GC/re-entry.
use crate::js::{self, Arg};

fn check(source: &'static str) {
    let _lock = crate::EVAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    js::on_js_thread(move || {
        let (mut ctx, _) = crate::prepare();
        let result = js::run(&mut ctx, source).unwrap_or_else(|e| panic!("{e}\n{source}"));
        assert!(matches!(result, Arg::Bool(true)), "check failed: {source}");
    });
}

#[test]
fn canvas_objects_keep_identity_properties_and_style_stack() {
    check(
        r#"
        const c = getDrawContext();
        const g = c.createLinearGradient(); g.addColorStop(0, 'red'); g.addColorStop(1, 'blue');
        const extra = {x: 1}; c.extra = extra; c.fillStyle = g; c.lineWidth = 7;
        c.save(); c.fillStyle = 'blue'; c.lineWidth = 2; c.extra = {}; c.restore();
        const restored = c.fillStyle === g && c.extra === extra && c.lineWidth === 7;
        const img = c.createImageData(2, 3);
        c.font = 'italic 12.5px sans';
        const metrics = c.measureText('😀');
        const before = c.canvas.width; c.canvas.width = 42;
        restored && g.color === 'red' && img.data instanceof Uint8ClampedArray && img.data.length === 24 &&
        metrics.width === 13.750000000000002 && c.fontSize() === 12.5 && c.canvas.width === before &&
        document.createElement('canvas') === c.canvas && document.createElement('div').getContext() === c &&
        getDrawContext() === c && window === globalThis && document.body === document.documentElement
    "#,
    );
}

#[test]
fn animation_queue_and_document_dispatch_allow_reentry() {
    check(
        r#"
        const c = getDrawContext(); let seen = []; let saved;
        const removed = () => seen.push('removed');
        document.addEventListener('mousemove', removed); document.removeEventListener('mousemove', removed);
        document.onmousemove = e => { saved = e; seen.push('handler'); document.addEventListener('mousemove', () => seen.push('late')); };
        document.addEventListener('mousemove', e => seen.push(e === saved ? 'same' : 'different'));
        const cancelled = requestAnimationFrame(() => seen.push('cancelled'));
        const removedFrame = cancelAnimationFrame(cancelled);
        requestAnimationFrame(ms => { seen.push(ms); requestAnimationFrame(() => seen.push('next')); });
        __drawFrame(2, 16, 320, 240, [['mousemove', 10, 20, 0, '', '', false, false, false, false]]);
        __drawFrame(3, 32, 321, 241);
        removedFrame && cancelAnimationFrame(cancelled) === false &&
        JSON.stringify(seen) === '["handler","same",16,"next"]' &&
        innerWidth === 321 && innerHeight === 241 && c.canvas.clientWidth === 321 &&
        Hydra.mouse.x === 10 && Hydra.mouse.y === 20 && getTime() === 3 && saved.target === document.body
    "#,
    );
}

#[test]
fn painter_error_reporting_propagates_a_failing_script_logger() {
    check(
        r#"
        console.log = () => { throw 'logger failed'; };
        s('bd').draw(() => { throw 'painter failed'; });
        let caught;
        try { __drawFrame(0,0,100,100); } catch (error) { caught = error; }
        caught === 'logger failed'
    "#,
    );
}

#[test]
fn painters_keep_hap_identity_and_use_the_original_callback_receivers() {
    check(
        r#"
        let kept; let seen = []; const pat = s('bd');
        pat.draw(function(haps, time, ahead, p) {
            seen.push(this.pattern === p);
            if (!kept) { kept = haps[0]; kept.custom = 42; }
            else seen.push(haps[0] === kept && haps[0].custom === 42);
            seen.push(kept.hasOnset() && kept.isActive(0.1) && !kept.isInFuture(0.1) && kept.endClipped === 1);
        }, {lookbehind: 1});
        pat.onPaint(function(c, time, haps, span) {
            seen.push(this.pattern === pat && c === getDrawContext() && span[0] === -2 && span[1] === 2 && haps.length > 0);
        });
        __drawFrame(0, 0, 100, 100); __drawFrame(0.05, 50, 100, 100);
        seen.length === 7 && seen.every(Boolean)
    "#,
    );
}

#[test]
fn dynamic_controls_and_native_currying_preserve_values() {
    check(
        r##"
        const ab = createParam(['aa', 'bb']);
        const originalReify = reify;
        globalThis.reify = value => ({withValue: mapper => mapper(value)});
        const tagged = ab({value: [5, 6], color: 'red'});
        globalThis.reify = originalReify;
        const mapped = pure(7).aa().queryArc(0, 1)[0].value;
        const bound = bind(x => pure(x + 1));
        const nested = bound(pure(2)).queryArc(0, 1)[0].value;
        tagged.aa === 5 && tagged.bb === 6 && tagged.color === 'red' && !('value' in tagged) &&
        mapped.aa === 7 && nested === 3 &&
        JSON.stringify(tokenizeNote('C#-2')) === '["C","#",-2]' &&
        JSON.stringify(tokenizeNote('c-')) === '["c","",null]' &&
        tokenizeNote('h4').length === 0 && tokenizeNote(42).length === 0
    "##,
    );
}

#[test]
fn hydra_array_helpers_and_analyser_keep_mutable_script_state() {
    check(
        r#"
        const promise = initHydra({detectAudio: true});
        const xs = [2, 4, 6].fast(2).smooth(0.5).ease('sin').offset(-1.25);
        const fitted = xs.fit(10, 20);
        a.setBins(2); a.setScale(5); a.setSmooth(0.25); a.setCutoff(1); a.setMax(20);
        let beats = 0; a.onBeat = () => beats++;
        Hydra._loudness = () => ({frame: 1, specific: [10, 20, 30, 40]});
        a.tick(); a.tick();
        const read = a0(2, 1);
        promise instanceof Promise && xs._offset === -0.25 &&
        JSON.stringify(fitted) === '[10,15,20]' && fitted._speed === 2 && fitted._smooth === 1 && fitted._ease === 'sin' &&
        a.settings[1].scale === 5 && a.max === 20 && beats === 1 &&
        a.vol === 100 && a.bins[0] === 22.5 && a.bins[1] === 52.5 && read() === 9.6 &&
        s0.initCam(2) === s0 && s1.initVideo('movie.mp4') === s1 && s2.initScreen() === s2 && s3.initImage('image.png') === s3
    "#,
    );
}

#[test]
fn traced_native_captures_and_canvas_roots_survive_collection() {
    let _lock = crate::EVAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    js::on_js_thread(|| {
        let (mut ctx, _) = crate::prepare();
        js::run(&mut ctx, r#"
            const control = createParam('custom'); const p = pure(2).custom(3);
            const f = bind(x => pure(x + 5));
            initHydra({detectAudio:true}); let rendered = 0; let moved = 0;
            document.onmousemove = e => moved = e.clientX;
            p.draw(haps => { rendered++; getDrawContext().fillStyle = 'red'; });
            let animations = 0; function animateAgain() { animations++; requestAnimationFrame(animateAgain); }
            requestAnimationFrame(animateAgain);
        "#).unwrap();
        for i in 0..8 {
            boa_gc::force_collect();
            let source = format!(
                r#"
                __drawFrame({i} / 20, {i}, 100, 100, [['mousemove', 7, 0, 0]]);
                rendered === {i} + 1 && animations === {i} + 1 && moved === 7 &&
                f(pure(1)).queryArc(0,1)[0].value === 6 && p.queryArc(0,1)[0].value.custom === 3
            "#
            );
            assert!(matches!(
                js::run(&mut ctx, &source).unwrap(),
                Arg::Bool(true)
            ));
        }
    });
}

// Outputs captured from the original JS implementations before their deletion.
#[test]
fn native_bindings_match_pre_migration_results() {
    let cases: Vec<(String, serde_json::Value)> =
        serde_json::from_str(include_str!("native_bindings_golden.json")).unwrap();
    let _lock = crate::EVAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    js::on_js_thread(move || {
        for (source, expected) in cases {
            let (mut ctx, _) = crate::prepare();
            let result = js::run(&mut ctx, &source).unwrap_or_else(|e| panic!("{source}: {e}"));
            let Arg::Str(actual) = result else {
                panic!("expected JSON: {source}")
            };
            let actual: serde_json::Value = serde_json::from_str(&actual).unwrap();
            assert_eq!(actual, expected, "{source}");
        }
    });
}
#[test]
fn native_painter_iteration_still_obeys_engine_limits() {
    let _lock = crate::EVAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    js::on_js_thread(|| {
        let (mut ctx, _) = crate::prepare();
        js::run(
            &mut ctx,
            "function addPainter(){s('bd').draw(addPainter)};addPainter()",
        )
        .unwrap();
        ctx.runtime_limits_mut().set_loop_iteration_limit(3);
        assert!(js::run(&mut ctx, "__drawFrame(0,0,100,100)").is_err());
    });
}

#[test]
fn native_bindings_preserve_proxy_getter_and_conversion_semantics() {
    check(
        r#"
        const names = new Proxy(['aa', 'bb'], {});
        const ab = createParam(names); names[0] = 'renamed';
        const originalReify = reify;
        globalThis.reify = value => ({withValue: mapper => mapper(value)});
        let reads = 0;
        const tagged = ab({get value() { reads++; return new Proxy([5,6], {}); }, color:'red'});
        const scalar = ab(7);
        globalThis.reify = originalReify;
        const c = getDrawContext(); c.fillText(Symbol('x'), 0, 0);
        const symbolWidth = c.measureText(Symbol('x')).width;
        initHydra(); let sources = []; Hydra._image = (...args) => sources.push(args);
        s0.initCam(Infinity); s1.initCam(1n);
        reads === 3 && tagged.renamed === 5 && tagged.bb === 6 && tagged.color === 'red' && scalar.aa === 7 &&
        symbolWidth === c.measureText('Symbol(x)').width &&
        JSON.stringify(sources) === '[[0,"camera:Infinity"],[1,"camera:1"]]'
    "#,
    );
}

#[test]
fn hydra_sequences_use_array_iterators_and_keep_function_captures() {
    let result = crate::eval_result(
        r#"
        await initHydra();
        const xs = new Proxy([1, 2], {});
        xs[Symbol.iterator] = function* () { yield 30; yield 40; };
        osc(xs, ({time}) => time + 5).out();
        silence
    "#,
    )
    .unwrap();
    assert_eq!(result.meta.hydra_params.len(), 2);
    assert_eq!(result.meta.hydra_params[0].value(0.0, 0.0), Some(30.0));
    assert_eq!(result.meta.hydra_params[0].value(2.0, 0.0), Some(40.0));
    js::on_js_thread(boa_gc::force_collect);
    assert_eq!(result.meta.hydra_params[1].value(2.0, 0.0), Some(7.0));
}
