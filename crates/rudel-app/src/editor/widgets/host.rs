use super::size::surface_size;
use crate::editor::decorations::WidgetDecoration;
use eframe::egui;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct WidgetHostSync {
    pub(crate) created: Vec<String>,
    pub(crate) removed: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct WidgetSurface {
    pub(super) serial: u64,
    pub(super) size: egui::Vec2,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct WidgetHostState {
    surfaces: HashMap<WidgetKey, WidgetSurface>,
    next_serial: u64,
    /// The widget shown in its own window, if any (double-click to pop out).
    popped: Option<WidgetKey>,
}

impl WidgetHostState {
    pub(crate) fn sync(&mut self, widgets: &[WidgetDecoration]) -> WidgetHostSync {
        let mut active = HashSet::new();
        let mut created = Vec::new();
        for widget in widgets {
            let key = WidgetKey::from(widget);
            let size = surface_size(widget);
            active.insert(key.clone());
            if let Some(surface) = self.surfaces.get_mut(&key) {
                surface.size = size;
            } else {
                let serial = self.next_serial;
                self.next_serial += 1;
                self.surfaces
                    .insert(key.clone(), WidgetSurface { serial, size });
                created.push(widget.id.clone());
            }
        }

        let mut removed = Vec::new();
        self.surfaces.retain(|key, _| {
            let keep = active.contains(key);
            if !keep {
                removed.push(key.id.clone());
            }
            keep
        });
        removed.sort();
        removed.dedup();
        if self
            .popped
            .as_ref()
            .is_some_and(|key| !active.contains(key))
        {
            self.popped = None;
        }
        WidgetHostSync { created, removed }
    }

    pub(super) fn surface(&self, widget: &WidgetDecoration) -> Option<&WidgetSurface> {
        self.surfaces.get(&WidgetKey::from(widget))
    }

    /// Pop `widget` out into its own window, or dock it if it already is.
    pub(super) fn toggle_popped(&mut self, widget: &WidgetDecoration) {
        let key = WidgetKey::from(widget);
        self.popped = (self.popped.as_ref() != Some(&key)).then_some(key);
    }

    pub(super) fn dock(&mut self) {
        self.popped = None;
    }

    /// The popped-out widget and its surface.
    pub(super) fn popped<'w>(
        &self,
        widgets: &'w [WidgetDecoration],
    ) -> Option<(&'w WidgetDecoration, &WidgetSurface)> {
        let key = self.popped.as_ref()?;
        let widget = widgets.iter().find(|w| WidgetKey::from(*w) == *key)?;
        Some((widget, self.surfaces.get(key)?))
    }

    #[cfg(test)]
    pub(super) fn surface_serial(&self, widget_type: &str, id: &str) -> Option<u64> {
        self.surfaces
            .get(&WidgetKey {
                widget_type: widget_type.to_string(),
                id: id.to_string(),
            })
            .map(|surface| surface.serial)
    }

    #[cfg(test)]
    pub(super) fn surface_count(&self) -> usize {
        self.surfaces.len()
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct WidgetKey {
    widget_type: String,
    id: String,
}

impl From<&WidgetDecoration> for WidgetKey {
    fn from(widget: &WidgetDecoration) -> Self {
        Self {
            widget_type: widget.widget_type.clone(),
            id: widget.id.clone(),
        }
    }
}
