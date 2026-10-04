use super::themes::{ThemeData, all as themes};
use eframe::egui;

/// The editor's theme, by its place in the list of themes ([`super::themes`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EditorTheme(usize);

impl Default for EditorTheme {
    fn default() -> Self {
        Self::named("strudelTheme").expect("Strudel's own theme is in the table")
    }
}

fn rgba([r, g, b, a]: [u8; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(r, g, b, a)
}

impl EditorTheme {
    /// Every theme, by name.
    pub(crate) fn all() -> impl Iterator<Item = EditorTheme> {
        (0..themes().len()).map(EditorTheme)
    }

    /// The theme `theme("githubDark")` names.
    pub(crate) fn named(name: &str) -> Option<EditorTheme> {
        themes()
            .iter()
            .position(|t| t.name == name)
            .map(EditorTheme)
    }

    fn data(self) -> &'static ThemeData {
        &themes()[self.0]
    }

    pub(crate) fn label(self) -> &'static str {
        &self.data().name
    }

    pub(crate) fn draw_theme(self) -> DrawTheme {
        let t = self.data();
        DrawTheme {
            background: rgba(t.background),
            line_background: rgba(t.line_background),
            foreground: rgba(t.foreground),
            muted: rgba(t.muted),
            caret: rgba(t.caret),
            selection: rgba(t.selection),
            selection_match: rgba(t.selection_match),
            line_highlight: rgba(t.line_highlight),
            gutter_background: rgba(t.gutter_background),
            gutter_foreground: rgba(t.gutter_foreground),
            light: t.light,
        }
    }

    pub(crate) fn palette(self) -> EditorPalette {
        let t = self.data();
        let draw = self.draw_theme();
        let [r, g, b, _] = t.caret;
        EditorPalette {
            foreground: draw.foreground,
            keyword: rgba(t.keyword),
            method: rgba(t.method),
            string: rgba(t.string),
            number: rgba(t.number),
            comment: rgba(t.comment),
            mini_op: rgba(t.mini_op),
            mini_word: rgba(t.mini_word),
            // The caret's colour, faint: Strudel's own theme's `#ffcc0033`.
            flash: egui::Color32::from_rgba_unmultiplied(r, g, b, 0x33),
            bracket_flash: draw.selection_match,
            active_line: draw.line_highlight,
            line_number: draw.gutter_foreground,
            line_number_active: draw.foreground,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditorFontFamily {
    Monospace,
    Proportional,
}

impl EditorFontFamily {
    pub(crate) const ALL: [EditorFontFamily; 2] =
        [EditorFontFamily::Monospace, EditorFontFamily::Proportional];

    pub(crate) fn label(self) -> &'static str {
        match self {
            EditorFontFamily::Monospace => "monospace",
            EditorFontFamily::Proportional => "proportional",
        }
    }

    fn egui_family(self) -> egui::FontFamily {
        match self {
            EditorFontFamily::Monospace => egui::FontFamily::Monospace,
            EditorFontFamily::Proportional => egui::FontFamily::Proportional,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EditorSettings {
    pub(crate) line_wrapping: bool,
    pub(crate) bracket_matching: bool,
    pub(crate) bracket_closing: bool,
    pub(crate) line_numbers: bool,
    pub(crate) active_line: bool,
    pub(crate) autocomplete: bool,
    pub(crate) pattern_highlighting: bool,
    pub(crate) flash: bool,
    pub(crate) tooltips: bool,
    pub(crate) tab_indentation: bool,
    pub(crate) block_based_eval: bool,
    pub(crate) theme: EditorTheme,
    pub(crate) font_family: EditorFontFamily,
    pub(crate) font_size: f32,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            line_wrapping: false,
            bracket_matching: false,
            bracket_closing: true,
            line_numbers: true,
            active_line: false,
            autocomplete: true,
            pattern_highlighting: true,
            flash: true,
            tooltips: true,
            tab_indentation: false,
            block_based_eval: false,
            theme: EditorTheme::default(),
            font_family: EditorFontFamily::Monospace,
            font_size: 18.0,
        }
    }
}

impl EditorSettings {
    /// The on/off settings by the name they are saved under.
    fn switches(&mut self) -> [(&'static str, &mut bool); 11] {
        [
            ("line_wrapping", &mut self.line_wrapping),
            ("bracket_matching", &mut self.bracket_matching),
            ("bracket_closing", &mut self.bracket_closing),
            ("line_numbers", &mut self.line_numbers),
            ("active_line", &mut self.active_line),
            ("autocomplete", &mut self.autocomplete),
            ("pattern_highlighting", &mut self.pattern_highlighting),
            ("flash", &mut self.flash),
            ("tooltips", &mut self.tooltips),
            ("tab_indentation", &mut self.tab_indentation),
            ("block_based_eval", &mut self.block_based_eval),
        ]
    }

    /// The settings as `key=value` lines, for the app's storage. The theme is
    /// saved by name, so a regenerated theme table cannot shift it.
    pub(crate) fn to_saved(mut self) -> String {
        let mut lines: Vec<String> = self
            .switches()
            .into_iter()
            .map(|(key, on)| format!("{key}={on}"))
            .collect();
        lines.push(format!("theme={}", self.theme.label()));
        lines.push(format!("font_family={}", self.font_family.label()));
        lines.push(format!("font_size={}", self.font_size));
        lines.join("\n")
    }

    /// Settings read back from [`Self::to_saved`]'s text. A key it does not
    /// know, or a value that does not read, keeps its default, so a save from
    /// an older or newer rudel still loads.
    pub(crate) fn from_saved(text: &str) -> Self {
        let mut settings = Self::default();
        for (key, value) in text.lines().filter_map(|line| line.split_once('=')) {
            match key {
                "theme" => {
                    if let Some(theme) = EditorTheme::named(value) {
                        settings.theme = theme;
                    }
                }
                "font_family" => {
                    if let Some(family) = EditorFontFamily::ALL
                        .into_iter()
                        .find(|f| f.label() == value)
                    {
                        settings.font_family = family;
                    }
                }
                "font_size" => {
                    if let Ok(size) = value.parse::<f32>()
                        && size.is_finite()
                    {
                        settings.font_size = size.clamp(6.0, 96.0);
                    }
                }
                _ => {
                    if let Some((_, on)) = settings.switches().into_iter().find(|(k, _)| *k == key)
                        && let Ok(value) = value.parse()
                    {
                        *on = value;
                    }
                }
            }
        }
        settings
    }

    pub(crate) fn font_id(self) -> egui::FontId {
        egui::FontId::new(self.font_size, self.font_family.egui_family())
    }

    pub(crate) fn draw_theme(self) -> DrawTheme {
        self.theme.draw_theme()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DrawTheme {
    pub(crate) background: egui::Color32,
    pub(crate) line_background: egui::Color32,
    pub(crate) foreground: egui::Color32,
    pub(crate) muted: egui::Color32,
    pub(crate) caret: egui::Color32,
    pub(crate) selection: egui::Color32,
    pub(crate) selection_match: egui::Color32,
    pub(crate) line_highlight: egui::Color32,
    pub(crate) gutter_background: egui::Color32,
    pub(crate) gutter_foreground: egui::Color32,
    pub(crate) light: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EditorPalette {
    pub(crate) foreground: egui::Color32,
    pub(crate) keyword: egui::Color32,
    pub(crate) method: egui::Color32,
    pub(crate) string: egui::Color32,
    pub(crate) number: egui::Color32,
    pub(crate) comment: egui::Color32,
    pub(crate) mini_op: egui::Color32,
    pub(crate) mini_word: egui::Color32,
    pub(crate) flash: egui::Color32,
    pub(crate) bracket_flash: egui::Color32,
    pub(crate) active_line: egui::Color32,
    pub(crate) line_number: egui::Color32,
    pub(crate) line_number_active: egui::Color32,
}

pub(crate) fn apply_editor_style(ui: &mut egui::Ui, settings: &EditorSettings) {
    let draw = settings.draw_theme();
    let mut style = (**ui.style()).clone();
    style
        .text_styles
        .insert(egui::TextStyle::Monospace, settings.font_id());
    // Wire the theme's selection and caret colors into egui's visuals. egui's
    // built-in TextEdit recolors every selected glyph to `selection.stroke.color`
    // (it cannot preserve per-token syntax colors under a selection), so we pair
    // Strudel's translucent selection fill with the readable foreground color.
    style.visuals.selection.bg_fill = draw.selection;
    style.visuals.selection.stroke = egui::Stroke::new(1.0, draw.foreground);
    style.visuals.text_cursor.stroke.color = draw.caret;
    ui.set_style(style);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_settings_read_back_and_bad_lines_keep_their_defaults() {
        let mut settings = EditorSettings::default();
        for (_, on) in settings.switches() {
            *on = !*on;
        }
        settings.theme = EditorTheme::named("githubLight").unwrap();
        settings.font_family = EditorFontFamily::Proportional;
        settings.font_size = 13.5;
        assert_eq!(EditorSettings::from_saved(&settings.to_saved()), settings);

        // Unknown keys, bad values and lines without `=` are skipped.
        let read = EditorSettings::from_saved(
            "flash=maybe\ntheme=noSuchTheme\nfont_size=NaN\nfuture_setting=1\ngarbage\nline_numbers=false",
        );
        let expected = EditorSettings {
            line_numbers: false,
            ..EditorSettings::default()
        };
        assert_eq!(read, expected);
        assert_eq!(
            EditorSettings::from_saved("font_size=500").font_size,
            96.0,
            "clamped as the pattern setting is"
        );
    }

    #[test]
    fn editor_settings_default_to_native_strudel_compatible_values() {
        let settings = EditorSettings::default();

        assert!(!settings.line_wrapping);
        assert!(!settings.bracket_matching);
        assert!(settings.bracket_closing);
        assert!(settings.line_numbers);
        assert!(settings.autocomplete);
        assert!(settings.pattern_highlighting);
        assert!(settings.flash);
        assert!(settings.tooltips);
        assert!(!settings.tab_indentation);
        assert_eq!(settings.theme, EditorTheme::default());
        assert_eq!(settings.font_size, 18.0);
    }

    #[test]
    fn draw_theme_matches_strudel_theme_settings() {
        let dark = EditorTheme::default().draw_theme();
        assert_eq!(dark.background, egui::Color32::from_rgb(0x22, 0x22, 0x22));
        assert_eq!(dark.foreground, egui::Color32::WHITE);
        assert_eq!(
            dark.gutter_foreground,
            egui::Color32::from_rgba_unmultiplied(0x8a, 0x91, 0x99, 0x66)
        );
        assert!(!dark.light);

        let light = EditorTheme::named("whitescreen")
            .expect("whitescreen")
            .draw_theme();
        assert_eq!(light.background, egui::Color32::WHITE);
        assert_eq!(light.foreground, egui::Color32::BLACK);
        assert_eq!(
            EditorTheme::named("whitescreen").map(EditorTheme::label),
            Some("whitescreen")
        );
        assert!(light.light);
    }

    #[test]
    fn font_family_labels_and_families_stay_paired() {
        assert_eq!(EditorFontFamily::Monospace.label(), "monospace");
        assert_eq!(EditorFontFamily::Proportional.label(), "proportional");
        let settings = EditorSettings {
            font_size: 17.0,
            font_family: EditorFontFamily::Proportional,
            ..Default::default()
        };
        assert_eq!(
            settings.font_id(),
            egui::FontId::new(17.0, egui::FontFamily::Proportional)
        );
        assert_eq!(
            EditorSettings {
                font_family: EditorFontFamily::Monospace,
                ..settings
            }
            .font_id(),
            egui::FontId::new(17.0, egui::FontFamily::Monospace)
        );
    }
}
