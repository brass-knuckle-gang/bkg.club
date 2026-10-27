//! Voice clips: IMA ADPCM, 4 bits a sample, packed the way CPython's `audioop.lin2adpcm` writes
//! them (first sample in the high nibble, starting from predictor 0 and step index 0).

const STEPS: [u16; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97, 107, 118, 130, 143,
    157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411,
    1552, 1707, 1878, 2066, 2272, 2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493, 10442,
    11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];
const INDEX_CHANGE: [i8; 8] = [-1, -1, -1, -1, 2, 4, 6, 8];

/// The samples of one clip.
pub struct Clip<'a> {
    bytes: &'a [u8],
    /// Nibbles decoded so far.
    at: usize,
    predictor: i32,
    index: usize,
}

impl<'a> Clip<'a> {
    pub fn new(bytes: &'a [u8]) -> Clip<'a> {
        Clip { bytes, at: 0, predictor: 0, index: 0 }
    }

    fn decode(&mut self, nibble: u8) -> i16 {
        let step = STEPS[self.index] as i32;
        let mut diff = step >> 3;
        if nibble & 4 != 0 {
            diff += step;
        }
        if nibble & 2 != 0 {
            diff += step >> 1;
        }
        if nibble & 1 != 0 {
            diff += step >> 2;
        }
        self.predictor = if nibble & 8 != 0 { self.predictor - diff } else { self.predictor + diff };
        self.predictor = self.predictor.clamp(i16::MIN as i32, i16::MAX as i32);
        self.index = (self.index as i32 + INDEX_CHANGE[(nibble & 7) as usize] as i32).clamp(0, STEPS.len() as i32 - 1) as usize;
        self.predictor as i16
    }
}

impl Iterator for Clip<'_> {
    type Item = i16;

    fn next(&mut self) -> Option<i16> {
        let byte = *self.bytes.get(self.at / 2)?;
        let nibble = if self.at % 2 == 0 { byte >> 4 } else { byte & 0x0F };
        self.at += 1;
        Some(self.decode(nibble))
    }
}

/// The most clips one `Clips` plays.
pub const MAX_CLIPS: usize = 32;

/// Where a clip ends in the bytes of `Clips`, and the silent samples played before it.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Slot {
    pub end: u16,
    pub gap: u16,
}

/// How many samples `Upsample::new(Clips::new(..), factor)` gives for `slots` when it keeps them
/// all: two per byte and each gap, each `factor` times.
pub fn played_samples(slots: &[Slot], factor: u8) -> u32 {
    let bytes = slots.last().map_or(0, |s| s.end as u32);
    (2 * bytes + slots.iter().map(|s| s.gap as u32).sum::<u32>()) * factor.max(1) as u32
}

/// Clips one after another in one buffer, each decoded from a fresh start after its gap.
pub struct Clips<'a> {
    bytes: &'a [u8],
    slots: [Slot; MAX_CLIPS],
    count: usize,
    index: usize,
    clip: Clip<'a>,
    /// Silent samples left before the current clip starts.
    silence: u16,
}

impl<'a> Clips<'a> {
    /// The clips of `slots` in `bytes`, the first starting at 0. Slots past `MAX_CLIPS` or
    /// ending past `bytes` are left out.
    pub fn new(bytes: &'a [u8], slots: &[Slot]) -> Clips<'a> {
        let mut kept = [Slot::default(); MAX_CLIPS];
        let mut count = 0;
        for (slot, &given) in kept.iter_mut().zip(slots).take_while(|&(_, given)| given.end as usize <= bytes.len()) {
            *slot = given;
            count += 1;
        }
        Clips { bytes, slots: kept, count, index: 0, clip: Clip::new(&bytes[..kept[0].end as usize]), silence: kept[0].gap }
    }
}

impl Iterator for Clips<'_> {
    type Item = i16;

    fn next(&mut self) -> Option<i16> {
        loop {
            if self.silence > 0 {
                self.silence -= 1;
                return Some(0);
            }
            if let Some(sample) = self.clip.next() {
                return Some(sample);
            }
            self.index += 1;
            if self.index >= self.count {
                return None;
            }
            self.clip = Clip::new(&self.bytes[self.slots[self.index - 1].end as usize..self.slots[self.index].end as usize]);
            self.silence = self.slots[self.index].gap;
        }
    }
}

