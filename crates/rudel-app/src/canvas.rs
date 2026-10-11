//! Strudel's draw canvas behind the code: what `getDrawContext()`, `.draw`,
//! `.onPaint`, `requestAnimationFrame` and `animate` paint
//! (`rudel_lang::canvas`).
//!
//! Like a browser canvas it keeps its pixels from frame to frame until
//! something clears them, which trails and `animate`'s smear rely on, so it
//! is a raster (tiny-skia) rather than shapes egui redraws: each frame's
//! operations are replayed onto it and the result uploaded as a texture.

use ab_glyph::{Font as _, FontArc, ScaleFont as _};
use eframe::egui;
use rudel_lang::canvas::{CanvasEvent, CanvasOp};
use tiny_skia::{
    BlendMode, Color, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform,
};

pub(crate) struct Canvas {
    pixmap: Option<Pixmap>,
    texture: Option<egui::TextureHandle>,
    font: FontArc,
    /// The last colour that parsed, which a bad one leaves standing, as a
    /// browser ignores an invalid `fillStyle`.
    last_color: Color,
    /// The modifiers held last frame: egui reports Ctrl/Shift/Alt pressed on
    /// their own as a change of state, not as key events.
    modifiers: egui::Modifiers,
}

impl Default for Canvas {
    fn default() -> Self {
        Self {
            pixmap: None,
            texture: None,
            font: FontArc::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT)
                .expect("egui's default font parses"),
            last_color: Color::BLACK,
            modifiers: egui::Modifiers::NONE,
        }
    }
}

impl Canvas {
    /// The canvas as a texture, once something has drawn on it.
    pub(crate) fn texture(&self) -> Option<egui::TextureId> {
        self.texture.as_ref().map(egui::TextureHandle::id)
    }

