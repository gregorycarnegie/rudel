use crate::{
    pattern::Pattern,
    transforms::IntoPattern,
    value::{Value, ValueMap},
    xen::freq_to_midi,
};

impl Pattern {
    /// `hsl(h, s, l)`: set the `color` control to a CSS `hsl(...)` string built
    /// from hue (in turns), saturation and lightness (each `0..1`). Mirrors
    /// Strudel's `register('hsl', ...)`: `h` is the structural argument, `s`/`l`
    /// are sampled by `appLeft`, then `innerJoin`ed onto the coloured pattern.
    pub fn hsl(&self, h: impl IntoPattern, s: impl IntoPattern, l: impl IntoPattern) -> Pattern {
        let pat = self.clone();
        h.into_pattern()
            .fmap(move |hv| {
                let pat = pat.clone();
                Value::func(move |sv| {
                    let pat = pat.clone();
                    let hv = hv.clone();
                    Value::func(move |lv| {
                        let css = hsl_css(&hv, &sv, &lv, None);
                        Value::Pat(Box::new(pat.color(Value::Str(css))))
                    })
                })
            })
            .app_left(&s.into_pattern())
            .app_left(&l.into_pattern())
            .inner_join()
    }

    /// `hsla(h, s, l, a)`: like [`hsl`](Self::hsl) but with an extra alpha
    /// channel (`0..1`), writing a CSS `hsla(...)` string to the `color` control.
    pub fn hsla(
        &self,
        h: impl IntoPattern,
        s: impl IntoPattern,
        l: impl IntoPattern,
        a: impl IntoPattern,
    ) -> Pattern {
        let pat = self.clone();
        h.into_pattern()
            .fmap(move |hv| {
                let pat = pat.clone();
                Value::func(move |sv| {
                    let pat = pat.clone();
                    let hv = hv.clone();
                    Value::func(move |lv| {
                        let pat = pat.clone();
                        let hv = hv.clone();
                        let sv = sv.clone();
                        Value::func(move |av| {
                            let css = hsl_css(&hv, &sv, &lv, Some(&av));
                            Value::Pat(Box::new(pat.color(Value::Str(css))))
                        })
                    })
                })
            })
            .app_left(&s.into_pattern())
            .app_left(&l.into_pattern())
            .app_left(&a.into_pattern())
            .inner_join()
    }

    /// Strudel's `piano()` convenience: select the piano sample bank, set a
    /// short release and a clip of 1, then spread notes gently by pitch.
    ///
    /// The clip is *set*, not defaulted. Upstream opens with `this.clip(1)`,
    /// which overwrites whatever the chain had already put there — an echo that
    /// shortens each repeat with `.clip(1/(i+1))` before reaching `.piano()`
    /// ends up at 1 all the same. Filling it in only when absent left those
    /// repeats clipped.
    pub fn piano(&self) -> Pattern {
        self.clip(1).s("piano").release(0.1).fmap(|v| match v {
            Value::Map(mut m) => {
                let pan = piano_pan(&m);
                if let Some(pan) = pan {
                    let existing = m.get("pan").and_then(Value::as_f64).unwrap_or(1.0);
                    m.insert("pan".to_string(), Value::F64(existing * pan));
                }
                Value::Map(m)
            }
            other => other,
        })
    }
}

/// Format an `hsl(...)`/`hsla(...)` CSS colour string. Saturation and lightness
/// are scaled from `0..1` to percentages; hue is expressed in turns.
fn hsl_css(h: &Value, s: &Value, l: &Value, a: Option<&Value>) -> String {
    let h = h.as_f64().unwrap_or(0.0);
    let s = s.as_f64().unwrap_or(0.0) * 100.0;
    let l = l.as_f64().unwrap_or(0.0) * 100.0;
    match a {
        Some(a) => format!("hsla({h}turn,{s}%,{l}%,{})", a.as_f64().unwrap_or(1.0)),
        None => format!("hsl({h}turn,{s}%,{l}%)"),
    }
}

fn piano_pan(m: &ValueMap) -> Option<f64> {
    let midi = m
        .get("note")
        .and_then(value_to_midi)
        .or_else(|| m.get("freq").and_then(|v| v.as_f64().map(freq_to_midi)))?;
    let max_pan = crate::tonal::note_to_midi("C8")? as f64;
    let pitch_pan = (midi.round() / max_pan).clamp(0.0, 1.0);
    Some(pitch_pan * 0.5 + 0.25)
}

