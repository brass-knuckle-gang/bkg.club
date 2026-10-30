//! Settings and memories by name, for GET and SET on the serial port. A setting's value is what
//! its settings row shows, without the unit, so the screen's wording is the only list of values.
// Without a heap the reason can't be boxed; it is a value as long as the values it lists.
#![allow(clippy::result_large_err)]

use core::fmt::Write;

use crate::memories::{self, Memories};
use crate::morse;
use crate::player::MAX_TEXT;
use crate::settings::{Field, Settings, UsbOut};
use crate::text::Text;

/// The longest value a settings row shows.
pub const SHOWN_LEN: usize = 12;
/// Room for a SET error's reason: the values a setting takes, such as all 16 palettes.
pub const REASON_LEN: usize = 160;

/// Each setting's name: its settings row, with `_` for a space.
const SETTINGS: [(&str, Field); 24] = [
    ("MODE", Field::Mode),
    ("PADDLES", Field::Swap),
    ("SPEED", Field::Speed),
    ("KEY_OUT", Field::KeyOut),
    ("OUT_TYPE", Field::Output),
    ("OUT_LINES", Field::OutputSwap),
    ("USB_OUT", Field::UsbOut),
    ("TONE", Field::Tone),
    ("VOLUME", Field::Volume),
    ("WAVE", Field::Wave),
    ("NOISE", Field::Noise),
    ("FADING", Field::Fading),
    ("CHAR_SPEED", Field::RxSpeed),
    ("FARNSWORTH", Field::RxEffective),
    ("CHEW_SHOWS", Field::Reveal),
    ("NINJA_WORDS", Field::NinjaWords),
    ("DECODER_BOARD", Field::DecoderBoard),
    ("PILEUP", Field::Pileup),
    ("FIST", Field::Fist),
    ("SPEED_SPREAD", Field::Spread),
    ("STYLE", Field::Style),
    ("PALETTE", Field::Palette),
    ("BRIGHTNESS", Field::Brightness),
    ("F1_HOLD", Field::Shortcut),
];
const MEMORY_NAMES: [&str; memories::SLOTS] = ["M1", "M2", "M3", "M4"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Name {
    Setting(Field),
    Call,
    /// A memory's text.
    Memory(usize),
    /// Seconds between repeats.
    Every,
    /// The MEM row stays open after a send.
    StayOpen,
}

impl Name {
    pub fn parse(name: &str) -> Option<Name> {
        let slot = |names: &[&str]| names.iter().position(|&n| n == name);
        match name {
            "CALL" => Some(Name::Call),
            "MEM_EVERY" => Some(Name::Every),
            "MEM_STAY_OPEN" => Some(Name::StayOpen),
            _ => SETTINGS
                .iter()
                .find(|(n, _)| *n == name)
                .map(|&(_, field)| Name::Setting(field))
                .or_else(|| slot(&MEMORY_NAMES).map(Name::Memory)),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Name::Setting(field) => SETTINGS.iter().find(|&&(_, f)| f == field).map_or("", |&(n, _)| n),
            Name::Call => "CALL",
            Name::Memory(i) => MEMORY_NAMES[i],
            Name::Every => "MEM_EVERY",
            Name::StayOpen => "MEM_STAY_OPEN",
        }
    }

    /// Every name, in the order a plain GET lists them.
    pub fn all() -> impl Iterator<Item = Name> {
        let slots = 0..memories::SLOTS;
        SETTINGS.iter().map(|&(_, field)| Name::Setting(field)).chain([Name::Call]).chain(slots.map(Name::Memory)).chain([Name::Every, Name::StayOpen])
    }
}

/// A settings row's value without its unit: `21 WPM` is `21`, `700 HZ` is `700`, `80%` is `80`.
pub fn bare(shown: &str) -> &str {
    [" WPM", " HZ", "%"].iter().find_map(|unit| shown.strip_suffix(unit)).unwrap_or(shown)
}

fn on_off(on: bool) -> &'static str {
    if on { "ON" } else { "OFF" }
}

