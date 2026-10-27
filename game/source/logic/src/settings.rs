//! User settings and their EEPROM record.

use crate::drill::{self, FIRST_RUNG, RUNGS};
use crate::keyer::{MAX_WPM, MIN_WPM, Mode, Output};
use crate::qso::Program;
use crate::record::{self, body_mut, frame, is_sealed, unframe};
use crate::synth::{self, Sound};
use crate::volume;

/// How the QSO bot's pileup calls.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pileup {
    /// One hunter at a time.
    Off,
    /// Several hunters call, one after another.
    TakeTurns,
    /// Several hunters call at once.
    Overlap,
}

const PILEUPS: [Pileup; 3] = [Pileup::Off, Pileup::TakeTurns, Pileup::Overlap];

/// What the USB port sends a computer, which sees the device as a keyboard and a serial port
/// unless this is Off. The value is the one stored.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UsbOut {
    /// No USB device: the port only charges.
    Off = 0,
    /// Each character you key, typed.
    Text = 1,
    /// Your keying as VBand's Ctrl keys.
    Vband = 2,
    /// Each character the decoder board hears, typed. Offered only with the board on.
    Radio = 3,
    /// The serial port only: nothing typed.
    Serial = 4,
}

/// What holding F1 does on every screen. Stored as its index in `SHORTCUTS`, which only grows
/// at its end.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shortcut {
    /// Turns KEY OUT on or off.
    KeyOut,
    /// Opens the paused practice.
    Resume,
    /// Opens KEYER with the MEM row open.
    Mem,
    Timing,
}

pub const SHORTCUTS: [Shortcut; 4] = [Shortcut::KeyOut, Shortcut::Resume, Shortcut::Mem, Shortcut::Timing];

/// A practice that RESUME starts again after power on. Stored as its index in `PRACTICES` plus
/// one, with 0 for none; the table only grows at its end.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Practice {
    Drill(drill::Kind),
    Qso(Program),
    Hunt,
    Chew { quiz: bool },
}

const PRACTICES: [Practice; 10] = [
    Practice::Drill(drill::Kind::Koch),
    Practice::Drill(drill::Kind::Words),
    Practice::Drill(drill::Kind::Calls),
    Practice::Drill(drill::Kind::Ninja),
    Practice::Drill(drill::Kind::Head),
    Practice::Qso(Program::Pota),
    Practice::Qso(Program::Sota),
    Practice::Hunt,
    Practice::Chew { quiz: false },
    Practice::Chew { quiz: true },
];

impl Practice {
    pub fn name(self) -> &'static str {
        match self {
            Practice::Drill(kind) => kind.name(),
            Practice::Qso(program) => program.name(),
            Practice::Hunt => "HUNT",
            Practice::Chew { quiz: true } => "QUIZ",
            Practice::Chew { quiz: false } => "CHEW",
        }
    }
}

/// In the order the setting steps through. RADIO is last, so the steps without the board are
/// all but the last.
const USB_OUTS: [UsbOut; 5] = [UsbOut::Off, UsbOut::Text, UsbOut::Vband, UsbOut::Serial, UsbOut::Radio];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Settings {
    pub wpm: u8,
    pub mode: Mode,
    pub swap: bool,
    /// Level from `volume`: 0 mutes, `MAX_VOLUME` is +4.5 dB.
    pub volume: u8,
    pub tone_hz: u16,
    /// Key the "func" jack along with the sidetone.
    pub key_out: bool,
    pub output: Output,
    /// Exchange the jack's tip and ring.
    pub output_swap: bool,
    /// Counts power-ons; picks the boot screen flag.
    pub boot_count: u16,
    /// UI style, below `STYLE_COUNT`.
    pub style: u8,
    /// UI palette, below `PALETTE_COUNT`.
    pub palette: u8,
    /// Uppercase letters, digits and '/', padded with zeros.
    callsign: [u8; CALLSIGN_LEN],
    /// Character speed of what the device sends you; 0 matches your keyer speed.
    pub rx_wpm: u8,
    /// Farnsworth speed: gaps stretch so text arrives at this rate; 0 is off.
    pub rx_effective: u8,
    pub pileup: Pileup,
    /// QSO bot: hunters key with human timing jitter.
    pub human_fist: bool,
    /// QSO bot: hunters send at different speeds around the receive speed.
    pub spread: bool,
    /// Ragchew listening shows each line as it ends; otherwise the whole over at its end.
    pub reveal_lines: bool,
    /// Backlight level, `MIN_BRIGHTNESS..=MAX_BRIGHTNESS` tenths.
    pub brightness: u8,
    /// Tones are squares with soft corners instead of sines.
    pub square: bool,
    /// Band noise under what the device sends you, 0 (off) to `synth::MAX_LEVEL`.
    pub noise: u8,
    /// How deep received stations fade, 0 (off) to `synth::MAX_LEVEL`.
    pub fading: u8,
    /// A Pro decoder board is fitted: power it and show the RADIO screen.
    pub decoder_board: bool,
    pub usb_out: UsbOut,
    /// NINJA picks among the first this many pack words, a multiple of `NINJA_WORDS_STEP`.
    pub ninja_words: u16,
    pub shortcut: Shortcut,
    /// The practice last opened; None until one is.
    pub last_practice: Option<Practice>,
    /// The HEAD rung, from `FIRST_RUNG`.
    pub head_rung: u8,
}

pub const MAX_VOLUME: u8 = volume::MAX_LEVEL;
pub const MIN_TONE_HZ: u16 = 300;
pub const MAX_TONE_HZ: u16 = 1200;
pub const STYLE_COUNT: u8 = 3;
pub const PALETTE_COUNT: u8 = 16;
pub const MIN_RX_WPM: u8 = 5;
pub const MAX_RX_WPM: u8 = 40;
pub const TONE_STEP_HZ: u16 = 10;
pub const CALLSIGN_LEN: usize = 10;
/// Above zero, so the screen can't be dimmed to black.
pub const MIN_BRIGHTNESS: u8 = 1;
pub const MAX_BRIGHTNESS: u8 = 10;
pub const NINJA_WORDS_STEP: u16 = 100;
pub const MIN_NINJA_WORDS: u16 = NINJA_WORDS_STEP;
/// About the words in the content pack.
pub const MAX_NINJA_WORDS: u16 = 3000;
/// The amateur radio placeholder call, until the user enters theirs.
const DEFAULT_CALLSIGN: &[u8] = b"N0CALL";
const MODES: [Mode; 3] = [Mode::IambicA, Mode::IambicB, Mode::Straight];

