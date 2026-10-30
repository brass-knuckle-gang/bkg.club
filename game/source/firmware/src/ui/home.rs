//! Home: speed, sound and key settings, the timing tape, and what your keying decodes to.

use core::fmt::Write;

use pocketcw_logic::{
    decode_lines::DecodeLines,
    dial::Target,
    input::Action,
    keyer::{MAX_WPM, MIN_WPM},
    settings::{Field, Settings},
};

use super::{COLS, Screen, Ui, dit_ms, lanes, mode_name, settings::Section};
use crate::gfx::{CHAR_W, TINY_H, TINY_W, blocks, blocks_width, para, rect, text, tiny};
use crate::lcd::Lcd;
use crate::text::{Tenths, Text};
use crate::theme::{MAIN, UNDERLINE_H, underline};

const DIGITS_X: i32 = 7;
const DIGITS_Y: i32 = 29;
const DIGIT_PX: i32 = 5;
const DIGIT_GAP: i32 = 4;
const DIGITS_W: i32 = blocks_width(2, DIGIT_PX, DIGIT_GAP);
const DIGITS_H: i32 = 7 * DIGIT_PX;
const CELL_X: i32 = 98;
const CELL_STEP: i32 = 44;
const CELL_LABEL_Y: i32 = 26;
const CELL_VALUE_Y: i32 = 38;
const BAR_Y: i32 = 58;
const STATS_Y: i32 = 131;
const STATS_COLS: usize = ((MAIN - 2) / TINY_W) as usize;
const RULE_Y: i32 = 145;
const DECODE_Y: i32 = 148;
const DECODE_STEP: i32 = 18;
/// The underline takes the bottom row of the text cell and the 2 rows under it.
const UNDERLINE_Y: i32 = DECODE_STEP - UNDERLINE_H;
const ROWS: usize = 4;
pub(super) const KEYS: [&str; 4] = ["SET", "MEM", "TMNG", "MENU"];

/// What the dial sets here, in click order.
pub(super) const TARGETS: [Target; 3] = [Target::Send, Target::Volume, Target::Tone];

/// The field a target shows in, and its value cell if not the digits.
fn field(target: Target) -> (Field, Option<usize>) {
    match target {
        Target::Tone => (Field::Tone, Some(0)),
        Target::Volume => (Field::Volume, Some(1)),
        _ => (Field::Speed, None),
    }
}

pub(super) type Decode = DecodeLines<COLS, ROWS>;

impl Ui {
    pub(super) fn draw_home_header(&self, lcd: &mut Lcd, s: &Settings) {
        self.header(lcd, s, "KEYER", &[mode_name(s.mode)]);
    }

    pub(super) fn draw_home(&mut self, lcd: &mut Lcd, s: &Settings) {
        let t = &self.theme;
        self.draw_home_header(lcd, s);
        self.draw_home_keys(lcd);
        rect(lcd, DIGITS_X - 5, DIGITS_Y - 3, DIGITS_W + 10, DIGITS_H + 6, t.panel());
        tiny(lcd, DIGITS_X + DIGITS_W + 8, DIGITS_Y + DIGITS_H - 12, "WPM", t.muted, t.bg);
        for (i, label) in ["TONE", "VOL", "OUT", "DIT"].iter().enumerate() {
            tiny(lcd, CELL_X + i as i32 * CELL_STEP, CELL_LABEL_Y, label, t.muted, t.bg);
        }
        for field in [Field::Speed, Field::Tone, Field::Volume, Field::KeyOut] {
            self.draw_home_value(lcd, s, field);
        }
        self.focus_mark(lcd, self.focus_index(), true);
        self.tape.clear(lcd, &self.theme, lanes(s));
        self.draw_home_stats(lcd);
        rect(lcd, 0, RULE_Y, MAIN, 1, self.theme.rule());
        self.draw_decode_all(lcd);
    }

    pub(super) fn draw_home_value(&self, lcd: &mut Lcd, s: &Settings, field: Field) {
        let t = &self.theme;
        let mut value = Text::<6>::new();
        match field {
            Field::Speed => {
                let _ = write!(value, "{:>2}", s.wpm);
                blocks(lcd, DIGITS_X, DIGITS_Y, value.as_str(), DIGIT_PX, DIGIT_GAP, t.slant(), t.fg, Some(t.panel()));
                t.bar(lcd, CELL_X, BAR_Y, MAIN - CELL_X - 4, 8, (s.wpm - MIN_WPM) as i32, (MAX_WPM - MIN_WPM) as i32);
                value.clear();
                let _ = write!(value, "{}", dit_ms(s));
                self.cell(lcd, 3, value.as_str(), "MS");
            }
            Field::Tone => {
                let _ = write!(value, "{}", s.tone_hz);
                self.cell(lcd, 0, value.as_str(), "HZ");
            }
            Field::Volume => {
                let _ = write!(value, "{}", s.volume);
                self.cell(lcd, 1, value.as_str(), "");
            }
            Field::KeyOut => self.cell(lcd, 2, if s.key_out { "ON" } else { "OFF" }, ""),
            _ => {}
        }
    }

    /// A value under its label, with a unit when there is room.
    fn cell(&self, lcd: &mut Lcd, i: usize, value: &str, unit: &str) {
        let t = &self.theme;
        let x = CELL_X + i as i32 * CELL_STEP;
        rect(lcd, x, CELL_VALUE_Y, (CELL_STEP - 1).min(MAIN - x), 16, t.bg);
        text(lcd, x, CELL_VALUE_Y, value, t.fg, t.bg);
        let unit_x = x + value.len() as i32 * CHAR_W;
        if !unit.is_empty() && unit_x + unit.len() as i32 * TINY_W <= x + CELL_STEP {
            tiny(lcd, unit_x, CELL_VALUE_Y + 4, unit, t.muted, t.bg);
        }
    }

