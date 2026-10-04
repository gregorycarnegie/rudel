//! Whole-app UI tests driven through [`egui_kittest`].
//!
//! The rest of the app suite pokes at state structs directly; nothing there
//! ever paints a frame, so panics, egui id clashes and dead wiring in the panel
//! and inline-widget code only showed up by running the real binary. These
//! tests build the real [`RudelApp`] (minus the audio device) on a headless
//! egui harness, click the real buttons and press the real shortcuts.

use super::RudelApp;
use eframe::egui::{self, Key, Modifiers};
use egui_kittest::{Harness, kittest::Queryable};

fn harness<'a>() -> Harness<'a, RudelApp> {
    let mut harness = Harness::builder()
        .with_size(eframe::egui::vec2(1100.0, 640.0))
        .build_eframe(|cc| {
            crate::theme::apply(&cc.egui_ctx);
            RudelApp::headless()
        });
    harness.run();
    harness
}

#[test]
fn every_panel_puts_its_own_furniture_on_screen() {
    // One assertion per panel that it drew at all. Cheap, but it is the only
    // thing standing between a panel quietly becoming a no-op and someone
    // noticing at run time — none of the state-level tests would blink.
    let mut harness = harness();

    // Transport, reference list, editor (and the settings header nested inside
    // it) are always on screen.
    harness.get_by_label_contains("Play");
    harness.get_by_label_contains("Hush");
    harness.get_by_label_contains("reference");
    harness.get_by_label_contains("editor settings");

    // The errors panel carries the shortcut hint while nothing is wrong...
    harness.get_by_label_contains("Ctrl+Enter eval");

    // ...and the error itself once something is.
    harness.state_mut().code = "s(\"bd\"".to_string();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    harness.run_steps(2);
    let error = harness
        .state()
        .eval_error
        .clone()
        .expect("an unbalanced paren should not evaluate");
    harness.get_by_label_contains(error.split(':').next().unwrap_or(&error));

    // The console appears only once a pattern has logged something.
    harness
        .state_mut()
        .log_lines
        .push("hello-from-log".to_string());
    harness.run_steps(2);
    harness.get_by_label_contains("console");
    harness.get_by_label_contains("hello-from-log");
}

#[test]
fn the_reference_filter_opens_the_sections_holding_its_matches() {
    // `signals` and `factories` start collapsed, so a match inside one of them
    // is only reachable because filtering forces every section open. Without
    // that, typing a filter appears to find nothing.
    let mut harness = harness();
    assert!(
        harness.query_by_label_contains("perlin").is_none(),
        "signals start collapsed"
    );

    harness.state_mut().reference_filter = "perlin".to_string();
    harness.run_steps(2);
    harness.get_by_label_contains("perlin");

    // And a filter that matches no sound drops the sounds section entirely
    // rather than leaving an empty header behind.
    harness.state_mut().reference_filter = "zzzznotasound".to_string();
    harness.run_steps(2);
    assert!(
        harness.query_by_label_contains("sounds").is_none(),
        "an empty sounds section should not be drawn"
    );

    // Clearing the filter brings the sounds section back. (The signals section
    // stays open: egui remembers the state it was forced into, which is what a
    // user who just went looking would want.)
    harness.state_mut().reference_filter = String::new();
    harness.run_steps(2);
    harness.get_by_label_contains("sounds");
}

#[test]
fn only_ctrl_s_itself_saves() {
    // Save with no file yet asks where (Save As), so the save dialog opening
    // is what "saved" looks like. Another Ctrl chord, or a bare `s`, must not.
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let mut harness = harness();
    let asked = Arc::new(AtomicUsize::new(0));
    let counter = asked.clone();
    harness.state_mut().dialogs = super::files::Dialogs {
        pick: Box::new(|_| None),
        save: Box::new(move |_| {
            counter.fetch_add(1, Ordering::Relaxed);
            None
        }),
        confirm: Box::new(|_| rfd::MessageDialogResult::Yes),
    };
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    harness.key_press(Key::S);
    harness.run_steps(2);
    assert_eq!(asked.load(Ordering::Relaxed), 0, "nothing asked to save");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::S);
    harness.run_steps(2);
    assert_eq!(asked.load(Ordering::Relaxed), 1, "Ctrl+S saves");
}

#[test]
fn a_reference_insertion_leaves_the_cursor_after_what_it_inserted() {
    // A double-clicked reference name lands at the cursor, and the cursor
    // moves past it, so a second double-click continues rather than splitting
    // the first. The editor has no cursor yet, so the first goes at the end.
    let mut harness = harness();
    harness.state_mut().code = "s(\"bd\")".to_string();
    harness.state_mut().pending_insert = Some(".fast".to_string());
    harness.run_steps(2);
    harness.state_mut().pending_insert = Some("(2)".to_string());
    harness.run_steps(2);
    assert_eq!(harness.state().code, "s(\"bd\").fast(2)");
}

#[test]
fn the_console_keeps_only_the_most_recent_lines() {
    // The panel drains rudel-core's log ring into its own buffer every frame
    // and trims the front, so a long-running pattern cannot grow it forever.
    let mut harness = harness();
    harness.state_mut().log_lines = (0..600).map(|i| format!("line-{i}")).collect();
    harness.run_steps(2);

    let lines = &harness.state().log_lines;
    assert_eq!(lines.len(), 512, "trimmed to the window");
    assert_eq!(lines.first().map(String::as_str), Some("line-88"));
    assert_eq!(
        lines.last().map(String::as_str),
        Some("line-599"),
        "the newest lines are the ones kept"
    );
}

#[test]
fn play_button_evaluates_the_default_pattern_and_starts_playback() {
    let mut harness = harness();

    harness.get_by_label_contains("Play").click();
    harness.run_steps(2);

    assert!(harness.state().playing, "play button should start playback");
    assert_eq!(harness.state().eval_error, None);
    assert!(
        harness.state().current.is_some(),
        "pressing play with nothing evaluated should evaluate first"
    );
}

