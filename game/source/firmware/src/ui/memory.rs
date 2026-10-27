//! Memory keyer: the MEM row on Home sends a stored message on the radio, once on a press and
//! again and again on a hold. Touching the key stops a message.

use core::fmt::Write;

use pocketcw_logic::{
    keyer::Output,
    memories::SLOTS,
    player::{Keying, MAX_TEXT},
    settings::Settings,
};

use super::{COLS, Play, Screen, Ui};
use crate::gfx::{rect, text_cols, tiny};
use crate::lcd::Lcd;
use crate::text::Text;
use crate::theme::MAIN;

const HOME_Y: i32 = 148;
const HOME_STEP: i32 = 18;
const HOME_TEXT_X: i32 = 24;

/// A memory being sent from Home.
#[derive(Clone, Copy)]
pub(super) struct Sending {
    slot: usize,
    /// It sends again after REPEAT EVERY, until you touch the key.
    repeat: bool,
    /// Tick time of the next repeat, while waiting between repeats.
    pub(super) again_at: Option<u32>,
}

impl Ui {
    pub(super) fn open_mem_row(&mut self, lcd: &mut Lcd) {
        self.mem_row = true;
        self.draw_home_keys(lcd);
        let t = &self.theme;
        for i in 0..SLOTS {
            let y = HOME_Y + i as i32 * HOME_STEP;
            rect(lcd, 0, y, MAIN, HOME_STEP, t.bg);
            let label = Text::<3>::format(format_args!("M{}", i + 1));
            tiny(lcd, 2, y + 3, label.as_str(), t.muted, t.bg);
            let shown = self.memories.text(i);
            let (shown, color) = if shown.is_empty() { ("EMPTY", t.muted) } else { (shown, t.fg) };
            text_cols(lcd, HOME_TEXT_X, y, shown, COLS - 3, color, t.bg);
        }
        self.idle_status(lcd);
    }

    /// The KEYER keys: M1 to M4 on the open MEM row, with STOP on the memory that is sending.
    /// With the row closed, every key reads STOP while a memory sends, since any press stops it.
    pub(super) fn draw_home_keys(&self, lcd: &mut Lcd) {
        if self.screen != Screen::Home {
            return;
        }
        let keys = match (self.mem_row, self.sending) {
            (false, None) => super::home::KEYS,
            (false, Some(_)) => ["STOP"; 4],
            (true, sending) => {
                let mut keys = ["M1", "M2", "M3", "M4"];
                if let Some(sending) = sending {
                    keys[sending.slot] = "STOP";
                }
                keys
            }
        };
        self.softkeys(lcd, keys);
    }

    /// The footer when no memory is sending: what the MEM row does, if it is open.
    fn idle_status(&mut self, lcd: &mut Lcd) {
        self.set_footer(lcd, if self.mem_row { "HOLD REPEATS  CLICK CLOSES" } else { "" });
    }

    pub(super) fn close_mem_row(&mut self, lcd: &mut Lcd) {
        self.mem_row = false;
        self.draw_home_keys(lcd);
        self.draw_decode_all(lcd);
        if self.sending.is_none() {
            self.idle_status(lcd);
        }
    }

    /// An F key on the MEM row sends its memory, and closes the row unless AFTER SEND keeps it.
    pub(super) fn memory_key(&mut self, lcd: &mut Lcd, s: &Settings, slot: usize, repeat: bool) {
        if !self.memories.stay_open {
            self.close_mem_row(lcd);
        }
        self.send_memory(lcd, s, slot, repeat);
    }

    /// Sends memory `slot` on the radio at your keyer speed, again and again with `repeat`.
    pub(super) fn send_memory(&mut self, lcd: &mut Lcd, s: &Settings, slot: usize, repeat: bool) {
        let mut line = Text::<COLS>::new();
        let memory = self.memories.text(slot);
        if memory.is_empty() {
            self.sending = None;
            let _ = write!(line, "M{} IS EMPTY (SETTINGS)", slot + 1);
            return self.status(lcd, line.as_str());
        }
        let message = Text::<MAX_TEXT>::format(format_args!("{memory}"));
        let seed = self.next_seed();
        let keying = Keying { wpm: s.wpm, effective: s.wpm, fist: 0 };
        self.play = Some(Play { text: message, keying, seed, delay_ms: 0, pitch_hz: s.tone_hz, transmit: true });
        self.sending = Some(Sending { slot, repeat, again_at: None });
        self.draw_home_keys(lcd);
        let _ = match (s.output, s.key_out) {
            (Output::Paddle, _) => write!(line, "M{} TONE ONLY: OUT TYPE PADDLE", slot + 1),
            (_, false) => write!(line, "M{} TONE ONLY: KEY OUT OFF", slot + 1),
            _ => write!(line, "SENDING M{}  KEY TO STOP", slot + 1),
        };
        self.set_footer(lcd, line.as_str());
    }

    /// Stops a memory being sent or waiting to repeat.
    pub(super) fn stop_sending(&mut self, lcd: &mut Lcd) {
        if self.sending.take().is_some() {
            self.stop_players();
            self.draw_home_keys(lcd);
            self.idle_status(lcd);
        }
    }

    /// A memory ended: a repeating one waits and goes again.
    pub(super) fn memory_played(&mut self, lcd: &mut Lcd, now_ms: u32) {
        let Some(sending) = &mut self.sending else { return };
        if sending.again_at.is_some() {
            return;
        }
        if !sending.repeat {
            self.sending = None;
            self.draw_home_keys(lcd);
            return self.idle_status(lcd);
        }
        sending.again_at = Some(now_ms.wrapping_add(self.memories.every_s as u32 * 1000));
        let line = Text::<COLS>::format(format_args!("M{} REPEATS EVERY {}S  KEY TO STOP", sending.slot + 1, self.memories.every_s));
        self.set_footer(lcd, line.as_str());
    }

    pub(super) fn memory_timeout(&mut self, lcd: &mut Lcd, s: &Settings) {
        if let Some(sending) = self.sending {
            self.send_memory(lcd, s, sending.slot, true);
        }
    }
}
