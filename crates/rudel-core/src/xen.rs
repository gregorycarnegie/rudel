// xen.rs - Strudel-compatible xenharmonic helpers and transforms.
// SPDX-License-Identifier: AGPL-3.0-or-later
#![allow(non_snake_case)]

use crate::hap::Hap;
use crate::pattern::{Pattern, silence};
use crate::transforms::IntoPattern;
use crate::value::Value;
use std::collections::BTreeMap;

const DEFAULT_BASE: f64 = 220.0;

const JI_12: &[f64] = &[
    1.0,
    16.0 / 15.0,
    9.0 / 8.0,
    6.0 / 5.0,
    5.0 / 4.0,
    4.0 / 3.0,
    45.0 / 32.0,
    3.0 / 2.0,
    8.0 / 5.0,
    5.0 / 3.0,
    16.0 / 9.0,
    15.0 / 8.0,
];

#[derive(Clone, Debug)]
struct XenScale {
    ratios: Vec<f64>,
    edo_size: Option<f64>,
}

/// Convert a MIDI note number to frequency in Hz (`a4 = 69 = 440Hz`).
pub fn midi_to_freq(midi: f64) -> f64 {
    440.0 * 2f64.powf((midi - 69.0) / 12.0)
}

/// Convert a frequency in Hz to a fractional MIDI note number.
pub fn freq_to_midi(freq: f64) -> f64 {
    12.0 * (freq / 440.0).log2() + 69.0
}

/// Convert a note name or MIDI-number-like value to frequency in Hz.
pub fn get_freq(value: &Value) -> Option<f64> {
    match value {
        Value::Str(s) => s
            .parse::<f64>()
            .ok()
            .or_else(|| crate::tonal::note_to_midi(s).map(|m| m as f64))
            .map(midi_to_freq),
        other => other.as_f64().map(midi_to_freq),
    }
}

/// Return the octave-normalized ratios for an EDO scale such as `"31edo"`.
pub fn edo_ratios(name: &str) -> Option<Vec<f64>> {
    let divisions = edo_divisions(name)?;
    let divisions_f = divisions as f64;
    Some(
        (0..divisions)
            .map(|i| 2f64.powf(i as f64 / divisions_f))
            .collect(),
    )
}

fn edo_divisions(name: &str) -> Option<usize> {
    let digits = name.strip_suffix("edo")?;
    if digits.is_empty() || digits.starts_with('0') || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let n = digits.parse::<usize>().ok()?;
    (n > 0).then_some(n)
}

fn tune_freqs(name: &str) -> Option<&'static [f64]> {
    let scales = crate::tune_table::TUNE_SCALES;
    scales
        .binary_search_by(|scale| scale.name.cmp(name))
        .ok()
        .map(|idx| scales[idx].freqs)
}

fn numeric_list(value: &Value) -> Option<Vec<f64>> {
    match value {
        Value::List(items) => items.iter().map(Value::as_f64).collect(),
        _ => None,
    }
}

fn ratios_from_frequencies(freqs: &[f64]) -> Option<Vec<f64>> {
    let first = *freqs.first()?;
    if first == 0.0 {
        return None;
    }
    let stop = freqs.len().saturating_sub(1);
    let ratios: Vec<f64> = freqs.iter().take(stop.max(1)).map(|f| f / first).collect();
    (!ratios.is_empty()).then_some(ratios)
}

fn tune_scale_from_value(value: &Value) -> Option<XenScale> {
    match value {
        Value::Str(name) => {
            let ratios = match name.as_str() {
                "12ji" => JI_12.to_vec(),
                _ => ratios_from_frequencies(tune_freqs(name)?)?,
            };
            Some(XenScale {
                ratios,
                edo_size: None,
            })
        }
        Value::List(_) => {
            let freqs = numeric_list(value)?;
            Some(XenScale {
                ratios: ratios_from_frequencies(&freqs)?,
                edo_size: None,
            })
        }
        _ => None,
    }
}

