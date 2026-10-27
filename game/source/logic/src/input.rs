//! Maps button events to UI actions.

use crate::buttons::{Button, Event};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    /// Dial steps: positive is right.
    Turn(i8),
    /// Dial pressed and released before the power-off hold time.
    Click,
    /// F1..F4 as 0..3 went down. The key's own action waits for `Soft` or `SoftHold`.
    SoftDown(u8),
    /// F1..F4 as 0..3 released: the key's short action, also sent after a hold.
    Soft(u8),
    /// F1..F4 as 0..3 held for the hold time: the key's second action.
    SoftHold(u8),
    /// The dial is held long enough to power off; power off on release.
    PowerHold,
    /// A button or dial turn during the power-off hold: stay on.
    PowerCancel,
    PowerOff,
}

pub struct Input {
    /// False while the click that woke the device is still down.
    armed: bool,
    holding: bool,
    /// F keys whose press was mapped, so their hold and release map too. A press that skipped
    /// the boot screen or cancelled a power off is not.
    soft_down: [bool; 4],
}

impl Input {
    /// `click_down`: the dial is pressed now, as it is right after the wakeup click.
    pub fn new(click_down: bool) -> Self {
        Input { armed: !click_down, holding: false, soft_down: [false; 4] }
    }

    pub fn map(&mut self, event: Event) -> Option<Action> {
        match event {
            // The click is still down; disarm so its release does nothing.
            Event::Press(_) if self.holding => {
                self.holding = false;
                self.armed = false;
                Some(Action::PowerCancel)
            }
            Event::Press(Button::Right) | Event::Repeat(Button::Right) => Some(Action::Turn(1)),
            Event::Press(Button::Left) | Event::Repeat(Button::Left) => Some(Action::Turn(-1)),
            // F1..F4 are buttons 0..3.
            Event::Press(f @ (Button::F1 | Button::F2 | Button::F3 | Button::F4)) => {
                self.soft_down[f as usize] = true;
                Some(Action::SoftDown(f as u8))
            }
            Event::Hold(f @ (Button::F1 | Button::F2 | Button::F3 | Button::F4)) => {
                self.soft_down[f as usize].then_some(Action::SoftHold(f as u8))
            }
            Event::Release(f @ (Button::F1 | Button::F2 | Button::F3 | Button::F4)) => {
                core::mem::take(&mut self.soft_down[f as usize]).then_some(Action::Soft(f as u8))
            }
            Event::Hold(Button::Click) if self.armed => {
                self.holding = true;
                Some(Action::PowerHold)
            }
            Event::Release(Button::Click) if self.holding => Some(Action::PowerOff),
            Event::Release(Button::Click) if self.armed => Some(Action::Click),
            Event::Release(Button::Click) => {
                self.armed = true;
                None
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    fn map(input: &mut Input, events: &[Event]) -> Vec<Action> {
        events.iter().filter_map(|&e| input.map(e)).collect()
    }

    #[test]
    fn short_click_acts_on_release() {
        let mut input = Input::new(false);
        let actions = map(&mut input, &[Event::Press(Button::Click), Event::Release(Button::Click)]);
        assert_eq!(actions, [Action::Click]);
    }

    #[test]
    fn hold_powers_off_on_release_without_click() {
        let mut input = Input::new(false);
        let events = [Event::Press(Button::Click), Event::Hold(Button::Click), Event::Release(Button::Click)];
        assert_eq!(map(&mut input, &events), [Action::PowerHold, Action::PowerOff]);
    }

    #[test]
    fn a_press_during_the_hold_cancels_power_off() {
        let mut input = Input::new(false);
        let events = [
            Event::Press(Button::Click),
            Event::Hold(Button::Click),
            Event::Press(Button::Right),
            Event::Release(Button::Right),
            Event::Release(Button::Click),
            Event::Press(Button::Click),
            Event::Release(Button::Click),
        ];
        assert_eq!(map(&mut input, &events), [Action::PowerHold, Action::PowerCancel, Action::Click]);
    }

    #[test]
    fn wakeup_click_is_ignored() {
        let mut input = Input::new(true);
        let events = [Event::Hold(Button::Click), Event::Release(Button::Click), Event::Press(Button::Click), Event::Release(Button::Click)];
        assert_eq!(map(&mut input, &events), [Action::Click]);
    }

    #[test]
    fn dial_and_keys() {
        let mut input = Input::new(false);
        let events = [Event::Press(Button::Right), Event::Repeat(Button::Left), Event::Press(Button::F4), Event::Release(Button::F4)];
        assert_eq!(map(&mut input, &events), [Action::Turn(1), Action::Turn(-1), Action::SoftDown(3), Action::Soft(3)]);
    }

    #[test]
    fn a_held_key_sends_its_hold_then_its_release() {
        let mut input = Input::new(false);
        let events = [Event::Press(Button::F1), Event::Hold(Button::F1), Event::Release(Button::F1)];
        assert_eq!(map(&mut input, &events), [Action::SoftDown(0), Action::SoftHold(0), Action::Soft(0)]);
    }

    #[test]
    fn a_key_that_cancels_power_off_does_nothing_on_release() {
        let mut input = Input::new(false);
        let events = [
            Event::Press(Button::Click),
            Event::Hold(Button::Click),
            Event::Press(Button::F2),
            Event::Hold(Button::F2),
            Event::Release(Button::F2),
            Event::Release(Button::Click),
        ];
        assert_eq!(map(&mut input, &events), [Action::PowerHold, Action::PowerCancel]);
    }

    #[test]
    fn a_key_pressed_before_input_starts_does_nothing() {
        let mut input = Input::new(false);
        let events = [Event::Hold(Button::F3), Event::Release(Button::F3)];
        assert!(map(&mut input, &events).is_empty());
    }
}