    fn focus_mark(&self, lcd: &mut Lcd, index: usize, on: bool) {
        let t = &self.theme;
        let color = if on { t.accent } else { t.bg };
        match field(TARGETS[index]).1 {
            None => para(lcd, DIGITS_X - 1, DIGITS_Y + DIGITS_H + 5, DIGITS_W + 4, 3, t.skew(), color),
            Some(cell) => para(lcd, CELL_X + cell as i32 * CELL_STEP, CELL_VALUE_Y + 17, 36, 2, t.skew(), color),
        }
    }

    pub(super) fn draw_home_stats(&self, lcd: &mut Lcd) {
        let t = &self.theme;
        let mut line = Text::<40>::new();
        let st = &self.stats;
        let _ = write!(line, "CHR ");
        units(&mut line, st.char_gap.mean());
        let _ = write!(line, " WRD ");
        units(&mut line, st.word_gap.mean());
        let _ = write!(line, " RATIO ");
        number(&mut line, st.ratio());
        let _ = write!(line, " ");
        number(&mut line, st.effective_wpm());
        let _ = write!(line, "WPM");
        let shown = &line.as_str()[..line.len().min(STATS_COLS)];
        tiny(lcd, 2, STATS_Y, shown, t.muted, t.bg);
        rect(lcd, 2 + shown.len() as i32 * TINY_W, STATS_Y, (STATS_COLS - shown.len()) as i32 * TINY_W, TINY_H, t.bg);
    }

    pub(super) fn draw_decode_all(&self, lcd: &mut Lcd) {
        for row in 0..ROWS {
            self.draw_decode_line(lcd, row);
        }
        self.draw_cursor(lcd);
    }

    fn draw_decode_line(&self, lcd: &mut Lcd, row: usize) {
        let t = &self.theme;
        let y = DECODE_Y + row as i32 * DECODE_STEP;
        text(lcd, 0, y, self.decode.line(row), t.fg, t.bg);
        rect(lcd, 0, y + 16, MAIN, 2, t.bg);
        for (col, &q) in self.decode.quality(row).iter().enumerate() {
            if let Some(q) = q {
                underline(lcd, col as i32 * CHAR_W + 1, y + UNDERLINE_Y, CHAR_W - 2, q, t.bg);
            }
        }
    }

    /// Draws the newest decoded character, or every line after a scroll.
    pub(super) fn draw_decode_push(&self, lcd: &mut Lcd) {
        let d = &self.decode;
        if d.scrolled() {
            return self.draw_decode_all(lcd);
        }
        let (row, col) = d.cursor();
        let Some(col) = col.checked_sub(1) else { return };
        let t = &self.theme;
        let y = DECODE_Y + row as i32 * DECODE_STEP;
        let x = col as i32 * CHAR_W;
        text(lcd, x, y, &d.line(row)[col..=col], t.fg, t.bg);
        if let Some(q) = d.quality(row)[col] {
            underline(lcd, x + 1, y + UNDERLINE_Y, CHAR_W - 2, q, t.bg);
        }
        self.draw_cursor(lcd);
    }

    fn draw_cursor(&self, lcd: &mut Lcd) {
        let (row, col) = self.decode.cursor();
        if col < COLS {
            let y = DECODE_Y + row as i32 * DECODE_STEP;
            rect(lcd, col as i32 * CHAR_W + 1, y + 2, CHAR_W - 2, 13, self.theme.accent);
        }
    }

    pub(super) fn home_act(&mut self, lcd: &mut Lcd, s: &mut Settings, action: Action, now_ms: u32) {
        if self.mem_row {
            match action {
                Action::Soft(slot) => return self.memory_key(lcd, s, slot as usize, false),
                Action::Click if self.sending.is_none() => return self.close_mem_row(lcd),
                _ => {}
            }
        }
        // A press while a memory sends only stops it; the dial still sets speed.
        if self.sending.is_some() && !matches!(action, Action::Turn(_)) {
            return self.stop_sending(lcd);
        }
        match action {
            Action::Turn(_) if !self.key_guard.quiet(now_ms) => {}
            Action::Turn(delta) => {
                let target = TARGETS[self.focus_index()];
                target.step(s, delta);
                self.draw_home_value(lcd, s, field(target).0);
            }
            Action::Click => {
                self.focus_mark(lcd, self.focus_index(), false);
                self.focus_next();
                self.focus_mark(lcd, self.focus_index(), true);
            }
            Action::Soft(0) => self.open_settings_at(lcd, s, Section::Keyer),
            Action::Soft(1) => self.open_mem_row(lcd),
            Action::Soft(2) => self.open(lcd, s, Screen::Timing),
            Action::Soft(3) => self.open_menu(lcd, s),
            _ => {}
        }
    }
}

/// Writes `value` with one decimal and a U for units, or dashes.
pub(super) fn units<const N: usize>(out: &mut Text<N>, value: Option<f32>) {
    number(out, value);
    if value.is_some() {
        let _ = write!(out, "U");
    }
}

pub(super) fn number<const N: usize>(out: &mut Text<N>, value: Option<f32>) {
    let _ = match value {
        Some(v) => write!(out, "{}", Tenths(v)),
        None => write!(out, "--"),
    };
}
