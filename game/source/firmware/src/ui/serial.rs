//! Serial port commands the UI answers: settings and memories by name, PLAY, INFO and the stats.
//! Each reply line ends with `\n`.

use core::fmt::Write;

use pocketcw_logic::{
    config::{self, Name},
    letters::{KOCH, Mastery},
    morse,
    pack::Error as PackError,
    panel::Panel,
    player::{Keying, MAX_TEXT},
    serial::Stats,
    settings::{Field, Settings},
};

use super::{Play, Screen, Session, Ui, settings};
use crate::audio;
use crate::lcd::Lcd;
use crate::text::Text;
use crate::theme::Theme;

/// Room for the longest reply: an error that lists a setting's values.
pub const REPLY_LEN: usize = config::REASON_LEN + 32;
pub type Reply = Text<REPLY_LEN>;

impl Ui {
    /// `INFO VERSION=0.2.0 SCREEN=KEYER PANEL=IPS PACK=3131 AUDIO=1900/4096`. AUDIO is the sample
    /// interrupt's longest run since the last INFO, against its budget, in CPU cycles.
    pub fn info(&self) -> Reply {
        let screen = match self.screen {
            Screen::Home => "KEYER",
            Screen::Timing => "TIMING",
            Screen::Qso => "QSO",
            Screen::Hunt => "HUNT",
            Screen::Menu => "MENU",
            Screen::Settings => "SETTINGS",
            Screen::System => "SYSTEM",
            Screen::Receive => "RECEIVE",
            Screen::Ragchew => "RAGCHEW",
            Screen::Letters => "LETTERS",
            Screen::Progress => "PROGRESS",
            Screen::Radio => "RADIO",
            Screen::Boxing => "BOXING",
        };
        let panel = match self.system.panel {
            Panel::Ips => "IPS",
            Panel::Ili9341 => "ILI9341",
        };
        let mut pack = Text::<8>::new();
        let _ = match self.pack() {
            Ok(p) => write!(pack, "{}", p.count()),
            Err(PackError::Missing) => write!(pack, "NONE"),
            Err(PackError::Format(_)) => write!(pack, "OLD"),
            Err(PackError::Corrupt) => write!(pack, "BAD"),
        };
        let (audio, budget) = (audio::take_worst_cycles(), audio::SAMPLE_CYCLES);
        let version = env!("CARGO_PKG_VERSION");
        Text::format(format_args!("INFO VERSION={version} SCREEN={screen} PANEL={panel} PACK={} AUDIO={audio}/{budget}\n", pack.as_str()))
    }

    /// `VALUE SPEED 25`
    pub fn get(&self, s: &Settings, name: Name) -> Reply {
        let value = config::get(name, s, &self.memories, settings::value);
        Text::format(format_args!("VALUE {} {}\n", name.as_str(), value.as_str()))
    }

    /// Sets `name` to `value` and redraws the screen. `OK SPEED 25`, or `ERROR SPEED: 5 TO 60`.
    pub fn set(&mut self, lcd: &mut Lcd, s: &mut Settings, name: Name, value: &str) -> Reply {
        if let Err(reason) = config::set(name, value, s, &mut self.memories, settings::value) {
            return Text::format(format_args!("ERROR {}: {}\n", name.as_str(), reason.as_str()));
        }
        if let Name::Setting(Field::Style | Field::Palette) = name {
            self.theme = Theme::new(s.style, s.palette, self.boot_count);
        }
        // The QSO and hunt stations take what the settings screen gives them on its way back.
        match name {
            Name::Setting(Field::RxSpeed | Field::Pileup | Field::Fist | Field::Spread) => self.bot_speed_changed(s),
            Name::Call => match &mut self.session {
                Some(Session::Qso(q)) => q.bot.set_call(s.callsign()),
                Some(Session::Hunt(h)) => h.bot.set_call(s.callsign()),
                _ => {}
            },
            _ => {}
        }
        if name == Name::Setting(Field::Speed) {
            self.speed_set(lcd, s);
        } else {
            self.draw(lcd, s);
        }
        let value = config::get(name, s, &self.memories, settings::value);
        Text::format(format_args!("OK {} {}\n", name.as_str(), value.as_str()))
    }

    /// Sounds `text` as the receive drills do, with band noise on any screen until it ends.
    pub fn play_serial(&mut self, s: &Settings, text: Text<MAX_TEXT>) {
        let (wpm, effective) = s.receive_speed(s.wpm);
        let keying = Keying { wpm, effective, fist: 0 };
        let seed = self.next_seed();
        self.play = Some(Play { text, keying, seed, delay_ms: 0, pitch_hz: s.tone_hz, transmit: false });
        self.serial_play = true;
    }

    pub fn stop_serial(&mut self) {
        self.stop_players();
        self.serial_play = false;
    }

    /// The stats lines, then `END`. Letters: `LEVEL n`, then `LETTER C TRIES PERCENT MASTERY`
    /// for each Koch character, then `MIXUP SENT COPIED COUNT`, most common first. Progress:
    /// `SESSION SEQ ACTIVE_S CONTACTS RX_CHARS RX_RIGHT SENT_CHARS`, oldest first. Timing: the
    /// lines of `timing::Stats::report`.
    pub fn stats(&self, kind: Stats) -> impl Iterator<Item = Reply> + '_ {
        let letters = &self.letters;
        let level = (kind == Stats::Letters).then(|| Text::format(format_args!("LEVEL {}\n", letters.level)));
        let each_letter = KOCH.iter().enumerate().filter(move |_| kind == Stats::Letters).map(move |(i, c)| {
            let (percent, tries) = letters.accuracy(i).unwrap_or((0, 0));
            let mastery = match letters.mastery(i) {
                None => "NEW",
                Some(Mastery::Known) => "KNOWN",
                Some(Mastery::Unsure) => "UNSURE",
                Some(Mastery::Weak) => "WEAK",
            };
            Text::format(format_args!("LETTER {} {tries} {percent} {mastery}\n", morse::text(c)))
        });
        let mixups = letters.mixups().filter(move |_| kind == Stats::Letters).map(|m| {
            Text::format(format_args!("MIXUP {} {} {}\n", morse::text(&m.sent), morse::text(&m.copied), m.count))
        });
        let (sessions, n) = self.history.sessions();
        let sessions = sessions.into_iter().take(if kind == Stats::Progress { n } else { 0 }).map(|s| {
            Text::format(format_args!(
                "SESSION {} {} {} {} {} {}\n",
                s.seq, s.active_s, s.contacts, s.rx_chars, s.rx_right, s.sent_chars
            ))
        });
        let timing = self.stats.report::<REPLY_LEN>().chain(self.hand.report::<REPLY_LEN>());
        let timing = timing.take(if kind == Stats::Timing { usize::MAX } else { 0 });
        level.into_iter().chain(each_letter).chain(mixups).chain(sessions).chain(timing).chain([Text::format(format_args!("END\n"))])
    }
}
