//! QSO bot for park (POTA) and summit (SOTA) activations. You are the activator: you call CQ, a
//! pileup of hunters answers, and you work them one at a time. The bot reads each over you send
//! and answers the way hunters do on the air.

use core::fmt::Write;

use crate::player::MAX_TEXT;
use crate::rng::Rng;
use crate::text::Text;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Program {
    Pota,
    Sota,
}

impl Program {
    pub fn name(self) -> &'static str {
        match self {
            Program::Pota => "POTA",
            Program::Sota => "SOTA",
        }
    }

    /// Contacts that make a valid activation: 10 for a park, 4 for summit points.
    pub fn goal(self) -> u8 {
        match self {
            Program::Pota => 10,
            Program::Sota => 4,
        }
    }

    /// The sign-off number its activators send.
    pub fn bye(self) -> &'static str {
        match self {
            Program::Pota => "72",
            Program::Sota => "73",
        }
    }
}

pub const PILE_MAX: usize = 4;

/// How the hunters behave.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Options {
    /// Hunters' base speed; 0 matches your sending speed.
    pub wpm: u8,
    /// Several hunters call at once; otherwise one at a time.
    pub pileup: bool,
    /// Hunters send at different speeds around the base speed.
    pub spread: bool,
    /// Hunters key with human timing jitter.
    pub human_fist: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { wpm: 0, pileup: true, spread: true, human_fist: true }
    }
}
pub const CALL_LEN: usize = crate::settings::CALLSIGN_LEN;

#[derive(Clone, Copy)]
pub struct Hunter {
    pub call: Text<CALL_LEN>,
    pub state: &'static str,
    /// The report this hunter gives you.
    pub report: &'static str,
    habits: Habits,
    wpm: u8,
    fist: u8,
    pub pitch_offset_hz: i16,
}

/// One transmission from one hunter.
#[derive(Clone, Copy)]
pub struct Tx {
    /// Index into the pileup at the time it was sent.
    pub station: usize,
    pub text: Text<MAX_TEXT>,
    pub wpm: u8,
    pub fist: u8,
    pub pitch_offset_hz: i16,
    /// Silence before it, after the previous transmission ends.
    pub delay_ms: u32,
    /// Sent by the hunter you are working, as opposed to the pileup calling.
    pub working: bool,
}

/// The contact in progress.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Exchange {
    /// The hunter has sent its report and state.
    pub replied: bool,
    /// Whether you sent the hunter's state back when you closed the contact.
    pub state_copied: Option<bool>,
}

/// Bot answers queue here; the caller plays them in order.
pub type Out<'a> = &'a mut dyn FnMut(Tx);

pub struct Bot {
    pub program: Program,
    options: Options,
    my_call: Text<CALL_LEN>,
    rng: Rng,
    pile: [Option<Hunter>; PILE_MAX],
    working: Option<usize>,
    pub exchange: Exchange,
    pub logged: u8,
    /// Contacts closed with the hunter's state sent back correctly.
    pub copied: u8,
    /// The last transmissions, at the hunters' own speeds.
    last: [Option<Tx>; PILE_MAX],
    /// Words per minute slower than the hunters' own speed, from your QRS requests to the hunter
    /// you are working.
    qrs: u8,
    /// GM, GA or GE: one time of day for the activation.
    greeting: &'static str,
}

pub(crate) const QRS_STEP: u8 = 4;
pub(crate) const MIN_WPM: u8 = 8;
pub(crate) const MAX_WPM: u8 = 40;

impl Bot {
    pub fn new(program: Program, my_call: &str, seed: u32, options: Options) -> Bot {
        let call = Text::format(format_args!("{my_call}"));
        let mut rng = Rng::new(seed);
        let greeting = GREETINGS[rng.below(GREETINGS.len() as u32) as usize];
        Bot {
            program,
            options,
            my_call: call,
            rng,
            pile: [None; PILE_MAX],
            working: None,
            exchange: Exchange::default(),
            logged: 0,
            copied: 0,
            last: [None; PILE_MAX],
            qrs: 0,
            greeting,
        }
    }

    /// Applies new options to the hunters already here, who keep their habits and send at their
    /// own speed again, without QRS. Without a pileup only the hunter you are working (or the
    /// first) stays.
    pub fn set_options(&mut self, options: Options, my_wpm: u8) {
        self.options = options;
        self.qrs = 0;
        if !options.pileup {
            let keep = self.working.unwrap_or_else(|| self.first_in_pile());
            for (i, slot) in self.pile.iter_mut().enumerate() {
                if i != keep {
                    *slot = None;
                }
            }
        }
        for i in 0..PILE_MAX {
            let Some(mut h) = self.pile[i] else { continue };
            (h.wpm, h.fist) = h.habits.keying(self.options, my_wpm);
            self.pile[i] = Some(h);
        }
        for slot in &mut self.last {
            *slot = slot.and_then(|tx| {
                let h = self.pile[tx.station]?;
                Some(Tx { wpm: h.wpm, fist: h.fist, ..tx })
            });
        }
    }

    /// Your call changed: the hunters listen for the new one.
    pub fn set_call(&mut self, my_call: &str) {
        self.my_call = Text::format(format_args!("{my_call}"));
    }

    /// The hunter you are working, if any.
    pub fn working(&self) -> Option<&Hunter> {
        self.pile[self.working?].as_ref()
    }

    pub fn my_call(&self) -> &str {
        self.my_call.as_str()
    }

