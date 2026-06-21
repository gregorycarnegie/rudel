use crate::pattern::Pattern;
use crate::transforms::IntoPattern;
use crate::value::{Value, ValueMap};
use crate::xen::freq_to_midi;

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
    /// short release and default clip, then spread notes gently by pitch.
    pub fn piano(&self) -> Pattern {
        self.s("piano").release(0.1).fmap(|v| match v {
            Value::Map(mut m) => {
                let pan = piano_pan(&m);
                m.entry("clip".to_string()).or_insert(Value::Int(1));
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
