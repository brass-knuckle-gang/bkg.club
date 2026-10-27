//! Memory keyer messages: four texts you key in once and send with one press, or send again and
//! again with a hold, for calling CQ until someone answers.

use core::fmt::Write;

use crate::morse;
use crate::player::MAX_TEXT;
use crate::record::{self, body_mut, frame, is_sealed, unframe};
use crate::text::Text;

pub const SLOTS: usize = 4;
pub const LEN: usize = MAX_TEXT;
pub const MIN_EVERY_S: u8 = 2;
pub const MAX_EVERY_S: u8 = 30;

const MAGIC: [u8; 2] = *b"Rm";
const VERSION: u8 = 1;
/// Set in the flags byte to keep the Home MEM row open after a send.
const STAY_OPEN: u8 = 1 << SLOTS;
/// Body: flags (bits 0 to 3 unused, then STAY_OPEN), seconds between repeats, the texts
/// zero-padded.
const BODY_LEN: usize = 2 + SLOTS * LEN;
pub const RECORD_LEN: usize = BODY_LEN + record::OVERHEAD;
/// Records before frames: magic, version, the body, checksum.
const LEGACY_MAGIC: [u8; 2] = *b"RM";
const LEGACY_LEN: usize = 3 + BODY_LEN + 1;
const LEGACY_V2: u8 = 2;
/// Legacy version 1 stored one repeating memory as its slot number plus one, or 0 for none.
const ONE_REPEAT_VERSION: u8 = 1;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Memories {
    texts: [Text<LEN>; SLOTS],
    /// Silence between the end of a repeating memory and its next start.
    pub every_s: u8,
    /// The Home MEM row stays open after a send, for working one contact after another.
    pub stay_open: bool,
}

impl Memories {
    /// Starting messages for a POTA activation from `call`.
    pub fn new(call: &str) -> Memories {
        let mut texts = [Text::new(); SLOTS];
        let _ = write!(texts[0], "CQ POTA DE {call} {call} K");
        let _ = write!(texts[1], "UR 5NN 5NN <BK>");
        let _ = write!(texts[2], "TU 72 E E");
        let _ = write!(texts[3], "QRZ DE {call} K");
        Memories { texts, every_s: 6, stay_open: false }
    }

    pub fn text(&self, i: usize) -> &str {
        self.texts[i].as_str()
    }

    /// Adds a decoded character to memory `i`, a prosign as `<AR>`; leading and repeated
    /// spaces are dropped. False when the memory has no room for it.
    pub fn push(&mut self, i: usize, c: u8) -> bool {
        let t = &mut self.texts[i];
        if c == b' ' && (t.is_empty() || t.as_str().ends_with(' ')) {
            return true;
        }
        t.write_str(morse::text(&c)).is_ok()
    }

    /// Removes the last character of memory `i`, a whole prosign at once.
    pub fn pop(&mut self, i: usize) {
        let bytes = self.texts[i].as_str().as_bytes();
        let len = bytes.len().checked_sub(4).and_then(|at| morse::first(&bytes[at..])).map_or(1, |(_, len)| len);
        for _ in 0..len {
            self.texts[i].pop();
        }
    }

    pub fn clear(&mut self, i: usize) {
        self.texts[i].clear();
    }

    pub fn step_every(&mut self, delta: i8) {
        self.every_s = (self.every_s as i32 + delta as i32).clamp(MIN_EVERY_S as i32, MAX_EVERY_S as i32) as u8;
    }

    pub fn encode(&self) -> [u8; RECORD_LEN] {
        let mut record = [0; RECORD_LEN];
        let body = body_mut(&mut record);
        let flags = if self.stay_open { STAY_OPEN } else { 0 };
        body[..2].copy_from_slice(&[flags, self.every_s]);
        for (i, t) in self.texts.iter().enumerate() {
            let at = 2 + i * LEN;
            body[at..at + t.len()].copy_from_slice(t.as_str().as_bytes());
        }
        frame(&mut record, MAGIC, VERSION);
        record
    }

    /// The record at the start of `slot`; None for a blank or foreign record, a bad check, or a
    /// value out of range.
    pub fn decode(slot: &[u8]) -> Option<Memories> {
        if let Some(body) = unframe(slot, MAGIC, VERSION) {
            return Self::from_body(body.get(..BODY_LEN)?, body[0]);
        }
        let old = slot.get(..LEGACY_LEN).filter(|old| old[..2] == LEGACY_MAGIC && is_sealed(old))?;
        let flags = match old[2] {
            ONE_REPEAT_VERSION if old[3] as usize <= SLOTS => (1u8 << old[3]) >> 1,
            LEGACY_V2 => old[3],
            _ => return None,
        };
        Self::from_body(&old[3..LEGACY_LEN - 1], flags)
    }

    /// The body, with its flags byte read as `flags`.
    fn from_body(body: &[u8], flags: u8) -> Option<Memories> {
        let every_s = body[1];
        if flags >> (SLOTS + 1) != 0 {
            return None;
        }
        if !(MIN_EVERY_S..=MAX_EVERY_S).contains(&every_s) {
            return None;
        }
        let mut texts = [Text::new(); SLOTS];
        for (i, t) in texts.iter_mut().enumerate() {
            let bytes = &body[2 + i * LEN..2 + (i + 1) * LEN];
            let len = bytes.iter().position(|&b| b == 0).unwrap_or(LEN);
            if !bytes[..len].iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
                return None;
            }
            for &b in &bytes[..len] {
                t.push(b);
            }
        }
        Some(Memories { texts, every_s, stay_open: flags & STAY_OPEN != 0 })
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::seal;

    #[test]
    fn editing_and_round_trip() {
        let mut m = Memories::new("K5MME");
        assert_eq!(m.text(0), "CQ POTA DE K5MME K5MME K");
        m.clear(1);
        for &c in b" TU  73" {
            m.push(1, c);
        }
        assert_eq!(m.text(1), "TU 73");
        m.pop(1);
        assert_eq!(m.text(1), "TU 7");
        m.push(1, morse::decode("...-.-").unwrap());
        assert_eq!(m.text(1), "TU 7<SK>");
        m.pop(1);
        assert_eq!(m.text(1), "TU 7");
        for _ in 0..LEN {
            m.push(2, b'E');
        }
        assert!(!m.push(2, b'E'));
        m.step_every(100);
        m.stay_open = true;
        assert_eq!(Memories::decode(&m.encode()), Some(m));
        assert_eq!(Memories::decode(&[0xFF; RECORD_LEN]), None);
    }

    #[test]
    fn legacy_records_keep_their_texts_and_stay_open() {
        let mut record = [0u8; LEGACY_LEN];
        record[..3].copy_from_slice(&[b'R', b'M', ONE_REPEAT_VERSION]);
        record[3..3 + BODY_LEN].copy_from_slice(&Memories::new("K5MME").encode()[4..4 + BODY_LEN]);
        record[3] = 2;
        seal(&mut record);
        let v1 = Memories::decode(&record).unwrap();
        assert_eq!((v1.text(0), v1.stay_open), ("CQ POTA DE K5MME K5MME K", false));
        record[2] = LEGACY_V2;
        record[3] = 0b1_0010;
        seal(&mut record);
        assert!(Memories::decode(&record).unwrap().stay_open);
    }

    #[test]
    fn repeat_bits_from_older_saves_still_read() {
        let mut record = Memories::new("K5MME").encode();
        body_mut(&mut record)[0] = 0b0001;
        frame(&mut record, MAGIC, VERSION);
        assert_eq!(Memories::decode(&record), Some(Memories::new("K5MME")));
    }
}
