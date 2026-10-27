//! The Pro's decoder board link: frames the board sends, the host's replies, and the host's
//! commands. The rules follow the vendor parser (V5.03 at 0x08005D00).

use crate::text::Text;

/// What a frame from the board says.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Event {
    /// `RE+IF`: the board announced itself. `present` when the next byte is `1`; `flag` when the
    /// byte after that is `1` too (its use is unknown).
    Hello { present: bool, flag: bool },
    /// `RE+MD0`, `RE+MD1`; use unknown.
    Mode(u8),
    /// `RE+FL0` to `RE+FL2`.
    Filter(u8),
    /// `RE+NR0` to `RE+NR6`.
    NoiseReduction(u8),
    /// A known code that needs no action, or a known code with a digit out of range.
    Ignored,
    /// A frame without the `RE+` prefix. The vendor drops these without a reply.
    Foreign,
    /// `RE+` and a code the vendor does not know.
    Unknown,
}

pub const MAX_FILTER: u8 = 2;
pub const MAX_NOISE_REDUCTION: u8 = 6;
/// The vendor volume scale for `AT+VL`.
pub const MAX_VOLUME: u8 = 63;

pub fn parse(frame: &[u8]) -> Event {
    let Some(rest) = frame.strip_prefix(b"RE+") else {
        return Event::Foreign;
    };
    let digit = |max: u8| match rest.get(2) {
        Some(&d) if (b'0'..=b'0' + max).contains(&d) => Some(d - b'0'),
        _ => None,
    };
    match rest.get(..2) {
        Some(b"IF") => Event::Hello { present: rest.get(2) == Some(&b'1'), flag: rest.get(2..4) == Some(b"11") },
        Some(b"MD") => digit(1).map_or(Event::Ignored, Event::Mode),
        Some(b"FL") => digit(MAX_FILTER).map_or(Event::Ignored, Event::Filter),
        Some(b"NR") => digit(MAX_NOISE_REDUCTION).map_or(Event::Ignored, Event::NoiseReduction),
        Some(b"MC" | b"AC" | b"MU" | b"VL") => Event::Ignored,
        _ => Event::Unknown,
    }
}

impl Event {
    /// The host's answer, if any.
    pub fn reply(self) -> Option<&'static [u8]> {
        match self {
            Event::Hello { .. } => Some(b"AT+OK\r\n"),
            Event::Unknown => Some(b"AT+ERR\r\n"),
            _ => None,
        }
    }
}

pub type Command = Text<12>;

pub fn filter(level: u8) -> Command {
    Text::format(format_args!("AT+FL{}\r\n", level.min(MAX_FILTER)))
}

pub fn noise_reduction(level: u8) -> Command {
    Text::format(format_args!("AT+NR{}\r\n", level.min(MAX_NOISE_REDUCTION)))
}

/// `volume` on the vendor scale, 0 to `MAX_VOLUME`.
pub fn volume(volume: u8) -> Command {
    Text::format(format_args!("AT+VL{:03}\r\n", volume.min(MAX_VOLUME)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_parse_as_the_vendor_reads_them() {
        assert_eq!(parse(b"RE+IF11"), Event::Hello { present: true, flag: true });
        assert_eq!(parse(b"RE+IF10"), Event::Hello { present: true, flag: false });
        assert_eq!(parse(b"RE+IF0"), Event::Hello { present: false, flag: false });
        assert_eq!(parse(b"RE+MD1"), Event::Mode(1));
        assert_eq!(parse(b"RE+FL2"), Event::Filter(2));
        assert_eq!(parse(b"RE+NR6"), Event::NoiseReduction(6));
        assert_eq!(parse(b"RE+NR7"), Event::Ignored, "out of range: no change");
        assert_eq!(parse(b"RE+FL"), Event::Ignored);
        assert_eq!(parse(b"RE+VL032"), Event::Ignored);
        assert_eq!(parse(b"RE+XX"), Event::Unknown);
        assert_eq!(parse(b"RE+"), Event::Unknown);
        assert_eq!(parse(b"\0RE+IF11"), Event::Foreign);
    }

    #[test]
    fn hello_gets_ok_and_unknown_gets_err() {
        assert_eq!(parse(b"RE+IF0").reply(), Some(&b"AT+OK\r\n"[..]));
        assert_eq!(parse(b"RE+ZZ").reply(), Some(&b"AT+ERR\r\n"[..]));
        assert_eq!(parse(b"RE+FL1").reply(), None);
        assert_eq!(parse(b"garbage").reply(), None);
    }

    #[test]
    fn commands_match_the_vendor_format() {
        assert_eq!(filter(1).as_str(), "AT+FL1\r\n");
        assert_eq!(noise_reduction(9).as_str(), "AT+NR6\r\n");
        assert_eq!(volume(5).as_str(), "AT+VL005\r\n");
        assert_eq!(volume(200).as_str(), "AT+VL063\r\n");
    }
}
