//! What the dial sets on a screen: a list of targets, in click order, the first focused by
//! default. A click moves to the next target and a turn steps the focused one.

use crate::settings::{Field, Settings};
use crate::ticks;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    /// Keyer speed.
    Send,
    /// Character speed of what the device sends you.
    Receive,
    Farnsworth,
    Volume,
    Tone,
    /// The Koch level, which the screen keeps.
    Level,
}

impl Target {
    /// Steps the setting this target sets; false for a target the screen keeps.
    pub fn step(self, s: &mut Settings, delta: i8) -> bool {
        match self {
            Target::Send => s.adjust(Field::Speed, delta),
            Target::Receive => s.step_receive(delta),
            Target::Farnsworth => s.step_farnsworth(delta),
            Target::Volume => s.adjust(Field::Volume, delta),
            Target::Tone => s.adjust(Field::Tone, delta),
            Target::Level => return false,
        }
        true
    }
}

/// Dots after the key goes up before the dial turns again: a word gap.
const QUIET_UNITS: u32 = 7;

/// Holds dial turns off while you key, so brushing the dial mid-over changes nothing.
#[derive(Default)]
pub struct KeyGuard {
    down: bool,
    quiet_at: u32,
}

impl KeyGuard {
    /// The key went down or up at `at_ms`, with dots of `dit_ms`.
    pub fn edge(&mut self, down: bool, at_ms: u32, dit_ms: u16) {
        self.down = down;
        if !down {
            self.quiet_at = at_ms.wrapping_add(QUIET_UNITS * dit_ms as u32);
        }
    }

    /// The dial may turn at `now_ms`.
    pub fn quiet(&self, now_ms: u32) -> bool {
        !self.down && ticks::reached(now_ms, self.quiet_at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_wait_a_word_gap_after_the_key() {
        let mut guard = KeyGuard::default();
        assert!(guard.quiet(10));
        guard.edge(true, 100, 60);
        assert!(!guard.quiet(5000));
        guard.edge(false, 160, 60);
        assert!(!guard.quiet(160 + 7 * 60 - 1));
        assert!(guard.quiet(160 + 7 * 60));
    }

    #[test]
    fn targets_step_their_settings() {
        let mut s = Settings::default();
        (s.wpm, s.volume) = (20, 10);
        assert!(Target::Send.step(&mut s, 2));
        assert!(Target::Volume.step(&mut s, -1));
        assert!(Target::Receive.step(&mut s, 1));
        assert!(!Target::Level.step(&mut s, 1));
        assert_eq!((s.wpm, s.volume, s.rx_wpm), (22, 9, 23));
    }
}
