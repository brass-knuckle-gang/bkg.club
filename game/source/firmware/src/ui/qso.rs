//! QSO bot screen: you activate a park or summit and work the pileup the bot sends. What the
//! hunters send stays hidden until the contact is logged, so you copy it by ear; HINT shows it.

use core::fmt::Write;

use pocketcw_logic::{
    decoder::{Decoded, copy_end},
    input::Action,
    player::MAX_TEXT,
    player::Keying,
    qso::{Bot, CALL_LEN, Hunter, Options, PILE_MAX, Program, Tx, ends_with_turn},
    settings::{Pileup, Settings},
};

use super::exchange::{self, Chip, RULE_Y};
use super::transcript::{self, Entry};
use super::{Play, Screen, Session, Ui, push_scrolling, settings::Section};
use crate::gfx::rect;
use crate::lcd::Lcd;
use crate::text::Text;
use crate::theme::{BAD, GOOD, MAIN, WARN};

const LINES: usize = 7;
const LINE_X: i32 = 46;
const QUEUE: usize = PILE_MAX + 2;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Who {
    You,
    Pile,
    Working,
}

/// Between pileup calls on one transcript line.
const PILE_GAP: &str = "  ";

#[derive(Clone, Copy)]
struct Line {
    who: Who,
    /// The hunter's call, for lines from the hunter you are working.
    call: Text<CALL_LEN>,
    text: Text<MAX_TEXT>,
    hidden: bool,
}

/// A logged contact, shown in the chips until the next one.
#[derive(Clone, Copy)]
struct Logged {
    hunter: Hunter,
    copied: bool,
}

pub(super) struct Qso {
    pub(super) bot: Bot,
    /// Your over so far, as decoded.
    over: Text<MAX_TEXT>,
    /// Tick time at which your over ends, while you are sending one.
    pub(super) over_deadline: Option<u32>,
    /// Transmissions to play, each marked when it starts a new bot answer.
    queue: [Option<(Tx, bool)>; QUEUE],
    playing: bool,
    /// More of the answer now playing may start at once, for an overlapping pileup.
    joining: bool,
    lines: [Option<Line>; LINES],
    hint: bool,
    last: Option<Logged>,
}

impl Qso {
    pub(super) fn badge(&self) -> &'static str {
        exchange::badge(self.playing || self.queue.iter().any(Option::is_some), self.over.as_str())
    }

    pub(super) fn pause(&mut self) {
        self.queue = [None; QUEUE];
        self.playing = false;
        self.joining = false;
        self.over.clear();
        self.over_deadline = None;
    }

    pub(super) fn new(program: Program, my_call: &str, seed: u32, options: Options) -> Qso {
        Qso {
            bot: Bot::new(program, my_call, seed, options),
            over: Text::new(),
            over_deadline: None,
            queue: [None; QUEUE],
            playing: false,
            joining: false,
            lines: [None; LINES],
            hint: false,
            last: None,
        }
    }

    /// Adds the bot's answers to the queue and the transcript; the pileup's calls share a line.
    /// `signing_off` labels the farewell of a hunter the bot has already logged.
    fn queue(&mut self, txs: &[Option<Tx>], signing_off: Option<Text<CALL_LEN>>) {
        let mut pile = Text::<MAX_TEXT>::new();
        for (k, tx) in txs.iter().flatten().enumerate() {
            if let Some(slot) = self.queue.iter_mut().find(|q| q.is_none()) {
                *slot = Some((*tx, k == 0));
            }
            if tx.working {
                let working = self.bot.working().map(|h| h.call);
                let call = working.or(signing_off).unwrap_or_default();
                push_scrolling(&mut self.lines, Line { who: Who::Working, call, text: tx.text, hidden: working.is_some() });
            } else {
                // A full line goes to the transcript, and the next call starts a new one.
                if !pile.is_empty() && pile.len() + PILE_GAP.len() + tx.text.len() > MAX_TEXT {
                    push_scrolling(&mut self.lines, Line { who: Who::Pile, call: Text::new(), text: pile, hidden: true });
                    pile = Text::new();
                }
                if !pile.is_empty() {
                    let _ = write!(pile, "{PILE_GAP}");
                }
                let _ = write!(pile, "{}", tx.text.as_str());
            }
        }
        if !pile.is_empty() {
            push_scrolling(&mut self.lines, Line { who: Who::Pile, call: Text::new(), text: pile, hidden: true });
        }
    }

    /// What to send next; with HINT on, the exact over.
    fn next_step(&self) -> Text<40> {
        let bot = &self.bot;
        let bye = bot.program.bye();
        let mut step = Text::new();
        let _ = if !self.over.is_empty() {
            write!(step, "END YOUR OVER WITH K, OR PAUSE")
        } else if let Some(h) = bot.working() {
            if h.state.is_empty() {
                write!(step, "SEND: TU {bye} E E")
            } else if self.hint {
                write!(step, "SEND: TU {} {bye} E E", h.state)
            } else { write!(step, "SEND: TU, THEIR STATE, {bye} E E") }
        } else if let Some(h) = bot.pile().next() {
            if self.hint { write!(step, "SEND: {} 5NN", h.call.as_str()) } else { write!(step, "SEND A CALL YOU COPIED, THEN 5NN") }
        } else {
            write!(step, "SEND: CQ {} DE {} K", bot.program.name(), bot.my_call())
        };
        step
    }

    fn pop_queue(&mut self) -> Option<Tx> {
        let (tx, _) = self.queue[0].take()?;
        self.queue.rotate_left(1);
        Some(tx)
    }

    /// The next queued transmission continues the answer now starting.
    fn same_answer_next(&self) -> bool {
        self.queue[0].is_some_and(|(_, starts)| !starts)
    }
}

