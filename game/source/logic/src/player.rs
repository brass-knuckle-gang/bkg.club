//! Sends text as Morse, stepped once per millisecond. A fist setting varies each mark and gap
//! at random, the way a human operator does.

use crate::keyer;
use crate::morse;
use crate::text::Text;

pub const MAX_TEXT: usize = 48;

/// How a sender keys: character speed, Farnsworth speed, and fist.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Keying {
    pub wpm: u8,
    /// Gaps between characters and words stretch so text arrives at this rate. At or above
    /// `wpm` it has no effect.
    pub effective: u8,
    /// Largest random change to any length, in percent.
    pub fist: u8,
}

impl Keying {
    pub fn dit_ms(&self) -> u16 {
        keyer::dit_ms(self.wpm)
    }

    /// The unit of letter and word gaps, to the nearest millisecond. ARRL Farnsworth timing: the
    /// 19 spacing units of PARIS take what is left of a minute at the effective rate once its 31
    /// units of characters are sent at the full rate.
    pub fn space_ms(&self) -> u16 {
        let wpm = self.wpm.clamp(keyer::MIN_WPM, keyer::MAX_WPM) as u32;
        let effective = self.effective as u32;
        if effective == 0 || effective >= wpm {
            return self.dit_ms();
        }
        let per_minute = 19 * effective * wpm;
        ((60_000 * wpm - 37_200 * effective + per_minute / 2) / per_minute) as u16
    }

    /// The silence between two words.
    pub fn word_gap_ms(&self) -> u32 {
        7 * self.space_ms() as u32
    }
}

#[derive(Clone, Copy)]
pub struct Player {
    text: Text<MAX_TEXT>,
    /// Next character, and next element inside it.
    pos: usize,
    element: usize,
    /// Gap owed before the next mark, in units.
    gap: u8,
    down: bool,
    remaining_ms: u32,
    dit_ms: u32,
    /// Length of one unit of the gaps between characters and words.
    space_ms: u32,
    fist: u8,
    rng: u32,
    pitch_hz: u16,
    active: bool,
}

impl Default for Player {
    fn default() -> Self {
        Self::new()
    }
}

impl Player {
    pub const fn new() -> Self {
        Player {
            text: Text::new(),
            pos: 0,
            element: 0,
            gap: 0,
            down: false,
            remaining_ms: 0,
            dit_ms: 60,
            space_ms: 60,
            fist: 0,
            rng: 1,
            pitch_hz: 0,
            active: false,
        }
    }

    /// Starts sending `text` after `delay_ms` of silence. Text past `MAX_TEXT` is dropped.
    pub fn start(&mut self, text: &str, keying: Keying, seed: u32, delay_ms: u32, pitch_hz: u16) {
        self.text.clear();
        for b in text.bytes().take(MAX_TEXT) {
            self.text.push(b);
        }
        self.pos = 0;
        self.element = 0;
        self.gap = 0;
        self.down = false;
        self.remaining_ms = delay_ms;
        self.dit_ms = keying.dit_ms() as u32;
        self.space_ms = keying.space_ms() as u32;
        self.fist = keying.fist;
        self.rng = seed | 1;
        self.pitch_hz = pitch_hz;
        self.active = true;
    }

