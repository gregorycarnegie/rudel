use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SourceRange {
    pub(crate) from: usize,
    pub(crate) to: usize,
}

impl SourceRange {
    pub(crate) fn new(from: usize, to: usize) -> Self {
        Self { from, to }
    }

    fn mapped(self, change: TextChange) -> Self {
        let from = change.map_pos(self.from, Assoc::Before);
        let to = change.map_pos(self.to, Assoc::After);
        if from <= to {
            Self { from, to }
        } else {
            Self { from: to, to: from }
        }
    }
}

impl From<(usize, usize)> for SourceRange {
    fn from((from, to): (usize, usize)) -> Self {
        Self { from, to }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Assoc {
    Before,
    After,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TextChange {
    pub(crate) from: usize,
    pub(crate) to: usize,
    pub(crate) insert_len: usize,
}

impl TextChange {
    pub(crate) fn from_texts(before: &str, after: &str) -> Option<Self> {
        if before == after {
            return None;
        }
        let prefix = common_prefix_bytes(before, after);
        let suffix = common_suffix_bytes(&before[prefix..], &after[prefix..]);
        let before_to = before.len() - suffix;
        let after_to = after.len() - suffix;
        Some(Self {
            from: prefix,
            to: before_to,
            insert_len: after_to - prefix,
        })
    }

    fn map_pos(self, pos: usize, assoc: Assoc) -> usize {
        if pos < self.from {
            return pos;
        }
        if pos > self.to {
            return pos
                .saturating_add(self.insert_len)
                .saturating_sub(self.to - self.from);
        }
        match assoc {
            Assoc::Before => self.from,
            Assoc::After => self.from + self.insert_len,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SliderDecoration {
    pub(crate) id: String,
    pub(crate) range: SourceRange,
    pub(crate) index: usize,
    pub(crate) value: Option<String>,
    pub(crate) min: Option<f64>,
    pub(crate) max: Option<f64>,
    pub(crate) step: Option<f64>,
}

impl SliderDecoration {
    fn placement(&self) -> usize {
        self.range.from
    }

    fn map(&mut self, change: TextChange) {
        self.range = self.range.mapped(change);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WidgetDecoration {
    pub(crate) widget_type: String,
    pub(crate) id: String,
    pub(crate) range: SourceRange,
    pub(crate) index: usize,
    pub(crate) options: BTreeMap<String, rudel_lang::WidgetOption>,
}

impl WidgetDecoration {
    pub(crate) fn placement(&self) -> usize {
        if self.range.to > self.range.from {
            self.range.to
        } else {
            self.range.from
        }
    }

    fn map(&mut self, change: TextChange) {
        self.range = self.range.mapped(change);
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct EditorDecorationState {
    sliders: Vec<SliderDecoration>,
    widgets: Vec<WidgetDecoration>,
    mini_locations: Vec<SourceRange>,
    flash_ranges: Vec<SourceRange>,
    changes_since_eval: Vec<TextChange>,
}

impl EditorDecorationState {
    pub(crate) fn replace_all(&mut self, meta: &rudel_lang::EvalMeta) {
        self.sliders = sliders_from_meta(meta);
        self.widgets = widgets_from_meta(meta);
        self.mini_locations = ranges_from_tuples(&meta.mini_locations);
        self.flash_ranges.clear();
        self.changes_since_eval.clear();
    }

    #[allow(dead_code)]
    pub(crate) fn replace_range(&mut self, meta: &rudel_lang::EvalMeta, range: SourceRange) {
        let mut sliders: Vec<_> = self
            .sliders
            .iter()
            .filter(|slider| outside_replaced_range(slider.placement(), range))
            .cloned()
            .chain(sliders_from_meta(meta))
            .collect();
        dedupe_sliders_for_range_update(&mut sliders);
        sliders.sort_by_key(|slider| slider.range.from);
        self.sliders = sliders;

        let mut widgets: Vec<_> = self
            .widgets
            .iter()
            .filter(|widget| outside_replaced_range(widget.placement(), range))
            .cloned()
            .chain(widgets_from_meta(meta))
            .collect();
        dedupe_widgets(&mut widgets);
        widgets.sort_by_key(|widget| widget.placement());
        self.widgets = widgets;

        let mut mini_locations: Vec<_> = self
            .mini_locations
            .iter()
            .copied()
            .filter(|location| outside_replaced_range(location.from, range))
            .chain(ranges_from_tuples(&meta.mini_locations))
            .collect();
        dedupe_ranges(&mut mini_locations);
        mini_locations.sort_by_key(|location| location.from);
        self.mini_locations = mini_locations;
    }

    pub(crate) fn map_change(&mut self, change: TextChange) {
        for slider in &mut self.sliders {
            slider.map(change);
        }
        for widget in &mut self.widgets {
            widget.map(change);
        }
        for range in &mut self.mini_locations {
            *range = range.mapped(change);
        }
        for range in &mut self.flash_ranges {
            *range = range.mapped(change);
        }
        self.changes_since_eval.push(change);
    }

    pub(crate) fn set_flash_ranges_from_eval(&mut self, ranges: &[(usize, usize)]) {
        self.flash_ranges = ranges
            .iter()
            .copied()
            .map(SourceRange::from)
            .map(|range| self.map_eval_range_to_current(range))
            .filter(|range| range.from < range.to)
            .collect();
        dedupe_ranges(&mut self.flash_ranges);
        self.flash_ranges.sort_by_key(|range| range.from);
    }

    pub(crate) fn flash_ranges(&self) -> Vec<(usize, usize)> {
        self.flash_ranges
            .iter()
            .map(|range| (range.from, range.to))
            .collect()
    }

    #[allow(dead_code)]
    pub(crate) fn sliders(&self) -> &[SliderDecoration] {
        &self.sliders
    }

    pub(crate) fn set_slider_literal(&mut self, id: &str, insert: String) -> bool {
        let Some(slider) = self.sliders.iter_mut().find(|slider| slider.id == id) else {
            return false;
        };
        slider.value = Some(insert);
        true
    }

    #[allow(dead_code)]
    pub(crate) fn widgets(&self) -> &[WidgetDecoration] {
        &self.widgets
    }

    #[allow(dead_code)]
    pub(crate) fn mini_locations(&self) -> &[SourceRange] {
        &self.mini_locations
    }

    fn map_eval_range_to_current(&self, mut range: SourceRange) -> SourceRange {
        for change in &self.changes_since_eval {
            range = range.mapped(*change);
        }
        range
    }
}

fn common_prefix_bytes(a: &str, b: &str) -> usize {
    let mut prefix = 0;
    for ((_, ac), (_, bc)) in a.char_indices().zip(b.char_indices()) {
        if ac != bc {
            break;
        }
        prefix += ac.len_utf8();
    }
    prefix
}

fn common_suffix_bytes(a: &str, b: &str) -> usize {
    let mut suffix = 0;
    for ((_, ac), (_, bc)) in a.char_indices().rev().zip(b.char_indices().rev()) {
        if ac != bc {
            break;
        }
        let len = ac.len_utf8();
        if suffix + len > a.len() || suffix + len > b.len() {
            break;
        }
        suffix += len;
    }
    suffix
}

fn sliders_from_meta(meta: &rudel_lang::EvalMeta) -> Vec<SliderDecoration> {
    let mut sliders: Vec<_> = meta
        .widgets
        .iter()
        .filter(|widget| widget.widget_type == "slider")
        .map(|widget| SliderDecoration {
            id: widget.id.clone(),
            range: SourceRange::new(widget.from, widget.to),
            index: widget.index,
            value: widget.value.clone(),
            min: widget.min,
            max: widget.max,
            step: widget.step,
        })
        .collect();
    dedupe_sliders_for_full_update(&mut sliders);
    sliders.sort_by_key(|slider| slider.range.from);
    sliders
}

fn widgets_from_meta(meta: &rudel_lang::EvalMeta) -> Vec<WidgetDecoration> {
    let mut widgets: Vec<_> = meta
        .widgets
        .iter()
        .filter(|widget| widget.widget_type != "slider")
        .map(|widget| WidgetDecoration {
            widget_type: widget.widget_type.clone(),
            id: widget.id.clone(),
            range: SourceRange::new(widget.from, widget.to),
            index: widget.index,
            options: widget.options.clone(),
        })
        .collect();
    dedupe_widgets(&mut widgets);
    widgets.sort_by_key(|widget| widget.placement());
    widgets
}

fn ranges_from_tuples(ranges: &[(usize, usize)]) -> Vec<SourceRange> {
    let mut ranges: Vec<_> = ranges.iter().copied().map(SourceRange::from).collect();
    dedupe_ranges(&mut ranges);
    ranges.sort_by_key(|range| range.from);
    ranges
}

fn outside_replaced_range(position: usize, range: SourceRange) -> bool {
    position < range.from || position > range.to
}

fn dedupe_sliders_for_full_update(sliders: &mut Vec<SliderDecoration>) {
    let mut seen = HashSet::new();
    sliders.retain(|slider| seen.insert((slider.range.from, slider.range.to)));
}

fn dedupe_sliders_for_range_update(sliders: &mut Vec<SliderDecoration>) {
    let mut seen = HashSet::new();
    sliders.retain(|slider| seen.insert(("slider".to_string(), slider.id.clone())));
}

fn dedupe_widgets(widgets: &mut Vec<WidgetDecoration>) {
    let mut seen = HashSet::new();
    widgets.retain(|widget| seen.insert((widget.widget_type.clone(), widget.id.clone())));
}

fn dedupe_ranges(ranges: &mut Vec<SourceRange>) {
    let mut seen = HashSet::new();
    ranges.retain(|range| seen.insert((range.from, range.to)));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widget(widget_type: &str, id: &str, from: usize, to: usize) -> rudel_lang::WidgetConfig {
        rudel_lang::WidgetConfig {
            widget_type: widget_type.to_string(),
            id: id.to_string(),
            from,
            to,
            options: BTreeMap::new(),
            value: (widget_type == "slider").then(|| "0.5".to_string()),
            min: (widget_type == "slider").then_some(0.0),
            max: (widget_type == "slider").then_some(1.0),
            ..Default::default()
        }
    }

    fn meta(
        widgets: Vec<rudel_lang::WidgetConfig>,
        mini: Vec<(usize, usize)>,
    ) -> rudel_lang::EvalMeta {
        rudel_lang::EvalMeta {
            widgets,
            mini_locations: mini,
            ..Default::default()
        }
    }

    #[test]
    fn text_change_detects_one_replacement_in_byte_offsets() {
        let change = TextChange::from_texts("s(\"bd\")", "xxs(\"hh\")").unwrap();
        assert_eq!(
            change,
            TextChange {
                from: 0,
                to: 5,
                insert_len: 7
            }
        );

        let change = TextChange::from_texts("åbd", "åxxbd").unwrap();
        assert_eq!(
            change,
            TextChange {
                from: "å".len(),
                to: "å".len(),
                insert_len: 2
            }
        );
    }

    #[test]
    fn maps_widget_source_and_flash_ranges_across_edits() {
        let mut state = EditorDecorationState::default();
        state.replace_all(&meta(
            vec![
                widget("slider", "3:6", 3, 6),
                widget("_spiral", "spiral", 10, 20),
            ],
            vec![(7, 9)],
        ));
        state.set_flash_ranges_from_eval(&[(3, 6), (10, 12)]);
        state.map_change(TextChange {
            from: 0,
            to: 0,
            insert_len: 2,
        });

        assert_eq!(state.sliders()[0].range, SourceRange::new(5, 8));
        assert_eq!(state.widgets()[0].range, SourceRange::new(12, 22));
        assert_eq!(state.mini_locations(), &[SourceRange::new(9, 11)]);
        assert_eq!(state.flash_ranges(), vec![(5, 8), (12, 14)]);
    }

    #[test]
    fn maps_fresh_flash_ranges_from_eval_source_to_current_text() {
        let mut state = EditorDecorationState::default();
        state.replace_all(&meta(Vec::new(), Vec::new()));
        state.map_change(TextChange {
            from: 0,
            to: 0,
            insert_len: 2,
        });

        state.set_flash_ranges_from_eval(&[(3, 6)]);

        assert_eq!(state.flash_ranges(), vec![(5, 8)]);
    }

    #[test]
    fn range_update_preserves_decorations_outside_the_evaluated_range() {
        let mut state = EditorDecorationState::default();
        state.replace_all(&meta(
            vec![
                widget("slider", "2:3", 2, 3),
                widget("slider", "7:8", 7, 8),
                widget("_spiral", "outside", 10, 20),
                widget("_scope", "inside", 8, 12),
            ],
            vec![(1, 2), (7, 9)],
        ));

        state.replace_range(
            &meta(
                vec![
                    widget("slider", "6:7", 6, 7),
                    widget("_pitchwheel", "new", 9, 14),
                ],
                vec![(6, 8)],
            ),
            SourceRange::new(5, 15),
        );

        assert_eq!(
            state
                .sliders()
                .iter()
                .map(|slider| slider.id.as_str())
                .collect::<Vec<_>>(),
            vec!["2:3", "6:7"]
        );
        assert_eq!(
            state
                .widgets()
                .iter()
                .map(|widget| (widget.widget_type.as_str(), widget.id.as_str()))
                .collect::<Vec<_>>(),
            vec![("_pitchwheel", "new"), ("_spiral", "outside")]
        );
        assert_eq!(
            state.mini_locations(),
            &[SourceRange::new(1, 2), SourceRange::new(6, 8)]
        );
    }
}
