//! Measures keying against a dot length: each mark and gap in dots, and statistics over a
//! session.

use core::fmt::Write;

use crate::decoder::{CHAR_GAP_DOTS, DAH_DOTS, WORD_GAP_DOTS};
use crate::math::sqrt;
use crate::text::{Hundredths, Text};

/// A keyed line: the keyer output, or one contact of the key jack.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Line {
    Key,
    Tip,
    Ring,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Dit,
    Dah,
    /// Between the elements of one character.
    ElementGap,
    CharGap,
    WordGap,
}

impl Kind {
    pub fn ideal_units(self) -> f32 {
        match self {
            Kind::Dit | Kind::ElementGap => 1.0,
            Kind::Dah | Kind::CharGap => 3.0,
            Kind::WordGap => 7.0,
        }
    }

    pub fn is_mark(self) -> bool {
        matches!(self, Kind::Dit | Kind::Dah)
    }

    /// What the decoder reads a mark `units` dots long as.
    pub fn mark(units: f32) -> Kind {
        if units < DAH_DOTS as f32 { Kind::Dit } else { Kind::Dah }
    }

    /// What the decoder reads a gap `units` dots long as.
    pub fn gap(units: f32) -> Kind {
        if units < CHAR_GAP_DOTS as f32 {
            Kind::ElementGap
        } else if units < WORD_GAP_DOTS as f32 {
            Kind::CharGap
        } else {
            Kind::WordGap
        }
    }

    /// The decoder's split points below and above the ideal length, in dots.
    fn splits(self) -> (Option<f32>, Option<f32>) {
        let (dah, char_gap, word_gap) = (DAH_DOTS as f32, CHAR_GAP_DOTS as f32, WORD_GAP_DOTS as f32);
        match self {
            Kind::Dit => (None, Some(dah)),
            Kind::Dah => (Some(dah), None),
            Kind::ElementGap => (None, Some(char_gap)),
            Kind::CharGap => (Some(char_gap), Some(word_gap)),
            Kind::WordGap => (Some(word_gap), None),
        }
    }
}

/// One finished mark or gap.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Interval {
    pub kind: Kind,
    pub start_ms: u32,
    pub len_ms: u32,
    /// The dot length it was measured against.
    pub dit_ms: u16,
    /// Length in dots of `dit_ms`.
    pub units: f32,
}

impl Interval {
    pub fn ideal_ms(&self) -> u32 {
        (self.kind.ideal_units() * self.dit_ms as f32) as u32
    }

    /// Relative to the ideal length: 0.1 is 10% long.
    pub fn error(&self) -> f32 {
        self.units / self.kind.ideal_units() - 1.0
    }

    pub fn quality(&self) -> Quality {
        Quality::of(self.kind, self.units)
    }
}

/// Timing within this fraction of ideal is good.
const GOOD_ERROR: f32 = 0.20;

/// Timing quality. Good: within GOOD_ERROR of ideal. Bad: more than halfway from ideal to a
/// decoder split point, where a little more error reads as another kind. Off: between, and
/// beyond ideal on a side with no split point, such as a long dah, which still decodes.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Debug)]
pub enum Quality {
    #[default]
    Good,
    Off,
    Bad,
}

impl Quality {
    /// The quality of a `kind` interval `units` dots long.
    pub fn of(kind: Kind, units: f32) -> Quality {
        let ideal = kind.ideal_units();
        if (units / ideal - 1.0).abs() <= GOOD_ERROR {
            return Quality::Good;
        }
        let (below, above) = kind.splits();
        match if units < ideal { below } else { above } {
            Some(split) if (units - ideal).abs() > (split - ideal).abs() / 2.0 => Quality::Bad,
            _ => Quality::Off,
        }
    }
}

/// A gap this long is a pause between sessions of sending, not a word gap.
pub const PAUSE_UNITS: f32 = 14.0;

/// Turns key edges into intervals, of the kinds the decoder reads them as.
#[derive(Default)]
pub struct Meter {
    down_at: Option<u32>,
    up_at: Option<u32>,
    char_worst: Quality,
    char_start_ms: u32,
    char_end_ms: u32,
}

