//! Whole-app UI tests driven through [`egui_kittest`].
//!
//! The rest of the app suite pokes at state structs directly; nothing there
//! ever paints a frame, so panics, egui id clashes and dead wiring in the panel
//! and inline-widget code only showed up by running the real binary. These
//! tests build the real [`RudelApp`] (minus the audio device) on a headless
//! egui harness, click the real buttons and press the real shortcuts.

use super::RudelApp;
use eframe::egui::{Key, Modifiers};
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
