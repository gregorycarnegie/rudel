//! Scroll-wheel adjustment for sliders.
//!
//! egui's `Slider` responds to drags and to keyboard focus, but not to the
//! wheel, so this is the one shared helper every slider in the app calls.
//! SPDX-License-Identifier: AGPL-3.0-or-later

use eframe::egui;

/// Scroll points in one wheel notch, for the devices that report points
/// (trackpads) rather than lines. Matches egui's native `line_scroll_speed`,
/// so a trackpad and a wheel move a slider at the same rate.
const POINTS_PER_NOTCH: f32 = 40.0;

/// Notches to cross a slider end to end. The increment is a fraction of the
/// range rather than the slider's own step, so every slider takes the same
/// amount of scrolling however wide it is — `cps` (0.1..2) used to need 190
/// notches and an inline `slider(300, 2000)` a thousand, while the volume
/// slider's 100 was the one that felt right.
const NOTCHES_PER_RANGE: f64 = 100.0;

/// Nudge `value` by a fraction of its range per wheel notch while `response`
/// is hovered, returning whether it changed.
///
/// This reads the raw `MouseWheel` events rather than `smooth_scroll_delta`.
/// The smoothed value is spread over several frames, and rudel only repaints
/// on input while the transport is stopped — so the ramp is cut off partway
/// and most of a notch is silently dropped (measured: about a sixth of the
/// scroll arriving). The raw events carry the whole thing, once.
///
/// `smooth_scroll_delta` is still zeroed while hovered, so a slider inside a
/// scroll area (the inline `slider(...)` controls live in the code editor)
/// adjusts instead of scrolling the view out from under the pointer.
///
/// `step` is the slider's own granularity, used only as a floor: an integral
/// slider like the editor font size must not land between its stops just
/// because its range is narrow.
pub(crate) fn scroll_adjust(
    ui: &egui::Ui,
    response: &egui::Response,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    step: f64,
) -> bool {
    if !response.hovered() {
        return false;
    }
    let notches = ui.input_mut(|i| {
        let mut notches = 0.0f32;
        i.events.retain(|event| match event {
            // Modified scrolls mean something else (zoom, horizontal pan).
            egui::Event::MouseWheel {
                unit,
                delta,
                modifiers,
                ..
            } if modifiers.is_none() => {
                notches += match unit {
                    egui::MouseWheelUnit::Line => delta.y,
                    egui::MouseWheelUnit::Point => delta.y / POINTS_PER_NOTCH,
                    // No desktop backend sends this; a page means end to end.
                    egui::MouseWheelUnit::Page => delta.y * NOTCHES_PER_RANGE as f32,
                };
                false
            }
            _ => true,
        });
        // Whether or not this frame carried an event, suppress the smoothed
        // value: it is the tail of a wheel already counted here, and letting it
        // through would scroll whatever is behind the slider.
        i.smooth_scroll_delta.y = 0.0;
        notches
    });
    if notches == 0.0 {
        return false;
    }
    let notch = notch_size(&range, step);
    // Scrolling up raises the value, which is the direction every host DAW uses.
    let next = (*value + notches as f64 * notch).clamp(*range.start(), *range.end());
    if next == *value {
        return false;
    }
    *value = next;
    true
}