impl Meter {
    /// The key went down or up at `at_ms`. Returns the interval the edge finished.
    pub fn edge(&mut self, down: bool, at_ms: u32, dit_ms: u16) -> Option<Interval> {
        let units = |since: u32| at_ms.wrapping_sub(since) as f32 / dit_ms as f32;
        if down {
            if self.down_at.is_some() {
                return None;
            }
            self.down_at = Some(at_ms);
            let up = self.up_at.take();
            let Some(up) = up.filter(|&up| units(up) < PAUSE_UNITS) else {
                self.char_worst = Quality::Good;
                self.char_start_ms = at_ms;
                return None;
            };
            let u = units(up);
            let kind = Kind::gap(u);
            let gap = Interval { kind, start_ms: up, len_ms: at_ms.wrapping_sub(up), dit_ms, units: u };
            if kind == Kind::ElementGap {
                self.char_worst = self.char_worst.max(gap.quality());
            } else {
                self.char_worst = gap.quality();
                self.char_start_ms = at_ms;
            }
            Some(gap)
        } else {
            let start = self.down_at.take()?;
            self.up_at = Some(at_ms);
            let u = units(start);
            let kind = Kind::mark(u);
            let mark = Interval { kind, start_ms: start, len_ms: at_ms.wrapping_sub(start), dit_ms, units: u };
            self.char_worst = self.char_worst.max(mark.quality());
            self.char_end_ms = at_ms;
            Some(mark)
        }
    }

    /// Worst timing in the current character: its marks, the gaps inside it, and the gap before
    /// it.
    pub fn char_worst(&self) -> Quality {
        self.char_worst
    }

    /// First mark start and last mark end of the current character.
    pub fn char_span(&self) -> (u32, u32) {
        (self.char_start_ms, self.char_end_ms)
    }
}

/// Running mean and standard deviation by Welford's method, which stays accurate in f32 over
/// thousands of samples.
#[derive(Clone, Copy, Default)]
pub struct Moments {
    pub count: u32,
    mean: f32,
    /// Sum of squared differences from the mean.
    m2: f32,
}

impl Moments {
    pub(crate) fn add(&mut self, x: f32) {
        self.count += 1;
        let before = x - self.mean;
        self.mean += before / self.count as f32;
        self.m2 += before * (x - self.mean);
    }

    pub fn mean(&self) -> Option<f32> {
        (self.count > 0).then_some(self.mean)
    }

    pub fn sd(&self) -> Option<f32> {
        (self.count > 0).then(|| sqrt(self.m2 / self.count as f32))
    }
}

/// A report line: `name`, then the count, mean and spread of `m`, with `-` for no value.
pub(crate) fn moments_line<const N: usize>(name: &str, m: &Moments) -> Text<N> {
    let mut line = Text::format(format_args!("{name} {}", m.count));
    for value in [m.mean(), m.sd()] {
        let _ = match value {
            Some(v) => write!(line, " {}", Hundredths(v)),
            None => write!(line, " -"),
        };
    }
    let _ = line.write_str("\n");
    line
}

pub const BINS: usize = 24;
/// Histogram ranges in dit units.
pub const MARK_RANGE: f32 = 4.0;
pub const GAP_RANGE: f32 = 8.0;

#[derive(Clone, Copy)]
pub struct Stats {
    pub marks: [u16; BINS],
    pub gaps: [u16; BINS],
    /// Lengths in ms.
    pub dit: Moments,
    pub dah: Moments,
    /// Lengths in units.
    pub element_gap: Moments,
    pub char_gap: Moments,
    pub word_gap: Moments,
    ideal_units: f32,
    total_ms: f32,
}

impl Default for Stats {
    fn default() -> Self {
        Stats {
            marks: [0; BINS],
            gaps: [0; BINS],
            dit: Moments::default(),
            dah: Moments::default(),
            element_gap: Moments::default(),
            char_gap: Moments::default(),
            word_gap: Moments::default(),
            ideal_units: 0.0,
            total_ms: 0.0,
        }
    }
}

