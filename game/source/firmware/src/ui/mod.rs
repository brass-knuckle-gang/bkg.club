//! Screens: the keyer, timing, QSO and hunt practice, receive drills and ragchews, letters,
//! progress, the menu, settings and system. Each event redraws only what it changed; a full
//! redraw takes about 70 ms.

mod boxing;
mod dial;
mod exchange;
mod footer;
mod home;
mod hunt;
mod letters;
mod memory;
mod menu;
mod progress;
mod qso;
mod radio;
mod ragchew;
mod receive;
mod ring;
mod serial;
mod settings;
mod system;
mod tape;
mod timing;
mod transcript;

use embedded_graphics::{pixelcolor::Rgb565, prelude::Point};
use pocketcw_logic::{
    battery::Gauge,
    boxing::{Career, RECORD_LEN as CAREER_LEN},
    decoder::Decoded,
    dial::{KeyGuard, Target},
    input::Action,
    morse,
    letters::{Letters, RECORD_LEN as LETTERS_LEN},
    pack::WORD_TEXT,
    memories::{Memories, RECORD_LEN as MEMORIES_LEN},
    progress::{History, SLOT_LEN},
    keyer::{self, Mode},
    player::{Keying, MAX_TEXT},
    qso::{Options, Program},
    settings::{Field, Pileup, Practice, Settings, Shortcut},
    synth::Sound,
    ticks,
    hand::Hand,
    timing::{Interval, Line, Meter, PAUSE_UNITS, Stats},
};

use crate::font::Font;
use crate::gfx::rect;
use crate::lcd::Lcd;
use crate::pack::Pack;
use crate::text::Text;
use crate::theme::{Battery, MAIN, Theme};

use boxing::Boxing;
use home::Decode;
use hunt::Hunting;
use qso::Qso;
use ragchew::Chew;
use receive::Receive;
use tape::Tape;

pub use serial::Reply;
pub use system::{PanelFrom, SystemInfo};

const COLS: usize = (MAIN / crate::gfx::CHAR_W) as usize;
/// When each contact, tip then ring, stops keying.
const STUCK_MS: [u32; 2] = [keyer::TIP_STUCK_MS as u32, keyer::RING_STUCK_MS as u32];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Home,
    Timing,
    Qso,
    Hunt,
    Menu,
    Settings,
    System,
    Receive,
    Ragchew,
    Letters,
    Progress,
    Radio,
    Boxing,
}

/// The practice open on its screen, or paused while you are on another screen.
// Without a heap the large variant can't be boxed; one union still costs less than four fields.
#[allow(clippy::large_enum_variant)]
enum Session {
    Qso(Qso),
    Hunt(Hunting),
    Receive(Receive),
    Ragchew(Chew),
}

impl Session {
    fn screen(&self) -> Screen {
        match self {
            Session::Qso(_) => Screen::Qso,
            Session::Hunt(_) => Screen::Hunt,
            Session::Receive(_) => Screen::Receive,
            Session::Ragchew(_) => Screen::Ragchew,
        }
    }

    fn practice(&self) -> Practice {
        match self {
            Session::Qso(q) => Practice::Qso(q.bot.program),
            Session::Hunt(_) => Practice::Hunt,
            Session::Receive(rx) => Practice::Drill(rx.kind()),
            Session::Ragchew(c) => Practice::Chew { quiz: c.quiz },
        }
    }

    /// Stops the practice where it stands: nothing plays or waits until it resumes.
    fn pause(&mut self) {
        match self {
            Session::Qso(q) => q.pause(),
            Session::Hunt(h) => h.pause(),
            Session::Receive(rx) => rx.pause(),
            Session::Ragchew(c) => c.pause(),
        }
    }
}

/// Something for the tone player to do.
pub enum Command {
    Play(Play),
    Stop,
}

/// A transmission for the player, with everything it needs.
pub struct Play {
    pub text: Text<MAX_TEXT>,
    pub keying: Keying,
    pub seed: u32,
    pub delay_ms: u32,
    pub pitch_hz: u16,
    /// Key the radio too, as a memory keyer; touching the key stops it.
    pub transmit: bool,
}

