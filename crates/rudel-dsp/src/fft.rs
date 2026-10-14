// fft.rs - a small in-place radix-2 FFT, shared by the phase vocoder
// (`vocoder.rs`) and the convolution reverb (`convolver.rs`).
//
// Stands in for the `fft.js` instance superdough's worklets hold. Only the
// operations those need are provided: a forward complex transform and an
// inverse one that includes the `1/size` scaling (as `fft.js`'s
// `inverseTransform` does).
// SPDX-License-Identifier: AGPL-3.0-or-later

/// An in-place iterative radix-2 Cooley-Tukey FFT over a fixed power-of-two
/// size, with the bit-reversal permutation and twiddle factors precomputed.
pub struct Fft {
    size: usize,
    rev: Vec<u32>,
    /// Twiddles `e^{-2πi k / size}` for `k` in `0..size/2`.
    cos: Vec<f32>,
    sin: Vec<f32>,
}

impl Fft {
    pub fn new(size: usize) -> Fft {
        assert!(size.is_power_of_two());
        let bits = size.trailing_zeros();
        let rev = (0..size)
            // A one-sample transform has no index bits to reverse.
            .map(|i| {
                (i as u32)
                    .reverse_bits()
                    .checked_shr(32 - bits)
                    .unwrap_or(0)
            })
            .collect();
        let (cos, sin) = (0..size / 2)
            .map(|k| {
                // Computed in f64 so the twiddles are accurate to f32 at large
                // sizes; the transform itself runs in f32 like the JS one.
                let a = -std::f64::consts::TAU * k as f64 / size as f64;
                (a.cos() as f32, a.sin() as f32)
            })
            .unzip();
        Fft {
            size,
            rev,
            cos,
            sin,
        }
    }

