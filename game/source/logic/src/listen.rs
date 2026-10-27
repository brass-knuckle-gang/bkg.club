//! Decodes a keyed line at any speed from 5 to 60 WPM, stepped once per millisecond: the Pro
//! decoder board's tone line, or a straight key or sideswiper. The speed comes from the marks: two adjacent marks 2 to 4.5 times apart are a dot
//! and a dash. The letter gap is learned the same way from adjacent gaps, so Farnsworth spacing,
//! where letters are sent fast and spaced slow, decodes too.

use crate::decoder::{CHAR_GAP_DOTS, Decoded, MAX_ELEMENTS, WORD_GAP_DOTS};
use crate::morse;

/// Silence this long or shorter inside a mark is a dropout, not the end of the mark. The vendor
/// firmware's value for the board's tone detector.
const BRIDGE_MS: u32 = 6;
/// A shorter mark is noise: half a dot at 60 WPM.
const MIN_MARK_MS: u32 = 10;
/// A longer mark is a tuning carrier: it still decodes, but gives no speed sample.
const MAX_MARK_MS: u32 = 1000;
/// 60 and 5 WPM.
const MIN_DOT_MS: f32 = 20.0;
const MAX_DOT_MS: f32 = 240.0;
const MIN_DASH_DOTS: f32 = 2.0;
const MAX_DASH_DOTS: f32 = 4.5;
/// 20 WPM until the first dot and dash pair.
const START_DOT_MS: f32 = 60.0;
const START_DASH_MS: f32 = 180.0;
/// Standard spacing. Unlike the speed, this is a fair guess, so the first sample only averages.
const START_LETTER_GAP_DOTS: f32 = 3.0;
/// Standard 3:7 spacing puts the word threshold at 5/3 of the letter gap.
const WORD_GAP_LETTER_GAPS: f32 = 5.0 / 3.0;
/// Two adjacent letter gaps this far apart are a letter gap and a word gap (7:3 nominal).
const LETTER_PAIR_RATIO: f32 = 1.8;
/// A sample outside estimate / SNAP ..= estimate x SNAP is a new speed or spacing and replaces
/// the estimate. Jitter of 20 % per element stays inside it.
const SNAP: f32 = 1.5;
/// Otherwise samples average in with this weight.
const WEIGHT: f32 = 1.0 / 8.0;
/// PARIS: words per minute = 1200 / dot length in ms.
const WPM_DOT_MS: f32 = 1200.0;

pub struct Listener {
    now: u32,
    /// The first tone sample of the mark being heard.
    mark_start: Option<u32>,
    last_tone: u32,
    /// The first silent sample after the last real mark.
    last_end: Option<u32>,
    dot_ms: f32,
    dash_ms: f32,
    letter_gap_dots: f32,
    /// A dot and dash pair has set the speed; before that, gaps in dots mean nothing.
    locked: bool,
    previous_mark: Option<u32>,
    previous_letter_gap: Option<u32>,
    /// Mark lengths of the character being heard, classified when it ends.
    marks: [u32; MAX_ELEMENTS],
    len: usize,
    overflow: bool,
    word_open: bool,
}

impl Default for Listener {
    fn default() -> Self {
        Listener {
            now: 0,
            mark_start: None,
            last_tone: 0,
            last_end: None,
            dot_ms: START_DOT_MS,
            dash_ms: START_DASH_MS,
            letter_gap_dots: START_LETTER_GAP_DOTS,
            locked: false,
            previous_mark: None,
            previous_letter_gap: None,
            marks: [0; MAX_ELEMENTS],
            len: 0,
            overflow: false,
            word_open: false,
        }
    }
}

impl Listener {
    /// Starts at a known speed, such as the set speed for your own key: the first dot and dash
    /// pair refines it instead of setting it.
    pub fn at(dot_ms: u16) -> Listener {
        let dot_ms = (dot_ms as f32).clamp(MIN_DOT_MS, MAX_DOT_MS);
        Listener { dot_ms, dash_ms: 3.0 * dot_ms, locked: true, ..Listener::default() }
    }

    pub fn wpm(&self) -> u16 {
        (WPM_DOT_MS / self.dot_ms + 0.5) as u16
    }

    /// The dot length in use, to the nearest millisecond.
    pub fn dot_ms(&self) -> u16 {
        (self.dot_ms + 0.5) as u16
    }