fn xen_scale_from_value(value: &Value) -> Option<XenScale> {
    match value {
        Value::Str(name) => {
            if let Some(ratios) = edo_ratios(name) {
                return Some(XenScale {
                    ratios,
                    edo_size: edo_divisions(name).map(|n| n as f64),
                });
            }
            let ratios = match name.as_str() {
                "12ji" => JI_12.to_vec(),
                _ => ratios_from_frequencies(tune_freqs(name)?)?,
            };
            Some(XenScale {
                ratios,
                edo_size: None,
            })
        }
        Value::List(_) => Some(XenScale {
            ratios: numeric_list(value)?,
            edo_size: None,
        }),
        _ => None,
    }
}

fn trim_precision_10(x: f64) -> f64 {
    if !x.is_finite() || x == 0.0 {
        return x;
    }
    format!("{x:.9e}").parse::<f64>().unwrap_or(x)
}

fn tune_floor(x: f64) -> f64 {
    (x * 100_000_000_000.0).floor() / 100_000_000_000.0
}

fn scale_offset(scale: &[f64], offset: f64) -> Option<f64> {
    if scale.is_empty() {
        return None;
    }
    let offset = offset.trunc() as i64;
    let len = scale.len() as i64;
    let index = offset.rem_euclid(len) as usize;
    let octave = offset.div_euclid(len);
    Some(scale[index] * 2f64.powi(octave as i32))
}

fn apply_xen_to_hap(hap: Hap, scale: &XenScale) -> Option<Hap> {
    let Hap {
        whole,
        part,
        value,
        mut context,
    } = hap;
    let Value::Map(mut m) = value else {
        return None;
    };
    let step = m.remove("i").and_then(|v| v.as_f64())?;
    let freq = trim_precision_10(DEFAULT_BASE * scale_offset(&scale.ratios, step)?);
    m.insert("freq".to_string(), Value::F64(freq));
    if let Some(edo_size) = scale.edo_size {
        context.edo_size = Some(edo_size);
    }
    Some(Hap {
        whole,
        part,
        value: Value::Map(m),
        context,
    })
}

fn apply_tune_to_hap(hap: Hap, scale: &XenScale) -> Option<Hap> {
    let Hap {
        whole,
        part,
        value,
        context,
    } = hap;
    let Value::Map(m) = value else {
        return None;
    };
    let step = m.get("i").and_then(Value::as_f64)?;
    let ratio = tune_floor(scale_offset(&scale.ratios, step)?);
    Some(Hap {
        whole,
        part,
        value: Value::F64(ratio),
        context,
    })
}

/// Interpret a value as a numeral (Strudel's `parseNumeral`): numbers pass
/// through; note-name strings convert to MIDI; otherwise 0.
fn numeral(v: &Value) -> f64 {
    match v {
        Value::Str(s) => s
            .parse::<f64>()
            .ok()
            .or_else(|| crate::tonal::note_to_midi(s).map(|m| m as f64))
            .unwrap_or(0.0),
        other => other.as_f64().unwrap_or(0.0),
    }
}

fn base_pair(value: &Value) -> (f64, f64) {
    match value {
        Value::List(items) if !items.is_empty() => {
            let base = items
                .first()
                .and_then(Value::as_f64)
                .unwrap_or(DEFAULT_BASE);
            let original = items.get(1).and_then(Value::as_f64).unwrap_or(DEFAULT_BASE);
            (base, original)
        }
        other => (other.as_f64().unwrap_or(DEFAULT_BASE), DEFAULT_BASE),
    }
}

fn rescale_freq_value(value: Value, base: f64, original: f64) -> Value {
    let factor = if original == 0.0 {
        1.0
    } else {
        base / original
    };
    match value {
        Value::Map(mut m) => {
            if let Some(freq) = m.get("freq").and_then(Value::as_f64) {
                m.insert("freq".to_string(), Value::F64(freq * factor));
            }
            Value::Map(m)
        }
        other => Value::F64(other.as_f64().unwrap_or(0.0) * factor),
    }
}

