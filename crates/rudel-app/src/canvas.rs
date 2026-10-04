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
use rudel_lang::canvas::CanvasOp;
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
}

impl Default for Canvas {
    fn default() -> Self {
        Self {
            pixmap: None,
            texture: None,
            font: FontArc::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT)
                .expect("egui's default font parses"),
            last_color: Color::BLACK,
        }
    }
}

impl Canvas {
    /// The canvas as a texture, once something has drawn on it.
    pub(crate) fn texture(&self) -> Option<egui::TextureId> {
        self.texture.as_ref().map(egui::TextureHandle::id)
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
}