pub struct Ui {
    theme: Theme,
    boot_count: u16,
    screen: Screen,
    /// The focused dial target, once a click has moved it on this screen.
    dial_focus: Option<Target>,
    /// The screen's own footer line, and a status message with the tick time it ends, if it does.
    footer: footer::FootLine,
    /// A timed status message, and the tick time it ends.
    status: Option<(footer::FootLine, u32)>,
    alert: Option<footer::FootLine>,
    /// The state badge drawn now.
    badge_shown: &'static str,
    key_guard: KeyGuard,
    decode: Decode,
    /// What holding F1 does, as the settings last drawn had it, for its label.
    shortcut: Shortcut,
    /// The menu list open, and the selected row of each list.
    list: menu::List,
    cursor: [usize; menu::LISTS],
    /// The settings row at the top of the list, and the selected row.
    scroll: usize,
    row: usize,
    /// The dial sets the selected row's value instead of moving the selection.
    editing: bool,
    meter: Meter,
    stats: Stats,
    hand: Hand,
    /// TIMING shows the HAND view, not BARS.
    timing_hand: bool,
    tape: Tape,
    /// When each contact closed, while it is closed: tip, ring.
    contact_since: [Option<u32>; 2],
    /// The stuck warning is showing for this contact.
    stuck_shown: [bool; 2],
    session: Option<Session>,
    letters: Letters,
    /// `letters` as last saved.
    letters_saved: Letters,
    /// The Koch drill's window, kept while you leave the drill, and the level it counts toward.
    koch_window: (u8, (u16, u16)),
    history: History,
    memories: Memories,
    /// `memories` as last saved.
    memories_saved: Memories,
    /// The MEM row is open on Home: F1..F4 send M1..M4.
    mem_row: bool,
    /// Each F key's press or hold already acted, so its release does nothing.
    spent: [bool; 4],
    sending: Option<memory::Sending>,
    /// Every player stops before anything else plays.
    stop: bool,
    /// A transmission to start, after any stop.
    play: Option<Play>,
    /// Text for the voice to say, after any stop.
    say: Option<Text<WORD_TEXT>>,
    /// A PLAY from the serial port is sounding.
    serial_play: bool,
    /// A practice screen's transmission is sounding, which counts as practice time.
    listening: bool,
    seed: u32,
    system: SystemInfo,
    /// None when the ADC did not answer.
    battery: Option<crate::battery::Reading>,
    gauge: Gauge,
    /// What the header's battery icon shows now.
    battery_shown: Option<Battery>,
    /// Tick count at the last poll.
    now_ms: u32,
    /// The practice screen whose SET key opened settings; BACK returns to it.
    settings_from: Option<Screen>,
    heard: radio::Heard,
    board: radio::Board,
    /// USB OUT is not OFF, so MIDI in can arrive.
    usb_on: bool,
    /// A command for the decoder board, not sent yet.
    board_command: Option<pocketcw_logic::board::Command>,
    pack: Result<Pack, pocketcw_logic::pack::Error>,
    /// Brass Knuckles Knock Out, kept while you are on other screens.
    boxing: Option<Boxing>,
    /// The game's progress, and as last saved.
    career: Career,
    career_saved: Career,
    /// SUPERFISH CW was picked: the UI task runs it next.
    superfish: bool,
}

impl Ui {
    pub fn new(
        settings: &Settings,
        system: SystemInfo,
        letters: Letters,
        history: History,
        memories: Memories,
        gauge: Gauge,
        pack: Result<Pack, pocketcw_logic::pack::Error>,
        career: Career,
    ) -> Ui {
        Ui {
            theme: Theme::new(settings.style, settings.palette, settings.boot_count),
            boot_count: settings.boot_count,
            screen: Screen::Home,
            dial_focus: None,
            footer: footer::FootLine::new(),
            status: None,
            alert: None,
            badge_shown: "",
            key_guard: KeyGuard::default(),
            decode: Decode::new(),
            shortcut: settings.shortcut,
            list: menu::List::Top,
            cursor: [0; menu::LISTS],
            scroll: 0,
            row: 0,
            editing: false,
            meter: Meter::default(),
            stats: Stats::default(),
            hand: Hand::default(),
            timing_hand: false,
            tape: Tape::new(false),
            contact_since: [None; 2],
            stuck_shown: [false; 2],
            session: None,
            letters,
            letters_saved: letters,
            koch_window: (letters.level, (0, 0)),
            history,
            memories,
            memories_saved: memories,
            mem_row: false,
            spent: [false; 4],
            sending: None,
            stop: false,
            play: None,
            say: None,
            serial_play: false,
            listening: false,
            seed: settings.boot_count as u32 * 7919 + 1,
            system,
            battery: None,
            gauge,
            battery_shown: None,
            now_ms: 0,
            settings_from: None,
            heard: radio::Heard::new(),
            board: radio::Board::new(settings.decoder_board),
            usb_on: settings.usb_out != pocketcw_logic::settings::UsbOut::Off,
            board_command: None,
            pack,
            boxing: None,
            career,
            career_saved: career,
            superfish: false,
        }
    }