    pub fn pile_size(&self) -> usize {
        self.pile.iter().flatten().count()
    }

    pub fn pile(&self) -> impl Iterator<Item = &Hunter> {
        self.pile.iter().flatten()
    }

    /// Answers one over you sent. `my_wpm` sets the pace hunters match.
    pub fn heard(&mut self, over: &str, my_wpm: u8, out: Out) {
        let mut words: [&str; 16] = [""; 16];
        let mut n = 0;
        for w in over.split_ascii_whitespace().take(16) {
            words[n] = w;
            n += 1;
        }
        let words = &words[..n];
        let has = |w: &str| words.contains(&w);
        // The over with spaces removed and your call cut out, so calls match however the
        // decoder split or joined the words around them.
        let squashed = Text::<MAX_TEXT>::format(format_args!("{}", Squashed(over)));
        let names_me = !self.my_call.is_empty() && crate::text::has(squashed.as_str(), self.my_call.as_str());
        let others = cut(squashed.as_str(), self.my_call.as_str());

        if has("QRS") {
            self.qrs = self.qrs.saturating_add(QRS_STEP);
            return self.repeat(out);
        }
        if has("AGN") || has("AGN?") || words == ["?"] {
            return self.repeat(out);
        }

        if self.exchange.replied && self.closes(&others) {
            let names_next = self.exact_call(others.as_str()).is_some_and(|i| Some(i) != self.working);
            if !names_next {
                self.farewell(out);
            }
            self.log(words);
            // A call after the closing works that hunter next: TU NY 72 K2XYZ 5NN.
            if let Some(i) = self.exact_call(others.as_str()) {
                self.working = Some(i);
                return self.send_exchange(i, out);
            }
            return self.pile_calls(my_wpm, false, out);
        }

        if let Some(i) = self.exact_call(others.as_str()) {
            self.working = Some(i);
            self.exchange = Exchange::default();
            return self.send_exchange(i, out);
        }
        if let Some(i) = self.partial_call(words) {
            let hunter = self.pile[i].expect("partial_call returns a pileup slot");
            let text = Text::format(format_args!("{0} {0}", hunter.call.as_str()));
            return self.send(out, [Some((i, text, false))]);
        }

        if has("CQ") || has("QRZ") || has("QRZ?") {
            if has("CQ") && !self.my_call.is_empty() && !names_me {
                if self.pile_size() == 0 {
                    self.spawn(my_wpm, 1);
                }
                let i = self.first_in_pile();
                return self.send(out, [Some((i, text_of("CALL?"), false))]);
            }
            return self.pile_calls(my_wpm, true, out);
        }

        // Anything else: the hunter you are working asks again; otherwise the pileup calls.
        match self.working {
            Some(i) => self.send(out, [Some((i, text_of("?"), true))]),
            None => self.pile_calls(my_wpm, false, out),
        }
    }

    /// Sends the last transmissions again, at the current QRS speed.
    pub fn repeat(&mut self, out: Out) {
        for tx in self.last.iter().flatten() {
            let mut tx = *tx;
            tx.wpm = self.pace(tx.wpm);
            out(tx);
        }
    }

    /// A closing over: TU, 73, 72, TNX, or E E. `others` is the squashed over without your
    /// call; the pileup's calls are cut out too, since a call can hold any of these.
    fn closes(&self, others: &Text<MAX_TEXT>) -> bool {
        let mut rest = *others;
        for h in self.pile.iter().flatten() {
            rest = cut(rest.as_str(), h.call.as_str());
        }
        ["TU", "73", "72", "TNX", "EE", "<SK>"].iter().any(|w| crate::text::has(rest.as_str(), w))
    }

    /// Two hunters in three sign off before they go.
    fn farewell(&mut self, out: Out) {
        let Some(i) = self.working else { return };
        let text = match self.random(3) {
            0 => return,
            1 => "E E",
            _ => "73 E E",
        };
        self.send(out, [Some((i, text_of(text), true))]);
    }

    fn log(&mut self, words: &[&str]) {
        let Some(i) = self.working.take() else { return };
        let Some(hunter) = self.pile[i].take() else { return };
        // A DX hunter sends no state, so there is nothing to copy back.
        let copied = hunter.state.is_empty() || sent_word(words, hunter.state);
        self.exchange = Exchange { replied: false, state_copied: Some(copied) };
        self.logged = self.logged.saturating_add(1);
        self.copied = self.copied.saturating_add(copied as u8);
        self.qrs = 0;
    }

    /// The pileup hunter whose whole call is in `squashed`; the longest wins, so K5ABC beats
    /// K5AB.
    fn exact_call(&self, squashed: &str) -> Option<usize> {
        let found = |h: &Option<Hunter>| h.filter(|h| crate::text::has(squashed, h.call.as_str())).map(|h| h.call.len());
        (0..PILE_MAX).filter(|&i| found(&self.pile[i]).is_some()).max_by_key(|&i| found(&self.pile[i]))
    }