/// One setting the user can change.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    Speed,
    Mode,
    Swap,
    KeyOut,
    Output,
    OutputSwap,
    Tone,
    Volume,
    Style,
    Palette,
    RxSpeed,
    RxEffective,
    Pileup,
    Fist,
    Spread,
    Reveal,
    Brightness,
    Wave,
    Noise,
    Fading,
    DecoderBoard,
    UsbOut,
    NinjaWords,
    Shortcut,
}

impl Default for Settings {
    fn default() -> Self {
        let mut callsign = [0; CALLSIGN_LEN];
        callsign[..DEFAULT_CALLSIGN.len()].copy_from_slice(DEFAULT_CALLSIGN);
        Settings {
            wpm: 20, mode: Mode::IambicB, swap: false, volume: 23, tone_hz: 700, key_out: true, output: Output::Straight,
            output_swap: false, boot_count: 0, style: 0,
            palette: 0, callsign, rx_wpm: 0, rx_effective: 0, pileup: Pileup::TakeTurns, human_fist: true,
            spread: true, reveal_lines: false, brightness: MAX_BRIGHTNESS, square: false, noise: 0, fading: 0,
            decoder_board: false, usb_out: UsbOut::Off, ninja_words: 500, shortcut: Shortcut::KeyOut,
            last_practice: None, head_rung: FIRST_RUNG,
        }
    }
}

const MAGIC: [u8; 2] = *b"Rw";
const VERSION: u8 = 1;
/// Records before frames: magic, version 1..=7, the body as long as that version had it, checksum.
const LEGACY_MAGIC: [u8; 2] = *b"RW";
const LEGACY_OVERHEAD: usize = 4;
/// Legacy versions 1 and 2 stored the codec's output register, which is 11 levels below today's
/// scale.
const OLD_VOLUME_OFFSET: u8 = 11;
const VOLUME_SCALE_VERSION: u8 = 3;
/// Body lengths: version 1 ended after the boot count, version 2 after the palette, and each
/// later version appended fields.
const V1_BODY: usize = 8;
const V2_BODY: usize = 10;
const V3_BODY: usize = V2_BODY + CALLSIGN_LEN;
const V4_BODY: usize = V3_BODY + 3;
const V5_BODY: usize = V4_BODY + 1;
const V6_BODY: usize = V5_BODY + 3;
const BODY_LEN: usize = V6_BODY + 1;
/// The save count follows the fields of version 7. Later fields follow the count, so older
/// firmware still finds its count.
const COUNT_AT: usize = BODY_LEN;
const NINJA_AT: usize = COUNT_AT + 1;
const SHORTCUT_AT: usize = NINJA_AT + 1;
const PRACTICE_AT: usize = SHORTCUT_AT + 1;
const RUNG_AT: usize = PRACTICE_AT + 1;
pub const RECORD_LEN: usize = RUNG_AT + 1 + record::OVERHEAD;
/// Settings are kept in two copies and saved to them in turn, so a save cut off by power loss
/// leaves the other copy whole. A save with count `n` goes to copy `n % COPIES`.
pub const COPIES: usize = 2;
/// QSO flags: bit 0 a pileup, then human fist, speed spread, ragchew line reveal, and bit 4 a
/// pileup that overlaps.
const OVERLAP_BIT: u8 = 1 << 4;
/// Hardware flags: bit 0 the decoder board, bits 1 to 3 the USB OUT value. Bit 3 was always
/// saved as 0 before SERIAL, so older saves read the same.
const USB_OUT_SHIFT: u8 = 1;
const USB_OUT_MASK: u8 = 0b111;

impl Settings {
    /// Body: wpm, mode, flags, volume, tone (LE), boot count (LE; the boot count record overrides
    /// it once that exists), style, palette, callsign, receive speed, effective speed, QSO and
    /// ragchew flags, brightness, sound flags (bit 0 square), noise, fading, hardware flags, save
    /// count, NINJA words in hundreds, the F1 hold's index in `SHORTCUTS`, the last practice's
    /// index in `PRACTICES` plus one, the HEAD rung.
    pub fn encode(&self, count: u8) -> [u8; RECORD_LEN] {
        let mode = match self.mode {
            Mode::IambicA => 0,
            Mode::IambicB => 1,
            Mode::Straight => 2,
        };
        let paddle = self.output == Output::Paddle;
        let flags = self.swap as u8 | (self.key_out as u8) << 1 | (paddle as u8) << 2 | (self.output_swap as u8) << 3;
        let [tone_lo, tone_hi] = self.tone_hz.to_le_bytes();
        let [boot_lo, boot_hi] = self.boot_count.to_le_bytes();
        let mut record = [0; RECORD_LEN];
        let body = body_mut(&mut record);
        body[..V2_BODY].copy_from_slice(&[
            self.wpm, mode, flags, self.volume, tone_lo, tone_hi, boot_lo, boot_hi, self.style, self.palette,
        ]);
        body[V2_BODY..V3_BODY].copy_from_slice(&self.callsign);
        let overlap = if self.pileup == Pileup::Overlap { OVERLAP_BIT } else { 0 };
        let qso = (self.pileup != Pileup::Off) as u8
            | overlap
            | (self.human_fist as u8) << 1
            | (self.spread as u8) << 2
            | (self.reveal_lines as u8) << 3;
        body[V3_BODY..BODY_LEN].copy_from_slice(&[
            self.rx_wpm,
            self.rx_effective,
            qso,
            self.brightness,
            self.square as u8,
            self.noise,
            self.fading,
            self.decoder_board as u8 | (self.usb_out as u8) << USB_OUT_SHIFT,
        ]);
        body[COUNT_AT] = count;
        body[NINJA_AT] = (self.ninja_words / NINJA_WORDS_STEP) as u8;
        body[SHORTCUT_AT] = SHORTCUTS.iter().position(|&c| c == self.shortcut).unwrap_or(0) as u8;
        body[PRACTICE_AT] = self.last_practice.and_then(|p| PRACTICES.iter().position(|&q| q == p)).map_or(0, |i| i as u8 + 1);
        body[RUNG_AT] = self.head_rung;
        frame(&mut record, MAGIC, VERSION);
        record
    }