    pub fn pack(&self) -> Result<&Pack, pocketcw_logic::pack::Error> {
        self.pack.as_ref().map_err(|&e| e)
    }

    /// Redraws the whole current screen.
    pub fn draw(&mut self, lcd: &mut Lcd, s: &Settings) {
        self.shortcut = s.shortcut;
        self.theme.frame(lcd);
        match self.screen {
            Screen::Home => self.draw_home(lcd, s),
            Screen::Timing => self.draw_timing(lcd, s),
            Screen::Qso => self.draw_qso(lcd, s),
            Screen::Hunt => self.draw_hunt(lcd, s),
            Screen::Menu => self.draw_menu(lcd, s),
            Screen::Settings => self.draw_settings(lcd, s),
            Screen::System => self.draw_system(lcd, s),
            Screen::Receive => self.draw_receive(lcd, s),
            Screen::Ragchew => self.draw_ragchew(lcd, s),
            Screen::Letters => self.draw_letters(lcd, s),
            Screen::Progress => self.draw_progress(lcd, s),
            Screen::Radio => self.draw_radio(lcd, s),
            Screen::Boxing => self.draw_boxing(lcd, s),
        }
        if self.full_screen() {
            return;
        }
        self.battery_shown = self.battery_icon();
        self.theme.battery(lcd, self.battery_shown);
        self.draw_badge(lcd, true);
        self.draw_footer(lcd);
    }

    /// The screen fills the whole panel with its own drawing: no header, footer or badge.
    fn full_screen(&self) -> bool {
        self.screen == Screen::Boxing && self.boxing.as_ref().is_some_and(Boxing::full_screen)
    }

    /// Runs `action` on the current screen at `now_ms`. Changes land in `s`; the caller applies
    /// them.
    pub fn act(&mut self, lcd: &mut Lcd, s: &mut Settings, action: Action, now_ms: u32) {
        self.now_ms = now_ms;
        match action {
            Action::SoftDown(key) => {
                self.spent[key as usize] = self.soft_down(lcd);
                return;
            }
            // A key whose press stopped a memory does nothing more.
            Action::SoftHold(key) if self.spent[key as usize] => return,
            Action::SoftHold(key) => {
                self.spent[key as usize] = self.hold_act(lcd, s, key);
                return;
            }
            Action::Soft(key) if core::mem::take(&mut self.spent[key as usize]) => return,
            _ => {}
        }
        if self.dial_act(lcd, s, action, now_ms) {
            return;
        }
        match self.screen {
            Screen::Home => self.home_act(lcd, s, action, now_ms),
            Screen::Timing => self.timing_act(lcd, s, action),
            Screen::Qso => self.qso_act(lcd, s, action),
            Screen::Hunt => self.hunt_act(lcd, s, action),
            Screen::Menu => self.menu_act(lcd, s, action),
            Screen::Settings => self.settings_act(lcd, s, action),
            Screen::System => self.system_act(lcd, s, action),
            Screen::Receive => self.receive_act(lcd, s, action),
            Screen::Ragchew => self.ragchew_act(lcd, s, action),
            Screen::Letters => self.letters_act(lcd, s, action),
            Screen::Progress => self.progress_act(lcd, s, action),
            Screen::Radio => self.radio_act(lcd, s, action),
            Screen::Boxing => self.boxing_act(lcd, s, action),
        }
    }

    /// An F key went down. A memory that sends stops at once, and that is all the key does.
    /// True when the key is spent.
    fn soft_down(&mut self, lcd: &mut Lcd) -> bool {
        if self.screen != Screen::Home || self.sending.is_none() {
            return false;
        }
        self.stop_sending(lcd);
        true
    }

    /// The second action of a held F key: F1 does the F1 HOLD setting and F4 opens KEYER, on
    /// every screen; F2 or F3 clears what the screen holds, and F3 on KEYER opens RADIO. On the
    /// MEM row each key sends its memory again and again. True when the key has one here.
    fn hold_act(&mut self, lcd: &mut Lcd, s: &mut Settings, key: u8) -> bool {
        if self.mem_row {
            self.memory_key(lcd, s, key as usize, true);
            return true;
        }
        match (key, self.screen) {
            (0, _) => self.shortcut_act(lcd, s),
            (1, Screen::Letters) => {
                self.letters.clear_stats();
                self.draw(lcd, s);
            }
            (1, Screen::Timing) => {
                self.stats = Stats::default();
                self.hand = Hand::default();
                self.tape.new_page();
                self.draw(lcd, s);
            }
            (2, Screen::Settings) if self.editing_text() => self.clear_text(lcd, s),
            (2, Screen::Home) if self.radio_available() => self.open(lcd, s, Screen::Radio),
            (3, screen) if screen != Screen::Home => self.open(lcd, s, Screen::Home),
            _ => return false,
        }
        true
    }