    pub fn stop(&mut self) {
        self.active = false;
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn pitch_hz(&self) -> u16 {
        self.pitch_hz
    }

    /// Some(key down) while sending; None once the text is done.
    pub fn tick(&mut self) -> Option<bool> {
        if !self.active {
            return None;
        }
        while self.remaining_ms == 0 {
            let Some((down, units)) = self.next() else {
                self.active = false;
                return None;
            };
            self.down = down;
            let unit_ms = if !down && units >= 3 { self.space_ms } else { self.dit_ms };
            self.remaining_ms = self.length_ms(units, unit_ms);
        }
        self.remaining_ms -= 1;
        Some(self.down)
    }

    /// The next mark or gap: (key down, length in units).
    fn next(&mut self) -> Option<(bool, u8)> {
        let bytes = self.text.as_str().as_bytes();
        while let Some((c, len)) = morse::first(&bytes[self.pos..]) {
            if c == b' ' {
                if self.gap > 0 {
                    self.gap = 7;
                }
                self.pos += 1;
                continue;
            }
            let Some(code) = morse::code(c) else {
                self.pos += len;
                continue;
            };
            if self.gap > 0 {
                return Some((false, core::mem::take(&mut self.gap)));
            }
            let mark = code.as_bytes()[self.element];
            self.element += 1;
            if self.element == code.len() {
                self.pos += len;
                self.element = 0;
                self.gap = 3;
            } else {
                self.gap = 1;
            }
            return Some((true, if mark == b'-' { 3 } else { 1 }));
        }
        None
    }

    fn length_ms(&mut self, units: u8, unit_ms: u32) -> u32 {
        let ideal = units as u32 * unit_ms;
        if self.fist == 0 {
            return ideal;
        }
        // xorshift32
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        let span = 2 * self.fist as u32 + 1;
        let percent = 100 + (self.rng % span) as i32 - self.fist as i32;
        (ideal as i32 * percent / 100).max(1) as u32
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    /// Runs of (down, ms) until the player stops.
    fn runs(p: &mut Player) -> Vec<(bool, u32)> {
        let mut out: Vec<(bool, u32)> = Vec::new();
        while let Some(down) = p.tick() {
            match out.last_mut() {
                Some((d, n)) if *d == down => *n += 1,
                _ => out.push((down, 1)),
            }
        }
        out
    }

    fn keying(wpm: u8, effective: u8, fist: u8) -> Keying {
        Keying { wpm, effective, fist }
    }

    #[test]
    fn farnsworth_stretches_only_character_and_word_gaps() {
        let mut p = Player::new();
        p.start("EE E", keying(20, 10, 0), 1, 0, 650);
        // 20 WPM characters at 10 WPM: one spacing unit is (60000 * 20 - 37200 * 10) / (19 * 10 * 20) = 217.9 ms.
        assert_eq!(runs(&mut p), [(true, 60), (false, 3 * 218), (true, 60), (false, 7 * 218), (true, 60)]);
    }

    #[test]
    fn farnsworth_paris_takes_the_effective_time() {
        let mut p = Player::new();
        p.start("PARIS ", keying(18, 12, 0), 1, 0, 650);
        let total: u32 = runs(&mut p).iter().map(|r| r.1).sum();
        // PARIS ends at its last mark: the 7-unit word gap after it belongs to the next word.
        let without_word_gap = 60_000 / 12 - 7 * ((60_000 * 18 - 37_200 * 12) / (19 * 12 * 18));
        // Lengths round to whole milliseconds: 1200 / 18 is 66.7 ms.
        assert!(total.abs_diff(without_word_gap) <= without_word_gap / 100, "{total} vs {without_word_gap}");
    }

    #[test]
    fn word_gap_matches_what_the_player_sends() {
        let mut p = Player::new();
        let k = keying(20, 10, 0);
        p.start("A B", k, 1, 0, 650);
        assert_eq!(runs(&mut p)[3], (false, k.word_gap_ms()));
    }

    #[test]
    fn lengths_round_to_the_nearest_millisecond() {
        let mut p = Player::new();
        p.start("EE", keying(55, 55, 0), 1, 0, 650);
        assert_eq!(runs(&mut p), [(true, 22), (false, 66), (true, 22)]);
    }

    #[test]
    fn exact_timing_with_word_space() {
        let mut p = Player::new();
        p.start(" A E", keying(20, 20, 0), 1, 100, 650);
        assert_eq!(runs(&mut p), [(false, 100), (true, 60), (false, 60), (true, 180), (false, 420), (true, 60)]);
        assert!(!p.is_active());
    }

    #[test]
    fn letters_have_three_unit_gaps_and_unknowns_are_skipped() {
        let mut p = Player::new();
        p.start("E#T", keying(20, 20, 0), 1, 0, 650);
        assert_eq!(runs(&mut p), [(true, 60), (false, 180), (true, 180)]);
    }

    #[test]
    fn prosigns_send_as_one_character_from_text_or_byte() {
        let kn = crate::morse::decode("-.--.").unwrap();
        let expected = [
            (true, 180),
            (false, 60),
            (true, 60),
            (false, 60),
            (true, 180),
            (false, 60),
            (true, 180),
            (false, 60),
            (true, 60),
            (false, 180),
            (true, 60),
        ];
        for sent in [std::string::String::from("<KN>E"), std::format!("{}E", char::from(kn))] {
            let mut p = Player::new();
            p.start(&sent, keying(20, 20, 0), 1, 0, 650);
            assert_eq!(runs(&mut p), expected, "{sent:?}");
        }
    }

    #[test]
    fn fist_varies_lengths_within_bounds() {
        let mut p = Player::new();
        p.start("EEEEEEEEEE", keying(20, 20, 10), 42, 0, 650);
        let marks: Vec<u32> = runs(&mut p).into_iter().filter(|r| r.0).map(|r| r.1).collect();
        assert!(marks.iter().all(|&m| (54..=66).contains(&m)), "{marks:?}");
        assert!(marks.iter().any(|&m| m != 60));
    }
}