    /// The hunter whose call best matches a fragment: a word with '?', a piece of a call, or a
    /// call with a character wrong.
    fn partial_call(&self, words: &[&str]) -> Option<usize> {
        let mut best = None;
        let mut best_score = 1;
        for w in words {
            let fragment = w.trim_end_matches('?');
            if fragment.len() < 2 || fragment == self.my_call.as_str() {
                continue;
            }
            for (i, h) in self.pile.iter().enumerate() {
                let Some(h) = h else { continue };
                let call = h.call.as_str();
                let same = call.bytes().zip(fragment.bytes()).filter(|(a, b)| a == b).count();
                let score = if crate::text::has(call, fragment) {
                    fragment.len() + 1
                } else if call.len() == fragment.len() && same + 1 >= call.len() {
                    same
                } else {
                    0
                };
                if score > best_score {
                    best_score = score;
                    best = Some(i);
                }
            }
        }
        best
    }

    fn send_exchange(&mut self, i: usize, out: Out) {
        let Some(h) = self.pile[i] else { return };
        let variant = self.random(6);
        let (g, st, rst, me) = (self.greeting, h.state, h.report, self.my_call.as_str());
        let text = match (self.program, variant) {
            (Program::Pota, 0) => phrase(&["RR", "TU", rst, st, st, "72"]),
            (Program::Pota, 1) => phrase(&["TU", "UR", rst, rst, st, st, "73"]),
            (Program::Pota, 2) => phrase(&[me, "TU", rst, st, "72"]),
            (Program::Pota, 3) => phrase(&["R", "TU", rst, st, "BK"]),
            (Program::Pota, 4) => phrase(&[g, "UR", rst, rst, st, st, "BK"]),
            (Program::Pota, _) => phrase(&["R", g, "TU", rst, st, st, "73"]),
            (Program::Sota, 0 | 1) => phrase(&["R", "R", "TU", rst, rst, st, "73"]),
            (Program::Sota, 2 | 3) => phrase(&["TU", rst, st, st, "73"]),
            (Program::Sota, _) => phrase(&[g, "UR", rst, rst, st, st, "73"]),
        };
        self.exchange.replied = true;
        self.send(out, [Some((i, text, true))]);
    }

    /// Each hunter in the pileup calls, in random order; new hunters may join first.
    fn pile_calls(&mut self, my_wpm: u8, invited: bool, out: Out) {
        let joining = if invited && self.pile_size() == 0 { 1 + self.random(3) } else { self.random(2) };
        self.spawn(my_wpm, joining as usize);
        let mut order = [0, 1, 2, 3];
        for k in (1..PILE_MAX).rev() {
            order.swap(k, self.random(k as u32 + 1) as usize);
        }
        let mut calls: [Option<(usize, Text<MAX_TEXT>, bool)>; PILE_MAX] = [None; PILE_MAX];
        for (slot, &i) in calls.iter_mut().zip(order.iter()) {
            if let Some(h) = self.pile[i] {
                let text = if self.random(3) == 0 {
                    Text::format(format_args!("{0} {0}", h.call.as_str()))
                } else {
                    Text::format(format_args!("{}", h.call.as_str()))
                };
                *slot = Some((i, text, false));
            }
        }
        self.send(out, calls);
    }

    fn first_in_pile(&self) -> usize {
        self.pile.iter().position(Option::is_some).unwrap_or(0)
    }

    fn send<const N: usize>(&mut self, out: Out, txs: [Option<(usize, Text<MAX_TEXT>, bool)>; N]) {
        self.last = [None; PILE_MAX];
        let mut k = 0;
        for (i, text, working) in txs.into_iter().flatten() {
            let Some(h) = self.pile[i] else { continue };
            let delay_ms = if k == 0 { 300 + self.random(600) } else { 150 + self.random(350) };
            let tx = Tx { station: i, text, wpm: h.wpm, fist: h.fist, pitch_offset_hz: h.pitch_offset_hz, delay_ms, working };
            if k < PILE_MAX {
                self.last[k] = Some(tx);
            }
            k += 1;
            out(Tx { wpm: self.pace(h.wpm), ..tx });
        }
    }

    fn pace(&self, wpm: u8) -> u8 {
        wpm.saturating_sub(self.qrs).clamp(MIN_WPM, MAX_WPM)
    }

    /// Adds up to `count` new hunters to free pileup slots; one slot without a pileup.
    fn spawn(&mut self, my_wpm: u8, count: usize) {
        let room = if self.options.pileup { PILE_MAX } else { 1 };
        for _ in 0..count.min(room.saturating_sub(self.pile_size())) {
            let Some(slot) = self.pile.iter().position(Option::is_none) else { return };
            let hunter = self.new_hunter(my_wpm);
            let taken = self.pile.iter().flatten().any(|h| h.call == hunter.call) || hunter.call == self.my_call;
            if !taken {
                self.pile[slot] = Some(hunter);
            }
        }
    }

    fn new_hunter(&mut self, my_wpm: u8) -> Hunter {
        let home = home(&mut self.rng);
        let call = home.call(&mut self.rng);
        let state = home.region(&mut self.rng);
        let report = REPORTS[self.random(REPORTS.len() as u32) as usize];
        let habits = Habits::new(&mut self.rng);
        let (wpm, fist) = habits.keying(self.options, my_wpm);
        Hunter {
            call,
            state,
            report,
            habits,
            wpm,
            fist,
            pitch_offset_hz: (self.random(41) as i16 - 20) * 10,
        }
    }

    /// Pseudo-random number in 0..n.
    fn random(&mut self, n: u32) -> u32 {
        self.rng.below(n)
    }
}

/// A station's own offset from the base speed and its timing jitter, picked once, so new options
/// change how it sends without making it a different station.
#[derive(Clone, Copy)]
pub(crate) struct Habits {
    spread: i8,
    fist: u8,
}

