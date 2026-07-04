//! Procedurally-drawn window icon: a two-turn spiral (a nod to Strudel's
//! pretzel swirl) on a dark rounded square, purple at the core fading to green
//! at the tail — echoing the editor's keyword/string colors. Generated once at
//! startup so there's no image file or rasterizer dependency to carry.

use eframe::egui::IconData;

const SIZE: usize = 256;

/// The window/taskbar icon as RGBA pixels.
pub(crate) fn icon() -> IconData {
    let n = SIZE as f32;
    let mut rgba = vec![0u8; SIZE * SIZE * 4];

    // Dark rounded-square background with an antialiased edge; everything
    // outside the rounded rect stays transparent so the corners read as round.
    let radius = n * 0.19; // corner radius
    let bg = [0x1e, 0x1e, 0x2a];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let cov = rounded_rect_coverage(x as f32 + 0.5, y as f32 + 0.5, n, radius);
            if cov > 0.0 {
                blend(&mut rgba, x, y, bg, cov);
            }
        }
    }

    // Archimedean spiral, stamped as a tapering round brush along the curve.
    let (cx, cy) = (n * 0.5, n * 0.5);
    let max_r = n * 0.33;
    let turns = 2.25;
    let theta_max = turns * std::f32::consts::TAU;
    let purple = [0xc0, 0x8c, 0xff];
    let green = [0x8c, 0xe0, 0x9a];
    let steps = 4000;
    for i in 0..=steps {
        let t = i as f32 / steps as f32; // 0..1 along the spiral
        let theta = t * theta_max;
        let r = max_r * t;
        let x = cx + r * theta.cos();
        let y = cy + r * theta.sin();
        // Thicker at the core, thinner at the tail — like a drawn stroke.
        let w = n * (0.055 - 0.028 * t);
        let color = lerp(purple, green, t);
        stamp(&mut rgba, x, y, w, color);
    }

    IconData {
        rgba,
        width: SIZE as u32,
        height: SIZE as u32,
    }
}

/// Antialiased coverage (0..1) of a rounded square inset slightly from the
/// full `n`×`n` bounds, evaluated at pixel-center `(px, py)`.
fn rounded_rect_coverage(px: f32, py: f32, n: f32, radius: f32) -> f32 {
    let inset = n * 0.02;
    let half = (n - 2.0 * inset) * 0.5;
    let cx = n * 0.5;
    // Signed distance to a rounded box centered at the icon center.
    let dx = (px - cx).abs() - (half - radius);
    let dy = (py - cx).abs() - (half - radius);
    let ax = dx.max(0.0);
    let ay = dy.max(0.0);
    let dist = (ax * ax + ay * ay).sqrt() + dx.max(dy).min(0.0) - radius;
    // 1px-wide antialiased edge.
    (0.5 - dist).clamp(0.0, 1.0)
}

/// Stamp a soft round brush of `color` centered at `(fx, fy)` with radius `w`.
fn stamp(rgba: &mut [u8], fx: f32, fy: f32, w: f32, color: [u8; 3]) {
    let x0 = ((fx - w - 1.0).floor() as isize).max(0) as usize;
    let x1 = ((fx + w + 1.0).ceil() as isize).min(SIZE as isize - 1) as usize;
    let y0 = ((fy - w - 1.0).floor() as isize).max(0) as usize;
    let y1 = ((fy + w + 1.0).ceil() as isize).min(SIZE as isize - 1) as usize;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let d = ((x as f32 + 0.5 - fx).powi(2) + (y as f32 + 0.5 - fy).powi(2)).sqrt();
            let a = (w - d + 0.5).clamp(0.0, 1.0);
            if a > 0.0 {
                blend(rgba, x, y, color, a);
            }
        }
    }
}

/// Alpha-blend `color` over the existing pixel at `(x, y)`.
fn blend(rgba: &mut [u8], x: usize, y: usize, color: [u8; 3], a: f32) {
    let i = (y * SIZE + x) * 4;
    for c in 0..3 {
        let src = color[c] as f32;
        let dst = rgba[i + c] as f32;
        rgba[i + c] = (src * a + dst * (1.0 - a)).round() as u8;
    }
    let dst_a = rgba[i + 3] as f32 / 255.0;
    let out_a = a + dst_a * (1.0 - a);
    rgba[i + 3] = (out_a * 255.0).round() as u8;
}

fn lerp(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    [
        (a[0] as f32 + (b[0] as f32 - a[0] as f32) * t) as u8,
        (a[1] as f32 + (b[1] as f32 - a[1] as f32) * t) as u8,
        (a[2] as f32 + (b[2] as f32 - a[2] as f32) * t) as u8,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_is_correctly_sized_and_partly_opaque() {
        let icon = icon();
        assert_eq!(icon.width, SIZE as u32);
        assert_eq!(icon.height, SIZE as u32);
        assert_eq!(icon.rgba.len(), SIZE * SIZE * 4);
        // Center pixel sits inside the rounded square, so it must be opaque.
        let center = ((SIZE / 2) * SIZE + SIZE / 2) * 4 + 3;
        assert_eq!(icon.rgba[center], 255);
        // A corner pixel is outside the rounded square, so fully transparent.
        assert_eq!(icon.rgba[3], 0);
    }
}
