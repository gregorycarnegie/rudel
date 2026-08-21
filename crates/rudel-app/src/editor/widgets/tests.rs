use super::{
    claviature::{hap_midi, is_black},
    geometry::WIDGET_GAP_PADDING,
    options::{DrawWindow, VisualWidgetOptions},
    pianoroll::{RollRectInput, RollValue, horizontal_roll_rect, pianoroll_value},
    pitchwheel::freq_to_angle,
    query::{hap_matches_widget, in_window, widget_haps},
    size::{default_surface_size, surface_size},
    spiral::spiral_point,
    style::{color_with_alpha, resolve_color, widget_draw_colors},
    *,
};
use crate::editor::{
    decorations::{SourceRange, WidgetDecoration},
    settings::EditorTheme,
};
use eframe::egui;
use rudel_core::{Frac, Hap, Value, ValueMap};
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
    // `_spiral` is the one exception, and has its own test below saying why.
    assert_eq!(default_surface_size("_pianoroll"), egui::vec2(500.0, 60.0));
    assert_eq!(default_surface_size("_scope"), egui::vec2(500.0, 60.0));
    assert_eq!(
        default_surface_size("_pitchwheel"),
        egui::vec2(200.0, 200.0)
    );
}

#[test]
fn the_default_spiral_surface_fits_the_now_arc() {
    // The one default that deliberately does not follow Strudel's canvas.
    // Upstream's `_spiral` canvas is 275 with `size` = 275/5 = 55, so `inset: 3`
    // puts the "now" arc at radius 165 -- past the 137.5 that canvas inscribes,
    // and the current position ends up clipped into the corners.
    //
    // Rudel widens the surface rather than touching the geometry: `inset` keeps
    // its documented default and `spiral_size` stays 55, so a pattern copied
    // from Strudel draws the same spiral. Only the canvas is bigger.
    let options = VisualWidgetOptions::from_widget(&widget_with_options("_spiral", &[]));
    assert_eq!(options.spiral_size, 55.0, "the geometry must not move");
    assert_eq!(options.inset, 3.0, "nor may `inset` drift from upstream's");

    let margin = options.spiral_size / options.stretch;
    let thickness = options
        .spiral_thickness
        .unwrap_or(options.spiral_size / 2.0);
    let now_outer_edge = options.inset * margin + thickness / 2.0;
    let inscribed = default_surface_size("_spiral").x / 2.0;
    assert!(
        inscribed > now_outer_edge,
        "the now arc reaches {now_outer_edge} but the surface inscribes {inscribed}"
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
    assert_eq!(colors.foreground, egui::Color32::WHITE);
    assert_eq!(
        colors.muted,
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
    let freq = hap(Value::Map(ValueMap::from([(
        "freq".to_string(),
        Value::F64(440.0),
    )])));
    let note = hap(Value::Map(ValueMap::from([(
        "note".to_string(),
        Value::Str("c4".to_string()),
    )])));
    let sound = hap(Value::Map(ValueMap::from([(
        "s".to_string(),
        Value::Str("bd".to_string()),
    )])));

    // Transposing is the one case that needs the control map owned; everything
    // else reads it borrowed, so this is what catches the borrow being kept.
    let transposed = hap(Value::Map(ValueMap::from([
        ("note".to_string(), Value::Str("c4".to_string())),
        ("ctranspose".to_string(), Value::F64(3.0)),
    ])));

    assert_eq!(pianoroll_value(&freq), RollValue::Number(69.0));
    assert_eq!(pianoroll_value(&note), RollValue::Number(60.0));
    assert_eq!(pianoroll_value(&sound), RollValue::Text("_bd".to_string()));
    assert_eq!(pianoroll_value(&transposed), RollValue::Number(63.0));
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
fn analyzer_and_claviature_options_follow_strudel_names() {
    let scope = widget_with_options(
        "_scope",
        &[
            ("align", rudel_lang::WidgetOption::Number(0.0)),
            ("trigger", rudel_lang::WidgetOption::Number(0.1)),
            ("pos", rudel_lang::WidgetOption::Number(0.25)),
            ("scale", rudel_lang::WidgetOption::Number(0.5)),
            ("smear", rudel_lang::WidgetOption::Number(0.8)),
            ("lowest", rudel_lang::WidgetOption::String("c1".to_string())),
            ("highest", rudel_lang::WidgetOption::Number(96.0)),
        ],
    );
    let options = VisualWidgetOptions::from_widget(&scope);

    assert!(!options.align);
    assert_eq!(options.trigger, 0.1);
    assert_eq!(options.pos, Some(0.25));
    assert_eq!(options.scale, Some(0.5));
    assert_eq!(options.smear, 0.8);
    assert_eq!(options.lowest, Some(24.0)); // note name resolves to midi
    assert_eq!(options.highest, Some(96.0));

    // per-widget defaults live in the painters; the parse defaults are open
    let defaults = VisualWidgetOptions::from_widget(&widget("_spectrum", "s", 0, 1));
    assert!(defaults.align);
    assert_eq!(defaults.pos, None);
    assert_eq!(defaults.speed, 1.0);
}

#[test]
fn claviature_maps_notes_to_midi_and_key_colors() {
    // note name and freq both resolve to the same midi key
    let named = hap(Value::Map(ValueMap::from([(
        "note".to_string(),
        Value::Str("c4".to_string()),
    )])));
    let tuned = hap(Value::Map(ValueMap::from([(
        "freq".to_string(),
        Value::F64(440.0),
    )])));
    assert_eq!(hap_midi(&named), Some(60));
    assert_eq!(hap_midi(&tuned), Some(69));
    // black-key pattern of an octave: C# D# F# G# A#
    let blacks: Vec<i32> = (60..72).filter(|&m| is_black(m)).collect();
    assert_eq!(blacks, vec![61, 63, 66, 68, 70]);
}

#[test]
fn parses_hex_event_colors_and_applies_alpha() {
    assert_eq!(
        resolve_color("#ff000080"),
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

#[test]
fn overscan_widens_the_query_window_only() {
    // Strudel widens `lookbehind`/`lookahead` by overscan while the drawn
    // frame still spans `cycles`, so the geometry must not move.
    let plain = VisualWidgetOptions::from_widget(&widget_with_options(
        "_pianoroll",
        &[("cycles", rudel_lang::WidgetOption::Number(4.0))],
    ));
    let scanned = VisualWidgetOptions::from_widget(&widget_with_options(
        "_pianoroll",
        &[
            ("cycles", rudel_lang::WidgetOption::Number(4.0)),
            ("overscan", rudel_lang::WidgetOption::Number(1.0)),
        ],
    ));
    assert_eq!(plain.overscan, 0.0, "overscan defaults to 0 like upstream");
    assert_eq!(scanned.overscan, 1.0);

    let (plain, scanned) = (plain.window(10.0), scanned.window(10.0));
    // cycles=4, playhead=0.5 -> [8, 12], widened by 1 on each side.
    assert_eq!((plain.begin, plain.end), (8.0, 12.0));
    assert_eq!((scanned.begin, scanned.end), (7.0, 13.0));
}

#[test]
fn spiral_cap_follows_the_canvas_line_cap_names() {
    use crate::editor::widgets::spiral::SpiralCap;
    let cap = |name: &str| {
        VisualWidgetOptions::from_widget(&widget_with_options(
            "_spiral",
            &[("cap", rudel_lang::WidgetOption::String(name.to_string()))],
        ))
        .spiral_cap
    };
    assert_eq!(cap("round"), SpiralCap::Round);
    assert_eq!(cap("square"), SpiralCap::Square);
    assert_eq!(cap("butt"), SpiralCap::Butt);
    // Upstream's default is butt, and an unknown name falls back to it.
    assert_eq!(cap("nonsense"), SpiralCap::Butt);
    assert_eq!(
        VisualWidgetOptions::from_widget(&widget("_spiral", "s", 0, 1)).spiral_cap,
        SpiralCap::Butt
    );
}

#[test]
fn pitchwheel_interval_labels_index_by_degree_position() {
    use crate::editor::widgets::pitchwheel::degree_label;

    // `intLabels` is indexed by the degree's position within `degreeIndexes`,
    // not by the ring degree itself (upstream's `degreeIndexes.indexOf(i)`).
    let degrees = Some(vec![0, 2, 4, 5, 7, 9, 11]);
    let labels = Some(vec![
        "1".to_string(),
        "2".to_string(),
        "3".to_string(),
        "4".to_string(),
        "5".to_string(),
        "6".to_string(),
        "7".to_string(),
    ]);
    assert_eq!(degree_label(&degrees, &labels, 0).as_deref(), Some("1"));
    assert_eq!(degree_label(&degrees, &labels, 4).as_deref(), Some("3"));
    assert_eq!(degree_label(&degrees, &labels, 11).as_deref(), Some("7"));
    // A degree outside the scale has no label.
    assert_eq!(degree_label(&degrees, &labels, 1), None);
    // Missing or empty data is skipped rather than drawn blank.
    assert_eq!(degree_label(&degrees, &None, 0), None);
    assert_eq!(degree_label(&None, &labels, 0), None);
    assert_eq!(
        degree_label(&Some(vec![0]), &Some(vec![String::new()]), 0),
        None
    );
}

/// The hap cache queries whole cycles and slices the sliding draw window out of
/// the result. That is only sound if it returns exactly what querying the
/// window directly would — widening a query widens each hap's `part` clip, but
/// must never change *which* haps overlap the window.
#[test]
fn cached_whole_cycle_query_matches_querying_the_window_directly() {
    let src = r#"s("bd sd hh*3").fast(2)"#;
    let pattern = rudel_lang::eval_result(src).expect("eval").pattern;
    let widget = widget("_pianoroll", "roll", 0, src.len());
    let ctx = egui::Context::default();

    let uncached = |window: DrawWindow| {
        let mut haps: Vec<Hap> = pattern
            .query_arc(Frac::from_f64(window.begin), Frac::from_f64(window.end))
            .into_iter()
            .filter(|hap| hap.whole.is_some())
            .filter(|hap| hap_matches_widget(hap, &widget))
            .collect();
        haps.sort_by_key(|hap| hap.whole_or_part().begin);
        haps
    };
    let shape = |haps: &[&Hap]| -> Vec<(Frac, Frac, String)> {
        haps.iter()
            .map(|hap| {
                let whole = hap.whole.expect("filtered to haps with a whole");
                (whole.begin, whole.end, format!("{:?}", hap.value))
            })
            .collect()
    };

    // Sweep the playhead across cycle boundaries in steps that never land on
    // one, so most windows straddle the whole-cycle span that gets cached.
    for step in 0..40 {
        let time = f64::from(step) * 0.17;
        let window = DrawWindow::around(time);
        assert_eq!(
            shape(&in_window(&widget_haps(&ctx, 1, &pattern, &widget, window), window)),
            shape(&uncached(window).iter().collect::<Vec<_>>()),
            "cached and direct queries disagree at time {time}"
        );
    }
}

/// The cache is keyed on the widget, so a re-evaluation has to invalidate it —
/// otherwise an edited pattern keeps drawing the old one's haps.
#[test]
fn bumping_the_generation_drops_haps_from_the_previous_pattern() {
    let widget = widget("_pianoroll", "roll", 0, 40);
    let ctx = egui::Context::default();
    let window = DrawWindow::around(0.5);
    let count = |src: &str, generation: u64| {
        let pattern = rudel_lang::eval_result(src).expect("eval").pattern;
        in_window(&widget_haps(&ctx, generation, &pattern, &widget, window), window).len()
    };

    let one = count(r#"s("bd")"#, 1);
    let four = count(r#"s("bd*4")"#, 2);
    assert!(
        one > 0,
        "the window covers several cycles of a once-a-cycle bd"
    );
    assert!(
        four > one,
        "the new pattern's haps, not the cached old ones ({four} vs {one})"
    );
    // Same generation as the last call: the cached result is reused even though
    // the pattern argument changed, which is exactly why `evaluate` must bump.
    assert_eq!(count(r#"s("bd")"#, 2), four);
}

#[test]
fn a_note_block_stays_visible_however_wide_the_value_range() {
    use super::pianoroll::keep_note_visible;
    use eframe::egui;

    // An unfolded roll over the default `minMidi..maxMidi` gives 81 slots; in
    // an ~80pt-tall widget that is under a point per note, which painted
    // nothing at all. The block is grown across its value axis instead.
    let sliver = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(30.0, 0.9));
    let grown = keep_note_visible(sliver, false);
    assert!(grown.height() >= 2.5, "grown to {}", grown.height());
    assert_eq!(grown.width(), sliver.width(), "time axis is untouched");
    assert_eq!(
        grown.center(),
        sliver.center(),
        "the note stays on its own slot"
    );

    // A vertical roll grows the other way.
    let thin = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(0.9, 30.0));
    let grown = keep_note_visible(thin, true);
    assert!(grown.width() >= 2.5, "grown to {}", grown.width());
    assert_eq!(grown.height(), thin.height());

    // A block already big enough is left exactly as it is.
    let roomy = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(20.0, 12.0));
    assert_eq!(keep_note_visible(roomy, false), roomy);
    assert_eq!(keep_note_visible(roomy, true), roomy);
}