impl Habits {
    /// Offsets spread evenly 5 WPM either side of the base speed.
    pub(crate) fn new(rng: &mut Rng) -> Habits {
        Habits { spread: rng.below(11) as i8 - 5, fist: rng.below(13) as u8 }
    }

    /// Speed and fist under `options`.
    pub(crate) fn keying(self, options: Options, my_wpm: u8) -> (u8, u8) {
        let base = if options.wpm == 0 { my_wpm } else { options.wpm };
        let spread = if options.spread { self.spread as i32 } else { 0 };
        let wpm = (base as i32 + spread).clamp(MIN_WPM as i32, MAX_WPM as i32) as u8;
        (wpm, if options.human_fist { self.fist } else { 0 })
    }
}

/// Second letters of a mainland two-letter prefix: H, L and P mark Hawaii and the Pacific,
/// Alaska, and the Caribbean.
const MAINLAND_SECOND: &[u8] = b"ABCDEFGIJKMNOQRSTUVWXYZ";
/// A-prefixes run AA to AL, and AL is Alaska's.
const A_SECOND: &[u8] = b"ABCDEFGHIJK";
/// W-prefix second letters the FCC leaves out of 2x3 calls.
const W_2X3_SECOND: &[u8] = b"ABDEFGIJNOQSUVWXYZ";
/// First suffix letters of a 2x3 call: never X.
const SUFFIX_2X3_FIRST: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWYZ";

/// The over's last word hands the turn back: K, BK or KN, as letters or as a prosign.
pub fn ends_with_turn(over: &str) -> bool {
    matches!(over.split_whitespace().last(), Some("K" | "BK" | "KN" | "<BK>" | "<KN>"))
}

/// A random US call in call area `digit`, in a form the FCC sequential system issues to the
/// mainland: 1x2, 1x3, 2x1 and 2x2 from K, N, W or AA..AK; 2x3 from K or W only.
pub fn us_call(digit: u8, rng: &mut Rng) -> Text<CALL_LEN> {
    let mut pick = |letters: &[u8]| letters[rng.below(letters.len() as u32) as usize];
    let mut call = Text::new();
    // Prefix by suffix lengths: 1x2, 1x3, 2x1, 2x2, 2x3, weighted toward the common ones.
    let (prefix_len, suffix_len) = [(1, 2), (1, 3), (1, 3), (2, 1), (2, 2), (2, 2), (2, 3), (2, 3), (2, 3)][pick(b"012345678") as usize - b'0' as usize];
    let first = match (prefix_len, suffix_len) {
        (1, _) => pick(b"KNW"),
        (_, 3) => pick(b"KW"),
        _ => pick(b"KNWA"),
    };
    call.push(first);
    if prefix_len == 2 {
        call.push(match (first, suffix_len) {
            (b'A', _) => pick(A_SECOND),
            (b'W', 3) => pick(W_2X3_SECOND),
            _ => pick(MAINLAND_SECOND),
        });
    }
    call.push(b'0' + digit);
    for k in 0..suffix_len {
        let two_by_three = prefix_len == 2 && suffix_len == 3;
        call.push(pick(if k == 0 && two_by_three { SUFFIX_2X3_FIRST } else { b"ABCDEFGHIJKLMNOPQRSTUVWXYZ" }));
    }
    call
}

/// Words that come around a report or state in a closing over.
const FILLER: [&str; 12] = ["TU", "TNX", "R", "72", "73", "E", "K", "BK", "<AR>", "<BK>", "<KN>", "<SK>"];

/// Whether `target` was sent as its own word, split over several words (C A), or run into
/// filler words (TUCA72).
fn sent_word(words: &[&str], target: &str) -> bool {
    let split = (0..words.len()).any(|start| {
        let mut rest = target;
        for w in &words[start..] {
            match rest.strip_prefix(w) {
                Some("") => return true,
                Some(r) => rest = r,
                None => return false,
            }
        }
        false
    });
    split
        || words.iter().any(|w| {
            (0..=w.len()).filter(|&at| w.is_char_boundary(at)).any(|at| {
                w[at..].starts_with(target) && made_of(&w[..at], &FILLER) && made_of(&w[at + target.len()..], &FILLER)
            })
        })
}

/// Whether `s` is a run of `tokens` with nothing between them.
fn made_of(s: &str, tokens: &[&str]) -> bool {
    s.is_empty() || tokens.iter().any(|t| s.strip_prefix(t).is_some_and(|rest| made_of(rest, tokens)))
}

/// `s` without the first `part` in it.
pub(crate) fn cut(s: &str, part: &str) -> Text<MAX_TEXT> {
    let mut out = Text::new();
    let (before, after) = match crate::text::find(s, part).filter(|_| !part.is_empty()) {
        Some(at) => (&s[..at], &s[at + part.len()..]),
        None => (s, ""),
    };
    let _ = write!(out, "{before}{after}");
    out
}

/// Formats text with its spaces removed.
pub(crate) struct Squashed<'a>(pub(crate) &'a str);

impl core::fmt::Display for Squashed<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        self.0.split_ascii_whitespace().try_for_each(|w| f.write_str(w))
    }
}

pub(crate) fn text_of(s: &str) -> Text<MAX_TEXT> {
    Text::format(format_args!("{s}"))
}

