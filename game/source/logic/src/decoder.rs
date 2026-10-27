//! Turns key up and down timing into characters, stepped once per millisecond.

use core::fmt::Write;

use crate::listen::Listener;
use crate::morse;
use crate::text::Text;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Decoded {
    /// A character, one byte; a prosign is its control byte, and `morse::text` writes it.
    Char(u8),
    /// A pattern no character uses.
    Unknown,
    /// A gap long enough to end a word.
    Space,
}

impl Decoded {
    /// Adds this to copied text: `*` for a pattern no character uses, and a space only between
    /// words. False when nothing was added, including when `text` is full.
    pub fn write_to<const N: usize>(&self, text: &mut Text<N>) -> bool {
        let written = match self {
            Decoded::Char(c) => morse::text(c),
            Decoded::Unknown => "*",
            Decoded::Space if text.is_empty() || text.as_str().ends_with(' ') => return false,
            Decoded::Space => " ",
        };
        text.write_str(written).is_ok()
    }
}

/// Key-up silence that ends your over or copy: three word gaps.
pub const COPY_END_UNITS: u32 = 21;

/// When your over or copy ends if the key stays up from `up_at_ms`.
pub fn copy_end(up_at_ms: u32, dit_ms: u16) -> u32 {
    up_at_ms.wrapping_add(COPY_END_UNITS * dit_ms as u32)
}

/// A mark this many dots or longer is a dah: halfway between the 1-dot dit and the 3-dot dah.
pub const DAH_DOTS: u16 = 2;
/// Longest pattern kept; longer ones decode as `Unknown`.
pub const MAX_ELEMENTS: usize = 8;
/// A gap of this many dots ends the character: halfway between the 1-dot element gap and the
/// 3-dot letter gap.
pub const CHAR_GAP_DOTS: u16 = 2;
/// A gap of this many dots ends the word: halfway between the 3-dot letter gap and the 7-dot word
/// gap.
pub const WORD_GAP_DOTS: u16 = 5;

#[derive(Default)]
pub struct Decoder {
    mark_ms: u16,
    space_ms: u16,
    pattern: [u8; MAX_ELEMENTS],
    len: usize,
    overflow: bool,
    word_open: bool,
}

impl Decoder {
    /// `dit_ms` sets the thresholds halfway between standard lengths: DAH_DOTS, CHAR_GAP_DOTS and
    /// WORD_GAP_DOTS.
    pub fn tick(&mut self, down: bool, dit_ms: u16) -> Option<Decoded> {
        if down {
            self.space_ms = 0;
            self.mark_ms = self.mark_ms.saturating_add(1);
            return None;
        }
        if self.mark_ms > 0 {
            let element = if self.mark_ms >= DAH_DOTS * dit_ms { b'-' } else { b'.' };
            if self.len < MAX_ELEMENTS {
                self.pattern[self.len] = element;
                self.len += 1;
            } else {
                self.overflow = true;
            }
            self.mark_ms = 0;
        }
        self.space_ms = self.space_ms.saturating_add(1);
        if self.len > 0 && self.space_ms >= CHAR_GAP_DOTS * dit_ms {
            let pattern = core::str::from_utf8(&self.pattern[..self.len]).unwrap_or("");
            let decoded = match morse::decode(pattern) {
                Some(c) if !self.overflow => Decoded::Char(c),
                _ => Decoded::Unknown,
            };
            self.len = 0;
            self.overflow = false;
            self.word_open = true;
            return Some(decoded);
        }
        if self.word_open && self.space_ms >= WORD_GAP_DOTS * dit_ms {
            self.word_open = false;
            return Some(Decoded::Space);
        }
        None
    }
}

/// Decodes your keying. Keyer output has exact timing, so it decodes at the set speed. A straight
/// key or sideswiper decodes at the speed you send, starting from the set speed.
pub struct KeyDecoder {
    fixed: Decoder,
    /// The straight key decoder, and the set dot length it started from.
    adaptive: Option<(u16, Listener)>,
    dit_ms: u16,
}

impl KeyDecoder {
    pub fn new(dit_ms: u16) -> KeyDecoder {
        KeyDecoder { fixed: Decoder::default(), adaptive: None, dit_ms }
    }

    /// `dit_ms` is the set speed's.
    pub fn tick(&mut self, down: bool, dit_ms: u16, straight: bool) -> Option<Decoded> {
        if !straight {
            self.adaptive = None;
            self.dit_ms = dit_ms;
            return self.fixed.tick(down, dit_ms);
        }
        let listener = match &mut self.adaptive {
            Some((from, listener)) if *from == dit_ms => listener,
            adaptive => {
                self.fixed = Decoder::default();
                &mut adaptive.insert((dit_ms, Listener::at(dit_ms))).1
            }
        };
        let decoded = listener.tick(down);
        self.dit_ms = listener.dot_ms();
        decoded
    }

