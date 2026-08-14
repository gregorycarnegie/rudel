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
            app.sample_jobs.push(crate::app::SampleJob {
                key: "k".to_string(),
                label: "l".to_string(),
                handle: std::thread::spawn(|| Ok(0)),
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