    /// The newest settings in the copies, and the count for the next save. A copy without a count
    /// was written by firmware that saved one copy, after any save of this firmware, so it is the
    /// newest.
    pub fn load(copies: [&[u8]; COPIES]) -> (Option<Settings>, u8) {
        let [a, b] = copies.map(Self::decode_copy);
        let newest = match (a, b) {
            (Some(a), Some(b)) if b_is_newer(a.1, b.1) => Some((b, 1)),
            (Some(a), _) => Some((a, 0)),
            (None, Some(b)) => Some((b, 1)),
            (None, None) => None,
        };
        match newest {
            Some(((settings, count), copy)) => (Some(settings), count.unwrap_or(copy).wrapping_add(1)),
            None => (None, 0),
        }
    }

    /// The settings in the record at the start of `slot`; None for a blank or foreign record, a
    /// bad check, or a value out of range.
    pub fn decode(slot: &[u8]) -> Option<Settings> {
        Self::decode_copy(slot).map(|(settings, _)| settings)
    }

    /// The settings and the save count, which records before copies lack.
    fn decode_copy(slot: &[u8]) -> Option<(Settings, Option<u8>)> {
        if let Some(body) = unframe(slot, MAGIC, VERSION) {
            return Self::from_body(body, false).map(|s| (s, body.get(COUNT_AT).copied()));
        }
        let version = *slot.get(2)?;
        let body_len = match version {
            1 => V1_BODY,
            2 => V2_BODY,
            3 => V3_BODY,
            4 => V4_BODY,
            5 => V5_BODY,
            6 => V6_BODY,
            7 => BODY_LEN,
            _ => return None,
        };
        let len = body_len + LEGACY_OVERHEAD;
        let old = slot.get(..len).filter(|old| old[..2] == LEGACY_MAGIC && is_sealed(old))?;
        Self::from_body(&old[3..len - 1], version < VOLUME_SCALE_VERSION).map(|s| (s, None))
    }

    /// Fields from the front of `body`; a field past its end takes the default, so shorter bodies
    /// from older records keep the values they have.
    fn from_body(body: &[u8], old_volume: bool) -> Option<Settings> {
        let len = body.len();
        if len < V1_BODY {
            return None;
        }
        let defaults = Settings::default();
        let (style, palette) = if len >= V2_BODY { (body[8], body[9]) } else { (defaults.style, defaults.palette) };
        let mut callsign = defaults.callsign;
        if len >= V3_BODY {
            callsign.copy_from_slice(&body[V2_BODY..V3_BODY]);
        }
        let (rx_wpm, rx_effective, qso) = if len >= V4_BODY {
            (body[V3_BODY], body[V3_BODY + 1], body[V3_BODY + 2])
        } else {
            let d = &defaults;
            (d.rx_wpm, d.rx_effective, (d.pileup != Pileup::Off) as u8 | (d.human_fist as u8) << 1 | (d.spread as u8) << 2)
        };
        let brightness = if len >= V5_BODY { body[V4_BODY] } else { defaults.brightness };
        let (sound_flags, noise, fading) = if len >= V6_BODY {
            (body[V5_BODY], body[V5_BODY + 1], body[V5_BODY + 2])
        } else {
            (defaults.square as u8, defaults.noise, defaults.fading)
        };
        // Clamped, so a value from later firmware keeps the rest of the record.
        let ninja_words = body.get(NINJA_AT).map_or(defaults.ninja_words, |&hundreds| {
            (hundreds as u16 * NINJA_WORDS_STEP).clamp(MIN_NINJA_WORDS, MAX_NINJA_WORDS)
        });
        // A value from later firmware takes the default and keeps the rest of the record.
        let shortcut = body.get(SHORTCUT_AT).and_then(|&i| SHORTCUTS.get(i as usize)).copied().unwrap_or(defaults.shortcut);
        let last_practice = body.get(PRACTICE_AT).and_then(|&i| PRACTICES.get((i as usize).checked_sub(1)?)).copied();
        let rungs = FIRST_RUNG..=RUNGS.len() as u8;
        let head_rung = body.get(RUNG_AT).copied().filter(|r| rungs.contains(r)).unwrap_or(FIRST_RUNG);
        let hardware = if len >= BODY_LEN { body[V6_BODY] } else { defaults.decoder_board as u8 };
        let stored = hardware >> USB_OUT_SHIFT & USB_OUT_MASK;
        // A value from later firmware turns USB off instead of dropping the whole record.
        let usb_out = USB_OUTS.iter().copied().find(|&u| u as u8 == stored).unwrap_or(UsbOut::Off);
        let pileup = match (qso & 1 != 0, qso & OVERLAP_BIT != 0) {
            (false, _) => Pileup::Off,
            (true, false) => Pileup::TakeTurns,
            (true, true) => Pileup::Overlap,
        };
        let mode = match body[1] {
            0 => Mode::IambicA,
            1 => Mode::IambicB,
            2 => Mode::Straight,
            _ => return None,
        };
        let settings = Settings {
            wpm: body[0],
            mode,
            swap: body[2] & 1 != 0,
            key_out: body[2] & 2 != 0,
            output: if body[2] & 4 != 0 { Output::Paddle } else { Output::Straight },
            output_swap: body[2] & 8 != 0,
            volume: if old_volume { body[3].saturating_add(OLD_VOLUME_OFFSET) } else { body[3] },
            tone_hz: u16::from_le_bytes([body[4], body[5]]),
            boot_count: u16::from_le_bytes([body[6], body[7]]),
            style,
            palette,
            callsign,
            rx_wpm,
            rx_effective,
            pileup,
            human_fist: qso & 2 != 0,
            spread: qso & 4 != 0,
            reveal_lines: qso & 8 != 0,
            brightness,
            square: sound_flags & 1 != 0,
            noise,
            fading,
            decoder_board: hardware & 1 != 0,
            usb_out,
            ninja_words,
            shortcut,
            last_practice,
            head_rung,
        };
        let in_range = (MIN_WPM..=MAX_WPM).contains(&settings.wpm)
            && settings.volume <= MAX_VOLUME
            && (MIN_TONE_HZ..=MAX_TONE_HZ).contains(&settings.tone_hz)
            && settings.style < STYLE_COUNT
            && settings.palette < PALETTE_COUNT
            && valid_callsign(&settings.callsign)
            && (MIN_BRIGHTNESS..=MAX_BRIGHTNESS).contains(&brightness)
            && noise <= synth::MAX_LEVEL
            && fading <= synth::MAX_LEVEL
            && [rx_wpm, rx_effective].iter().all(|&w| w == 0 || (MIN_RX_WPM..=MAX_RX_WPM).contains(&w));
        in_range.then_some(settings)
    }
}