#[test]
fn transport_shortcuts_evaluate_and_hush() {
    let mut harness = harness();

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    harness.run_steps(2);
    assert_eq!(harness.state().status, "evaluated");
    assert_eq!(harness.state().eval_error, None);

    harness.get_by_label_contains("Play").click();
    harness.run_steps(2);
    assert!(harness.state().playing);

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Period);
    harness.run_steps(2);
    assert!(!harness.state().playing, "Ctrl+. should hush");
    assert_eq!(harness.state().status, "hushed");
}

#[test]
fn right_clicking_the_editor_opens_a_menu_that_edits_and_evaluates() {
    // The menu is the only place these actions are discoverable without knowing
    // the shortcut, and it lives outside the editor's `has_focus` gate — so it
    // is exactly the wiring that a state-level test would not catch.
    let mut harness = harness();
    harness.state_mut().code = "s(\"bd\")".to_string();
    harness.run_steps(2);

    // The editor has no label; its accesskit value is the code it holds.
    harness
        .get_all_by_value("s(\"bd\")")
        .next()
        .expect("the code editor")
        .click_secondary();
    harness.run_steps(2);
    harness.get_by_label_contains("Toggle comment");

    harness.get_by_label_contains("Toggle comment").click();
    harness.run_steps(2);
    assert_eq!(
        harness.state().code,
        "// s(\"bd\")",
        "the menu entry runs the same edit as Ctrl+/"
    );

    // And an app-level entry reaches the engine, not just the text buffer.
    harness
        .get_all_by_value("// s(\"bd\")")
        .next()
        .expect("the code editor")
        .click_secondary();
    harness.run_steps(2);
    harness.get_by_label("Evaluate Ctrl+Enter").click();
    harness.run_steps(2);
    assert_eq!(harness.state().status, "evaluated");
}

#[test]
fn inline_widgets_paint_while_playing() {
    // The inline widget surfaces (pianoroll/spiral/pitchwheel/scope) are the
    // one part of the app that only draws while a pattern is running, so this
    // is the only test that exercises their paint path at all.
    let mut harness = harness();
    harness.state_mut().code = r#"stack(
  note("c3 e3 g3 b3")._pianoroll(),
  note("c4 e4")._spiral(),
  note("d4 a4")._pitchwheel()
)"#
    .to_string();

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    harness.run_steps(2);
    assert_eq!(harness.state().eval_error, None);
    assert_eq!(
        harness.state().editor_decorations.widgets().len(),
        3,
        "three inline widgets should be registered from the evaluated source"
    );

    harness.get_by_label_contains("Play").click();
    // Playing requests a repaint every frame, so step a fixed count rather than
    // running to quiescence.
    harness.run_steps(8);
    assert!(harness.state().playing);
}

#[test]
fn transport_buttons_stay_clickable_under_a_scrolled_widget_surface() {
    // The widget surfaces are foreground areas anchored to their code line. When
    // the line scrolls up behind the transport bar the surface used to keep its
    // full (unclipped) interact rect there and swallow clicks meant for the
    // buttons.
    let mut harness = harness();
    harness.state_mut().code = format!(
        "note(\"c3 e3 g3 b3\")._pianoroll({{ height: 300 }})\n{}",
        "\n".repeat(80)
    );

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    harness.run_steps(2);
    assert_eq!(harness.state().editor_decorations.widgets().len(), 1);

    // Scroll the editor until the widget sits behind the transport bar.
    let over_editor = egui::pos2(550.0, 400.0);
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(over_editor));
    harness.input_mut().events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -150.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    harness.run_steps(4);

    harness.get_by_label_contains("Play").click();
    harness.run_steps(2);
    assert!(
        harness.state().playing,
        "play button should still take clicks with a widget surface scrolled behind it"
    );
}

fn double_click(harness: &mut Harness<'_, RudelApp>, pos: egui::Pos2) {
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(pos));
    // In one frame: a harness step is longer than egui's double-click window.
    for pressed in [true, false, true, false] {
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    harness.run_steps(2);
}

#[test]
fn double_clicking_a_widget_pops_it_out_until_the_widget_goes() {
    let mut harness = harness();
    harness.state_mut().code = "note(\"c3 e3 g3 b3\")._pianoroll()".to_string();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    harness.run_steps(2);
    let widget = harness.state().editor_decorations.widgets()[0].clone();
    // The first surface created gets serial 0.
    let area = egui::Id::new((
        "rudel-inline-widget",
        widget.widget_type.as_str(),
        widget.id.as_str(),
        0u64,
    ));
    let rect = harness
        .ctx
        .memory(|m| m.area_rect(area))
        .expect("the widget surface is on screen");

    // The harness cannot open OS windows, so egui embeds the viewport as a
    // window titled like the real one.
    assert!(
        harness
            .query_by_label_contains("rudel: pianoroll")
            .is_none()
    );
    double_click(&mut harness, rect.center());
    harness.get_by_label_contains("rudel: pianoroll");

    // Evaluating code without the widget takes its window away with it.
    harness.state_mut().code = "note(\"c3\")".to_string();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    harness.run_steps(2);
    assert!(
        harness
            .query_by_label_contains("rudel: pianoroll")
            .is_none()
    );
}

/// Does one more frame ask for another, after `setup` changes the app state?
///
/// `step` rather than `run`: `run` keeps painting until nothing wants a
/// repaint, which is exactly what is being asserted here.
fn repaints_after(setup: impl FnOnce(&mut RudelApp)) -> bool {
    let mut harness = harness();
    setup(harness.state_mut());
    harness.step();
    harness.ctx.has_requested_repaint()
}