impl Stats {
    pub fn add(&mut self, iv: &Interval) {
        let (bins, range) = if iv.kind.is_mark() { (&mut self.marks, MARK_RANGE) } else { (&mut self.gaps, GAP_RANGE) };
        let bin = ((iv.units / range * BINS as f32) as usize).min(BINS - 1);
        bins[bin] = bins[bin].saturating_add(1);
        match iv.kind {
            Kind::Dit => self.dit.add(iv.len_ms as f32),
            Kind::Dah => self.dah.add(iv.len_ms as f32),
            Kind::ElementGap => self.element_gap.add(iv.units),
            Kind::CharGap => self.char_gap.add(iv.units),
            Kind::WordGap => self.word_gap.add(iv.units),
        }
        self.ideal_units += iv.kind.ideal_units();
        self.total_ms += iv.len_ms as f32;
    }

    /// Dah length over dit length; 3 is standard.
    pub fn ratio(&self) -> Option<f32> {
        Some(self.dah.mean()? / self.dit.mean()?)
    }

    /// The speed the measured timing works out to.
    pub fn effective_wpm(&self) -> Option<f32> {
        (self.total_ms > 0.0).then(|| 1200.0 * self.ideal_units / self.total_ms)
    }

    pub fn is_empty(&self) -> bool {
        self.total_ms == 0.0
    }

    /// The lines of `STATS TIMING` (docs/dev/serial.md), each ending in `\n`.
    pub fn report<const N: usize>(&self) -> impl Iterator<Item = Text<N>> + '_ {
        // The longest line: "BINS MARK 4", then 24 counts of up to " 65535", then "\n".
        const { assert!(N >= 11 + BINS * 6 + 1) };
        let moments = [
            ("MARK DIT", &self.dit),
            ("MARK DAH", &self.dah),
            ("GAP ELEMENT", &self.element_gap),
            ("GAP CHAR", &self.char_gap),
            ("GAP WORD", &self.word_gap),
        ];
        let moments = moments.into_iter().map(|(name, m)| moments_line(name, m));
        let known = [("RATIO", self.ratio()), ("WPM", self.effective_wpm())]
            .into_iter()
            .filter_map(|(name, value)| Some(Text::format(format_args!("{name} {}\n", Hundredths(value?)))));
        let histograms = [("MARK", MARK_RANGE, &self.marks), ("GAP", GAP_RANGE, &self.gaps)].into_iter().map(|(name, range, bins)| {
            let mut line = Text::format(format_args!("BINS {name} {}", range as u32));
            for n in bins {
                let _ = write!(line, " {n}");
            }
            let _ = line.write_str("\n");
            line
        });
        moments.chain(known).chain(histograms)
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    /// Feeds alternating down/up runs starting at `t0`; returns the intervals.
    fn run(meter: &mut Meter, t0: u32, runs: &[u32], dit_ms: u16) -> Vec<Interval> {
        let mut t = t0;
        let mut out = Vec::new();
        out.extend(meter.edge(true, t, dit_ms));
        for (i, &len) in runs.iter().enumerate() {
            t += len;
            out.extend(meter.edge(i % 2 == 1, t, dit_ms));
        }
        out
    }

    #[test]
    fn the_report_gives_each_moment_the_ratio_the_speed_and_both_histograms() {
        let mut stats = Stats::default();
        let interval = |kind, len_ms, units| Interval { kind, start_ms: 0, len_ms, dit_ms: 60, units };
        stats.add(&interval(Kind::Dit, 60, 1.0));
        stats.add(&interval(Kind::Dit, 66, 1.1));
        stats.add(&interval(Kind::Dah, 189, 3.15));
        stats.add(&interval(Kind::ElementGap, 60, 1.0));
        let lines: Vec<std::string::String> = stats.report::<192>().map(|l| l.as_str().into()).collect();
        let mut marks = [0u16; BINS];
        marks[6] = 2;
        marks[18] = 1;
        let marks: std::string::String = marks.iter().map(|n| std::format!(" {n}")).collect();
        let gaps: std::string::String = (0..BINS).map(|i| if i == 3 { " 1" } else { " 0" }).collect();
        assert_eq!(
            lines,
            [
                "MARK DIT 2 63.00 3.00\n".into(),
                "MARK DAH 1 189.00 0.00\n".into(),
                "GAP ELEMENT 1 1.00 0.00\n".into(),
                "GAP CHAR 0 - -\n".into(),
                "GAP WORD 0 - -\n".into(),
                "RATIO 3.00\n".into(),
                "WPM 19.20\n".into(),
                std::format!("BINS MARK 4{marks}\n"),
                std::format!("BINS GAP 8{gaps}\n"),
            ]
        );
    }

