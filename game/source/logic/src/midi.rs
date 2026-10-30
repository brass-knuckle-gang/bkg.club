//! USB MIDI: the keyed line as Note On and Note Off, and the messages Vail's page sends.

/// One USB MIDI event: the cable and code index, then the MIDI message.
pub type Packet = [u8; 4];

/// Vail reads note 0 as a straight key in every one of its keyer modes, so every key type works.
pub const VAIL_NOTE: u8 = 0;

pub fn note_on(note: u8) -> Packet {
    [0x09, 0x90, note, 0x7F]
}

/// A real Note Off: Vail reads any Note On, velocity 0 included, as a press.
pub fn note_off(note: u8) -> Packet {
    [0x08, 0x80, note, 0x00]
}

/// Vail's page sends Control Change 0 = 0 on channel 1 to every MIDI output when it loads.
pub fn is_vail_hello(packet: &Packet) -> bool {
    packet[1..] == [0xB0, 0x00, 0x00]
}

/// The MIDI note nearest `hz`, from 0 (8.18 Hz) to 127.
pub fn nearest_note(hz: u16) -> u8 {
    const SEMITONE: f32 = 1.059_463_1;
    const QUARTER_TONE: f32 = 1.029_302_2;
    let mut note_hz = 8.175_799;
    for note in 0..127 {
        if (hz as f32) < note_hz * QUARTER_TONE {
            return note;
        }
        note_hz *= SEMITONE;
    }
    127
}

/// The pitch of MIDI note `note`, rounded to whole Hz.
pub fn note_hz(note: u8) -> u16 {
    const SEMITONE: f32 = 1.059_463_1;
    let mut hz = 8.175_799;
    for _ in 0..note.min(127) {
        hz *= SEMITONE;
    }
    (hz + 0.5) as u16
}

/// A remote tone longer than this ends, and every held note is dropped, so a lost Note Off
/// cannot hold the speaker on.
pub const MAX_MARK_MS: u32 = 3000;

/// The notes the computer holds down. Overlapping senders sound as one tone, at the pitch of the
/// newest note.
pub struct In {
    /// Note Ons minus Note Offs, per note.
    counts: [u8; 128],
    held: u16,
    hz: u16,
    on_ms: u32,
}

impl In {
    pub const fn new() -> In {
        In { counts: [0; 128], held: 0, hz: 0, on_ms: 0 }
    }

    /// Takes one packet from the computer; anything but a note is ignored.
    pub fn packet(&mut self, packet: &Packet) {
        let [_, status, note, velocity] = *packet;
        let count = &mut self.counts[note as usize & 0x7F];
        match status & 0xF0 {
            0x90 if velocity > 0 => {
                if *count == 0 {
                    self.held += 1;
                }
                *count = count.saturating_add(1);
                self.hz = note_hz(note & 0x7F);
            }
            0x80 | 0x90 if *count > 0 => {
                *count -= 1;
                if *count == 0 {
                    self.held -= 1;
                }
            }
            _ => {}
        }
    }

    /// Steps 1 ms: whether the tone sounds.
    pub fn tick(&mut self) -> bool {
        if self.held == 0 {
            self.on_ms = 0;
            return false;
        }
        self.on_ms += 1;
        if self.on_ms > MAX_MARK_MS {
            *self = In { hz: self.hz, ..In::new() };
            return false;
        }
        true
    }

    /// The newest note's pitch, kept after it ends so the tone fades out at it.
    pub fn hz(&self) -> u16 {
        self.hz
    }
}

/// The note the computer last took, so each mark gets one Note On and one Note Off.
pub struct Out {
    sounding: Option<u8>,
}

impl Out {
    pub const fn new() -> Out {
        Out { sounding: None }
    }

    /// The packet that moves the computer toward `note` sounding while `keyed`, if any. A note
    /// change while sounding ends the old note first.
    pub fn next(&self, keyed: bool, note: u8) -> Option<Packet> {
        match self.sounding {
            Some(n) if !keyed || n != note => Some(note_off(n)),
            None if keyed => Some(note_on(note)),
            _ => None,
        }
    }

    /// The computer took `packet`, one that `next` gave.
    pub fn sent(&mut self, packet: Packet) {
        self.sounding = (packet[1] == 0x90).then_some(packet[2]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_note_rounds_to_the_closer_semitone() {
        for (hz, note) in [(440, 69), (600, 74), (700, 77), (880, 81), (8, 0), (20_000, 127), (452, 69), (453, 70)] {
            assert_eq!(nearest_note(hz), note, "{hz}");
        }
    }

    #[test]
    fn a_mark_is_one_note_on_and_one_note_off() {
        let mut out = Out::new();
        assert_eq!(out.next(false, 74), None);
        let on = out.next(true, 74).unwrap();
        assert_eq!(on, [0x09, 0x90, 74, 0x7F]);
        assert_eq!(out.next(true, 74), Some(on), "unsent until the computer takes it");
        out.sent(on);
        assert_eq!(out.next(true, 74), None);
        let off = out.next(false, 74).unwrap();
        assert_eq!(off, [0x08, 0x80, 74, 0x00]);
        out.sent(off);
        assert_eq!(out.next(false, 74), None);
    }

    #[test]
    fn a_note_change_ends_the_old_note_first() {
        let mut out = Out::new();
        out.sent(out.next(true, 74).unwrap());
        let off = out.next(true, VAIL_NOTE).unwrap();
        assert_eq!(off, note_off(74));
        out.sent(off);
        assert_eq!(out.next(true, VAIL_NOTE), Some(note_on(VAIL_NOTE)));
    }

    #[test]
    fn note_hz_matches_equal_temperament() {
        for (note, hz) in [(0, 8), (60, 262), (69, 440), (72, 523), (127, 12544)] {
            assert_eq!(note_hz(note), hz, "{note}");
        }
    }

    #[test]
    fn in_sounds_while_any_note_is_held() {
        let mut remote = In::new();
        assert!(!remote.tick());
        remote.packet(&note_on(72));
        assert!(remote.tick());
        assert_eq!(remote.hz(), 523);
        remote.packet(&note_on(69));
        remote.packet(&note_off(72));
        assert!(remote.tick());
        assert_eq!(remote.hz(), 440, "the newest note's pitch");
        remote.packet(&[0x09, 0x90, 69, 0]);
        assert!(!remote.tick(), "Note On at velocity 0 is a Note Off");
        assert_eq!(remote.hz(), 440, "the pitch stays for the fade out");
        remote.packet(&note_off(69));
        remote.packet(&[0x0B, 0xB0, 1, 30]);
        assert!(!remote.tick(), "an extra Note Off and a CC change nothing");
    }

    #[test]
    fn in_drops_a_stuck_note_after_max_mark() {
        let mut remote = In::new();
        remote.packet(&note_on(72));
        for _ in 0..MAX_MARK_MS {
            assert!(remote.tick());
        }
        assert!(!remote.tick());
        assert_eq!(remote.hz(), 523);
        assert!(!remote.tick(), "stays off with no Note Off");
        remote.packet(&note_on(72));
        assert!(remote.tick(), "the next Note On sounds");
    }

    #[test]
    fn vail_hello_is_cc0_zero_on_channel_1() {
        assert!(is_vail_hello(&[0x0B, 0xB0, 0, 0]));
        assert!(!is_vail_hello(&[0x0B, 0xB1, 0, 0]));
        assert!(!is_vail_hello(&[0x0B, 0xB0, 0, 1]));
    }
}