#[test]
fn the_frame_loop_keeps_going_for_each_thing_that_moves() {
    // The playhead, the sample queue, an incoming clock and a live MIDI input
    // each keep the UI repainting on their own. Miss one and the display
    // freezes until the user happens to move the mouse — which is how a
    // "stuck" playhead gets reported.
    assert!(repaints_after(|app| app.playing = true), "playing");
    assert!(
        repaints_after(|app| app.clock_sync = true),
        "following a MIDI clock"
    );
    assert!(
        repaints_after(|app| {
            // The job has to still be running when the frame is stepped: a
            // finished one is reaped before the repaint check, so a thread that
            // returns immediately makes this pass or fail on scheduling. Block
            // it on a channel whose sender is never dropped.
            let (tx, rx) = std::sync::mpsc::channel::<()>();
            std::mem::forget(tx);
            app.sample_jobs.push(crate::app::SampleJob {
                key: "k".to_string(),
                label: "l".to_string(),
                handle: std::thread::spawn(move || {
                    let _ = rx.recv();
                    Ok(0)
                }),
                quiet: true,
            })
        }),
        "a sample still loading"
    );
    // ...and with none of them, the app is allowed to go idle.
    assert!(!repaints_after(|_| {}), "nothing is moving");
}

#[test]
fn a_pending_midi_open_keeps_the_frame_loop_going() {
    // The three polls are joined with a non-short-circuiting `|` so all three
    // run every frame; any one of them still in flight has to hold the loop
    // open on its own.
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let held = repaints_after(move |app| {
        app.script_midi_in_pending.push((
            "slow".to_string(),
            std::thread::spawn(move || {
                let _ = rx.recv();
                Err("cancelled".to_string())
            }),
        ));
    });
    assert!(held, "an open still in flight");
    drop(tx);
}

#[test]
fn play_evaluates_only_when_there_is_nothing_to_play() {
    // Pressing Play with nothing evaluated evaluates first, so the button
    // does what it says on a cold start...
    // `step`, not `run`: once playing, every frame asks for another.
    let mut cold = harness();
    cold.get_by_label_contains("Play").click();
    cold.run_steps(2);
    assert!(
        cold.state().current.is_some(),
        "Play on a cold start evaluates the buffer"
    );

    // ...but pressing it to *stop* must not re-evaluate. Staged directly,
    // since reaching this state through the button would evaluate on the way.
    let mut stopping = harness();
    stopping.state_mut().playing = true;
    stopping.step();
    stopping.get_by_label_contains("Stop").click();
    stopping.run_steps(2);
    assert!(
        stopping.state().current.is_none(),
        "stopping is not an evaluation"
    );
}

#[test]
fn disconnect_is_offered_only_once_something_is_connected() {
    // The button is behind a `&&` that short-circuits, so a wrong operator
    // here does not just mis-enable it — it draws a control for a device that
    // is not there.
    // The i/o section is collapsed by default, so open it — otherwise this
    // asserts nothing at all.
    let mut harness = harness();
    harness.get_by_label_contains("i/o").click();
    harness.run_steps(2);
    // Proves the section really is open — `Connect` only exists inside it.
    harness.get_by_label_contains("Connect");
    assert!(
        harness.query_by_label_contains("Disconnect").is_none(),
        "nothing is connected yet"
    );
}

#[test]
fn connect_is_clickable_until_a_connection_is_in_flight() {
    let mut harness = harness();
    harness.get_by_label_contains("i/o").click();
    harness.run_steps(2);
    harness.get_by_label_contains("Connect").click();
    harness.run_steps(2);
    // Either it is still in flight or it already failed (no MIDI ports on a
    // test machine) — both prove the click reached `connect_input`.
    let state = harness.state();
    assert!(
        state.midi_in_pending.is_some() || state.io_error.is_some(),
        "the Connect button has to be enabled when nothing is connecting"
    );
}

#[test]
fn the_secondary_eval_button_evaluates_too() {
    // Ctrl+Shift+Enter's button. With block-based eval off it evaluates the
    // block under the cursor, which on the default buffer is the whole thing.
    let mut harness = harness();
    assert!(harness.state().current.is_none(), "nothing evaluated yet");
    harness.get_by_label_contains("Block").click();
    harness.run_steps(2);
    assert!(
        harness.state().current.is_some(),
        "the secondary button has to reach an evaluation"
    );
}

#[test]
fn any_one_connection_in_flight_holds_the_frame_loop_open() {
    // The three polls are joined with `|` — non-short-circuiting, so all three
    // run every frame, and *any* one still working keeps the loop alive. Each
    // combination below is a different way to get that wrong: `&` needs two,
    // `^` cancels on the second.
    // Blocked until the sender is dropped, so the poll sees it in flight.
    // Never resolves, so the `Ok` type is free to be whatever the field wants.
    fn blocked<T: Send + 'static>(
        keep: &mut Vec<std::sync::mpsc::Sender<()>>,
    ) -> std::thread::JoinHandle<Result<T, String>> {
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        keep.push(tx);
        std::thread::spawn(move || {
            let _ = rx.recv();
            Err("cancelled".to_string())
        })
    }

    for (out, input, script) in [
        (true, false, false),
        (false, true, false),
        (false, false, true),
        (true, true, false),
        (false, true, true),
    ] {
        let mut keep = Vec::new();
        let held = repaints_after(|app| {
            if out {
                app.midi_pending = Some(blocked(&mut keep));
            }
            if input {
                app.midi_in_pending = Some(blocked(&mut keep));
            }
            if script {
                app.script_midi_in_pending
                    .push(("slow".to_string(), blocked(&mut keep)));
            }
        });
        assert!(held, "out {out}, in {input}, script {script}");
        drop(keep);
    }
}

