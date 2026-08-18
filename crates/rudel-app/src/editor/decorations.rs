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
    flash_ranges: Vec<(SourceRange, Option<u32>)>,
    changes_since_eval: Vec<TextChange>,
}

/// A source span to flash, with the colour its event asked for (`markcss` /
/// `color`, packed `0xRRGGBBAA`) or `None` for the theme's default flash.
pub(crate) type FlashSpan = (usize, usize, Option<u32>);

impl EditorDecorationState {
    pub(crate) fn replace_all(&mut self, meta: &rudel_lang::EvalMeta) {
        self.sliders = sliders_from_meta(meta);
        self.widgets = widgets_from_meta(meta);
        self.flash_ranges.clear();
        self.changes_since_eval.clear();
    }

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
    }

    pub(crate) fn map_change(&mut self, change: TextChange) {
        for slider in &mut self.sliders {
            slider.map(change);
        }
        for widget in &mut self.widgets {
            widget.map(change);
        }
        for (range, _) in &mut self.flash_ranges {
            *range = range.mapped(change);
        }
        self.changes_since_eval.push(change);
    }

    pub(crate) fn set_flash_ranges_from_eval(&mut self, ranges: &[FlashSpan]) {
        self.flash_ranges = ranges
            .iter()
            .map(|&(from, to, color)| (self.map_eval_range_to_current((from, to).into()), color))
            .filter(|(range, _)| range.from < range.to)
            .collect();
        dedupe_ranges(&mut self.flash_ranges);
        self.flash_ranges.sort_by_key(|(range, _)| range.from);
    }

    pub(crate) fn flash_ranges(&self) -> Vec<FlashSpan> {
        self.flash_ranges
            .iter()
            .map(|&(range, color)| (range.from, range.to, color))
            .collect()
    }

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

    pub(crate) fn widgets(&self) -> &[WidgetDecoration] {
        &self.widgets
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
        // No bounds check: every character counted is one of `a`'s own, and
        // `bc == ac` so it is one of `b`'s too, which is why the sum can never
        // pass either length.
        suffix += ac.len_utf8();
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

fn dedupe_ranges(ranges: &mut Vec<(SourceRange, Option<u32>)>) {
    let mut seen = HashSet::new();
    ranges.retain(|(range, _)| seen.insert((range.from, range.to)));
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

    fn meta(widgets: Vec<rudel_lang::WidgetConfig>) -> rudel_lang::EvalMeta {
        rudel_lang::EvalMeta { widgets }
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
        state.replace_all(&meta(vec![
            widget("slider", "3:6", 3, 6),
            widget("_spiral", "spiral", 10, 20),
        ]));
        state.set_flash_ranges_from_eval(&[(3, 6, None), (10, 12, None)]);
        state.map_change(TextChange {
            from: 0,
            to: 0,
            insert_len: 2,
        });

        assert_eq!(state.sliders()[0].range, SourceRange::new(5, 8));
        assert_eq!(state.widgets()[0].range, SourceRange::new(12, 22));
        assert_eq!(state.flash_ranges(), vec![(5, 8, None), (12, 14, None)]);
    }

    #[test]
    fn maps_fresh_flash_ranges_from_eval_source_to_current_text() {
        let mut state = EditorDecorationState::default();
        state.replace_all(&meta(Vec::new()));
        state.map_change(TextChange {
            from: 0,
            to: 0,
            insert_len: 2,
        });

        state.set_flash_ranges_from_eval(&[(3, 6, None)]);

        assert_eq!(state.flash_ranges(), vec![(5, 8, None)]);
    }

    #[test]
    fn range_update_preserves_decorations_outside_the_evaluated_range() {
        let mut state = EditorDecorationState::default();
        state.replace_all(&meta(vec![
            widget("slider", "2:3", 2, 3),
            widget("slider", "7:8", 7, 8),
            widget("_spiral", "outside", 10, 20),
            widget("_scope", "inside", 8, 12),
        ]));

        state.replace_range(
            &meta(vec![
                widget("slider", "6:7", 6, 7),
                widget("_pitchwheel", "new", 9, 14),
            ]),
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
    }
    fn slider(id: &str, from: usize, to: usize) -> SliderDecoration {
        SliderDecoration {
            id: id.to_string(),
            range: SourceRange::new(from, to),
            index: 0,
            value: None,
            min: None,
            max: None,
            step: None,
        }
    }

    #[test]
    fn a_position_moves_with_the_text_edited_before_it() {
        // Replacing bytes 2..5 with five bytes: everything after shifts by the
        // difference, everything before stays put.
        let change = TextChange {
            from: 2,
            to: 5,
            insert_len: 5,
        };
        assert_eq!(change.map_pos(1, Assoc::Before), 1, "before the edit");
        assert_eq!(change.map_pos(6, Assoc::Before), 8, "after it, shifted +2");
        assert_eq!(change.map_pos(9, Assoc::After), 11);

        // Inside the replaced span there is no corresponding position, so it
        // collapses to one end or the other — which end is the caller's choice,
        // and is what keeps a range from inverting.
        assert_eq!(change.map_pos(3, Assoc::Before), 2);
        assert_eq!(change.map_pos(3, Assoc::After), 7);
        // The span's own ends count as inside it.
        assert_eq!(change.map_pos(2, Assoc::After), 7, "at the start");
        assert_eq!(change.map_pos(5, Assoc::Before), 2, "at the end");
    }

    #[test]
    fn a_deletion_pulls_later_positions_back() {
        // Three bytes replaced by none: the shift is negative.
        let change = TextChange {
            from: 2,
            to: 5,
            insert_len: 0,
        };
        assert_eq!(change.map_pos(8, Assoc::Before), 5);
        assert_eq!(change.map_pos(3, Assoc::After), 2, "nothing left to sit in");
    }

    #[test]
    fn the_common_suffix_is_counted_in_whole_characters() {
        assert_eq!(common_suffix_bytes("ab", "ab"), 2, "identical");
        assert_eq!(
            common_suffix_bytes("xab", "ab"),
            2,
            "the shorter one ends it"
        );
        assert_eq!(common_suffix_bytes("ab", "cb"), 1);
        assert_eq!(common_suffix_bytes("ab", "cd"), 0, "nothing in common");
        assert_eq!(common_suffix_bytes("", "ab"), 0);
        // Multi-byte: the count is bytes, but a character is never split.
        assert_eq!(common_suffix_bytes("aé", "bé"), 2);
        assert_eq!(common_suffix_bytes("é", "é"), 2);
    }

    #[test]
    fn a_position_outside_the_replaced_range_is_the_one_past_either_end() {
        let range = SourceRange::new(3, 7);
        assert!(outside_replaced_range(2, range));
        assert!(outside_replaced_range(8, range));
        assert!(!outside_replaced_range(3, range), "the ends are inside");
        assert!(!outside_replaced_range(7, range));
        assert!(!outside_replaced_range(5, range));
    }

    #[test]
    fn a_widget_is_placed_at_the_end_of_its_range_unless_it_has_none() {
        let widget = |from, to| WidgetDecoration {
            widget_type: "slider".to_string(),
            id: "a".to_string(),
            range: SourceRange::new(from, to),
            index: 0,
            options: BTreeMap::new(),
        };
        assert_eq!(widget(2, 7).placement(), 7);
        // An empty range has no end to sit after, so it places at its start.
        assert_eq!(widget(4, 4).placement(), 4);
    }

    #[test]
    fn duplicate_decorations_are_dropped_by_the_key_each_update_uses() {
        // A full update keys on the source span, since ids are reassigned...
        let mut sliders = vec![slider("a", 0, 4), slider("b", 0, 4), slider("c", 5, 9)];
        dedupe_sliders_for_full_update(&mut sliders);
        assert_eq!(
            sliders.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            ["a", "c"],
            "first of each span wins"
        );

        // ...and a range update keys on the id, since the spans have moved.
        let mut sliders = vec![slider("a", 0, 4), slider("a", 5, 9), slider("b", 5, 9)];
        dedupe_sliders_for_range_update(&mut sliders);
        assert_eq!(
            sliders.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );

        // Widgets key on type *and* id, so two kinds may share a name.
        let mut widgets = vec![
            WidgetDecoration {
                widget_type: "slider".to_string(),
                id: "a".to_string(),
                range: SourceRange::new(0, 4),
                index: 0,
                options: BTreeMap::new(),
            },
            WidgetDecoration {
                widget_type: "slider".to_string(),
                id: "a".to_string(),
                range: SourceRange::new(5, 9),
                index: 1,
                options: BTreeMap::new(),
            },
            WidgetDecoration {
                widget_type: "spiral".to_string(),
                id: "a".to_string(),
                range: SourceRange::new(5, 9),
                index: 2,
                options: BTreeMap::new(),
            },
        ];
        dedupe_widgets(&mut widgets);
        assert_eq!(
            widgets.iter().map(|w| w.index).collect::<Vec<_>>(),
            [0, 2],
            "the second slider goes, the spiral stays"
        );

        let mut ranges = vec![
            (SourceRange::new(0, 4), None),
            (SourceRange::new(0, 4), Some(1)),
            (SourceRange::new(5, 9), None),
        ];
        dedupe_ranges(&mut ranges);
        assert_eq!(ranges.len(), 2, "one per span, whatever the colour");
    }
    #[test]
    fn an_empty_flash_range_is_dropped_rather_than_painted() {
        // A zero-width span has nothing to highlight, and a text edit can
        // collapse one to nothing — mapping both ends of a span that sat
        // inside the replaced text lands them on the same position.
        let mut state = EditorDecorationState::default();
        state.set_flash_ranges_from_eval(&[(0, 4, None), (6, 6, None), (8, 12, Some(1))]);
        assert_eq!(
            state.flash_ranges(),
            vec![(0, 4, None), (8, 12, Some(1))],
            "the empty one in the middle is not kept"
        );
    }
}
