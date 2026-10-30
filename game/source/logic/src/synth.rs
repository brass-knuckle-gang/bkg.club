//! The sound the device makes, one sample at a time: your sidetone, the stations the players
//! send, band noise and fading, and a spoken word, mixed into one signal for PA8's PWM.

use crate::adpcm::{Clips, Upsample};
use crate::math::sqrt;

/// The voice your own keying sounds on; voices after it carry the players, then MIDI in.
pub const SIDETONE: usize = 0;
pub const PLAYERS: usize = 4;
/// The tone the computer sends as MIDI notes. It does not fade: it is a station on the internet.
pub const REMOTE: usize = 1 + PLAYERS;
pub const VOICES: usize = 2 + PLAYERS;
/// Noise and fading levels run 0 (off) to this.
pub const MAX_LEVEL: u8 = 10;

/// One voice's peak, and the most all voices together reach: when stations overlap, the band's
/// gain drops so their sum stays here.
const VOICE_PEAK: f32 = 0.7;
/// The ceiling of the pack's clips before ADPCM, `LIMIT_PEAK` in `tools/pack/voiceclips.py`.
const CLIP_CEILING: f32 = 0.8;
/// The level a clip's full scale plays at, so a clip's ceiling sounds at a voice's peak. ADPCM
/// overshoot took a clip to 0.925 of full scale at most in a test, 0.81 here, just past
/// `LIMIT_KNEE`.
const CLIP_FULL_SCALE: f32 = VOICE_PEAK / CLIP_CEILING;
/// How long the band's gain takes to recover after an overlap ends, like a receiver's AGC. Much
/// longer than a Morse element, so the gain holds through an over instead of pumping per element.
const AGC_RELEASE_S: f32 = 0.5;
/// Above this the limiter bends peaks of noise and speech down so the mix never clips.
const LIMIT_KNEE: f32 = 0.8;
/// Key edges rise and fall over 5 ms on a raised cosine, so they don't click.
const RAMP_S: f32 = 0.005;
/// A square sounds about 7.5 dB louder than a sine of the same peak, by ear on the speaker and the
/// headphones, so it plays 7.5 dB down.
const SQUARE_GAIN: f32 = 0.4217;
/// 1/n for the odd harmonics a square sums: n = 1, 3, .. 31. At 17.6 kHz this reaches the
/// Nyquist frequency for pitches down to 284 Hz; a lower pitch loses its top harmonics.
const ODD_RECIPROCALS: [f32; 16] = {
    let mut r = [0.0; 16];
    let mut i = 0;
    while i < r.len() {
        r[i] = 1.0 / (2 * i + 1) as f32;
        i += 1;
    }
    r
};
/// Noise RMS for levels 1..=10, 2.5 dB apart, from 0.35 at the top.
const NOISE_RMS: [f32; MAX_LEVEL as usize] = [0.0263, 0.0350, 0.0467, 0.0623, 0.0830, 0.1107, 0.1476, 0.1969, 0.2625, 0.35];
/// A receiver CW filter's width.
const NOISE_BANDWIDTH_HZ: f32 = 500.0;
/// Deepest fade, at the top level: down to a tenth.
const MAX_FADE_DEPTH: f32 = 0.9;
/// Each player's fade rate, different so stations fade apart.
const FADE_HZ: [f32; PLAYERS] = [0.13, 0.19, 0.11, 0.23];
const PI: f32 = core::f32::consts::PI;

/// Sine of a small angle from its Taylor series: within 0.006% up to pi/4 rad.
#[inline(always)]
fn sin_small(x: f32) -> f32 {
    let x2 = x * x;
    x * (1.0 - x2 / 6.0 + x2 * x2 / 120.0)
}

fn cos_small(x: f32) -> f32 {
    let x2 = x * x;
    1.0 - x2 / 2.0 + x2 * x2 / 24.0
}

/// Sine oscillator with no table: the "magic circle" rotation, stable in single precision.
/// After `next` returns sin(t), `cos` holds cos(t + w/2) for a step of w per sample.
#[derive(Clone, Copy)]
struct Oscillator {
    sin: f32,
    cos: f32,
    k: f32,
    /// cos(w/2).
    half_step_cos: f32,
}