#[test]
fn without_a_filter_the_collapsed_sections_stay_shut() {
    // Tall enough that nothing in the reference list is scrolled out of view,
    // so a missing entry is collapsed, not just off screen.
    let mut harness = Harness::builder()
        .with_size(eframe::egui::vec2(1100.0, 30_000.0))
        .build_eframe(|cc| {
            crate::theme::apply(&cc.egui_ctx);
            RudelApp::headless()
        });
    harness.run();
    harness.get_by_label_contains("sounds");
    assert!(
        harness.query_by_label_contains("perlin").is_none(),
        "signals should start collapsed when nothing is being filtered"
    );
}

#[test]
fn the_menu_indents_and_outdents_the_line() {
    let mut harness = harness();
    harness.state_mut().code = "s(\"bd\")".to_string();
    harness.run_steps(2);
    let open_menu = |harness: &mut Harness<'_, RudelApp>| {
        let code = harness.state().code.clone();
        harness
            .get_all_by_value(&code)
            .next()
            .expect("the code editor")
            .click_secondary();
        harness.run_steps(2);
    };
    open_menu(&mut harness);
    harness.get_by_label_contains("Indent").click();
    harness.run_steps(2);
    let indented = harness.state().code.clone();
    assert!(
        indented.starts_with(' ') && indented.trim_start() == "s(\"bd\")",
        "{indented:?}"
    );
    open_menu(&mut harness);
    harness.get_by_label_contains("Outdent").click();
    harness.run_steps(2);
    assert_eq!(harness.state().code, "s(\"bd\")");
}

#[test]
fn the_status_bar_says_when_there_is_no_audio_device() {
    let mut harness = harness();
    harness.state_mut().audio_error = Some("no device".to_string());
    harness.state_mut().status = "status-text".to_string();
    harness.run_steps(2);
    harness.get_by_label("no audio");
    harness.get_by_label("status-text");
}

// --- the GPU widgets, rendered for real ------------------------------------
//
// kittest's wgpu renderer hands the app a render state, so the shader, hydra
// and spiral widgets take their GPU paths, and `render()` reads the frame
// back. A fake audio engine that nothing pulls holds the playhead at 0, so
// two renders of the same pattern see the same moment.

/// The app on a GPU renderer, playing `code` at a frozen playhead.
fn gpu_app<'a>(code: &str) -> Harness<'a, RudelApp> {
    gpu_app_at(code, 1.0)
}

/// [`gpu_app`] on a display of `pixels_per_point`.
fn gpu_app_at<'a>(code: &str, pixels_per_point: f32) -> Harness<'a, RudelApp> {
    let mut harness = Harness::builder()
        .with_size(eframe::egui::vec2(1100.0, 640.0))
        .with_pixels_per_point(pixels_per_point)
        .wgpu()
        .build_eframe(|cc| {
            crate::theme::apply(&cc.egui_ctx);
            super::install_gpu_stores(cc);
            let mut app = RudelApp::headless();
            let (engine, output) = rudel_audio::Engine::with_fake_output(48_000.0);
            app.engine = Some(engine);
            // Kept alive with the app, never pulled.
            std::mem::forget(output);
            app
        });
    harness.state_mut().code = code.to_string();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    harness.run_steps(2);
    assert_eq!(harness.state().eval_error, None, "{code}");
    harness.get_by_label_contains("Play").click();
    harness.run_steps(4);
    harness
}

/// Pixels of `image` close to a pure colour (each channel within 40).
fn count(rgba: &[u8], rgb: [u8; 3]) -> usize {
    rgba.chunks(4)
        .filter(|p| (0..3).all(|c| (i32::from(p[c]) - i32::from(rgb[c])).abs() < 40))
        .count()
}

const RED: [u8; 3] = [255, 0, 0];
const GREEN: [u8; 3] = [0, 255, 0];
const BLUE: [u8; 3] = [0, 0, 255];
/// The widget surface is 200x200.
const SURFACE: usize = 200 * 200;

#[test]
fn a_shader_widget_paints_its_body_and_recompiles_when_it_changes() {
    let mut harness =
        gpu_app("s(\"bd\").shader({ code: 'return vec4<f32>(1.0, 0.0, 0.0, 1.0);' })");
    let image = harness.render().expect("renders");
    assert!(
        count(image.as_raw(), RED) > SURFACE * 9 / 10,
        "{}",
        count(image.as_raw(), RED)
    );
    // A new body is a new program, not the cached one.
    harness.state_mut().code =
        "s(\"bd\").shader({ code: 'return vec4<f32>(0.0, 1.0, 0.0, 1.0);' })".to_string();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    harness.run_steps(4);
    let image = harness.render().expect("renders");
    assert!(
        count(image.as_raw(), GREEN) > SURFACE * 9 / 10,
        "{}",
        count(image.as_raw(), GREEN)
    );
    assert!(count(image.as_raw(), RED) < 100);

    // A body that does not compile says why, written over the widget's area.
    // The same length as the last, so the widget keeps its id and its cached
    // check has to notice the change.
    let green = |p: &[u8]| (0..3).all(|c| (i32::from(p[c]) - i32::from(GREEN[c])).abs() < 40);
    let (mut lo, mut hi) = ((u32::MAX, u32::MAX), (0, 0));
    for (x, y, p) in image.enumerate_pixels() {
        if green(&p.0) {
            (lo, hi) = ((lo.0.min(x), lo.1.min(y)), (hi.0.max(x), hi.1.max(y)));
        }
    }
    harness.state_mut().code =
        "s(\"bd\").shader({ code: 'return nothing_by_this_name_at_all__;' })".to_string();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    harness.run_steps(4);
    let image = harness.render().expect("renders");
    let area: Vec<[u8; 4]> = (lo.1..=hi.1)
        .flat_map(|y| (lo.0..=hi.0).map(move |x| (x, y)))
        .map(|(x, y)| image.get_pixel(x, y).0)
        .collect();
    let mut tally = std::collections::HashMap::new();
    for p in &area {
        *tally.entry(*p).or_insert(0) += 1;
    }
    let background = tally.into_iter().max_by_key(|&(_, n)| n).map(|(p, _)| p);
    let text = area.iter().filter(|&&p| Some(p) != background).count();
    assert!(text > 500, "{text} pixels of error text");
}

