// gamepad.rs - `@strudel/gamepad` (packages/gamepad/gamepad.mjs).
//
// Strudel reads controllers through the browser's Gamepad API. Here the host
// (the app's controller thread) writes each connected pad into a slot table in
// the browser's "standard gamepad" layout, and `gamepad(index)` reads it at
// query time, as upstream's signals poll `navigator.getGamepads()`.
//
// Ported behaviour, including the odd parts: before a pad is read the axes are
// 0 (not the 0.5 centre), toggles and the sequence detector advance whenever
// one of their patterns is queried, and a sequence matches by name, by
// `buttonMap` index, or by `parseInt` of the target.
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    pattern::Pattern,
    signal::signal,
    value::{Value, ValueMap},
};
use std::{
    collections::HashMap,
    sync::{
        Arc, LazyLock, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

/// `buttonMap`: the Logitech Dual Action / standard-layout button names, in
/// upstream's key order (the first key with an index names it in a sequence).
pub const BUTTON_MAP: &[(&str, usize)] = &[
    ("a", 0),
    ("b", 1),
    ("x", 2),
    ("y", 3),
    ("lb", 4),
    ("rb", 5),
    ("lt", 6),
    ("rt", 7),
    ("back", 8),
    ("start", 9),
    ("l3", 10),
    ("ls", 10),
    ("r3", 11),
    ("rs", 11),
    ("u", 12),
    ("up", 12),
    ("d", 13),
    ("down", 13),
    ("l", 14),
    ("left", 14),
    ("r", 15),
    ("right", 15),
];

fn button_index(name: &str) -> Option<usize> {
    BUTTON_MAP.iter().find(|(k, _)| *k == name).map(|&(_, i)| i)
}

/// One pad as the Gamepad API reports it: axes in -1..1 (stick y down
/// positive), button values 0..1 (analog triggers in between).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PadState {
    pub axes: Vec<f64>,
    pub buttons: Vec<f64>,
}

/// `navigator.getGamepads()`: connected pads by index, `None` where one left.
static PADS: LazyLock<RwLock<Vec<Option<PadState>>>> = LazyLock::new(|| RwLock::new(Vec::new()));

/// Put pad `index`'s current state on the bus (the host's controller thread).
pub fn set_gamepad(index: usize, state: PadState) {
    let mut pads = PADS.write().unwrap_or_else(|e| e.into_inner());
    if pads.len() <= index {
        pads.resize(index + 1, None);
    }
    pads[index] = Some(state);
}

/// Pad `index` was disconnected.
pub fn remove_gamepad(index: usize) {
    let mut pads = PADS.write().unwrap_or_else(|e| e.into_inner());
    if let Some(slot) = pads.get_mut(index) {
        *slot = None;
    }
}

/// The lowest slot no pad holds: where the browser puts a newly connected one.
pub fn free_gamepad_slot() -> usize {
    let pads = PADS.read().unwrap_or_else(|e| e.into_inner());
    pads.iter().position(Option::is_none).unwrap_or(pads.len())
}

fn read_pad(index: usize) -> Option<PadState> {
    let pads = PADS.read().unwrap_or_else(|e| e.into_inner());
    pads.get(index).cloned().flatten()
}

/// `getGamepadStates`/`clearGamepadStates`: the toggles' shared state, keyed
/// `gamepad{index}_btn{i}`, as `(lastButtonState, toggleState)`.
static TOGGLES: LazyLock<Mutex<HashMap<String, (f64, f64)>>> = LazyLock::new(Default::default);

/// `getGamepadStates()`: every toggle's state.
pub fn gamepad_states() -> ValueMap {
    let toggles = TOGGLES.lock().unwrap_or_else(|e| e.into_inner());
    let mut keys: Vec<&String> = toggles.keys().collect();
    keys.sort();
    keys.into_iter()
        .map(|k| {
            let (last, toggle) = toggles[k];
            let mut m = ValueMap::new();
            m.insert("lastButtonState".to_string(), Value::F64(last));
            m.insert("toggleState".to_string(), Value::F64(toggle));
            (k.clone(), Value::Map(m))
        })
        .collect()
}

/// `clearGamepadStates()`.
pub fn clear_gamepad_states() {
    TOGGLES.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// `ButtonSequenceDetector`: the buttons pressed within a time window.
#[derive(Debug)]
struct SequenceDetector {
    window_ms: f64,
    /// `(button name, time in ms)`.
    sequence: Vec<(String, f64)>,
    last_input: f64,
    states: Vec<f64>,
}

impl SequenceDetector {
    fn new(window_ms: f64) -> SequenceDetector {
        SequenceDetector {
            window_ms,
            sequence: Vec::new(),
            last_input: 0.0,
            states: vec![0.0; 16],
        }
    }

    /// `addInput`: record a press (a rising edge to exactly 1) at `now` ms.
    fn add_input(&mut self, index: usize, value: f64, now: f64) {
        if self.states.len() <= index {
            self.states.resize(index + 1, 0.0);
        }
        if value == 1.0 && self.states[index] == 0.0 {
            if now - self.last_input > self.window_ms {
                self.sequence.clear();
            }
            let name = BUTTON_MAP
                .iter()
                .find(|(_, i)| *i == index)
                .map_or_else(|| index.to_string(), |(k, _)| k.to_string());
            self.sequence.push((name, now));
            self.last_input = now;
            let window = self.window_ms;
            self.sequence.retain(|(_, t)| now - t <= window);
        }
        self.states[index] = value;
    }

    /// `checkSequence`: 1 when the last presses are `target`.
    fn check(&self, target: &[String]) -> f64 {
        if self.sequence.len() < target.len() {
            return 0.0;
        }
        let last = &self.sequence[self.sequence.len() - target.len()..];
        let matches = last.iter().zip(target).all(|((input, _), want)| {
            // `buttonMap[input] === buttonMap[target]`: two names missing from
            // the map are `undefined === undefined`, so they match too.
            input == want
                || button_index(input) == button_index(want)
                || button_index(input).is_some_and(|i| Some(i as i64) == parse_int(want))
        });
        if matches { 1.0 } else { 0.0 }
    }
}

/// JS `parseInt(s)`: the leading (signed) digits after whitespace, or `None`
/// for JS's `NaN`.
fn parse_int(s: &str) -> Option<i64> {
    let s = s.trim_start();
    let (sign, rest) = match s.strip_prefix('-') {
        Some(r) => (-1, r),
        None => (1, s.strip_prefix('+').unwrap_or(s)),
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse::<i64>().ok().map(|n| sign * n)
}

/// What a sequence target can be: a string of one-letter names, or a list.
pub enum SequenceTarget {
    Text(String),
    List(Vec<String>),
}

impl SequenceTarget {
    fn names(&self) -> Vec<String> {
        match self {
            SequenceTarget::Text(s) => s.to_lowercase().chars().map(String::from).collect(),
            SequenceTarget::List(l) => l.iter().map(|s| s.to_lowercase()).collect(),
        }
    }
}

static WANTED: AtomicBool = AtomicBool::new(false);

/// Whether a script has called `gamepad()`: the host starts reading
/// controllers then, not before.
pub fn gamepad_requested() -> bool {
    WANTED.load(Ordering::Relaxed)
}

/// One `gamepad(index)` call: its handler and its sequence detector, shared by
/// every pattern it hands out.
#[derive(Clone)]
pub struct Gamepad {
    index: usize,
    inner: Arc<Mutex<Handler>>,
    started: Instant,
}

struct Handler {
    /// The last state read, kept when the pad goes away (the browser's
    /// handler keeps its arrays too).
    axes: Vec<f64>,
    buttons: Vec<f64>,
    detector: SequenceDetector,
}

impl Gamepad {
    pub fn new(index: usize) -> Gamepad {
        WANTED.store(true, Ordering::Relaxed);
        Gamepad {
            index,
            inner: Arc::new(Mutex::new(Handler {
                axes: vec![0.0; 4],
                buttons: vec![0.0; 16],
                detector: SequenceDetector::new(2000.0),
            })),
            started: Instant::now(),
        }
    }

    /// The base signal's work: poll the pad, feed every button to the
    /// detector, and return `(axes, buttons)`.
    fn poll(&self) -> (Vec<f64>, Vec<f64>) {
        let mut h = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        // `gamepad(0)` adopts the first pad that connects, as upstream's
        // `if (!this._activeGamepad)` does with its falsy 0.
        let pad = read_pad(self.index).or_else(|| {
            if self.index == 0 {
                let pads = PADS.read().unwrap_or_else(|e| e.into_inner());
                pads.iter().flatten().next().cloned()
            } else {
                None
            }
        });
        if let Some(pad) = pad {
            h.axes = pad.axes.iter().map(|a| (a + 1.0) / 2.0).collect();
            h.buttons = pad.buttons;
        }
        let now = self.started.elapsed().as_secs_f64() * 1000.0;
        let buttons = h.buttons.clone();
        for (i, v) in buttons.iter().enumerate() {
            h.detector.add_input(i, *v, now);
        }
        (h.axes.clone(), buttons)
    }

    /// Axis `i` (`x1`, `y1`, `x2`, `y2`), 0..1.
    pub fn axis(&self, i: usize) -> Pattern {
        let pad = self.clone();
        signal(move |_| pad.poll().0.get(i).map_or(Value::Null, |v| Value::F64(*v)))
    }

    /// Button `i`'s value.
    pub fn button(&self, i: usize) -> Pattern {
        let pad = self.clone();
        signal(move |_| pad.poll().1.get(i).map_or(Value::Null, |v| Value::F64(*v)))
    }

    /// Button `i`'s toggle, flipped on each press (shared across `gamepad`
    /// calls for the same pad and button).
    pub fn toggle(&self, i: usize) -> Pattern {
        let pad = self.clone();
        let key = format!("gamepad{}_btn{i}", self.index);
        TOGGLES
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(key.clone())
            .or_insert((0.0, 0.0));
        signal(move |_| {
            let current = pad.poll().1.get(i).copied().unwrap_or(0.0);
            let mut toggles = TOGGLES.lock().unwrap_or_else(|e| e.into_inner());
            let state = toggles.entry(key.clone()).or_insert((0.0, 0.0));
            if current == 1.0 && state.0 == 0.0 {
                state.1 = if state.1 == 0.0 { 1.0 } else { 0.0 };
            }
            state.0 = current;
            Value::F64(state.1)
        })
    }

    /// `btnSequence(sequence)`: 1 while the last presses spell `target`.
    pub fn sequence(&self, target: SequenceTarget) -> Pattern {
        let pad = self.clone();
        let target = target.names();
        signal(move |_| {
            pad.poll();
            let h = pad.inner.lock().unwrap_or_else(|e| e.into_inner());
            Value::F64(h.detector.check(&target))
        })
    }

    /// `raw`: `{ axes, buttons, t }`.
    pub fn raw(&self) -> Pattern {
        let pad = self.clone();
        signal(move |t| {
            let (axes, buttons) = pad.poll();
            let mut m = ValueMap::new();
            m.insert(
                "axes".to_string(),
                Value::List(axes.into_iter().map(Value::F64).collect()),
            );
            m.insert(
                "buttons".to_string(),
                Value::List(buttons.into_iter().map(Value::F64).collect()),
            );
            m.insert("t".to_string(), Value::F64(t.to_f64()));
            Value::Map(m)
        })
    }
}

/// Serialises tests that touch the process-wide pad and toggle tables.
#[cfg(test)]
pub(crate) fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fraction::Frac;

    fn at(p: &Pattern) -> Value {
        p.query_arc(Frac::zero(), Frac::new(1, 16)).remove(0).value
    }

    fn pad(axes: &[f64], pressed: &[usize]) -> PadState {
        let mut buttons = vec![0.0; 17];
        for &b in pressed {
            buttons[b] = 1.0;
        }
        PadState {
            axes: axes.to_vec(),
            buttons,
        }
    }

    #[test]
    fn axes_read_zero_until_a_pad_then_its_unipolar_value() {
        let _l = test_lock();
        remove_gamepad(3);
        let g = Gamepad::new(3);
        assert_eq!(
            at(&g.axis(0)),
            Value::F64(0.0),
            "upstream's initial [0, 0, 0, 0]"
        );
        set_gamepad(3, pad(&[-1.0, 0.0, 1.0, 0.5], &[]));
        assert_eq!(at(&g.axis(0)), Value::F64(0.0));
        assert_eq!(at(&g.axis(1)), Value::F64(0.5));
        assert_eq!(at(&g.axis(2)), Value::F64(1.0));
        assert_eq!(at(&g.axis(3)), Value::F64(0.75));
        remove_gamepad(3);
        assert_eq!(at(&g.axis(2)), Value::F64(1.0), "the last state is kept");
    }

    #[test]
    fn a_toggle_flips_on_each_press() {
        let _l = test_lock();
        clear_gamepad_states();
        let g = Gamepad::new(4);
        let tgl = g.toggle(0);
        let mut seen = Vec::new();
        for pressed in [false, true, true, false, true, false] {
            set_gamepad(4, pad(&[0.0; 4], if pressed { &[0] } else { &[] }));
            seen.push(at(&tgl));
        }
        let f = Value::F64;
        assert_eq!(seen, [f(0.0), f(1.0), f(1.0), f(1.0), f(0.0), f(0.0)]);
        assert!(gamepad_states().contains_key("gamepad4_btn0"));
        remove_gamepad(4);
    }

    #[test]
    fn a_sequence_matches_by_name_index_or_number() {
        let mut d = SequenceDetector::new(2000.0);
        let press = |d: &mut SequenceDetector, i: usize, t: f64| {
            d.add_input(i, 1.0, t);
            d.add_input(i, 0.0, t);
        };
        press(&mut d, 0, 0.0); // a
        press(&mut d, 12, 10.0); // u (up)
        press(&mut d, 1, 20.0); // b
        let check = |d: &SequenceDetector, t: SequenceTarget| d.check(&t.names());
        assert_eq!(check(&d, SequenceTarget::Text("aub".into())), 1.0);
        assert_eq!(
            check(&d, SequenceTarget::Text("AUB".into())),
            1.0,
            "case folds"
        );
        assert_eq!(
            check(&d, SequenceTarget::List(vec!["up".into(), "b".into()])),
            1.0,
            "`up` and `u` are the same button"
        );
        assert_eq!(
            check(&d, SequenceTarget::List(vec!["12".into(), "1".into()])),
            1.0,
            "a number matches the button's index"
        );
        assert_eq!(check(&d, SequenceTarget::Text("ba".into())), 0.0);
        // A press long after the last starts a new sequence.
        press(&mut d, 2, 5000.0);
        assert_eq!(check(&d, SequenceTarget::Text("bx".into())), 0.0);
        assert_eq!(check(&d, SequenceTarget::Text("x".into())), 1.0);
    }

    #[test]
    fn only_a_full_press_counts() {
        let mut d = SequenceDetector::new(2000.0);
        d.add_input(6, 0.5, 0.0); // a half-pulled trigger
        assert!(d.sequence.is_empty());
        // Upstream needs an edge from exactly 0, so pulling on through is no press.
        d.add_input(6, 1.0, 1.0);
        assert!(d.sequence.is_empty());
        d.add_input(6, 0.0, 2.0);
        d.add_input(6, 1.0, 3.0);
        assert_eq!(d.sequence.len(), 1);
    }

    #[test]
    fn parse_int_is_javascripts() {
        assert_eq!(parse_int("12"), Some(12));
        assert_eq!(parse_int(" 3x"), Some(3));
        assert_eq!(parse_int("-4"), Some(-4));
        assert_eq!(parse_int("a"), None);
    }
}
