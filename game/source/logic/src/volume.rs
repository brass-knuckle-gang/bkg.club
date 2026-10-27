//! Volume levels: 0 mutes, and `MAX_LEVEL` is +4.5 dB on both outputs. Below it, the headphones
//! step 1.5 dB a level down to -60 dB at level 1, and the ES8388's speaker steps 1 dB a level down
//! to -38.5 dB, since a quieter tone is lost through the speaker.

pub const MAX_LEVEL: u8 = 44;
pub const MAX_MIXER_CUT: u8 = 5;

/// `MAX_LEVEL`'s gain, in half dB: the ES8388 output stage's top.
const TOP: i32 = 9;
/// Half dB per level.
const HEADPHONE_STEP: i32 = 3;
const SPEAKER_STEP: i32 = 2;
/// The ES8388 output stage's step and its bottom, -45 dB, in half dB.
const OUTPUT_STEP: i32 = 3;
const OUTPUT_FLOOR: i32 = -90;
const MIXER_STEP: i32 = 6;
/// The synth's gain for each trim: 0, -0.5 and -1 dB.
const TRIM_GAIN: [f32; 3] = [1.0, 0.944_061, 0.891_251];

/// How the ES8388 makes one level.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CodecGain {
    /// Input mixer cut in 3 dB steps, 0..=`MAX_MIXER_CUT`. It feeds both outputs.
    pub mixer_cut: u8,
    /// OUT1 volume register: 0 is -45 dB, then 1.5 dB per step.
    pub speaker: u8,
    /// OUT2 volume register, on the same scale.
    pub headphones: u8,
    /// The synth's cut in 0.5 dB steps, 0..=2: it makes the speaker's levels between the output
    /// stage's steps. The headphones hear it too, so they land within 0.5 dB of their level.
    pub trim: u8,
}

impl CodecGain {
    pub fn synth_gain(self) -> f32 {
        TRIM_GAIN[self.trim as usize]
    }
}

/// `level`'s gain in half dB, at `step` half dB a level below `MAX_LEVEL`.
fn half_db(level: u8, step: i32) -> i32 {
    TOP - step * (MAX_LEVEL - level) as i32
}

/// None mutes. The output stage cuts first; the mixer cuts only below its range, since a cut
/// before the output stage raises the hiss relative to the tone.
pub fn codec_gain(level: u8) -> Option<CodecGain> {
    if level == 0 {
        return None;
    }
    let level = level.min(MAX_LEVEL);
    let speaker = half_db(level, SPEAKER_STEP);
    // The output step at or above the speaker's level, less the trim, is its level exactly.
    let trim = (-speaker).rem_euclid(OUTPUT_STEP);
    let headphones = (half_db(level, HEADPHONE_STEP) + trim + OUTPUT_STEP / 2).div_euclid(OUTPUT_STEP) * OUTPUT_STEP;
    let below_floor = (OUTPUT_FLOOR - headphones).max(0);
    let mixer = (below_floor + MIXER_STEP - 1) / MIXER_STEP * MIXER_STEP;
    let register = |half_db: i32| ((half_db + mixer - OUTPUT_FLOOR) / OUTPUT_STEP) as u8;
    Some(CodecGain {
        mixer_cut: (mixer / MIXER_STEP) as u8,
        speaker: register(speaker + trim),
        headphones: register(headphones),
        trim: trim as u8,
    })
}

/// The WM8978 speaker volume register for `level`, in 1 dB steps with 57 at 0 dB; None mutes.
/// The WM8978 has one output for the speaker and the headphones, so levels keep the ES8388
/// headphone dB values: `MAX_LEVEL` gives +4 dB, under the vendor's +6 dB top.
pub fn wm8978_speaker(level: u8) -> Option<u8> {
    const ZERO_DB: i32 = 57;
    const TOP: i32 = 63;
    if level == 0 {
        return None;
    }
    let half_db = -120 + 3 * (level.min(MAX_LEVEL) as i32 - 1);
    Some((ZERO_DB + half_db.div_euclid(2)).clamp(0, TOP) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An output's gain in half dB, after the mixer and the synth's trim.
    fn heard(g: CodecGain, register: u8) -> i32 {
        OUTPUT_FLOOR + OUTPUT_STEP * register as i32 - MIXER_STEP * g.mixer_cut as i32 - g.trim as i32
    }

    #[test]
    fn the_speaker_steps_by_1_db_up_to_4_5_db() {
        assert_eq!(codec_gain(0), None);
        for level in 1..=MAX_LEVEL {
            let g = codec_gain(level).unwrap();
            assert_eq!(heard(g, g.speaker), -79 + 2 * level as i32, "level {level}");
        }
    }

    #[test]
    fn the_headphones_stay_within_0_5_db_of_1_5_db_steps() {
        let mut before = i32::MIN;
        for level in 1..=MAX_LEVEL {
            let g = codec_gain(level).unwrap();
            let gain = heard(g, g.headphones);
            assert!((gain - (-123 + 3 * level as i32)).abs() <= 1, "level {level}: {gain}");
            assert!(gain > before, "level {level}");
            before = gain;
        }
    }

    #[test]
    fn registers_stay_in_range() {
        for level in 1..=u8::MAX {
            let g = codec_gain(level).unwrap();
            assert!(g.mixer_cut <= MAX_MIXER_CUT && g.trim <= 2, "level {level}");
            assert!(g.speaker <= 0x21 && g.headphones <= 0x21, "level {level}");
        }
        assert_eq!(codec_gain(MAX_LEVEL), Some(CodecGain { mixer_cut: 0, speaker: 0x21, headphones: 0x21, trim: 0 }));
        assert_eq!(codec_gain(u8::MAX), codec_gain(MAX_LEVEL));
    }

    #[test]
    fn the_mixer_cuts_only_with_the_headphone_output_at_its_bottom() {
        for level in 1..=MAX_LEVEL {
            let g = codec_gain(level).unwrap();
            assert!(g.mixer_cut == 0 || g.headphones <= 1, "level {level}: {g:?}");
        }
        assert_eq!(codec_gain(1).unwrap().mixer_cut, MAX_MIXER_CUT);
    }

    #[test]
    fn wm8978_follows_the_same_db_and_stays_under_the_vendor_top() {
        assert_eq!(wm8978_speaker(0), None);
        assert_eq!(wm8978_speaker(1), Some(0));
        assert_eq!(wm8978_speaker(MAX_LEVEL - 3), Some(57));
        assert_eq!(wm8978_speaker(MAX_LEVEL), Some(61));
        assert_eq!(wm8978_speaker(u8::MAX), Some(61));
    }
}
