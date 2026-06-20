use super::geometry::WIDGET_GAP_PADDING;
use super::options::VisualWidgetOptions;
use super::pianoroll::{RollRectInput, RollValue, horizontal_roll_rect, pianoroll_value};
use super::pitchwheel::freq_to_angle;
use super::query::hap_matches_widget;
use super::size::{default_surface_size, surface_size};
use super::spiral::spiral_point;
use super::style::{color_with_alpha, parse_hex_color, resolve_color, widget_draw_colors};
use super::*;
use crate::editor::decorations::{SourceRange, WidgetDecoration};
use crate::editor::settings::EditorTheme;
use eframe::egui;
use rudel_core::{Frac, Hap, Value};
use std::collections::BTreeMap;

fn widget(widget_type: &str, id: &str, from: usize, to: usize) -> WidgetDecoration {
    WidgetDecoration {
        widget_type: widget_type.to_string(),
        id: id.to_string(),
        range: SourceRange::new(from, to),
        index: 0,
        options: BTreeMap::new(),
    }
}

fn widget_with_options(
    widget_type: &str,
    options: &[(&str, rudel_lang::WidgetOption)],
) -> WidgetDecoration {
    let mut widget = widget(widget_type, "options", 0, 1);
    widget.options = options
        .iter()
        .map(|(key, value)| ((*key).to_string(), value.clone()))
        .collect();
    widget
}

fn hap(value: Value) -> Hap {
    Hap::new(
        Some(rudel_core::TimeSpan::new(Frac::zero(), Frac::new(1, 2))),
        rudel_core::TimeSpan::new(Frac::zero(), Frac::new(1, 2)),
        value,
    )
}

fn tagged_hap(tag: &str, value: Value) -> Hap {
    let mut hap = hap(value);
    hap.context.tags.push(tag.to_string());
    hap
}

#[test]
fn sync_creates_reuses_and_removes_surfaces_by_type_and_id() {
    let mut host = WidgetHostState::default();
    let first = vec![
        widget("_spiral", "same", 0, 12),
        widget("_scope", "scope", 20, 30),
    ];
    let sync = host.sync(&first);
    let spiral_serial = host.surface_serial("_spiral", "same").unwrap();

    assert_eq!(sync.created, vec!["same", "scope"]);
    assert!(sync.removed.is_empty());
    assert_eq!(host.surface_count(), 2);

    let second = vec![
        widget("_spiral", "same", 100, 120),
        widget("_pitchwheel", "wheel", 40, 50),
    ];
    let sync = host.sync(&second);

    assert_eq!(host.surface_serial("_spiral", "same"), Some(spiral_serial));
    assert_eq!(sync.created, vec!["wheel"]);
    assert_eq!(sync.removed, vec!["scope"]);
    assert_eq!(host.surface_count(), 2);
}

#[test]
fn block_widget_line_heights_reserve_a_gap_below_the_widget_line() {
    // Widget anchored on line 1 (its placement byte falls in "line1").
    let code = "line0\nline1\nline2";
    let heights = block_widget_line_heights(code, &[widget("_pianoroll", "p", 6, 11)], 20.0);

    // base row (20) + default _pianoroll height (60) + padding.
    assert_eq!(heights.get(&1), Some(&(20.0 + 60.0 + WIDGET_GAP_PADDING)));
    assert_eq!(heights.get(&0), None);
}

#[test]
fn widget_identity_includes_type_and_id() {
    let mut host = WidgetHostState::default();
    host.sync(&[
        widget("_scope", "shared", 0, 1),
        widget("_spectrum", "shared", 2, 3),
    ]);

    assert_eq!(host.surface_count(), 2);
    assert_ne!(
        host.surface_serial("_scope", "shared"),
        host.surface_serial("_spectrum", "shared")
    );
}

#[test]
fn placement_uses_to_or_from_like_codemirror_widget_range() {
    assert_eq!(widget("_spiral", "a", 4, 12).placement(), 12);
    assert_eq!(widget("_spiral", "a", 4, 4).placement(), 4);
}

#[test]
fn default_sizes_follow_strudel_canvas_defaults() {
    assert_eq!(default_surface_size("_pianoroll"), egui::vec2(500.0, 60.0));
    assert_eq!(default_surface_size("_scope"), egui::vec2(500.0, 60.0));
    assert_eq!(default_surface_size("_spiral"), egui::vec2(275.0, 275.0));
    assert_eq!(
        default_surface_size("_pitchwheel"),
        egui::vec2(200.0, 200.0)
    );
}

#[test]
fn surface_size_follows_widget_size_width_and_height_options() {
    let sized = widget_with_options(
        "_spiral",
        &[("size", rudel_lang::WidgetOption::Number(180.0))],
    );
    let explicit = widget_with_options(
        "_pianoroll",
        &[
            ("width", rudel_lang::WidgetOption::Number(320.0)),
            ("height", rudel_lang::WidgetOption::Number(90.0)),
        ],
    );

    assert_eq!(surface_size(&sized), egui::vec2(180.0, 180.0));
    assert_eq!(surface_size(&explicit), egui::vec2(320.0, 90.0));
}

#[test]
fn widget_draw_colors_follow_strudel_draw_theme_defaults() {
    let colors = widget_draw_colors(EditorTheme::StrudelDark.draw_theme());
    assert_eq!(colors.active, egui::Color32::WHITE);
    assert_eq!(
        colors.inactive,
        egui::Color32::from_rgba_unmultiplied(0x8a, 0x91, 0x99, 0x66)
    );
    assert_eq!(
        colors.background,
        egui::Color32::from_rgba_unmultiplied(0x22, 0x22, 0x22, 0x99)
    );
}

