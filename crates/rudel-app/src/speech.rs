//! The engine behind `.speak(lang, voice)` — the platform's speech
//! synthesiser, standing in for the browser's `speechSynthesis`.
//!
//! `rudel_core::speak` marks a hap with the words to say and the voice to say
//! them in; this speaks them, on the same thread and at the same moment the
//! `onTriggerTime` hooks fire, which is where upstream's `onTrigger` runs too.
//!
//! Voice selection follows `core/speak.mjs` exactly: the installed voices are
//! filtered by language tag (`v.lang.includes(lang)`), and what is left is
//! indexed by number, modulo the count, or matched by name. A hap that names
//! no language leaves the choice to the system, as an undefined `lang` does
//! upstream — its filter matches nothing, so no voice is set.
//! SPDX-License-Identifier: AGPL-3.0-or-later

use rudel_core::{
    speak::{SPEAK, SPEAK_LANG, SPEAK_VOICE},
    value::Value,
};

/// How a hap asked for its voice.
#[derive(Debug, PartialEq)]
pub(crate) enum Pick {
    /// `speak('en', 2)`: the third voice of that language, wrapping.
    Index(usize),
    /// `speak('en', 'Zira')`: the voice with that name.
    Name(String),
}

/// The words, language and voice a hap asks to have spoken, or `None` if it
/// asks for nothing.
pub(crate) fn request(value: &Value) -> Option<(String, Option<String>, Option<Pick>)> {
    let Value::Map(map) = value else {
        return None;
    };
    let words = map.get(SPEAK)?.as_str()?.to_string();
    let lang = map
        .get(SPEAK_LANG)
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let pick = match map.get(SPEAK_VOICE) {
        Some(Value::Str(name)) => Some(Pick::Name(name.clone())),
        Some(other) => other
            .as_f64()
            .filter(|n| n.is_finite() && *n >= 0.0)
            .map(|n| Pick::Index(n as usize)),
        None => None,
    };
    Some((words, lang, pick))
}

/// A speech synthesiser, opened on first use.
#[derive(Default)]
pub(crate) struct Speech {
    engine: Option<backend::Engine>,
    /// Set once opening has been tried and failed, so it is not retried on
    /// every hap. The message is shown in the app's error line.
    failed: Option<String>,
}

impl Speech {
    /// Say `words`, replacing anything still being spoken — upstream cancels
    /// the previous utterance before every new one.
    ///
    /// Returns the reason nothing was said, once per session: a machine with no
    /// speech synthesiser should not turn every hap into an error.
    pub(crate) fn say(
        &mut self,
        words: &str,
        lang: Option<&str>,
        pick: Option<&Pick>,
    ) -> Option<String> {
        if self.failed.is_some() {
            return None;
        }
        if self.engine.is_none() {
            match backend::Engine::open() {
                Ok(engine) => self.engine = Some(engine),
                Err(why) => {
                    self.failed = Some(why.clone());
                    return Some(format!("speak: {why}"));
                }
            }
        }
        let engine = self.engine.as_ref()?;
        engine
            .say(words, lang, pick)
            .err()
            .map(|e| format!("speak: {e}"))
    }
}

/// The voices a backend found, in the order the platform lists them.
struct Installed<T> {
    /// BCP-47 tag, as a browser's `SpeechSynthesisVoice.lang` would read.
    locale: String,
    name: String,
    handle: T,
}

/// `speak.mjs`'s choice: filter by language, then index or name.
fn choose<'a, T>(
    installed: &'a [Installed<T>],
    lang: Option<&str>,
    pick: Option<&Pick>,
) -> Option<&'a T> {
    // No language means no filter *and* no match — upstream compares against
    // the string "undefined", which no voice's tag contains.
    let lang = lang?;
    let matching: Vec<&Installed<T>> = installed
        .iter()
        .filter(|v| v.locale.contains(lang))
        .collect();
    match pick? {
        Pick::Index(i) if !matching.is_empty() => Some(&matching[i % matching.len()].handle),
        Pick::Name(name) => matching.iter().find(|v| &v.name == name).map(|v| &v.handle),
        Pick::Index(_) => None,
    }
}

#[cfg(windows)]
mod backend {
    //! SAPI, through the same `windows` crate cpal already pulls in.

