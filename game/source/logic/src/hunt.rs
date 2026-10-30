//! Hunt bot: each activator is on a park (POTA) or a summit (SOTA) and you hunt it. It calls CQ, you
//! send your call, it sends your report and its state, you close, and then you log it by keying
//! its call. Send P2P with your call for a park-to-park contact.

use core::fmt::Write;

use crate::player::MAX_TEXT;
use crate::qso::{
    CALL_LEN, GREETINGS, MAX_WPM, MIN_WPM, Options, Out, Program, QRS_STEP, REPORTS, Squashed, Tx, cut, Habits, home,
    phrase, text_of,
};
use crate::rng::Rng;
use crate::text::Text;

#[derive(Clone, Copy)]
pub struct Activator {
    pub program: Program,
    /// With /P on a summit.
    pub call: Text<CALL_LEN>,
    /// Empty for DX.
    pub state: &'static str,
    /// The report the activator gives you.
    pub report: &'static str,
    /// GM, GA or GE.
    greeting: &'static str,
    habits: Habits,
    wpm: u8,
    fist: u8,
    pitch_offset_hz: i16,
}

impl Activator {
    /// The call without /P, which is what you log.
    pub fn base_call(&self) -> &str {
        crate::text::trim_end(self.call.as_str(), "/P")
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// Calling CQ; send your call.
    Calling,
    /// The activator sent its exchange to `heard`, which is your call unless it copied you
    /// wrong.
    Working { heard: Text<CALL_LEN>, p2p: bool },
    /// The activator said goodbye; key its call to log the contact.
    Closed { busted: bool, p2p: bool },
}

/// A contact you logged.
#[derive(Clone, Copy)]
pub struct Logged {
    pub activator: Activator,
    /// You keyed the activator's call right.
    pub copied: bool,
    /// The activator logged your call wrong.
    pub busted: bool,
    pub p2p: bool,
}

pub struct Hunt {
    options: Options,
    my_call: Text<CALL_LEN>,
    rng: Rng,
    pub activator: Activator,
    pub phase: Phase,
    pub logged: u8,
    /// Contacts logged with the activator's call right and your call not busted.
    pub good: u8,
    /// CQs sent by this activator, so later ones can be shorter.
    cqs: u8,
    last: Option<Tx>,
    qrs: u8,
}

/// Words a hunter's closing over holds.
const CLOSING: [&str; 7] = ["TU", "72", "73", "5NN", "599", "TNX", "RR"];

impl Hunt {
    pub fn new(my_call: &str, seed: u32, options: Options, my_wpm: u8) -> Hunt {
        let call = Text::format(format_args!("{my_call}"));
        let mut rng = Rng::new(seed);
        let activator = new_activator(options, my_wpm, &mut rng);
        Hunt {
            options,
            my_call: call,
            rng,
            activator,
            phase: Phase::Calling,
            logged: 0,
            good: 0,
            cqs: 0,
            last: None,
            qrs: 0,
        }
    }

    pub fn my_call(&self) -> &str {
        self.my_call.as_str()
    }

    /// Your call changed: the activator listens for the new one.
    pub fn set_call(&mut self, my_call: &str) {
        self.my_call = Text::format(format_args!("{my_call}"));
    }

    /// Your over ended. After the activator's goodbye, an over holding a call logs the contact;
    /// anything else goes to the activator as `heard`.
    pub fn over(&mut self, over: &str, my_wpm: u8, out: Out) -> Option<Logged> {
        let names_a_call = over.split_ascii_whitespace().any(|w| call_like(w) && !REPORTS.contains(&w));
        if matches!(self.phase, Phase::Closed { .. }) && names_a_call {
            return self.log(over, my_wpm);
        }
        self.heard(over.trim(), out);
        None
    }

    /// The activator's CQ, sent while nobody answers.
    pub fn cq(&mut self, out: Out) {
        let mut text = Text::new();
        let (name, call) = (self.activator.program.name(), self.activator.call.as_str());
        let _ = match (self.cqs, self.rng.below(2)) {
            (0, 0) => write!(text, "CQ {name} CQ {name} DE {call} {call} K"),
            (0, _) | (_, 0) => write!(text, "CQ {name} DE {call} {call} K"),
            _ => write!(text, "QRZ {name} DE {call} K"),
        };
        self.cqs = self.cqs.saturating_add(1);
        self.send(out, text);
    }