fn frequency_from_value(value: Value) -> (bool, BTreeMap<String, Value>, f64) {
    match value {
        Value::Map(mut m) => {
            let freq = m.remove("freq").and_then(|v| v.as_f64()).unwrap_or(0.0);
            (true, m, freq)
        }
        other => (false, BTreeMap::new(), other.as_f64().unwrap_or(0.0)),
    }
}

fn ftrans_amount(value: &Value) -> (f64, Option<f64>) {
    match value {
        Value::List(items) => (
            items.first().and_then(Value::as_f64).unwrap_or(0.0),
            items.get(1).and_then(Value::as_f64),
        ),
        Value::Str(s) => {
            if let Some((steps, edo)) = s.split_once(':') {
                return (
                    steps.trim().parse::<f64>().unwrap_or(0.0),
                    edo.trim().parse::<f64>().ok(),
                );
            }
            (s.parse::<f64>().unwrap_or(0.0), None)
        }
        other => (other.as_f64().unwrap_or(0.0), None),
    }
}

fn ftrans_hap(hap: Hap, steps: f64, explicit_edo_size: Option<f64>) -> Hap {
    let Hap {
        whole,
        part,
        value,
        mut context,
    } = hap;
    let edo_size = explicit_edo_size.or(context.edo_size).unwrap_or(12.0);
    let (was_map, mut rest, freq) = frequency_from_value(value);
    let freq = trim_precision_10(freq * 2f64.powf(steps / edo_size));
    context.edo_size = Some(edo_size);
    let value = if was_map {
        rest.insert("freq".to_string(), Value::F64(freq));
        Value::Map(rest)
    } else {
        Value::F64(freq)
    };
    Hap {
        whole,
        part,
        value,
        context,
    }
}

impl Pattern {
    /// Tune.js lookup. Expects an `i` control and returns frequency ratios.
    pub fn tune(&self, scale: impl IntoPattern) -> Pattern {
        let arg = scale.into_pattern();
        if let Some(v) = &arg.pure_value {
            return self.apply_tune((**v).clone());
        }
        let pat = self.clone();
        arg.fmap(move |v| Value::Pat(Box::new(pat.apply_tune(v))))
            .inner_join()
    }

    fn apply_tune(&self, scale_value: Value) -> Pattern {
        let Some(scale) = tune_scale_from_value(&scale_value) else {
            return silence();
        };
        let steps = self.steps;
        self.with_haps(move |haps, _| {
            haps.into_iter()
                .filter_map(|hap| apply_tune_to_hap(hap, &scale))
                .collect()
        })
        .set_steps(steps)
    }

    /// Map `i` controls into frequencies using an EDO, Tune.js scale, preset, or
    /// explicit ratio list.
    pub fn xen(&self, scale: impl IntoPattern) -> Pattern {
        let arg = scale.into_pattern();
        if let Some(v) = &arg.pure_value {
            return self.apply_xen((**v).clone());
        }
        let pat = self.clone();
        arg.fmap(move |v| Value::Pat(Box::new(pat.apply_xen(v))))
            .inner_join()
    }

    fn apply_xen(&self, scale_value: Value) -> Pattern {
        let Some(scale) = xen_scale_from_value(&scale_value) else {
            return silence();
        };
        let steps = self.steps;
        self.with_haps(move |haps, _| {
            haps.into_iter()
                .filter_map(|hap| apply_xen_to_hap(hap, &scale))
                .collect()
        })
        .set_steps(steps)
    }

    /// Rescale frequency values from 220Hz or `[base, originalBase]`.
    pub fn with_base(&self, base: impl IntoPattern) -> Pattern {
        let arg = base.into_pattern();
        if let Some(v) = &arg.pure_value {
            let (base, original) = base_pair(v);
            return self.with_value(move |value| rescale_freq_value(value, base, original));
        }
        let pat = self.clone();
        arg.fmap(move |v| {
            let (base, original) = base_pair(&v);
            Value::Pat(Box::new(pat.with_value(move |value| {
                rescale_freq_value(value, base, original)
            })))
        })
        .inner_join()
    }

