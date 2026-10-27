//! USB keyboard output: decoded characters as key presses on a US layout, and VBand's Ctrl keys.

use crate::decoder::Decoded;
use crate::keyer::Mode;
use crate::morse;

/// HID modifier bits.
pub const LEFT_CTRL: u8 = 0x01;
pub const LEFT_SHIFT: u8 = 0x02;
pub const RIGHT_CTRL: u8 = 0x10;

/// A keyboard report: the modifier bits and one key's usage, 0 for none.
pub type Report = (u8, u8);
pub const NO_KEYS: Report = (0, 0);

/// Characters waiting to type. A prosign takes 4, so this holds several words.
const QUEUE: usize = 64;

/// The report that types `c` on a US layout; letters type lowercase. None for a byte no key types.
pub fn key(c: u8) -> Option<Report> {
    let shifted = |usage| Some((LEFT_SHIFT, usage));
    match c.to_ascii_lowercase() {
        c @ b'a'..=b'z' => Some((0, 0x04 + c - b'a')),
        c @ b'1'..=b'9' => Some((0, 0x1E + c - b'1')),
        b'0' => Some((0, 0x27)),
        b' ' => Some((0, 0x2C)),
        b'-' => Some((0, 0x2D)),
        b'=' => Some((0, 0x2E)),
        b'\'' => Some((0, 0x34)),
        b',' => Some((0, 0x36)),
        b'.' => Some((0, 0x37)),
        b'/' => Some((0, 0x38)),
        b'@' => shifted(0x1F),
        b'*' => shifted(0x25),
        b'(' => shifted(0x26),
        b')' => shifted(0x27),
        b'+' => shifted(0x2E),
        b':' => shifted(0x33),
        b'"' => shifted(0x34),
        b'<' => shifted(0x36),
        b'>' => shifted(0x37),
        b'?' => shifted(0x38),
        _ => None,
    }
}

/// VBand's Ctrl keys. Iambic modes send the paddle contacts, as the VBand adapter does, so VBand
/// runs its own keyer. Straight mode sends the keyed line on Left Ctrl, for VBand's straight key.
pub fn vband(mode: Mode, dit: bool, dah: bool, keyed: bool) -> u8 {
    let ctrl = |down: bool, bit: u8| if down { bit } else { 0 };
    match mode {
        Mode::IambicA | Mode::IambicB => ctrl(dit, LEFT_CTRL) | ctrl(dah, RIGHT_CTRL),
        Mode::Straight => ctrl(keyed, LEFT_CTRL),
    }
}

/// The reports that type one key, and how many of them there are. Shift goes down alone before
/// the key and comes up after it: sent in the key's own report, it was missed and `*` typed `8`.
fn strokes((shift, key): Report) -> ([Report; 4], usize) {
    if shift == 0 {
        ([(0, key), NO_KEYS, NO_KEYS, NO_KEYS], 2)
    } else {
        ([(shift, 0), (shift, key), (shift, 0), NO_KEYS], 4)
    }
}

/// Types decoded characters as the screen writes them: `*` for an unknown pattern, `<AR>` for a
/// prosign, and one space between words.
pub struct Typist {
    queue: [u8; QUEUE],
    head: usize,
    len: usize,
    /// The reports of the first character's `strokes` already sent.
    step: usize,
    /// The last character queued, so a word gap types one space and none before the first word.
    last: u8,
}

impl Default for Typist {
    fn default() -> Self {
        Typist { queue: [0; QUEUE], head: 0, len: 0, step: 0, last: b' ' }
    }
}

impl Typist {
    /// Queues what `decoded` types. False when the queue is full; the characters that did not
    /// fit are lost.
    pub fn push(&mut self, decoded: Decoded) -> bool {
        let text = match &decoded {
            Decoded::Char(c) => morse::text(c),
            Decoded::Unknown => "*",
            Decoded::Space if self.last == b' ' => return true,
            Decoded::Space => " ",
        };
        text.bytes().filter(|&c| key(c).is_some()).all(|c| {
            if self.len == QUEUE {
                return false;
            }
            self.queue[(self.head + self.len) % QUEUE] = c;
            self.len += 1;
            self.last = c;
            true
        })
    }