/// The value of `name`. `show` gives what a settings row shows.
pub fn get(name: Name, s: &Settings, m: &Memories, show: impl Fn(&Settings, Field) -> Text<SHOWN_LEN>) -> Text<MAX_TEXT> {
    match name {
        Name::Setting(field) => Text::format(format_args!("{}", bare(show(s, field).as_str()))),
        Name::Call => Text::format(format_args!("{}", s.callsign())),
        Name::Memory(i) => Text::format(format_args!("{}", m.text(i))),
        Name::Every => Text::format(format_args!("{}", m.every_s)),
        Name::StayOpen => Text::format(format_args!("{}", on_off(m.stay_open))),
    }
}

/// Sets `name` to `value`, upper case; on an error, nothing changes and the reason says why.
pub fn set(
    name: Name,
    value: &str,
    s: &mut Settings,
    m: &mut Memories,
    show: impl Fn(&Settings, Field) -> Text<SHOWN_LEN>,
) -> Result<(), Text<REASON_LEN>> {
    let on = match value {
        "ON" => Some(true),
        "OFF" => Some(false),
        _ => None,
    };
    match name {
        Name::Setting(field) => {
            let set = find_value(s, field, value, &show)?;
            // Without USB the computer loses the port this command came in on.
            if set.usb_out == UsbOut::Off && s.usb_out != UsbOut::Off {
                return reason(format_args!("THAT TURNS USB OFF; SET IT ON THE DEVICE"));
            }
            *s = set;
        }
        Name::Call => {
            let mut set = *s;
            while !set.callsign().is_empty() {
                set.pop_callsign();
            }
            if !value.chars().all(|c| set.push_callsign(c)) {
                return reason(format_args!("A TO Z, 0 TO 9 AND /, UP TO 10"));
            }
            *s = set;
        }
        Name::Memory(i) => {
            let mut set = *m;
            set.clear(i);
            for c in morse::chars(value.as_bytes()) {
                if c != b' ' && morse::code(c).is_none() {
                    return reason(format_args!("NO MORSE FOR {}", morse::text(&c)));
                }
                if !set.push(i, c) {
                    return reason(format_args!("UP TO {} CHARACTERS; A PROSIGN TAKES 4", memories::LEN));
                }
            }
            *m = set;
        }
        Name::StayOpen => m.stay_open = on.ok_or_else(|| Text::format(format_args!("ON OR OFF")))?,
        Name::Every => match value.parse::<u8>() {
            Ok(every_s) if (memories::MIN_EVERY_S..=memories::MAX_EVERY_S).contains(&every_s) => m.every_s = every_s,
            _ => return reason(format_args!("{} TO {}", memories::MIN_EVERY_S, memories::MAX_EVERY_S)),
        },
    }
    Ok(())
}

/// More steps than any setting has values: TONE has 91.
const MAX_STEPS: usize = 128;

/// `s` with `field` stepped to the value shown as `value`. Steps from the lowest value of a
/// number, or the current value of a choice, until the shown value stops changing or comes back.
/// The error lists the values passed on the way.
fn find_value(
    s: &Settings,
    field: Field,
    value: &str,
    show: &impl Fn(&Settings, Field) -> Text<SHOWN_LEN>,
) -> Result<Settings, Text<REASON_LEN>> {
    let shown = |t: &Settings| show(t, field);
    if bare(shown(s).as_str()) == value {
        return Ok(*s);
    }
    let mut t = *s;
    // An even step toggles nothing, so ON and OFF settings start where they are.
    t.adjust(field, i8::MIN);
    let start = shown(&t);
    let mut values = Values::default();
    for _ in 0..MAX_STEPS {
        let now = shown(&t);
        if bare(now.as_str()) == value {
            return Ok(t);
        }
        values.add(bare(now.as_str()));
        t.adjust(field, 1);
        if shown(&t) == now || shown(&t) == start {
            break;
        }
    }
    Err(values.finish())
}

fn reason(args: core::fmt::Arguments) -> Result<(), Text<REASON_LEN>> {
    Err(Text::format(args))
}

/// The values a setting takes, with an even run of numbers written as `5 TO 60` or
/// `300 TO 1200 BY 10`.
#[derive(Default)]
struct Values {
    text: Text<REASON_LEN>,
    run: Option<Run>,
    full: bool,
}

#[derive(Clone, Copy)]
struct Run {
    first: u16,
    last: u16,
    /// 0 until the run has two numbers.
    step: u16,
}