impl Oscillator {
    fn new(hz: f32, sample_hz: f32) -> Oscillator {
        let mut o = Oscillator { sin: 0.0, cos: 1.0, k: 0.0, half_step_cos: 1.0 };
        o.tune(hz, sample_hz);
        o
    }

    fn tune(&mut self, hz: f32, sample_hz: f32) {
        let half_step = PI * hz / sample_hz;
        (self.k, self.half_step_cos) = (2.0 * sin_small(half_step), cos_small(half_step));
    }

    fn reset(&mut self) {
        self.sin = 0.0;
        self.cos = 1.0;
    }

    #[inline(always)]
    fn next(&mut self) -> f32 {
        self.sin += self.k * self.cos;
        self.cos -= self.k * self.sin;
        self.sin
    }

    /// (sin t, cos t) of the next sample at unit length. From a reset, the rotation's sine runs
    /// 1 / cos(w/2) high, and the mean of the cosines half a step either side of t is cos(t).
    fn next_unit(&mut self) -> (f32, f32) {
        let cos_before = self.cos;
        let sin = self.next() * self.half_step_cos;
        (sin, (cos_before + self.cos) / 2.0)
    }
}

/// How many odd harmonics of `hz` lie below the Nyquist frequency, up to the table's length.
fn odd_harmonics(hz: f32, sample_hz: f32) -> usize {
    let below_nyquist = (sample_hz / 2.0 / hz.max(1.0) + 1.0) / 2.0;
    (below_nyquist as usize).clamp(1, ODD_RECIPROCALS.len())
}

/// A band-limited square, sum of sin(n t) / n over the first `harmonics` odd n, from sin t and
/// cos t by sin((n + 2) t) = 2 cos(2t) sin(n t) - sin((n - 2) t). Its peak stays at or below 1.
fn square(sin: f32, cos: f32, harmonics: usize) -> f32 {
    let two_cos_2t = 2.0 * (2.0 * cos * cos - 1.0);
    let (mut before, mut this) = (-sin, sin);
    let mut sum = sin;
    for reciprocal in &ODD_RECIPROCALS[1..harmonics] {
        (before, this) = (this, two_cos_2t * this - before);
        sum += this * reciprocal;
    }
    sum
}

/// The raised cosine (1 - cos(pi x)) / 2 for x in 0..=1: exactly 0 and 1 at the ends.
#[inline(always)]
fn raised_cosine(x: f32) -> f32 {
    #[inline(always)]
    fn half(x: f32) -> f32 {
        let s = sin_small(PI / 2.0 * x);
        s * s
    }
    if x <= 0.5 { half(x) } else { 1.0 - half(1.0 - x) }
}

#[derive(Clone, Copy)]
struct Voice {
    tone: Oscillator,
    hz: u16,
    harmonics: usize,
    down: bool,
    /// Ramp position, 0..1, linear in time.
    level: f32,
    fade: Oscillator,
}

impl Voice {
    /// The key envelope's gain this sample.
    fn envelope(&mut self, step: f32) -> f32 {
        self.level = if self.down { (self.level + step).min(1.0) } else { (self.level - step).max(0.0) };
        raised_cosine(self.level)
    }

    fn silent(&self) -> bool {
        !self.down && self.level == 0.0
    }
}

/// White noise through a two-pole band-pass at the sidetone pitch, scaled to an RMS of 1.
struct BandNoise {
    seed: u32,
    a1: f32,
    a2: f32,
    gain: f32,
    y1: f32,
    y2: f32,
}

impl BandNoise {
    fn new(hz: f32, sample_hz: f32) -> BandNoise {
        let mut n = BandNoise { seed: 0x1234_5679, a1: 0.0, a2: 0.0, gain: 0.0, y1: 0.0, y2: 0.0 };
        n.tune(hz, sample_hz);
        n
    }

