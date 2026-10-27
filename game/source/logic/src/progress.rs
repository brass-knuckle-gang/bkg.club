//! Practice history: one record per session (power-on), kept for the last `SLOTS` sessions.
//! There is no clock, so sessions stand in for days. Each slot saves on its own, so a save
//! writes only the session in progress.

use crate::math::sort_by_key;
use crate::record::{is_sealed, seal};

pub const SLOTS: usize = 32;
pub const SLOT_LEN: usize = 12;
pub const RECORD_LEN: usize = SLOTS * SLOT_LEN;
/// A gap between activities longer than this is a break, not practice.
const IDLE_MS: u32 = 60_000;
/// The session in progress saves at most this often, and again at power off.
const SAVE_EVERY_MS: u32 = 60_000;

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Session {
    /// Counts up from 1 across sessions; the highest is the newest.
    pub seq: u16,
    pub active_s: u16,
    /// QSO and hunt contacts logged.
    pub contacts: u8,
    /// Characters sent in receive drills, and how many you copied right.
    pub rx_chars: u16,
    pub rx_right: u16,
    /// Characters you sent on the key, anywhere.
    pub sent_chars: u16,
}

impl Session {
    fn encode(&self) -> [u8; SLOT_LEN] {
        let mut s = [0; SLOT_LEN];
        s[0..2].copy_from_slice(&self.seq.to_le_bytes());
        s[2..4].copy_from_slice(&self.active_s.to_le_bytes());
        s[4] = self.contacts;
        s[5..7].copy_from_slice(&self.rx_chars.to_le_bytes());
        s[7..9].copy_from_slice(&self.rx_right.to_le_bytes());
        s[9..11].copy_from_slice(&self.sent_chars.to_le_bytes());
        seal(&mut s);
        s
    }

    /// None for a blank slot or a bad checksum.
    fn decode(s: &[u8]) -> Option<Session> {
        let word = |at: usize| u16::from_le_bytes([s[at], s[at + 1]]);
        let session = Session {
            seq: word(0),
            active_s: word(2),
            contacts: s[4],
            rx_chars: word(5),
            rx_right: word(7),
            sent_chars: word(9),
        };
        (is_sealed(&s[..SLOT_LEN]) && session.rx_right <= session.rx_chars).then_some(session)
    }

    pub fn minutes(&self) -> u16 {
        self.active_s / 60
    }

    /// Receive drill accuracy, once you have copied anything.
    pub fn rx_percent(&self) -> Option<u8> {
        (self.rx_chars > 0).then(|| (self.rx_right as u32 * 100 / self.rx_chars as u32) as u8)
    }
}

pub struct History {
    slots: [Option<Session>; SLOTS],
    /// The slot of this power-on's session, once you have done anything.
    current: Option<usize>,
    active_ms: u32,
    last_active: Option<u32>,
    dirty: bool,
    saved_at: Option<u32>,
}

impl History {
    pub fn decode(record: &[u8; RECORD_LEN]) -> History {
        let mut slots = [None; SLOTS];
        for (slot, bytes) in slots.iter_mut().zip(record.chunks_exact(SLOT_LEN)) {
            *slot = Session::decode(bytes);
        }
        History { slots, current: None, active_ms: 0, last_active: None, dirty: false, saved_at: None }
    }

    /// Practice at `now_ms`: starts this session's slot, and counts the time since the last
    /// activity unless it was a break.
    pub fn active(&mut self, now_ms: u32) {
        let i = match self.current {
            Some(i) => i,
            None => self.start(),
        };
        if let Some(last) = self.last_active {
            let gap = now_ms.wrapping_sub(last);
            if gap <= IDLE_MS {
                self.active_ms += gap;
            }
        }
        self.last_active = Some(now_ms);
        let session = self.slots[i].get_or_insert_with(Session::default);
        let active_s = (self.active_ms / 1000).min(u16::MAX as u32) as u16;
        if session.active_s != active_s {
            session.active_s = active_s;
            self.dirty = true;
        }
    }

    pub fn sent(&mut self) {
        self.update(|s| s.sent_chars = s.sent_chars.saturating_add(1));
    }

    pub fn copied(&mut self, right: u8, total: u8) {
        self.update(|s| {
            // Saturate both together so right never passes the total.
            if let Some(chars) = s.rx_chars.checked_add(total as u16) {
                s.rx_chars = chars;
                s.rx_right += right as u16;
            }
        });
    }

