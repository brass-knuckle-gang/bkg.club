//! Progress: practice time and copy accuracy over recent sessions, and totals.

use core::fmt::Write;

use pocketcw_logic::{
    input::Action,
    progress::{SLOTS, Session},
    settings::Settings,
};

use super::{Screen, Ui, settings::Section};
use crate::gfx::{self, TINY_W, rect, text, tiny};
use crate::lcd::Lcd;
use crate::text::Text;
use crate::theme::MAIN;

const SHOWN: usize = 14;
const LEGEND_Y: i32 = 28;
const CHART_X: i32 = 28;
const CHART_TOP: i32 = 44;
const CHART_H: i32 = 86;
const CHART_BOTTOM: i32 = CHART_TOP + CHART_H;
const COLUMN_W: i32 = (MAIN - CHART_X - 4) / SHOWN as i32;
const BAR_W: i32 = COLUMN_W - 6;
const MARK: i32 = 7;
const TABLE_Y: i32 = 150;
const TABLE_STEP: i32 = 20;
const COLUMNS_X: [i32; 4] = [56, 118, 178, 226];

#[derive(Default)]
struct Totals {
    active_s: u32,
    sent: u32,
    rx_chars: u32,
    rx_right: u32,
    contacts: u32,
}

impl Totals {
    fn add(&mut self, s: &Session) {
        self.active_s += s.active_s as u32;
        self.sent += s.sent_chars as u32;
        self.rx_chars += s.rx_chars as u32;
        self.rx_right += s.rx_right as u32;
        self.contacts += s.contacts as u32;
    }
}

impl Ui {
    pub(super) fn draw_progress(&mut self, lcd: &mut Lcd, s: &Settings) {
        let t = &self.theme;
        let (sessions, n) = self.history.sessions();
        let count = Text::<12>::format(format_args!("{n} SESSIONS"));
        self.header(lcd, s, "PROGRESS", &[count.as_str()]);
        self.softkeys(lcd, ["SET", "LTRS", "", "BACK"]);
        let bar_color = gfx::mix(t.fg, t.bg, 45);

        // Bars and hollow squares differ in shape, so the legend works without color.
        rect(lcd, 2, LEGEND_Y + 2, 6, 9, bar_color);
        tiny(lcd, 12, LEGEND_Y, "MINUTES", t.muted, t.bg);
        self.copy_mark(lcd, 80, LEGEND_Y + 3);
        tiny(lcd, 92, LEGEND_Y, "COPY % (RECEIVE)", t.muted, t.bg);

        let recent = &sessions[n.saturating_sub(SHOWN)..n];
        let most = recent.iter().map(Session::minutes).max().unwrap_or(0).max(10);
        let top = Text::<6>::format(format_args!("{most}M"));
        tiny(lcd, 2, CHART_TOP - 4, top.as_str(), t.muted, t.bg);
        tiny(lcd, 2, CHART_BOTTOM - 10, "0", t.muted, t.bg);
        rect(lcd, CHART_X, CHART_BOTTOM, MAIN - CHART_X - 4, 1, t.rule());
        if recent.is_empty() {
            tiny(lcd, CHART_X + 8, CHART_TOP + 30, "NOTHING YET: START KEYING", t.muted, t.bg);
        }
        for (k, s) in recent.iter().enumerate() {
            let x = CHART_X + k as i32 * COLUMN_W + 3;
            let h = (s.minutes() as i32 * CHART_H / most as i32).max(1);
            rect(lcd, x, CHART_BOTTOM - h, BAR_W, h, bar_color);
            if let Some(p) = s.rx_percent() {
                let y = CHART_BOTTOM - p as i32 * (CHART_H - MARK) / 100 - MARK;
                self.copy_mark(lcd, x + (BAR_W - MARK) / 2, y);
            }
        }
        if self.history.current().is_some() {
            let x = CHART_X + (recent.len() as i32 - 1) * COLUMN_W + 3;
            tiny(lcd, x + BAR_W - 3 * TINY_W, CHART_BOTTOM + 3, "NOW", t.muted, t.bg);
        }

        let headings = ["TIME", "SENT", "COPY", "QSOS"];
        for (x, h) in COLUMNS_X.iter().zip(headings) {
            tiny(lcd, *x, TABLE_Y, h, t.muted, t.bg);
        }
        let mut all = Totals::default();
        for s in &sessions[..n] {
            all.add(s);
        }
        let now = self.history.current().map(|s| {
            let mut now = Totals::default();
            now.add(&s);
            now
        });
        let last = Text::<8>::format(format_args!("LAST {}", n.min(SLOTS)));
        let rows = [("NOW", now), (last.as_str(), Some(all))];
        for (k, (label, row)) in rows.into_iter().enumerate() {
            let y = TABLE_Y + 12 + k as i32 * TABLE_STEP;
            tiny(lcd, 2, y + 3, label, t.muted, t.bg);
            let Some(row) = row else {
                text(lcd, COLUMNS_X[0], y, "-", t.muted, t.bg);
                continue;
            };
            let mut cells: [Text<8>; 4] = Default::default();
            let minutes = row.active_s / 60;
            let _ = if minutes >= 60 { write!(cells[0], "{}H{:02}", minutes / 60, minutes % 60) } else { write!(cells[0], "{minutes}M") };
            let _ = write!(cells[1], "{}", row.sent);
            let _ = match row.rx_chars {
                0 => write!(cells[2], "-"),
                chars => write!(cells[2], "{}%", row.rx_right * 100 / chars),
            };
            let _ = write!(cells[3], "{}", row.contacts);
            for (x, cell) in COLUMNS_X.iter().zip(&cells) {
                text(lcd, *x, y, cell.as_str(), t.fg, t.bg);
            }
        }
        tiny(lcd, 2, TABLE_Y + 12 + 2 * TABLE_STEP + 4, "TIME COUNTS KEYING AND LISTENING", t.muted, t.bg);
    }

    /// The copy accuracy marker: a hollow square.
    fn copy_mark(&self, lcd: &mut Lcd, x: i32, y: i32) {
        let t = &self.theme;
        rect(lcd, x, y, MARK, MARK, t.accent);
        rect(lcd, x + 2, y + 2, MARK - 4, MARK - 4, t.bg);
    }

    pub(super) fn progress_act(&mut self, lcd: &mut Lcd, s: &mut Settings, action: Action) {
        match action {
            Action::Soft(0) => self.open_settings_at(lcd, s, Section::Receive),
            Action::Soft(1) => self.open(lcd, s, Screen::Letters),
            Action::Soft(3) => self.back(lcd, s),
            _ => {}
        }
    }
}
