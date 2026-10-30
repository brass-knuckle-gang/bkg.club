//! Settings: one list in sections. The dial moves between rows, a click starts or ends editing
//! the selected row, and F2 and F3 jump between sections. A memory or the callsign takes what
//! you key while you edit it.

use core::fmt::Write;

use pocketcw_logic::{
    config,
    decoder::Decoded,
    input::Action,
    keyer::Output,
    memories::LEN,
    settings::{Field, MAX_BRIGHTNESS, Pileup, Settings, Shortcut, UsbOut},
};

use super::{Ui, mode_name};
use crate::gfx::{TINY_H, TINY_W, rect, text, text_right, tiny};
use crate::lcd::Lcd;
use crate::text::Text;
use crate::theme::{self, MAIN, Theme};

const LIST_Y: i32 = 26;
const ROW_STEP: i32 = 19;
const VISIBLE: usize = 9;
const RULE_Y: i32 = LIST_Y + VISIBLE as i32 * ROW_STEP + 2;
const ABOUT_Y: i32 = RULE_Y + 5;
const MEMORY_X: i32 = 44;
/// Tiny characters of a memory that fit between `MEMORY_X` and the row's right end.
const MEMORY_COLS: usize = ((MAIN - 10 - MEMORY_X) / TINY_W) as usize;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Section {
    Keyer,
    Sound,
    Receive,
    Drills,
    Qso,
    Memories,
    Callsign,
    Device,
    Theme,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Row {
    Section(Section),
    Value(Field),
    Memory(usize),
    Every,
    AfterSend,
    Call,
}

/// Each row: its name, what it is, and the footer line that says what it does.
const ROWS: [(&str, Row, &str); 40] = [
    ("KEYER", Row::Section(Section::Keyer), ""),
    ("MODE", Row::Value(Field::Mode), "IAMBIC A OR B, OR STRAIGHT/SIDESWIPER"),
    ("PADDLES", Row::Value(Field::Swap), "WHICH LEVER SENDS DITS"),
    ("SPEED", Row::Value(Field::Speed), "YOUR KEYER SPEED"),
    ("KEY OUT", Row::Value(Field::KeyOut), "KEYS THE RADIO ON KEYER TIMING RADIO"),
    ("OUT TYPE", Row::Value(Field::Output), "WHAT THE FUNC JACK SENDS THE RADIO"),
    ("OUT LINES", Row::Value(Field::OutputSwap), "WHICH FUNC JACK LINES KEY"),
    ("SOUND", Row::Section(Section::Sound), ""),
    ("TONE", Row::Value(Field::Tone), "THE PITCH OF YOUR SIDETONE"),
    ("VOLUME", Row::Value(Field::Volume), "THE VOLUME OF EVERYTHING"),
    ("WAVE", Row::Value(Field::Wave), "SINE IS SOFT, SQUARE IS BRIGHT"),
    ("NOISE", Row::Value(Field::Noise), "BAND NOISE UNDER DRILLS AND STATIONS"),
    ("FADING", Row::Value(Field::Fading), "HOW DEEP STATIONS FADE"),
    ("RECEIVE", Row::Section(Section::Receive), ""),
    ("CHAR SPEED", Row::Value(Field::RxSpeed), "DRILLS AND STATIONS  MATCH: YOURS"),
    ("FARNSWORTH", Row::Value(Field::RxEffective), "LOWER IT FOR LONGER GAPS"),
    ("DRILLS", Row::Section(Section::Drills), ""),
    ("CHEW SHOWS", Row::Value(Field::Reveal), "WHEN A RAGCHEW'S TEXT SHOWS"),
    ("NINJA WORDS", Row::Value(Field::NinjaWords), "NINJA PICKS FROM THIS MANY WORDS"),
    ("QSO", Row::Section(Section::Qso), ""),
    ("PILEUP", Row::Value(Field::Pileup), "HOW HUNTERS CALL YOU"),
    ("FIST", Row::Value(Field::Fist), "HUMAN: SMALL TIMING ERRORS"),
    ("SPEED SPREAD", Row::Value(Field::Spread), "STATIONS SEND FASTER OR SLOWER"),
    ("MEMORIES", Row::Section(Section::Memories), ""),
    ("M1", Row::Memory(0), "CLICK, THEN KEY THE MESSAGE"),
    ("M2", Row::Memory(1), "CLICK, THEN KEY THE MESSAGE"),
    ("M3", Row::Memory(2), "CLICK, THEN KEY THE MESSAGE"),
    ("M4", Row::Memory(3), "CLICK, THEN KEY THE MESSAGE"),
    ("REPEAT EVERY", Row::Every, "GAP BEFORE A HELD MEMORY REPEATS"),
    ("AFTER SEND", Row::AfterSend, "CLOSE OR KEEP THE MEM ROW OPEN"),
    ("CALLSIGN", Row::Section(Section::Callsign), ""),
    ("CALL", Row::Call, "FOR THE STATIONS AND THE BOOT SCREEN"),
    ("DEVICE", Row::Section(Section::Device), ""),
    ("USB OUT", Row::Value(Field::UsbOut), "WHAT THE USB PORT SENDS A COMPUTER"),
    ("DECODER BOARD", Row::Value(Field::DecoderBoard), "THE RADIO DECODER OF THE PRO MODEL"),
    ("F1 HOLD", Row::Value(Field::Shortcut), "WHAT HOLDING F1 DOES ON EVERY SCREEN"),
    ("THEME", Row::Section(Section::Theme), ""),
    ("STYLE", Row::Value(Field::Style), "THE SHAPES OF THE SCREEN"),
    ("PALETTE", Row::Value(Field::Palette), "THE COLORS  DAILY: A NEW FLAG EACH BOOT"),
    ("BRIGHTNESS", Row::Value(Field::Brightness), "THE BACKLIGHT"),
];

fn is_section(i: usize) -> bool {
    matches!(ROWS[i].1, Row::Section(_))
}

fn section_start(section: Section) -> usize {
    ROWS.iter().position(|r| r.1 == Row::Section(section)).unwrap_or(0)
}

/// The scroll that shows row `i`, with its section's heading when that sits just above it.
fn scroll_to(scroll: usize, i: usize) -> usize {
    let top = if i > 0 && is_section(i - 1) { i - 1 } else { i };
    let scroll = if top < scroll { top } else if i >= scroll + VISIBLE { i + 1 - VISIBLE } else { scroll };
    scroll.min(ROWS.len() - VISIBLE)
}

impl Ui {
    /// Selects the first row of `section`, with its heading at the top of the list.
    pub(super) fn select_section(&mut self, section: Section) {
        let start = section_start(section);
        self.row = start + 1;
        self.scroll = start.min(ROWS.len() - VISIBLE);
        self.editing = false;
    }

    /// A SOUND row is selected, so band noise plays at the level you set.
    pub(super) fn sound_selected(&self) -> bool {
        (0..=self.row).rev().find_map(|i| match ROWS[i].1 {
            Row::Section(section) => Some(section),
            _ => None,
        }) == Some(Section::Sound)
    }

    /// A memory or the callsign is taking what you key.
    pub(super) fn editing_text(&self) -> bool {
        self.editing && matches!(ROWS[self.row].1, Row::Memory(_) | Row::Call)
    }

    pub(super) fn draw_settings(&mut self, lcd: &mut Lcd, s: &Settings) {
        self.header(lcd, s, "SETTINGS", &[]);
        self.draw_settings_keys(lcd);
        rect(lcd, 0, RULE_Y, MAIN, 1, self.theme.rule());
        self.draw_settings_list(lcd, s);
    }

    fn draw_settings_keys(&self, lcd: &mut Lcd) {
        let keys = if self.editing_text() { ["", "DEL", "", "BACK"] } else { ["", "PREV", "NEXT", "BACK"] };
        self.softkeys(lcd, keys);
    }

    fn draw_settings_list(&self, lcd: &mut Lcd, s: &Settings) {
        for i in self.scroll..self.scroll + VISIBLE {
            self.draw_settings_row(lcd, s, i);
        }
        self.draw_settings_about(lcd);
    }

    fn draw_settings_row(&self, lcd: &mut Lcd, s: &Settings, i: usize) {
        if !(self.scroll..self.scroll + VISIBLE).contains(&i) {
            return;
        }
        let t = &self.theme;
        let y = LIST_Y + (i - self.scroll) as i32 * ROW_STEP;
        rect(lcd, 0, y, MAIN, ROW_STEP, t.bg);
        let (name, row, _) = ROWS[i];
        if is_section(i) {
            tiny(lcd, 4, y + 5, name, t.accent, t.bg);
            let x = 10 + name.len() as i32 * TINY_W;
            rect(lcd, x, y + 11, MAIN - 6 - x, 1, t.rule());
            return;
        }
        let selected = i == self.row;
        let editing = selected && self.editing;
        let (fg, value_fg, muted, bg) = if selected {
            let (fg, muted, bg) = t.select(lcd, 2, y, MAIN - 6, ROW_STEP - 1);
            (fg, fg, muted, bg)
        } else {
            (t.fg, t.accent, t.muted, t.bg)
        };
        text(lcd, 10, y + 2, name, fg, bg);
        if let Row::Memory(m) = row {
            let memory = self.memories.text(m);
            let shown = match (editing, memory.is_empty()) {
                (true, _) => Text::<{ MEMORY_COLS + 1 }>::format(format_args!(
                    "{}_",
                    &memory[memory.len().saturating_sub(MEMORY_COLS - 1)..]
                )),
                (false, true) => Text::format(format_args!("EMPTY")),
                (false, false) => Text::format(format_args!("{}", &memory[..memory.len().min(MEMORY_COLS)])),
            };
            let color = if memory.is_empty() && !editing { muted } else { fg };
            tiny(lcd, MEMORY_X, y + 5, shown.as_str(), color, bg);
            return;
        }
        let value = self.row_value(s, row);
        let shown = match (editing, row) {
            (false, _) => Text::<20>::format(format_args!("{}", value.as_str())),
            (true, Row::Call) => Text::format(format_args!("{}_", value.as_str())),
            (true, _) => Text::format(format_args!("< {} >", value.as_str())),
        };
        text_right(lcd, MAIN - 12, y + 2, shown.as_str(), value_fg, bg);
    }

    fn row_value(&self, s: &Settings, row: Row) -> Text<16> {
        match row {
            Row::Value(field) => Text::format(format_args!("{}", value(s, field).as_str())),
            Row::Every => Text::format(format_args!("{} S", self.memories.every_s)),
            Row::AfterSend => Text::format(format_args!("{}", if self.memories.stay_open { "STAY OPEN" } else { "CLOSE" })),
            Row::Call => Text::format(format_args!("{}", s.callsign())),
            Row::Section(_) | Row::Memory(_) => Text::new(),
        }
    }

    /// What the selected row does, or how to edit the text it takes.
    fn draw_settings_about(&self, lcd: &mut Lcd) {
        let t = &self.theme;
        let (_, row, about) = ROWS[self.row];
        let line = match (self.editing, row) {
            (true, Row::Memory(m)) => {
                Text::<40>::format(format_args!("{} LEFT  F2 DELETES  HOLD F3 CLEARS", LEN - self.memories.text(m).len()))
            }
            (true, Row::Call) => Text::format(format_args!("KEY IT  F2 DELETES  HOLD F3 CLEARS")),
            _ => Text::format(format_args!("{about}")),
        };
        rect(lcd, 0, ABOUT_Y, MAIN, TINY_H, t.bg);
        tiny(lcd, 2, ABOUT_Y, line.as_str(), t.muted, t.bg);
    }

    pub(super) fn settings_act(&mut self, lcd: &mut Lcd, s: &mut Settings, action: Action) {
        let row = ROWS[self.row].1;
        let text_row = matches!(row, Row::Memory(_) | Row::Call);
        match action {
            Action::Soft(3) => {
                self.editing = false;
                self.close_settings(lcd, s);
            }
            Action::Soft(1) if self.editing && text_row => {
                match row {
                    Row::Memory(m) => self.memories.pop(m),
                    _ => s.pop_callsign(),
                }
                self.draw_settings_row(lcd, s, self.row);
                self.draw_settings_about(lcd);
            }
            Action::Soft(1 | 2) if self.editing && text_row => {}
            Action::Soft(key @ (1 | 2)) => self.jump_section(lcd, s, key == 2),
            Action::Click => {
                self.editing = !self.editing;
                if text_row {
                    self.draw_settings_keys(lcd);
                }
                self.draw_settings_row(lcd, s, self.row);
                self.draw_settings_about(lcd);
            }
            Action::Turn(delta) if self.editing => match row {
                Row::Value(field) => self.adjust(lcd, s, field, delta),
                Row::Every => {
                    self.memories.step_every(delta);
                    self.draw_settings_row(lcd, s, self.row);
                }
                Row::AfterSend => {
                    self.memories.stay_open ^= delta % 2 != 0;
                    self.draw_settings_row(lcd, s, self.row);
                }
                Row::Section(_) | Row::Memory(_) | Row::Call => {}
            },
            Action::Turn(delta) => self.move_row(lcd, s, delta),
            _ => {}
        }
    }

    /// Selects the next row in the direction of `delta`, past section headings.
    fn move_row(&mut self, lcd: &mut Lcd, s: &Settings, delta: i8) {
        let next = if delta > 0 {
            (self.row + 1..ROWS.len()).find(|&i| !is_section(i))
        } else {
            (0..self.row).rev().find(|&i| !is_section(i))
        };
        let Some(next) = next else { return };
        let old = core::mem::replace(&mut self.row, next);
        let scroll = scroll_to(self.scroll, next);
        if scroll != self.scroll {
            self.scroll = scroll;
            return self.draw_settings_list(lcd, s);
        }
        self.draw_settings_row(lcd, s, old);
        self.draw_settings_row(lcd, s, next);
        self.draw_settings_about(lcd);
    }

    /// Selects the first row of the next section, or of the one before this row's.
    fn jump_section(&mut self, lcd: &mut Lcd, s: &Settings, forward: bool) {
        let here = (0..self.row).rev().find(|&i| is_section(i)).unwrap_or(0);
        let previous = |end: usize| (0..end).rev().find(|&i| is_section(i));
        // Both ends wrap; ROWS starts with a section.
        let start = if forward {
            (self.row..ROWS.len()).find(|&i| is_section(i)).unwrap_or(0)
        } else {
            previous(here).or_else(|| previous(ROWS.len())).unwrap_or(0)
        };
        let Row::Section(section) = ROWS[start].1 else { return };
        self.select_section(section);
        self.draw_settings_list(lcd, s);
    }

    fn adjust(&mut self, lcd: &mut Lcd, s: &mut Settings, field: Field, delta: i8) {
        s.adjust(field, delta);
        match field {
            Field::Style | Field::Palette => {
                self.theme = Theme::new(s.style, s.palette, self.boot_count);
                self.draw(lcd, s);
            }
            Field::Shortcut => {
                self.shortcut = s.shortcut;
                self.draw_settings_keys(lcd);
                self.draw_settings_row(lcd, s, self.row);
            }
            // OUT LINES names its values by the output type, and the board offers USB OUT RADIO.
            Field::Output | Field::DecoderBoard => self.draw_settings_list(lcd, s),
            _ => self.draw_settings_row(lcd, s, self.row),
        }
    }

    /// Empties the memory or callsign being edited.
    pub(super) fn clear_text(&mut self, lcd: &mut Lcd, s: &mut Settings) {
        match ROWS[self.row].1 {
            Row::Memory(m) => self.memories.clear(m),
            _ => {
                while !s.callsign().is_empty() {
                    s.pop_callsign();
                }
            }
        }
        self.draw_settings_row(lcd, s, self.row);
        self.draw_settings_about(lcd);
    }

    pub(super) fn settings_decoded(&mut self, lcd: &mut Lcd, s: &mut Settings, decoded: Decoded) {
        if !self.editing {
            return;
        }
        match (ROWS[self.row].1, decoded) {
            (Row::Memory(m), decoded) => {
                let byte = match decoded {
                    Decoded::Char(c) => c.to_ascii_uppercase(),
                    Decoded::Space => b' ',
                    Decoded::Unknown => return,
                };
                if !self.memories.push(m, byte) {
                    return self.status(lcd, "MEMORY FULL");
                }
            }
            (Row::Call, Decoded::Char(c)) if s.push_callsign(c as char) => {}
            _ => return,
        }
        self.draw_settings_row(lcd, s, self.row);
        self.draw_settings_about(lcd);
    }
}

/// What a value row shows: the screen's wording, which GET and SET on the serial port also use.
pub(super) fn value(s: &Settings, field: Field) -> Text<{ config::SHOWN_LEN }> {
    let on_off = |on: bool| if on { "ON" } else { "OFF" };
    let mut t = Text::new();
    let _ = match field {
        Field::Speed => write!(t, "{} WPM", s.wpm),
        Field::Mode => write!(t, "{}", mode_name(s.mode)),
        Field::Swap => write!(t, "{}", if s.swap { "REVERSED" } else { "NORMAL" }),
        Field::KeyOut => write!(t, "{}", on_off(s.key_out)),
        Field::Output => write!(t, "{}", if s.output == Output::Paddle { "PADDLE" } else { "STRAIGHT" }),
        Field::OutputSwap => write!(
            t,
            "{}",
            match (s.output, s.output_swap) {
                (Output::Straight, false) => "TIP KEYS",
                (Output::Straight, true) => "RING KEYS",
                (Output::Paddle, false) => "DIT ON TIP",
                (Output::Paddle, true) => "DIT ON RING",
            }
        ),
        Field::Tone => write!(t, "{} HZ", s.tone_hz),
        Field::Volume => write!(t, "{}", s.volume),
        Field::Style => write!(t, "{}", theme::style_name(s.style)),
        Field::Palette => write!(t, "{}", theme::palette_name(s.palette)),
        Field::RxSpeed if s.rx_wpm == 0 => write!(t, "MATCH"),
        Field::RxSpeed => write!(t, "{} WPM", s.rx_wpm),
        Field::RxEffective if s.rx_effective == 0 => write!(t, "OFF"),
        Field::RxEffective => write!(t, "{} WPM", s.rx_effective),
        Field::Pileup => write!(
            t,
            "{}",
            match s.pileup {
                Pileup::Off => "OFF",
                Pileup::TakeTurns => "TAKE TURNS",
                Pileup::Overlap => "OVERLAP",
            }
        ),
        Field::Fist => write!(t, "{}", if s.human_fist { "HUMAN" } else { "CLEAN" }),
        Field::Spread => write!(t, "{}", on_off(s.spread)),
        Field::Reveal => write!(t, "{}", if s.reveal_lines { "EACH LINE" } else { "WHOLE OVER" }),
        Field::Wave => write!(t, "{}", if s.square { "SQUARE" } else { "SINE" }),
        Field::DecoderBoard => write!(t, "{}", on_off(s.decoder_board)),
        Field::NinjaWords => write!(t, "{}", s.ninja_words),
        Field::UsbOut => write!(
            t,
            "{}",
            match s.usb_out {
                UsbOut::Off => "OFF",
                UsbOut::Text => "TEXT",
                UsbOut::Vband => "VBAND",
                UsbOut::Radio => "RADIO",
                UsbOut::Serial => "SERIAL",
            }
        ),
        Field::Noise | Field::Fading => {
            let level = if field == Field::Noise { s.noise } else { s.fading };
            if level == 0 { write!(t, "OFF") } else { write!(t, "{level}") }
        }
        Field::Brightness => write!(t, "{}%", s.brightness as u16 * 100 / MAX_BRIGHTNESS as u16),
        Field::Shortcut => write!(
            t,
            "{}",
            match s.shortcut {
                Shortcut::KeyOut => "KEY OUT",
                Shortcut::Resume => "RESUME",
                Shortcut::Mem => "MEM ROW",
                Shortcut::Timing => "TIMING",
            }
        ),
    };
    t
}