impl Ui {
    pub(super) fn draw_qso(&mut self, lcd: &mut Lcd, s: &Settings) {
        let Some(Session::Qso(q)) = &self.session else { return };
        let t = &self.theme;
        self.header(lcd, s, q.bot.program.name(), &[]);
        self.softkeys(lcd, exchange::keys(q.hint));
        rect(lcd, 0, RULE_Y, MAIN, 1, t.rule());
        self.draw_qso_lines(lcd);
        self.draw_qso_state(lcd);
        self.draw_over(lcd);
    }

    fn draw_qso_lines(&self, lcd: &mut Lcd) {
        let Some(Session::Qso(q)) = &self.session else { return };
        let t = &self.theme;
        let mut entries: [Option<Entry>; LINES] = Default::default();
        for (entry, line) in entries.iter_mut().zip(&q.lines) {
            let Some(line) = line else { continue };
            let (who, who_color, color) = match line.who {
                Who::You => ("YOU", t.muted, t.fg),
                Who::Pile => ("PILE", WARN, t.accent),
                Who::Working => (line.call.as_str(), t.echo, t.accent),
            };
            *entry = Some(Entry { who, who_color, color, text: line.text.as_str(), hidden: line.hidden && !q.hint });
        }
        transcript::draw(lcd, t, &entries, LINE_X, transcript::ROWS);
    }

    /// The log count and the chips for the contact in progress or the last one logged.
    fn draw_qso_state(&self, lcd: &mut Lcd) {
        let Some(Session::Qso(q)) = &self.session else { return };
        let t = &self.theme;
        let bot = &q.bot;
        let working = bot.working();
        let log = Text::<24>::format(format_args!("LOG {}/{}  GOOD {}", bot.logged, bot.program.goal(), bot.copied));
        let done = bot.logged >= bot.program.goal();
        let label = if working.is_some() || q.last.is_none() { "NOW" } else { "LAST" };
        exchange::draw_now(lcd, t, label, log.as_str(), if done { GOOD } else { t.muted });

        let shown = working.copied().or(q.last.map(|l| l.hunter));
        let reveal = working.is_none() || q.hint;
        let state_color = match (working, q.last) {
            (None, Some(last)) => Some(if last.copied { GOOD } else { BAD }),
            _ => None,
        };
        let pile = Text::<4>::format(format_args!("{}", bot.pile_size()));
        // The state chip's label says what its color does: whether you sent the state back.
        let state_label = match (working, q.last) {
            (None, Some(last)) if last.copied => "COPIED",
            (None, Some(_)) => "MISSED",
            _ => "STATE",
        };
        let chips = [
            Chip { label: "CALL", value: shown.as_ref().map_or("", |h| h.call.as_str()), mark: working.map(|_| GOOD) },
            Chip { label: "RST", value: if reveal { shown.map_or("", |h| h.report) } else { "" }, mark: None },
            Chip { label: state_label, value: if reveal { shown.map_or("", |h| exchange::region(h.state)) } else { "" }, mark: state_color },
            Chip { label: "PILE", value: pile.as_str(), mark: None },
        ];
        exchange::draw_chips(lcd, t, chips);
    }

    fn draw_over(&mut self, lcd: &mut Lcd) {
        let Some(Session::Qso(q)) = &self.session else { return };
        exchange::draw_over(lcd, &self.theme, q.over.as_str());
        let next_step = q.next_step();
        self.set_footer(lcd, next_step.as_str());
    }