    fn tune(&mut self, hz: f32, sample_hz: f32) {
        let r = 1.0 - PI * NOISE_BANDWIDTH_HZ / sample_hz;
        let (a1, a2) = (2.0 * r * cos_small(2.0 * PI * hz / sample_hz), r * r);
        // Output variance of this filter for white input of variance 1/3, uniform in -1..1.
        let variance = (1.0 + a2) / ((1.0 - a2) * ((1.0 + a2) * (1.0 + a2) - a1 * a1)) / 3.0;
        (self.a1, self.a2, self.gain) = (a1, a2, 1.0 / sqrt(variance));
    }

    fn next(&mut self) -> f32 {
        // xorshift32: the top 16 bits as a value in -1..1.
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        let x = (self.seed >> 16) as f32 / 32768.0 - 1.0;
        let y = x + self.a1 * self.y1 - self.a2 * self.y2;
        self.y2 = self.y1;
        self.y1 = y;
        y * self.gain
    }
}

/// How the sound is set up, from settings.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sound {
    pub square: bool,
    /// 0..=MAX_LEVEL.
    pub noise: u8,
    /// 0..=MAX_LEVEL.
    pub fading: u8,
    /// Your sidetone pitch, where the noise centers.
    pub tone_hz: u16,
}

/// Voice clips as the synth plays them: decoded and raised to the output rate.
pub type Speech = Upsample<Clips<'static>>;

pub struct Synth {
    sample_hz: f32,
    speech: Option<Speech>,
    voices: [Voice; VOICES],
    noise: BandNoise,
    square: bool,
    noise_rms: f32,
    fade_depth: f32,
    /// The screen plays band noise.
    band: bool,
    /// Gain on the band while your key is down, ramping like a break-in receiver's mute.
    duck: f32,
    /// Gain on the stations, below 1 while they overlap.
    agc: f32,
    agc_release: f32,
    /// Gain on the whole mix, easing toward `gain_target` so a change doesn't click.
    gain: f32,
    gain_target: f32,
    step: f32,
}

impl Synth {
    pub fn new(sample_hz: f32, sound: Sound) -> Synth {
        let voice = |i: usize| Voice {
            tone: Oscillator::new(sound.tone_hz as f32, sample_hz),
            hz: sound.tone_hz,
            harmonics: odd_harmonics(sound.tone_hz as f32, sample_hz),
            down: false,
            level: 0.0,
            fade: Oscillator::new(if i == SIDETONE || i == REMOTE { 0.0 } else { FADE_HZ[i - 1] }, sample_hz),
        };
        let mut synth = Synth {
            sample_hz,
            speech: None,
            voices: core::array::from_fn(voice),
            noise: BandNoise::new(sound.tone_hz as f32, sample_hz),
            square: false,
            noise_rms: 0.0,
            fade_depth: 0.0,
            band: false,
            duck: 1.0,
            agc: 1.0,
            agc_release: 1.0 / (AGC_RELEASE_S * sample_hz),
            gain: 1.0,
            gain_target: 1.0,
            step: 1.0 / (RAMP_S * sample_hz),
        };
        synth.set_sound(sound);
        synth
    }

    pub fn set_sound(&mut self, sound: Sound) {
        self.square = sound.square;
        let noise = sound.noise.min(MAX_LEVEL);
        self.noise_rms = if noise == 0 { 0.0 } else { NOISE_RMS[noise as usize - 1] };
        self.fade_depth = sound.fading.min(MAX_LEVEL) as f32 * MAX_FADE_DEPTH / MAX_LEVEL as f32;
        self.noise.tune(sound.tone_hz as f32, self.sample_hz);
    }

    /// Scales everything the synth plays by `gain`, 0..=1, easing in over about `RAMP_S`.
    pub fn set_gain(&mut self, gain: f32) {
        self.gain_target = gain;
    }

    /// Whether the current screen plays band noise.
    pub fn set_band(&mut self, on: bool) {
        self.band = on;
    }

