//! Ragchew listening: two stations hold a contact at the receive speed. CHEW is for listening:
//! each over shows after it ends, or each line, as the RECV tab sets. QUIZ asks about each over
//! before it shows, and scores the answer you key like a receive drill.

use pocketcw_logic::{
    decoder::{Decoded, copy_end},
    drill::{MAX_COPY, Score, score_copy},
    input::Action,
    player::{Keying, MAX_TEXT},
    ragchew::{Over, Question, Ragchew},
    settings::Settings,
};

use super::footer::{INPUT_COLS, INPUT_Y};
use super::receive::{draw_marked, session_line};
use super::transcript::{self, Entry};
use super::{Play, Screen, Session, Ui, push_scrolling, settings::Section};
use crate::gfx::{CHAR_W, rect, text_cols, tiny};
use crate::lcd::Lcd;
use crate::text::Text;
use crate::theme::MAIN;

const LINE_X: i32 = 46;
const CHEW_ROWS: usize = transcript::MAX_ROWS;
const QUIZ_ROWS: usize = transcript::ROWS;
/// Enough lines to fill the transcript when each takes one row.
const LINES: usize = transcript::MAX_ROWS;
const RULE_Y: i32 = 155;
const PROMPT_Y: i32 = 160;
/// The answer as sent, marked against your copy.
const RESULT_Y: i32 = 178;
const COPY_X: i32 = 130;
/// Time after the question shows to start keying the answer before it counts as missed.
const ASK_MS: u32 = 10_000;
/// How long the score shows before the next over.
const RESULT_MS: u32 = 2500;
/// Silence between overs, while the other station takes over.
const OVER_GAP_MS: u32 = 1500;
/// Word spaces between lines of one over.
const LINE_GAP_WORDS: u32 = 1;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Playing,
    /// The over ended; waiting for you to start keying the answer.
    Asking,
    Answering,
    /// Scored; the next over starts at the deadline.
    Showing,
    Paused,
}

#[derive(Clone, Copy)]
struct Line {
    /// Index into `Ragchew::stations`.
    from: usize,
    /// The over's first line, which shows the sender's call.
    first: bool,
    text: Text<MAX_TEXT>,
    hidden: bool,
}

pub(super) struct Chew {
    pub(super) quiz: bool,
    chat: Ragchew,
    over: Over,
    /// The current over's lines are in `lines`.
    started: bool,
    /// Index of the line playing in `over`.
    line: usize,
    lines: [Option<Line>; LINES],
    question: Option<Question>,
    copy: Text<MAX_COPY>,
    result: Option<(Question, Score)>,
    state: State,
    pub(super) deadline: Option<u32>,
    session_right: u16,
    session_total: u16,
}

impl Chew {
    pub(super) fn badge(&self) -> &'static str {
        match self.state {
            State::Playing => "LISTEN",
            State::Asking => "YOUR TURN",
            State::Answering => "KEYING",
            State::Showing => "",
            State::Paused => "PAUSED",
        }
    }

    fn new(quiz: bool, seed: u32) -> Chew {
        let chat = Ragchew::new(seed);
        Chew {
            quiz,
            over: chat.over(),
            chat,
            started: false,
            line: 0,
            lines: [None; LINES],
            question: None,
            copy: Text::new(),
            result: None,
            state: State::Playing,
            deadline: None,
            session_right: 0,
            session_total: 0,
        }
    }

    /// The contact is playing or waiting on you, not paused.
    pub(super) fn running(&self) -> bool {
        self.state != State::Paused
    }

    pub(super) fn pause(&mut self) {
        self.state = State::Paused;
        self.deadline = None;
    }

    /// Shows line `k` of the current over, or all of them with None.
    fn reveal(&mut self, k: Option<usize>) {
        let n = self.over.lines().len();
        let count = self.lines.iter().flatten().count();
        let current = self.lines.iter_mut().flatten().skip(count.saturating_sub(n));
        for (i, line) in current.enumerate() {
            if k.is_none_or(|k| k == i) {
                line.hidden = false;
            }
        }
    }

    /// Moves on to the next over, with nothing of it shown yet.
    fn next_over(&mut self) {
        self.chat.next();
        self.over = self.chat.over();
        self.started = false;
        self.question = None;
    }
}

impl Ui {
    pub(super) fn open_ragchew(&mut self, lcd: &mut Lcd, s: &Settings, quiz: bool) {
        let seed = self.next_seed();
        self.session = Some(Session::Ragchew(Chew::new(quiz, seed)));
        self.open(lcd, s, Screen::Ragchew);
        self.start_over(lcd, s, OVER_GAP_MS);
    }