    pub fn tick(&mut self, tone: bool) -> Option<Decoded> {
        self.now = self.now.wrapping_add(1);
        if tone {
            self.mark_start.get_or_insert(self.now);
            self.last_tone = self.now;
            return None;
        }
        if let Some(start) = self.mark_start {
            if self.now.wrapping_sub(self.last_tone) <= BRIDGE_MS {
                return None;
            }
            self.mark_start = None;
            self.end_mark(start);
        }
        let gap = self.now.wrapping_sub(self.last_end?) + 1;
        if self.len > 0 && gap as f32 >= CHAR_GAP_DOTS as f32 * self.dot_ms {
            return Some(self.end_character());
        }
        if self.word_open && self.len == 0 && gap as f32 >= self.word_gap_ms() {
            self.word_open = false;
            return Some(Decoded::Space);
        }
        None
    }

    fn word_gap_ms(&self) -> f32 {
        (WORD_GAP_DOTS as f32).max(WORD_GAP_LETTER_GAPS * self.letter_gap_dots) * self.dot_ms
    }

    fn end_mark(&mut self, start: u32) {
        let mark = self.last_tone.wrapping_sub(start) + 1;
        if mark < MIN_MARK_MS {
            return;
        }
        if let Some(end) = self.last_end {
            self.sample_gap(start.wrapping_sub(end));
        }
        self.sample_mark(mark);
        if self.len < MAX_ELEMENTS {
            self.marks[self.len] = mark;
            self.len += 1;
        } else {
            self.overflow = true;
        }
        self.last_end = Some(self.last_tone.wrapping_add(1));
    }

    fn sample_mark(&mut self, mark: u32) {
        if mark > MAX_MARK_MS {
            self.previous_mark = None;
            return;
        }
        let previous = self.previous_mark.replace(mark);
        let Some((dot, dash)) = previous.and_then(|p| pair(p, mark, MIN_DASH_DOTS, MAX_DASH_DOTS)) else {
            return;
        };
        let first = !core::mem::replace(&mut self.locked, true);
        let new_speed = first || follow(&mut self.dot_ms, dot);
        if first {
            (self.dot_ms, self.dash_ms) = (dot, dash);
        } else {
            follow(&mut self.dash_ms, dash);
        }
        self.dot_ms = self.dot_ms.clamp(MIN_DOT_MS, MAX_DOT_MS);
        self.dash_ms = self.dash_ms.clamp(MIN_DASH_DOTS * self.dot_ms, MAX_DASH_DOTS * self.dot_ms);
        if new_speed {
            // Gaps measured at the old speed say nothing about the spacing now.
            self.letter_gap_dots = START_LETTER_GAP_DOTS;
            self.previous_letter_gap = None;
        }
    }

    fn sample_gap(&mut self, gap: u32) {
        if !self.locked || (gap as f32) < CHAR_GAP_DOTS as f32 * self.dot_ms {
            return;
        }
        let previous = self.previous_letter_gap.replace(gap);
        if let Some((letter_gap, _)) = previous.and_then(|p| pair(p, gap, LETTER_PAIR_RATIO, f32::INFINITY)) {
            follow(&mut self.letter_gap_dots, letter_gap / self.dot_ms);
        }
    }

    fn end_character(&mut self) -> Decoded {
        let dash_from = (self.dot_ms + self.dash_ms) / 2.0;
        let mut pattern = [0u8; MAX_ELEMENTS];
        for (element, &mark) in pattern.iter_mut().zip(&self.marks[..self.len]) {
            *element = if mark as f32 >= dash_from { b'-' } else { b'.' };
        }
        let pattern = core::str::from_utf8(&pattern[..self.len]).unwrap_or("");
        let decoded = match morse::decode(pattern) {
            Some(c) if !self.overflow => Decoded::Char(c),
            _ => Decoded::Unknown,
        };
        self.len = 0;
        self.overflow = false;
        self.word_open = true;
        decoded
    }
}

/// The shorter and the longer of `a` and `b`, when the longer is `low..=high` times the shorter.
fn pair(a: u32, b: u32, low: f32, high: f32) -> Option<(f32, f32)> {
    let (short, long) = (a.min(b) as f32, a.max(b) as f32);
    (long >= low * short && long <= high * short).then_some((short, long))
}