impl Values {
    fn add(&mut self, value: &str) {
        let Ok(n) = value.parse::<u16>() else {
            self.end_run();
            self.write(value);
            return;
        };
        match self.run {
            Some(run) if n > run.last && (run.step == 0 || n - run.last == run.step) => {
                self.run = Some(Run { last: n, step: n - run.last, ..run })
            }
            _ => {
                self.end_run();
                self.run = Some(Run { first: n, last: n, step: 0 });
            }
        }
    }

    fn end_run(&mut self) {
        let Some(Run { first, last, step }) = self.run.take() else { return };
        let item: Text<24> = match step {
            0 => Text::format(format_args!("{first}")),
            1 => Text::format(format_args!("{first} TO {last}")),
            _ => Text::format(format_args!("{first} TO {last} BY {step}")),
        };
        self.write(item.as_str());
    }

    /// Adds one item; once one does not fit, the list ends there.
    fn write(&mut self, item: &str) {
        let comma = if self.text.is_empty() { "" } else { ", " };
        if self.full || self.text.len() + comma.len() + item.len() > REASON_LEN {
            self.full = true;
            return;
        }
        let _ = write!(self.text, "{comma}{item}");
    }

    fn finish(mut self) -> Text<REASON_LEN> {
        self.end_run();
        self.text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keyer::Mode;

    /// The settings rows' wording for the fields the tests use.
    fn show(s: &Settings, field: Field) -> Text<SHOWN_LEN> {
        match field {
            Field::Speed => Text::format(format_args!("{} WPM", s.wpm)),
            Field::Tone => Text::format(format_args!("{} HZ", s.tone_hz)),
            Field::Mode => Text::format(format_args!(
                "{}",
                match s.mode {
                    Mode::IambicA => "IAMBIC A",
                    Mode::IambicB => "IAMBIC B",
                    Mode::Straight => "STRAIGHT",
                }
            )),
            Field::KeyOut => Text::format(format_args!("{}", on_off(s.key_out))),
            Field::RxSpeed if s.rx_wpm == 0 => Text::format(format_args!("MATCH")),
            Field::RxSpeed => Text::format(format_args!("{} WPM", s.rx_wpm)),
            Field::DecoderBoard => Text::format(format_args!("{}", on_off(s.decoder_board))),
            Field::UsbOut => Text::format(format_args!("{:?}", s.usb_out)),
            Field::NinjaWords => Text::format(format_args!("{}", s.ninja_words)),
            _ => Text::new(),
        }
    }

    fn setting(s: &mut Settings, name: &str, value: &str) -> Result<(), Text<REASON_LEN>> {
        let mut m = Memories::new("K5MME");
        set(Name::parse(name).unwrap(), value, s, &mut m, show)
    }

    fn reason(text: &str) -> Result<(), Text<REASON_LEN>> {
        Err(Text::format(format_args!("{text}")))
    }

    /// The port these commands come in on needs USB on.
    fn usb_on() -> Settings {
        with(UsbOut::Serial, false)
    }

    fn with(usb_out: UsbOut, decoder_board: bool) -> Settings {
        let mut s = Settings::default();
        (s.usb_out, s.decoder_board) = (usb_out, decoder_board);
        s
    }

    #[test]
    fn names_round_trip_and_list_everything() {
        for name in Name::all() {
            assert_eq!(Name::parse(name.as_str()), Some(name));
        }
        assert_eq!(Name::all().count(), SETTINGS.len() + 1 + memories::SLOTS + 2);
        assert_eq!(Name::parse("SPEED"), Some(Name::Setting(Field::Speed)));
        assert_eq!(Name::parse("M3"), Some(Name::Memory(2)));
        assert_eq!(Name::parse("speed"), None);
        assert_eq!(Name::parse("M5"), None);
    }

    #[test]
    fn numbers_and_choices_set_by_their_shown_value() {
        let mut s = usb_on();
        assert_eq!(setting(&mut s, "SPEED", "35"), Ok(()));
        assert_eq!(setting(&mut s, "TONE", "650"), Ok(()));
        assert_eq!(setting(&mut s, "MODE", "STRAIGHT"), Ok(()));
        assert_eq!(setting(&mut s, "CHAR_SPEED", "MATCH"), Ok(()));
        assert_eq!((s.wpm, s.tone_hz, s.mode, s.rx_wpm), (35, 650, Mode::Straight, 0));
        assert_eq!(setting(&mut s, "CHAR_SPEED", "12"), Ok(()));
        assert_eq!(s.rx_wpm, 12);
        assert_eq!(setting(&mut s, "NINJA_WORDS", "1200"), Ok(()));
        assert_eq!(s.ninja_words, 1200);
        let m = Memories::new("K5MME");
        assert_eq!(get(Name::Setting(Field::Tone), &s, &m, show).as_str(), "650");
    }

    #[test]
    fn a_value_not_offered_lists_the_values() {
        let mut s = usb_on();
        assert_eq!(setting(&mut s, "SPEED", "61"), reason("5 TO 60"));
        assert_eq!(setting(&mut s, "MODE", "IAMBIC C"), reason("STRAIGHT, IAMBIC A, IAMBIC B"));
        assert_eq!(setting(&mut s, "CHAR_SPEED", "50"), reason("MATCH, 5 TO 40"));
        assert_eq!(setting(&mut s, "TONE", "655"), reason("300 TO 1200 BY 10"));
        assert_eq!(setting(&mut s, "NINJA_WORDS", "150"), reason("100 TO 3000 BY 100"));
        assert_eq!(s, usb_on(), "nothing changed");
    }


    #[test]
    fn on_off_settings_set_either_way() {
        let mut s = usb_on();
        assert_eq!(setting(&mut s, "KEY_OUT", "OFF"), Ok(()));
        assert!(!s.key_out);
        assert_eq!(setting(&mut s, "KEY_OUT", "OFF"), Ok(()));
        assert!(!s.key_out);
        assert_eq!(setting(&mut s, "KEY_OUT", "ON"), Ok(()));
        assert!(s.key_out);
    }

    #[test]
    fn nothing_sets_usb_off() {
        let mut s = usb_on();
        assert_eq!(setting(&mut s, "USB_OUT", "Off"), reason("THAT TURNS USB OFF; SET IT ON THE DEVICE"));
        assert_eq!(setting(&mut s, "USB_OUT", "Text"), Ok(()));
        s = with(UsbOut::Radio, true);
        assert!(setting(&mut s, "DECODER_BOARD", "OFF").is_err(), "it would take RADIO with it");
        assert_eq!(setting(&mut s, "DECODER_BOARD", "ON"), Ok(()));
        assert_eq!(s.usb_out, UsbOut::Radio);
    }

    #[test]
    fn call_takes_callsign_characters() {
        let mut s = usb_on();
        assert_eq!(setting(&mut s, "CALL", "K5MME/P"), Ok(()));
        assert_eq!(s.callsign(), "K5MME/P");
        assert_eq!(setting(&mut s, "CALL", "K5-MME"), reason("A TO Z, 0 TO 9 AND /, UP TO 10"));
        assert_eq!(setting(&mut s, "CALL", "K5MMEK5MMEK"), reason("A TO Z, 0 TO 9 AND /, UP TO 10"));
        assert_eq!(s.callsign(), "K5MME/P");
    }

    #[test]
    fn memories_take_morse_text_and_their_options() {
        let (mut s, mut m) = (usb_on(), Memories::new("K5MME"));
        let mut put = |name: &str, value: &str| set(Name::parse(name).unwrap(), value, &mut s, &mut m, show);
        assert_eq!(put("M2", "CQ  TEST <AR>"), Ok(()));
        assert_eq!(put("MEM_EVERY", "9"), Ok(()));
        assert_eq!(put("MEM_STAY_OPEN", "ON"), Ok(()));
        assert_eq!(put("M2", "CQ ~"), reason("NO MORSE FOR ~"));
        assert_eq!(put("M2", &"E".repeat(memories::LEN + 1)), reason("UP TO 48 CHARACTERS; A PROSIGN TAKES 4"));
        assert_eq!(put("MEM_EVERY", "31"), reason("2 TO 30"));
        assert_eq!(put("MEM_STAY_OPEN", "YES"), reason("ON OR OFF"));
        assert_eq!((m.text(1), m.every_s, m.stay_open), ("CQ TEST <AR>", 9, true));
        assert_eq!(get(Name::Memory(1), &s, &m, show).as_str(), "CQ TEST <AR>");
    }

    #[test]
    fn bare_drops_the_unit() {
        assert_eq!([bare("21 WPM"), bare("700 HZ"), bare("80%"), bare("IAMBIC B")], ["21", "700", "80", "IAMBIC B"]);
    }
}
