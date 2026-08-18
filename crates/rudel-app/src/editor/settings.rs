use eframe::egui;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditorTheme {
    StrudelDark,
    Light,
    /// Ported from `codemirror/themes/dracula.mjs` (Michael Kaminsky's port of
    /// Zeno Rocha's scheme): its `settings` export drives the draw theme and its
    /// `styles` list the syntax palette.
    Dracula,
}

impl EditorTheme {
    pub(crate) const ALL: [EditorTheme; 3] = [
        EditorTheme::StrudelDark,
        EditorTheme::Light,
        EditorTheme::Dracula,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            EditorTheme::StrudelDark => "strudelTheme",
            EditorTheme::Light => "whitescreen",
            EditorTheme::Dracula => "dracula",
        }
    }

    pub(crate) fn draw_theme(self) -> DrawTheme {
        match self {
            EditorTheme::StrudelDark => DrawTheme {
                background: egui::Color32::from_rgb(0x22, 0x22, 0x22),
                line_background: egui::Color32::from_rgba_unmultiplied(0x22, 0x22, 0x22, 0x99),
                foreground: egui::Color32::WHITE,
                muted: egui::Color32::from_rgba_unmultiplied(0x8a, 0x91, 0x99, 0x66),
                caret: egui::Color32::from_rgb(0xff, 0xcc, 0x00),
                selection: egui::Color32::from_rgba_unmultiplied(128, 203, 196, 128),
                selection_match: egui::Color32::from_rgba_unmultiplied(0x03, 0x6d, 0xd6, 0x26),
                line_highlight: egui::Color32::from_rgba_unmultiplied(0, 0, 0, 0x50),
                gutter_background: egui::Color32::TRANSPARENT,
                gutter_foreground: egui::Color32::from_rgba_unmultiplied(0x8a, 0x91, 0x99, 0x66),
                light: false,
            },
            EditorTheme::Light => DrawTheme {
                background: egui::Color32::WHITE,
                line_background: egui::Color32::from_rgba_unmultiplied(0xff, 0xff, 0xff, 0x50),
                foreground: egui::Color32::BLACK,
                muted: egui::Color32::from_rgba_unmultiplied(0, 0, 0, 0x50),
                caret: egui::Color32::BLACK,
                selection: egui::Color32::from_rgba_unmultiplied(128, 203, 196, 128),
                selection_match: egui::Color32::from_rgba_unmultiplied(0xff, 0xff, 0xff, 0x26),
                line_highlight: egui::Color32::from_rgba_unmultiplied(0xcc, 0xcc, 0xcc, 0x50),
                gutter_background: egui::Color32::TRANSPARENT,
                gutter_foreground: egui::Color32::BLACK,
                light: true,
            },
            EditorTheme::Dracula => DrawTheme {
                background: egui::Color32::from_rgb(0x28, 0x2a, 0x36),
                line_background: egui::Color32::from_rgba_unmultiplied(0x28, 0x2a, 0x36, 0x99),
                foreground: egui::Color32::from_rgb(0xf8, 0xf8, 0xf2),
                muted: egui::Color32::from_rgba_unmultiplied(0xf8, 0xf8, 0xf2, 0x50),
                caret: egui::Color32::from_rgb(0xf8, 0xf8, 0xf0),
                selection: egui::Color32::from_rgba_unmultiplied(255, 255, 255, 26),
                selection_match: egui::Color32::from_rgba_unmultiplied(255, 255, 255, 51),
                line_highlight: egui::Color32::from_rgba_unmultiplied(255, 255, 255, 26),
                gutter_background: egui::Color32::from_rgb(0x28, 0x2a, 0x36),
                gutter_foreground: egui::Color32::from_rgb(0x62, 0x72, 0xa4),
                light: false,
            },
        }
    }

    pub(crate) fn palette(self) -> EditorPalette {
        let draw = self.draw_theme();
        match self {
            EditorTheme::StrudelDark => EditorPalette {
                foreground: draw.foreground,
                keyword: egui::Color32::from_rgb(0xc7, 0x92, 0xea),
                method: egui::Color32::from_rgb(0xc7, 0x92, 0xea),
                string: egui::Color32::from_rgb(0xc3, 0xe8, 0x8d),
                number: egui::Color32::from_rgb(0xc3, 0xe8, 0x8d),
                comment: egui::Color32::from_rgb(0x7d, 0x87, 0x99),
                mini_op: egui::Color32::from_rgb(0x82, 0xaa, 0xff),
                mini_word: egui::Color32::from_rgb(0xc3, 0xe8, 0x8d),
                flash: egui::Color32::from_rgba_unmultiplied(0xff, 0xcc, 0x00, 0x33),
                bracket_flash: draw.selection_match,
                active_line: draw.line_highlight,
                line_number: draw.gutter_foreground,
                line_number_active: draw.foreground,
            },
            EditorTheme::Light => EditorPalette {
                foreground: draw.foreground,
                keyword: draw.foreground,
                method: draw.foreground,
                string: draw.foreground,
                number: draw.foreground,
                comment: draw.muted,
                mini_op: draw.foreground,
                mini_word: draw.foreground,
                flash: egui::Color32::from_rgba_unmultiplied(0xff, 0xcc, 0x00, 0x33),
                bracket_flash: draw.line_highlight,
                active_line: draw.line_highlight,
                line_number: draw.gutter_foreground,
                line_number_active: draw.foreground,
            },
            // dracula.mjs `styles`: comments/gutter `#6272a4`, strings `#f1fa8c`,
            // numbers/atoms `#bd93f9`, keywords `#ff79c6`, property/function
            // names (Rudel's method tokens) `#50fa7b`.
            EditorTheme::Dracula => EditorPalette {
                foreground: draw.foreground,
                keyword: egui::Color32::from_rgb(0xff, 0x79, 0xc6),
                method: egui::Color32::from_rgb(0x50, 0xfa, 0x7b),
                string: egui::Color32::from_rgb(0xf1, 0xfa, 0x8c),
                number: egui::Color32::from_rgb(0xbd, 0x93, 0xf9),
                comment: egui::Color32::from_rgb(0x62, 0x72, 0xa4),
                mini_op: egui::Color32::from_rgb(0xff, 0x79, 0xc6),
                mini_word: egui::Color32::from_rgb(0xf1, 0xfa, 0x8c),
                flash: egui::Color32::from_rgba_unmultiplied(0xbd, 0x93, 0xf9, 0x44),
                bracket_flash: draw.selection_match,
                active_line: draw.line_highlight,
                line_number: draw.gutter_foreground,
                line_number_active: draw.foreground,
            },
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
            theme: EditorTheme::StrudelDark,
            font_family: EditorFontFamily::Monospace,
            font_size: 18.0,
        }
    }
}

impl EditorSettings {
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
        assert_eq!(settings.theme, EditorTheme::StrudelDark);
        assert_eq!(settings.font_size, 18.0);
    }

    #[test]
    fn draw_theme_matches_strudel_theme_settings() {
        let dark = EditorTheme::StrudelDark.draw_theme();
        assert_eq!(dark.background, egui::Color32::from_rgb(0x22, 0x22, 0x22));
        assert_eq!(dark.foreground, egui::Color32::WHITE);
        assert_eq!(
            dark.gutter_foreground,
            egui::Color32::from_rgba_unmultiplied(0x8a, 0x91, 0x99, 0x66)
        );
        assert!(!dark.light);

        let light = EditorTheme::Light.draw_theme();
        assert_eq!(light.background, egui::Color32::WHITE);
        assert_eq!(light.foreground, egui::Color32::BLACK);
        assert_eq!(EditorTheme::Light.label(), "whitescreen");
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