    #[test]
    fn spread_stays_accurate_over_many_dits() {
        let mut dits = Moments::default();
        for i in 0..20_000 {
            dits.add(if i % 2 == 0 { 57.0 } else { 63.0 });
        }
        let (mean, sd) = (dits.mean().unwrap(), dits.sd().unwrap());
        assert!((mean - 60.0).abs() < 0.01 && (sd - 3.0).abs() < 0.03, "{mean} {sd}");
    }

    #[test]
    fn good_is_within_a_fifth_and_bad_is_past_halfway_to_a_misread() {
        use Kind::*;
        let q = Quality::of;
        assert_eq!([q(Dit, 1.19), q(Dit, 0.81), q(Dah, 3.59), q(WordGap, 5.61)], [Quality::Good; 4]);
        assert_eq!([q(Dit, 1.21), q(Dah, 3.61)], [Quality::Off; 2]);
        assert_eq!([q(Dit, 1.5), q(Dit, 1.51)], [Quality::Off, Quality::Bad], "split at 2 dots");
        assert_eq!([q(CharGap, 4.0), q(CharGap, 4.01)], [Quality::Off, Quality::Bad], "split at 5 dots");
        assert_eq!(q(Dah, 2.39), Quality::Bad, "past halfway to 2 dots, and outside good");
        assert_eq!([q(Dit, 0.3), q(Dah, 6.0), q(WordGap, 13.0)], [Quality::Off; 3], "no split on that side");
    }

    #[test]
    fn perfect_paris_is_exact() {
        // P .--. then A .- with a 3-unit gap, at 20 wpm: 60 ms units.
        let mut m = Meter::default();
        let ivs = run(&mut m, 1000, &[60, 60, 180, 60, 180, 60, 60, 180, 60, 60, 180], 60);
        let kinds: Vec<Kind> = ivs.iter().map(|i| i.kind).collect();
        use Kind::*;
        assert_eq!(kinds, [Dit, ElementGap, Dah, ElementGap, Dah, ElementGap, Dit, CharGap, Dit, ElementGap, Dah]);
        assert!(ivs.iter().all(|i| i.error().abs() < 1e-6));
        let mut stats = Stats::default();
        ivs.iter().for_each(|i| stats.add(i));
        assert!((stats.ratio().unwrap() - 3.0).abs() < 1e-4);
        assert!((stats.effective_wpm().unwrap() - 20.0).abs() < 1e-3);
        assert_eq!(stats.marks.iter().map(|&n| n as u32).sum::<u32>(), 6);
    }

    #[test]
    fn errors_and_char_worst() {
        let mut m = Meter::default();
        // Dit 20% long, element gap, dah 10% short, then a 4.5-unit char gap.
        let ivs = run(&mut m, 0, &[72, 60, 162, 270], 60);
        assert!((ivs[0].error() - 0.2).abs() < 1e-4);
        assert!((ivs[2].error() + 0.1).abs() < 1e-4);
        assert_eq!(m.char_worst(), Quality::Bad, "the char gap, past halfway to a word gap, starts the next character");
        assert_eq!(ivs[3].kind, Kind::CharGap);
        assert_eq!(m.char_span(), (564, 294));
    }

    #[test]
    fn long_silence_is_a_pause() {
        let mut m = Meter::default();
        let ivs = run(&mut m, 0, &[60, 60 * 20, 60], 60);
        assert_eq!(ivs.iter().map(|i| i.kind).collect::<Vec<_>>(), [Kind::Dit, Kind::Dit]);
        assert_eq!(m.char_worst(), Quality::Good);
    }

    #[test]
    fn sd_of_spread_dits() {
        let mut s = Stats::default();
        for len in [50, 70] {
            s.add(&Interval { kind: Kind::Dit, start_ms: 0, len_ms: len, dit_ms: 60, units: len as f32 / 60.0 });
        }
        assert!((s.dit.mean().unwrap() - 60.0).abs() < 1e-4);
        assert!((s.dit.sd().unwrap() - 10.0).abs() < 1e-3);
    }
}