    fn shortcut_act(&mut self, lcd: &mut Lcd, s: &mut Settings) {
        match s.shortcut {
            Shortcut::KeyOut => {
                s.adjust(Field::KeyOut, 1);
                self.key_out_changed(lcd, s);
                let line = match (s.key_out, self.screen_keys_radio()) {
                    (false, _) => "KEY OUT OFF",
                    (true, true) => "KEY OUT ON",
                    (true, false) => "KEY OUT ON: NOT ON THIS SCREEN",
                };
                self.status(lcd, line);
            }
            Shortcut::Resume => match (self.session.as_ref().map(Session::screen), s.last_practice) {
                (Some(screen), _) if screen == self.screen => {}
                (Some(_), _) => self.resume(lcd, s),
                (None, Some(practice)) => self.start(lcd, s, practice),
                (None, None) => self.status(lcd, "NOTHING TO RESUME"),
            },
            Shortcut::Mem => {
                if self.screen != Screen::Home {
                    self.open(lcd, s, Screen::Home);
                }
                self.open_mem_row(lcd);
            }
            Shortcut::Timing if self.screen == Screen::Timing => {}
            Shortcut::Timing => self.open(lcd, s, Screen::Timing),
        }
    }

    /// Your key keys the radio on this screen when KEY OUT is ON. Practice and settings
    /// never key it, so a drill or a memory you key in stays off the air.
    pub fn screen_keys_radio(&self) -> bool {
        matches!(self.screen, Screen::Home | Screen::Timing | Screen::Radio)
    }

    fn keys_radio(&self, s: &Settings) -> bool {
        s.key_out && self.screen_keys_radio()
    }

    /// Shows a new KEY OUT value, and TX in the header of a screen that keys the radio.
    fn key_out_changed(&mut self, lcd: &mut Lcd, s: &Settings) {
        match self.screen {
            Screen::Home => {
                self.draw_home_value(lcd, s, Field::KeyOut);
                self.draw_home_header(lcd, s);
            }
            Screen::Settings | Screen::Timing | Screen::Radio => self.draw(lcd, s),
            _ => {}
        }
    }

    /// The F key labels: `keys` for a press, and this screen's hold labels.
    fn softkeys(&self, lcd: &mut Lcd, keys: [&str; 4]) {
        let f1 = match self.shortcut {
            Shortcut::KeyOut => "KEYOUT",
            Shortcut::Resume => "RESUME",
            Shortcut::Mem => "MEM",
            Shortcut::Timing => "TIMING",
        };
        let holds = match (self.mem_row, self.screen) {
            // While a memory sends, a press or hold only stops it.
            (_, Screen::Home) if self.sending.is_some() => [""; 4],
            (true, _) => ["REPEAT"; 4],
            (false, Screen::Home) => [f1, "", if self.radio_available() { "RADIO" } else { "" }, ""],
            (false, Screen::Letters | Screen::Timing) => [f1, "CLEAR", "", "HOME"],
            (false, Screen::Settings) if self.editing_text() => [f1, "", "CLEAR", "HOME"],
            (false, _) => [f1, "", "", "HOME"],
        };
        self.theme.softkeys(lcd, keys, holds);
    }

    /// Shows a keyer speed set from outside the screen, on the screens that show it.
    pub fn speed_set(&mut self, lcd: &mut Lcd, s: &Settings) {
        match self.screen {
            Screen::Home => self.draw_home_value(lcd, s, Field::Speed),
            Screen::Settings => self.draw_settings(lcd, s),
            _ => {
                if s.rx_wpm == 0 {
                    self.bot_speed_changed(s);
                }
                self.draw_dial_chips(lcd, s);
            }
        }
    }

    /// The bots' base speed changed: their stations move to it, each keeping its own offset.
    fn bot_speed_changed(&mut self, s: &Settings) {
        let options = Self::qso_options(s);
        match &mut self.session {
            Some(Session::Qso(q)) => q.bot.set_options(options, s.wpm),
            Some(Session::Hunt(h)) => h.bot.set_options(options, s.wpm),
            _ => {}
        }
    }

