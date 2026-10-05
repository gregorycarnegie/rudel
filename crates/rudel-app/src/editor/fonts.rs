// fonts.rs - Strudel's editor fonts (the `fontFamily` setting).
// The website bundles a dozen display fonts (website/src/styles/index.css).
// Rather than redistribute fonts under a dozen licences, rudel fetches each
// from strudel.cc the first time it is chosen, cached on disk like a sample,
// and the editor stays in monospace until it arrives. egui's own monospace is
// Hack, so Strudel's `Hack` is simply that, and `Courier` (a system font in the
// browser) is the system-independent monospace too.
// SPDX-License-Identifier: AGPL-3.0-or-later

use eframe::egui;
use std::{
    collections::HashSet,
    sync::{Arc, LazyLock, Mutex},
};

/// One of the fonts Strudel's settings offer, by the name its `@font-face`
/// declares (which is also the value the setting stores).
#[derive(Debug, PartialEq)]
pub(crate) struct WebFont {
    pub(crate) name: &'static str,
    /// Where the font file lives: under strudel.cc's `/fonts/`, or a full URL.
    path: &'static str,
    /// The `@font-face` `size-adjust`, as a factor.
    pub(crate) size_adjust: f32,
}

const fn font(name: &'static str, path: &'static str, size_adjust: f32) -> WebFont {
    WebFont {
        name,
        path,
        size_adjust,
    }
}

pub(crate) const WEB_FONTS: &[WebFont] = &[
    font("CutiePi", "CutiePi/Cute_Aurora_demo.ttf", 1.2),
    // The website serves JetBrains Mono as woff2, which egui cannot read; this
    // is the same face as a TTF, from JetBrains' own repository.
    font(
        "JetBrains",
        "https://raw.githubusercontent.com/JetBrains/JetBrainsMono/master/fonts/ttf/JetBrainsMono-Regular.ttf",
        1.0,
    ),
    font("FiraCode", "FiraCode/FiraCode-Regular.ttf", 1.0),
    font("FiraCode-SemiBold", "FiraCode/FiraCode-SemiBold.ttf", 1.0),
    font("teletext", "teletext/EuropeanTeletext.ttf", 0.9),
    font("tic80", "tic80/tic-80-wide-font.otf", 0.6),
    font("mode7", "mode7/MODE7GX3.TTF", 0.82),
    font(
        "BigBlueTerminal",
        "BigBlueTerminal/BigBlue_TerminalPlus.TTF",
        1.0,
    ),
    font("x3270", "3270/3270-Regular.ttf", 1.0),
    font("Monocraft", "Monocraft/Monocraft.ttf", 0.9),
    font("PressStart", "PressStart2P/PressStart2P-Regular.ttf", 0.65),
    font(
        "we-come-in-peace",
        "we-come-in-peace/we-come-in-peace-bb.regular.ttf",
        1.0,
    ),
    font("galactico", "galactico/Galactico-Basic.otf", 1.0),
];

impl WebFont {
    fn url(&self) -> String {
        if self.path.starts_with("https://") {
            self.path.to_string()
        } else {
            format!("https://strudel.cc/fonts/{}", self.path)
        }
    }
}