/// Whether copy B holds newer settings than copy A, from their save counts.
fn b_is_newer(a: Option<u8>, b: Option<u8>) -> bool {
    match (a, b) {
        (None, _) => false,
        (Some(_), None) => true,
        (Some(a), Some(b)) => (b.wrapping_sub(a) as i8) > 0,
    }
}

impl Settings {
    /// Steps `field` by `delta`: numbers stop at their limits, choices wrap around.
    pub fn adjust(&mut self, field: Field, delta: i8) {
        let d = delta as i32;
        match field {
            Field::Speed => self.wpm = (self.wpm as i32 + d).clamp(MIN_WPM as i32, MAX_WPM as i32) as u8,
            Field::Mode => {
                let i = MODES.iter().position(|&m| m == self.mode).unwrap_or(0);
                self.mode = MODES[(i as i32 + d).rem_euclid(MODES.len() as i32) as usize];
            }
            Field::Swap => self.swap ^= d % 2 != 0,
            Field::KeyOut => self.key_out ^= d % 2 != 0,
            Field::Output if d % 2 != 0 => {
                self.output = if self.output == Output::Straight { Output::Paddle } else { Output::Straight }
            }
            Field::Output => {}
            Field::OutputSwap => self.output_swap ^= d % 2 != 0,
            Field::Tone => {
                let hz = self.tone_hz as i32 + d * TONE_STEP_HZ as i32;
                self.tone_hz = hz.clamp(MIN_TONE_HZ as i32, MAX_TONE_HZ as i32) as u16;
            }
            Field::Volume => self.volume = (self.volume as i32 + d).clamp(0, MAX_VOLUME as i32) as u8,
            Field::Style => self.style = (self.style as i32 + d).rem_euclid(STYLE_COUNT as i32) as u8,
            Field::Palette => self.palette = (self.palette as i32 + d).rem_euclid(PALETTE_COUNT as i32) as u8,
            Field::RxSpeed => self.rx_wpm = step_optional(self.rx_wpm, d),
            Field::RxEffective => self.rx_effective = step_optional(self.rx_effective, d),
            Field::Pileup => {
                let i = PILEUPS.iter().position(|&p| p == self.pileup).unwrap_or(0);
                self.pileup = PILEUPS[(i as i32 + d).rem_euclid(PILEUPS.len() as i32) as usize];
            }
            Field::Wave => self.square ^= d % 2 != 0,
            Field::Noise => self.noise = (self.noise as i32 + d).clamp(0, synth::MAX_LEVEL as i32) as u8,
            Field::Fading => self.fading = (self.fading as i32 + d).clamp(0, synth::MAX_LEVEL as i32) as u8,
            Field::Fist => self.human_fist ^= d % 2 != 0,
            Field::Spread => self.spread ^= d % 2 != 0,
            Field::Reveal => self.reveal_lines ^= d % 2 != 0,
            Field::DecoderBoard => {
                self.decoder_board ^= d % 2 != 0;
                if !self.decoder_board && self.usb_out == UsbOut::Radio {
                    self.usb_out = UsbOut::Off;
                }
            }
            Field::UsbOut => {
                let offered = if self.decoder_board { USB_OUTS.len() } else { USB_OUTS.len() - 1 };
                let i = USB_OUTS.iter().position(|&u| u == self.usb_out).unwrap_or(0);
                self.usb_out = USB_OUTS[(i as i32 + d).rem_euclid(offered as i32) as usize];
            }
            Field::Shortcut => {
                let i = SHORTCUTS.iter().position(|&c| c == self.shortcut).unwrap_or(0);
                self.shortcut = SHORTCUTS[(i as i32 + d).rem_euclid(SHORTCUTS.len() as i32) as usize];
            }
            Field::NinjaWords => {
                let words = self.ninja_words as i32 + d * NINJA_WORDS_STEP as i32;
                self.ninja_words = words.clamp(MIN_NINJA_WORDS as i32, MAX_NINJA_WORDS as i32) as u16;
            }
            Field::Brightness => {
                self.brightness = (self.brightness as i32 + d).clamp(MIN_BRIGHTNESS as i32, MAX_BRIGHTNESS as i32) as u8
            }
        }
    }

    /// Character speed and Farnsworth speed for what the device sends you, from a base speed
    /// of `base` when the receive speed matches it. The effective speed is never above the
    /// character speed.
    pub fn receive_speed(&self, base: u8) -> (u8, u8) {
        let wpm = if self.rx_wpm == 0 { base } else { self.rx_wpm };
        (wpm, self.farnsworth(wpm))
    }