    pub fn contact(&mut self) {
        self.update(|s| s.contacts = s.contacts.saturating_add(1));
    }

    fn update(&mut self, change: impl FnOnce(&mut Session)) {
        let i = match self.current {
            Some(i) => i,
            None => self.start(),
        };
        if let Some(session) = self.slots[i].as_mut() {
            change(session);
            self.dirty = true;
        }
    }

    /// The slot after the newest one, wiping the oldest when all are full.
    fn start(&mut self) -> usize {
        let newest = self.slots.iter().enumerate().filter_map(|(i, s)| Some((i, s.as_ref()?.seq))).max_by_key(|&(_, seq)| seq);
        let (i, seq) = match newest {
            Some((i, seq)) => ((i + 1) % SLOTS, seq.wrapping_add(1).max(1)),
            None => (0, 1),
        };
        self.slots[i] = Some(Session { seq, ..Session::default() });
        self.current = Some(i);
        self.dirty = true;
        i
    }

    /// This session's slot index and bytes when they should be saved: at most once a
    /// `SAVE_EVERY_MS`, or now with `force`.
    pub fn take_save(&mut self, now_ms: u32, force: bool) -> Option<(usize, [u8; SLOT_LEN])> {
        let i = self.current?;
        let due = force || self.saved_at.is_none_or(|at| now_ms.wrapping_sub(at) >= SAVE_EVERY_MS);
        if !self.dirty || !due {
            return None;
        }
        self.dirty = false;
        self.saved_at = Some(now_ms);
        Some((i, self.slots[i]?.encode()))
    }

    /// Sessions oldest first, and how many there are.
    pub fn sessions(&self) -> ([Session; SLOTS], usize) {
        let mut sorted = [Session::default(); SLOTS];
        let mut n = 0;
        for s in self.slots.iter().flatten() {
            sorted[n] = *s;
            n += 1;
        }
        sort_by_key(&mut sorted[..n], |s| s.seq);
        (sorted, n)
    }

    /// The session in progress, if you have done anything.
    pub fn current(&self) -> Option<Session> {
        self.slots[self.current?]
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn blank() -> History {
        History::decode(&[0xFF; RECORD_LEN])
    }

    #[test]
    fn nothing_saves_until_you_practice() {
        let mut h = blank();
        assert!(h.take_save(0, true).is_none());
        assert_eq!(h.sessions().1, 0);
    }

    #[test]
    fn breaks_do_not_count_as_practice_time() {
        let mut h = blank();
        h.active(1000);
        h.active(31_000);
        h.active(200_000);
        h.active(210_000);
        assert_eq!(h.current().unwrap().active_s, 40);
    }

    #[test]
    fn saves_round_trip_and_the_next_session_follows() {
        let mut h = blank();
        h.sent();
        h.copied(4, 5);
        h.contact();
        let (i, slot) = h.take_save(20, false).unwrap();
        assert!(h.take_save(30, false).is_none(), "nothing changed");
        h.sent();
        assert!(h.take_save(50, false).is_none(), "saved under a minute ago");
        assert!(h.take_save(50, true).is_some());

        let mut record = [0xFF; RECORD_LEN];
        record[i * SLOT_LEN..(i + 1) * SLOT_LEN].copy_from_slice(&slot);
        let mut next = History::decode(&record);
        let s = next.sessions().0[0];
        assert_eq!((s.seq, s.sent_chars, s.rx_percent(), s.contacts), (1, 1, Some(80), 1));
        next.sent();
        let (j, _) = next.take_save(0, true).unwrap();
        assert_eq!((j, next.current().unwrap().seq), (i + 1, 2));
    }

    #[test]
    fn a_full_ring_replaces_the_oldest() {
        let mut record = [0xFF; RECORD_LEN];
        for k in 0..SLOTS {
            let s = Session { seq: k as u16 + 1, ..Session::default() };
            record[k * SLOT_LEN..(k + 1) * SLOT_LEN].copy_from_slice(&s.encode());
        }
        let mut h = History::decode(&record);
        h.sent();
        let (sessions, n) = h.sessions();
        assert_eq!((n, sessions[0].seq, sessions[n - 1].seq), (SLOTS, 2, SLOTS as u16 + 1));
    }
}