    /// The dot length decoding uses now.
    pub fn dit_ms(&self) -> u16 {
        self.dit_ms
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::keyer::{Contacts, Keyer, Mode};
    use std::vec::Vec;

    #[test]
    fn copied_text_keeps_one_space_between_words() {
        let mut text = Text::<4>::new();
        assert!(!Decoded::Space.write_to(&mut text));
        assert!(Decoded::Char(b'K').write_to(&mut text));
        assert!(Decoded::Space.write_to(&mut text));
        assert!(!Decoded::Space.write_to(&mut text));
        assert!(Decoded::Unknown.write_to(&mut text));
        assert_eq!(text.as_str(), "K *");
        assert!(!Decoded::Char(0x07).write_to(&mut text), "a prosign that doesn't fit");
        assert_eq!(text.as_str(), "K *");
    }

    #[test]
    fn copy_end_wraps_with_the_tick_count() {
        assert_eq!(copy_end(1000, 60), 1000 + 21 * 60);
        assert_eq!(copy_end(u32::MAX, 50), 21 * 50 - 1);
    }

    /// Feeds runs of (down, ms) and collects the output.
    fn decode(runs: &[(bool, u16)], dit_ms: u16) -> Vec<Decoded> {
        let mut d = Decoder::default();
        let mut out = Vec::new();
        for &(down, ms) in runs {
            for _ in 0..ms {
                out.extend(d.tick(down, dit_ms));
            }
        }
        out
    }

    #[test]
    fn letters_and_word_space() {
        // K = -.-, then a word gap, then E = .
        let runs = [(true, 180), (false, 60), (true, 60), (false, 60), (true, 180), (false, 420), (true, 60), (false, 200)];
        assert_eq!(decode(&runs, 60), [Decoded::Char(b'K'), Decoded::Space, Decoded::Char(b'E')]);
    }

    #[test]
    fn unknown_pattern() {
        let runs = [(true, 60), (false, 60), (true, 60), (false, 60), (true, 60), (false, 60), (true, 60), (false, 60),
            (true, 60), (false, 60), (true, 60), (false, 60), (true, 60), (false, 60), (true, 60), (false, 60),
            (true, 60), (false, 200)];
        assert_eq!(decode(&runs, 60), [Decoded::Unknown]);
    }

    /// Key runs for `text` with dots of `dot_ms` and standard spacing, then silence.
    fn straight_key(text: &str, dot_ms: u16) -> Vec<(bool, u16)> {
        let mut runs = Vec::new();
        for word in text.split(' ') {
            for c in word.bytes() {
                for element in morse::code(c).unwrap().bytes() {
                    runs.push((true, if element == b'-' { 3 * dot_ms } else { dot_ms }));
                    runs.push((false, dot_ms));
                }
                runs.last_mut().unwrap().1 = 3 * dot_ms;
            }
            runs.last_mut().unwrap().1 = 7 * dot_ms;
        }
        runs.push((false, 20 * dot_ms));
        runs
    }

    fn copy(decoded: &[Decoded]) -> std::string::String {
        let mut text = Text::<40>::new();
        for d in decoded {
            d.write_to(&mut text);
        }
        std::string::String::from(text.as_str().trim_end())
    }

    #[test]
    fn a_straight_key_decodes_at_the_speed_it_sends() {
        let set_dit = crate::keyer::dit_ms(25);
        let runs = straight_key("PARIS CODE", 80);
        assert_ne!(copy(&decode(&runs, set_dit)), "PARIS CODE", "the set speed alone misreads 15 WPM");
        let mut d = KeyDecoder::new(set_dit);
        let mut out = Vec::new();
        for &(down, ms) in &runs {
            for _ in 0..ms {
                out.extend(d.tick(down, set_dit, true));
            }
        }
        assert_eq!(copy(&out), "PARIS CODE");
        assert_eq!(d.dit_ms(), 80);
    }

    #[test]
    fn a_straight_key_at_the_set_speed_decodes_from_the_first_mark() {
        let set_dit = crate::keyer::dit_ms(8);
        let mut d = KeyDecoder::new(set_dit);
        let mut out = Vec::new();
        for &(down, ms) in &straight_key("EI", set_dit) {
            for _ in 0..ms {
                out.extend(d.tick(down, set_dit, true));
            }
        }
        assert_eq!(copy(&out), "EI");
    }

    #[test]
    fn keyer_output_decodes_at_the_set_speed() {
        let mut d = KeyDecoder::new(48);
        for &(down, ms) in &straight_key("PARIS", 80) {
            for _ in 0..ms {
                d.tick(down, 48, false);
            }
        }
        assert_eq!(d.dit_ms(), 48);
    }

    #[test]
    fn decodes_keyer_output() {
        // A dah, with the dit paddle tapped during it so iambic B memory sends a dit next: N.
        let mut keyer = Keyer::new(Mode::IambicB, false, 25);
        let mut d = Decoder::default();
        let mut out = Vec::new();
        let dah = Contacts { tip: false, ring: true };
        let dit = Contacts { tip: true, ring: false };
        let none = Contacts::default();
        for (contacts, ms) in [(dah, 10), (none, 100), (dit, 10), (none, 400)] {
            for _ in 0..ms {
                let down = keyer.tick(contacts);
                out.extend(d.tick(down, keyer.dit_ms()));
            }
        }
        assert_eq!(out, [Decoded::Char(b'N'), Decoded::Space]);
    }
}
