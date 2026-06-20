use super::options::DrawWindow;
use crate::editor::decorations::WidgetDecoration;
use rudel_core::{Frac, Hap, Pattern};

pub(super) fn widget_haps(
    pattern: &Pattern,
    widget: &WidgetDecoration,
    window: DrawWindow,
) -> Vec<Hap> {
    let begin = Frac::from_f64(window.begin);
    let end = Frac::from_f64(window.end);
    let mut haps: Vec<Hap> = pattern
        .query_arc(begin, end)
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
