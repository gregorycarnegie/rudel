use super::fonts::{self, WEB_FONTS, WebFont};
use super::keymap::Keymap;
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
    /// One of Strudel's web fonts, by index into [`WEB_FONTS`].
    Web(usize),
}

impl EditorFontFamily {
    pub(crate) fn all() -> impl Iterator<Item = EditorFontFamily> {
        [EditorFontFamily::Monospace, EditorFontFamily::Proportional]
            .into_iter()
            .chain((0..WEB_FONTS.len()).map(EditorFontFamily::Web))
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            EditorFontFamily::Monospace => "monospace",
            EditorFontFamily::Proportional => "proportional",
            EditorFontFamily::Web(i) => WEB_FONTS[i].name,
        }
    }

    /// The family a saved setting or Strudel's `fontFamily` names. `Hack` is
    /// egui's own monospace and `Courier` the browser's; CSS generic names map
    /// to their egui families.
    pub(crate) fn named(name: &str) -> Option<Self> {
        match name.trim() {
            "Hack" | "Courier" => Some(EditorFontFamily::Monospace),
            "sans-serif" | "serif" => Some(EditorFontFamily::Proportional),
            name => Self::all().find(|f| f.label().eq_ignore_ascii_case(name)),
        }
    }

    pub(crate) fn web_font(self) -> Option<&'static WebFont> {
        match self {
            EditorFontFamily::Web(i) => WEB_FONTS.get(i),
            _ => None,
        }
    }

    /// The font at `size`. A web font that has not arrived yet draws in
    /// monospace, as a browser shows its fallback while a font loads.
    fn font_id(self, size: f32) -> egui::FontId {
        match self {
            EditorFontFamily::Monospace => egui::FontId::monospace(size),
            EditorFontFamily::Proportional => egui::FontId::proportional(size),
            EditorFontFamily::Web(i) => match WEB_FONTS.get(i) {
                Some(font) if fonts::is_installed(font) => egui::FontId::new(
                    size * font.size_adjust,
                    egui::FontFamily::Name(font.name.into()),
                ),
                _ => egui::FontId::monospace(size),
            },
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
    /// Strudel's `keybindings` setting.
    pub(crate) keymap: Keymap,
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
            keymap: Keymap::Codemirror,
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
        lines.push(format!("keymap={}", self.keymap.label()));
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
                    if let Some(family) = EditorFontFamily::named(value) {
                        settings.font_family = family;
                    }
                }
                "keymap" => {
                    if let Some(keymap) = Keymap::named(value) {
                        settings.keymap = keymap;
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
        self.font_family.font_id(self.font_size)
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
        settings.keymap = Keymap::Helix;
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

    #[test]
    fn strudel_font_names_are_read_and_saved() {
        let family = EditorFontFamily::named("PressStart").expect("a Strudel font");
        assert_eq!(family.label(), "PressStart");
        assert_eq!(EditorFontFamily::named("pressstart"), Some(family));
        assert_eq!(
            EditorFontFamily::named("Hack"),
            Some(EditorFontFamily::Monospace)
        );
        assert_eq!(EditorFontFamily::named("NoSuchFont"), None);
        let settings = EditorSettings {
            font_family: family,
            ..Default::default()
        };
        assert_eq!(EditorSettings::from_saved(&settings.to_saved()), settings);
        // Not fetched in a test, so it draws in monospace meanwhile.
        assert_eq!(
            settings.font_id(),
            egui::FontId::monospace(settings.font_size)
        );
    }

    #[test]
    fn an_arrived_web_font_draws_at_its_size_adjust() {
        let ctx = egui::Context::default();
        let font = &WEB_FONTS[0];
        fonts::install_for_test(&ctx, font, epaint_default_fonts::UBUNTU_LIGHT);
        fonts::install(&ctx, None);
        let settings = EditorSettings {
            font_family: EditorFontFamily::Web(0),
            font_size: 10.0,
            ..Default::default()
        };
        assert_eq!(
            settings.font_id(),
            egui::FontId::new(
                10.0 * font.size_adjust,
                egui::FontFamily::Name(font.name.into())
            )
        );
    }
}
