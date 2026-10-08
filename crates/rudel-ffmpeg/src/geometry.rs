//! The arithmetic around a decoded frame — display rotation, fitting it to a
//! width, turning its pixels and timing it — kept free of FFmpeg so it can be
//! tested exhaustively without a decoder.

use std::time::Duration;

/// The largest frame, in pixels, either side of scaling may allocate.
pub(crate) const MAX_PIXELS: i64 = 64 * 1024 * 1024;

/// How a frame is shown upright: turned `turns` quarter turns clockwise, then
/// mirrored left to right if `mirror`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Orientation {
    pub turns: u32,
    pub mirror: bool,
}

impl Orientation {
    /// Read a 16.16 fixed-point display matrix, as MOV/MP4 store it.
    ///
    /// Cameras write the eight orientations a picture can have: quarter turns,
    /// and for some front cameras the same mirrored. `av_display_rotation_get`
    /// reads only an angle, so it takes a mirror for a half turn and shows the
    /// picture upside down. A mirror is a negative determinant; undoing it on
    /// the first column — the one `av_display_matrix_flip` negates — leaves the
    /// rotation, measured the way `av_display_rotation_get` measures it. Angles
    /// between quarter turns round to the nearest (an arbitrary affine
    /// transform would need libavfilter); a singular matrix shows the
    /// picture as stored.
    pub(crate) fn from_display_matrix(m: &[i32; 9]) -> Self {
        let [a, b, _, c, d, ..] = m.map(f64::from);
        // A matrix that collapses the picture to a line or a point is no
        // orientation at all.
        let determinant = a * d - b * c;
        if determinant == 0.0 {
            return Self::default();
        }
        let mirror = determinant < 0.0;
        let (a, c) = if mirror { (-a, -c) } else { (a, c) };
        let (across, down) = (a.hypot(c), b.hypot(d));
        let counter_clockwise = -(b / down).atan2(a / across).to_degrees();
        Self {
            turns: ((-counter_clockwise / 90.0).round() as i64).rem_euclid(4) as u32,
            mirror,
        }
    }

    /// Whether the shown picture is the stored one on its side.
    pub(crate) fn sideways(self) -> bool {
        self.turns % 2 == 1
    }
}

/// The size a `width`×`height` frame is scaled to before it is oriented, so
/// that its *shown* width — the stored height when it is turned on its side —
/// is no more than `max_width`. Frames are never enlarged, and never scaled to
/// nothing.
pub(crate) fn fit(width: u32, height: u32, sideways: bool, max_width: u32) -> (u32, u32) {
    let display_width = if sideways { height } else { width };
    let scale = (f64::from(max_width) / f64::from(display_width.max(1))).min(1.0);
    let side = |n: u32| (f64::from(n) * scale).round().max(1.0) as u32;
    (side(width), side(height))
}

/// The packed RGBA of a `width`×`height` picture shown as `orientation` says.
/// `row(y)` yields at least row `y`'s `width * 4` bytes, wherever its stride
/// puts it.
pub(crate) fn orient<'a>(
    width: usize,
    height: usize,
    orientation: Orientation,
    row: impl Fn(usize) -> &'a [u8],
) -> Vec<u8> {
    let out_width = if orientation.sideways() {
        height
    } else {
        width
    };
    let mut rgba = vec![0; width * height * 4];
    for y in 0..height {
        let row = &row(y)[..width * 4];
        if orientation == Orientation::default() {
            rgba[y * row.len()..(y + 1) * row.len()].copy_from_slice(row);
            continue;
        }
        for (x, pixel) in row.as_chunks::<4>().0.iter().enumerate() {
            let (ox, oy) = match orientation.turns % 4 {
                0 => (x, y),
                1 => (height - 1 - y, x),
                2 => (width - 1 - x, height - 1 - y),
                _ => (y, width - 1 - x),
            };
            let ox = if orientation.mirror {
                out_width - 1 - ox
            } else {
                ox
            };
            let offset = (oy * out_width + ox) * 4;
            rgba[offset..offset + 4].copy_from_slice(pixel);
        }
    }
    rgba
}

/// `ticks` of a `num/den` second time base in seconds; None for a time base
/// that cannot be one.
pub(crate) fn seconds(ticks: i64, num: i32, den: i32) -> Option<f64> {
    (num > 0 && den > 0).then(|| ticks as f64 * f64::from(num) / f64::from(den))
}