    pub(super) fn qso_decoded(&mut self, lcd: &mut Lcd, s: &Settings, decoded: Decoded) {
        let Some(Session::Qso(q)) = &mut self.session else { return };
        let written = decoded.write_to(&mut q.over);
        let ended = decoded == Decoded::Space && ends_with_turn(q.over.as_str());
        if written {
            self.draw_over(lcd);
        }
        if ended {
            self.qso_over_done(lcd, s);
        }
    }

    /// Your key went down or up; the over ends after a long enough silence, or at K.
    pub(super) fn qso_key(&mut self, down: bool, at_ms: u32, dit_ms: u16) {
        let Some(Session::Qso(q)) = &mut self.session else { return };
        q.over_deadline = if down { None } else { Some(copy_end(at_ms, dit_ms)) };
    }

    /// Your over ended: the bot answers it.
    pub(super) fn qso_over_done(&mut self, lcd: &mut Lcd, s: &Settings) {
        let Some(Session::Qso(q)) = &mut self.session else { return };
        q.over_deadline = None;
        let over = q.over;
        q.over.clear();
        if over.as_str().trim().is_empty() {
            return self.draw_over(lcd);
        }
        push_scrolling(&mut q.lines, Line { who: Who::You, call: Text::new(), text: over, hidden: false });
        self.bot_answer(lcd, s, over.as_str());
    }

    /// Passes `over` to the bot as if you had sent it, then shows and queues the answers.
    fn bot_answer(&mut self, lcd: &mut Lcd, s: &Settings, over: &str) {
        let Some(Session::Qso(q)) = &mut self.session else { return };
        let working = q.bot.working().copied();
        let logged = q.bot.logged;
        let mut txs: [Option<Tx>; QUEUE] = [None; QUEUE];
        let mut n = 0;
        q.bot.heard(over.trim(), s.wpm, &mut |tx| {
            if n < QUEUE {
                txs[n] = Some(tx);
                n += 1;
            }
        });
        if q.bot.logged != logged
            && let Some(hunter) = working
        {
            q.last = Some(Logged { hunter, copied: q.bot.exchange.state_copied == Some(true) });
            self.history.contact();
            for line in q.lines.iter_mut().flatten() {
                line.hidden = false;
            }
        }
        q.queue(&txs, working.map(|h| h.call));
        self.draw_qso_lines(lcd);
        self.draw_qso_state(lcd);
        self.draw_over(lcd);
    }

    pub(super) fn qso_act(&mut self, lcd: &mut Lcd, s: &mut Settings, action: Action) {
        match action {
            Action::Soft(0) => self.open_settings_at(lcd, s, Section::Qso),
            Action::Soft(1) => self.bot_answer(lcd, s, "QRS"),
            Action::Soft(2) => {
                let Some(Session::Qso(q)) = &mut self.session else { return };
                q.hint = !q.hint;
                let hint = q.hint;
                self.softkeys(lcd, exchange::keys(hint));
                self.draw_qso_lines(lcd);
                self.draw_qso_state(lcd);
                self.draw_over(lcd);
            }
            Action::Soft(3) => self.back(lcd, s),
            _ => {}
        }
    }

    /// Back from settings: the hunters take the new options and send their last over again.
    pub(super) fn qso_resume(&mut self, lcd: &mut Lcd, s: &Settings) {
        if let Some(Session::Qso(q)) = &mut self.session {
            q.bot.set_options(Self::qso_options(s), s.wpm);
            q.bot.set_call(s.callsign());
        }
        self.open(lcd, s, Screen::Qso);
        self.bot_answer(lcd, s, "AGN");
    }

    pub(super) fn qso_played(&mut self) {
        if let Some(Session::Qso(q)) = &mut self.session {
            q.playing = false;
        }
    }

    /// The next bot transmission, once the players are free. With an overlapping pileup, every
    /// transmission of one answer starts together, each after its own delay.
    pub(super) fn qso_next_play(&mut self, s: &Settings) -> Option<Play> {
        let seed = self.next_seed();
        let Some(Session::Qso(q)) = &mut self.session else { return None };
        if q.playing && !q.joining {
            return None;
        }
        let tx = q.pop_queue()?;
        q.playing = true;
        q.joining = s.pileup == Pileup::Overlap && q.same_answer_next();
        let keying = Keying { wpm: tx.wpm, effective: s.farnsworth(tx.wpm), fist: tx.fist };
        let pitch_hz = s.station_pitch(tx.pitch_offset_hz);
        Some(Play { text: tx.text, keying, seed, delay_ms: tx.delay_ms, pitch_hz, transmit: false })
    }
}