    use super::{Installed, Pick, choose};
    use windows::{
        Win32::{
            Globalization::LCIDToLocaleName,
            Media::Speech::{
                ISpeechObjectToken, ISpeechVoice, SVSFPurgeBeforeSpeak, SVSFlagsAsync, SpVoice,
                SpeechVoiceSpeakFlags,
            },
            System::Com::{CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx},
        },
        core::BSTR,
    };

    pub(super) struct Engine {
        voice: ISpeechVoice,
        installed: Vec<Installed<ISpeechObjectToken>>,
    }

    /// A voice token's `Language` attribute is one or more LCIDs in hex
    /// (`"409"`, or `"409;9"` for a voice covering a whole language). The first
    /// is the one a browser would report as the voice's `lang`.
    fn locale_of(token: &ISpeechObjectToken) -> String {
        let attribute = unsafe { token.GetAttribute(&BSTR::from("Language")) };
        let Ok(attribute) = attribute else {
            return String::new();
        };
        let first = attribute.to_string();
        let first = first.split(';').next().unwrap_or_default();
        let Ok(lcid) = u32::from_str_radix(first, 16) else {
            return String::new();
        };
        let mut name = [0u16; 85]; // LOCALE_NAME_MAX_LENGTH
        let written = unsafe { LCIDToLocaleName(lcid, Some(&mut name), 0) };
        if written <= 0 {
            return String::new();
        }
        // The count includes the terminating nul.
        String::from_utf16_lossy(&name[..written as usize - 1])
    }

    impl Engine {
        pub(super) fn open() -> Result<Engine, String> {
            unsafe {
                // The window already put this thread in an apartment; a second
                // call in the same mode only adds a reference, and a different
                // mode (`RPC_E_CHANGED_MODE`) means one is established anyway.
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                let voice: ISpeechVoice =
                    CoCreateInstance(&SpVoice, None, CLSCTX_ALL).map_err(|e| e.message())?;
                let tokens = voice
                    .GetVoices(&BSTR::new(), &BSTR::new())
                    .map_err(|e| e.message())?;
                let installed = (0..tokens.Count().unwrap_or(0))
                    .filter_map(|i| {
                        let handle = tokens.Item(i).ok()?;
                        Some(Installed {
                            locale: locale_of(&handle),
                            name: handle
                                .GetDescription(0)
                                .map(|d| d.to_string())
                                .unwrap_or_default(),
                            handle,
                        })
                    })
                    .collect();
                Ok(Engine { voice, installed })
            }
        }

        /// The `(locale, name)` of every voice found — the test's window onto
        /// what the enumeration made of them.
        #[cfg(test)]
        pub(super) fn voices(&self) -> Vec<(String, String)> {
            self.installed
                .iter()
                .map(|v| (v.locale.clone(), v.name.clone()))
                .collect()
        }

        pub(super) fn say(
            &self,
            words: &str,
            lang: Option<&str>,
            pick: Option<&Pick>,
        ) -> Result<(), String> {
            unsafe {
                if let Some(token) = choose(&self.installed, lang, pick) {
                    let _ = self.voice.putref_Voice(token);
                }
                // Async so the frame does not wait for the sentence, and purge
                // so a new hap cuts the last one off — upstream's `cancel()`.
                let flags = SpeechVoiceSpeakFlags(SVSFlagsAsync.0 | SVSFPurgeBeforeSpeak.0);
                self.voice
                    .Speak(&BSTR::from(words), flags)
                    .map(|_| ())
                    .map_err(|e| e.message())
            }
        }
    }
}

#[cfg(not(windows))]
mod backend {
    //! macOS's `say` and speech-dispatcher's `spd-say`, which are what a
    //! desktop program without a browser has to talk to.

    use super::{Installed, Pick, choose};
    use std::process::{Command, Stdio};

    pub(super) struct Engine {
        program: &'static str,
        installed: Vec<Installed<String>>,
    }

    /// `say -v '?'` lists `Alex                en_US    # …`; `spd-say -o` has
    /// no such listing, so `spd-say -L`'s `NAME  LANGUAGE  VARIANT` columns
    /// stand in. Both give a name and a locale, which is all `choose` needs.
    fn installed(program: &str) -> Vec<Installed<String>> {
        let listing = if program == "say" {
            Command::new(program).args(["-v", "?"]).output()
        } else {
            Command::new(program).arg("-L").output()
        };
        let Ok(listing) = listing else {
            return Vec::new();
        };
        String::from_utf8_lossy(&listing.stdout)
            .lines()
            .skip(usize::from(program != "say")) // spd-say prints a header row
            .filter_map(|line| {
                let mut columns = line.split_whitespace();
                let name = columns.next()?.to_string();
                // A browser writes its tags with a hyphen; `say` uses `_`.
                let locale = columns.next()?.replace('_', "-");
                Some(Installed {
                    locale,
                    handle: name.clone(),
                    name,
                })
            })
            .collect()
    }