#[test]
fn a_shader_sees_its_surface_size_in_physical_pixels() {
    // Green only if `u.res` is the 200x200-point surface at 2 pixels a point,
    // and `u.note` is negative for a sound with no pitch.
    let harness_code = "s(\"bd\").shader({ code: 'let ok = abs(u.res.x - 400.0) < 0.5 && abs(u.res.y - 400.0) < 0.5 && u.note < 0.0; return select(vec4<f32>(1.0, 0.0, 0.0, 1.0), vec4<f32>(0.0, 1.0, 0.0, 1.0), ok);' })";
    let mut harness = gpu_app_at(harness_code, 2.0);
    let image = harness.render().expect("renders");
    assert!(
        count(image.as_raw(), GREEN) > 4 * SURFACE * 9 / 10,
        "green {} red {}",
        count(image.as_raw(), GREEN),
        count(image.as_raw(), RED)
    );
}

#[test]
fn a_hydra_argument_set_per_frame_reaches_the_shader() {
    // `H(pattern)` fills a uniform slot each frame; the chain reads it.
    let mut harness = gpu_app("s(\"bd\").hydra({ chain: Hydra.solid(H(\"1\"), 0, 0) })");
    let image = harness.render().expect("renders");
    assert!(
        count(image.as_raw(), RED) > SURFACE * 9 / 10,
        "{}",
        count(image.as_raw(), RED)
    );
    // A function that returns no number leaves the input's default, 0.
    let mut harness = gpu_app("s(\"bd\").hydra({ chain: Hydra.solid(() => 'x', 0, 0) })");
    let image = harness.render().expect("renders");
    assert!(
        count(image.as_raw(), RED) < 100,
        "{}",
        count(image.as_raw(), RED)
    );
}

#[test]
fn a_picture_loaded_into_a_source_shows_through_src() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let path = dir.path().join("red.png");
    image::RgbaImage::from_pixel(8, 8, image::Rgba([255, 0, 0, 255]))
        .save(&path)
        .expect("writes the png");
    let url = path.to_string_lossy().replace('\\', "/");
    let mut harness = gpu_app(&format!(
        "Hydra.s0.initImage('{url}')\ns(\"bd\").hydra({{ chain: Hydra.src(Hydra.s0) }})"
    ));
    // The picture decodes on a thread; it binds on the frame after it lands.
    let mut red = 0;
    for _ in 0..50 {
        harness.run_steps(1);
        red = count(harness.render().expect("renders").as_raw(), RED);
        if red > SURFACE * 9 / 10 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(red > SURFACE * 9 / 10, "{red}");
}