    pub(super) fn draw_ragchew(&mut self, lcd: &mut Lcd, s: &Settings) {
        let Some(Session::Ragchew(c)) = &self.session else { return };
        let t = &self.theme;
        self.header(lcd, s, if c.quiz { "QUIZ" } else { "CHEW" }, &[]);
        self.draw_ragchew_keys(lcd);
        if c.quiz {
            rect(lcd, 0, RULE_Y, MAIN, 1, t.rule());
        }
        self.draw_ragchew_lines(lcd);
        self.draw_ragchew_now(lcd, s);
    }

    fn draw_ragchew_keys(&self, lcd: &mut Lcd) {
        let Some(Session::Ragchew(c)) = &self.session else { return };
        let (again, pause) = if c.state == State::Paused { ("", "GO") } else { ("AGN", "STOP") };
        self.softkeys(lcd, ["SET", again, pause, "BACK"]);
    }

    fn draw_ragchew_lines(&self, lcd: &mut Lcd) {
        let Some(Session::Ragchew(c)) = &self.session else { return };
        let t = &self.theme;
        let mut entries: [Option<Entry>; LINES] = Default::default();
        for (entry, line) in entries.iter_mut().zip(&c.lines) {
            let Some(line) = line else { continue };
            let who = if line.first { c.chat.stations[line.from].call.as_str() } else { "" };
            let color = if line.from == 0 { t.fg } else { t.accent };
            *entry = Some(Entry { who, who_color: t.muted, color, text: line.text.as_str(), hidden: line.hidden });
        }
        transcript::draw(lcd, t, &entries, LINE_X, if c.quiz { QUIZ_ROWS } else { CHEW_ROWS });
    }

    /// The question, your answer and its score, what the screen waits for, and the footer.
    fn draw_ragchew_now(&mut self, lcd: &mut Lcd, s: &Settings) {
        let Some(Session::Ragchew(c)) = &self.session else { return };
        let t = &self.theme;
        if !c.quiz {
            let hint = if s.reveal_lines { "EACH LINE SHOWS AFTER IT ENDS" } else { "EACH OVER SHOWS AFTER IT ENDS" };
            return self.set_footer(lcd, hint);
        }
        rect(lcd, 0, PROMPT_Y, MAIN, 12, t.bg);
        let asking = matches!(c.state, State::Asking | State::Answering);
        let prompt = match (&c.question, &c.result) {
            (Some(q), _) if asking => q.prompt,
            (_, Some((q, _))) if c.state == State::Showing => q.prompt,
            _ => "",
        };
        tiny(lcd, 2, PROMPT_Y, prompt, t.fg, t.bg);

        rect(lcd, 0, RESULT_Y, MAIN, 18, t.bg);
        let showing = c.state == State::Showing;
        let copy = if asking || showing { c.copy.as_str() } else { "" };
        let shown = &copy[copy.len().saturating_sub(INPUT_COLS - 1)..];
        text_cols(lcd, 0, INPUT_Y, shown, INPUT_COLS, t.fg, t.bg);
        if asking {
            rect(lcd, shown.len() as i32 * CHAR_W + 1, INPUT_Y + 2, CHAR_W - 2, 13, t.accent);
        } else if let Some((q, score)) = c.result.filter(|_| showing) {
            let sent = q.answer.as_str().bytes().filter(|&b| b != b' ');
            draw_marked(lcd, t, RESULT_Y, COPY_X, sent, &score.marks, score.right);
        }

        let foot = session_line(c.session_right, c.session_total, "KEY ANSWERS AFTER EACH OVER  ? REPEATS");
        self.set_footer(lcd, foot.as_str());
    }

    /// Plays the current over from its first line after `delay_ms`, adding its lines hidden the
    /// first time.
    fn start_over(&mut self, lcd: &mut Lcd, s: &Settings, delay_ms: u32) {
        let Some(Session::Ragchew(c)) = &mut self.session else { return };
        if !c.started {
            c.started = true;
            let over = c.over;
            for (k, text) in over.lines().iter().enumerate() {
                push_scrolling(&mut c.lines, Line { from: over.from, first: k == 0, text: *text, hidden: true });
            }
        }
        c.line = 0;
        c.copy.clear();
        self.play_line(s, delay_ms);
        self.draw_ragchew_lines(lcd);
        self.draw_ragchew_now(lcd, s);
    }

    fn play_line(&mut self, s: &Settings, delay_ms: u32) {
        let seed = self.next_seed();
        let Some(Session::Ragchew(c)) = &mut self.session else { return };
        c.state = State::Playing;
        c.deadline = None;
        let station = &c.chat.stations[c.over.from];
        let (wpm, effective) = s.receive_speed(s.wpm);
        let keying = Keying { wpm, effective, fist: if s.human_fist { station.fist } else { 0 } };
        let pitch_hz = s.station_pitch(station.pitch_offset_hz);
        let text = c.over.lines()[c.line];
        self.play = Some(Play { text, keying, seed, delay_ms, pitch_hz, transmit: false });
    }