/// Moves `estimate` to `sample`: replaced when `sample` is outside SNAP of it, else averaged in.
/// True when replaced.
fn follow(estimate: &mut f32, sample: f32) -> bool {
    let replace = sample < *estimate / SNAP || sample > *estimate * SNAP;
    *estimate = if replace { sample } else { *estimate + WEIGHT * (sample - *estimate) };
    replace
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::rng::Rng;
    use std::string::String;
    use std::vec::Vec;

    /// Tone runs, `(tone, ms)`.
    type Runs = Vec<(bool, u32)>;

    /// How the test text is keyed. Lengths are rounded like the sent signal would be: to whole
    /// milliseconds, ties to even.
    #[derive(Clone, Copy)]
    struct Keying {
        dot_ms: f64,
        dash_dots: f64,
        /// Farnsworth: letter and word gaps times this.
        stretch: f64,
        /// Each element and gap is off by up to this fraction, from `seed`.
        jitter: f64,
        seed: u32,
        /// A 1 to 6 ms dropout in every mark of 20 ms or more.
        dropouts: bool,
        /// A 1 to 9 ms blip in every gap of 30 ms or more, more than BRIDGE_MS from any mark.
        blips: bool,
        lead_ms: u32,
        tail_ms: u32,
    }

    fn at(wpm: f64) -> Keying {
        Keying {
            dot_ms: 1200.0 / wpm,
            dash_dots: 3.0,
            stretch: 1.0,
            jitter: 0.0,
            seed: 1,
            dropouts: false,
            blips: false,
            lead_ms: 10,
            tail_ms: 3000,
        }
    }

    /// ARRL Farnsworth timing: the gap stretch for characters at `char_wpm` and text at
    /// `overall_wpm`.
    fn farnsworth(char_wpm: f64, overall_wpm: f64) -> f64 {
        let gap_s_per_word = (60.0 * char_wpm - 37.2 * overall_wpm) / (overall_wpm * char_wpm);
        gap_s_per_word / 19.0 / (1.2 / char_wpm)
    }

    fn key(text: &str, k: Keying) -> Runs {
        let mut rng = Rng::new(k.seed);
        let mut vary = |ms: f64| -> u32 {
            if k.jitter == 0.0 {
                return ms.round_ties_even() as u32;
            }
            let pct = (k.jitter * 100.0) as u32;
            let off = rng.below(2 * pct + 1) as f64 - pct as f64;
            ((ms * (1.0 + off / 100.0)).round_ties_even() as u32).max(1)
        };
        let mut lengths = Vec::new();
        let words: Vec<&str> = text.split_whitespace().collect();
        for (wi, word) in words.iter().enumerate() {
            for (ci, c) in word.bytes().enumerate() {
                let code = morse::code(c).unwrap();
                for (ei, e) in code.bytes().enumerate() {
                    lengths.push((true, vary(if e == b'.' { k.dot_ms } else { k.dash_dots * k.dot_ms })));
                    if ei + 1 < code.len() {
                        lengths.push((false, vary(k.dot_ms)));
                    }
                }
                if ci + 1 < word.len() {
                    lengths.push((false, vary(3.0 * k.dot_ms * k.stretch)));
                }
            }
            if wi + 1 < words.len() {
                lengths.push((false, vary(7.0 * k.dot_ms * k.stretch)));
            }
        }
        let mut runs = std::vec![(false, k.lead_ms)];
        for (tone, ms) in lengths {
            if tone && k.dropouts && ms >= 20 {
                let drop = 1 + rng.below(6);
                let first = 1 + rng.below(ms - drop - 1);
                runs.extend([(true, first), (false, drop), (true, ms - first - drop)]);
            } else if !tone && k.blips && ms >= 30 {
                let blip = 1 + rng.below(9);
                let margin = BRIDGE_MS + 1;
                let first = margin + rng.below(ms - blip - 2 * margin + 1);
                runs.extend([(false, first), (true, blip), (false, ms - first - blip)]);
            } else {
                runs.push((tone, ms));
            }
        }
        runs.push((false, k.tail_ms));
        runs
    }

    fn text_of(d: Decoded) -> String {
        match d {
            Decoded::Char(c) => String::from(morse::text(&c)),
            Decoded::Unknown => String::from("*"),
            Decoded::Space => String::from(" "),
        }
    }

    fn listen(listener: &mut Listener, runs: &[(bool, u32)]) -> String {
        let mut out = String::new();
        for &(tone, ms) in runs {
            for _ in 0..ms {
                out.extend(listener.tick(tone).map(text_of));
            }
        }
        out
    }

    fn decode(runs: &[(bool, u32)]) -> (String, Listener) {
        let mut listener = Listener::default();
        let out = listen(&mut listener, runs);
        (out, listener)
    }

    fn words(s: &str) -> String {
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// The fewest sent characters, counted from `start`, that `out` may get wrong: `out` is
    /// `sent[..start]`, then anything, then the rest of `sent` from some character on. None when
    /// no count works.
    fn misread(out: &str, sent: &str, start: usize) -> Option<usize> {
        let (out, sent) = (words(out), words(sent));
        let prefix = &sent[..start];
        if !out.starts_with(prefix) {
            return None;
        }
        let positions: Vec<usize> = (start..sent.len()).filter(|&i| sent.as_bytes()[i] != b' ').collect();
        (0..=positions.len()).find(|&n| {
            let rest = positions.get(n).map_or("", |&i| sent[i..].trim_start());
            out.ends_with(rest) && out.len() >= prefix.len() + rest.len()
        })
    }

    /// The character, counted without spaces, that holds the second mark of the first dot and
    /// dash pair: before it, nothing tells the speed.
    fn first_pair_character(text: &str) -> usize {
        let mut previous = None;
        for (i, c) in text.bytes().filter(|&c| c != b' ').enumerate() {
            for e in morse::code(c).unwrap().bytes() {
                if previous.is_some_and(|p| p != e) {
                    return i;
                }
                previous = Some(e);
            }
        }
        usize::MAX
    }

    const TEXTS: [&str; 5] =
        ["THE QUICK BROWN FOX JUMPS OVER THE LAZY DOG 0123456789", "CQ CQ DE K5MME K5MME PSE K", "EISH5 TEST 73", "5NN TU", "PARIS PARIS PARIS"];
    const SEEDS: core::ops::RangeInclusive<u32> = 1..=100;

    #[test]
    fn clean_text_decodes_from_the_character_after_the_first_dot_dash_pair() {
        for wpm in [5.0, 8.0, 12.0, 15.0, 20.0, 25.0, 30.0, 45.0, 60.0] {
            // Near the 20 WPM start, nothing may be misread.
            let allowed_by_speed = if (15.0..=25.0).contains(&wpm) { 0 } else { 2 };
            for text in TEXTS {
                let allowed = if allowed_by_speed == 0 { 0 } else { allowed_by_speed.max(first_pair_character(text) + 1) };
                let (out, listener) = decode(&key(text, at(wpm)));
                let n = misread(&out, text, 0);
                assert!(n.is_some_and(|n| n <= allowed), "{wpm} WPM {text:?}: got {out:?}");
                if text == TEXTS[0] {
                    let error = (listener.wpm() as f64 - wpm).abs() / wpm;
                    assert!(error <= 0.1, "{wpm} WPM read as {}", listener.wpm());
                }
            }
        }
    }

    #[test]
    fn jitter_and_weighting_decode_after_the_first_word() {
        for wpm in [12.0, 20.0, 35.0] {
            for dash_dots in [2.5, 3.5] {
                for text in &TEXTS[..2] {
                    let allowed = text.split(' ').next().unwrap().len();
                    for seed in SEEDS {
                        let k = Keying { dash_dots, jitter: 0.2, seed, ..at(wpm) };
                        let (out, _) = decode(&key(text, k));
                        assert!(misread(&out, text, 0).is_some_and(|n| n <= allowed), "{wpm} WPM dash {dash_dots} seed {seed}: {out:?}");
                    }
                }
            }
        }
    }

    /// `out` is anything, then each of `parts` without its first word, with anything between.
    fn exact_after_first_words(out: &str, parts: &[&str]) -> bool {
        let out = words(out);
        let mut from = 0;
        for part in parts {
            let rest = part.split_once(' ').map_or("", |(_, rest)| rest);
            let Some(at) = out[from..].find(rest) else {
                return false;
            };
            from += at + rest.len();
        }
        from == out.len()
    }

    #[test]
    fn farnsworth_decodes_after_the_first_word_space() {
        let text = TEXTS[1];
        for (char_wpm, overall_wpm) in [(20.0, 8.0), (18.0, 10.0)] {
            let stretch = farnsworth(char_wpm, overall_wpm);
            let (out, _) = decode(&key(text, Keying { stretch, ..at(char_wpm) }));
            assert!(exact_after_first_words(&out, &[text]), "{char_wpm}/{overall_wpm}: {out:?}");
        }
    }

    #[test]
    fn spacing_changes_between_farnsworth_and_standard() {
        let (slow, normal) = ("CQ CQ DE K5MME K5MME", "THE QUICK BROWN FOX 73");
        for (char_wpm, overall_wpm) in [(20.0, 8.0), (18.0, 10.0)] {
            let stretch = farnsworth(char_wpm, overall_wpm);
            let dot_ms = 1200.0 / char_wpm;
            let spaced = Keying { stretch, ..at(char_wpm) };
            let mut runs = key(slow, Keying { tail_ms: 0, ..spaced });
            runs.push((false, (7.0 * dot_ms * stretch).round() as u32));
            runs.extend(key(normal, Keying { lead_ms: 0, ..at(char_wpm) }));
            let (out, _) = decode(&runs);
            assert!(exact_after_first_words(&out, &[slow, normal]), "{char_wpm}/{overall_wpm} then standard: {out:?}");

            let mut runs = key(normal, Keying { tail_ms: 0, ..at(char_wpm) });
            runs.push((false, (7.0 * dot_ms).round() as u32));
            runs.extend(key(slow, Keying { lead_ms: 0, ..spaced }));
            let (out, _) = decode(&runs);
            assert!(exact_after_first_words(&out, &[normal, slow]), "standard then {char_wpm}/{overall_wpm}: {out:?}");
        }
    }

    #[test]
    fn pauses_between_overs_keep_every_word() {
        for (first, second) in [("CQ CQ DE K5MME", "K5MME DE W1AW K"), ("CQ CQ DE K5MME", "R TU 73")] {
            for wpm in [12.0, 20.0, 30.0] {
                for pause in [1000, 1500, 3000, 10000] {
                    let mut runs = key(first, Keying { tail_ms: 0, ..at(wpm) });
                    runs.push((false, pause));
                    runs.extend(key(second, Keying { lead_ms: 0, ..at(wpm) }));
                    let (out, _) = decode(&runs);
                    assert_eq!(words(&out), std::format!("{first} {second}"), "{wpm} WPM, {pause} ms pause");
                }
            }
        }
    }

    #[test]
    fn dropouts_and_blips_change_nothing() {
        for wpm in [20.0, 40.0] {
            for text in TEXTS {
                let allowed = if wpm == 20.0 { 0 } else { 2.max(first_pair_character(text) + 1) };
                for seed in SEEDS {
                    let (out, _) = decode(&key(text, Keying { seed, dropouts: true, blips: true, ..at(wpm) }));
                    assert!(misread(&out, text, 0).is_some_and(|n| n <= allowed), "{wpm} WPM seed {seed}: {out:?}");
                }
            }
        }
    }

    #[test]
    fn speed_changes_decode_again_within_two_characters() {
        let (before, after) = ("CQ CQ DE K5MME", "THE QUICK BROWN FOX 73");
        let sent = std::format!("{before} {after}");
        for (from, to) in [(20.0, 40.0), (40.0, 13.0), (10.0, 30.0), (30.0, 10.0), (13.0, 40.0), (40.0, 20.0), (20.0, 28.0), (28.0, 20.0)] {
            // The text before the change must read as it does alone; the clean test judges that.
            let (alone, _) = decode(&key(before, at(from)));
            for gap_wpm in [from, to] {
                let mut runs = key(before, Keying { tail_ms: 0, ..at(from) });
                runs.push((false, (7.0 * 1200.0 / gap_wpm).round() as u32));
                runs.extend(key(after, Keying { lead_ms: 0, ..at(to) }));
                let (out, _) = decode(&runs);
                let out = words(&out).replacen(&words(&alone), before, 1);
                // The gap before the change is already new-speed timing, so it counts in the two.
                let n = misread(&out, &sent, before.len());
                assert!(n.is_some_and(|n| n <= 2), "{from} -> {to}, word gap at {gap_wpm}: {out:?}");
            }
        }
    }

    #[test]
    fn a_noise_burst_does_not_set_the_speed() {
        // A burst of 12 to 30 ms is a mark, 6 to 15 times shorter than the dash before it: too far
        // apart to be a dot and dash pair, so the speed holds. 200 ms gaps are letter gaps, so the
        // burst reads as an E between the words.
        for burst in [12, 16, 20, 25, 30] {
            let mut runs = key("CQ CQ", Keying { tail_ms: 0, ..at(20.0) });
            runs.extend([(false, 200), (true, burst), (false, 200)]);
            runs.extend(key("DE K5MME PSE K", Keying { lead_ms: 0, ..at(20.0) }));
            let (out, _) = decode(&runs);
            assert_eq!(words(&out), "CQ CQEDE K5MME PSE K", "{burst} ms burst");
        }
    }

    #[test]
    fn words_of_only_dots_or_only_dashes_keep_the_speed() {
        let lead = "CQ DE K5MME";
        for word in ["EISH5 SEE", "TMO0"] {
            let mut runs = key(lead, Keying { tail_ms: 0, ..at(20.0) });
            let mut listener = Listener::default();
            let mut out = listen(&mut listener, &runs);
            let dot_ms = listener.dot_ms;
            runs.clear();
            runs.push((false, 420));
            runs.extend(key(word, Keying { lead_ms: 0, ..at(20.0) }));
            out += &listen(&mut listener, &runs);
            assert_eq!(words(&out), std::format!("{lead} {word}"));
            assert_eq!(listener.dot_ms, dot_ms);
        }
    }

    /// Inputs from the vendor decoder's tests, as runs of 1 ms samples: `H` tone, `L` silence.
    /// `sent` is the text they were keyed from, or for noise what BRIDGE_MS and MIN_MARK_MS make
    /// of it.
    struct Case {
        name: &'static str,
        input: &'static str,
        sent: &'static str,
    }

    const CASES: &[Case] = &[
        Case { name: "paris_x3_20wpm", input: "L10 H60 L60 H180 L60 H180 L60 H60 L180 H60 L60 H180 L180 H60 L60 H180 L60 H60 L180 H60 L60 H60 L180 H60 L60 H60 L60 H60 L420 H60 L60 H180 L60 H180 L60 H60 L180 H60 L60 H180 L180 H60 L60 H180 L60 H60 L180 H60 L60 H60 L180 H60 L60 H60 L60 H60 L420 H60 L60 H180 L60 H180 L60 H60 L180 H60 L60 H180 L180 H60 L60 H180 L60 H60 L180 H60 L60 H60 L180 H60 L60 H60 L60 H60 L1500", sent: "PARIS PARIS PARIS " },
        Case { name: "cq_12wpm", input: "L10 H300 L100 H100 L100 H300 L100 H100 L300 H300 L100 H300 L100 H100 L100 H300 L700 H300 L100 H100 L100 H300 L100 H100 L300 H300 L100 H300 L100 H100 L100 H300 L700 H300 L100 H100 L100 H100 L300 H100 L700 H300 L100 H100 L300 H300 L100 H300 L100 H300 L100 H300 L100 H300 L300 H300 L100 H100 L100 H300 L100 H100 L300 H100 L100 H300 L300 H100 L100 H300 L100 H100 L100 H100 L300 H100 L100 H300 L100 H100 L100 H100 L700 H300 L100 H100 L100 H300 L1500", sent: "CQ CQ DE N0CALL K " },
        Case { name: "cq_20wpm", input: "L10 H180 L60 H60 L60 H180 L60 H60 L180 H180 L60 H180 L60 H60 L60 H180 L420 H180 L60 H60 L60 H180 L60 H60 L180 H180 L60 H180 L60 H60 L60 H180 L420 H180 L60 H60 L60 H60 L180 H60 L420 H180 L60 H60 L180 H180 L60 H180 L60 H180 L60 H180 L60 H180 L180 H180 L60 H60 L60 H180 L60 H60 L180 H60 L60 H180 L180 H60 L60 H180 L60 H60 L60 H60 L180 H60 L60 H180 L60 H60 L60 H60 L420 H180 L60 H60 L60 H180 L1500", sent: "CQ CQ DE N0CALL K " },
        Case { name: "cq_30wpm", input: "L10 H120 L40 H40 L40 H120 L40 H40 L120 H120 L40 H120 L40 H40 L40 H120 L280 H120 L40 H40 L40 H120 L40 H40 L120 H120 L40 H120 L40 H40 L40 H120 L280 H120 L40 H40 L40 H40 L120 H40 L280 H120 L40 H40 L120 H120 L40 H120 L40 H120 L40 H120 L40 H120 L120 H120 L40 H40 L40 H120 L40 H40 L120 H40 L40 H120 L120 H40 L40 H120 L40 H40 L40 H40 L120 H40 L40 H120 L40 H40 L40 H40 L280 H120 L40 H40 L40 H120 L1500", sent: "CQ CQ DE N0CALL K " },
        Case { name: "speed_change_20_to_30wpm", input: "L10 H180 L60 H60 L60 H180 L60 H60 L180 H180 L60 H180 L60 H60 L60 H180 L420 H180 L60 H60 L60 H180 L60 H60 L180 H180 L60 H180 L60 H60 L60 H180 L420 H180 L60 H60 L60 H60 L180 H60 L420 H180 L60 H60 L180 H180 L60 H180 L60 H180 L60 H180 L60 H180 L180 H180 L60 H60 L60 H180 L60 H60 L180 H60 L60 H180 L180 H60 L60 H180 L60 H60 L60 H60 L180 H60 L60 H180 L60 H60 L60 H60 L420 H120 L40 H40 L40 H120 L40 H40 L120 H120 L40 H120 L40 H40 L40 H120 L280 H120 L40 H40 L40 H120 L40 H40 L120 H120 L40 H120 L40 H40 L40 H120 L280 H120 L40 H40 L40 H40 L120 H40 L280 H120 L40 H40 L120 H120 L40 H120 L40 H120 L40 H120 L40 H120 L120 H120 L40 H40 L40 H120 L40 H40 L120 H40 L40 H120 L120 H40 L40 H120 L40 H40 L40 H40 L120 H40 L40 H120 L40 H40 L40 H40 L280 H120 L40 H40 L40 H120 L1500", sent: "CQ CQ DE N0CALL CQ CQ DE N0CALL K " },
        Case { name: "dropout_6ms_in_dashes_is_bridged", input: "L10 H90 L6 H84 L180 H60 L180 H60 L60 H60 L60 H60 L180 H90 L6 H84 L420 H90 L6 H84 L60 H90 L6 H84 L180 H90 L6 H84 L60 H90 L6 H84 L60 H90 L6 H84 L180 H90 L6 H84 L60 H90 L6 H84 L1500", sent: "TEST MOM " },
        Case { name: "dropout_7ms_in_dashes_splits_them", input: "L10 H90 L7 H83 L180 H60 L180 H60 L60 H60 L60 H60 L180 H90 L7 H83 L420 H90 L7 H83 L60 H90 L7 H83 L180 H90 L7 H83 L60 H90 L7 H83 L60 H90 L7 H83 L180 H90 L7 H83 L60 H90 L7 H83 L1500", sent: "IESI H*H " },
        Case { name: "blips_3ms_in_letter_and_word_gaps_are_ignored", input: "L10 H60 L60 H180 L60 H180 L60 H60 L90 H3 L87 H60 L60 H180 L180 H60 L60 H180 L60 H60 L180 H60 L60 H60 L180 H60 L60 H60 L60 H60 L210 H3 L207 H60 L60 H180 L60 H180 L60 H60 L180 H60 L60 H180 L180 H60 L60 H180 L60 H60 L180 H60 L60 H60 L180 H60 L60 H60 L60 H60 L1500", sent: "PARIS PARIS " },
        Case { name: "mark_of_10ms_alone_is_e", input: "L510 H10 L1500", sent: "E " },
        Case { name: "long_silence_after_text", input: "L10 H180 L60 H60 L60 H180 L20000", sent: "K " },
        Case { name: "eight_dots_are_the_error_signal", input: "L10 H60 L60 H60 L60 H60 L60 H60 L60 H60 L60 H60 L60 H60 L60 H60 L1500", sent: "<HH> " },
        Case { name: "nine_dots_are_unknown", input: "L10 H60 L60 H60 L60 H60 L60 H60 L60 H60 L60 H60 L60 H60 L60 H60 L60 H60 L1500", sent: "* " },
        Case { name: "unknown_pattern_then_e", input: "L10 H60 L60 H60 L60 H180 L60 H180 L180 H60 L1500", sent: "*E " },
        Case { name: "single_dot", input: "L10 H60 L1000", sent: "E " },
        Case { name: "long_silence_then_more_text", input: "L10 H60 L5000 H60 L1500", sent: "E E " },
    ];

    #[test]
    fn vendor_test_inputs() {
        for case in CASES {
            let runs: Vec<(bool, u32)> = case.input.split_whitespace().map(|r| (r.starts_with('H'), r[1..].parse().unwrap())).collect();
            assert_eq!(decode(&runs).0, case.sent, "{}", case.name);
        }
    }
}