    /// A character read from the paddles.
    pub fn decoded(&mut self, lcd: &mut Lcd, s: &mut Settings, decoded: Decoded) {
        self.hand.char_ended();
        let worst = self.meter.char_worst();
        let span = self.meter.char_span();
        self.decode.push(decoded, worst);
        if let Decoded::Char(_) = decoded {
            self.history.sent();
        }
        match self.screen {
            Screen::Home => {
                // The MEM row covers the decode lines; closing it draws them all.
                if !self.mem_row {
                    self.draw_decode_push(lcd);
                }
                if let Decoded::Char(c) = decoded {
                    self.tape.letter(lcd, &self.theme, c, span);
                }
                self.draw_home_stats(lcd);
            }
            Screen::Timing => {
                if let Decoded::Char(c) = decoded {
                    self.tape.letter(lcd, &self.theme, c, span);
                }
                self.draw_timing_stats(lcd, s);
            }
            Screen::Qso => self.qso_decoded(lcd, s, decoded),
            Screen::Hunt => self.hunt_decoded(lcd, s, decoded),
            Screen::Settings => self.settings_decoded(lcd, s, decoded),
            Screen::Receive => self.receive_decoded(lcd, s, decoded),
            Screen::Ragchew => self.ragchew_decoded(lcd, s, decoded),
            Screen::Boxing => self.boxing_decoded(lcd, s, decoded),
            Screen::Menu | Screen::System | Screen::Letters | Screen::Progress | Screen::Radio => {}
        }
    }

    /// The key or a contact went down or up at `at_ms` on the millisecond tick count, while
    /// decoding used dots of `dit`.
    pub fn edge(&mut self, lcd: &mut Lcd, s: &Settings, line: Line, down: bool, at_ms: u32, dit: u16) {
        let on_tape = matches!(self.screen, Screen::Home | Screen::Timing);
        match line {
            Line::Key => {
                self.key_guard.edge(down, at_ms, dit);
                if down {
                    self.history.active(at_ms);
                }
                // Serial keying has no contact edge, and must still end a memory and its repeats.
                if down && self.screen == Screen::Home {
                    self.stop_sending(lcd);
                }
                if down && on_tape {
                    let paused = self.meter_paused(at_ms, dit);
                    self.tape.key_down(lcd, &self.theme, at_ms, dit, paused, lanes(s));
                }
                self.hand.key(down, at_ms, dit, s.mode != Mode::Straight);
                if let Some(iv) = self.meter.edge(down, at_ms, dit) {
                    self.stats.add(&iv);
                    if on_tape {
                        self.tape_interval(lcd, s, &iv);
                    }
                    if self.screen == Screen::Boxing {
                        self.boxing_interval(&iv);
                    }
                }
                match self.screen {
                    Screen::Qso => self.qso_key(down, at_ms, dit),
                    Screen::Hunt => self.hunt_key(down, at_ms, dit),
                    Screen::Receive => self.receive_key(lcd, down, at_ms, dit),
                    Screen::Ragchew => self.ragchew_key(lcd, s, down, at_ms, dit),
                    Screen::Boxing => self.boxing_key(at_ms),
                    _ => {}
                }
            }
            Line::Tip | Line::Ring => {
                let i = (line == Line::Ring) as usize;
                self.hand.contact(line, down, at_ms);
                if down && self.screen == Screen::Home {
                    self.stop_sending(lcd);
                    if self.mem_row && !self.memories.stay_open {
                        self.close_mem_row(lcd);
                    }
                }
                if self.stuck_shown[i] {
                    self.stuck_shown[i] = false;
                    self.alert(lcd, "");
                }
                if down {
                    self.contact_since[i] = Some(at_ms);
                } else if let Some(since) = self.contact_since[i].take()
                    && self.screen == Screen::Home
                {
                    let lane = if s.mode.dit_on_ring(s.swap) { 1 - i } else { i };
                    self.tape.contact(lcd, &self.theme, lane, since, at_ms, dit, lanes(s));
                    // The first contact of a squeeze to open draws it, from the later close.
                    if let Some(other) = self.contact_since[1 - i] {
                        let both = if ticks::reached(since, other) { since } else { other };
                        self.tape.squeeze(lcd, &self.theme, both, at_ms);
                    }
                }
            }
        }
    }

    fn meter_paused(&self, at_ms: u32, dit: u16) -> bool {
        let (_, last_end) = self.meter.char_span();
        at_ms.wrapping_sub(last_end) as f32 >= PAUSE_UNITS * dit as f32
    }

    fn tape_interval(&mut self, lcd: &mut Lcd, s: &Settings, iv: &Interval) {
        let iambic = s.mode != Mode::Straight;
        if iv.kind.is_mark() {
            self.tape.mark(lcd, &self.theme, iv, iambic, lanes(s));
        } else {
            self.tape.gap(lcd, &self.theme, iv);
        }
    }

