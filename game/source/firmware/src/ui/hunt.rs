//! Hunt screen: an activator calls CQ from a park or summit, you work it, then log it by keying
//! its call. What the activator sends stays hidden until you log it; HINT shows it.

use core::fmt::Write;

use pocketcw_logic::{
    decoder::{Decoded, copy_end},
    hunt::{Hunt, Logged, Phase},
    input::Action,
    player::{Keying, MAX_TEXT},
    qso::{Options, Program, Tx, ends_with_turn},
    settings::Settings,
};

use super::exchange::{self, Chip, RULE_Y};
use super::transcript::{self, Entry};
use super::{Play, Screen, Session, Ui, push_scrolling, settings::Section};
use crate::gfx::rect;
use crate::lcd::Lcd;
use crate::text::Text;
use crate::theme::{BAD, GOOD, MAIN};

const LINES: usize = 7;
const LINE_X: i32 = 60;
/// Silence after a CQ before the activator calls again.
const CQ_GAP_MS: u32 = 3500;
/// Silence after you log a contact before the next activator calls.
const NEXT_MS: u32 = 1500;

#[derive(Clone, Copy)]
struct Line {
    yours: bool,
    text: Text<MAX_TEXT>,
    hidden: bool,
}

pub(super) struct Hunting {
    pub(super) bot: Hunt,
    /// Your over so far, as decoded.
    over: Text<MAX_TEXT>,
    /// Tick time at which your over ends, while you are sending one.
    pub(super) over_deadline: Option<u32>,
    /// Tick time at which the activator calls CQ again.
    pub(super) cq_deadline: Option<u32>,
    queued: Option<Tx>,
    playing: bool,
    lines: [Option<Line>; LINES],
    hint: bool,
    last: Option<Logged>,
}

impl Hunting {
    pub(super) fn badge(&self) -> &'static str {
        exchange::badge(self.playing || self.queued.is_some(), self.over.as_str())
    }

    pub(super) fn new(my_call: &str, seed: u32, options: Options, my_wpm: u8) -> Hunting {
        Hunting {
            bot: Hunt::new(my_call, seed, options, my_wpm),
            over: Text::new(),
            over_deadline: None,
            cq_deadline: None,
            queued: None,
            playing: false,
            lines: [None; LINES],
            hint: false,
            last: None,
        }
    }

    pub(super) fn pause(&mut self) {
        self.queued = None;
        self.playing = false;
        self.over.clear();
        self.over_deadline = None;
        self.cq_deadline = None;
    }

    fn queue(&mut self, tx: Tx) {
        self.queued = Some(tx);
        push_scrolling(&mut self.lines, Line { yours: false, text: tx.text, hidden: true });
    }

    /// What to send next; with HINT on, the exact over.
    fn next_step(&self) -> Text<40> {
        let bot = &self.bot;
        let a = &bot.activator;
        let mut step = Text::new();
        let _ = match bot.phase {
            _ if !self.over.is_empty() => write!(step, "END YOUR OVER WITH K, OR PAUSE"),
            Phase::Calling if self.hint => write!(step, "SEND: {}  OR  P2P {0}", bot.my_call()),
            Phase::Calling => write!(step, "SEND YOUR CALL"),
            Phase::Working { heard, .. } if self.hint && heard.as_str() != bot.my_call() => {
                write!(step, "THEY HEARD {}: SEND CALL AGAIN", heard.as_str())
            }
            Phase::Working { .. } => {
                write!(step, "SEND: TU 5NN, YOUR STATE, {}", a.program.bye())
            }
            Phase::Closed { .. } if self.hint => write!(step, "TO LOG, KEY {}", a.base_call()),
            Phase::Closed { .. } => write!(step, "TO LOG, KEY THEIR CALL"),
        };
        step
    }
}

impl Ui {
    pub(super) fn draw_hunt(&mut self, lcd: &mut Lcd, s: &Settings) {
        let t = &self.theme;
        self.header(lcd, s, "HUNT", &[]);
        let Some(Session::Hunt(h)) = &self.session else { return };
        self.softkeys(lcd, exchange::keys(h.hint));
        rect(lcd, 0, RULE_Y, MAIN, 1, t.rule());
        self.draw_hunt_lines(lcd);
        self.draw_hunt_state(lcd);
        self.draw_hunt_over(lcd);
    }

