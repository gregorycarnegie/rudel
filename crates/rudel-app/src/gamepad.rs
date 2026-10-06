// gamepad.rs - feeds `@strudel/gamepad`: reads controllers with gilrs and puts
// each on rudel-core's pad bus in the browser's "standard gamepad" layout.
// SPDX-License-Identifier: AGPL-3.0-or-later

use gilrs::{Axis, Button, EventType, GamepadId, Gilrs};
use rudel_core::gamepad::{PadState, free_gamepad_slot, remove_gamepad, set_gamepad};
use std::{
    collections::HashMap,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

/// The standard layout's buttons, by index.
const BUTTONS: [Button; 17] = [
    Button::South,
    Button::East,
    Button::West,
    Button::North,
    Button::LeftTrigger,
    Button::RightTrigger,
    Button::LeftTrigger2,
    Button::RightTrigger2,
    Button::Select,
    Button::Start,
    Button::LeftThumb,
    Button::RightThumb,
    Button::DPadUp,
    Button::DPadDown,
    Button::DPadLeft,
    Button::DPadRight,
    Button::Mode,
];

/// One pad's state in the standard layout. gilrs's sticks read up as
/// positive; the browser's read down as positive.
fn standard(button: impl Fn(Button) -> f32, axis: impl Fn(Axis) -> f32) -> PadState {
    PadState {
        axes: [
            (Axis::LeftStickX, 1.0),
            (Axis::LeftStickY, -1.0),
            (Axis::RightStickX, 1.0),
            (Axis::RightStickY, -1.0),
        ]
        .into_iter()
        .map(|(a, sign)| f64::from(axis(a)) * sign)
        .collect(),
        buttons: BUTTONS.iter().map(|&b| f64::from(button(b))).collect(),
    }
}

/// Start reading controllers once a script has called `gamepad()`. Cheap to
/// call every frame.
pub(crate) fn start_if_requested() {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if !rudel_core::gamepad::gamepad_requested() || STARTED.swap(true, Ordering::Relaxed) {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("gamepad".into())
        .spawn(run);
}

fn run() {
    let mut gilrs = match Gilrs::new() {
        Ok(g) => g,
        Err(gilrs::Error::NotImplemented(g)) => g,
        Err(e) => {
            eprintln!("gamepad: {e}");
            return;
        }
    };
    // Pads get the lowest free slot as they connect, as the browser's do.
    let mut slots: HashMap<GamepadId, usize> = HashMap::new();
    let connect = |slots: &mut HashMap<GamepadId, usize>, id| {
        let slot = free_gamepad_slot();
        set_gamepad(slot, PadState::default());
        slots.insert(id, slot);
    };
    for (id, _) in gilrs.gamepads() {
        connect(&mut slots, id);
    }
    loop {
        while let Some(ev) = gilrs.next_event_blocking(Some(Duration::from_millis(4))) {
            match ev.event {
                EventType::Connected => connect(&mut slots, ev.id),
                EventType::Disconnected => {
                    if let Some(slot) = slots.remove(&ev.id) {
                        remove_gamepad(slot);
                    }
                }
                _ => {}
            }
        }
        for (&id, &slot) in &slots {
            let pad = gilrs.gamepad(id);
            let state = standard(
                |b| pad.button_data(b).map_or(0.0, |d| d.value()),
                |a| pad.value(a),
            );
            set_gamepad(slot, state);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_to_the_standard_layout() {
        let state = standard(
            |b| match b {
                Button::South | Button::Mode => 1.0,
                Button::LeftTrigger2 => 0.5,
                _ => 0.0,
            },
            |a| match a {
                Axis::LeftStickY => 1.0,
                Axis::RightStickX => -0.5,
                _ => 0.0,
            },
        );
        assert_eq!(state.axes, [0.0, -1.0, -0.5, 0.0], "stick up reads as -1");
        assert_eq!(state.buttons.len(), 17);
        assert_eq!(state.buttons[0], 1.0, "a");
        assert_eq!(state.buttons[6], 0.5, "lt is analog");
        assert_eq!(state.buttons[16], 1.0, "home");
        assert_eq!(state.buttons.iter().sum::<f64>(), 2.5);
    }
}