    /// What the sound engine needs from these settings.
    pub fn sound(&self) -> Sound {
        Sound { square: self.square, noise: self.noise, fading: self.fading, tone_hz: self.tone_hz }
    }

    /// The pitch of a station sending `offset_hz` from your tone, kept in the tone range.
    pub fn station_pitch(&self, offset_hz: i16) -> u16 {
        (self.tone_hz as i32 + offset_hz as i32).clamp(MIN_TONE_HZ as i32, MAX_TONE_HZ as i32) as u16
    }

    /// The Farnsworth speed for text sent at `wpm`.
    pub fn farnsworth(&self, wpm: u8) -> u8 {
        if self.rx_effective == 0 { wpm } else { self.rx_effective.min(wpm) }
    }

    /// Steps the character speed from the dial. MATCH steps from the speed it matches, and the
    /// dial never sets MATCH.
    pub fn step_receive(&mut self, delta: i8) {
        let (wpm, _) = self.receive_speed(self.wpm);
        self.rx_wpm = (wpm as i32 + delta as i32).clamp(MIN_RX_WPM as i32, MAX_RX_WPM as i32) as u8;
    }

    /// Steps the Farnsworth speed from the dial, up to the character speed, where it is off.
    pub fn step_farnsworth(&mut self, delta: i8) {
        let (wpm, effective) = self.receive_speed(self.wpm);
        let to = (effective as i32 + delta as i32).clamp(MIN_RX_WPM as i32, wpm.max(MIN_RX_WPM) as i32) as u8;
        self.rx_effective = if to >= wpm { 0 } else { to.min(MAX_RX_WPM) };
    }
}

/// Steps a receive speed where 0 means "off" or "match" and sits just below the minimum.
fn step_optional(wpm: u8, delta: i32) -> u8 {
    let from = if wpm == 0 { MIN_RX_WPM as i32 - 1 } else { wpm as i32 };
    let to = from + delta;
    if to < MIN_RX_WPM as i32 { 0 } else { to.min(MAX_RX_WPM as i32) as u8 }
}

impl Settings {
    pub fn callsign(&self) -> &str {
        let len = self.callsign.iter().position(|&b| b == 0).unwrap_or(CALLSIGN_LEN);
        core::str::from_utf8(&self.callsign[..len]).unwrap_or("")
    }

    /// Appends a callsign character; false when it is not allowed or the call is full.
    pub fn push_callsign(&mut self, c: char) -> bool {
        let len = self.callsign().len();
        let c = c.to_ascii_uppercase();
        if len == CALLSIGN_LEN || !callsign_char(c as u8) {
            return false;
        }
        self.callsign[len] = c as u8;
        true
    }

    pub fn pop_callsign(&mut self) {
        if let Some(len) = self.callsign().len().checked_sub(1) {
            self.callsign[len] = 0;
        }
    }
}

fn callsign_char(b: u8) -> bool {
    b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'/'
}

/// Characters first, then only padding.
fn valid_callsign(call: &[u8]) -> bool {
    let len = call.iter().position(|&b| b == 0).unwrap_or(call.len());
    call[..len].iter().all(|&b| callsign_char(b)) && call[len..].iter().all(|&b| b == 0)
}

const BOOT_MAGIC: [u8; 2] = *b"Rc";
const BOOT_VERSION: u8 = 1;
/// Body: count LE. Short enough for one EEPROM page, so the write at every startup can't tear it.
pub const BOOT_RECORD_LEN: usize = 2 + record::OVERHEAD;
/// Before frames: magic, count LE, checksum.
const LEGACY_BOOT_MAGIC: [u8; 2] = *b"RC";
const LEGACY_BOOT_LEN: usize = 5;

pub fn encode_boot_count(count: u16) -> [u8; BOOT_RECORD_LEN] {
    let mut record = [0; BOOT_RECORD_LEN];
    body_mut(&mut record).copy_from_slice(&count.to_le_bytes());
    frame(&mut record, BOOT_MAGIC, BOOT_VERSION);
    record
}

