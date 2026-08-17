use super::options::DrawWindow;
use crate::editor::decorations::WidgetDecoration;
use eframe::egui;
use rudel_core::{Frac, Hap, Pattern};

/// One widget's haps over a whole-cycle span, kept in egui's temp store between
/// frames. `generation` and `range` are the inputs to the query that are not in
/// the cache id: a re-eval swaps the pattern, and typing slides the widget's
/// source range (which decides what `hap_matches_widget` keeps).
#[derive(Clone)]
struct CachedHaps {
    generation: u64,
    range: (usize, usize),
    cycles: (i64, i64),
    haps: Vec<Hap>,
}

/// The haps a widget should draw for `window`.
///
/// The draw window slides with the playhead, so querying it directly re-runs
/// the whole pattern at the repaint rate — profiling put that at 66% of the UI
/// thread. Whole cycles change only once per cycle, so query those, cache them,
/// and slice the visible window out of the result.
pub(super) fn widget_haps(
    ctx: &egui::Context,
    generation: u64,
    pattern: &Pattern,
    widget: &WidgetDecoration,
    window: DrawWindow,
) -> Vec<Hap> {
    let cycles = (window.begin.floor() as i64, window.end.ceil() as i64);
    let range = (widget.range.from, widget.range.to);
    let id = egui::Id::new((
        "rudel-widget-haps",
        widget.widget_type.as_str(),
        widget.id.as_str(),
    ));
    let fresh = || CachedHaps {
        generation,
        range,
        cycles,
        haps: query_cycles(pattern, widget, cycles),
    };
    ctx.data_mut(|d| {
        let cached = d.get_temp_mut_or_insert_with(id, fresh);
        if cached.generation != generation || cached.range != range || cached.cycles != cycles {
            *cached = fresh();
        }
        // The cached span is a superset of `window`, and widening a query only
        // widens each hap's `part` clip — it never changes which haps overlap
        // `window` — so this is the same set `query_arc(window)` would return.
        let (begin, end) = (Frac::from_f64(window.begin), Frac::from_f64(window.end));
        cached
            .haps
            .iter()
            .filter(|hap| hap.part.begin < end && begin < hap.part.end)
            .cloned()
            .collect()
    })
}

fn query_cycles(pattern: &Pattern, widget: &WidgetDecoration, cycles: (i64, i64)) -> Vec<Hap> {
    let mut haps: Vec<Hap> = pattern
        .query_arc(Frac::new(cycles.0, 1), Frac::new(cycles.1, 1))
        .into_iter()
        .filter(|hap| hap.whole.is_some())
        .filter(|hap| hap_matches_widget(hap, widget))
        .collect();
    haps.sort_by_key(|hap| hap.whole_or_part().begin);
    haps
}

pub(super) fn hap_matches_widget(hap: &Hap, widget: &WidgetDecoration) -> bool {
    if hap.has_tag(&widget.id) {
        return true;
    }
    if !hap.context.tags.is_empty() {
        return false;
    }
    hap.context
        .locations
        .iter()
        .any(|&location| ranges_overlap(location, (widget.range.from, widget.range.to)))
}

fn ranges_overlap(a: (usize, usize), b: (usize, usize)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

pub(super) fn hap_is_active(hap: &Hap, time: f64) -> bool {
    let t = Frac::from_f64(time);
    hap.whole
        .is_some_and(|whole| whole.begin <= t && hap.end_clipped() > t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::decorations::SourceRange;

    fn span(begin: (i64, i64), end: (i64, i64)) -> rudel_core::TimeSpan {
        rudel_core::TimeSpan::new(Frac::new(begin.0, begin.1), Frac::new(end.0, end.1))
    }

    #[test]
    fn ranges_touching_end_to_end_do_not_overlap() {
        // Source spans are half-open, so two adjacent calls each keep their own
        // widget rather than both claiming the boundary character.
        assert!(!ranges_overlap((0, 2), (2, 4)), "a ends where b begins");
        assert!(!ranges_overlap((2, 4), (0, 2)), "and the other way round");
        assert!(ranges_overlap((0, 3), (2, 4)), "sharing a character");
        assert!(ranges_overlap((0, 5), (1, 2)), "one inside the other");
        assert!(!ranges_overlap((0, 0), (0, 1)), "an empty range overlaps nothing");
    }

    #[test]
    fn a_hap_is_active_from_its_onset_until_its_end() {
        let hap = Hap::new(
            Some(span((0, 1), (1, 2))),
            span((0, 1), (1, 2)),
            rudel_core::Value::F64(1.0),
        );
        assert!(hap_is_active(&hap, 0.0), "at the onset");
        assert!(hap_is_active(&hap, 0.25), "part way through");
        assert!(!hap_is_active(&hap, 0.5), "the end is exclusive");
        assert!(!hap_is_active(&hap, -0.1), "before it starts");

        // A continuous hap has no onset to be active from — the widgets draw
        // discrete events only.
        let signal = Hap::new(None, span((0, 1), (1, 2)), rudel_core::Value::F64(1.0));
        assert!(!hap_is_active(&signal, 0.25));
    }

    #[test]
    fn only_the_haps_touching_the_window_are_handed_back() {
        // The cache holds whole cycles; the visible window is sliced out of it,
        // again half-open, so a note ending exactly as the window opens is
        // already gone.
        let ctx = egui::Context::default();
        let pattern = rudel_lang::eval(r#"note("c3 e3 g3 a3")"#).expect("eval");
        let widget = WidgetDecoration {
            widget_type: "_pianoroll".to_string(),
            id: "w".to_string(),
            // Wide enough to claim every hap the source produced.
            range: SourceRange::new(0, 100),
            index: 0,
            options: Default::default(),
        };
        let haps = |begin, end| {
            widget_haps(&ctx, 0, &pattern, &widget, DrawWindow { begin, end })
                .into_iter()
                .map(|hap| hap.part.begin)
                .collect::<Vec<_>>()
        };
        // The second quarter only: [0.25, 0.5) touches the note starting at
        // 0.25 and neither neighbour.
        assert_eq!(haps(0.25, 0.5), vec![Frac::new(1, 4)]);
        // Widening to the whole cycle takes all four.
        assert_eq!(haps(0.0, 1.0).len(), 4);
    }
}