    pub(super) fn ragchew_played(&mut self, lcd: &mut Lcd, s: &Settings, now_ms: u32) {
        let Some(Session::Ragchew(c)) = &mut self.session else { return };
        if c.state != State::Playing {
            return;
        }
        if !c.quiz && s.reveal_lines {
            c.reveal(Some(c.line));
        }
        c.line += 1;
        if c.line < c.over.lines().len() {
            let (wpm, effective) = s.receive_speed(s.wpm);
            let gap = LINE_GAP_WORDS * Keying { wpm, effective, fist: 0 }.word_gap_ms();
            self.play_line(s, gap);
            return self.draw_ragchew_lines(lcd);
        }
        let question = if c.quiz { c.question.or_else(|| c.chat.question()) } else { None };
        match question {
            Some(q) => {
                c.question = Some(q);
                c.state = State::Asking;
                c.deadline = Some(now_ms.wrapping_add(ASK_MS));
            }
            None => {
                c.reveal(None);
                c.next_over();
                return self.start_over(lcd, s, OVER_GAP_MS);
            }
        }
        self.draw_ragchew_now(lcd, s);
    }

    pub(super) fn ragchew_key(&mut self, lcd: &mut Lcd, s: &Settings, down: bool, at_ms: u32, dit_ms: u16) {
        let Some(Session::Ragchew(c)) = &mut self.session else { return };
        let was = c.state;
        match (c.state, down) {
            (State::Asking, true) => {
                c.state = State::Answering;
                c.deadline = None;
            }
            (State::Answering, true) => c.deadline = None,
            (State::Answering, false) => c.deadline = Some(copy_end(at_ms, dit_ms)),
            _ => {}
        }
        if c.state != was {
            self.draw_ragchew_now(lcd, s);
        }
    }

    pub(super) fn ragchew_decoded(&mut self, lcd: &mut Lcd, s: &Settings, decoded: Decoded) {
        let Some(Session::Ragchew(c)) = &mut self.session else { return };
        if !matches!(c.state, State::Asking | State::Answering) {
            return;
        }
        if decoded.write_to(&mut c.copy) {
            self.draw_ragchew_now(lcd, s);
        }
    }

    /// The answer ended or timed out: score it. Or the score has shown long enough: play the
    /// next over.
    pub(super) fn ragchew_timeout(&mut self, lcd: &mut Lcd, s: &Settings, now_ms: u32) {
        let Some(Session::Ragchew(c)) = &mut self.session else { return };
        c.deadline = None;
        if c.state == State::Showing {
            return self.start_over(lcd, s, 0);
        }
        if !matches!(c.state, State::Asking | State::Answering) {
            return;
        }
        if c.copy.as_str().trim() == "?" {
            return self.start_over(lcd, s, OVER_GAP_MS);
        }
        let Some(question) = c.question else { return };
        let score = score_copy(question.answer.as_str(), c.copy.as_str(), &mut self.letters);
        self.history.copied(score.right, score.len as u8);
        c.session_right += score.right as u16;
        c.session_total += score.len as u16;
        c.result = Some((question, score));
        c.reveal(None);
        c.next_over();
        c.state = State::Showing;
        c.deadline = Some(now_ms.wrapping_add(RESULT_MS));
        self.draw_ragchew_lines(lcd);
        self.draw_ragchew_now(lcd, s);
    }

    pub(super) fn ragchew_act(&mut self, lcd: &mut Lcd, s: &mut Settings, action: Action) {
        let Some(Session::Ragchew(c)) = &mut self.session else { return };
        match action {
            Action::Soft(0) => self.open_settings_at(lcd, s, Section::Drills),
            Action::Soft(1) if matches!(c.state, State::Playing | State::Asking | State::Answering) => {
                self.stop_players();
                self.start_over(lcd, s, 0);
            }
            Action::Soft(2) if c.state == State::Paused => {
                self.start_over(lcd, s, OVER_GAP_MS);
                self.draw(lcd, s);
            }
            Action::Soft(2) => {
                c.pause();
                self.stop_players();
                self.draw(lcd, s);
            }
            Action::Soft(3) => self.back(lcd, s),
            _ => {}
        }
    }

    /// Back from settings: the current over plays again at the new speed.
    pub(super) fn ragchew_resume(&mut self, lcd: &mut Lcd, s: &Settings) {
        self.open(lcd, s, Screen::Ragchew);
        self.start_over(lcd, s, OVER_GAP_MS);
        self.draw_ragchew_keys(lcd);
    }
}
