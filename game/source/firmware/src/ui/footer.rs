//! The bottom of every screen: the input row with its state badge, then the footer line. A
//! status message takes the footer line for a while, then the screen's own footer comes back.

use pocketcw_logic::ticks;

use super::{COLS, Screen, Session, Ui};
use crate::gfx::{TINY_H, TINY_W, rect, tiny};
use crate::lcd::{HEIGHT, Lcd};
use crate::text::Text;
use crate::theme::MAIN;

/// Top of the input row: your copy or over, and the state badge at its right end.
pub(super) const INPUT_Y: i32 = 204;
/// Characters of the input row left of the badge, one of them for the cursor.
pub(super) const INPUT_COLS: usize = COLS - 7;
const BADGE_Y: i32 = INPUT_Y + 5;
/// Room for the longest badge, `YOUR TURN`.
const BADGE_W: i32 = 9 * TINY_W;
const _: () = assert!(INPUT_COLS as i32 * crate::gfx::CHAR_W <= MAIN - 2 - BADGE_W);
/// The footer clears from here, below the input row's 16 px text.
pub(super) const FOOT_TOP: i32 = INPUT_Y + 18;
const FOOT_Y: i32 = 226;
pub(super) const FOOT_COLS: usize = ((MAIN - 2) / TINY_W) as usize;
const STATUS_MS: u32 = 5000;

pub(super) type FootLine = Text<FOOT_COLS>;

impl Ui {
    /// The screen's own footer line. It shows whenever no status message does.
    pub(super) fn set_footer(&mut self, lcd: &mut Lcd, line: &str) {
        self.footer = FootLine::format(format_args!("{line}"));
        if self.status.is_none() && self.alert.is_none() {
            self.draw_footer(lcd);
        }
    }

    /// A status message on the footer line for a few seconds. An empty line brings the
    /// footer back at once.
    pub fn status(&mut self, lcd: &mut Lcd, line: &str) {
        let until = self.now_ms.wrapping_add(STATUS_MS);
        self.status = (!line.is_empty()).then(|| (FootLine::format(format_args!("{line}")), until));
        self.draw_footer(lcd);
    }

    /// A message for a state that lasts, such as a stuck contact, until an empty line ends it.
    /// A status message shows over it for its time.
    pub fn alert(&mut self, lcd: &mut Lcd, line: &str) {
        self.alert = (!line.is_empty()).then(|| FootLine::format(format_args!("{line}")));
        self.draw_footer(lcd);
    }

    /// Brings the footer back once a timed status message has had its time.
    pub(super) fn status_timeout(&mut self, lcd: &mut Lcd, now_ms: u32) {
        if let Some((_, until)) = self.status
            && ticks::reached(now_ms, until)
        {
            self.status = None;
            self.draw_footer(lcd);
        }
    }

    /// The status message if one shows, else the alert, else the screen's footer.
    pub(super) fn draw_footer(&self, lcd: &mut Lcd) {
        if self.full_screen() {
            return;
        }
        let t = &self.theme;
        let (line, color) = match (&self.status, &self.alert) {
            (Some((line, _)), _) | (None, Some(line)) => (line.as_str(), t.fg),
            (None, None) => (self.footer.as_str(), t.muted),
        };
        rect(lcd, 0, FOOT_TOP, MAIN, HEIGHT as i32 - FOOT_TOP, t.bg);
        tiny(lcd, 2, FOOT_Y, line, color, t.bg);
    }

    /// What the practice on this screen waits for; empty on other screens.
    fn badge(&self) -> &'static str {
        match (self.screen, &self.session) {
            (Screen::Receive, Some(Session::Receive(rx))) => rx.badge(),
            (Screen::Ragchew, Some(Session::Ragchew(c))) => c.badge(),
            (Screen::Qso, Some(Session::Qso(q))) => q.badge(),
            (Screen::Hunt, Some(Session::Hunt(h))) => h.badge(),
            _ => "",
        }
    }

    /// Draws the state badge at the same spot on every practice screen, when it changed or
    /// `redrawn` cleared it.
    pub(super) fn draw_badge(&mut self, lcd: &mut Lcd, redrawn: bool) {
        if redrawn {
            self.badge_shown = "";
        }
        let badge = self.badge();
        if badge == self.badge_shown || self.full_screen() {
            return;
        }
        self.badge_shown = badge;
        let t = &self.theme;
        rect(lcd, MAIN - 2 - BADGE_W, BADGE_Y, BADGE_W + 2, TINY_H, t.bg);
        tiny(lcd, MAIN - 2 - badge.len() as i32 * TINY_W, BADGE_Y, badge, t.echo, t.bg);
    }
}