    fn draw_hunt_lines(&self, lcd: &mut Lcd) {
        let Some(Session::Hunt(h)) = &self.session else { return };
        let t = &self.theme;
        let mut entries: [Option<Entry>; LINES] = Default::default();
        for (entry, line) in entries.iter_mut().zip(&h.lines) {
            let Some(line) = line else { continue };
            let (who, who_color, color) = if line.yours { ("YOU", t.muted, t.fg) } else { ("THEM", t.echo, t.accent) };
            *entry = Some(Entry { who, who_color, color, text: line.text.as_str(), hidden: line.hidden && !h.hint });
        }
        transcript::draw(lcd, t, &entries, LINE_X, transcript::ROWS);
    }

    /// The log count and the chips for the activator you are working or the last one logged.
    fn draw_hunt_state(&self, lcd: &mut Lcd) {
        let Some(Session::Hunt(h)) = &self.session else { return };
        let t = &self.theme;
        let bot = &h.bot;
        let calling = bot.phase == Phase::Calling;
        let shown = if calling { h.last } else { None };
        let log = Text::<24>::format(format_args!("HUNTED {}  GOOD {}", bot.logged, bot.good));
        exchange::draw_now(lcd, t, if shown.is_some() { "LAST" } else { "NOW" }, log.as_str(), t.muted);

        let reveal = shown.is_some() || h.hint;
        let a = shown.map_or(bot.activator, |l| l.activator);
        // The call chip's label says what its color does: whether the contact counts.
        let (call_label, mark) = match shown {
            Some(l) if l.busted => ("BUSTED", Some(BAD)),
            Some(l) if l.copied => ("COPIED", Some(GOOD)),
            Some(_) => ("MISSED", Some(BAD)),
            None => ("CALL", None),
        };
        let place = match (shown.map(|l| l.p2p), a.program) {
            (Some(true), _) => "P2P",
            (_, Program::Pota) => "PARK",
            (_, Program::Sota) => "SUMMIT",
        };
        let chips = [
            Chip { label: call_label, value: if reveal { a.call.as_str() } else { "" }, mark },
            Chip { label: "RST", value: if reveal { a.report } else { "" }, mark: None },
            Chip { label: "STATE", value: if reveal { exchange::region(a.state) } else { "" }, mark: None },
            Chip { label: "FROM", value: if reveal { place } else { "" }, mark: None },
        ];
        exchange::draw_chips(lcd, t, chips);
    }

    fn draw_hunt_over(&mut self, lcd: &mut Lcd) {
        let Some(Session::Hunt(h)) = &self.session else { return };
        exchange::draw_over(lcd, &self.theme, h.over.as_str());
        let next_step = h.next_step();
        self.set_footer(lcd, next_step.as_str());
    }

    pub(super) fn open_hunt(&mut self, lcd: &mut Lcd, s: &Settings) {
        let seed = self.next_seed();
        self.session = Some(Session::Hunt(Hunting::new(s.callsign(), seed, Self::qso_options(s), s.wpm)));
        self.open(lcd, s, Screen::Hunt);
        self.hunt_cq();
        self.draw_hunt_lines(lcd);
    }

    fn hunt_cq(&mut self) {
        let Some(Session::Hunt(h)) = &mut self.session else { return };
        h.cq_deadline = None;
        let mut sent = None;
        h.bot.cq(&mut |tx| sent = Some(tx));
        if let Some(tx) = sent {
            h.queue(tx);
        }
    }

    pub(super) fn hunt_decoded(&mut self, lcd: &mut Lcd, s: &Settings, decoded: Decoded) {
        let Some(Session::Hunt(h)) = &mut self.session else { return };
        let written = decoded.write_to(&mut h.over);
        let ended = decoded == Decoded::Space && ends_with_turn(h.over.as_str());
        if ended {
            h.over_deadline = None;
        }
        if written {
            self.draw_hunt_over(lcd);
        }
        if ended {
            let now_ms = self.now_ms;
            self.hunt_over_done(lcd, s, now_ms);
        }
    }

    /// Your key went down or up; the over ends after a long enough silence, or at K.
    pub(super) fn hunt_key(&mut self, down: bool, at_ms: u32, dit_ms: u16) {
        let Some(Session::Hunt(h)) = &mut self.session else { return };
        h.cq_deadline = None;
        h.over_deadline = if down { None } else { Some(copy_end(at_ms, dit_ms)) };
    }

    pub(super) fn hunt_timeout(&mut self, lcd: &mut Lcd, s: &Settings, now_ms: u32) {
        let Some(Session::Hunt(h)) = &mut self.session else { return };
        if h.over_deadline.take().is_some() {
            return self.hunt_over_done(lcd, s, now_ms);
        }
        self.hunt_cq();
        self.draw_hunt_lines(lcd);
    }

