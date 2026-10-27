//! Button debounce and events, stepped once per millisecond.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Button {
    F1,
    F2,
    F3,
    F4,
    Left,
    Right,
    /// Dial press, also the power button.
    Click,
}

pub const COUNT: usize = 7;
const ALL: [Button; COUNT] = [Button::F1, Button::F2, Button::F3, Button::F4, Button::Left, Button::Right, Button::Click];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Event {
    /// Sent on the press itself, without waiting for release.
    Press(Button),
    /// Dial left or right held: sent again every `REPEAT_MS` after `REPEAT_DELAY_MS`.
    Repeat(Button),
    /// Sent once when a button stays down for its hold time.
    Hold(Button),
    Release(Button),
}

pub const DEBOUNCE_MS: u16 = 8;
const REPEAT_DELAY_MS: u16 = 400;
const REPEAT_MS: u16 = 80;
const HOLD_MS: u16 = 500;
/// Power-off hold time: well past a click, which the dial uses to move focus.
pub const CLICK_HOLD_MS: u16 = 1000;
/// The dial press also closes the dial-left contact; ignore left this long after a press ends.
const CLICK_MASKS_LEFT_MS: u16 = 50;

#[derive(Default)]
pub struct Buttons {
    down: [bool; COUNT],
    /// Milliseconds the raw input has disagreed with `down`.
    unstable_ms: [u16; COUNT],
    /// Milliseconds the button has been down.
    held_ms: [u16; COUNT],
    since_click_ms: u16,
}

impl Buttons {
    /// `raw[b as usize]` is true while button `b` reads pressed.
    pub fn tick(&mut self, mut raw: [bool; COUNT], mut emit: impl FnMut(Event)) {
        if raw[Button::Click as usize] {
            self.since_click_ms = 0;
        } else {
            self.since_click_ms = self.since_click_ms.saturating_add(1);
        }
        if raw[Button::Click as usize] || self.since_click_ms < CLICK_MASKS_LEFT_MS {
            raw[Button::Left as usize] = false;
        }

        for button in ALL {
            let i = button as usize;
            if raw[i] == self.down[i] {
                self.unstable_ms[i] = 0;
            } else {
                self.unstable_ms[i] += 1;
                if self.unstable_ms[i] >= DEBOUNCE_MS {
                    self.down[i] = raw[i];
                    self.unstable_ms[i] = 0;
                    self.held_ms[i] = 0;
                    emit(if raw[i] { Event::Press(button) } else { Event::Release(button) });
                    continue;
                }
            }
            if !self.down[i] {
                continue;
            }
            self.held_ms[i] = self.held_ms[i].saturating_add(1);
            let held = self.held_ms[i];
            match button {
                Button::Left | Button::Right => {
                    if held >= REPEAT_DELAY_MS && (held - REPEAT_DELAY_MS).is_multiple_of(REPEAT_MS) {
                        emit(Event::Repeat(button));
                    }
                }
                Button::Click if held == CLICK_HOLD_MS => emit(Event::Hold(button)),
                Button::F1 | Button::F2 | Button::F3 | Button::F4 if held == HOLD_MS => emit(Event::Hold(button)),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    fn pressed(buttons: &[Button]) -> [bool; COUNT] {
        let mut raw = [false; COUNT];
        for &b in buttons {
            raw[b as usize] = true;
        }
        raw
    }

    /// Runs `ms` ticks per step; returns (tick index, event) pairs.
    fn run(steps: &[(&[Button], u32)]) -> Vec<(u32, Event)> {
        let mut b = Buttons::default();
        let mut events = Vec::new();
        let mut t = 0;
        for &(down, ms) in steps {
            for _ in 0..ms {
                b.tick(pressed(down), |e| events.push((t, e)));
                t += 1;
            }
        }
        events
    }

    #[test]
    fn press_after_debounce_and_release() {
        let events = run(&[(&[Button::F1], 20), (&[], 20)]);
        assert_eq!(events, [(7, Event::Press(Button::F1)), (27, Event::Release(Button::F1))]);
    }

    #[test]
    fn bounce_shorter_than_debounce_is_ignored() {
        let events = run(&[(&[Button::F2], 5), (&[], 5), (&[Button::F2], 5), (&[], 20)]);
        assert!(events.is_empty());
    }

    #[test]
    fn click_hold_fires_once_at_power_off_time() {
        let events = run(&[(&[Button::Click], 2000)]);
        assert_eq!(events, [(7, Event::Press(Button::Click)), (7 + CLICK_HOLD_MS as u32, Event::Hold(Button::Click))]);
    }

    #[test]
    fn dial_repeats_while_held() {
        let events = run(&[(&[Button::Right], 600)]);
        let repeats: Vec<u32> = events.iter().filter(|(_, e)| *e == Event::Repeat(Button::Right)).map(|(t, _)| *t).collect();
        assert_eq!(repeats, [407, 487, 567]);
    }

    #[test]
    fn dial_left_alone_presses() {
        let events = run(&[(&[], 100), (&[Button::Left], 20)]);
        assert_eq!(events, [(107, Event::Press(Button::Left))]);
    }

    #[test]
    fn click_does_not_count_as_dial_left() {
        // The press closes left 3 ms before the click contact, and opens it 4 ms after.
        let events = run(&[
            (&[], 100),
            (&[Button::Left], 3),
            (&[Button::Left, Button::Click], 100),
            (&[], 4),
            (&[Button::Left], 4),
            (&[], 100),
        ]);
        assert_eq!(events, [(110, Event::Press(Button::Click)), (210, Event::Release(Button::Click))]);
    }
}