#[test]
fn feed_strudel_draws_the_canvas_visuals_into_s0() {
    let bright = |code: &str| {
        let mut harness = gpu_app(code);
        harness.run_steps(2);
        let image = harness.render().expect("renders");
        // The blocks, through the code's wash: grey, lighter than the theme.
        image
            .as_raw()
            .chunks(4)
            .filter(|p| (48..120).contains(&p[0]) && p[0].abs_diff(p[2]) < 12)
            .count()
    };
    // The punchcard fills the scene through `src(s0)`; without the feed it is
    // only its small inline surface.
    let fed = bright(
        "await initHydra({feedStrudel: 1})
src(s0).out()
note(\"c e g b\").punchcard()",
    );
    let unfed = bright(
        "await initHydra()
src(s0).out()
note(\"c e g b\").punchcard()",
    );
    assert!(fed > unfed * 3, "fed {fed}, unfed {unfed}");
}

#[test]
fn a_hydra_scene_draws_behind_the_code() {
    let mut harness = gpu_app(
        "await initHydra()
solid(0, [1, 1].fast(2), 0).out()",
    );
    assert!(harness.state().editor_decorations.backdrop().is_some());
    let image = harness.render().expect("renders");
    // Most of the editor, washed: still clearly green rather than the theme's.
    let greenish = image
        .as_raw()
        .chunks(4)
        .filter(|p| p[1] > 100 && p[0] < 80 && p[2] < 80)
        .count();
    assert!(greenish > SURFACE * 4, "{greenish}");
}

#[test]
fn a_hydra_widget_renders_its_chain_and_reads_other_outputs() {
    let mut harness = gpu_app("s(\"bd\").hydra({ chain: Hydra.solid(0, 1, 0) })");
    let image = harness.render().expect("renders");
    assert!(
        count(image.as_raw(), GREEN) > SURFACE * 9 / 10,
        "{}",
        count(image.as_raw(), GREEN)
    );
    // A new chain at the same size is a new surface, not the cached one.
    harness.state_mut().code = "s(\"bd\").hydra({ chain: Hydra.solid(1, 0, 0) })".to_string();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    harness.run_steps(4);
    let image = harness.render().expect("renders");
    assert!(
        count(image.as_raw(), RED) > SURFACE * 9 / 10,
        "{}",
        count(image.as_raw(), RED)
    );
    // `src(o1)` reads the buffer `o1` drew into last frame, so it shows from
    // the second GPU frame on (kittest paints only when asked to render).
    let mut harness =
        gpu_app("s(\"bd\").hydra({ chain: Hydra.src(Hydra.o1), o1: Hydra.solid(0, 0, 1) })");
    let first = harness.render().expect("renders");
    assert!(
        count(first.as_raw(), BLUE) < 100,
        "nothing to read on the first frame"
    );
    harness.run_steps(1);
    let image = harness.render().expect("renders");
    assert!(
        count(image.as_raw(), BLUE) > SURFACE * 9 / 10,
        "{}",
        count(image.as_raw(), BLUE)
    );
}

#[test]
fn a_hydra_widget_draws_at_its_size_in_physical_pixels() {
    // `modulateHue` against solid green shifts by 80 pixels on each axis, a
    // fifth of the 400-pixel surface, which leaves the centred half-size
    // square in view. Sized wrong, the shift grows and the square slides off.
    const WHITE: [u8; 3] = [255, 255, 255];
    let white = |amount: u32| {
        let code = format!(
            "s(\"bd\").hydra({{ chain: Hydra.shape(4, 0.5).modulateHue(Hydra.solid(0, 1, 0), {amount}) }})"
        );
        count(
            gpu_app_at(&code, 2.0).render().expect("renders").as_raw(),
            WHITE,
        )
    };
    let (still, shifted) = (white(0), white(80));
    assert!(shifted > still * 95 / 100, "{shifted} of {still}");
}

#[test]
fn a_hydra_buffer_read_back_scaled_is_filtered_smoothly() {
    // A hard-edged red circle in `o1`, read back zoomed in and then out: a
    // linear sampler leaves partly-red pixels along the edge, a nearest one
    // only full red and black.
    let partly_red = |rgba: &[u8]| {
        rgba.chunks(4)
            .filter(|p| (50..205).contains(&p[0]) && p[1] < 20 && p[2] < 20)
            .count()
    };
    for scale in ["1.5", "0.5"] {
        let mut harness = gpu_app(&format!(
            "s(\"bd\").hydra({{ chain: Hydra.src(Hydra.o1).scale({scale}), o1: Hydra.shape(60, 0.5, 0).color(1, 0, 0) }})"
        ));
        harness.render().expect("renders");
        harness.run_steps(1);
        let n = partly_red(harness.render().expect("renders").as_raw());
        assert!(n > 40, "scale {scale}: {n}");
    }
}

#[test]
fn a_hydra_widget_shows_the_output_it_is_told_to_or_all_four() {
    let mut harness = gpu_app(
        "s(\"bd\").hydra({ chain: Hydra.solid(0, 1, 0), o2: Hydra.solid(1, 0, 0), render: 2 })",
    );
    let image = harness.render().expect("renders");
    assert!(
        count(image.as_raw(), RED) > SURFACE * 9 / 10,
        "{}",
        count(image.as_raw(), RED)
    );
    let mut harness = gpu_app(
        "s(\"bd\").hydra({ chain: Hydra.solid(0, 1, 0), o1: Hydra.solid(0, 0, 1), o2: Hydra.solid(1, 0, 0), render: 'all' })",
    );
    let image = harness.render().expect("renders");
    // A quarter each; o3 has no chain and stays dark.
    for (name, rgb) in [("o0", GREEN), ("o1", BLUE), ("o2", RED)] {
        let n = count(image.as_raw(), rgb);
        assert!(n > SURFACE / 5 && n < SURFACE / 3, "{name}: {n}");
    }
}

#[test]
fn the_gpu_spiral_draws_what_the_cpu_spiral_draws() {
    // The tessellated painter is the oracle: same pattern, same moment, and
    // the two frames should differ only at the edges of the strokes.
    // At 2 pixels a point, so lengths the shader takes in physical pixels
    // have to have been scaled to them.
    let render = |code: &str| gpu_app_at(code, 2.0).render().expect("renders").into_raw();
    let differing = |a: &[u8], b: &[u8]| {
        a.chunks(4)
            .zip(b.chunks(4))
            .filter(|(a, b)| (0..3).any(|c| (i32::from(a[c]) - i32::from(b[c])).abs() > 60))
            .count()
    };
    let none = render("note(\"c4 e4 g4 b4\")");
    let gpu = render("note(\"c4 e4 g4 b4\")._spiral({ gpu: true })");
    let cpu = render("note(\"c4 e4 g4 b4\")._spiral({ gpu: false })");
    let drawn = differing(&cpu, &none);
    let off = differing(&gpu, &cpu);
    assert!(off * 10 < drawn, "{off} pixels differ, of {drawn} drawn");
}

/// The completion popup's rows: each reads `"<name>  <kind>"`, which nothing
/// else on screen does. Returns `(label, selected)` per row.
fn completion_rows(harness: &Harness<'_, RudelApp>) -> Vec<(String, bool)> {
    use egui_kittest::kittest::NodeT;
    harness
        .query_all_by_label_contains("  ")
        .filter_map(|n| {
            let node = n.accesskit_node();
            let label = node.label()?;
            let kind = label.rsplit("  ").next()?;
            matches!(
                kind,
                "function" | "method" | "control" | "keyword" | "sound"
            )
            .then(|| {
                (
                    label.to_string(),
                    node.toggled() == Some(egui::accesskit::Toggled::True),
                )
            })
        })
        .collect()
}

/// Focus the editor holding `code` and put the cursor at its end.
fn focus_editor(harness: &mut Harness<'_, RudelApp>, code: &str) {
    harness.state_mut().code = code.to_string();
    harness.run_steps(2);
    harness
        .get_all_by_value(code)
        .next()
        .expect("the code editor")
        .click();
    harness.run_steps(1);
    harness.key_press(Key::End);
    harness.run_steps(1);
}

fn type_chars(harness: &mut Harness<'_, RudelApp>, text: &str) {
    for ch in text.chars() {
        harness
            .input_mut()
            .events
            .push(egui::Event::Text(ch.to_string()));
        harness.run_steps(1);
    }
    harness.run_steps(2);
}

#[test]
fn typing_opens_completions_for_the_word_with_the_first_selected() {
    let mut harness = harness();
    focus_editor(&mut harness, "s(\"bd\")");
    // The cursor sits on a word, but nothing was typed: no popup.
    harness.key_press(Key::Home);
    harness.key_press(Key::ArrowRight);
    harness.run_steps(2);
    assert!(
        completion_rows(&harness).is_empty(),
        "{:?}",
        completion_rows(&harness)
    );
    harness.key_press(Key::End);
    type_chars(&mut harness, ".fas");
    let rows = completion_rows(&harness);
    assert!(
        rows.iter().any(|(label, _)| label.starts_with("fastGap")),
        "{rows:?}"
    );
    assert!(
        rows.iter().all(|(label, _)| label.starts_with("fas")),
        "{rows:?}"
    );
    // The first row is the selected one, and only it.
    assert_eq!(
        rows.iter().filter(|(_, selected)| *selected).count(),
        1,
        "{rows:?}"
    );
    assert!(rows[0].1, "{rows:?}");
}

#[test]
fn holding_ctrl_on_a_name_shows_what_it_is_beside_the_editor() {
    let mut harness = harness();
    focus_editor(&mut harness, "arpWith(x => x)");
    harness.key_press(Key::Home);
    harness.key_press(Key::ArrowRight);
    harness.run_steps(2);
    let tip = |harness: &Harness<'_, RudelApp>| harness.query_by_label("arpWith").map(|n| n.rect());
    assert!(tip(&harness).is_none(), "no tooltip without Ctrl");
    harness
        .input_mut()
        .events
        .push(egui::Event::ModifiersChanged(Modifiers::CTRL));
    harness.run_steps(2);
    let editor = harness
        .get_all_by_value("arpWith(x => x)")
        .next()
        .expect("the editor")
        .rect();
    let tip = tip(&harness).expect("a tooltip for the name under the cursor");
    assert!(
        tip.min.x >= editor.max.x,
        "the tooltip sits right of the editor: {tip:?} vs {editor:?}"
    );
}

#[test]
fn the_arrows_step_through_completions_and_tab_takes_the_selected_one() {
    let mut harness = harness();
    focus_editor(&mut harness, "s(\"bd\")");
    type_chars(&mut harness, ".fas");
    harness.key_press(Key::ArrowDown);
    harness.run_steps(2);
    let rows = completion_rows(&harness);
    assert!(rows.len() > 1 && rows[1].1 && !rows[0].1, "{rows:?}");
    let second = rows[1].0.split("  ").next().unwrap().to_string();
    harness.key_press(Key::Tab);
    harness.run_steps(2);
    let code = &harness.state().code;
    assert!(code.starts_with(&format!("s(\"bd\").{second}")), "{code}");
    assert!(completion_rows(&harness).is_empty(), "taken, so closed");
}

#[test]
fn moving_the_cursor_through_a_word_keeps_its_completions_open() {
    let mut harness = harness();
    focus_editor(&mut harness, "s(\"bd\")");
    type_chars(&mut harness, ".fas");
    harness.key_press(Key::ArrowLeft);
    harness.run_steps(2);
    assert!(
        !completion_rows(&harness).is_empty(),
        "refreshed, not closed"
    );
}

#[test]
fn with_autocomplete_off_typing_opens_nothing() {
    let mut harness = harness();
    harness.state_mut().editor_settings.autocomplete = false;
    focus_editor(&mut harness, "s(\"bd\")");
    type_chars(&mut harness, ".fas");
    assert!(
        completion_rows(&harness).is_empty(),
        "{:?}",
        completion_rows(&harness)
    );
}

#[test]
fn the_editor_grows_to_fill_its_panel() {
    let mut harness = harness();
    harness.state_mut().code = "s(\"bd\")".to_string();
    harness.run_steps(2);
    let editor = harness
        .get_all_by_value("s(\"bd\")")
        .next()
        .expect("the editor")
        .rect();
    // A 640-point window; the minimum is four rows.
    assert!(editor.height() > 300.0, "{editor:?}");
}

#[test]
fn the_menu_cuts_the_selection_and_pastes_over_it() {
    let mut harness = harness();
    focus_editor(&mut harness, "s(\"bd\")");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.run_steps(1);
    let menu = |harness: &mut Harness<'_, RudelApp>, code: &str, entry: &str| {
        harness
            .get_all_by_value(code)
            .next()
            .expect("the editor")
            .click_secondary();
        harness.run_steps(2);
        harness.get_by_label_contains(entry).click();
        harness.step();
    };
    menu(&mut harness, "s(\"bd\")", "Cut");
    harness.run_steps(2);
    assert_eq!(harness.state().code, "", "the whole selection cut");

    harness.state_mut().code = "s(\"bd\")".to_string();
    harness.run_steps(1);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.run_steps(1);
    menu(&mut harness, "s(\"bd\")", "Paste");
    let asked = harness
        .output()
        .viewport_output
        .values()
        .any(|v| v.commands.contains(&egui::ViewportCommand::RequestPaste));
    assert!(asked, "the menu asks the platform for the clipboard");
    // ...which eframe answers with a paste event.
    harness
        .input_mut()
        .events
        .push(egui::Event::Paste("n(\"0\")".to_string()));
    harness.run_steps(2);
    assert_eq!(
        harness.state().code,
        "n(\"0\")",
        "pasted over the selection"
    );
}

/// The editor holding `code`, focused with the cursor at its end, rendered.
fn rendered_editor(
    code: &str,
    settings: impl FnOnce(&mut crate::editor::settings::EditorSettings),
) -> Vec<u8> {
    let mut harness = Harness::builder()
        .with_size(eframe::egui::vec2(1100.0, 640.0))
        .wgpu()
        .build_eframe(|cc| {
            crate::theme::apply(&cc.egui_ctx);
            RudelApp::headless()
        });
    settings(&mut harness.state_mut().editor_settings);
    focus_editor(&mut harness, code);
    harness.run_steps(4);
    harness.render().expect("renders").into_raw()
}

#[test]
fn bracket_matching_and_the_active_line_follow_the_cursor() {
    let plain = rendered_editor("n(\"0\")", |_| {});
    let brackets = rendered_editor("n(\"0\")", |s| s.bracket_matching = true);
    let line = rendered_editor("n(\"0\")", |s| s.active_line = true);
    assert_ne!(plain, brackets, "the brackets around the cursor are marked");
    assert_ne!(plain, line, "the cursor's line is marked");
}

#[test]
fn dragging_the_cps_slider_sets_the_tempo() {
    let mut harness = harness();
    harness.get_by_role(egui::accesskit::Role::Slider).click();
    harness.run_steps(2);
    // Clicked at its middle: halfway along 0.1..=2.0.
    assert!(
        (harness.state().cps - 1.05).abs() < 0.1,
        "{}",
        harness.state().cps
    );
}

#[test]
fn choosing_an_output_reroutes_the_pattern_to_it() {
    let mut harness = harness();
    harness.get_by_label_contains("Play").click();
    harness.run_steps(2);
    assert!(harness.state().midi_pending.is_none());
    harness.get_by_role(egui::accesskit::Role::ComboBox).click();
    harness.run_steps(2);
    harness.get_by_label("MIDI").click();
    // On the frame it changes, not just eventually.
    harness.step();
    assert!(
        harness.state().midi_pending.is_some(),
        "playing on MIDI, so it connects"
    );
}

#[test]
fn clicking_an_inline_slider_writes_its_value_into_the_code() {
    let mut harness = harness();
    harness.state_mut().code = "note(\"c4\").gain(slider(0.2, 0, 1))".to_string();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    harness.run_steps(2);
    assert_eq!(harness.state().eval_error, None);
    harness.get_by_label_contains("Play").click();
    harness.run_steps(4);
    // The cps slider sits in the transport bar above; the inline one is lower.
    let inline = harness
        .query_all_by_role(egui::accesskit::Role::Slider)
        .max_by(|a, b| a.rect().min.y.total_cmp(&b.rect().min.y))
        .expect("the inline slider");
    inline.click();
    harness.run_steps(4);
    let code = &harness.state().code;
    assert!(!code.contains("slider(0.2,"), "{code}");
    assert!(code.contains("slider(0.5"), "{code}");
}

#[test]
fn a_gpu_spiral_that_gains_bands_gets_room_for_them() {
    // The same length of source, so the widget keeps its id and its buffers,
    // but eight times the notes.
    let mut harness = gpu_app("note(\"c4 e4 g4 b4\")._spiral({ gpu: true })");
    let sparse = harness.render().expect("renders").into_raw();
    harness.state_mut().code = "note(\"c4*16 e4*16\")._spiral({ gpu: true })".to_string();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    harness.run_steps(4);
    let dense = harness.render().expect("renders").into_raw();
    assert_ne!(sparse, dense);
}

/// Type `keys` the way a keyboard does: a key event, then the text it makes.
fn type_keys(harness: &mut Harness<'_, RudelApp>, keys: &str) {
    for c in keys.chars() {
        if let Some(key) = Key::from_name(&c.to_ascii_uppercase().to_string()) {
            harness.key_press(key);
        }
        harness.event(egui::Event::Text(c.to_string()));
        harness.run_steps(1);
    }
}

#[test]
fn vim_keys_edit_through_the_real_editor() {
    let mut harness = harness();
    harness.state_mut().editor_settings.keymap = crate::editor::keymap::Keymap::Vim;
    let code = "s(\"bd\")\nn(1)\nn(2)";
    harness.state_mut().code = code.to_string();
    harness.run_steps(2);
    harness
        .get_all_by_value(code)
        .next()
        .expect("the code editor")
        .click();
    harness.run_steps(2);

    // Normal mode: the keys are commands, not text.
    type_keys(&mut harness, "ggdd");
    assert_eq!(harness.state().code, "n(1)\nn(2)");
    type_keys(&mut harness, "u");
    assert_eq!(
        harness.state().code,
        code,
        "u undoes through the editor's own history"
    );

    // Insert mode hands typing to the editor; Escape comes back without
    // losing focus, as it otherwise would.
    type_keys(&mut harness, "ggI");
    type_keys(&mut harness, "x");
    harness.key_press(Key::Escape);
    harness.run_steps(1);
    assert_eq!(harness.state().code, "xs(\"bd\")\nn(1)\nn(2)");
    type_keys(&mut harness, "x");
    assert_eq!(harness.state().code, code, "back in normal mode, x deletes");

    // `:w` evaluates, as Ctrl+Enter does.
    type_keys(&mut harness, ":w");
    harness.key_press(Key::Enter);
    harness.run_steps(2);
    assert_eq!(harness.state().status, "evaluated");
    assert_eq!(harness.state().code, code, ":w is not typed into the code");
}

#[test]
fn emacs_keys_take_over_the_editor_s_own_ctrl_keys() {
    let mut harness = harness();
    harness.state_mut().editor_settings.keymap = crate::editor::keymap::Keymap::Emacs;
    let code = "s(\"bd\")\nn(1)";
    harness.state_mut().code = code.to_string();
    harness.run_steps(2);
    harness
        .get_all_by_value(code)
        .next()
        .expect("the code editor")
        .click();
    harness.run_steps(2);

    // M-< to the start, C-k kills the line (not egui's nothing), C-y yanks it
    // back (not egui's redo), and typing still types.
    harness.key_press_modifiers(Modifiers::ALT | Modifiers::SHIFT, Key::Comma);
    harness.key_press_modifiers(Modifiers::CTRL, Key::K);
    harness.run_steps(1);
    assert_eq!(harness.state().code, "\nn(1)");
    harness.key_press_modifiers(Modifiers::CTRL, Key::Y);
    harness.run_steps(1);
    assert_eq!(harness.state().code, code);
    // C-a goes to the line start rather than selecting everything.
    harness.key_press_modifiers(Modifiers::CTRL, Key::A);
    type_keys(&mut harness, "x");
    assert_eq!(harness.state().code, "xs(\"bd\")\nn(1)");
}
