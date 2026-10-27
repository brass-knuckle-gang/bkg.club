//! Copy accuracy per character and the most common mixups, kept across sessions.

use crate::math::{sort_by_key, sqrt};
use crate::record::{self, body_mut, frame, is_sealed, unframe};

/// Every drilled character: the Koch order LCWO uses, then the prosigns AR, SK and KN as
/// `morse` writes them in one byte.
pub const KOCH: &[u8; 44] = b"KMURESNAPTLWI.JZ=FOY,VG5/Q92H38B?47C1D60X\x01\x07\x06";
pub const MIXUP_SLOTS: usize = 11;
/// Counts halve when a character reaches this many tries, so they follow recent copying.
const DECAY_AT: u8 = 200;

const MAGIC: [u8; 2] = *b"Rl";
const VERSION: u8 = 1;
/// Body: level, tries and right per KOCH character, then sent, copied and count per mixup.
const fn body_len(chars: usize, mixups: usize) -> usize {
    1 + 2 * chars + 3 * mixups
}
const BODY_LEN: usize = body_len(KOCH.len(), MIXUP_SLOTS);
pub const RECORD_LEN: usize = BODY_LEN + record::OVERHEAD;
/// Records before frames: magic, version, the body, checksum.
const LEGACY_MAGIC: [u8; 2] = *b"RL";
const LEGACY_V2: u8 = 2;
/// Legacy version 1 had no prosigns and one more mixup slot.
const LEGACY_V1: u8 = 1;
const V1_CHARS: usize = 41;
const V1_MIXUPS: usize = 12;
/// Characters in the first Koch lesson: K and M.
pub const FIRST_LEVEL: u8 = 2;

/// A character sent as `sent` and copied as `copied`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Mixup {
    pub sent: u8,
    pub copied: u8,
    pub count: u8,
}