    /// This frame's mouse and key input over `rect` (the editor's visible
    /// area), as the DOM events a script's `document` handlers expect, in
    /// canvas pixels.
    pub(crate) fn events(&mut self, ctx: &egui::Context, rect: egui::Rect) -> Vec<CanvasEvent> {
        ctx.input(|i| {
            let mods = i.modifiers;
            let base = |kind: &'static str, m: egui::Modifiers| CanvasEvent {
                kind,
                ctrl: m.ctrl,
                shift: m.shift,
                alt: m.alt,
                meta: m.mac_cmd,
                ..CanvasEvent::default()
            };
            let mut events = Vec::new();
            for event in &i.events {
                match event {
                    egui::Event::PointerMoved(pos) if rect.contains(*pos) => {
                        let at = *pos - rect.min;
                        events.push(CanvasEvent {
                            x: at.x,
                            y: at.y,
                            ..base("mousemove", mods)
                        });
                    }
                    egui::Event::PointerButton {
                        pos,
                        button,
                        pressed,
                        modifiers,
                    } if rect.contains(*pos) => {
                        let at = *pos - rect.min;
                        let button = match button {
                            egui::PointerButton::Secondary => 2,
                            egui::PointerButton::Middle => 1,
                            _ => 0,
                        };
                        let kinds: &[&'static str] = if *pressed {
                            &["mousedown"]
                        } else {
                            &["mouseup", "click"]
                        };
                        for kind in kinds {
                            events.push(CanvasEvent {
                                x: at.x,
                                y: at.y,
                                button,
                                ..base(kind, *modifiers)
                            });
                        }
                    }
                    egui::Event::Key {
                        key,
                        pressed,
                        modifiers,
                        ..
                    } => {
                        let (name, code) = key_names(*key, modifiers.shift);
                        let kind = if *pressed { "keydown" } else { "keyup" };
                        events.push(CanvasEvent {
                            key: name,
                            code,
                            ..base(kind, *modifiers)
                        });
                    }
                    _ => {}
                }
            }
            for (was, is, key) in [
                (self.modifiers.ctrl, mods.ctrl, "Control"),
                (self.modifiers.shift, mods.shift, "Shift"),
                (self.modifiers.alt, mods.alt, "Alt"),
            ] {
                if was != is {
                    let kind = if is { "keydown" } else { "keyup" };
                    events.push(CanvasEvent {
                        key: key.into(),
                        code: format!("{key}Left"),
                        ..base(kind, mods)
                    });
                }
            }
            self.modifiers = mods;
            events
        })
    }

    /// Forget everything drawn (a new evaluation).
    pub(crate) fn clear(&mut self) {
        self.pixmap = None;
        self.texture = None;
    }

    /// Replay one frame's drawing onto a `width` by `height` canvas. A canvas
    /// that changes size starts empty, as a resized browser canvas does.
    pub(crate) fn draw(&mut self, ctx: &egui::Context, ops: &[CanvasOp], width: u32, height: u32) {
        let resized = self
            .pixmap
            .as_ref()
            .is_none_or(|p| (p.width(), p.height()) != (width, height));
        if resized {
            self.pixmap = Pixmap::new(width.max(1), height.max(1));
        }
        if ops.is_empty() && !resized {
            return;
        }
        let Some(mut pixmap) = self.pixmap.take() else {
            return;
        };
        for op in ops {
            self.replay(&mut pixmap, op);
        }
        let image = egui::ColorImage::from_rgba_premultiplied(
            [pixmap.width() as usize, pixmap.height() as usize],
            pixmap.data(),
        );
        match &mut self.texture {
            Some(texture) => texture.set(image, egui::TextureOptions::LINEAR),
            None => {
                self.texture = Some(ctx.load_texture(
                    "rudel-draw-canvas",
                    image,
                    egui::TextureOptions::LINEAR,
                ));
            }
        }
        self.pixmap = Some(pixmap);
    }

    fn color(&mut self, css: &str, alpha: f32) -> Color {
        if let Some(color) = parse_color(css) {
            self.last_color = color;
        }
        let mut color = self.last_color;
        color.apply_opacity(alpha.clamp(0.0, 1.0));
        color
    }

    fn replay(&mut self, pixmap: &mut Pixmap, op: &CanvasOp) {
        match op {
            CanvasOp::Fill {
                color,
                alpha,
                even_odd,
                polygons,
            } => {
                let paint = solid(self.color(color, *alpha));
                let rule = if *even_odd {
                    FillRule::EvenOdd
                } else {
                    FillRule::Winding
                };
                if let Some(path) = polygons_path(polygons.iter().map(|p| (true, p.as_slice()))) {
                    pixmap.fill_path(&path, &paint, rule, Transform::identity(), None);
                }
            }
            CanvasOp::Stroke {
                color,
                alpha,
                width,
                cap,
                join,
                lines,
            } => {
                let paint = solid(self.color(color, *alpha));
                let stroke = Stroke {
                    width: width.max(0.0),
                    line_cap: match cap.as_str() {
                        "round" => LineCap::Round,
                        "square" => LineCap::Square,
                        _ => LineCap::Butt,
                    },
                    line_join: match join.as_str() {
                        "round" => LineJoin::Round,
                        "bevel" => LineJoin::Bevel,
                        _ => LineJoin::Miter,
                    },
                    ..Stroke::default()
                };
                let lines = lines.iter().map(|(closed, p)| (*closed, p.as_slice()));
                if let Some(path) = polygons_path(lines) {
                    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
                }
            }
            CanvasOp::Clear { polygon } => {
                let mut paint = solid(Color::BLACK);
                paint.blend_mode = BlendMode::Clear;
                if let Some(path) = polygons_path([(true, polygon.as_slice())]) {
                    pixmap.fill_path(
                        &path,
                        &paint,
                        FillRule::Winding,
                        Transform::identity(),
                        None,
                    );
                }
            }
            CanvasOp::Text {
                text,
                x,
                y,
                size,
                color,
                alpha,
                align,
                baseline,
            } => {
                let color = self.color(color, *alpha);
                draw_text(
                    pixmap,
                    &self.font,
                    text,
                    [*x, *y],
                    *size,
                    color,
                    align,
                    baseline,
                );
            }
        }
    }
}

