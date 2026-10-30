//! PA8's duty for each pulse. A sample lasts several pulses, and rounding each pulse's duty to a
//! whole count leaves an error that the next two pulses take back, so the rounding noise moves far
//! above the audio band: its spectrum follows (1 - z^-1)^2 at the pulse rate.

/// Fraction bits of the shaper's fixed point: duties and errors are in 1/4096 count. At 1/256 the
/// sample's own rounding, which the shaper cannot move, cost 9 dB in the audio band.
const FRACTION: u32 = 12;
const ONE: i32 = 1 << FRACTION;

pub struct Shaper {
    /// Half duty, the silent level, in fixed point.
    mid: i32,
    /// Counts for a sample of 1.
    scale: f32,
    top: i32,
    /// The last two pulses' rounding errors, in fixed point.
    errors: [i32; 2],
}

/// `x` held to `low..=high`. Written out so it inlines into the pulse loop.
#[inline(always)]
fn bound(x: i32, low: i32, high: i32) -> i32 {
    if x < low {
        low
    } else if x > high {
        high
    } else {
        x
    }
}

impl Shaper {
    /// For a timer of `steps` counts per pulse: duty 0 holds the pin low, `steps` holds it high.
    pub fn new(steps: u16) -> Shaper {
        let mid = (steps / 2) as i32;
        Shaper { mid: mid << FRACTION, scale: ((mid - 1) << FRACTION) as f32, top: steps as i32, errors: [0; 2] }
    }

    /// Fills `pulses` with the duties that play `sample`, -1..1. Silence plays half duty exactly.
    pub fn fill(&mut self, sample: f32, pulses: &mut [u16]) {
        // Never negative, so `as` rounds to nearest.
        let want = (self.mid as f32 + sample.clamp(-1.0, 1.0) * self.scale + 0.5) as i32;
        let [mut last, mut before] = self.errors;
        for duty in pulses {
            let shaped = want - 2 * last + before;
            let rounded = bound((shaped + ONE / 2) >> FRACTION, 0, self.top);
            // Near full scale the bound can leave more than half a count; taking that back as well
            // would build up.
            (last, before) = (bound((rounded << FRACTION) - shaped, -ONE, ONE), last);
            *duty = rounded as u16;
        }
        self.errors = [last, before];
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::synth::duty;
    use std::vec::Vec;

    const STEPS: u16 = 256;
    const PULSES: usize = 16;
    const PULSE_HZ: f64 = 281_250.0;
    const SAMPLE_HZ: f64 = PULSE_HZ / PULSES as f64;

    fn tone(level: f64, hz: f64, samples: usize) -> Vec<f32> {
        (0..samples).map(|i| (level * (2.0 * std::f64::consts::PI * hz * i as f64 / SAMPLE_HZ).sin()) as f32).collect()
    }

    /// Each pulse's duty less the exact duty the sample asks for, in counts.
    fn errors(samples: &[f32], duties: &[u16]) -> Vec<f64> {
        let mid = (STEPS / 2) as f64;
        let scale = (STEPS / 2 - 1) as f64;
        duties.iter().enumerate().map(|(i, &d)| d as f64 - (mid + samples[i / PULSES] as f64 * scale)).collect()
    }

    fn held(samples: &[f32]) -> Vec<u16> {
        samples.iter().flat_map(|&s| [duty(s, STEPS); PULSES]).collect()
    }

    fn shaped(samples: &[f32]) -> Vec<u16> {
        let mut shaper = Shaper::new(STEPS);
        let mut pulses = [0; PULSES];
        samples.iter().flat_map(|&s| {
            shaper.fill(s, &mut pulses);
            pulses
        }).collect()
    }

    /// Power below 8 kHz, through four one-pole low-passes at 8 kHz. Their slow roll-off lets
    /// some noise from above the band through, so this overstates the shaper's noise.
    fn audio_band_power(errors: &[f64]) -> f64 {
        let a = 1.0 - (-2.0 * std::f64::consts::PI * 8_000.0 / PULSE_HZ).exp();
        let mut stages = [0.0; 4];
        let settle = errors.len() / 10;
        let mut sum = 0.0;
        for (i, &e) in errors.iter().enumerate() {
            let mut x = e;
            for y in &mut stages {
                *y += a * (x - *y);
                x = *y;
            }
            if i >= settle {
                sum += x * x;
            }
        }
        sum / (errors.len() - settle) as f64
    }

    fn improvement_db(samples: &[f32]) -> f64 {
        let before = audio_band_power(&errors(samples, &held(samples)));
        let after = audio_band_power(&errors(samples, &shaped(samples)));
        10.0 * (before / after).log10()
    }

    #[test]
    fn shaping_takes_the_rounding_noise_out_of_the_audio_band() {
        for (level, hz) in [(0.3, 700.0), (0.05, 700.0), (0.7, 450.0), (0.01, 1000.0)] {
            let db = improvement_db(&tone(level, hz, 20_000));
            assert!(db > 40.0, "{level} at {hz} Hz: {db:.1} dB");
        }
    }

    #[test]
    fn each_sample_averages_to_its_exact_duty() {
        let samples = tone(0.3, 700.0, 2_000);
        let errors = errors(&samples, &shaped(&samples));
        let worst = errors.chunks(PULSES).map(|c| c.iter().sum::<f64>() / PULSES as f64).fold(0.0f64, |m, e| m.max(e.abs()));
        // The error left over from the sample before can carry into this one's first pulses.
        assert!(worst < 3.0 / PULSES as f64, "{worst}");
    }

    #[test]
    fn silence_is_exactly_half_duty() {
        let mut shaper = Shaper::new(STEPS);
        let mut pulses = [0; PULSES];
        shaper.fill(0.0, &mut pulses);
        assert_eq!(pulses, [STEPS / 2; PULSES]);
    }

    #[test]
    fn full_scale_stays_on_the_timer_and_recovers() {
        let mut shaper = Shaper::new(STEPS);
        let mut pulses = [0; PULSES];
        for s in [1.0, -1.0, 1.0, 1.0, -1.0, -1.0] {
            shaper.fill(s, &mut pulses);
            assert!(pulses.iter().all(|&d| d <= STEPS), "{pulses:?}");
        }
        let samples = tone(0.3, 700.0, 2_000);
        let mut quiet = [0; PULSES];
        let errors: Vec<f64> = samples.iter().flat_map(|&s| {
            shaper.fill(s, &mut quiet);
            errors(&[s], &quiet)
        }).collect();
        assert!(errors[PULSES * 10..].iter().all(|e| e.abs() < 3.0), "the shaper did not recover");
    }
}