    /// Answers one over you sent while hunting.
    pub fn heard(&mut self, over: &str, out: Out) {
        let mut words: [&str; 16] = [""; 16];
        let mut n = 0;
        for w in over.split_ascii_whitespace().take(16) {
            words[n] = w;
            n += 1;
        }
        let words = &words[..n];
        let has = |w: &str| words.contains(&w);
        let squashed = Text::<MAX_TEXT>::format(format_args!("{}", Squashed(over)));
        let me = self.my_call.as_str();
        let names_me = !me.is_empty() && crate::text::has(squashed.as_str(), me);
        let others = cut(cut(squashed.as_str(), me).as_str(), self.activator.base_call());
        let p2p = crate::text::has(others.as_str(), "P2P");

        if has("QRS") {
            self.qrs = self.qrs.saturating_add(QRS_STEP);
            return self.repeat(out);
        }
        if has("AGN") || has("AGN?") || words == ["?"] {
            return self.repeat(out);
        }

        match self.phase {
            Phase::Calling | Phase::Closed { .. } => {
                if let Some(heard) = self.heard_call(words, names_me) {
                    return self.send_exchange(heard, p2p, out);
                }
                match self.fragment(words) {
                    Some(fragment) => {
                        let text = Text::format(format_args!("{fragment}?"));
                        self.send(out, text)
                    }
                    None => self.send(out, text_of("QRZ?")),
                }
            }
            Phase::Working { heard, p2p: was_p2p } => {
                let p2p = was_p2p || p2p;
                if names_me && (heard.as_str() != me || !closes(others.as_str())) {
                    return self.send_exchange(self.my_call, p2p, out);
                }
                if closes(others.as_str()) {
                    self.phase = Phase::Closed { busted: heard.as_str() != me, p2p };
                    let bye = self.activator.program.bye();
                    let p2p = if p2p { "P2P" } else { "" };
                    let text = match self.rng.below(3) {
                        0 => phrase(&["RR", "TU", p2p, bye, "E", "E"]),
                        1 => phrase(&["TU", p2p, bye, "E", "E", "QRZ"]),
                        _ => phrase(&["TU", p2p, bye, "E", "E"]),
                    };
                    return self.send(out, text);
                }
                self.send(out, text_of("AGN?"))
            }
        }
    }

    /// Logs the contact from the call you keyed; the next activator starts calling.
    pub fn log(&mut self, copy: &str, my_wpm: u8) -> Option<Logged> {
        let Phase::Closed { busted, p2p } = self.phase else { return None };
        let squashed = Text::<MAX_TEXT>::format(format_args!("{}", Squashed(copy)));
        let copied = crate::text::has(squashed.as_str(), self.activator.base_call());
        let logged = Logged { activator: self.activator, copied, busted, p2p };
        self.logged = self.logged.saturating_add(1);
        self.good = self.good.saturating_add((copied && !busted) as u8);
        self.activator = new_activator(self.options, my_wpm, &mut self.rng);
        self.phase = Phase::Calling;
        self.cqs = 0;
        self.qrs = 0;
        self.last = None;
        Some(logged)
    }

    /// Sends the last transmission again, at the current QRS speed.
    pub fn repeat(&mut self, out: Out) {
        if let Some(mut tx) = self.last {
            tx.wpm = self.pace(self.activator.wpm);
            out(tx);
        }
    }

    /// Applies new options to the activator already calling, which keeps its habits and sends
    /// at its own speed again, without QRS.
    pub fn set_options(&mut self, options: Options, my_wpm: u8) {
        self.options = options;
        self.qrs = 0;
        (self.activator.wpm, self.activator.fist) = self.activator.habits.keying(options, my_wpm);
    }

    /// The call the activator copies from your over: yours when it is there, else a call-like
    /// word one character off yours.
    fn heard_call(&self, words: &[&str], names_me: bool) -> Option<Text<CALL_LEN>> {
        let me = self.my_call.as_str();
        if names_me {
            return Some(self.my_call);
        }
        // Without your call set, any call works.
        let heard = words.iter().find(|w| {
            call_like(w)
                && (me.is_empty()
                    || w.len() == me.len() && w.bytes().zip(me.bytes()).filter(|(a, b)| a != b).count() == 1)
        })?;
        let call = Text::format(format_args!("{heard}"));
        Some(call)
    }