    /// Forward transform of `(re, im)` in place.
    pub fn forward(&self, re: &mut [f32], im: &mut [f32]) {
        let n = self.size;
        debug_assert_eq!(re.len(), n);
        debug_assert_eq!(im.len(), n);
        for i in 0..n {
            let j = self.rev[i] as usize;
            if i < j {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let mut len = 2;
        while len <= n {
            let step = n / len;
            let half = len / 2;
            for start in (0..n).step_by(len) {
                for k in 0..half {
                    let (wr, wi) = (self.cos[k * step], self.sin[k * step]);
                    let (a, b) = (start + k, start + k + half);
                    let tr = re[b] * wr - im[b] * wi;
                    let ti = re[b] * wi + im[b] * wr;
                    re[b] = re[a] - tr;
                    im[b] = im[a] - ti;
                    re[a] += tr;
                    im[a] += ti;
                }
            }
            len <<= 1;
        }
    }

    /// Inverse transform of `(re, im)` in place, including the `1/size` scaling.
    pub fn inverse(&self, re: &mut [f32], im: &mut [f32]) {
        for v in im.iter_mut() {
            *v = -*v;
        }
        self.forward(re, im);
        let scale = 1.0 / self.size as f32;
        for (r, i) in re.iter_mut().zip(im.iter_mut()) {
            *r *= scale;
            *i *= -scale;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use googletest::prelude::*;
    use std::f32::consts::TAU;

    #[test]
    fn round_trips() {
        let n = 64;
        let fft = Fft::new(n);
        let orig: Vec<f32> = (0..n).map(|i| (i as f32 * 0.7).sin() + 0.3).collect();
        let mut re = orig.clone();
        let mut im = vec![0.0; n];
        fft.forward(&mut re, &mut im);
        fft.inverse(&mut re, &mut im);
        for (a, b) in re.iter().zip(&orig) {
            assert!((a - b).abs() < 1e-4, "{a} != {b}");
        }
    }

    #[gtest]
    fn a_complex_signal_round_trips_in_both_parts() {
        // The roundtrip above starts from a real signal and only checks the
        // real part, so the inverse's conjugation and its `1/n` scaling of the
        // *imaginary* half were free to be anything at all.
        let n = 64;
        let fft = Fft::new(n);
        let re0: Vec<f32> = (0..n).map(|i| (i as f32 * 0.7).sin() + 0.3).collect();
        let im0: Vec<f32> = (0..n).map(|i| (i as f32 * 0.31).cos() - 0.2).collect();
        let (mut re, mut im) = (re0.clone(), im0.clone());
        fft.forward(&mut re, &mut im);
        fft.inverse(&mut re, &mut im);
        expect_that!(re, pointwise!(|want| near(want, 1e-4), re0));
        expect_that!(im, pointwise!(|want| near(want, 1e-4), im0));
    }

    #[rstest::rstest]
    #[case(1)]
    #[case(2)]
    #[case(4)]
    #[case(8)]
    #[case(32)]
    fn complex_spectrum_matches_a_direct_dft(#[case] n: usize) {
        // A round trip alone can hide matching mistakes in forward and inverse.
        let input: Vec<(f64, f64)> = (0..n)
            .map(|i| ((i as f64 * 0.7).sin(), (i as f64 * 0.31).cos()))
            .collect();
        let expected: Vec<(f32, f32)> = (0..n)
            .map(|k| {
                let (mut re, mut im) = (0.0, 0.0);
                for (j, &(r, i)) in input.iter().enumerate() {
                    let angle = -std::f64::consts::TAU * (j * k) as f64 / n as f64;
                    re += r * angle.cos() - i * angle.sin();
                    im += r * angle.sin() + i * angle.cos();
                }
                (re as f32, im as f32)
            })
            .collect();
        let mut re: Vec<f32> = input.iter().map(|x| x.0 as f32).collect();
        let mut im: Vec<f32> = input.iter().map(|x| x.1 as f32).collect();
        Fft::new(n).forward(&mut re, &mut im);
        assert_that!(re, pointwise!(|x: &(f32, f32)| near(x.0, 1e-4), &expected));
        assert_that!(im, pointwise!(|x: &(f32, f32)| near(x.1, 1e-4), &expected));
    }

    #[test]
    fn a_sine_peaks_at_its_bin() {
        let n = 256;
        let fft = Fft::new(n);
        // Exactly 8 cycles over the window, so bin 8 should hold all the energy.
        let mut re: Vec<f32> = (0..n)
            .map(|i| (TAU * 8.0 * i as f32 / n as f32).sin())
            .collect();
        let mut im = vec![0.0; n];
        fft.forward(&mut re, &mut im);
        let mags: Vec<f32> = (0..n / 2)
            .map(|k| (re[k] * re[k] + im[k] * im[k]).sqrt())
            .collect();
        let peak = (0..n / 2)
            .max_by(|a, b| mags[*a].partial_cmp(&mags[*b]).unwrap())
            .unwrap();
        assert_eq!(peak, 8);
    }

    #[test]
    fn convolution_theorem_holds() {
        // Multiplying spectra convolves the time-domain signals (circularly).
        let n = 16;
        let fft = Fft::new(n);
        let a: Vec<f32> = (0..n)
            .map(|i| if i < 4 { i as f32 + 1.0 } else { 0.0 })
            .collect();
        let b: Vec<f32> = (0..n)
            .map(|i| if i < 3 { (i as f32) * 0.5 } else { 0.0 })
            .collect();

        let (mut ar, mut ai) = (a.clone(), vec![0.0; n]);
        let (mut br, mut bi) = (b.clone(), vec![0.0; n]);
        fft.forward(&mut ar, &mut ai);
        fft.forward(&mut br, &mut bi);
        let mut cr = vec![0.0; n];
        let mut ci = vec![0.0; n];
        for k in 0..n {
            cr[k] = ar[k] * br[k] - ai[k] * bi[k];
            ci[k] = ar[k] * bi[k] + ai[k] * br[k];
        }
        fft.inverse(&mut cr, &mut ci);

        for k in 0..n {
            let want: f32 = (0..=k).map(|j| a[j] * b[k - j]).sum();
            assert!((cr[k] - want).abs() < 1e-3, "tap {k}: {} != {want}", cr[k]);
        }
    }
}