/// The count in the record at the start of `slot`; None for a blank or foreign record or a bad
/// check.
pub fn decode_boot_count(slot: &[u8]) -> Option<u16> {
    if let Some(body) = unframe(slot, BOOT_MAGIC, BOOT_VERSION) {
        return Some(u16::from_le_bytes(*body.first_chunk()?));
    }
    let old = slot.get(..LEGACY_BOOT_LEN).filter(|old| old[..2] == LEGACY_BOOT_MAGIC && is_sealed(old))?;
    Some(u16::from_le_bytes([old[2], old[3]]))
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::seal;

    /// The record firmware before frames wrote at `version`: this body cut to `body_len`.
    fn legacy(s: &Settings, version: u8, body_len: usize) -> [u8; 64] {
        let framed = s.encode(0);
        let mut old = [0xFF; 64];
        old[..3].copy_from_slice(&[b'R', b'W', version]);
        old[3..3 + body_len].copy_from_slice(&framed[4..4 + body_len]);
        seal(&mut old[..body_len + LEGACY_OVERHEAD]);
        old
    }

    fn at(wpm: u8) -> Settings {
        Settings { wpm, ..Settings::default() }
    }

    #[test]
    fn saves_alternate_and_never_overwrite_the_newest() {
        let mut copies = [[0xFF; RECORD_LEN]; COPIES];
        for wpm in 10..20 {
            let (before, count) = Settings::load(copies.each_ref().map(|c| &c[..]));
            copies[count as usize % COPIES] = at(wpm).encode(count);
            assert_eq!(Settings::load(copies.each_ref().map(|c| &c[..])).0, Some(at(wpm)));
            assert_eq!(Settings::decode(&copies[(count as usize + 1) % COPIES]), before, "the other copy");
        }
    }

    #[test]
    fn a_save_cut_off_leaves_the_other_copy() {
        let mut torn = at(25).encode(1);
        torn[10] ^= 0xFF;
        let (settings, count) = Settings::load([&at(18).encode(0), &torn]);
        assert_eq!(settings, Some(at(18)));
        assert_eq!(count % COPIES as u8, 1, "the next save rewrites the torn copy");
    }

    #[test]
    fn the_newest_copy_wins_across_the_count_wrap() {
        assert_eq!(Settings::load([&at(18).encode(254), &at(25).encode(255)]), (Some(at(25)), 0));
        assert_eq!(Settings::load([&at(18).encode(0), &at(25).encode(255)]), (Some(at(18)), 1));
    }

    #[test]
    fn a_copy_from_firmware_with_one_copy_is_the_newest() {
        let counted = at(18).encode(7);
        let mut framed = [0xFF; RECORD_LEN];
        let one_copy = &mut framed[..BODY_LEN + record::OVERHEAD];
        body_mut(one_copy).copy_from_slice(&at(25).encode(0)[4..4 + BODY_LEN]);
        frame(one_copy, MAGIC, VERSION);
        assert_eq!(Settings::load([&framed, &counted]), (Some(at(25)), 1));
        assert_eq!(Settings::load([&counted, &framed]), (Some(at(25)), 2));
        assert_eq!(Settings::load([&legacy(&at(25), 7, BODY_LEN), &counted]), (Some(at(25)), 1));
        assert_eq!(Settings::load([&[0xFF; RECORD_LEN], &[0xFF; RECORD_LEN]]), (None, 0));
    }

    #[test]
    fn station_pitch_stays_in_the_tone_range() {
        let s = Settings { tone_hz: 400, ..Settings::default() };
        assert_eq!(s.station_pitch(-50), 350);
        assert_eq!(s.station_pitch(-200), MIN_TONE_HZ);
        let s = Settings { tone_hz: MAX_TONE_HZ, ..Settings::default() };
        assert_eq!(s.station_pitch(100), MAX_TONE_HZ);
    }

    #[test]
    fn round_trip() {
        let mut s = Settings {
            wpm: 32, mode: Mode::Straight, swap: true, volume: 7, tone_hz: 650, key_out: false, output: Output::Paddle,
            output_swap: true, boot_count: 513, style: 2,
            palette: 9, ..Settings::default()
        };
        while !s.callsign().is_empty() {
            s.pop_callsign();
        }
        for c in "kd9xyz/p".chars() {
            assert!(s.push_callsign(c));
        }
        assert!(!s.push_callsign('#'));
        assert_eq!(s.callsign(), "KD9XYZ/P");
        assert_eq!(Settings::decode(&s.encode(0)), Some(s));
    }

    #[test]
    fn version_1_record_migrates() {
        // A version 1 record as the first firmware wrote it, followed by erased EEPROM.
        let mut v1 = [0xFF; RECORD_LEN];
        v1[..11].copy_from_slice(&[b'R', b'W', 1, 27, 0, 0b11, 9, 0x8A, 0x02, 0x05, 0x00]);
        seal(&mut v1[..12]);
        let expected = Settings {
            wpm: 27, mode: Mode::IambicA, swap: true, key_out: true, volume: 9 + 11, tone_hz: 650, boot_count: 5,
            ..Settings::default()
        };
        assert_eq!(Settings::decode(&v1), Some(expected));
    }

    #[test]
    fn version_2_volume_keeps_its_loudness() {
        let v2 = legacy(&Settings { volume: 0x0C, style: 1, palette: 4, ..Settings::default() }, 2, V2_BODY);
        let s = Settings::decode(&v2).unwrap();
        assert_eq!((s.volume, s.style, s.palette, s.callsign()), (0x0C + 11, 1, 4, "N0CALL"));
    }

    #[test]
    fn version_3_keeps_volume_and_callsign_and_gets_receive_defaults() {
        let v3 = legacy(&Settings { volume: 30, rx_wpm: 12, pileup: Pileup::Off, ..Settings::default() }, 3, V3_BODY);
        let s = Settings::decode(&v3).unwrap();
        assert_eq!((s.volume, s.callsign(), s.rx_wpm, s.pileup), (30, "N0CALL", 0, Pileup::TakeTurns));
    }

    #[test]
    fn version_4_gets_full_brightness() {
        let v4 = legacy(&Settings { rx_wpm: 12, brightness: 3, ..Settings::default() }, 4, V4_BODY);
        let s = Settings::decode(&v4).unwrap();
        assert_eq!((s.rx_wpm, s.brightness), (12, MAX_BRIGHTNESS));
        let mut dim = s;
        dim.adjust(Field::Brightness, -20);
        assert_eq!(dim.brightness, MIN_BRIGHTNESS);
        assert_eq!(Settings::decode(&dim.encode(0)), Some(dim));
    }

    #[test]
    fn version_5_keeps_its_pileup_and_gets_quiet_sines() {
        let v5 = Settings { pileup: Pileup::Off, brightness: 4, square: true, noise: 7, ..Settings::default() };
        let s = Settings::decode(&legacy(&v5, 5, V5_BODY)).unwrap();
        assert_eq!((s.pileup, s.brightness, s.square, s.noise, s.fading), (Pileup::Off, 4, false, 0, 0));
    }

    #[test]
    fn version_6_keeps_its_sound_and_gets_no_decoder_board() {
        let v6 = legacy(&Settings { square: true, noise: 7, fading: 3, decoder_board: true, ..Settings::default() }, 6, V6_BODY);
        let s = Settings::decode(&v6).unwrap();
        assert_eq!((s.square, s.noise, s.fading, s.decoder_board), (true, 7, 3, false));
    }

    #[test]
    fn version_7_reads_whole() {
        let s = Settings { decoder_board: true, wpm: 31, ..Settings::default() };
        assert_eq!(Settings::decode(&legacy(&s, 7, BODY_LEN)), Some(s));
    }

    #[test]
    fn a_frame_with_fields_from_later_firmware_reads_what_it_knows() {
        let s = Settings { wpm: 31, square: true, ..Settings::default() };
        let mut slot = [0xFF; 64];
        let longer = &mut slot[..NINJA_AT + 1 + 3 + record::OVERHEAD];
        body_mut(longer)[..NINJA_AT + 1].copy_from_slice(&s.encode(0)[4..4 + NINJA_AT + 1]);
        frame(longer, MAGIC, VERSION);
        assert_eq!(Settings::decode(&slot), Some(s));
    }

    #[test]
    fn ninja_words_step_round_trip_and_follow_the_save_count() {
        let mut s = Settings::default();
        s.adjust(Field::NinjaWords, 3);
        assert_eq!(s.ninja_words, 800);
        s.adjust(Field::NinjaWords, -20);
        assert_eq!(s.ninja_words, MIN_NINJA_WORDS);
        s.adjust(Field::NinjaWords, 100);
        assert_eq!(s.ninja_words, MAX_NINJA_WORDS);
        let record = s.encode(9);
        assert_eq!(Settings::decode(&record), Some(s));
        assert_eq!(record[record::BODY_START + COUNT_AT], 9, "where older firmware reads the count");
        let mut before = [0xFF; 64];
        let older = &mut before[..NINJA_AT + record::OVERHEAD];
        body_mut(older)[..NINJA_AT].copy_from_slice(&Settings::default().encode(0)[4..4 + NINJA_AT]);
        frame(older, MAGIC, VERSION);
        assert_eq!(Settings::decode(&before).map(|s| s.ninja_words), Some(500), "a save from before the field");
        let mut later = s.encode(0);
        later[record::BODY_START + NINJA_AT] = 200;
        frame(&mut later, MAGIC, VERSION);
        assert_eq!(Settings::decode(&later), Some(s), "a value past the range clamps");
    }

    #[test]
    fn shortcut_steps_round_trips_and_reads_unknown_as_key_out() {
        let mut s = Settings { wpm: 31, ..Settings::default() };
        for expected in [Shortcut::Resume, Shortcut::Mem, Shortcut::Timing, Shortcut::KeyOut] {
            s.adjust(Field::Shortcut, 1);
            assert_eq!(s.shortcut, expected);
        }
        s.adjust(Field::Shortcut, -1);
        assert_eq!(Settings::decode(&s.encode(0)), Some(s));
        let mut record = s.encode(0);
        body_mut(&mut record)[SHORTCUT_AT] = 200;
        frame(&mut record, MAGIC, VERSION);
        assert_eq!(Settings::decode(&record), Some(Settings { shortcut: Shortcut::KeyOut, ..s }));
    }

    #[test]
    fn last_practice_and_head_rung_round_trip_and_read_unknown_as_defaults() {
        for (i, &practice) in PRACTICES.iter().enumerate() {
            let s = Settings { last_practice: Some(practice), head_rung: 1 + i as u8 % RUNGS.len() as u8, ..Settings::default() };
            assert_eq!(Settings::decode(&s.encode(0)), Some(s));
        }
        let s = Settings { last_practice: Some(Practice::Hunt), head_rung: 3, ..Settings::default() };
        let mut later = s.encode(0);
        body_mut(&mut later)[PRACTICE_AT] = PRACTICES.len() as u8 + 1;
        body_mut(&mut later)[RUNG_AT] = RUNGS.len() as u8 + 1;
        frame(&mut later, MAGIC, VERSION);
        assert_eq!(Settings::decode(&later), Some(Settings { last_practice: None, head_rung: FIRST_RUNG, ..s }));
        let mut before = [0xFF; 64];
        let older = &mut before[..PRACTICE_AT + record::OVERHEAD];
        body_mut(older).copy_from_slice(&s.encode(0)[record::BODY_START..record::BODY_START + PRACTICE_AT]);
        frame(older, MAGIC, VERSION);
        let read = Settings::decode(&before).map(|s| (s.last_practice, s.head_rung));
        assert_eq!(read, Some((None, FIRST_RUNG)), "a save from before the fields");
    }

    #[test]
    fn decoder_board_toggles_and_round_trips() {
        let mut s = Settings::default();
        s.adjust(Field::DecoderBoard, 1);
        assert!(s.decoder_board);
        assert_eq!(Settings::decode(&s.encode(0)), Some(s));
    }

    #[test]
    fn usb_out_steps_round_trips_and_keeps_the_decoder_board() {
        let mut s = Settings { decoder_board: true, ..Settings::default() };
        let steps = [(1, UsbOut::Text), (1, UsbOut::Vband), (1, UsbOut::Serial), (1, UsbOut::Radio), (1, UsbOut::Off), (-1, UsbOut::Radio)];
        for (delta, expected) in steps {
            s.adjust(Field::UsbOut, delta);
            assert_eq!(s.usb_out, expected);
            assert_eq!(Settings::decode(&s.encode(0)), Some(s));
        }
        let v7 = legacy(&Settings { decoder_board: true, ..Settings::default() }, 7, BODY_LEN);
        assert_eq!(Settings::decode(&v7).map(|s| (s.decoder_board, s.usb_out)), Some((true, UsbOut::Off)));
    }

    #[test]
    fn usb_out_keeps_its_stored_values() {
        for (usb_out, stored) in [(UsbOut::Off, 0), (UsbOut::Text, 1), (UsbOut::Vband, 2), (UsbOut::Radio, 3), (UsbOut::Serial, 4)] {
            let s = Settings { usb_out, decoder_board: true, ..Settings::default() };
            let mut record = s.encode(0);
            assert_eq!(crate::record::body_mut(&mut record)[V6_BODY], 1 | stored << USB_OUT_SHIFT);
        }
    }

    #[test]
    fn an_unknown_usb_out_reads_as_off_and_keeps_the_rest() {
        let s = Settings { wpm: 31, decoder_board: true, ..Settings::default() };
        let mut record = s.encode(0);
        body_mut(&mut record)[V6_BODY] = 1 | 7 << USB_OUT_SHIFT;
        frame(&mut record, MAGIC, VERSION);
        assert_eq!(Settings::decode(&record), Some(Settings { usb_out: UsbOut::Off, ..s }));
    }

    #[test]
    fn usb_out_offers_radio_only_with_the_decoder_board() {
        let mut s = Settings { usb_out: UsbOut::Serial, ..Settings::default() };
        s.adjust(Field::UsbOut, 1);
        assert_eq!(s.usb_out, UsbOut::Off);
        s.adjust(Field::UsbOut, -1);
        assert_eq!(s.usb_out, UsbOut::Serial);
        s.adjust(Field::DecoderBoard, 1);
        s.adjust(Field::UsbOut, 1);
        assert_eq!(s.usb_out, UsbOut::Radio);
        s.adjust(Field::DecoderBoard, 1);
        assert_eq!(s.usb_out, UsbOut::Off);
    }

    #[test]
    fn sound_levels_stop_at_their_ends_and_pileup_wraps() {
        let mut s = Settings::default();
        s.adjust(Field::Noise, -1);
        s.adjust(Field::Fading, 20);
        assert_eq!((s.noise, s.fading), (0, synth::MAX_LEVEL));
        s.adjust(Field::Pileup, 1);
        assert_eq!(s.pileup, Pileup::Overlap);
        assert_eq!(Settings::decode(&s.encode(0)), Some(s));
        s.adjust(Field::Pileup, 1);
        assert_eq!(s.pileup, Pileup::Off);
        s.adjust(Field::Wave, 1);
        assert_eq!(Settings::decode(&s.encode(0)), Some(s));
        let mut loud = s.encode(0);
        body_mut(&mut loud)[V5_BODY + 1] = synth::MAX_LEVEL + 1;
        frame(&mut loud, MAGIC, VERSION);
        assert_eq!(Settings::decode(&loud), None);
    }

    #[test]
    fn receive_speeds_step_through_off() {
        let mut s = Settings { wpm: 20, ..Settings::default() };
        assert_eq!(s.receive_speed(s.wpm), (20, 20));
        s.adjust(Field::RxSpeed, 1);
        assert_eq!(s.rx_wpm, MIN_RX_WPM);
        s.adjust(Field::RxSpeed, -1);
        assert_eq!(s.rx_wpm, 0);
        s.adjust(Field::RxSpeed, -1);
        assert_eq!(s.rx_wpm, 0);
        s.rx_wpm = 18;
        s.rx_effective = 25;
        assert_eq!(s.receive_speed(s.wpm), (18, 18));
        s.rx_effective = 10;
        assert_eq!(s.receive_speed(s.wpm), (18, 10));
        assert_eq!(Settings::decode(&s.encode(0)), Some(s));
    }

    #[test]
    fn the_dial_steps_receive_speeds_from_what_you_hear() {
        let mut s = Settings { wpm: 20, ..Settings::default() };
        s.step_receive(-1);
        assert_eq!((s.rx_wpm, s.receive_speed(s.wpm)), (19, (19, 19)));
        s.step_farnsworth(1);
        assert_eq!(s.rx_effective, 0);
        s.step_farnsworth(-2);
        assert_eq!(s.receive_speed(s.wpm), (19, 17));
        s.step_farnsworth(2);
        assert_eq!(s.rx_effective, 0);
        s.rx_wpm = MIN_RX_WPM;
        s.step_receive(-1);
        assert_eq!(s.rx_wpm, MIN_RX_WPM);
        s.rx_wpm = 0;
        s.wpm = 50;
        s.step_receive(1);
        assert_eq!(s.rx_wpm, MAX_RX_WPM);
    }

    #[test]
    fn theme_out_of_range_is_rejected() {
        let s = Settings { palette: PALETTE_COUNT, ..Settings::default() };
        assert_eq!(Settings::decode(&s.encode(0)), None);
    }

    #[test]
    fn adjust_clamps_numbers_and_wraps_choices() {
        let mut s = Settings { wpm: MAX_WPM, tone_hz: MIN_TONE_HZ, volume: 0, ..Settings::default() };
        s.adjust(Field::Speed, 1);
        s.adjust(Field::Tone, -1);
        s.adjust(Field::Volume, -1);
        assert_eq!((s.wpm, s.tone_hz, s.volume), (MAX_WPM, MIN_TONE_HZ, 0));
        s.adjust(Field::Tone, 2);
        assert_eq!(s.tone_hz, MIN_TONE_HZ + 2 * TONE_STEP_HZ);

        s.mode = Mode::Straight;
        s.adjust(Field::Mode, 1);
        assert_eq!(s.mode, Mode::IambicA);
        s.adjust(Field::Palette, -1);
        assert_eq!(s.palette, PALETTE_COUNT - 1);
        s.adjust(Field::Swap, -1);
        assert!(s.swap);
        s.adjust(Field::Reveal, 1);
        assert!(s.reveal_lines);
        assert_eq!(Settings::decode(&s.encode(0)), Some(s));
    }

    #[test]
    fn blank_eeprom_is_rejected() {
        assert_eq!(Settings::decode(&[0xFF; RECORD_LEN]), None);
        assert_eq!(Settings::decode(&[0x00; RECORD_LEN]), None);
    }

    #[test]
    fn corrupted_byte_is_rejected() {
        let mut record = Settings::default().encode(0);
        body_mut(&mut record)[2] ^= 0x04;
        assert_eq!(Settings::decode(&record), None);
    }

    #[test]
    fn boot_count_record_round_trips_and_rejects_blank() {
        assert_eq!(decode_boot_count(&encode_boot_count(513)), Some(513));
        assert_eq!(decode_boot_count(&[0xFF; BOOT_RECORD_LEN]), None);
        let mut record = encode_boot_count(7);
        body_mut(&mut record)[0] ^= 1;
        assert_eq!(decode_boot_count(&record), None);
        let mut old = [b'R', b'C', 0x01, 0x02, 0];
        seal(&mut old);
        assert_eq!(decode_boot_count(&old), Some(0x0201));
    }

}