/// `KeyboardEvent.key` and `.code` for an egui key: a letter is lower case
/// unless Shift is held, a digit is itself, Space is `" "`.
fn key_names(key: egui::Key, shift: bool) -> (String, String) {
    let name = key.name();
    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphabetic() => {
            let key = if shift {
                c.to_ascii_uppercase()
            } else {
                c.to_ascii_lowercase()
            };
            (key.to_string(), format!("Key{}", c.to_ascii_uppercase()))
        }
        (Some(c), None) if c.is_ascii_digit() => (c.to_string(), format!("Digit{c}")),
        _ if name == "Space" => (" ".into(), "Space".into()),
        // egui's "Left" is the DOM's "ArrowLeft".
        _ if matches!(name, "Left" | "Right" | "Up" | "Down") => {
            (format!("Arrow{name}"), format!("Arrow{name}"))
        }
        _ => (name.to_string(), name.to_string()),
    }
}

fn solid(color: Color) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    paint
}

/// One path of several polygons or polylines, each closed or not.
fn polygons_path<'a>(
    parts: impl IntoIterator<Item = (bool, &'a [[f32; 2]])>,
) -> Option<tiny_skia::Path> {
    let mut builder = PathBuilder::new();
    for (closed, points) in parts {
        let Some(([x, y], rest)) = points.split_first() else {
            continue;
        };
        builder.move_to(*x, *y);
        for [x, y] in rest {
            builder.line_to(*x, *y);
        }
        if closed {
            builder.close();
        }
    }
    builder.finish()
}

/// `fillText`: glyphs laid out along the baseline, aligned as the canvas's
/// `textAlign`/`textBaseline` say, coverage blended in by hand (tiny-skia has
/// no text).
#[allow(clippy::too_many_arguments)]
fn draw_text(
    pixmap: &mut Pixmap,
    font: &FontArc,
    text: &str,
    [x, y]: [f32; 2],
    size: f32,
    color: Color,
    align: &str,
    baseline: &str,
) {
    let scaled = font.as_scaled(size.max(1.0));
    let ids: Vec<_> = text.chars().map(|c| font.glyph_id(c)).collect();
    let width: f32 = ids.iter().map(|&id| scaled.h_advance(id)).sum::<f32>()
        + ids.windows(2).map(|p| scaled.kern(p[0], p[1])).sum::<f32>();
    let mut pen = x - match align {
        "center" => width / 2.0,
        "right" | "end" => width,
        _ => 0.0,
    };
    let (ascent, descent) = (scaled.ascent(), scaled.descent());
    let baseline_y = y + match baseline {
        "top" | "hanging" => ascent,
        "middle" => (ascent + descent) / 2.0,
        "bottom" | "ideographic" => descent,
        _ => 0.0,
    };
    let (w, h) = (pixmap.width() as i32, pixmap.height() as i32);
    let stride = pixmap.width() as usize;
    let rgba = color.premultiply().to_color_u8();
    let data = pixmap.data_mut();
    let mut previous = None;
    for id in ids {
        if let Some(prev) = previous {
            pen += scaled.kern(prev, id);
        }
        previous = Some(id);
        let glyph = id.with_scale_and_position(scaled.scale(), ab_glyph::point(pen, baseline_y));
        pen += scaled.h_advance(id);
        let Some(outline) = font.outline_glyph(glyph) else {
            continue;
        };
        let bounds = outline.px_bounds();
        outline.draw(|gx, gy, coverage| {
            let (px, py) = (
                bounds.min.x as i32 + gx as i32,
                bounds.min.y as i32 + gy as i32,
            );
            if px < 0 || py < 0 || px >= w || py >= h {
                return;
            }
            let i = (py as usize * stride + px as usize) * 4;
            let src = [rgba.red(), rgba.green(), rgba.blue(), rgba.alpha()]
                .map(|c| f32::from(c) * coverage.clamp(0.0, 1.0));
            let keep = 1.0 - src[3] / 255.0;
            for c in 0..4 {
                data[i + c] = (src[c] + f32::from(data[i + c]) * keep).round().min(255.0) as u8;
            }
        });
    }
}

