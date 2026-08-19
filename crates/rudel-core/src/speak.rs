// speak.rs - say a pattern's values out loud, ported from
// strudel/packages/core/speak.mjs.
//
// Upstream's is an `onTrigger` closed over `lang`/`voice` that reaches the
// browser's `speechSynthesis` directly. Rudel's query path cannot hold a
// closure that far, and the speaking happens on another thread entirely, so the
// request travels as controls instead — the same shape `csound` uses to hand a
// hap to an engine that is not the mixer.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    pattern::Pattern,
    transforms::{IntoPattern, core::patternify::patternify_value2},
    value::{Value, ValueMap},
};

/// The words a hap asks to have spoken.
pub const SPEAK: &str = "speak";
/// BCP-47 language tag to pick a voice by, as upstream's `utterance.lang`.
pub const SPEAK_LANG: &str = "speaklang";
/// Which of that language's voices: an index, or a voice name.
pub const SPEAK_VOICE: &str = "speakvoice";

/// The text `SpeechSynthesisUtterance` would be constructed with — a hap's
/// value stringified. A control map has no sensible reading as a sentence
/// (upstream would utter "[object Object]"), so its `value` entry stands in.
fn words(value: &Value) -> Option<String> {
    match value {
        Value::Str(s) => Some(s.clone()),
        Value::Int(n) => Some(n.to_string()),
        Value::F64(x) => Some(x.to_string()),
        Value::Frac(f) => Some(f.to_f64().to_string()),
        Value::Map(m) => m.get("value").and_then(words),
        _ => None,
    }
}

/// Whether this hap asks to be spoken rather than played. Upstream's
/// `onTrigger` is *dominant*, so a spoken hap makes no sound of its own.
pub fn is_speech(value: &Value) -> bool {
    matches!(value, Value::Map(m) if m.contains_key(SPEAK))
}

/// `speak(lang, voice)` for one sampled pair.
fn speak_one(pat: &Pattern, lang: &Value, voice: &Value) -> Pattern {
    let (lang, voice) = (lang.clone(), voice.clone());
    pat.fmap(move |v| {
        let Some(words) = words(&v) else {
            return v;
        };
        let mut map = match v {
            Value::Map(m) => m,
            _ => ValueMap::new(),
        };
        map.insert(SPEAK.to_string(), Value::Str(words));
        // `null` is how a script asks for the default voice, and upstream's
        // `typeof voice` tests then match neither branch.
        if !matches!(lang, Value::Null) {
            map.insert(SPEAK_LANG.to_string(), lang.clone());
        }
        if !matches!(voice, Value::Null) {
            map.insert(SPEAK_VOICE.to_string(), voice.clone());
        }
        Value::Map(map)
    })
}

impl Pattern {
    /// `speak(lang, voice)` (core/speak.mjs): say each hap's value out loud
    /// with the platform's speech synthesiser, instead of playing it.
    ///
    /// `lang` filters the installed voices (upstream: `v.lang.includes(lang)`)
    /// and `voice` picks among what is left, by index or by name. Both may be
    /// `null` for the system default, and both are patternable.
    pub fn speak(&self, lang: impl IntoPattern, voice: impl IntoPattern) -> Pattern {
        patternify_value2(self, lang.into_pattern(), voice.into_pattern(), speak_one)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{fraction::Frac, pattern::pure};

    #[test]
    fn speak_marks_the_words_and_the_voice() {
        let pat = pure(Value::Str("i am".into())).speak(Value::Str("en".into()), Value::Int(2));
        let haps = pat.query_arc(Frac::zero(), Frac::one());
        let Value::Map(m) = &haps[0].value else {
            panic!("expected a control map, got {:?}", haps[0].value)
        };
        assert_eq!(m.get(SPEAK).and_then(|v| v.as_str()), Some("i am"));
        assert_eq!(m.get(SPEAK_LANG).and_then(|v| v.as_str()), Some("en"));
        assert_eq!(m.get(SPEAK_VOICE).and_then(|v| v.as_f64()), Some(2.0));
    }

    #[test]
    fn a_null_voice_leaves_the_choice_to_the_system() {
        let pat = pure(Value::Str("hello".into())).speak(Value::Null, Value::Null);
        let haps = pat.query_arc(Frac::zero(), Frac::one());
        let Value::Map(m) = &haps[0].value else {
            panic!("expected a control map")
        };
        assert_eq!(m.get(SPEAK).and_then(|v| v.as_str()), Some("hello"));
        assert!(m.get(SPEAK_LANG).is_none() && m.get(SPEAK_VOICE).is_none());
    }

    #[test]
    fn the_voice_may_be_patterned() {
        // `"<[i am] here>".speak('en', "<2 3>")` — a per-cycle voice, which is
        // what `register`'s patternification buys and what pins the arity-3
        // `appLeft` shape rather than a plain `fmap`.
        let pat = pure(Value::Str("x".into())).speak(
            Value::Str("en".into()),
            crate::pattern::slowcat(&[pure(Value::Int(2)), pure(Value::Int(3))]),
        );
        let voice = |cycle: i64| {
            let haps = pat.query_arc(Frac::int(cycle), Frac::int(cycle + 1));
            match &haps[0].value {
                Value::Map(m) => m.get(SPEAK_VOICE).and_then(|v| v.as_f64()),
                other => panic!("expected a control map, got {other:?}"),
            }
        };
        assert_eq!((voice(0), voice(1)), (Some(2.0), Some(3.0)));
    }
}