/// How long a frame shows: its own duration if the stream gives one, else one
/// period of the guessed frame `rate` (`num/den` frames a second), else 30 fps.
/// A frame shows for between `MIN_FRAME` and `MAX_FRAME`, so a corrupt
/// duration can neither freeze the picture nor round to nothing and spin the
/// player as fast as it can decode.
pub(crate) fn frame_duration(own: Option<f64>, rate: (i32, i32)) -> Duration {
    const MIN_FRAME: Duration = Duration::from_millis(1);
    const MAX_FRAME: Duration = Duration::from_secs(10);
    const FALLBACK: Duration = Duration::from_nanos(1_000_000_000 / 30);
    let seconds = match (own, rate) {
        (Some(own), _) if own > 0.0 && own.is_finite() => own,
        (_, (num, den)) if num > 0 && den > 0 => f64::from(den) / f64::from(num),
        _ => return FALLBACK,
    };
    Duration::try_from_secs_f64(seconds).map_or(FALLBACK, |d| d.clamp(MIN_FRAME, MAX_FRAME))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `av_display_rotation_set(degrees)` then `av_display_matrix_flip`, as
    /// libavutil builds them, in 16.16 fixed point.
    fn matrix(counter_clockwise: f64, hflip: bool, vflip: bool) -> [i32; 9] {
        let (sin, cos) = counter_clockwise.to_radians().sin_cos();
        let fixed = |v: f64| (v * 65536.0).round() as i32;
        let mut m = [
            fixed(cos),
            fixed(-sin),
            0,
            fixed(sin),
            fixed(cos),
            0,
            0,
            0,
            1 << 30,
        ];
        for (i, value) in m.iter_mut().enumerate() {
            if (i % 3 == 0 && hflip) || (i % 3 == 1 && vflip) {
                *value = -*value;
            }
        }
        m
    }

    fn shown(turns: u32, mirror: bool) -> Orientation {
        Orientation { turns, mirror }
    }

    #[test]
    fn a_rotation_matrix_is_undone_in_clockwise_quarter_turns() {
        // libavutil's angles are counter-clockwise; showing the picture upright
        // turns it back the other way.
        for (degrees, turns) in [
            (0.0, 0),
            (90.0, 3),
            (-90.0, 1),
            (180.0, 2),
            (-180.0, 2),
            (270.0, 1),
            (-270.0, 3),
            (360.0, 0),
            (-89.0, 1),
            (44.0, 0),
            (46.0, 3),
        ] {
            assert_eq!(
                Orientation::from_display_matrix(&matrix(degrees, false, false)),
                shown(turns, false),
                "{degrees}°"
            );
        }
        // What a phone held upright writes into its MOV.
        let portrait = [0, 65536, 0, -65536, 0, 0, 0, 0, 1 << 30];
        assert_eq!(Orientation::from_display_matrix(&portrait), shown(1, false));
    }

    #[test]
    fn a_mirrored_matrix_is_a_mirror_not_a_half_turn() {
        assert_eq!(
            Orientation::from_display_matrix(&matrix(0.0, true, false)),
            shown(0, true)
        );
        // Upside down and mirrored is the same picture flipped vertically.
        assert_eq!(
            Orientation::from_display_matrix(&matrix(0.0, false, true)),
            shown(2, true)
        );
        assert_eq!(
            Orientation::from_display_matrix(&matrix(0.0, true, true)),
            shown(2, false)
        );
        for (degrees, turns) in [(90.0, 3), (-90.0, 1), (180.0, 2)] {
            assert_eq!(
                Orientation::from_display_matrix(&matrix(degrees, true, false)),
                shown(turns, true),
                "{degrees}° mirrored"
            );
        }
    }

    #[test]
    fn a_degenerate_matrix_shows_the_picture_as_stored() {
        let zero = [0; 9];
        assert_eq!(
            Orientation::from_display_matrix(&zero),
            Orientation::default()
        );
        let flat = [65536, 0, 0, 0, 0, 0, 0, 0, 1 << 30];
        assert_eq!(
            Orientation::from_display_matrix(&flat),
            Orientation::default()
        );
        assert_eq!(
            Orientation::from_display_matrix(&[i32::MIN; 9]),
            Orientation::default()
        );
    }

    #[test]
    fn fit_limits_the_displayed_width_and_keeps_the_aspect() {
        assert_eq!(fit(1600, 200, false, 1280), (1280, 160));
        // On its side, the stored height is what is shown across.
        assert_eq!(fit(200, 1600, true, 1280), (160, 1280));
        // A wide frame shown on its side is already narrow enough.
        assert_eq!(fit(1600, 200, true, 1280), (1600, 200));
    }

    #[test]
    fn fit_never_enlarges_and_never_scales_to_nothing() {
        assert_eq!(fit(16, 8, false, 1280), (16, 8));
        assert_eq!(fit(1280, 720, false, 1280), (1280, 720));
        assert_eq!(fit(1281, 720, false, 1280), (1280, 719));
        assert_eq!(fit(10_000, 1, false, 100), (100, 1));
        assert_eq!(fit(1, 10_000, true, 100), (1, 100));
        assert_eq!(fit(0, 0, false, 100), (1, 1));
        // Rounds to nearest, not down.
        assert_eq!(fit(3, 2, false, 2), (2, 1));
        assert_eq!(fit(3, 5, false, 2), (2, 3));
    }

    /// A 3×2 picture whose pixel at (x, y) is `[x, y, 9, 255]`, stored with
    /// two bytes of row padding as FFmpeg's aligned frames have.
    fn picture() -> (Vec<u8>, usize) {
        let stride = 3 * 4 + 2;
        let mut data = vec![0xEE; stride * 2];
        for y in 0..2 {
            for x in 0..3 {
                let at = y * stride + x * 4;
                data[at..at + 4].copy_from_slice(&[x as u8, y as u8, 9, 255]);
            }
        }
        (data, stride)
    }

    fn pixels(rgba: &[u8]) -> Vec<(u8, u8)> {
        rgba.chunks(4).map(|p| (p[0], p[1])).collect()
    }

    #[test]
    fn orient_turns_each_pixel_clockwise_and_skips_row_padding() {
        let (data, stride) = picture();
        let turned = |turns| orient(3, 2, shown(turns, false), |y| &data[y * stride..]);
        // Source, as (x, y) of where each output pixel came from:
        //   (0,0) (1,0) (2,0)
        //   (0,1) (1,1) (2,1)
        assert_eq!(
            pixels(&turned(0)),
            [(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (2, 1)]
        );
        // One clockwise turn: 2 wide, 3 tall, the bottom row now on the left.
        assert_eq!(
            pixels(&turned(1)),
            [(0, 1), (0, 0), (1, 1), (1, 0), (2, 1), (2, 0)]
        );
        assert_eq!(
            pixels(&turned(2)),
            [(2, 1), (1, 1), (0, 1), (2, 0), (1, 0), (0, 0)]
        );
        assert_eq!(
            pixels(&turned(3)),
            [(2, 0), (2, 1), (1, 0), (1, 1), (0, 0), (0, 1)]
        );
        for turns in 0..4 {
            let rgba = turned(turns);
            assert_eq!(rgba.len(), 3 * 2 * 4);
            assert!(rgba.chunks(4).all(|p| p[2..] == [9, 255]), "padding leaked");
        }
    }

    #[test]
    fn orient_mirrors_after_turning() {
        let (data, stride) = picture();
        let mirrored = |turns| orient(3, 2, shown(turns, true), |y| &data[y * stride..]);
        assert_eq!(
            pixels(&mirrored(0)),
            [(2, 0), (1, 0), (0, 0), (2, 1), (1, 1), (0, 1)]
        );
        // Turned on its side, then each 2-wide row read right to left.
        assert_eq!(
            pixels(&mirrored(1)),
            [(0, 0), (0, 1), (1, 0), (1, 1), (2, 0), (2, 1)]
        );
        // A half turn mirrored is a vertical flip.
        assert_eq!(
            pixels(&mirrored(2)),
            [(0, 1), (1, 1), (2, 1), (0, 0), (1, 0), (2, 0)]
        );
    }

    #[test]
    fn four_quarter_turns_are_the_identity_and_two_mirrors_cancel() {
        let (data, stride) = picture();
        let stored = orient(3, 2, Orientation::default(), |y| &data[y * stride..]);
        let mut rgba = stored.clone();
        let (mut width, mut height) = (3, 2);
        for _ in 0..4 {
            rgba = orient(width, height, shown(1, false), |y| &rgba[y * width * 4..]);
            (width, height) = (height, width);
        }
        assert_eq!(rgba, stored);
        let once = orient(3, 2, shown(0, true), |y| &stored[y * 12..]);
        assert_eq!(orient(3, 2, shown(0, true), |y| &once[y * 12..]), stored);
    }

    #[test]
    fn seconds_divides_by_the_time_base() {
        assert_eq!(seconds(0, 1, 25), Some(0.0));
        assert_eq!(seconds(1, 1, 25), Some(0.04));
        assert_eq!(
            seconds(3003, 1001, 30_000),
            Some(3003.0 * 1001.0 / 30_000.0)
        );
        assert_eq!(seconds(-50, 1, 100), Some(-0.5));
        for (num, den) in [(1, 0), (0, 1), (-1, 25), (1, -25)] {
            assert_eq!(seconds(1, num, den), None, "{num}/{den}");
        }
    }

    #[test]
    fn frame_duration_prefers_the_frame_then_the_rate_then_30_fps() {
        let ms = Duration::from_millis;
        assert_eq!(frame_duration(Some(0.04), (25, 1)), ms(40));
        assert_eq!(frame_duration(Some(0.04), (0, 0)), ms(40));
        assert_eq!(frame_duration(None, (25, 1)), ms(40));
        assert_eq!(frame_duration(Some(0.0), (50, 1)), ms(20));
        assert_eq!(frame_duration(Some(-1.0), (50, 1)), ms(20));
        assert_eq!(
            frame_duration(None, (30_000, 1001)),
            Duration::from_secs_f64(1001.0 / 30_000.0)
        );
        let fallback = Duration::from_nanos(33_333_333);
        assert_eq!(frame_duration(None, (0, 1)), fallback);
        assert_eq!(frame_duration(None, (25, 0)), fallback);
        assert_eq!(frame_duration(None, (-25, 1)), fallback);
        assert_eq!(frame_duration(Some(f64::NAN), (0, 0)), fallback);
    }

    #[test]
    fn frame_duration_caps_a_corrupt_duration() {
        // Found by `every_frame_shows_for_a_while_but_not_forever`: these
        // rounded to a zero Duration.
        let floor = Duration::from_millis(1);
        assert_eq!(frame_duration(Some(5e-115), (0, 0)), floor);
        assert_eq!(frame_duration(None, (i32::MAX, 1)), floor);
        let cap = Duration::from_secs(10);
        assert_eq!(frame_duration(Some(1e9), (25, 1)), cap);
        assert_eq!(
            frame_duration(Some(f64::INFINITY), (25, 1)),
            Duration::from_millis(40)
        );
        assert_eq!(frame_duration(None, (1, 3600)), cap);
        assert_eq!(
            frame_duration(Some(9.5), (0, 0)),
            Duration::from_secs_f64(9.5)
        );
    }

    // --- properties ----------------------------------------------------------
    //
    // The tables above pin the cases that are easy to reason about; these hold
    // the same functions to the laws they obey for every size and orientation.

    use proptest::prelude::*;

    /// A `width`×`height` picture whose every pixel is distinct, packed.
    fn numbered(width: usize, height: usize) -> Vec<u8> {
        (0..width * height)
            .flat_map(|i| [i as u8, (i >> 8) as u8, 7, 255])
            .collect()
    }

    /// `orient` over packed rows of a `width`-wide picture.
    fn oriented(rgba: &[u8], width: usize, height: usize, orientation: Orientation) -> Vec<u8> {
        orient(width, height, orientation, |y| &rgba[y * width * 4..])
    }

    /// The size `orientation` shows a `width`×`height` picture at.
    fn shown_size(width: usize, height: usize, orientation: Orientation) -> (usize, usize) {
        if orientation.sideways() {
            (height, width)
        } else {
            (width, height)
        }
    }

    fn any_orientation() -> impl Strategy<Value = Orientation> {
        (0..4u32, any::<bool>()).prop_map(|(turns, mirror)| Orientation { turns, mirror })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn orient_moves_every_pixel_and_loses_none(
            width in 1..24usize,
            height in 1..24usize,
            orientation in any_orientation(),
        ) {
            let picture = numbered(width, height);
            let out = oriented(&picture, width, height, orientation);
            prop_assert_eq!(out.len(), picture.len());
            let mut before: Vec<_> = picture.chunks(4).collect();
            let mut after: Vec<_> = out.chunks(4).collect();
            before.sort();
            after.sort();
            prop_assert_eq!(before, after);
        }

        #[test]
        fn quarter_turns_add_up(
            width in 1..16usize,
            height in 1..16usize,
            first in 0..4u32,
            second in 0..4u32,
        ) {
            let picture = numbered(width, height);
            let once = |turns| Orientation { turns, mirror: false };
            let turned = oriented(&picture, width, height, once(first));
            let (w, h) = shown_size(width, height, once(first));
            prop_assert_eq!(
                oriented(&turned, w, h, once(second)),
                oriented(&picture, width, height, once((first + second) % 4))
            );
        }

        #[test]
        fn a_mirror_comes_after_the_turn_and_undoes_itself(
            width in 1..16usize,
            height in 1..16usize,
            turns in 0..4u32,
        ) {
            let picture = numbered(width, height);
            let mirror = Orientation { turns: 0, mirror: true };
            let turn = Orientation { turns, mirror: false };
            let turned = oriented(&picture, width, height, turn);
            let (w, h) = shown_size(width, height, turn);
            let mirrored = oriented(&turned, w, h, mirror);
            prop_assert_eq!(
                &mirrored,
                &oriented(&picture, width, height, Orientation { turns, mirror: true })
            );
            prop_assert_eq!(oriented(&mirrored, w, h, mirror), turned);
        }

        #[test]
        fn a_camera_matrix_reads_as_its_turn_and_mirror(
            quarter in -8..8i32,
            // Off a quarter turn by less than half of one.
            wobble in -44.0..44.0f64,
            hflip: bool,
            vflip: bool,
            // A matrix scaled up is the same orientation.
            scale in 1..8i32,
        ) {
            let degrees = f64::from(quarter) * 90.0;
            let upright = Orientation::from_display_matrix(&matrix(degrees, false, false));
            // A vertical flip is a horizontal one turned upside down.
            let expected = match (hflip, vflip) {
                (false, false) => upright,
                (true, false) => shown(upright.turns, true),
                (false, true) => shown((upright.turns + 2) % 4, true),
                (true, true) => shown((upright.turns + 2) % 4, false),
            };
            let mut m = matrix(degrees + wobble, hflip, vflip);
            for value in &mut m[..8] {
                *value *= scale;
            }
            prop_assert_eq!(Orientation::from_display_matrix(&m), expected);
        }

        #[test]
        fn any_matrix_reads_as_some_orientation(m in any::<[i32; 9]>()) {
            prop_assert!(Orientation::from_display_matrix(&m).turns < 4);
        }

        #[test]
        fn fit_stays_within_the_frame_and_the_limit(
            width in 1..20_000u32,
            height in 1..20_000u32,
            sideways: bool,
            max_width in 1..=16_384u32,
        ) {
            let (w, h) = fit(width, height, sideways, max_width);
            prop_assert!((1..=width).contains(&w) && (1..=height).contains(&h));
            let (shown_width, stored) = if sideways { (h, height) } else { (w, width) };
            prop_assert!(shown_width <= max_width.max(1));
            // Unscaled unless it had to be, then to the limit exactly.
            if stored <= max_width {
                prop_assert_eq!((w, h), (width, height));
            } else {
                prop_assert_eq!(shown_width, max_width);
            }
            // Both sides by the same factor, give or take rounding.
            let scale = f64::from(shown_width) / f64::from(stored);
            for (side, original) in [(w, width), (h, height)] {
                let exact = (f64::from(original) * scale).max(1.0);
                prop_assert!((f64::from(side) - exact).abs() <= 0.5 + 1e-9);
            }
        }

        #[test]
        fn seconds_needs_a_positive_time_base(ticks: i64, num: i32, den: i32) {
            let got = seconds(ticks, num, den);
            prop_assert_eq!(got.is_some(), num > 0 && den > 0);
            if let Some(got) = got {
                prop_assert!(got.is_finite());
                prop_assert_eq!(got.signum(), if ticks < 0 { -1.0 } else { 1.0 });
            }
        }

        #[test]
        fn every_frame_shows_for_a_while_but_not_forever(
            own in proptest::option::of(proptest::num::f64::ANY),
            rate: (i32, i32),
        ) {
            let duration = frame_duration(own, rate);
            prop_assert!(duration > Duration::ZERO, "{own:?} {rate:?}");
            prop_assert!(duration <= Duration::from_secs(10), "{own:?} {rate:?}");
        }
    }
}