/// A CSS colour: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb()`/`rgba()`,
/// `hsl()`/`hsla()`, `transparent`, or a named colour.
fn parse_color(css: &str) -> Option<Color> {
    let css = css.trim().to_ascii_lowercase();
    if css == "transparent" {
        return Some(Color::TRANSPARENT);
    }
    if let Some(hex) = css.strip_prefix('#') {
        let digit = |i: usize, n: usize| u8::from_str_radix(hex.get(i..i + n)?, 16).ok();
        let [r, g, b, a] = match hex.len() {
            3 | 4 => [0, 1, 2, 3].map(|i| digit(i, 1).map(|v| v * 17)),
            6 | 8 => [0, 2, 4, 6].map(|i| digit(i, 2)),
            _ => return None,
        };
        return Some(Color::from_rgba8(r?, g?, b?, a.unwrap_or(255)));
    }
    let args = |name: &str| -> Option<Vec<f32>> {
        let inner = css
            .strip_prefix(name)?
            .trim_start_matches('a')
            .strip_prefix('(')?;
        inner
            .strip_suffix(')')?
            .split([',', ' ', '/'])
            .filter(|s| !s.is_empty())
            .map(|s| {
                s.strip_suffix('%')
                    .map_or_else(
                        || s.trim_end_matches("deg").parse(),
                        |p| p.parse().map(|v: f32| v / 100.0),
                    )
                    .ok()
            })
            .collect()
    };
    if let Some(v) = args("rgb") {
        let channel = |i: usize| {
            let x = *v.get(i)?;
            // A percentage arrives as a fraction.
            Some(if x <= 1.0 && css.contains('%') {
                x
            } else {
                x / 255.0
            })
        };
        let alpha = v.get(3).copied().unwrap_or(1.0);
        return Color::from_rgba(
            channel(0)?.clamp(0.0, 1.0),
            channel(1)?.clamp(0.0, 1.0),
            channel(2)?.clamp(0.0, 1.0),
            alpha.clamp(0.0, 1.0),
        );
    }
    if let Some(v) = args("hsl") {
        let (h, s, l) = (v.first()? / 360.0, *v.get(1)?, *v.get(2)?);
        let alpha = v.get(3).copied().unwrap_or(1.0);
        let q = if l < 0.5 {
            l * (1.0 + s)
        } else {
            l + s - l * s
        };
        let p = 2.0 * l - q;
        let channel = |t: f32| {
            let t = t.rem_euclid(1.0);
            if t < 1.0 / 6.0 {
                p + (q - p) * 6.0 * t
            } else if t < 0.5 {
                q
            } else if t < 2.0 / 3.0 {
                p + (q - p) * (2.0 / 3.0 - t) * 6.0
            } else {
                p
            }
        };
        return Color::from_rgba(
            channel(h + 1.0 / 3.0),
            channel(h),
            channel(h - 1.0 / 3.0),
            alpha.clamp(0.0, 1.0),
        );
    }
    let hex = rudel_core::css_color_hex(&css)?;
    parse_color(hex)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgba(css: &str) -> Option<[u8; 4]> {
        parse_color(css).map(|c| {
            let c = c.to_color_u8();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
    }

    #[test]
    fn css_colours_parse_as_a_canvas_reads_them() {
        assert_eq!(rgba("#f00"), Some([255, 0, 0, 255]));
        assert_eq!(rgba("#20001050"), Some([0x20, 0, 0x10, 0x50]));
        assert_eq!(rgba("rgba(0, 128, 255, 0.5)"), Some([0, 128, 255, 128]));
        assert_eq!(rgba("hsl(120, 100%, 50%)"), Some([0, 255, 0, 255]));
        assert_eq!(rgba("tomato"), Some([255, 99, 71, 255]));
        assert_eq!(rgba("ingigo"), None, "a typo is ignored, as a browser does");
    }

    #[test]
    fn the_canvas_keeps_its_pixels_until_cleared() {
        let ctx = egui::Context::default();
        let mut canvas = Canvas::default();
        let square = vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
        let fill = CanvasOp::Fill {
            color: "#ff0000".into(),
            alpha: 1.0,
            even_odd: false,
            polygons: vec![square.clone()],
        };
        let pixel = |c: &Canvas, x: usize| {
            let p = c.pixmap.as_ref().expect("a canvas");
            p.data()[(2 * p.width() as usize + x) * 4..][..4].to_vec()
        };
        canvas.draw(&ctx, &[fill], 8, 8);
        assert_eq!(pixel(&canvas, 2), [255, 0, 0, 255]);
        // A frame that draws nothing leaves it as it was.
        canvas.draw(&ctx, &[], 8, 8);
        assert_eq!(pixel(&canvas, 2), [255, 0, 0, 255]);
        canvas.draw(&ctx, &[CanvasOp::Clear { polygon: square }], 8, 8);
        assert_eq!(pixel(&canvas, 2), [0, 0, 0, 0]);
    }

    #[test]
    fn keys_are_named_as_the_dom_names_them() {
        assert_eq!(key_names(egui::Key::A, false), ("a".into(), "KeyA".into()));
        assert_eq!(key_names(egui::Key::A, true), ("A".into(), "KeyA".into()));
        assert_eq!(
            key_names(egui::Key::Num3, false),
            ("3".into(), "Digit3".into())
        );
        assert_eq!(
            key_names(egui::Key::Space, false),
            (" ".into(), "Space".into())
        );
        assert_eq!(key_names(egui::Key::ArrowLeft, false).0, "ArrowLeft");
        assert_eq!(
            key_names(egui::Key::Enter, false),
            ("Enter".into(), "Enter".into())
        );
    }

    #[test]
    fn text_puts_ink_on_the_canvas() {
        let ctx = egui::Context::default();
        let mut canvas = Canvas::default();
        let text = CanvasOp::Text {
            text: "Hi".into(),
            x: 2.0,
            y: 20.0,
            size: 20.0,
            color: "white".into(),
            alpha: 1.0,
            align: "start".into(),
            baseline: "alphabetic".into(),
        };
        canvas.draw(&ctx, &[text], 40, 30);
        let inked = canvas
            .pixmap
            .as_ref()
            .unwrap()
            .data()
            .chunks(4)
            .filter(|p| p[3] > 128)
            .count();
        assert!(inked > 20, "{inked}");
    }

    #[test]
    fn more_css_colours_parse_as_a_canvas_reads_them() {
        // A fraction from a percentage, a plain number on the 0..255 scale.
        assert_eq!(rgba("rgb(50%, 0%, 0%)"), Some([128, 0, 0, 255]));
        assert_eq!(rgba("rgb(1, 0, 0)"), Some([1, 0, 0, 255]));
        // Dark and light, and a hue in each sixth of the wheel that matters.
        assert_eq!(rgba("hsl(0, 100%, 25%)"), Some([128, 0, 0, 255]));
        assert_eq!(rgba("hsl(0, 100%, 75%)"), Some([255, 128, 128, 255]));
        assert_eq!(rgba("hsl(30, 100%, 50%)"), Some([255, 128, 0, 255]));
        assert_eq!(rgba("hsl(210, 100%, 50%)"), Some([0, 128, 255, 255]));
        // Below full saturation, where `l * s` is not just `l`.
        assert_eq!(rgba("hsl(0, 50%, 75%)"), Some([223, 159, 159, 255]));
    }

    /// One pass of input through `Canvas::events`, over a rect at (10, 20).
    fn events(canvas: &mut Canvas, raw: egui::RawInput) -> Vec<CanvasEvent> {
        let ctx = egui::Context::default();
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(100.0, 100.0));
        let mut got = Vec::new();
        let mut out = ctx.run_ui(raw, |ui| got = canvas.events(ui.ctx(), rect));
        out.textures_delta.clear();
        got
    }

    #[test]
    fn input_over_the_editor_becomes_dom_events_in_canvas_pixels() {
        let mut canvas = Canvas::default();
        let ctrl = egui::Modifiers {
            ctrl: true,
            ..egui::Modifiers::NONE
        };
        let raw = egui::RawInput {
            events: vec![
                egui::Event::ModifiersChanged(ctrl),
                egui::Event::PointerMoved(egui::pos2(15.0, 25.0)),
                // Outside the editor: not the canvas's.
                egui::Event::PointerMoved(egui::pos2(500.0, 25.0)),
                egui::Event::PointerButton {
                    pos: egui::pos2(12.0, 22.0),
                    button: egui::PointerButton::Secondary,
                    pressed: true,
                    modifiers: ctrl,
                },
                egui::Event::PointerButton {
                    pos: egui::pos2(500.0, 22.0),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: ctrl,
                },
                egui::Event::PointerButton {
                    pos: egui::pos2(13.0, 23.0),
                    button: egui::PointerButton::Middle,
                    pressed: false,
                    modifiers: egui::Modifiers::SHIFT,
                },
                egui::Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::SHIFT,
                },
            ],
            ..Default::default()
        };
        let got = events(&mut canvas, raw);
        let event = |kind: &'static str| CanvasEvent {
            kind,
            ..CanvasEvent::default()
        };
        let want = vec![
            CanvasEvent {
                x: 5.0,
                y: 5.0,
                ctrl: true,
                ..event("mousemove")
            },
            CanvasEvent {
                x: 2.0,
                y: 2.0,
                button: 2,
                ctrl: true,
                ..event("mousedown")
            },
            CanvasEvent {
                x: 3.0,
                y: 3.0,
                button: 1,
                shift: true,
                ..event("mouseup")
            },
            CanvasEvent {
                x: 3.0,
                y: 3.0,
                button: 1,
                shift: true,
                ..event("click")
            },
            CanvasEvent {
                key: "A".into(),
                code: "KeyA".into(),
                shift: true,
                ..event("keydown")
            },
            // Ctrl went down since the last frame.
            CanvasEvent {
                key: "Control".into(),
                code: "ControlLeft".into(),
                ctrl: true,
                ..event("keydown")
            },
        ];
        assert_eq!(got, want);
        // ...and comes back up, with every flag reported as it now is.
        let mods = egui::Modifiers {
            alt: true,
            mac_cmd: true,
            ..egui::Modifiers::NONE
        };
        let got = events(
            &mut canvas,
            egui::RawInput {
                events: vec![egui::Event::ModifiersChanged(mods)],
                ..Default::default()
            },
        );
        let up = CanvasEvent {
            key: "Control".into(),
            code: "ControlLeft".into(),
            alt: true,
            meta: true,
            ..event("keyup")
        };
        let alt = CanvasEvent {
            key: "Alt".into(),
            code: "AltLeft".into(),
            alt: true,
            meta: true,
            ..event("keydown")
        };
        assert_eq!(got, [up, alt]);
    }

    #[test]
    fn a_canvas_is_a_texture_from_its_first_frame_until_cleared() {
        let ctx = egui::Context::default();
        let mut canvas = Canvas::default();
        assert_eq!(canvas.texture(), None);
        // An empty first frame still makes the (blank) canvas.
        canvas.draw(&ctx, &[], 8, 8);
        let id = canvas.texture.as_ref().map(egui::TextureHandle::id);
        assert!(id.is_some());
        assert_eq!(canvas.texture(), id);
        canvas.clear();
        assert_eq!(canvas.texture(), None);
        assert!(canvas.pixmap.is_none());
    }

    /// Alpha at pixel (x, y) after `ops` on a 100 x 100 canvas.
    fn alpha_after(ops: &[CanvasOp], points: &[(usize, usize)]) -> Vec<u8> {
        let ctx = egui::Context::default();
        let mut canvas = Canvas::default();
        canvas.draw(&ctx, ops, 100, 100);
        let p = canvas.pixmap.as_ref().unwrap();
        points
            .iter()
            .map(|&(x, y)| p.data()[(y * 100 + x) * 4 + 3])
            .collect()
    }

    fn stroke(cap: &str, join: &str, points: Vec<[f32; 2]>) -> CanvasOp {
        CanvasOp::Stroke {
            color: "white".into(),
            alpha: 1.0,
            width: 20.0,
            cap: cap.into(),
            join: join.into(),
            lines: vec![(false, points)],
        }
    }

    #[rstest::rstest]
    // Past the end of (20, 20)..(50, 20), on the line and off its corner.
    #[case::butt("butt", [false, false])]
    #[case::square("square", [true, true])]
    #[case::round("round", [true, false])]
    fn a_line_ends_in_its_cap(#[case] cap: &str, #[case] inked: [bool; 2]) {
        let line = stroke(cap, "miter", vec![[20.0, 20.0], [50.0, 20.0]]);
        let got = alpha_after(&[line], &[(55, 20), (58, 28)]);
        let got_inked: Vec<bool> = got.iter().map(|&a| a > 128).collect();
        assert_eq!(got_inked, inked, "{got:?}");
    }

    #[rstest::rstest]
    // Outside the corner of (20, 50)..(50, 50)..(50, 80): far out, then near.
    #[case::miter("miter", [true, true])]
    #[case::round("round", [false, true])]
    #[case::bevel("bevel", [false, false])]
    fn a_corner_takes_its_join(#[case] join: &str, #[case] inked: [bool; 2]) {
        let corner = stroke("butt", join, vec![[20.0, 50.0], [50.0, 50.0], [50.0, 80.0]]);
        let got = alpha_after(&[corner], &[(58, 41), (56, 43)]);
        let got_inked: Vec<bool> = got.iter().map(|&a| a > 128).collect();
        assert_eq!(got_inked, inked, "{got:?}");
    }

    #[test]
    fn a_line_is_as_wide_as_its_width() {
        let line = stroke("butt", "miter", vec![[20.0, 20.0], [50.0, 20.0]]);
        assert!(alpha_after(&[line], &[(35, 28)])[0] > 128);
    }

    /// The inked box of `text` drawn at (50, 50), size 40: `(min x, min y,
    /// max x, max y)`.
    fn ink(text: &str, align: &str, baseline: &str) -> (usize, usize, usize, usize) {
        let ctx = egui::Context::default();
        let mut canvas = Canvas::default();
        let op = CanvasOp::Text {
            text: text.into(),
            x: 50.0,
            y: 50.0,
            size: 40.0,
            color: "white".into(),
            alpha: 1.0,
            align: align.into(),
            baseline: baseline.into(),
        };
        canvas.draw(&ctx, &[op], 100, 100);
        let p = canvas.pixmap.as_ref().unwrap();
        let mut b = (usize::MAX, usize::MAX, 0, 0);
        for (i, px) in p.data().chunks(4).enumerate() {
            if px[3] > 64 {
                let (x, y) = (i % 100, i / 100);
                b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
            }
        }
        b
    }

    #[test]
    fn text_is_placed_by_its_align_and_baseline() {
        let scaled = Canvas::default().font.into_scaled(40.0);
        let (ascent, descent) = (scaled.ascent(), scaled.descent());
        // ab_glyph reads only a legacy `kern` table, which this font lacks, so
        // even "AV" places by advances alone.
        let start = ink("AV", "start", "alphabetic");
        let width = (start.2 - start.0) as f32;
        let right = ink("AV", "right", "alphabetic");
        let centre = ink("AV", "center", "alphabetic");
        assert!((right.2 as f32 - 50.0).abs() <= 2.0, "{right:?}");
        assert!(
            ((centre.0 + centre.2) as f32 / 2.0 - 50.0).abs() <= 2.0,
            "{centre:?}"
        );
        assert!(((right.2 - right.0) as f32 - width).abs() <= 1.0);
        // The ink sits on the baseline, so its bottom moves with it.
        assert!((start.3 as f32 - 50.0).abs() <= 1.0, "{start:?}");
        let shift = |b: &str| ink("AV", "start", b).3 as f32 - start.3 as f32;
        assert!((shift("top") - ascent).abs() <= 1.0);
        assert!((shift("middle") - (ascent + descent) / 2.0).abs() <= 1.0);
        assert!((shift("bottom") - descent).abs() <= 1.0);
    }

    #[test]
    fn text_off_any_edge_is_clipped_not_wrapped() {
        // Ink reaches the very first row and column, and nothing wraps round.
        let ctx = egui::Context::default();
        let mut canvas = Canvas::default();
        let at = |x: f32, y: f32, baseline: &str| CanvasOp::Text {
            text: "MM".into(),
            x,
            y,
            size: 40.0,
            color: "white".into(),
            alpha: 1.0,
            align: "start".into(),
            baseline: baseline.into(),
        };
        canvas.draw(
            &ctx,
            &[at(-10.0, 20.0, "alphabetic"), at(80.0, 110.0, "bottom")],
            100,
            100,
        );
        let p = canvas.pixmap.as_ref().unwrap();
        let a = |x: usize, y: usize| p.data()[(y * 100 + x) * 4 + 3];
        assert!((0..30).any(|y| a(0, y) > 64), "column 0");
        let mut top = Canvas::default();
        top.draw(&ctx, &[at(10.0, 10.0, "alphabetic")], 100, 100);
        let t = top.pixmap.as_ref().unwrap();
        assert!((0..100).any(|x| t.data()[x * 4 + 3] > 64), "row 0");
        // Nothing from the right/bottom text lands back on the left.
        assert!((40..100).all(|y| (0..20).all(|x| a(x, y) == 0)));
    }

    #[test]
    fn text_blends_over_what_is_there() {
        // Half-transparent white over opaque grey: grey shows through half.
        let ctx = egui::Context::default();
        let mut canvas = Canvas::default();
        let grey = CanvasOp::Fill {
            color: "rgb(100, 100, 100)".into(),
            alpha: 1.0,
            even_odd: false,
            polygons: vec![vec![[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]]],
        };
        let text = |alpha: f32| CanvasOp::Text {
            text: "O".into(),
            x: 20.0,
            y: 80.0,
            size: 80.0,
            color: "white".into(),
            alpha,
            align: "start".into(),
            baseline: "alphabetic".into(),
        };
        canvas.draw(&ctx, &[grey, text(0.5)], 100, 100);
        let p = canvas.pixmap.as_ref().unwrap();
        let px: Vec<&[u8]> = p.data().chunks(4).collect();
        // Fully covered pixels: 128 + 100 / 2.
        let full = px.iter().filter(|c| c[0] == 178 && c[3] == 255).count();
        assert!(full > 50, "{full}");
        assert!(px.iter().all(|c| c[0] >= 100 && c[0] <= 178 && c[3] == 255));
        // The hole in the O is untouched: no coverage, no ink.
        let mut solo = Canvas::default();
        solo.draw(&ctx, &[text(1.0)], 100, 100);
        let p = solo.pixmap.as_ref().unwrap();
        let o = ink_box(p);
        let middle = ((o.1 + o.3) / 2 * 100 + (o.0 + o.2) / 2) * 4;
        assert_eq!(p.data()[middle + 3], 0, "the counter of the O");
    }

    fn ink_box(p: &Pixmap) -> (usize, usize, usize, usize) {
        let mut b = (usize::MAX, usize::MAX, 0, 0);
        for (i, px) in p.data().chunks(4).enumerate() {
            if px[3] > 64 {
                let (x, y) = (i % 100, i / 100);
                b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
            }
        }
        b
    }
}