/// The parts that are not empty, joined by spaces.
pub(crate) fn phrase(parts: &[&str]) -> Text<MAX_TEXT> {
    let mut text = Text::new();
    for part in parts.iter().filter(|part| !part.is_empty()) {
        if !text.is_empty() {
            let _ = text.write_str(" ");
        }
        let _ = text.write_str(part);
    }
    text
}

/// The time of day an operator greets with.
pub(crate) const GREETINGS: [&str; 3] = ["GM", "GA", "GE"];

/// Reports hunters give, weighted toward the usual 5NN.
pub(crate) const REPORTS: [&str; 8] = ["5NN", "5NN", "5NN", "599", "579", "559", "449", "339"];

/// US states by call area digit.
pub(crate) const STATES_BY_AREA: [&[&str]; 10] = [
    &["CO", "KS", "NE", "SD", "ND", "IA", "MN", "MO"],
    &["ME", "NH", "VT", "MA", "RI", "CT"],
    &["NY", "NJ"],
    &["PA", "DE", "MD"],
    &["VA", "NC", "SC", "GA", "FL", "AL", "TN", "KY"],
    &["TX", "OK", "AR", "LA", "MS", "NM"],
    &["CA"],
    &["WA", "OR", "ID", "MT", "WY", "NV", "UT", "AZ"],
    &["MI", "OH", "WV"],
    &["WI", "IL", "IN"],
];

/// Where a station is: a US call area, or a place abroad.
#[derive(Clone, Copy)]
pub(crate) enum Home {
    Us(u8),
    Abroad(&'static Abroad),
}

impl Home {
    pub(crate) fn call(self, rng: &mut Rng) -> Text<CALL_LEN> {
        match self {
            Home::Us(digit) => us_call(digit, rng),
            Home::Abroad(abroad) => abroad.call(rng),
        }
    }

    /// The state or province sent in an exchange; empty for DX.
    pub(crate) fn region(self, rng: &mut Rng) -> &'static str {
        match self {
            Home::Us(digit) => {
                let states = STATES_BY_AREA[digit as usize];
                states[rng.below(states.len() as u32) as usize]
            }
            Home::Abroad(abroad) => abroad.region,
        }
    }
}

/// A random home: 8 in 10 in the US, 1 in 10 in Canada, and 1 in 10 DX.
pub(crate) fn home(rng: &mut Rng) -> Home {
    match rng.below(10) {
        0 => Home::Abroad(&CANADA[rng.below(CANADA.len() as u32) as usize]),
        1 => Home::Abroad(&DX[rng.below(DX.len() as u32) as usize]),
        _ => Home::Us(rng.below(10) as u8),
    }
}

/// A place outside the US: its call prefixes, the call area digits they take, the province it
/// sends in an exchange (empty outside Canada), and cities and a country (empty in Canada) for a
/// ragchew QTH.
pub(crate) struct Abroad {
    prefixes: &'static [&'static str],
    digits: &'static [u8],
    pub(crate) region: &'static str,
    pub(crate) cities: &'static [&'static str],
    pub(crate) country: &'static str,
}

impl Abroad {
    /// A call of a two-letter prefix has a suffix of two or three letters; of a one-letter
    /// prefix, three.
    fn call(&self, rng: &mut Rng) -> Text<CALL_LEN> {
        let mut pick = |n: usize| rng.below(n as u32) as usize;
        let prefix = self.prefixes[pick(self.prefixes.len())];
        let digit = self.digits[pick(self.digits.len())];
        let suffix_len = if prefix.len() == 2 && pick(3) == 0 { 2 } else { 3 };
        let mut call = Text::new();
        let _ = write!(call, "{prefix}{digit}");
        for _ in 0..suffix_len {
            call.push(b'A' + pick(26) as u8);
        }
        call
    }
}

pub(crate) const CANADA: [Abroad; 8] = [
    Abroad { prefixes: &["VE", "VA"], digits: &[1], region: "NS", cities: &["HALIFAX"], country: "" },
    Abroad { prefixes: &["VE", "VA"], digits: &[2], region: "QC", cities: &["MONTREAL", "QUEBEC"], country: "" },
    Abroad { prefixes: &["VE", "VA"], digits: &[3], region: "ON", cities: &["TORONTO", "OTTAWA"], country: "" },
    Abroad { prefixes: &["VE", "VA"], digits: &[4], region: "MB", cities: &["WINNIPEG"], country: "" },
    Abroad { prefixes: &["VE", "VA"], digits: &[5], region: "SK", cities: &["REGINA", "SASKATOON"], country: "" },
    Abroad { prefixes: &["VE", "VA"], digits: &[6], region: "AB", cities: &["CALGARY", "EDMONTON"], country: "" },
    Abroad { prefixes: &["VE", "VA"], digits: &[7], region: "BC", cities: &["VANCOUVER", "VICTORIA"], country: "" },
    Abroad { prefixes: &["VE"], digits: &[9], region: "NB", cities: &["MONCTON"], country: "" },
];

