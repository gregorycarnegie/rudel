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