    impl Engine {
        pub(super) fn open() -> Result<Engine, String> {
            for program in ["say", "spd-say"] {
                if Command::new(program)
                    .arg("--version")
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .is_ok()
                {
                    return Ok(Engine {
                        program,
                        installed: installed(program),
                    });
                }
            }
            Err("no speech synthesiser found (install speech-dispatcher)".into())
        }

        pub(super) fn say(
            &self,
            words: &str,
            lang: Option<&str>,
            pick: Option<&Pick>,
        ) -> Result<(), String> {
            let mut command = Command::new(self.program);
            if let Some(name) = choose(&self.installed, lang, pick) {
                command.args(["-v", name]);
            }
            if self.program == "spd-say" {
                // Cancel what is still being said, as upstream's `cancel()`
                // does; `say` replaces its own utterance already.
                command.arg("-C");
                if let Some(lang) = lang {
                    command.args(["-l", lang]);
                }
            }
            command
                .arg("--")
                .arg(words)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rudel_core::value::ValueMap;

    fn voices() -> Vec<Installed<&'static str>> {
        [("en-US", "Zira"), ("en-GB", "Hazel"), ("de-DE", "Katja")]
            .into_iter()
            .map(|(locale, name)| Installed {
                locale: locale.to_string(),
                name: name.to_string(),
                handle: name,
            })
            .collect()
    }

    #[test]
    fn a_voice_is_chosen_by_language_then_index_or_name() {
        let voices = voices();
        // `speak('en', 1)` — the second English voice, and indices wrap.
        assert_eq!(
            choose(&voices, Some("en"), Some(&Pick::Index(1))),
            Some(&"Hazel")
        );
        assert_eq!(
            choose(&voices, Some("en"), Some(&Pick::Index(2))),
            Some(&"Zira")
        );
        assert_eq!(
            choose(&voices, Some("de"), Some(&Pick::Index(0))),
            Some(&"Katja")
        );
        assert_eq!(
            choose(&voices, Some("en"), Some(&Pick::Name("Hazel".into()))),
            Some(&"Hazel")
        );
        // A name from another language is not reachable through this one.
        assert_eq!(
            choose(&voices, Some("en"), Some(&Pick::Name("Katja".into()))),
            None
        );
        // No language, or a language nothing is installed for, leaves the
        // choice to the system — the same as upstream's empty filter.
        assert_eq!(choose(&voices, None, Some(&Pick::Index(0))), None);
        assert_eq!(choose(&voices, Some("fr"), Some(&Pick::Index(0))), None);
        assert_eq!(choose(&voices, Some("en"), None), None);
    }

    #[cfg(windows)]
    #[test]
    fn sapi_opens_and_lists_its_voices() {
        // Proves the COM wiring — apartment, class id, token enumeration and
        // the LCID-to-tag conversion — without making a sound. Every Windows
        // install ships at least one voice.
        let engine = backend::Engine::open().expect("open SAPI");
        let voices = engine.voices();
        assert!(!voices.is_empty(), "no voices installed");
        assert!(
            voices
                .iter()
                .any(|(locale, name)| { locale.contains('-') && !name.is_empty() }),
            "expected a BCP-47 tag and a name, got {voices:?}"
        );
    }

    #[test]
    fn a_marked_hap_yields_its_words_and_nothing_else_does() {
        let map = ValueMap::from([
            (SPEAK.to_string(), Value::Str("i am".into())),
            (SPEAK_LANG.to_string(), Value::Str("en".into())),
            (SPEAK_VOICE.to_string(), Value::Int(3)),
        ]);
        assert_eq!(
            request(&Value::Map(map)),
            Some(("i am".into(), Some("en".into()), Some(Pick::Index(3))))
        );
        // An ordinary sound hap is not a speech request.
        let sound = ValueMap::from([("s".to_string(), Value::Str("bd".into()))]);
        assert_eq!(request(&Value::Map(sound)), None);
        assert_eq!(request(&Value::Str("bd".into())), None);
    }
}