    /// Frequency transpose by EDO steps. The amount may be `[steps, edo]` or
    /// `steps`; EDO falls back to hap context, then 12.
    pub fn ftrans(&self, amount: impl IntoPattern) -> Pattern {
        let arg = amount.into_pattern();
        if let Some(v) = &arg.pure_value {
            let (steps, edo_size) = ftrans_amount(v);
            return self.with_hap(move |hap| ftrans_hap(hap, steps, edo_size));
        }
        let pat = self.clone();
        arg.fmap(move |v| {
            let (steps, edo_size) = ftrans_amount(&v);
            Value::Pat(Box::new(
                pat.with_hap(move |hap| ftrans_hap(hap, steps, edo_size)),
            ))
        })
        .inner_join()
    }

    /// Alias for [`with_base`](Self::with_base) with Strudel's camelCase name.
    pub fn withBase(&self, base: impl IntoPattern) -> Pattern {
        self.with_base(base)
    }

    /// Alias for [`ftrans`](Self::ftrans).
    pub fn fTrans(&self, amount: impl IntoPattern) -> Pattern {
        self.ftrans(amount)
    }

    /// Alias for [`ftrans`](Self::ftrans).
    pub fn ftranspose(&self, amount: impl IntoPattern) -> Pattern {
        self.ftrans(amount)
    }

    /// Alias for [`ftrans`](Self::ftrans).
    pub fn fTranspose(&self, amount: impl IntoPattern) -> Pattern {
        self.ftrans(amount)
    }

    /// Map each bare hap value through a ratio list (`tuning`). The proto-`xen`
    /// from `xen.mjs`: like [`xen`](Self::xen) but it reads the value directly as
    /// the scale index (no `i` control) and returns the raw ratio (no 220Hz base).
    pub fn tuning(&self, ratios: impl IntoPattern) -> Pattern {
        let arg = ratios.into_pattern();
        if let Some(v) = &arg.pure_value {
            return match numeric_list(v) {
                Some(r) => self.apply_tuning(r),
                None => silence(),
            };
        }
        let pat = self.clone();
        arg.fmap(move |v| {
            Value::Pat(Box::new(match numeric_list(&v) {
                Some(r) => pat.apply_tuning(r),
                None => silence(),
            }))
        })
        .inner_join()
    }