    /// A piece of your call, or a word sent with '?'.
    fn fragment<'a>(&self, words: &[&'a str]) -> Option<&'a str> {
        let me = self.my_call.as_str();
        words
            .iter()
            .map(|w| w.trim_end_matches('?'))
            .filter(|w| w.len() >= 2 && !["CQ", "DE", "K", "BK", "P2P"].contains(w))
            .find(|w| crate::text::has(me, w) || w.bytes().any(|b| b.is_ascii_digit()))
    }

    fn send_exchange(&mut self, heard: Text<CALL_LEN>, p2p: bool, out: Out) {
        self.phase = Phase::Working { heard, p2p };
        let a = self.activator;
        let (call, rst, st, g) = (heard.as_str(), a.report, a.state, a.greeting);
        let p2p = if p2p { "P2P" } else { "" };
        let text = match (a.program, self.rng.below(4)) {
            (Program::Pota, 0) => phrase(&[call, p2p, "TU", rst, st, st, "BK"]),
            (Program::Pota, 1) => phrase(&[call, p2p, "UR", rst, rst, st, st, "BK"]),
            (Program::Pota, 2) => phrase(&[call, p2p, g, "UR", rst, st, "BK"]),
            (Program::Pota, _) => phrase(&[call, p2p, "TU", g, "UR", rst, rst, st, st, "BK"]),
            (Program::Sota, 0 | 1) => phrase(&[call, p2p, "UR", rst, rst, st, st, "BK"]),
            (Program::Sota, _) => phrase(&[call, p2p, "TU", rst, st, "BK"]),
        };
        self.send(out, text);
    }

    fn send(&mut self, out: Out, text: Text<MAX_TEXT>) {
        let a = self.activator;
        let tx = Tx {
            station: 0,
            text,
            wpm: self.pace(a.wpm),
            fist: a.fist,
            pitch_offset_hz: a.pitch_offset_hz,
            delay_ms: 300 + self.rng.below(600),
            working: true,
        };
        self.last = Some(tx);
        out(tx);
    }

    fn pace(&self, wpm: u8) -> u8 {
        wpm.saturating_sub(self.qrs).clamp(MIN_WPM, MAX_WPM)
    }
}

/// A closing over: `others` is the squashed over without your call or the activator's.
fn closes(others: &str) -> bool {
    CLOSING.iter().any(|w| crate::text::has(others, w))
}

/// Letters and at least one digit, at least three characters: the shape of a call.
fn call_like(word: &str) -> bool {
    word.len() >= 3
        && word.len() <= CALL_LEN
        && word.bytes().any(|b| b.is_ascii_digit())
        && word.bytes().any(|b| b.is_ascii_alphabetic())
        && word.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'/')
}