#[derive(Default)]
struct Loads {
    requested: HashSet<&'static str>,
    /// Downloaded, not yet handed to egui.
    arrived: Vec<(&'static str, Vec<u8>)>,
    /// Handed to egui, in the order they arrived.
    bound: Vec<(&'static str, Arc<egui::FontData>)>,
    /// Bound before this frame, so egui's fonts have them: only these may be
    /// named in a `FontId`, as egui panics on a family it does not know.
    installed: HashSet<&'static str>,
    errors: Vec<String>,
}

static LOADS: LazyLock<Mutex<Loads>> = LazyLock::new(Mutex::default);

/// Whether `font` can be drawn with this frame.
pub(crate) fn is_installed(font: &WebFont) -> bool {
    LOADS.lock().unwrap().installed.contains(font.name)
}

/// Call once per frame, before anything draws: starts fetching `wanted` the
/// first time it is asked for, and hands whatever has arrived to egui (which
/// takes new fonts from the next frame). Returns fetch failures to report.
pub(crate) fn install(ctx: &egui::Context, wanted: Option<&'static WebFont>) -> Vec<String> {
    let mut loads = LOADS.lock().unwrap();
    let bound: Vec<&'static str> = loads.bound.iter().map(|(name, _)| *name).collect();
    loads.installed.extend(bound);
    if let Some(font) = wanted
        && loads.requested.insert(font.name)
    {
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let url = font.url();
            // egui panics on a font it cannot parse, so check before binding.
            let fetched = rudel_audio::samples::fetch_cached_bytes(&url).and_then(|bytes| {
                ab_glyph::FontRef::try_from_slice(&bytes).map_err(|e| format!("{url}: {e}"))?;
                Ok(bytes)
            });
            let mut loads = LOADS.lock().unwrap();
            match fetched {
                Ok(bytes) => loads.arrived.push((font.name, bytes)),
                Err(e) => loads.errors.push(format!("font {}: {e}", font.name)),
            }
            ctx.request_repaint();
        });
    }
    if !loads.arrived.is_empty() {
        let arrived = std::mem::take(&mut loads.arrived);
        loads.bound.extend(
            arrived
                .into_iter()
                .map(|(name, bytes)| (name, Arc::new(egui::FontData::from_owned(bytes)))),
        );
        ctx.set_fonts(definitions(&loads.bound));
    }
    std::mem::take(&mut loads.errors)
}

/// egui's default fonts plus each bound font as a family of its own, falling
/// back to the monospace fonts for any glyph it lacks.
fn definitions(bound: &[(&'static str, Arc<egui::FontData>)]) -> egui::FontDefinitions {
    let mut defs = egui::FontDefinitions::default();
    let fallback = defs.families[&egui::FontFamily::Monospace].clone();
    for (name, data) in bound {
        defs.font_data.insert(name.to_string(), data.clone());
        let mut family = vec![name.to_string()];
        family.extend(fallback.iter().cloned());
        defs.families
            .insert(egui::FontFamily::Name((*name).into()), family);
    }
    defs
}

#[cfg(test)]
pub(crate) fn install_for_test(ctx: &egui::Context, font: &'static WebFont, bytes: &[u8]) {
    LOADS.lock().unwrap().requested.insert(font.name);
    LOADS
        .lock()
        .unwrap()
        .arrived
        .push((font.name, bytes.to_vec()));
    install(ctx, None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fonts_come_from_strudel_cc_or_their_own_url() {
        assert_eq!(
            WEB_FONTS[2].url(),
            "https://strudel.cc/fonts/FiraCode/FiraCode-Regular.ttf"
        );
        assert!(WEB_FONTS[1].url().ends_with("JetBrainsMono-Regular.ttf"));
    }

    /// Network: every font still downloads and parses.
    #[test]
    #[ignore]
    fn every_web_font_downloads_and_parses() {
        for font in WEB_FONTS {
            let bytes = rudel_audio::samples::fetch_cached_bytes(&font.url()).unwrap();
            ab_glyph::FontRef::try_from_slice(&bytes)
                .unwrap_or_else(|e| panic!("{}: {e}", font.name));
        }
    }

    #[test]
    fn a_font_is_usable_only_from_the_frame_after_it_arrives() {
        let ctx = egui::Context::default();
        let font = &WEB_FONTS[WEB_FONTS.len() - 1];
        install_for_test(&ctx, font, epaint_default_fonts::UBUNTU_LIGHT);
        assert!(!is_installed(font), "egui binds it on the next pass");
        install(&ctx, None);
        assert!(is_installed(font));
        let defs = definitions(&LOADS.lock().unwrap().bound);
        let family = &defs.families[&egui::FontFamily::Name(font.name.into())];
        assert_eq!(family[0], font.name);
        assert!(
            family.len() > 1,
            "falls back to monospace for missing glyphs"
        );
    }
}