    fn apply_tuning(&self, ratios: Vec<f64>) -> Pattern {
        self.with_value(move |value| match scale_offset(&ratios, numeral(&value)) {
            Some(freq) => Value::F64(freq),
            None => value,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Frac, i, pure, sequence};

    fn freqs(pat: &Pattern) -> Vec<f64> {
        let mut haps = pat.query_arc(Frac::zero(), Frac::one());
        haps.sort_by_key(|h| h.part.begin);
        haps.into_iter()
            .map(|h| match h.value {
                Value::Map(m) => m.get("freq").and_then(Value::as_f64).unwrap(),
                other => other.as_f64().unwrap(),
            })
            .collect()
    }

    fn approx_eq(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-8, "expected {b}, got {a}");
    }

    #[test]
    fn tune_scales_are_sorted_for_binary_search() {
        for pair in crate::tune_table::TUNE_SCALES.windows(2) {
            assert!(
                pair[0].name < pair[1].name,
                "tune scales must stay strictly sorted for binary search: {} before {}",
                pair[0].name,
                pair[1].name
            );
        }
    }

    #[test]
    fn edo_ratios_make_octave_steps() {
        let got = edo_ratios("3edo").unwrap();
        approx_eq(got[0], 1.0);
        approx_eq(got[1], 2f64.powf(1.0 / 3.0));
        approx_eq(got[2], 2f64.powf(2.0 / 3.0));
    }

    #[test]
    fn xen_edo_maps_i_to_freq() {
        let pat = i(sequence(&[
            pure(Value::Int(0)),
            pure(Value::Int(8)),
            pure(Value::Int(18)),
        ]))
        .xen("31edo");
        let got = freqs(&pat);
        approx_eq(got[0], 220.0);
        approx_eq(got[1], trim_precision_10(220.0 * 2f64.powf(8.0 / 31.0)));
        approx_eq(got[2], trim_precision_10(220.0 * 2f64.powf(18.0 / 31.0)));
        assert_eq!(
            pat.query_arc(Frac::zero(), Frac::one())[0].context.edo_size,
            Some(31.0)
        );
    }

    #[test]
    fn negative_xen_steps_wrap_down_octaves() {
        let got = freqs(&i(pure(Value::Int(-1))).xen(Value::List(vec![
            Value::F64(1.0),
            Value::F64(5.0 / 4.0),
            Value::F64(3.0 / 2.0),
        ])));
        approx_eq(got[0], 220.0 * (3.0 / 2.0) / 2.0);
    }

    #[test]
    fn tune_accepts_named_archive_and_frequency_arrays() {
        let named = i(pure(Value::Int(0))).tune("hexany15");
        assert_eq!(freqs(&named), vec![1.0]);

        let array =
            i(sequence(&[pure(Value::Int(0)), pure(Value::Int(1))])).tune(Value::List(vec![
                Value::F64(440.0),
                Value::F64(550.0),
                Value::F64(880.0),
            ]));
        let got = freqs(&array);
        approx_eq(got[0], 1.0);
        approx_eq(got[1], 1.25);
    }

    #[test]
    fn with_base_rescales_default_or_explicit_original() {
        let pat = i(pure(Value::Int(0))).xen("12edo").with_base(440.0);
        approx_eq(freqs(&pat)[0], 440.0);

        let pat = i(pure(Value::Int(0)))
            .xen("12edo")
            .with_base(Value::List(vec![Value::F64(440.0), Value::F64(110.0)]));
        approx_eq(freqs(&pat)[0], 880.0);
    }

    #[test]
    fn tuning_maps_bare_value_through_ratios() {
        // tuning reads the bare value as the scale index and returns the ratio
        // (no `i` control, no 220Hz base): 0->1, 1->5/4, 2->3/2, 3->ratio[0]*2.
        let pat = sequence(&[
            pure(Value::Int(0)),
            pure(Value::Int(1)),
            pure(Value::Int(2)),
            pure(Value::Int(3)),
        ])
        .tuning(Value::List(vec![
            Value::F64(1.0),
            Value::F64(5.0 / 4.0),
            Value::F64(3.0 / 2.0),
        ]));
        let got = freqs(&pat);
        approx_eq(got[0], 1.0);
        approx_eq(got[1], 5.0 / 4.0);
        approx_eq(got[2], 3.0 / 2.0);
        approx_eq(got[3], 2.0);
    }

    #[test]
    fn ftrans_uses_explicit_context_then_default_edo() {
        let pat = i(pure(Value::Int(0))).xen("31edo").ftrans(7.0);
        approx_eq(
            freqs(&pat)[0],
            trim_precision_10(220.0 * 2f64.powf(7.0 / 31.0)),
        );

        let pat = crate::freq(pure(Value::F64(200.0)))
            .ftrans(Value::List(vec![Value::F64(7.0), Value::F64(31.0)]));
        approx_eq(
            freqs(&pat)[0],
            trim_precision_10(200.0 * 2f64.powf(7.0 / 31.0)),
        );

        let pat = crate::freq(pure(Value::F64(200.0))).ftrans(7.0);
        approx_eq(
            freqs(&pat)[0],
            trim_precision_10(200.0 * 2f64.powf(7.0 / 12.0)),
        );
    }
}
