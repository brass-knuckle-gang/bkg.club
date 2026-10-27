//! The WinKeyer host protocol (K1EL WK2, firmware 23), so logging programs can key through the
//! serial port. Bytes below 0x20 are commands, each with a fixed number of parameter bytes;
//! anything else is text to send. Commands this device has no use for are read and dropped, so
//! the bytes after them stay in step.

use crate::morse;
use crate::sender::Item;

/// What host open answers: WK2 firmware 23.
pub const REVISION: u8 = 23;
/// Load Defaults (0x0F) sends this many values.
const DEFAULTS_LEN: usize = 15;
/// Lowest speed the speed pot reports, until the host sets it.
const POT_MIN_WPM: u8 = 5;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    None,
    /// Send this byte back to the host.
    Reply(u8),
    /// The host took over: answer `REVISION`.
    Open,
    Close,
    Queue(Item),
    Clear,
    Backspace,
    Pause(bool),
    /// Hold the key down, or let it go.
    Tune(bool),
    /// Set the keyer speed.
    Speed(u8),
    /// Send `status`.
    Status,
    /// Send `speed_pot` for the keyer speed.
    SpeedPot,
}

pub struct Winkeyer {
    open: bool,
    /// The host asked for WK2 mode, which reports the pushbuttons.
    wk2: bool,
    mode: u8,
    pot_min: u8,
    /// The command whose parameters are still coming: its code, with an admin command as
    /// 0x100 plus its number.
    command: Option<u16>,
    params: [u8; DEFAULTS_LEN],
    got: usize,
    need: usize,
    /// Parameter bytes still to drop.
    skip: u16,
}

impl Default for Winkeyer {
    fn default() -> Self {
        Self::new()
    }
}

/// Parameter bytes after each command code 0x00 to 0x1F. Load Defaults (0x0F) keeps only the
/// values it uses.
const PARAMS: [u8; 32] = [1, 1, 1, 1, 2, 3, 1, 0, 0, 1, 0, 1, 1, 1, 1, 7, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1, 2, 1, 1, 0, 0];
const ADMIN: u16 = 0x100;