    /// Keys `voice` up or down at `hz`.
    pub fn key(&mut self, voice: usize, down: bool, hz: u16) {
        let sample_hz = self.sample_hz;
        let Some(v) = self.voices.get_mut(voice) else { return };
        if down && v.silent() {
            v.tone.reset();
        }
        if hz != v.hz {
            v.hz = hz;
            v.tone.tune(hz as f32, sample_hz);
            v.harmonics = odd_harmonics(hz as f32, sample_hz);
        }
        v.down = down;
    }

    /// Starts speaking `speech`, or stops speaking. The synth lets go of the clip when it ends.
    pub fn say(&mut self, speech: Option<Speech>) {
        self.speech = speech;
    }

    /// Nothing sounds and nothing will until a key goes down, the band opens or a clip starts.
    #[inline(always)]
    pub fn is_silent(&self) -> bool {
        // A loop, not `all`: the sample interrupt inlines this, and `all` stays a call into flash.
        for v in &self.voices {
            if !v.silent() {
                return false;
            }
        }
        !self.noise_on() && self.speech.is_none()
    }

    fn noise_on(&self) -> bool {
        self.band && self.noise_rms > 0.0
    }

    /// The next sample, -1..1.
    pub fn sample(&mut self) -> f32 {
        let step = self.step;
        let keying = self.voices[SIDETONE].down;
        self.duck = if keying { (self.duck - step).max(0.0) } else { (self.duck + step).min(1.0) };
        let (mut sidetone, mut stations, mut stations_peak) = (0.0, 0.0, 0.0);
        let mut sidetone_peak = 0.0;
        for (i, v) in self.voices.iter_mut().enumerate() {
            let envelope = v.envelope(step);
            if envelope == 0.0 {
                continue;
            }
            let wave = if self.square {
                let (sin, cos) = v.tone.next_unit();
                SQUARE_GAIN * square(sin, cos, v.harmonics)
            } else {
                v.tone.next()
            };
            if i == SIDETONE {
                sidetone_peak = VOICE_PEAK * envelope;
                sidetone = sidetone_peak * wave;
            } else {
                let fade = if i == REMOTE { 1.0 } else { 1.0 - self.fade_depth * (1.0 + v.fade.next()) / 2.0 };
                let peak = VOICE_PEAK * envelope * self.duck * fade;
                stations_peak += peak;
                stations += peak * wave;
            }
        }
        // Your sidetone keeps its level; the stations share what room it leaves. The gain drops at
        // once, as smoothly as the key ramps that raise the peak, and recovers slowly.
        let room = VOICE_PEAK - sidetone_peak;
        let target = if stations_peak > room { room / stations_peak } else { 1.0 };
        self.agc = if target < self.agc { target } else { self.agc + (target - self.agc) * self.agc_release };
        let mut mix = sidetone + self.agc * stations;
        if self.noise_on() {
            mix += self.duck * self.noise_rms * self.noise.next();
        }
        if let Some(speech) = &mut self.speech {
            match speech.next() {
                Some(s) => mix += CLIP_FULL_SCALE * s as f32 / -(i16::MIN as f32),
                None => self.speech = None,
            }
        }
        self.gain += (self.gain_target - self.gain) * step;
        self.gain * limit(mix)
    }
}

/// PWM compare value for `sample` (-1..1) on a timer of `steps` counts per pulse, rounded to the
/// nearest count. Silence is half duty, `steps / 2`.
pub fn duty(sample: f32, steps: u16) -> u16 {
    let mid = steps / 2;
    (mid as f32 + sample.clamp(-1.0, 1.0) * (mid - 1) as f32 + 0.5) as u16
}