/// How much one wheel notch moves a slider: a fixed fraction of its range, but
/// never finer than the slider's own step.
fn notch_size(range: &std::ops::RangeInclusive<f64>, step: f64) -> f64 {
    let span = (range.end() - range.start()).abs();
    (span / NOTCHES_PER_RANGE).max(step.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs a frame with the pointer over a slider-sized widget and `notches`
    /// separate wheel events, returning the value and whether it changed.
    /// `unit` is what the device reports: a mouse sends lines, a trackpad
    /// sends points.
    fn scrolled_with(
        unit: egui::MouseWheelUnit,
        delta: f32,
        events: usize,
        value: f64,
        step: f64,
    ) -> (f64, bool) {
        let mut harness = egui_kittest::Harness::new_ui_state(
            |ui, state: &mut (f64, bool)| {
                let (_, response) =
                    ui.allocate_exact_size(egui::vec2(200.0, 200.0), egui::Sense::hover());
                state.1 |= scroll_adjust(ui, &response, &mut state.0, 0.0..=1.0, step);
            },
            (value, false),
        );
        harness.run();
        // The pointer has to be over the widget for `hovered()` to hold.
        harness
            .input_mut()
            .events
            .push(egui::Event::PointerMoved(egui::pos2(20.0, 20.0)));
        for _ in 0..events {
            harness.input_mut().events.push(egui::Event::MouseWheel {
                unit,
                delta: egui::vec2(0.0, delta),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            });
        }
        harness.run();
        *harness.state()
    }

    /// One mouse notch: a single line.
    fn scrolled(notches: f32, value: f64, step: f64) -> (f64, bool) {
        scrolled_with(egui::MouseWheelUnit::Line, notches, 1, value, step)
    }

    /// Values are compared loosely: `step` is a decimal fraction.
    fn close(got: f64, want: f64) -> bool {
        (got - want).abs() < 1e-6
    }

    #[test]
    fn one_notch_up_is_one_step_up_and_down_is_down() {
        let (up, changed) = scrolled(1.0, 0.5, 0.1);
        assert!(close(up, 0.6), "scrolling up gave {up}");
        assert!(changed);
        let (down, _) = scrolled(-1.0, 0.5, 0.1);
        assert!(close(down, 0.4), "scrolling down gave {down}");
    }

    #[test]
    fn a_longer_scroll_moves_proportionally_further() {
        // Four notches, so four steps — a trackpad flick is not one notch.
        let (got, _) = scrolled(4.0, 0.5, 0.1);
        assert!(close(got, 0.9), "four notches gave {got}");
    }

    #[test]
    fn every_notch_of_a_burst_lands() {
        // The bug this replaced: rudel repaints on input while stopped, so the
        // smoothed delta was cut off partway and most of each notch was lost.
        // Four separate wheel events in one frame must all count.
        let (got, _) = scrolled_with(egui::MouseWheelUnit::Line, 1.0, 4, 0.5, 0.1);
        assert!(close(got, 0.9), "four separate notches gave {got}");
    }

    #[test]
    fn a_trackpad_reporting_points_moves_at_the_same_rate() {
        let (got, _) = scrolled_with(egui::MouseWheelUnit::Point, POINTS_PER_NOTCH, 1, 0.5, 0.1);
        assert!(close(got, 0.6), "one notch of points gave {got}");
    }

    #[test]
    fn a_slider_inside_a_scroll_area_adjusts_without_scrolling_it() {
        // The inline `slider(...)` controls live in the code editor's scroll
        // area. Hovering one and scrolling has to move the slider and leave the
        // view where it is, or the control slides out from under the pointer.
        let mut harness = egui_kittest::Harness::new_ui_state(
            |ui, state: &mut (f64, f32)| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let (_, response) =
                        ui.allocate_exact_size(egui::vec2(200.0, 100.0), egui::Sense::hover());
                    scroll_adjust(ui, &response, &mut state.0, 0.0..=1.0, 0.1);
                    // Far taller than the viewport, so it can scroll.
                    ui.allocate_space(egui::vec2(200.0, 4000.0));
                    state.1 = ui.min_rect().top();
                });
            },
            (0.5, 0.0),
        );
        harness.run();
        let top_before = harness.state().1;

        harness
            .input_mut()
            .events
            .push(egui::Event::PointerMoved(egui::pos2(20.0, 20.0)));
        for _ in 0..3 {
            harness.input_mut().events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: egui::vec2(0.0, -1.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            });
        }
        harness.run();

        let (value, top_after) = *harness.state();
        assert!(close(value, 0.2), "the slider should have moved: {value}");
        assert!(
            (top_after - top_before).abs() < 0.5,
            "the view scrolled from {top_before} to {top_after}"
        );
    }

    #[test]
    fn every_slider_takes_the_same_scrolling_end_to_end() {
        // The whole point: the increment tracks the range, so a wide slider is
        // not a thousand notches and a narrow one is not four.
        for (min, max) in [(0.1, 2.0), (0.0, 200.0), (300.0, 2000.0), (-1.0, 1.0)] {
            let notch = notch_size(&(min..=max), 0.0);
            assert!(
                close(notch, (max - min) / NOTCHES_PER_RANGE),
                "{min}..{max} gave {notch}"
            );
        }
    }

    #[test]
    fn a_slider_with_coarse_stops_keeps_them() {
        // The editor font size: 11..32 by 1. A hundredth of that range is 0.21,
        // which would land the size between its stops, so the step wins.
        assert!(close(notch_size(&(11.0..=32.0), 1.0), 1.0));
        // But a step finer than a hundredth of the range never slows it down —
        // this is the `cps` slider, which used to crawl at its own 0.01.
        assert!(close(notch_size(&(0.1..=2.0), 0.01), 0.019));
    }

    #[test]
    fn the_value_stops_at_the_ends_of_the_range() {
        assert_eq!(scrolled(1.0, 1.0, 0.1), (1.0, false));
        assert_eq!(scrolled(-1.0, 0.0, 0.1), (0.0, false));
    }

    #[test]
    fn no_scroll_is_no_change() {
        assert_eq!(scrolled(0.0, 0.5, 0.1), (0.5, false));
    }
}

#[cfg(test)]
mod page_and_modifier_tests {
    use super::*;

    fn wheel(unit: egui::MouseWheelUnit, delta: f32, modifiers: egui::Modifiers) -> f64 {
        let mut harness = egui_kittest::Harness::new_ui_state(
            |ui, value: &mut f64| {
                let (_, response) =
                    ui.allocate_exact_size(egui::vec2(200.0, 200.0), egui::Sense::hover());
                scroll_adjust(ui, &response, value, 0.0..=1.0, 0.0);
            },
            0.0,
        );
        harness.run();
        harness
            .input_mut()
            .events
            .push(egui::Event::PointerMoved(egui::pos2(20.0, 20.0)));
        harness.input_mut().events.push(egui::Event::MouseWheel {
            unit,
            delta: egui::vec2(0.0, delta),
            phase: egui::TouchPhase::Move,
            modifiers,
        });
        harness.run();
        *harness.state()
    }

    #[test]
    fn half_a_page_moves_half_the_range() {
        let got = wheel(egui::MouseWheelUnit::Page, 0.5, egui::Modifiers::NONE);
        assert!((got - 0.5).abs() < 1e-9, "{got}");
    }

    #[test]
    fn a_modified_wheel_is_left_to_whatever_else_wants_it() {
        assert_eq!(
            wheel(egui::MouseWheelUnit::Line, 3.0, egui::Modifiers::CTRL),
            0.0
        );
    }
}