#[test]
fn hap_matching_prefers_widget_tags_and_falls_back_to_source_locations() {
    let target = widget("_spiral", "target", 10, 20);
    let tagged = tagged_hap("target", Value::Int(60));
    let other = tagged_hap("other", Value::Int(60));
    let mut located = hap(Value::Int(60));
    located.context.locations.push((12, 14));

    assert!(hap_matches_widget(&tagged, &target));
    assert!(!hap_matches_widget(&other, &target));
    assert!(hap_matches_widget(&located, &target));
}

#[test]
fn pianoroll_value_matches_strudel_value_priority() {
    let freq = hap(Value::Map(BTreeMap::from([(
        "freq".to_string(),
        Value::F64(440.0),
    )])));
    let note = hap(Value::Map(BTreeMap::from([(
        "note".to_string(),
        Value::Str("c4".to_string()),
    )])));
    let sound = hap(Value::Map(BTreeMap::from([(
        "s".to_string(),
        Value::Str("bd".to_string()),
    )])));

    assert_eq!(pianoroll_value(&freq), RollValue::Number(69.0));
    assert_eq!(pianoroll_value(&note), RollValue::Number(60.0));
    assert_eq!(pianoroll_value(&sound), RollValue::Text("_bd".to_string()));
}

#[test]
fn pianoroll_rect_places_current_time_at_the_playhead() {
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 40.0));
    let block = horizontal_roll_rect(
        rect,
        RollRectInput {
            value_index: 0,
            slots: 1,
            begin: 10.0,
            end: 10.5,
            window_start: 8.0,
            time_extent: 4.0,
            options: VisualWidgetOptions::from_widget(&widget("_pianoroll", "piano", 0, 1)),
        },
    );

    assert!((block.left() - 201.0).abs() < 1e-4);
    assert!((block.right() - 249.0).abs() < 1e-4);
    assert!((block.top() - 1.0).abs() < 1e-4);
}

#[test]
fn visual_widget_options_read_strudel_style_booleans_numbers_and_colors() {
    let widget = widget_with_options(
        "_pianoroll",
        &[
            ("cycles", rudel_lang::WidgetOption::Number(2.0)),
            ("labels", rudel_lang::WidgetOption::Number(1.0)),
            (
                "active",
                rudel_lang::WidgetOption::String("#ff00ff".to_string()),
            ),
        ],
    );
    let options = VisualWidgetOptions::from_widget(&widget);

    assert_eq!(options.cycles, 2.0);
    assert!(options.labels);
    assert_eq!(
        options.active_color,
        Some(egui::Color32::from_rgb(0xff, 0, 0xff))
    );
}

#[test]
fn spiral_options_map_inline_canvas_size_to_draw_size() {
    let default = VisualWidgetOptions::from_widget(&widget("_spiral", "spiral", 0, 1));
    let sized = VisualWidgetOptions::from_widget(&widget_with_options(
        "_spiral",
        &[("size", rudel_lang::WidgetOption::Number(250.0))],
    ));

    assert_eq!(default.spiral_size, 55.0);
    assert_eq!(sized.spiral_size, 50.0);
}

#[test]
fn pitchwheel_angle_matches_strudel_frequency_mapping() {
    let root = rudel_core::midi_to_freq(36.0);

    assert!((freq_to_angle(root, root) - 0.5).abs() < 1e-6);
    assert!((freq_to_angle(root * 2f64.powf(0.5), root) - 0.0).abs() < 1e-6);
    assert!((freq_to_angle(root / 2f64.powf(0.25), root) - 0.75).abs() < 1e-6);
}

#[test]
fn spiral_point_matches_strudel_polar_mapping() {
    let center = egui::pos2(100.0, 100.0);
    let at_start = spiral_point(0.0, 10.0, center, 0.0, 1.0);
    let one_turn = spiral_point(1.0, 10.0, center, 0.0, 1.0);

    assert!((at_start.x - 100.0).abs() < 1e-4);
    assert!((at_start.y - 100.0).abs() < 1e-4);
    assert!((one_turn.x - 100.0).abs() < 1e-4);
    assert!((one_turn.y - 90.0).abs() < 1e-4);
}

#[test]
fn parses_hex_event_colors_and_applies_alpha() {
    assert_eq!(
        parse_hex_color("#ff000080"),
        Some(egui::Color32::from_rgba_unmultiplied(0xff, 0, 0, 0x80))
    );
    assert_eq!(
        color_with_alpha(egui::Color32::from_rgba_unmultiplied(10, 20, 30, 200), 0.5),
        egui::Color32::from_rgba_unmultiplied(10, 20, 30, 100)
    );
}

#[test]
fn resolves_css_named_colors_and_hex() {
    // hex passes straight through
    assert_eq!(
        resolve_color("#ff0000"),
        Some(egui::Color32::from_rgb(0xff, 0, 0))
    );
    // CSS names resolve through draw/color.mjs's table (case-insensitively)
    assert_eq!(
        resolve_color("red"),
        Some(egui::Color32::from_rgb(0xff, 0, 0))
    );
    assert_eq!(
        resolve_color("CadetBlue"),
        Some(egui::Color32::from_rgb(0x5f, 0x9e, 0xa0))
    );
    // unrecognized names fall back to None (caller uses the theme color)
    assert_eq!(resolve_color("notacolor"), None);
}
