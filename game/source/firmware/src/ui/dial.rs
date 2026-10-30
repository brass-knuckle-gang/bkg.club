//! The dial on screens that don't use it for a list of their own: a click moves the focus
//! through the screen's targets and a turn steps the focused one. Send speed, receive speed
//! and volume show as header chips.

use pocketcw_logic::{dial::Target, drill::Kind, input::Action, settings::Settings};

use super::{Screen, Session, Ui};
use crate::lcd::Lcd;
use crate::text::Text;
use crate::theme::{CHIP_GAP, CHIPS_RIGHT};

const SEND_FIRST: [Target; 2] = [Target::Send, Target::Volume];
const RECEIVE_FIRST: [Target; 4] = [Target::Receive, Target::Farnsworth, Target::Send, Target::Volume];
const KOCH: [Target; 5] = [Target::Receive, Target::Farnsworth, Target::Send, Target::Volume, Target::Level];
const LISTEN: [Target; 3] = [Target::Receive, Target::Farnsworth, Target::Volume];
const RADIO: [Target; 2] = [Target::Volume, Target::Send];
const VOLUME: [Target; 1] = [Target::Volume];

impl Ui {
    /// The current screen's targets in click order, the default first. Empty where the screen
    /// uses the dial for its own list.
    pub(super) fn dial_targets(&self) -> &'static [Target] {
        match (self.screen, &self.session) {
            (Screen::Home, _) => &super::home::TARGETS,
            (Screen::Timing, _) => &SEND_FIRST,
            (Screen::Qso | Screen::Hunt, _) => &RECEIVE_FIRST,
            (Screen::Receive, Some(Session::Receive(rx))) if rx.kind() == Kind::Koch => &KOCH,
            (Screen::Receive, _) => &RECEIVE_FIRST,
            (Screen::Ragchew, Some(Session::Ragchew(c))) if !c.quiz => &LISTEN,
            (Screen::Ragchew, _) => &RECEIVE_FIRST,
            (Screen::Radio, _) => &RADIO,
            (Screen::Letters | Screen::Progress | Screen::System, _) => &VOLUME,
            (Screen::Menu | Screen::Settings | Screen::Boxing, _) => &[],
        }
    }

    /// Index of the focused target: the one a click moved to, else the default. Opening a screen
    /// sets the default again.
    pub(super) fn focus_index(&self) -> usize {
        let targets = self.dial_targets();
        self.dial_focus.and_then(|t| targets.iter().position(|&x| x == t)).unwrap_or(0)
    }

    pub(super) fn focused(&self) -> Option<Target> {
        self.dial_targets().get(self.focus_index()).copied()
    }

    pub(super) fn focus_next(&mut self) {
        let targets = self.dial_targets();
        let next = targets.get((self.focus_index() + 1) % targets.len().max(1)).copied();
        self.dial_focus = next;
    }

    /// Title, the dial chips, then up to one of `chips` left of them where it fits, and TX
    /// while your key keys the radio.
    pub(super) fn header(&self, lcd: &mut Lcd, s: &Settings, title: &str, chips: &[&str]) {
        let title_end = self.theme.header(lcd, title);
        let right = self.draw_dial_chips(lcd, s);
        let tx = [chips.first().copied().unwrap_or(""), "TX"];
        let chips = match (self.keys_radio(s), chips.is_empty()) {
            (true, true) => &tx[1..],
            (true, false) => &tx[..],
            (false, _) => chips,
        };
        self.theme.chips(lcd, chips, title_end + CHIP_GAP, right);
    }

    /// The header chips for the targets this screen has: volume, send speed, then receive
    /// speed, right to left. Returns the right edge for chips left of them.
    pub(super) fn draw_dial_chips(&self, lcd: &mut Lcd, s: &Settings) -> i32 {
        let mut right = CHIPS_RIGHT;
        if self.screen == Screen::Home {
            return right;
        }
        let targets = self.dial_targets();
        let focus = self.focused();
        let at = |target: Target| (focus == Some(target)).then_some(0);
        let two = |n: u8| Text::<2>::format(format_args!("{n:>2}"));
        if targets.contains(&Target::Volume) {
            right = self.theme.dial_chip(lcd, right, "VOL", &[two(s.volume).as_str()], at(Target::Volume)) - CHIP_GAP;
        }
        if targets.contains(&Target::Send) {
            right = self.theme.dial_chip(lcd, right, "SND", &[two(s.wpm).as_str()], at(Target::Send)) - CHIP_GAP;
        }
        if targets.contains(&Target::Receive) {
            let (wpm, effective) = s.receive_speed(s.wpm);
            let focus = match focus {
                Some(Target::Receive) => Some(0),
                Some(Target::Farnsworth) => Some(1),
                _ => None,
            };
            let numbers = [two(wpm), two(effective)];
            right = self.theme.dial_chip(lcd, right, "RCV", &[numbers[0].as_str(), numbers[1].as_str()], focus) - CHIP_GAP;
        }
        right
    }

    /// A turn or click on a screen with dial targets; false for anything the screen handles
    /// itself. Turns wait while you key.
    pub(super) fn dial_act(&mut self, lcd: &mut Lcd, s: &mut Settings, action: Action, now_ms: u32) -> bool {
        if matches!(self.screen, Screen::Home | Screen::Menu | Screen::Settings | Screen::Boxing) {
            return false;
        }
        match action {
            Action::Click => self.focus_next(),
            Action::Turn(_) if !self.key_guard.quiet(now_ms) => return true,
            Action::Turn(delta) => {
                let Some(target) = self.focused() else { return false };
                if !target.step(s, delta) {
                    self.step_level(delta);
                }
                if target == Target::Receive || target == Target::Send && s.rx_wpm == 0 {
                    self.bot_speed_changed(s);
                }
            }
            _ => return false,
        }
        self.draw_dial_chips(lcd, s);
        if self.screen == Screen::Receive {
            self.draw_receive_info(lcd);
        }
        true
    }
}