/// `factor` samples for each sample of `samples`, on straight lines between them. Clips are
/// stored at the output rate divided by a whole number, so no fractional resampler is needed.
pub struct Upsample<I> {
    samples: I,
    factor: u8,
    from: i16,
    to: i16,
    /// Output samples so far between `from` and `to`.
    phase: u8,
}

impl<I: Iterator<Item = i16>> Upsample<I> {
    pub fn new(samples: I, factor: u8) -> Upsample<I> {
        Upsample { samples, factor: factor.max(1), from: 0, to: 0, phase: 0 }
    }
}

impl<I: Iterator<Item = i16>> Iterator for Upsample<I> {
    type Item = i16;

    fn next(&mut self) -> Option<i16> {
        if self.phase == 0 {
            self.from = self.to;
            self.to = self.samples.next()?;
        }
        let rise = (self.to as i32 - self.from as i32) * self.phase as i32 / self.factor as i32;
        self.phase = (self.phase + 1) % self.factor;
        Some((self.from as i32 + rise) as i16)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_what_cpython_audioop_decodes() {
        // `audioop.lin2adpcm` of 48 samples of a 440 Hz sine at 8789 Hz, peak 20000, and
        // `audioop.adpcm2lin` of the result.
        let encoded = [
            0x07, 0x77, 0x77, 0x77, 0x70, 0xFB, 0xCA, 0x98, 0x02, 0x36, 0x33, 0x33, 0x20, 0x8B, 0xDC, 0xCB, 0xBA, 0xA8, 0x12, 0x45,
            0x34, 0x32, 0x10, 0x8A,
        ];
        let decoded: [i16; 48] = [
            0, 11, 41, 104, 240, 533, 1164, 2521, 5431, 5846, 176, -5497, -12127, -16584, -19015, -19751, -19082, -16039, -12165,
            -5623, 617, 6290, 11446, 16133, 19176, 19729, 19226, 16024, 11451, 5972, -658, -6898, -12571, -16254, -19602, -20210,
            -18550, -16034, -11917, -5829, -156, 6474, 12714, 16766, 18975, 19644, 19036, 16269,
        ];
        assert!(Clip::new(&encoded).eq(decoded));
    }

    #[test]
    fn clips_play_in_turn_from_a_fresh_start_with_gaps() {
        let bytes = [0x77, 0x77, 0x07, 0x07];
        let one: [i16; 4] = core::array::from_fn(|i| Clip::new(&bytes[..2]).nth(i).unwrap());
        let mut both = Clips::new(&bytes, &[Slot { end: 2, gap: 1 }, Slot { end: 4, gap: 3 }]);
        assert_eq!(both.next(), Some(0));
        assert!(both.by_ref().take(4).eq(one));
        assert!(both.by_ref().take(3).eq([0, 0, 0]));
        assert!(both.eq(Clip::new(&bytes[2..])));
    }

    #[test]
    fn clips_leave_out_ends_past_the_bytes() {
        assert_eq!(Clips::new(&[0x77; 2], &[Slot { end: 1, gap: 0 }, Slot { end: 5, gap: 0 }]).count(), 2);
    }

    #[test]
    fn upsample_draws_straight_lines_from_silence() {
        let out: [i16; 6] = [0, 100, 200, 300, 400, 500];
        assert!(Upsample::new([300, 600].into_iter(), 3).eq(out));
        // Each line ends at its input sample, so output runs one input sample behind.
        assert!(Upsample::new([7, 9].into_iter(), 1).eq([0, 7]));
    }

    #[test]
    fn played_samples_counts_what_the_clips_play() {
        let bytes = [0x12, 0x34, 0x56, 0x78, 0x9A];
        let slots = [Slot { end: 2, gap: 0 }, Slot { end: 3, gap: 7 }, Slot { end: 5, gap: 11 }];
        let played = Upsample::new(Clips::new(&bytes, &slots), 2).count();
        assert_eq!(played as u32, played_samples(&slots, 2));
    }
}