impl Winkeyer {
    pub const fn new() -> Self {
        Winkeyer { open: false, wk2: false, mode: 0, pot_min: POT_MIN_WPM, command: None, params: [0; DEFAULTS_LEN], got: 0, need: 0, skip: 0 }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Mid-command: the next byte belongs to it, whatever it is.
    pub fn is_busy(&self) -> bool {
        self.command.is_some() || self.skip > 0
    }

    /// F1 to F4 are WinKeyer's pushbuttons PB1 to PB4, for the host's macros.
    pub fn reports_pushbuttons(&self) -> bool {
        self.open && self.wk2
    }

    /// Echo each character sent from the host, once keyed.
    pub fn serial_echo(&self) -> bool {
        self.mode & 1 << 2 != 0
    }

    /// Echo each character keyed on the paddles.
    pub fn paddle_echo(&self) -> bool {
        self.mode & 1 << 6 != 0
    }

    /// Back to closed, as after power on.
    pub fn close(&mut self) {
        *self = Winkeyer::new();
    }

    pub fn push(&mut self, byte: u8) -> Action {
        if self.skip > 0 {
            self.skip -= 1;
            return Action::None;
        }
        if let Some(command) = self.command {
            self.params[self.got] = byte;
            self.got += 1;
            if self.got < self.need {
                return Action::None;
            }
            self.command = None;
            return self.finish(command);
        }
        match byte {
            0x00 => self.start(0x00, 1),
            // Closed, only admin commands count.
            _ if !self.open => Action::None,
            0x01..0x20 => self.start(byte as u16, PARAMS[byte as usize] as usize),
            _ => item(byte).map_or(Action::None, Action::Queue),
        }
    }

    fn start(&mut self, command: u16, need: usize) -> Action {
        if need == 0 {
            return self.finish(command);
        }
        (self.command, self.got, self.need) = (Some(command), 0, need);
        Action::None
    }

    fn finish(&mut self, command: u16) -> Action {
        let [p0, p1, ..] = self.params;
        match command {
            0x00 => self.admin(p0),
            0x02 => Action::Speed(p0),
            0x05 => {
                self.pot_min = p0;
                Action::None
            }
            0x06 => Action::Pause(p0 != 0),
            0x07 => Action::SpeedPot,
            0x08 => Action::Backspace,
            0x0A => Action::Clear,
            0x0B => Action::Tune(p0 != 0),
            0x0E => {
                self.mode = p0;
                Action::None
            }
            0x0F => {
                // Mode, speed, sidetone, weight, lead-in, tail, then the speed pot's lowest WPM;
                // the 8 values after it are dropped.
                self.mode = p0;
                self.pot_min = self.params[6];
                self.skip = (DEFAULTS_LEN - PARAMS[0x0F] as usize) as u16;
                Action::Speed(p1)
            }
            0x15 => Action::Status,
            0x16 if p0 == 3 => {
                self.skip = 1;
                Action::None
            }
            0x1B => match (morse_char(p0), morse_char(p1)) {
                (Some(first), Some(second)) => Action::Queue(Item::Merge { first, second, echo: None }),
                _ => Action::None,
            },
            0x1C => Action::Queue(Item::Speed(p0)),
            0x1E => Action::Queue(Item::CancelSpeed),
            ADMIN_ECHO => Action::Reply(p0),
            _ => Action::None,
        }
    }

    fn admin(&mut self, number: u8) -> Action {
        match number {
            // Hosts often close first, to start from a known state.
            1 | 3 if !self.open => Action::None,
            1 | 3 => {
                self.close();
                Action::Close
            }
            2 => {
                self.open = true;
                Action::Open
            }
            4 => self.start(ADMIN_ECHO, 1),
            5..=7 | 21 | 24 => Action::Reply(0),
            9 => Action::Reply(REVISION),
            10 | 11 => {
                self.wk2 = number == 11;
                Action::None
            }
            23 => Action::Reply(0),
            13 => {
                self.skip = 256;
                Action::None
            }
            14 | 15 | 22 | 25 => {
                self.skip = 1;
                Action::None
            }
            19 => {
                self.skip = 2;
                Action::None
            }
            _ => Action::None,
        }
    }

    /// The status byte: busy sending, the paddles in use, and the queue over 2/3 full.
    pub fn status(busy: bool, break_in: bool, xoff: bool) -> u8 {
        0xC0 | (busy as u8) << 2 | (break_in as u8) << 1 | xoff as u8
    }

    /// The pushbutton status byte, with pushbutton `held` (0 for PB1 to 3 for PB4) down. A
    /// WinKeyer reports one press at a time.
    pub fn pushbutton_status(held: Option<usize>) -> u8 {
        const BITS: [u8; 4] = [1 << 0, 1 << 1, 1 << 2, 1 << 4];
        0xC8 | held.map_or(0, |i| BITS[i])
    }

    /// The speed pot byte for keyer speed `wpm`: its steps above the pot's lowest speed.
    pub fn speed_pot(&self, wpm: u8) -> u8 {
        0x80 | wpm.saturating_sub(self.pot_min).min(31)
    }
}

/// Admin Echo Test: its one parameter comes back.
const ADMIN_ECHO: u16 = ADMIN + 4;

/// The `morse` character for a letter, digit or sign the host sends as itself.
fn morse_char(byte: u8) -> Option<u8> {
    let c = byte.to_ascii_uppercase();
    morse::code(c).filter(|_| c >= b' ').map(|_| c)
}

/// The prosign each ASCII sign stands for, as two letters merged; signs not here with no code of
/// their own are ignored.
const SIGNS: [(u8, [u8; 2]); 17] = [
    (b'"', *b"RR"),
    (b'$', *b"SX"),
    (b'\'', *b"WG"),
    (b'(', *b"KN"),
    (b')', *b"KK"),
    (b'+', *b"AR"),
    (b'-', *b"DU"),
    (b'/', *b"DN"),
    (b':', *b"KN"),
    (b';', *b"AA"),
    (b'<', *b"AR"),
    (b'=', *b"BT"),
    (b'>', *b"SK"),
    (b'@', *b"AC"),
    (b'[', *b"AS"),
    (b'\\', *b"DN"),
    (b']', *b"KN"),
];

/// What a text byte from the host keys. Each echoes the byte itself once keyed.
fn item(byte: u8) -> Option<Item> {
    let echo = Some(byte);
    if byte == b'|' {
        return Some(Item::HalfDot);
    }
    if byte == b' ' {
        return Some(Item::Char { c: b' ', echo });
    }
    if let Some(&(_, [first, second])) = SIGNS.iter().find(|(sign, _)| *sign == byte) {
        return Some(Item::Merge { first, second, echo });
    }
    // Lower case is not in WinKeyer's table: it ignores everything above 0x5D but `|`.
    if byte > b']' {
        return None;
    }
    morse_char(byte).map(|c| Item::Char { c, echo })
}

/// The byte paddle echo sends for a character the decoder read: WinKeyer's sign for a prosign
/// that has one.
pub fn paddle_byte(c: u8) -> Option<u8> {
    let code = morse::code(c)?;
    if c >= b' ' {
        return Some(c);
    }
    SIGNS.iter().find(|(_, [a, b])| morse::code(*a).zip(morse::code(*b)).is_some_and(|(a, b)| a.len() + b.len() == code.len() && code.starts_with(a) && code.ends_with(b))).map(|(sign, _)| *sign)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    fn actions(wk: &mut Winkeyer, bytes: &[u8]) -> Vec<Action> {
        bytes.iter().map(|&b| wk.push(b)).filter(|a| *a != Action::None).collect()
    }

    fn opened() -> Winkeyer {
        let mut wk = Winkeyer::new();
        assert_eq!(actions(&mut wk, &[0x00, 0x02]), [Action::Open]);
        wk
    }

    #[test]
    fn closed_takes_only_admin_commands() {
        let mut wk = Winkeyer::new();
        assert_eq!(actions(&mut wk, b"CQ\x02\x14"), []);
        assert_eq!(actions(&mut wk, &[0x00, 0x04, 0x41, 0x00, 0x09]), [Action::Reply(0x41), Action::Reply(REVISION)]);
        assert_eq!(actions(&mut wk, &[0x00, 0x03]), [], "already closed");
        assert!(!wk.is_open());
    }

    #[test]
    fn open_queues_text_and_close_ends_it() {
        let mut wk = opened();
        let queued = actions(&mut wk, b"K5 ?");
        let char_of = |c: u8| Action::Queue(Item::Char { c, echo: Some(c) });
        assert_eq!(queued, [char_of(b'K'), char_of(b'5'), char_of(b' '), char_of(b'?')]);
        assert_eq!(actions(&mut wk, &[0x00, 0x03]), [Action::Close]);
        assert_eq!(actions(&mut wk, b"E"), []);
    }

    #[test]
    fn signs_merge_into_prosigns_and_lower_case_is_ignored() {
        let mut wk = opened();
        assert_eq!(actions(&mut wk, b"<"), [Action::Queue(Item::Merge { first: b'A', second: b'R', echo: Some(b'<') })]);
        assert_eq!(actions(&mut wk, b"|"), [Action::Queue(Item::HalfDot)]);
        assert_eq!(actions(&mut wk, b"e"), []);
        assert_eq!(actions(&mut wk, &[0x1B, b'S', b'K']), [Action::Queue(Item::Merge { first: b'S', second: b'K', echo: None })]);
    }

    #[test]
    fn parameters_are_read_so_the_text_after_stays_in_step() {
        let mut wk = opened();
        // Sidetone, weight, PTT, pot setup, pin config, first extension, ratio, pointer with nulls.
        let commands = [0x01, 0x05, 0x03, 0x32, 0x04, 0x01, 0xA0, 0x05, 0x0A, 0x1F, 0x00, 0x09, 0x06, 0x10, 0x05, 0x17, 0x32, 0x16, 0x03, 0x45];
        let mut bytes = commands.to_vec();
        bytes.push(b'E');
        assert_eq!(actions(&mut wk, &bytes), [Action::Queue(Item::Char { c: b'E', echo: Some(b'E') })]);
        assert_eq!(wk.speed_pot(25), 0x80 | 15, "pot from 10 WPM");
    }

    #[test]
    fn load_defaults_sets_mode_speed_and_pot() {
        let mut wk = opened();
        let defaults = [0x44, 22, 5, 50, 0, 0, 12, 20, 0, 0, 0, 50, 50, 0, 0];
        let mut bytes = std::vec![0x0F];
        bytes.extend(defaults);
        bytes.push(b'T');
        let got = actions(&mut wk, &bytes);
        assert_eq!(got, [Action::Speed(22), Action::Queue(Item::Char { c: b'T', echo: Some(b'T') })]);
        assert!(wk.serial_echo() && wk.paddle_echo());
        assert_eq!(wk.speed_pot(20), 0x80 | 8);
    }

    #[test]
    fn immediate_commands() {
        let mut wk = opened();
        let got = actions(&mut wk, &[0x02, 30, 0x06, 1, 0x07, 0x08, 0x0A, 0x0B, 1, 0x15, 0x1C, 15, 0x1E, 0x0E, 0x04]);
        assert_eq!(
            got,
            [
                Action::Speed(30),
                Action::Pause(true),
                Action::SpeedPot,
                Action::Backspace,
                Action::Clear,
                Action::Tune(true),
                Action::Status,
                Action::Queue(Item::Speed(15)),
                Action::Queue(Item::CancelSpeed),
            ]
        );
        assert!(wk.serial_echo() && !wk.paddle_echo());
    }

    #[test]
    fn admin_eeprom_load_is_skipped_whole() {
        let mut wk = Winkeyer::new();
        let mut bytes = std::vec![0x00, 13];
        bytes.extend([0x00; 256]);
        bytes.extend([0x00, 0x02]);
        assert_eq!(actions(&mut wk, &bytes), [Action::Open]);
    }

    #[test]
    fn status_and_paddle_bytes() {
        assert_eq!(Winkeyer::status(false, false, false), 0xC0);
        assert_eq!(Winkeyer::status(true, true, true), 0xC7);
        assert_eq!(paddle_byte(b'K'), Some(b'K'));
        let ar = morse::chars(b"<AR>").next().unwrap();
        assert_eq!(paddle_byte(ar), Some(b'+'), "the first sign for AR");
        let hh = morse::chars(b"<HH>").next().unwrap();
        assert_eq!(paddle_byte(hh), None);
    }

    #[test]
    fn wk2_mode_reports_pushbuttons_until_wk1_mode_or_close() {
        let mut wk = opened();
        assert!(!wk.reports_pushbuttons());
        assert_eq!(actions(&mut wk, &[0x00, 0x0B]), []);
        assert!(wk.reports_pushbuttons());
        assert_eq!(actions(&mut wk, &[0x00, 0x0A]), []);
        assert!(!wk.reports_pushbuttons());
        actions(&mut wk, &[0x00, 0x0B, 0x00, 0x03]);
        assert!(!wk.reports_pushbuttons());
        assert_eq!([None, Some(0), Some(1), Some(2), Some(3)].map(Winkeyer::pushbutton_status), [0xC8, 0xC9, 0xCA, 0xCC, 0xD8]);
    }
}