fn value_to_midi(value: &Value) -> Option<f64> {
    match value {
        Value::Str(s) => crate::tonal::note_to_midi(s).map(|m| m as f64),
        other => other.as_f64(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsl_css_scales_saturation_and_lightness_to_percentages() {
        let h = Value::F64(0.25);
        let s = Value::F64(0.5);
        let l = Value::F64(0.4);
        assert_eq!(hsl_css(&h, &s, &l, None), "hsl(0.25turn,50%,40%)");
        assert_eq!(
            hsl_css(&h, &s, &l, Some(&Value::F64(0.75))),
            "hsla(0.25turn,50%,40%,0.75)"
        );
        // Non-numeric channels fall back to 0, and alpha to fully opaque.
        assert_eq!(
            hsl_css(&Value::Null, &Value::Null, &Value::Null, Some(&Value::Null)),
            "hsla(0turn,0%,0%,1)"
        );
    }

    /// `piano()` *multiplies* an existing `pan` by the pitch spread rather
    /// than replacing or offsetting it, so an explicit pan still dominates.
    #[test]
    fn piano_scales_any_pan_already_set() {
        let pan_of = |pat: &Pattern| -> f64 {
            let hap = &pat.query_arc(crate::Frac::zero(), crate::Frac::one())[0];
            let Value::Map(m) = &hap.value else {
                panic!("piano() should emit a control map")
            };
            m.get("pan").and_then(Value::as_f64).expect("pan set")
        };
        // midi 72 of a 108-note range spreads to 72/108 * 0.5 + 0.25.
        let spread = 72.0 / 108.0 * 0.5 + 0.25;
        // Default pan is 1.0, so the spread comes through unchanged.
        let plain = crate::controls::note(72).piano();
        assert!((pan_of(&plain) - spread).abs() < 1e-9, "{}", pan_of(&plain));
        // Halving the pan halves the result — an offset or a divide would not.
        let halved = crate::controls::note(72).pan(0.5).piano();
        assert!(
            (pan_of(&halved) - spread * 0.5).abs() < 1e-9,
            "{}",
            pan_of(&halved)
        );
    }

    #[test]
    fn value_to_midi_reads_note_names_and_numbers() {
        assert_eq!(value_to_midi(&Value::Str("c5".to_string())), Some(72.0));
        assert_eq!(value_to_midi(&Value::Int(60)), Some(60.0));
        assert_eq!(value_to_midi(&Value::F64(60.5)), Some(60.5));
        assert_eq!(value_to_midi(&Value::Str("zz".to_string())), None);
        assert_eq!(value_to_midi(&Value::Null), None);
    }

    /// `piano()` spreads notes across the middle half of the stereo field —
    /// low notes left of centre, high notes right, never hard-panned.
    #[test]
    fn piano_pan_maps_pitch_into_the_middle_half() {
        let with = |k: &str, v: Value| {
            let mut m = ValueMap::new();
            m.insert(k.to_string(), v);
            m
        };
        // C8 (midi 108) is the top of the range: fully right of the window.
        assert_eq!(piano_pan(&with("note", Value::Int(108))), Some(0.75));
        // Midi 0 is the bottom.
        assert_eq!(piano_pan(&with("note", Value::Int(0))), Some(0.25));
        // Halfway up lands at centre.
        assert_eq!(piano_pan(&with("note", Value::Int(54))), Some(0.5));
        // Note names and `freq` are accepted too; 440Hz is midi 69.
        assert_eq!(
            piano_pan(&with("note", Value::Str("c5".to_string()))),
            piano_pan(&with("note", Value::Int(72)))
        );
        assert_eq!(
            piano_pan(&with("freq", Value::F64(440.0))),
            piano_pan(&with("note", Value::Int(69)))
        );
        // Past the top of the range it clamps rather than panning off-stage.
        assert_eq!(piano_pan(&with("note", Value::Int(127))), Some(0.75));
        // No pitch at all: nothing to pan by.
        assert_eq!(piano_pan(&ValueMap::new()), None);
    }
}