    /// The report to send now: the next of the first character's `strokes`.
    pub fn report(&self) -> Report {
        match self.len {
            0 => NO_KEYS,
            _ => strokes(key(self.queue[self.head]).unwrap_or(NO_KEYS)).0[self.step],
        }
    }

    /// The host took `report`: step to the next one.
    pub fn sent(&mut self) {
        if self.len == 0 {
            return;
        }
        self.step += 1;
        if self.step == strokes(key(self.queue[self.head]).unwrap_or(NO_KEYS)).1 {
            self.step = 0;
            self.head = (self.head + 1) % QUEUE;
            self.len -= 1;
        }
    }

    /// Drops everything queued, as when the host goes away.
    pub fn clear(&mut self) {
        *self = Typist::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every report the typist makes until it is empty.
    fn typed(typist: &mut Typist) -> impl Iterator<Item = Report> + '_ {
        core::iter::from_fn(move || {
            (typist.len > 0).then(|| {
                let report = typist.report();
                typist.sent();
                report
            })
        })
    }

    #[test]
    fn letters_type_lowercase_and_each_press_is_released() {
        let mut t = Typist::default();
        for c in *b"Hi" {
            assert!(t.push(Decoded::Char(c)));
        }
        assert!(typed(&mut t).eq([(0, 0x0B), NO_KEYS, (0, 0x0C), NO_KEYS]));
        assert_eq!(t.report(), NO_KEYS);
    }

    #[test]
    fn word_gaps_type_one_space_and_none_before_the_first_word() {
        let mut t = Typist::default();
        for decoded in [Decoded::Space, Decoded::Char(b'E'), Decoded::Space, Decoded::Space, Decoded::Char(b'T')] {
            t.push(decoded);
        }
        assert!(typed(&mut t).eq([(0, 0x08), NO_KEYS, (0, 0x2C), NO_KEYS, (0, 0x17), NO_KEYS]));
    }

    #[test]
    fn prosigns_and_unknown_patterns_type_as_the_screen_shows_them() {
        let mut t = Typist::default();
        let ar = morse::decode(".-.-.").unwrap();
        t.push(Decoded::Char(ar));
        t.push(Decoded::Unknown);
        let s = LEFT_SHIFT;
        let expected = [
            (s, 0), (s, 0x36), (s, 0), NO_KEYS,
            (0, 0x04), NO_KEYS,
            (0, 0x15), NO_KEYS,
            (s, 0), (s, 0x37), (s, 0), NO_KEYS,
            (s, 0), (s, 0x25), (s, 0), NO_KEYS,
        ];
        assert!(typed(&mut t).eq(expected));
    }

    #[test]
    fn shift_goes_down_before_its_key_and_up_after_it() {
        let mut t = Typist::default();
        t.push(Decoded::Char(b'?'));
        t.push(Decoded::Char(b'?'));
        let one = [(LEFT_SHIFT, 0), (LEFT_SHIFT, 0x38), (LEFT_SHIFT, 0), NO_KEYS];
        assert!(typed(&mut t).eq(one.into_iter().chain(one)));
    }

    #[test]
    fn every_character_morse_knows_has_a_key() {
        for c in b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789/?.,=:'-)\"@(+" {
            assert!(morse::code(*c).is_some() && key(*c).is_some(), "{}", *c as char);
        }
    }

    #[test]
    fn a_full_queue_drops_what_does_not_fit() {
        let mut t = Typist::default();
        assert!((0..QUEUE).all(|_| t.push(Decoded::Char(b'E'))));
        assert!(!t.push(Decoded::Char(b'T')));
        t.clear();
        assert_eq!(t.report(), NO_KEYS);
    }

    #[test]
    fn vband_sends_contacts_in_iambic_and_the_keyed_line_in_straight() {
        assert_eq!(vband(Mode::IambicB, true, true, false), LEFT_CTRL | RIGHT_CTRL);
        assert_eq!(vband(Mode::IambicA, false, true, true), RIGHT_CTRL);
        assert_eq!(vband(Mode::Straight, false, true, true), LEFT_CTRL);
        assert_eq!(vband(Mode::Straight, true, false, false), 0);
    }
}