/// Passes -LIMIT_KNEE..LIMIT_KNEE unchanged and bends anything louder smoothly toward 1.
fn limit(x: f32) -> f32 {
    let size = x.abs();
    if size <= LIMIT_KNEE {
        return x;
    }
    let over = (size - LIMIT_KNEE) / (1.0 - LIMIT_KNEE);
    let bent = LIMIT_KNEE + (1.0 - LIMIT_KNEE) * over / (1.0 + over);
    if x < 0.0 { -bent } else { bent }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    const RATE: f32 = 17_578.125;
    const SOUND: Sound = Sound { square: false, noise: 0, fading: 0, tone_hz: 700 };

    fn rms(samples: impl Iterator<Item = f32>) -> f32 {
        let (sum, n) = samples.fold((0.0f64, 0), |(s, n), x| (s + (x as f64) * (x as f64), n + 1));
        (sum / n as f64).sqrt() as f32
    }

    #[test]
    fn speech_plays_its_clip_then_lets_go() {
        static CLIP: [u8; 64] = [0x77; 64];
        let mut synth = Synth::new(RATE, SOUND);
        synth.say(Some(Upsample::new(Clips::new(&CLIP, &[crate::adpcm::Slot { end: CLIP.len() as u16, gap: 0 }]), 3)));
        assert!(!synth.is_silent());
        let samples: std::vec::Vec<f32> = (0..3 * 2 * CLIP.len()).map(|_| synth.sample()).collect();
        assert!(samples.iter().any(|&s| s > 0.1), "speech never got loud");
        assert!(!synth.is_silent());
        synth.sample();
        assert!(synth.is_silent());
    }

    #[test]
    fn a_keyed_voice_sounds_at_its_pitch() {
        let mut synth = Synth::new(RATE, SOUND);
        synth.key(1, true, 650);
        let samples: std::vec::Vec<f32> = (0..RATE as usize).map(|_| synth.sample()).collect();
        let crossings = samples.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        assert!((649..=651).contains(&crossings), "{crossings}");
        let peak = samples.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!((VOICE_PEAK * 0.98..VOICE_PEAK * 1.05).contains(&peak), "{peak}");
    }

    #[test]
    fn a_gain_change_eases_in_without_a_jump() {
        let (mut synth, mut plain) = (Synth::new(RATE, SOUND), Synth::new(RATE, SOUND));
        synth.key(SIDETONE, true, 700);
        plain.key(SIDETONE, true, 700);
        for _ in 0..RATE as usize / 10 {
            assert_eq!(synth.sample(), plain.sample());
        }
        synth.set_gain(0.5);
        let mut gain = 1.0;
        for _ in 0..RATE as usize / 10 {
            let (s, p) = (synth.sample(), plain.sample());
            // Near a zero crossing the ratio says little about the gain, so a few samples go by
            // between checks. Without easing the gain would drop by 0.5 at once.
            if p.abs() > 0.3 {
                assert!((s / p - gain).abs() < 0.05, "{gain} to {}", s / p);
                gain = s / p;
            }
        }
        assert!((gain - 0.5).abs() < 0.001, "{gain}");
    }

    #[test]
    fn key_edges_ramp_and_end_in_silence() {
        let mut synth = Synth::new(RATE, SOUND);
        assert!(synth.is_silent());
        synth.key(SIDETONE, true, 700);
        let ramp = RAMP_S * RATE;
        let first = synth.sample().abs();
        assert!(first < 0.001 * VOICE_PEAK, "{first}");
        for k in 2..=10 {
            // The rotation's sine peaks up to 1% above 1 at these pitches.
            assert!(synth.sample().abs() <= 1.01 * VOICE_PEAK * raised_cosine(k as f32 / ramp));
        }
        for _ in 0..1000 {
            synth.sample();
        }
        synth.key(SIDETONE, false, 700);
        for _ in 0..ramp as usize + 1 {
            synth.sample();
        }
        assert!(synth.is_silent());
        assert_eq!(synth.sample(), 0.0);
    }

    #[test]
    fn the_envelope_is_a_raised_cosine_both_ways() {
        let mut voice = Synth::new(RATE, SOUND).voices[SIDETONE];
        let step = 1.0 / (RAMP_S * RATE);
        voice.down = true;
        let rise: std::vec::Vec<f32> = (0..(RAMP_S * RATE) as usize + 2).map(|_| voice.envelope(step)).collect();
        for (i, &gain) in rise.iter().enumerate() {
            let x = ((i + 1) as f32 * step).min(1.0);
            assert!((gain - (1.0 - (PI * x).cos()) / 2.0).abs() < 1e-4, "sample {i}: {gain}");
        }
        assert_eq!(rise.last(), Some(&1.0));
        voice.down = false;
        let fall: std::vec::Vec<f32> = (0..rise.len()).map(|_| voice.envelope(step)).collect();
        for (i, &gain) in fall.iter().enumerate() {
            let x = (1.0 - (i + 1) as f32 * step).max(0.0);
            assert!((gain - (1.0 - (PI * x).cos()) / 2.0).abs() < 1e-4, "sample {i}: {gain}");
        }
        assert_eq!(fall.last(), Some(&0.0));
    }

    /// Level at `hz` of a Hann-windowed block, by direct correlation.
    fn level(samples: &[f32], hz: f64) -> f64 {
        let n = samples.len();
        let w = 2.0 * std::f64::consts::PI * hz / RATE as f64;
        let (mut re, mut im) = (0.0, 0.0);
        for (i, &x) in samples.iter().enumerate() {
            let hann = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (n - 1) as f64).cos();
            re += x as f64 * hann * (w * i as f64).cos();
            im += x as f64 * hann * (w * i as f64).sin();
        }
        (re * re + im * im).sqrt()
    }

    fn square_samples(hz: u16) -> std::vec::Vec<f32> {
        let mut synth = Synth::new(RATE, Sound { square: true, ..SOUND });
        synth.key(SIDETONE, true, hz);
        for _ in 0..(RAMP_S * RATE) as usize + 1 {
            synth.sample();
        }
        (0..(2.0 * RATE) as usize).map(|_| synth.sample()).collect()
    }

    #[test]
    fn square_has_no_aliases() {
        for hz in [700, 1000] {
            let samples = square_samples(hz);
            let fundamental = level(&samples, hz as f64);
            // Where the odd harmonics above the Nyquist frequency of a true square would fold to.
            for n in (1..60).step_by(2).filter(|n| (n * hz) as f32 > RATE / 2.0) {
                let harmonic = (n * hz) as f64;
                let alias = (harmonic - (harmonic / RATE as f64).round() * RATE as f64).abs();
                let db = 20.0 * (level(&samples, alias) / fundamental).log10();
                assert!(db < -90.0, "{hz} Hz harmonic {n} at {alias} Hz: {db:.1} dB");
            }
        }
    }

    #[test]
    fn square_harmonics_fall_as_one_over_n() {
        let samples = square_samples(700);
        let fundamental = level(&samples, 700.0);
        for n in [3, 5, 7, 11] {
            let ratio = level(&samples, 700.0 * n as f64) / fundamental * n as f64;
            assert!((ratio - 1.0).abs() < 0.01, "harmonic {n}: {ratio}");
        }
        assert!(level(&samples, 1400.0) / fundamental < 1e-4);
    }

    #[test]
    fn a_square_peaks_7_5_db_under_a_sine() {
        let mut sine = Synth::new(RATE, SOUND);
        sine.key(SIDETONE, true, 700);
        let peak = |samples: &[f32]| samples.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        let sine: std::vec::Vec<f32> = (0..RATE as usize).map(|_| sine.sample()).collect();
        let db = 20.0 * (peak(&square_samples(700)) / peak(&sine)).log10();
        // The band-limited square peaks up to 0.9 dB under 1.
        assert!((-8.4..=-7.4).contains(&db), "{db}");
    }

    #[test]
    fn square_peak_stays_within_one_at_every_pitch() {
        for hz in (crate::settings::MIN_TONE_HZ..=crate::settings::MAX_TONE_HZ).step_by(25) {
            let mut tone = Oscillator::new(hz as f32, RATE);
            let harmonics = odd_harmonics(hz as f32, RATE);
            let peak = (0..RATE as usize).map(|_| {
                let (sin, cos) = tone.next_unit();
                square(sin, cos, harmonics).abs()
            });
            let peak = peak.fold(0.0f32, f32::max);
            assert!((0.9..=1.0).contains(&peak), "{hz} Hz: {peak}");
        }
    }

    #[test]
    fn duty_rounds_evenly_about_half() {
        assert_eq!((duty(0.0, 256), duty(1.0, 256), duty(-1.0, 256)), (128, 255, 1));
        for x in [0.3, 0.7, 10.2, 99.9] {
            let x = x / 127.0;
            assert_eq!(duty(x, 256) + duty(-x, 256), 256, "{x}");
        }
    }

    #[test]
    fn noise_levels_match_their_rms_and_need_the_band() {
        let mut synth = Synth::new(RATE, Sound { noise: 5, ..SOUND });
        assert!(synth.is_silent());
        synth.set_band(true);
        assert!(!synth.is_silent());
        for tone_hz in [400, 700, 1200] {
            synth.set_sound(Sound { noise: 5, tone_hz, ..SOUND });
            let measured = rms((0..200_000).map(|_| synth.sample()));
            assert!((measured / NOISE_RMS[4] - 1.0).abs() < 0.08, "{tone_hz} Hz: {measured}");
        }
    }

    #[test]
    fn your_keying_mutes_the_band() {
        let mut synth = Synth::new(RATE, Sound { noise: 10, ..SOUND });
        synth.set_band(true);
        synth.key(2, true, 800);
        synth.key(SIDETONE, true, 700);
        for _ in 0..200 {
            synth.sample();
        }
        let mut only_sidetone = Synth::new(RATE, SOUND);
        only_sidetone.key(SIDETONE, true, 700);
        for _ in 0..200 {
            only_sidetone.sample();
        }
        for _ in 0..1000 {
            assert!((synth.sample() - only_sidetone.sample()).abs() < 1e-6);
        }
    }

    #[test]
    fn fading_dips_a_station_but_not_your_sidetone() {
        let mut synth = Synth::new(RATE, Sound { fading: MAX_LEVEL, ..SOUND });
        synth.key(SIDETONE, true, 700);
        let sidetone = rms((0..(RATE * 8.0) as usize).map(|_| synth.sample()));
        assert!((sidetone - VOICE_PEAK / 2f32.sqrt()).abs() < 0.02, "{sidetone}");
        synth.key(SIDETONE, false, 700);
        synth.key(1, true, 700);
        let windows: std::vec::Vec<f32> =
            (0..80).map(|_| rms((0..(RATE / 10.0) as usize).map(|_| synth.sample()))).collect();
        let (low, high) = windows.iter().fold((1.0f32, 0.0f32), |(l, h), &w| (l.min(w), h.max(w)));
        assert!(low < high * 0.2, "{low} {high}");
    }

    #[test]
    fn overlapping_stations_share_one_peak_without_bending() {
        let mut synth = Synth::new(RATE, Sound { fading: 0, ..SOUND });
        synth.key(1, true, 600);
        synth.key(2, true, 640);
        let peak = (0..RATE as usize).map(|_| synth.sample().abs()).fold(0.0f32, f32::max);
        assert!(peak <= VOICE_PEAK * 1.01 && peak > VOICE_PEAK * 0.9, "{peak}");
    }

    #[test]
    fn a_station_recovers_its_level_after_an_overlap() {
        let mut synth = Synth::new(RATE, SOUND);
        synth.key(1, true, 600);
        synth.key(2, true, 640);
        for _ in 0..1000 {
            synth.sample();
        }
        synth.key(2, false, 640);
        let window = |synth: &mut Synth| rms((0..(RATE / 20.0) as usize).map(|_| synth.sample()));
        let soon = window(&mut synth);
        for _ in 0..(RATE * AGC_RELEASE_S * 5.0) as usize {
            synth.sample();
        }
        let later = window(&mut synth);
        assert!(soon < 0.7 * later, "{soon} {later}");
        assert!((later - VOICE_PEAK / 2f32.sqrt()).abs() < 0.02, "{later}");
    }

    #[test]
    fn every_voice_and_full_noise_stay_in_range() {
        let mut synth = Synth::new(RATE, Sound { square: true, noise: MAX_LEVEL, fading: 0, tone_hz: 700 });
        synth.set_band(true);
        for v in 1..VOICES {
            synth.key(v, true, 600 + 50 * v as u16);
        }
        assert!((0..100_000).map(|_| synth.sample()).all(|x| (-1.0..=1.0).contains(&x)));
    }
}