    /// Your over ended: it logs the contact, or the activator answers it.
    fn hunt_over_done(&mut self, lcd: &mut Lcd, s: &Settings, now_ms: u32) {
        let Some(Session::Hunt(h)) = &mut self.session else { return };
        let over = h.over;
        h.over.clear();
        if over.as_str().trim().is_empty() {
            // The key went down, which stopped the CQs; nothing was sent, so they go on.
            if h.bot.phase == Phase::Calling && h.queued.is_none() && !h.playing {
                h.cq_deadline = Some(now_ms.wrapping_add(CQ_GAP_MS));
            }
            return self.draw_hunt_over(lcd);
        }
        push_scrolling(&mut h.lines, Line { yours: true, text: over, hidden: false });
        let mut sent = None;
        let logged = h.bot.over(over.as_str(), s.wpm, &mut |tx| sent = Some(tx));
        if let Some(tx) = sent {
            h.queue(tx);
        }
        if logged.is_some() {
            h.last = logged;
            self.history.contact();
            for line in h.lines.iter_mut().flatten() {
                line.hidden = false;
            }
            h.cq_deadline = Some(now_ms.wrapping_add(NEXT_MS));
        }
        self.draw_hunt_lines(lcd);
        self.draw_hunt_state(lcd);
        self.draw_hunt_over(lcd);
    }

    fn hunt_answer(&mut self, over: &str) {
        let Some(Session::Hunt(h)) = &mut self.session else { return };
        let mut sent = None;
        h.bot.heard(over.trim(), &mut |tx| sent = Some(tx));
        if let Some(tx) = sent {
            h.queue(tx);
        }
    }

    pub(super) fn hunt_act(&mut self, lcd: &mut Lcd, s: &mut Settings, action: Action) {
        match action {
            Action::Soft(0) => self.open_settings_at(lcd, s, Section::Qso),
            Action::Soft(1) => {
                self.hunt_answer("QRS");
                self.draw_hunt_lines(lcd);
            }
            Action::Soft(2) => {
                let Some(Session::Hunt(h)) = &mut self.session else { return };
                h.hint = !h.hint;
                let hint = h.hint;
                self.softkeys(lcd, exchange::keys(hint));
                self.draw_hunt_lines(lcd);
                self.draw_hunt_state(lcd);
                self.draw_hunt_over(lcd);
            }
            Action::Soft(3) => self.back(lcd, s),
            _ => {}
        }
    }

    /// Back from settings: the activator takes the new options and sends its last over again.
    pub(super) fn hunt_resume(&mut self, lcd: &mut Lcd, s: &Settings) {
        let Some(Session::Hunt(h)) = &mut self.session else { return self.back(lcd, s) };
        h.bot.set_options(Self::qso_options(s), s.wpm);
        h.bot.set_call(s.callsign());
        let calling = h.bot.phase == Phase::Calling;
        self.open(lcd, s, Screen::Hunt);
        if calling {
            self.hunt_cq();
        } else {
            self.hunt_answer("AGN");
        }
        self.draw_hunt_lines(lcd);
    }

    /// The activator finished sending; while it is calling and you are quiet, it calls again.
    pub(super) fn hunt_played(&mut self, now_ms: u32) {
        let Some(Session::Hunt(h)) = &mut self.session else { return };
        if !h.playing {
            return;
        }
        h.playing = false;
        let quiet = h.over.is_empty() && h.over_deadline.is_none() && h.queued.is_none();
        if quiet && h.bot.phase == Phase::Calling {
            h.cq_deadline = Some(now_ms.wrapping_add(CQ_GAP_MS));
        }
    }

    /// The next activator transmission, once the player is free.
    pub(super) fn hunt_next_play(&mut self, s: &Settings) -> Option<Play> {
        let seed = self.next_seed();
        let Some(Session::Hunt(h)) = &mut self.session else { return None };
        if h.playing {
            return None;
        }
        let tx = h.queued.take()?;
        h.playing = true;
        let keying = Keying { wpm: tx.wpm, effective: s.farnsworth(tx.wpm), fist: tx.fist };
        let pitch_hz = s.station_pitch(tx.pitch_offset_hz);
        Some(Play { text: tx.text, keying, seed, delay_ms: tx.delay_ms, pitch_hz, transmit: false })
    }
}