    /// Every player went quiet at `now_ms`. A serial PLAY can end here with nothing of the
    /// screen's sounding, so each screen checks that its own transmission was.
    pub fn played(&mut self, lcd: &mut Lcd, s: &Settings, now_ms: u32) {
        self.serial_play = false;
        self.listening = false;
        match self.screen {
            Screen::Qso => self.qso_played(),
            Screen::Hunt => self.hunt_played(now_ms),
            Screen::Receive => self.receive_played(lcd, now_ms),
            Screen::Ragchew => self.ragchew_played(lcd, s, now_ms),
            Screen::Home => self.memory_played(lcd, now_ms),
            Screen::Boxing => self.boxing_played(now_ms),
            _ => {}
        }
    }

    /// The next thing the players should do: a stop first, so a screen's old transmission never
    /// plays on beside its new one.
    pub fn take_command(&mut self, s: &Settings) -> Option<Command> {
        if core::mem::take(&mut self.stop) {
            return Some(Command::Stop);
        }
        let play = self.play.take().or_else(|| self.qso_next_play(s)).or_else(|| self.hunt_next_play(s))?;
        self.listening = matches!(self.screen, Screen::Receive | Screen::Ragchew | Screen::Qso | Screen::Hunt);
        Some(Command::Play(play))
    }

    /// Text for the voice to say now; run it after the players' commands, since a stop also
    /// stops the voice.
    pub fn take_say(&mut self) -> Option<Text<WORD_TEXT>> {
        self.say.take()
    }

    /// The voice started at `now_ms` and lasts `ms`, 0 when it said nothing.
    pub fn said(&mut self, ms: u32, now_ms: u32) {
        if self.screen == Screen::Receive {
            self.receive_said(ms, now_ms);
        }
    }

    /// Stops every player and the voice, and drops a transmission or speech not yet started.
    fn stop_players(&mut self) {
        self.stop = true;
        self.play = None;
        self.say = None;
        self.listening = false;
    }

    /// Band noise plays: practice while it runs, the SOUND tab, so you hear the noise level you
    /// set, and a PLAY from the serial port.
    pub fn band(&self) -> bool {
        let screen = match (self.screen, &self.session) {
            (Screen::Qso | Screen::Hunt, _) => true,
            (Screen::Receive, Some(Session::Receive(rx))) => rx.running(),
            (Screen::Ragchew, Some(Session::Ragchew(c))) => c.running(),
            (Screen::Settings, _) => self.sound_selected(),
            (Screen::Boxing, _) => self.boxing.as_ref().is_some_and(Boxing::fighting),
            _ => false,
        };
        screen || self.serial_play
    }

    /// The sound engine's setup: your settings, with the band of the opponent you are fighting.
    pub fn sound(&self, s: &Settings) -> Sound {
        self.boxing_sound(s).unwrap_or_else(|| s.sound())
    }

    /// Tick count at which `timeout` wants to run.
    pub fn deadline(&self) -> Option<u32> {
        let stuck = (0..2)
            .filter(|&i| !self.stuck_shown[i])
            .filter_map(|i| Some(self.contact_since[i]?.wrapping_add(STUCK_MS[i])))
            .fold(None, earliest);
        [stuck, self.screen_deadline()].into_iter().flatten().fold(None, earliest)
    }

    fn screen_deadline(&self) -> Option<u32> {
        match (self.screen, &self.session) {
            (Screen::Qso, Some(Session::Qso(q))) => q.over_deadline,
            (Screen::Hunt, Some(Session::Hunt(h))) => h.over_deadline.or(h.cq_deadline),
            (Screen::Receive, Some(Session::Receive(rx))) => rx.deadline,
            (Screen::Ragchew, Some(Session::Ragchew(c))) => c.deadline,
            (Screen::Home, _) => self.sending?.again_at,
            (Screen::Boxing, _) => self.boxing.as_ref()?.deadline(),
            _ => None,
        }
    }