/// Parks are about twice as common on the air as summits.
fn new_activator(options: Options, my_wpm: u8, rng: &mut Rng) -> Activator {
    let program = if rng.below(3) == 0 { Program::Sota } else { Program::Pota };
    let home = home(rng);
    let mut call = home.call(rng);
    if program == Program::Sota {
        let _ = write!(call, "/P");
    }
    let state = home.region(rng);
    let report = REPORTS[rng.below(REPORTS.len() as u32) as usize];
    let greeting = GREETINGS[rng.below(GREETINGS.len() as u32) as usize];
    let habits = Habits::new(rng);
    let (wpm, fist) = habits.keying(options, my_wpm);
    Activator { program, call, state, report, greeting, habits, wpm, fist, pitch_offset_hz: (rng.below(41) as i16 - 20) * 10 }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::string::{String, ToString};
    use std::vec::Vec;

    fn heard(hunt: &mut Hunt, over: &str) -> Vec<String> {
        let mut texts = Vec::new();
        hunt.heard(over, &mut |tx| texts.push(tx.text.as_str().to_string()));
        texts
    }

    fn cq(hunt: &mut Hunt) -> String {
        let mut text = String::new();
        hunt.cq(&mut |tx| text = tx.text.as_str().to_string());
        text
    }

    #[test]
    fn full_hunt_logs_and_checks_the_call() {
        let mut hunt = Hunt::new("K5MME", 7, Options::default(), 20);
        let a = hunt.activator;
        assert!(cq(&mut hunt).contains(a.call.as_str()));
        let exchange = heard(&mut hunt, "K5MME");
        assert!(exchange[0].starts_with("K5MME") && exchange[0].contains(a.state), "{exchange:?}");
        let bye = if a.program == Program::Pota { "72 E E" } else { "73 E E" };
        let closing = heard(&mut hunt, "TU 5NN TX 72");
        assert!(closing.len() == 1 && closing[0].contains(bye), "{closing:?}");
        let logged = hunt.log(&std::format!("{} US", a.call.as_str()), 20).unwrap();
        assert!(logged.copied && !logged.busted && !logged.p2p);
        assert_eq!((hunt.logged, hunt.good, hunt.phase), (1, 1, Phase::Calling));
        assert!(hunt.activator.call != a.call || hunt.activator.state != a.state);
    }

    #[test]
    fn a_full_length_callsign_gets_worked() {
        let mut hunt = Hunt::new("KD9XYZ/QRP", 7, Options::default(), 20);
        assert_eq!(hunt.my_call(), "KD9XYZ/QRP");
        cq(&mut hunt);
        let exchange = heard(&mut hunt, "KD9XYZ/QRP");
        assert!(exchange[0].starts_with("KD9XYZ/QRP"), "{exchange:?}");
    }

    #[test]
    fn wrong_log_copy_does_not_count() {
        let mut hunt = Hunt::new("K5MME", 3, Options::default(), 20);
        let a = hunt.activator;
        assert_eq!(a.call.as_str().ends_with("/P"), a.program == Program::Sota);
        heard(&mut hunt, "K5 MME K5MME");
        heard(&mut hunt, "R TU 579 TX 73");
        let logged = hunt.log("W1AW", 20).unwrap();
        assert!(!logged.copied);
        assert_eq!((hunt.logged, hunt.good), (1, 0));
    }

    #[test]
    fn busted_call_gets_worked_until_corrected() {
        let mut hunt = Hunt::new("K5MME", 11, Options::default(), 20);
        let exchange = heard(&mut hunt, "K5MMI");
        assert!(exchange[0].starts_with("K5MMI"), "{exchange:?}");
        let corrected = heard(&mut hunt, "K5MME K5MME");
        assert!(corrected[0].starts_with("K5MME"), "{corrected:?}");
        heard(&mut hunt, "TU 5NN TX 72");
        assert_eq!(hunt.phase, Phase::Closed { busted: false, p2p: false });
    }

    #[test]
    fn closing_on_a_busted_call_logs_busted() {
        let mut hunt = Hunt::new("K5MME", 13, Options::default(), 20);
        heard(&mut hunt, "K5MMI");
        heard(&mut hunt, "TU 5NN TX 72");
        assert!(matches!(hunt.phase, Phase::Closed { busted: true, .. }));
    }

    #[test]
    fn fragments_and_noise_get_asked_again() {
        let mut hunt = Hunt::new("K5MME", 5, Options::default(), 20);
        assert_eq!(heard(&mut hunt, "K5M"), ["K5M?"]);
        assert_eq!(heard(&mut hunt, "EEE"), ["QRZ?"]);
        assert_eq!(heard(&mut hunt, "?"), ["QRZ?"]);
    }

    #[test]
    fn p2p_is_acknowledged() {
        let mut hunt = Hunt::new("K5MME", 9, Options::default(), 20);
        let exchange = heard(&mut hunt, "P2P K5MME");
        assert!(exchange[0].starts_with("K5MME P2P"), "{exchange:?}");
        let closing = heard(&mut hunt, "TU 5NN TX US1234 72");
        assert!(closing[0].contains("TU P2P 7"), "{closing:?}");
        assert!(hunt.log(hunt.activator.base_call().to_string().as_str(), 20).unwrap().p2p);
    }

    #[test]
    fn only_an_over_with_a_call_logs() {
        let mut hunt = Hunt::new("K5MME", 7, Options::default(), 20);
        heard(&mut hunt, "K5MME");
        heard(&mut hunt, "TU 5NN TX 72");
        for over in ["E E", "TU", "AGN?", "5NN"] {
            assert!(hunt.over(over, 20, &mut |_| {}).is_none(), "{over}");
        }
        let call = hunt.activator.base_call().to_string();
        assert!(hunt.over(&call, 20, &mut |_| {}).is_some_and(|l| l.copied));
    }

    #[test]
    fn a_new_call_reaches_the_activator() {
        let mut hunt = Hunt::new("K5MME", 7, Options::default(), 20);
        hunt.set_call("W1AW");
        heard(&mut hunt, "W1AW");
        assert_eq!(hunt.phase, Phase::Working { heard: Text::format(format_args!("W1AW")), p2p: false });
    }

    #[test]
    fn counts_stop_at_their_top() {
        let mut hunt = Hunt::new("K5MME", 7, Options::default(), 20);
        for _ in 0..300 {
            heard(&mut hunt, "K5MME");
            heard(&mut hunt, "TU 5NN TX 72");
            let call = hunt.activator.base_call().to_string();
            hunt.log(&call, 20);
        }
        assert_eq!((hunt.logged, hunt.good), (u8::MAX, u8::MAX));
    }

    #[test]
    fn qrs_slows_the_repeat() {
        let options = Options { wpm: 20, spread: false, human_fist: false, pileup: true };
        let mut hunt = Hunt::new("K5MME", 1, options, 20);
        let mut wpm = 0;
        hunt.cq(&mut |_| {});
        hunt.heard("QRS", &mut |tx| wpm = tx.wpm);
        assert_eq!(wpm, 20 - QRS_STEP);
    }
}