pub(crate) const DX: [Abroad; 11] = [
    Abroad { prefixes: &["G", "M"], digits: &[0, 3, 4], region: "", cities: &["LONDON", "LEEDS", "BRISTOL", "MANCHESTER"], country: "ENGLAND" },
    Abroad { prefixes: &["DL", "DK", "DJ"], digits: &[1, 2, 3, 4, 5, 6, 7, 8, 9], region: "", cities: &["MUNICH", "BERLIN", "HAMBURG"], country: "GERMANY" },
    Abroad { prefixes: &["F"], digits: &[4, 5, 6, 8], region: "", cities: &["PARIS", "LYON"], country: "FRANCE" },
    Abroad { prefixes: &["I", "IK", "IZ"], digits: &[0, 1, 2, 3, 4, 5, 6, 7, 8], region: "", cities: &["ROME", "MILAN", "TURIN"], country: "ITALY" },
    Abroad { prefixes: &["EA"], digits: &[1, 2, 3, 4, 5, 6, 7], region: "", cities: &["MADRID", "VALENCIA"], country: "SPAIN" },
    Abroad { prefixes: &["PA", "PD"], digits: &[0, 1, 2, 3], region: "", cities: &["AMSTERDAM", "UTRECHT"], country: "HOLLAND" },
    Abroad { prefixes: &["JA", "JH", "JR"], digits: &[1], region: "", cities: &["TOKYO"], country: "JAPAN" },
    Abroad { prefixes: &["JA", "JH", "JR"], digits: &[3], region: "", cities: &["OSAKA", "KYOTO"], country: "JAPAN" },
    Abroad { prefixes: &["VK"], digits: &[2], region: "", cities: &["SYDNEY"], country: "AUSTRALIA" },
    Abroad { prefixes: &["VK"], digits: &[3], region: "", cities: &["MELBOURNE"], country: "AUSTRALIA" },
    Abroad { prefixes: &["VK"], digits: &[6], region: "", cities: &["PERTH"], country: "AUSTRALIA" },
];

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::string::{String, ToString};
    use std::vec::Vec;

    fn heard(bot: &mut Bot, over: &str) -> Vec<Tx> {
        let mut txs = Vec::new();
        bot.heard(over, 20, &mut |tx| txs.push(tx));
        txs
    }

    fn texts(txs: &[Tx]) -> Vec<String> {
        txs.iter().map(|t| t.text.as_str().to_string()).collect()
    }

    #[test]
    fn cq_brings_a_pileup_of_valid_calls() {
        let mut bot = Bot::new(Program::Pota, "K5MME", 7, Options::default());
        let txs = heard(&mut bot, "CQ POTA DE K5MME K");
        assert!(!txs.is_empty() && txs.len() == bot.pile_size(), "{:?}", texts(&txs));
        for h in bot.pile() {
            let call = h.call.as_str();
            assert!((4..=6).contains(&call.len()), "{call}");
            assert!(call.bytes().any(|b| b.is_ascii_digit()), "{call}");
            let abroad = CANADA.iter().chain(&DX).any(|a| a.region == h.state);
            assert!(abroad || STATES_BY_AREA.iter().any(|s| s.contains(&h.state)), "{}", h.state);
        }
        assert!(txs.iter().all(|t| !t.working));
    }

    #[test]
    fn cq_without_my_call_gets_asked_for_it() {
        let mut bot = Bot::new(Program::Pota, "K5MME", 3, Options::default());
        assert_eq!(texts(&heard(&mut bot, "CQ POTA K")), ["CALL?"]);
    }

    #[test]
    fn full_contact_logs_and_checks_copy() {
        let mut bot = Bot::new(Program::Pota, "K5MME", 11, Options::default());
        heard(&mut bot, "CQ POTA DE K5MME K");
        let hunter = *bot.pile().next().unwrap();
        let call = hunter.call.as_str().to_string();

        let reply = heard(&mut bot, &std::format!("{call} 5NN TX"));
        assert_eq!(reply.len(), 1);
        assert!(reply[0].working);
        let text = reply[0].text.as_str();
        assert!(text.contains(hunter.state) && text.contains(hunter.report), "{text}");
        assert!(bot.working().is_some_and(|h| h.call == hunter.call));

        let before = bot.pile_size();
        heard(&mut bot, &std::format!("TU {} 72 E E", hunter.state));
        assert_eq!((bot.logged, bot.copied), (1, 1));
        assert_eq!(bot.exchange, Exchange { replied: false, state_copied: Some(true) });
        assert!(bot.working().is_none());
        assert!(bot.pile().all(|h| h.call != hunter.call));
        assert!(bot.pile_size() + 1 >= before);
    }

    #[test]
    fn closing_without_the_state_logs_but_does_not_count_as_copied() {
        let mut bot = Bot::new(Program::Sota, "K5MME", 5, Options::default());
        heard(&mut bot, "CQ SOTA DE K5MME K");
        let call = bot.pile().next().unwrap().call.as_str().to_string();
        heard(&mut bot, &std::format!("{call} 559"));
        heard(&mut bot, "TU 73");
        assert_eq!((bot.logged, bot.copied), (1, 0));
    }

    #[test]
    fn fragment_gets_the_matching_call_twice() {
        let mut bot = Bot::new(Program::Pota, "K5MME", 21, Options::default());
        heard(&mut bot, "CQ POTA DE K5MME K");
        let call = bot.pile().next().unwrap().call.as_str().to_string();
        let fragment = std::format!("{}?", &call[..3]);
        let txs = heard(&mut bot, &fragment);
        assert_eq!(texts(&txs), [std::format!("{call} {call}")]);
        assert!(bot.working().is_none());
    }

    #[test]
    fn calls_match_across_split_and_joined_words() {
        let mut bot = Bot::new(Program::Pota, "K5MME", 13, Options::default());
        heard(&mut bot, "CQ POTA DE K5 MME K");
        assert!(bot.pile_size() > 0, "a split call in the CQ still names me");
        let hunter = *bot.pile().next().unwrap();
        let call = hunter.call.as_str();
        let split = std::format!("{} {}5NN", &call[..2], &call[2..]);
        let reply = heard(&mut bot, &split);
        assert!(reply.len() == 1 && reply[0].working, "{split}: {:?}", texts(&reply));
        let state = hunter.state;
        let close = std::format!("TU{state}72 E E");
        heard(&mut bot, &close);
        assert_eq!((bot.logged, bot.copied), (1, 1), "{close}");
    }

    #[test]
    fn sent_word_tolerates_spacing_but_not_other_letters() {
        assert!(sent_word(&["TU", "C", "A", "72"], "CA"));
        assert!(sent_word(&["TUCA72"], "CA"));
        assert!(sent_word(&["TU", "CA<SK>"], "CA"));
        assert!(sent_word(&["RR", "CA"], "CA"));
        assert!(!sent_word(&["TU", "5NNE", "E"], "NE"));
        assert!(!sent_word(&["TU", "AZ", "72"], "CA"));
    }

    #[test]
    fn options_set_one_caller_and_even_clean_hunters() {
        let options = Options { wpm: 15, pileup: false, spread: false, human_fist: false };
        let mut bot = Bot::new(Program::Sota, "K5MME", 17, options);
        for _ in 0..5 {
            let txs = heard(&mut bot, "CQ SOTA DE K5MME K");
            assert!(bot.pile_size() == 1 && txs.len() == 1);
            assert!(txs.iter().all(|t| t.wpm == 15 && t.fist == 0));
        }
    }

    #[test]
    fn new_options_apply_to_the_hunters_already_calling() {
        let mut bot = Bot::new(Program::Pota, "K5MME", 23, Options::default());
        while bot.pile_size() < 2 {
            heard(&mut bot, "CQ POTA DE K5MME K");
        }
        let options = Options { wpm: 12, pileup: false, spread: false, human_fist: false };
        bot.set_options(options, 20);
        assert_eq!(bot.pile_size(), 1);
        let again = heard(&mut bot, "AGN");
        assert!(again.len() <= 1 && again.iter().all(|t| t.wpm == 12 && t.fist == 0), "{:?}", texts(&again));
    }

    #[test]
    fn k_bk_and_kn_end_an_over_only_as_its_last_word() {
        for over in ["CQ POTA DE K5MME K", "K1ABC 5NN BK ", "TU <BK>", "QRZ KN", "QRZ <KN>"] {
            assert!(ends_with_turn(over), "{over}");
        }
        for over in ["", "K5MME", "TU CA 72 E E", "K 5NN", "CQ POTA DE K5MME K5"] {
            assert!(!ends_with_turn(over), "{over}");
        }
    }

    #[test]
    fn agn_with_a_question_mark_asks_for_a_repeat() {
        let mut bot = Bot::new(Program::Pota, "K5MME", 23, Options::default());
        let first = heard(&mut bot, "CQ POTA DE K5MME K");
        assert_eq!(texts(&heard(&mut bot, "AGN?")), texts(&first));
    }

    #[test]
    fn a_dx_hunter_sends_no_state_and_counts_as_copied() {
        for seed in 0..1000 {
            let mut bot = Bot::new(Program::Pota, "K5MME", seed, Options::default());
            heard(&mut bot, "CQ POTA DE K5MME K");
            let Some(hunter) = bot.pile().find(|h| h.state.is_empty()).copied() else { continue };
            heard(&mut bot, &std::format!("{} 5NN", hunter.call.as_str()));
            heard(&mut bot, "TU 72 E E");
            assert_eq!((bot.logged, bot.copied), (1, 1));
            return;
        }
        panic!("no DX hunter called");
    }

    #[test]
    fn most_hunters_sign_off_before_they_go() {
        let mut signed = 0;
        for seed in 0..30 {
            let mut bot = Bot::new(Program::Pota, "K5MME", seed, Options::default());
            heard(&mut bot, "CQ POTA DE K5MME K");
            let hunter = *bot.pile().next().unwrap();
            heard(&mut bot, &std::format!("{} 5NN", hunter.call.as_str()));
            let reply = heard(&mut bot, &std::format!("TU {} 72 E E", hunter.state));
            signed += reply.iter().filter(|t| t.working && t.text.as_str().ends_with("E E")).count();
        }
        assert!((10..30).contains(&signed), "{signed}");
    }

    #[test]
    fn phrase_skips_empty_parts() {
        assert_eq!(phrase(&["", "TU", "", "73", ""]).as_str(), "TU 73");
    }

    #[test]
    fn calls_abroad_use_their_prefixes_and_digits() {
        let mut rng = Rng::new(1);
        for abroad in CANADA.iter().chain(&DX) {
            for _ in 0..200 {
                let call = abroad.call(&mut rng);
                let call = call.as_str();
                let digit = call.find(|c: char| c.is_ascii_digit()).unwrap();
                assert!(abroad.prefixes.contains(&&call[..digit]), "{call}");
                assert!(abroad.digits.contains(&(call.as_bytes()[digit] - b'0')), "{call}");
                let suffix = &call[digit + 1..];
                assert!((2..=3).contains(&suffix.len()) && suffix.bytes().all(|b| b.is_ascii_uppercase()), "{call}");
            }
        }
    }

    #[test]
    fn us_calls_follow_the_sequential_system() {
        let mut rng = Rng::new(1);
        for k in 0..10_000 {
            let call = us_call((k % 10) as u8, &mut rng);
            let call = call.as_str();
            let digit = call.find(|c: char| c.is_ascii_digit()).unwrap();
            let (prefix, suffix) = (&call[..digit], &call[digit + 1..]);
            let p = prefix.as_bytes();
            assert!(matches!(p[0], b'K' | b'N' | b'W' | b'A') && !(p[0] == b'A' && p.len() == 1), "{call}");
            if p.len() == 2 {
                assert!(!b"HLP".contains(&p[1]) || p[0] == b'A', "{call}");
                assert!(p[0] != b'A' || p[1] <= b'K', "{call}");
            }
            if p.len() == 2 && suffix.len() == 3 {
                assert!(matches!(p[0], b'K' | b'W'), "{call}");
                assert!(!(p[0] == b'W' && b"CKMRT".contains(&p[1])), "{call}");
                assert!(!suffix.starts_with('X'), "{call}");
            }
        }
    }

    #[test]
    fn a_call_after_the_closing_works_that_hunter() {
        let mut bot = Bot::new(Program::Pota, "K5MME", 23, Options::default());
        while bot.pile_size() < 2 {
            heard(&mut bot, "CQ POTA DE K5MME K");
        }
        let calls: Vec<String> = bot.pile().map(|h| h.call.as_str().to_string()).collect();
        let state = bot.pile().next().unwrap().state;
        heard(&mut bot, &std::format!("{} 5NN", calls[0]));
        let reply = heard(&mut bot, &std::format!("TU {state} 72 {} 5NN", calls[1]));
        assert_eq!((bot.logged, bot.copied), (1, 1));
        assert_eq!(bot.working().map(|h| h.call.as_str()), Some(calls[1].as_str()));
        assert!(reply.len() == 1 && reply[0].working, "{:?}", texts(&reply));
    }

    #[test]
    fn a_new_call_reaches_the_hunters() {
        let mut bot = Bot::new(Program::Pota, "K5MME", 3, Options::default());
        bot.set_call("W1AW");
        let txs = heard(&mut bot, "CQ POTA DE W1AW K");
        assert!(!txs.is_empty() && txs.iter().all(|t| t.text.as_str() != "CALL?"), "{:?}", texts(&txs));
    }

    #[test]
    fn qrs_applies_once_and_ends_with_the_contact() {
        let options = Options { wpm: 20, spread: false, ..Options::default() };
        let mut bot = Bot::new(Program::Pota, "K5MME", 9, options);
        heard(&mut bot, "CQ POTA DE K5MME K");
        let hunter = *bot.pile().next().unwrap();
        let slower = heard(&mut bot, "QRS");
        let exchange = heard(&mut bot, &std::format!("{} 5NN", hunter.call.as_str()));
        let again = heard(&mut bot, "AGN");
        let speeds = (slower[0].wpm, exchange[0].wpm, again[0].wpm);
        assert_eq!(speeds, (20 - QRS_STEP, 20 - QRS_STEP, 20 - QRS_STEP));
        heard(&mut bot, &std::format!("TU {} 72", hunter.state));
        let next = heard(&mut bot, "CQ POTA DE K5MME K");
        assert!(!next.is_empty() && next.iter().all(|t| t.wpm == 20), "{:?}", texts(&next));
    }

    #[test]
    fn the_speed_spread_is_even_about_the_base() {
        let options = Options { wpm: 20, ..Options::default() };
        let mut rng = Rng::new(5);
        let speeds: Vec<u8> = (0..2000).map(|_| Habits::new(&mut rng).keying(options, 20).0).collect();
        assert_eq!((speeds.iter().min(), speeds.iter().max()), (Some(&15), Some(&25)));
        let mean = speeds.iter().map(|&w| w as f32).sum::<f32>() / speeds.len() as f32;
        assert!((mean - 20.0).abs() < 0.3, "{mean}");
    }

    #[test]
    fn a_new_speed_moves_each_hunter_by_the_same_amount() {
        let mut bot = Bot::new(Program::Pota, "K5MME", 9, Options { wpm: 20, ..Options::default() });
        while bot.pile_size() < 3 {
            heard(&mut bot, "CQ POTA DE K5MME K");
        }
        heard(&mut bot, "QRS");
        let before: Vec<(u8, u8)> = bot.pile().map(|h| (h.wpm, h.fist)).collect();
        bot.set_options(Options { wpm: 30, ..Options::default() }, 20);
        let after: Vec<(u8, u8)> = bot.pile().map(|h| (h.wpm, h.fist)).collect();
        let moved: Vec<(u8, u8)> = before.iter().map(|&(wpm, fist)| (wpm + 10, fist)).collect();
        assert_eq!(after, moved);
        assert_eq!(bot.qrs, 0);
    }

    #[test]
    fn qrs_repeats_slower_and_agn_repeats() {
        let mut bot = Bot::new(Program::Pota, "K5MME", 9, Options::default());
        let first = heard(&mut bot, "CQ POTA DE K5MME K");
        let again = heard(&mut bot, "AGN");
        assert_eq!(texts(&first), texts(&again));
        let slower = heard(&mut bot, "QRS");
        for (a, b) in first.iter().zip(&slower) {
            assert_eq!(b.wpm, (a.wpm - QRS_STEP).max(MIN_WPM));
        }
    }
}