    pub fn timeout(&mut self, lcd: &mut Lcd, s: &mut Settings, now_ms: u32) {
        self.now_ms = now_ms;
        self.status_timeout(lcd, now_ms);
        self.draw_badge(lcd, false);
        let passed = |at: u32| ticks::reached(now_ms, at);
        for (i, stuck_ms) in STUCK_MS.into_iter().enumerate() {
            if let Some(since) = self.contact_since[i]
                && !self.stuck_shown[i]
                && passed(since.wrapping_add(stuck_ms))
            {
                self.stuck_shown[i] = true;
                let lane = if s.mode.dit_on_ring(s.swap) { 1 - i } else { i };
                let line = Text::<COLS>::format(format_args!("{} STUCK ON: CHECK PLUG", lanes(s)[1 + lane]));
                self.alert(lcd, line.as_str());
            }
        }
        if !self.screen_deadline().is_some_and(passed) {
            return;
        }
        match self.screen {
            Screen::Qso => self.qso_over_done(lcd, s),
            Screen::Hunt => self.hunt_timeout(lcd, s, now_ms),
            Screen::Receive => self.receive_timeout(lcd, s, now_ms),
            Screen::Ragchew => self.ragchew_timeout(lcd, s, now_ms),
            Screen::Home => self.memory_timeout(lcd, s),
            Screen::Boxing => self.boxing_timeout(lcd, s, now_ms),
            _ => {}
        }
    }

    /// The letter statistics record when it has changed and should be saved now: after you
    /// leave a drill, or at any time with `now`.
    pub fn take_letters(&mut self, now: bool) -> Option<[u8; LETTERS_LEN]> {
        let due = self.letters != self.letters_saved
            && (now || !matches!(self.screen, Screen::Receive | Screen::Ragchew | Screen::Boxing));
        if due {
            self.letters_saved = self.letters;
        }
        due.then(|| self.letters.encode())
    }

    /// The memories record when it has changed and you have left settings, or at any time with
    /// `now`.
    pub fn take_memories(&mut self, now: bool) -> Option<[u8; MEMORIES_LEN]> {
        let due = self.memories != self.memories_saved && (now || self.screen != Screen::Settings);
        if due {
            self.memories_saved = self.memories;
        }
        due.then(|| self.memories.encode())
    }

    /// For the web build of the boxing game: the game, or a screen it opens, shows.
    pub fn in_boxing(&self) -> bool {
        matches!(self.screen, Screen::Boxing | Screen::Settings | Screen::Letters)
    }

    /// For the web build: opens the boxing game.
    pub fn open_boxing(&mut self, lcd: &mut Lcd, s: &Settings) {
        self.open(lcd, s, Screen::Boxing);
    }

    /// True once when SUPERFISH CW was picked: the caller runs it, then calls `superfish_done`.
    pub fn take_superfish(&mut self) -> bool {
        core::mem::take(&mut self.superfish)
    }

    /// Back from SUPERFISH CW: the screen it left shows again.
    pub fn superfish_done(&mut self, lcd: &mut Lcd, s: &Settings) {
        self.draw(lcd, s);
    }

    /// The game's progress record when it has changed and no fight is running, or at any time
    /// with `now`.
    pub fn take_career(&mut self, now: bool) -> Option<[u8; CAREER_LEN]> {
        let fighting = self.screen == Screen::Boxing && self.boxing.as_ref().is_some_and(Boxing::fighting);
        let due = self.career != self.career_saved && (now || !fighting);
        if due {
            self.career_saved = self.career;
        }
        due.then(|| self.career.encode())
    }

    /// This session's history slot and bytes when due for saving; see `History::take_save`.
    pub fn take_history(&mut self, now_ms: u32, force: bool) -> Option<(usize, [u8; SLOT_LEN])> {
        self.history.take_save(now_ms, force)
    }

    fn open(&mut self, lcd: &mut Lcd, s: &Settings, screen: Screen) {
        // SET and then BACK keep the dial focus of the screen you set from.
        let keeps_focus = screen == Screen::Settings || (self.screen == Screen::Settings && self.settings_from == Some(screen));
        if screen != self.screen
            && let Some(session) = &mut self.session
            && session.screen() == self.screen
        {
            session.pause();
            self.stop_players();
        }
        if screen != Screen::Settings {
            self.settings_from = None;
        }
        if self.sending.is_some() {
            self.sending = None;
            self.stop_players();
        }
        // The fight waits, paused, while you are anywhere else.
        if self.screen == Screen::Boxing && screen != Screen::Boxing {
            self.boxing_leave(screen);
            self.stop_players();
        }
        self.mem_row = false;
        self.screen = screen;
        self.editing = false;
        if !keeps_focus {
            self.dial_focus = None;
        }
        self.footer.clear();
        self.tape = Tape::new(screen == Screen::Timing);
        self.draw(lcd, s);
    }

    /// Opens settings at `section`; BACK returns to this screen, and a paused practice goes on.
    fn open_settings_at(&mut self, lcd: &mut Lcd, s: &Settings, section: settings::Section) {
        self.settings_from = Some(self.screen);
        self.select_section(section);
        self.open(lcd, s, Screen::Settings);
    }

