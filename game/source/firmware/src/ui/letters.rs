//! Letters: copy accuracy for every Koch character, and your most common mixups.

use pocketcw_logic::{
    drill::Kind,
    input::Action,
    letters::{KOCH, Mastery},
    settings::{Practice, Settings},
};

use super::{Ui, char_cells, draw_char, settings::Section};
use crate::fonts::SMALL;
use crate::gfx::{self, CHAR_W, TINY_W, rect, tiny};
use crate::lcd::Lcd;
use crate::text::Text;
use crate::theme::{BAD, GOOD, MAIN, WARN};

const PER_ROW: usize = 9;
const TILE_W: i32 = 30;
const TILE_H: i32 = 27;
const GRID_Y: i32 = 26;
const MIXUP_Y: i32 = 176;
const SHOWN_MIXUPS: usize = 6;

impl Ui {
    pub(super) fn draw_letters(&mut self, lcd: &mut Lcd, s: &Settings) {
        let t = &self.theme;
        let level = Text::<10>::format(format_args!("LEVEL {}", self.letters.level));
        self.header(lcd, s, "LETTERS", &[level.as_str()]);
        self.softkeys(lcd, ["SET", "", "KOCH", "BACK"]);
        for (i, &c) in KOCH.iter().enumerate() {
            let x = 2 + (i % PER_ROW) as i32 * TILE_W;
            let y = GRID_Y + (i / PER_ROW) as i32 * (TILE_H + 2);
            let label_x = x + (TILE_W - 2 - char_cells(c) as i32 * CHAR_W) / 2;
            match self.letters.accuracy(i) {
                None => {
                    rect(lcd, x, y, TILE_W - 2, TILE_H, gfx::mix(t.fg, t.bg, 94));
                    draw_char(lcd, &SMALL, label_x, y + 1, c, gfx::mix(t.fg, t.bg, 70), gfx::mix(t.fg, t.bg, 94));
                }
                Some((percent, _)) => {
                    let color = match self.letters.mastery(i) {
                        Some(Mastery::Known) => GOOD,
                        Some(Mastery::Weak) => BAD,
                        _ => WARN,
                    };
                    let tile = gfx::mix(color, t.bg, 78);
                    rect(lcd, x, y, TILE_W - 2, TILE_H, tile);
                    rect(lcd, x, y + TILE_H - 2, TILE_W - 2, 2, color);
                    draw_char(lcd, &SMALL, label_x, y + 1, c, t.fg, tile);
                    let p = Text::<4>::format(format_args!("{percent}"));
                    tiny(lcd, x + (TILE_W - 2 - p.len() as i32 * TINY_W) / 2, y + 14, p.as_str(), t.fg, tile);
                }
            }
        }
        rect(lcd, 0, MIXUP_Y - 4, MAIN, 1, t.rule());
        tiny(lcd, 2, MIXUP_Y, "MIXUPS: SENT/COPIED", t.muted, t.bg);
        let mut any = false;
        for (k, m) in self.letters.mixups().take(SHOWN_MIXUPS).enumerate() {
            any = true;
            let x = 2 + (k % 3) as i32 * 90;
            let y = MIXUP_Y + 14 + (k / 3) as i32 * 19;
            let mut pair_x = x + draw_char(lcd, &SMALL, x, y, m.sent, t.fg, t.bg);
            pair_x += draw_char(lcd, &SMALL, pair_x, y, b'/', t.fg, t.bg);
            pair_x += draw_char(lcd, &SMALL, pair_x, y, m.copied, t.fg, t.bg);
            let count = Text::<5>::format(format_args!("{}X", m.count));
            tiny(lcd, pair_x + 6, y + 3, count.as_str(), t.muted, t.bg);
        }
        if !any {
            tiny(lcd, 2, MIXUP_Y + 17, "NONE YET: RUN A RECEIVE DRILL", t.muted, t.bg);
        }
    }

    pub(super) fn letters_act(&mut self, lcd: &mut Lcd, s: &mut Settings, action: Action) {
        match action {
            Action::Soft(0) => self.open_settings_at(lcd, s, Section::Receive),
            Action::Soft(2) => self.open_practice(lcd, s, Practice::Drill(Kind::Koch)),
            Action::Soft(3) => self.back(lcd, s),
            _ => {}
        }
    }
}