/// Koch's mark for a character you know: 90% copied right, the rule the drill adds characters by.
const KNOWN: f32 = 0.90;
/// The normal quantile for a 95% interval.
const Z_95: f32 = 1.96;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mastery {
    Known,
    /// Too few tries to tell.
    Unsure,
    Weak,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Letters {
    /// Koch characters in play.
    pub level: u8,
    tries: [u8; KOCH.len()],
    right: [u8; KOCH.len()],
    mixups: [Mixup; MIXUP_SLOTS],
}

impl Default for Letters {
    fn default() -> Self {
        Letters { level: FIRST_LEVEL, tries: [0; KOCH.len()], right: [0; KOCH.len()], mixups: [Mixup::default(); MIXUP_SLOTS] }
    }
}

pub fn index(c: u8) -> Option<usize> {
    KOCH.iter().position(|&k| k == c.to_ascii_uppercase())
}

impl Letters {
    /// One copied character: `copied` is None when it was missed.
    pub fn record(&mut self, sent: u8, copied: Option<u8>) {
        let Some(i) = index(sent) else { return };
        if self.tries[i] == DECAY_AT {
            self.tries[i] /= 2;
            self.right[i] /= 2;
        }
        self.tries[i] += 1;
        let Some(copied) = copied else { return };
        if copied == sent {
            self.right[i] += 1;
        } else {
            self.mixup(sent, copied);
        }
    }

    /// Space-Saving (Metwally 2005): a new mixup takes the rarest slot and its count, so any
    /// mixup more common than 1 in MIXUP_SLOTS stays listed however much noise comes between.
    fn mixup(&mut self, sent: u8, copied: u8) {
        let slot = match self.mixups.iter().position(|m| m.count > 0 && m.sent == sent && m.copied == copied) {
            Some(slot) => slot,
            None => {
                let weakest = (0..MIXUP_SLOTS).min_by_key(|&k| self.mixups[k].count).unwrap_or(0);
                self.mixups[weakest] = Mixup { sent, copied, ..self.mixups[weakest] };
                weakest
            }
        };
        if self.mixups[slot].count == u8::MAX {
            for m in &mut self.mixups {
                m.count /= 2;
            }
        }
        self.mixups[slot].count += 1;
    }

    /// Erases the counts and mixups; the Koch level stays.
    pub fn clear_stats(&mut self) {
        *self = Letters { level: self.level, ..Letters::default() };
    }

    /// Percent copied right and the number of tries, once the character has been sent.
    pub fn accuracy(&self, i: usize) -> Option<(u8, u8)> {
        let tries = self.tries[i];
        (tries > 0).then(|| ((self.right[i] as u32 * 100 / tries as u32) as u8, tries))
    }

    /// How well you copy character `i`, once it has been sent: from a 95% Wilson score interval
    /// on its accuracy, known when the whole interval is at or above KNOWN, weak when it is all
    /// below.
    pub fn mastery(&self, i: usize) -> Option<Mastery> {
        let n = self.tries[i] as f32;
        if n == 0.0 {
            return None;
        }
        let (p, z2) = (self.right[i] as f32 / n, Z_95 * Z_95);
        let center = (p + z2 / (2.0 * n)) / (1.0 + z2 / n);
        let half = Z_95 * sqrt(p * (1.0 - p) / n + z2 / (4.0 * n * n)) / (1.0 + z2 / n);
        Some(if center - half >= KNOWN {
            Mastery::Known
        } else if center + half < KNOWN {
            Mastery::Weak
        } else {
            Mastery::Unsure
        })
    }

    /// How often a drill should pick character `i`: weak and new characters come up more.
    /// Accuracy is smoothed as (right + 1) / (tries + 2), so one early miss doesn't swamp the
    /// drill, and an untried character weighs as one at 50%.
    pub fn weight(&self, i: usize) -> u32 {
        let smoothed = (self.right[i] as u32 + 1) * 100 / (self.tries[i] as u32 + 2);
        2 + (100 - smoothed) / 8
    }

    /// Mixups, most common first.
    pub fn mixups(&self) -> impl Iterator<Item = Mixup> {
        let mut sorted = self.mixups;
        sort_by_key(&mut sorted, |m| core::cmp::Reverse(m.count));
        sorted.into_iter().filter(|m| m.count > 0)
    }

    pub fn encode(&self) -> [u8; RECORD_LEN] {
        let mut record = [0; RECORD_LEN];
        let body = body_mut(&mut record);
        let n = KOCH.len();
        body[0] = self.level;
        body[1..1 + n].copy_from_slice(&self.tries);
        body[1 + n..1 + 2 * n].copy_from_slice(&self.right);
        for (k, m) in self.mixups.iter().enumerate() {
            let at = 1 + 2 * n + 3 * k;
            body[at..at + 3].copy_from_slice(&[m.sent, m.copied, m.count]);
        }
        frame(&mut record, MAGIC, VERSION);
        record
    }

    /// The record at the start of `slot`; None for a blank or foreign record or a bad check. A
    /// legacy version 1 record keeps its counts and all but its rarest mixup.
    pub fn decode(slot: &[u8]) -> Option<Letters> {
        if let Some(body) = unframe(slot, MAGIC, VERSION) {
            return Self::from_body(body.get(..BODY_LEN)?, KOCH.len(), MIXUP_SLOTS);
        }
        let (n, slots) = match *slot.get(2)? {
            LEGACY_V2 => (KOCH.len(), MIXUP_SLOTS),
            LEGACY_V1 => (V1_CHARS, V1_MIXUPS),
            _ => return None,
        };
        let len = 3 + body_len(n, slots) + 1;
        let old = slot.get(..len).filter(|old| old[..2] == LEGACY_MAGIC && is_sealed(old))?;
        Self::from_body(&old[3..len - 1], n, slots)
    }

    /// A body of `n` characters' counts and `slots` mixups.
    fn from_body(body: &[u8], n: usize, slots: usize) -> Option<Letters> {
        let mut letters = Letters { level: body[0], ..Letters::default() };
        letters.tries[..n].copy_from_slice(&body[1..1 + n]);
        letters.right[..n].copy_from_slice(&body[1 + n..1 + 2 * n]);
        let mut mixups = [Mixup::default(); V1_MIXUPS];
        for (k, m) in mixups.iter_mut().take(slots).enumerate() {
            let at = 1 + 2 * n + 3 * k;
            *m = Mixup { sent: body[at], copied: body[at + 1], count: body[at + 2] };
        }
        if slots > MIXUP_SLOTS {
            sort_by_key(&mut mixups, |m| core::cmp::Reverse(m.count));
        }
        letters.mixups.copy_from_slice(&mixups[..MIXUP_SLOTS]);
        let valid = (FIRST_LEVEL..=KOCH.len() as u8).contains(&letters.level)
            && letters.tries.iter().zip(&letters.right).all(|(t, r)| r <= t && *t <= DECAY_AT);
        valid.then_some(letters)
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::seal;

    /// Letters with `right` of `tries` for K.
    fn k_scored(right: u8, tries: u8) -> Letters {
        let mut l = Letters::default();
        for t in 0..tries {
            l.record(b'K', Some(if t < right { b'K' } else { b'M' }));
        }
        l
    }

    #[test]
    fn mastery_needs_enough_tries_to_tell() {
        let k = index(b'K').unwrap();
        let mastery = |right, tries| k_scored(right, tries).mastery(k);
        assert_eq!(Letters::default().mastery(k), None);
        assert_eq!(mastery(1, 1), Some(Mastery::Unsure), "one right proves little");
        assert_eq!(mastery(34, 34), Some(Mastery::Unsure));
        assert_eq!(mastery(35, 35), Some(Mastery::Known));
        assert_eq!(mastery(5, 10), Some(Mastery::Weak));
        assert_eq!(mastery(80, 100), Some(Mastery::Weak));
        assert_eq!(mastery(85, 100), Some(Mastery::Unsure));
    }

    #[test]
    fn one_early_miss_does_not_swamp_the_drill() {
        let (k, m) = (index(b'K').unwrap(), index(b'M').unwrap());
        let untried = Letters::default().weight(m);
        assert_eq!(untried, 8);
        assert_eq!(k_scored(0, 1).weight(k), 10);
        assert_eq!(k_scored(1, 1).weight(k), 6);
        assert_eq!(k_scored(50, 50).weight(k), 2);
    }

    #[test]
    fn accuracy_mixups_and_round_trip() {
        let mut l = Letters::default();
        for _ in 0..3 {
            l.record(b'H', Some(b'H'));
        }
        l.record(b'H', Some(b'5'));
        l.record(b'H', None);
        l.record(b'U', Some(b'V'));
        l.record(b'H', Some(b'5'));
        let h = index(b'H').unwrap();
        assert_eq!(l.accuracy(h), Some((50, 6)));
        let top = l.mixups().next().unwrap();
        assert_eq!((top.sent, top.copied, top.count), (b'H', b'5', 2));
        assert!(l.weight(h) > l.weight(index(b'K').unwrap()) / 2);
        assert_eq!(Letters::decode(&l.encode()), Some(l));
        assert_eq!(Letters::decode(&[0xFF; RECORD_LEN]), None);
    }

    #[test]
    fn counts_decay_and_full_mixup_list_replaces_the_rarest() {
        let mut l = Letters::default();
        for _ in 0..250 {
            l.record(b'K', Some(b'K'));
        }
        assert_eq!(l.accuracy(0), Some((100, 150)));
        for (k, c) in b"ABCDEFGHIJKLM".iter().enumerate() {
            for _ in 0..=k {
                l.record(b'X', Some(*c));
            }
        }
        assert_eq!(l.mixups().count(), MIXUP_SLOTS);
        assert!(l.mixups().all(|m| m.copied != b'A'));
    }

    #[test]
    fn a_common_mixup_stays_listed_through_noise() {
        let mut l = Letters::default();
        for &c in &KOCH[..MIXUP_SLOTS] {
            l.record(c, Some(b'Z'));
            l.record(c, Some(b'Z'));
        }
        for k in 0..10 {
            l.record(b'H', Some(b'5'));
            l.record(b'S', Some(b'0' + k));
        }
        assert!(l.mixups().any(|m| (m.sent, m.copied) == (b'H', b'5')));
    }

    #[test]
    fn clearing_stats_keeps_the_level() {
        let mut l = Letters { level: 9, ..Letters::default() };
        l.record(b'K', Some(b'M'));
        l.clear_stats();
        assert_eq!(l, Letters { level: 9, ..Letters::default() });
    }

    #[test]
    fn koch_ends_with_prosigns() {
        let written: [&str; 3] = core::array::from_fn(|i| crate::morse::text(&KOCH[V1_CHARS + i]));
        assert_eq!(written, ["<AR>", "<SK>", "<KN>"]);
    }

    #[test]
    fn legacy_version_2_reads_whole() {
        let mut l = Letters::default();
        l.record(b'H', Some(b'5'));
        let mut old = [0u8; 3 + BODY_LEN + 1];
        old[..3].copy_from_slice(&[b'R', b'L', LEGACY_V2]);
        old[3..3 + BODY_LEN].copy_from_slice(&l.encode()[4..4 + BODY_LEN]);
        seal(&mut old);
        assert_eq!(Letters::decode(&old), Some(l));
    }

    #[test]
    fn version_1_keeps_counts_and_common_mixups() {
        const V1_LEN: usize = 4 + 2 * V1_CHARS + 3 * V1_MIXUPS + 1;
        let mut record = [0u8; RECORD_LEN];
        record[..4].copy_from_slice(&[LEGACY_MAGIC[0], LEGACY_MAGIC[1], LEGACY_V1, 5]);
        let h = index(b'H').unwrap();
        record[4 + h] = 6;
        record[4 + V1_CHARS + h] = 3;
        for k in 0..V1_MIXUPS {
            let at = 4 + 2 * V1_CHARS + 3 * k;
            record[at..at + 3].copy_from_slice(&[b'H', b'A' + k as u8, k as u8 + 1]);
        }
        seal(&mut record[..V1_LEN]);
        let l = Letters::decode(&record).unwrap();
        assert_eq!((l.level, l.accuracy(h)), (5, Some((50, 6))));
        assert_eq!(l.mixups().count(), MIXUP_SLOTS);
        assert!(l.mixups().all(|m| m.copied != b'A'));
    }
}