    fn close_settings(&mut self, lcd: &mut Lcd, s: &Settings) {
        match self.settings_from {
            Some(screen) if self.session.as_ref().map(Session::screen) == Some(screen) => self.resume(lcd, s),
            Some(Screen::Settings) | None => self.back(lcd, s),
            Some(screen) => self.open(lcd, s, screen),
        }
    }

    /// Opens the paused practice, which takes any new settings and goes on.
    fn resume(&mut self, lcd: &mut Lcd, s: &Settings) {
        match self.session.as_ref().map(Session::screen) {
            Some(Screen::Qso) => self.qso_resume(lcd, s),
            Some(Screen::Hunt) => self.hunt_resume(lcd, s),
            Some(Screen::Receive) => self.receive_resume(lcd, s),
            Some(Screen::Ragchew) => self.ragchew_resume(lcd, s),
            _ => {}
        }
    }

    /// Starts `practice` afresh, and keeps it for RESUME after power on.
    fn start(&mut self, lcd: &mut Lcd, s: &mut Settings, practice: Practice) {
        s.last_practice = Some(practice);
        match practice {
            Practice::Drill(kind) => self.open_receive(lcd, s, kind),
            Practice::Qso(program) => self.open_qso(lcd, s, program),
            Practice::Hunt => self.open_hunt(lcd, s),
            Practice::Chew { quiz } => self.open_ragchew(lcd, s, quiz),
        }
    }

    /// Goes on with `practice` if it is paused, or starts it afresh.
    fn open_practice(&mut self, lcd: &mut Lcd, s: &mut Settings, practice: Practice) {
        if self.session.as_ref().map(Session::practice) == Some(practice) {
            self.resume(lcd, s);
        } else {
            self.start(lcd, s, practice);
        }
    }

    fn qso_options(s: &Settings) -> Options {
        Options { wpm: s.rx_wpm, pileup: s.pileup != Pileup::Off, spread: s.spread, human_fist: s.human_fist }
    }

    fn next_seed(&mut self) -> u32 {
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.seed
    }

    fn open_qso(&mut self, lcd: &mut Lcd, s: &Settings, program: Program) {
        let seed = self.next_seed();
        self.session = Some(Session::Qso(Qso::new(program, s.callsign(), seed, Self::qso_options(s))));
        self.open(lcd, s, Screen::Qso);
    }
}

/// Adds `item` in the first free slot, or drops the oldest to make room at the end.
fn push_scrolling<T: Copy, const N: usize>(slots: &mut [Option<T>; N], item: T) {
    match slots.iter_mut().find(|slot| slot.is_none()) {
        Some(free) => *free = Some(item),
        None => {
            slots.copy_within(1.., 0);
            slots[N - 1] = Some(item);
        }
    }
}

fn earliest(a: Option<u32>, b: u32) -> Option<u32> {
    Some(a.map_or(b, |a| ticks::earlier(a, b)))
}

fn dit_ms(s: &Settings) -> u16 {
    keyer::dit_ms(s.wpm)
}

/// Tape lane labels: the keyer output and the two contacts.
fn lanes(s: &Settings) -> [&'static str; 3] {
    if s.mode == Mode::Straight { ["KEY", "TIP", "RNG"] } else { ["KEY", "DIT", "DAH"] }
}

fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::IambicA => "IAMBIC A",
        Mode::IambicB => "IAMBIC B",
        Mode::Straight => "STRAIGHT",
    }
}

/// Cells a character takes: two for a prosign, one for anything else.
pub(super) fn char_cells(c: u8) -> usize {
    if morse::prosign_letters(c).is_some() { 2 } else { 1 }
}

/// Character `c` in `font`, with a prosign as its two letters under a bar. Returns the width.
pub(super) fn draw_char(lcd: &mut Lcd, font: &Font, x: i32, y: i32, c: u8, fg: Rgb565, bg: Rgb565) -> i32 {
    let w = font.width as i32;
    let Some(letters) = morse::prosign_letters(c) else {
        let _ = font.draw(lcd, Point::new(x, y), morse::text(&c), fg, bg);
        return w;
    };
    let _ = font.draw(lcd, Point::new(x, y), letters, fg, bg);
    // Two rows above the capitals, which start font.height / 8 rows down: 2 in SMALL, 1 in TINY.
    rect(lcd, x + 1, y + font.height as i32 / 8 - 2, 2 * w - 2, 1, fg);
    2 * w
}
