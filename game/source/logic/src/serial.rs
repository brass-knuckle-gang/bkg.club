//! Commands a computer sends over the USB serial port, one per line.

use crate::buttons::Button;
use crate::config::Name;
use crate::player::MAX_TEXT;
use crate::text::Text;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Command {
    /// Send the screen.
    Shot,
    /// Press and release a button.
    Press(Button),
    /// Press an F key, hold it past its hold time, and release it.
    Hold(Button),
    /// Key this text, upper case, as a straight key at the keyer speed.
    Send(Text<MAX_TEXT>),
    /// Send the firmware version, the screen and the panel.
    Info,
    /// Send one setting or memory, or all of them.
    Get(Option<Name>),
    /// Set a setting or memory to this value, upper case.
    Set(Name, Text<MAX_TEXT>),
    /// Sound this text, upper case, as the device sends to you.
    Play(Text<MAX_TEXT>),
    /// Speak this word from the content pack, upper case.
    Say(Text<MAX_TEXT>),
    /// Stop what `Send` and `Play` started.
    Stop,
    /// Send a line for each change on the key lines, or stop.
    Edge(bool),
    /// VBand's page connected to us as a CW Hotline, or let go.
    Hotline(bool),
    Stats(Stats),
    /// A line or text longer than a command takes.
    TooLong,
    Unknown,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stats {
    Letters,
    Progress,
    Timing,
}

const SET: &[u8] = b"SET ";
/// The longest line that can hold a command: a memory set to its longest text.
const LINE_LEN: usize = SET.len() + b"M1 ".len() + MAX_TEXT;

const BUTTON_NAMES: [(&[u8], Button); 7] = [
    (b"F1", Button::F1),
    (b"F2", Button::F2),
    (b"F3", Button::F3),
    (b"F4", Button::F4),
    (b"LEFT", Button::Left),
    (b"RIGHT", Button::Right),
    (b"CLICK", Button::Click),
];

/// Collects bytes into lines and names the command on each. A line ends at `\n` or `\r`.
pub struct CommandLine {
    line: [u8; LINE_LEN],
    len: usize,
    /// The line outgrew `line`, so it is no command.
    overflow: bool,
}

impl Default for CommandLine {
    fn default() -> Self {
        CommandLine { line: [0; LINE_LEN], len: 0, overflow: false }
    }
}

impl CommandLine {
    /// The command that `byte` ends, if any. An empty line is none, so `\r\n` ends one line.
    pub fn push(&mut self, byte: u8) -> Option<Command> {
        if byte == b'\n' || byte == b'\r' {
            let line = &self.line[..self.len];
            let command = match (self.overflow, line.is_empty()) {
                (true, _) => Some(Command::TooLong),
                (false, true) => None,
                (false, false) => Some(parse(line)),
            };
            *self = CommandLine::default();
            return command;
        }
        if self.len == LINE_LEN {
            self.overflow = true;
        } else {
            self.line[self.len] = byte;
            self.len += 1;
        }
        None
    }
}

fn parse(line: &[u8]) -> Command {
    let name = |name: &[u8]| core::str::from_utf8(name).ok().and_then(Name::parse);
    let command = match line {
        b"SHOT" => Some(Command::Shot),
        b"INFO" => Some(Command::Info),
        b"GET" => Some(Command::Get(None)),
        b"STOP" => Some(Command::Stop),
        b"EDGE ON" => Some(Command::Edge(true)),
        b"EDGE OFF" => Some(Command::Edge(false)),
        // VBand writes these in mixed case.
        b"VBand On" => Some(Command::Hotline(true)),
        b"VBand Off" => Some(Command::Hotline(false)),
        b"STATS LETTERS" => Some(Command::Stats(Stats::Letters)),
        b"STATS PROGRESS" => Some(Command::Stats(Stats::Progress)),
        b"STATS TIMING" => Some(Command::Stats(Stats::Timing)),
        _ => None,
    };
    if let Some(command) = command {
        command
    } else if let Some(text) = line.strip_prefix(b"SEND ") {
        upper(text).map_or(Command::TooLong, Command::Send)
    } else if let Some(text) = line.strip_prefix(b"PLAY ") {
        upper(text).map_or(Command::TooLong, Command::Play)
    } else if let Some(text) = line.strip_prefix(b"SAY ") {
        upper(text).map_or(Command::TooLong, Command::Say)
    } else if let Some(button) = line.strip_prefix(b"PRESS ") {
        BUTTON_NAMES.iter().find(|(n, _)| *n == button).map_or(Command::Unknown, |&(_, b)| Command::Press(b))
    } else if let Some(button) = line.strip_prefix(b"HOLD ") {
        // Only the F keys: a held dial click powers off.
        BUTTON_NAMES[..4].iter().find(|(n, _)| *n == button).map_or(Command::Unknown, |&(_, b)| Command::Hold(b))
    } else if let Some(n) = line.strip_prefix(b"GET ") {
        name(n).map_or(Command::Unknown, |n| Command::Get(Some(n)))
    } else if let Some(rest) = line.strip_prefix(SET) {
        let space = rest.iter().position(|&b| b == b' ').unwrap_or(rest.len());
        match (name(&rest[..space]), rest.get(space + 1..)) {
            (Some(n), Some(value)) => upper(value).map_or(Command::TooLong, |v| Command::Set(n, v)),
            _ => Command::Unknown,
        }
    } else {
        Command::Unknown
    }
}

/// `text` in upper case without bytes past ASCII, or None when it is longer than a text
/// command takes.
fn upper(text: &[u8]) -> Option<Text<MAX_TEXT>> {
    let mut upper = Text::new();
    text.iter().filter(|b| b.is_ascii()).all(|&b| upper.push(b.to_ascii_uppercase())).then_some(upper)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Field;

    fn commands(bytes: &[u8]) -> [Option<Command>; 4] {
        let mut line = CommandLine::default();
        let mut found = [None; 4];
        let mut n = 0;
        for &b in bytes {
            if let Some(command) = line.push(b) {
                found[n] = Some(command);
                n += 1;
            }
        }
        found
    }

    fn one(line: &str) -> Command {
        match commands(line.as_bytes()) {
            [Some(command), None, None, None] => command,
            found => panic!("{line:?} gave {found:?}"),
        }
    }

    fn text(text: &str) -> Text<MAX_TEXT> {
        Text::format(format_args!("{text}"))
    }

    #[test]
    fn a_line_ends_at_either_line_end_and_empty_lines_are_skipped() {
        assert_eq!(commands(b"SHOT\n"), [Some(Command::Shot), None, None, None]);
        assert_eq!(commands(b"SHOT\r\nSHOT\r\n\n"), [Some(Command::Shot), Some(Command::Shot), None, None]);
        assert_eq!(commands(b"SHOT"), [None; 4], "no line end yet");
    }

    #[test]
    fn plain_commands() {
        assert_eq!(one("INFO\n"), Command::Info);
        assert_eq!(one("GET\n"), Command::Get(None));
        assert_eq!(one("STOP\n"), Command::Stop);
        assert_eq!(one("EDGE ON\n"), Command::Edge(true));
        assert_eq!(one("EDGE OFF\n"), Command::Edge(false));
        assert_eq!(one("VBand On\n"), Command::Hotline(true));
        assert_eq!(one("VBand Off\n"), Command::Hotline(false));
        assert_eq!(one("STATS LETTERS\n"), Command::Stats(Stats::Letters));
        assert_eq!(one("STATS PROGRESS\n"), Command::Stats(Stats::Progress));
        assert_eq!(one("STATS TIMING\n"), Command::Stats(Stats::Timing));
    }

    #[test]
    fn other_lines_are_unknown() {
        for line in ["shot\n", "SHOTS\n", " SHOT\n", "EDGE\n", "EDGES ON\n", "VBAND ON\n", "STATS\n", "PRESS\n", "PRESS F5\n", "PRESS f1\n"] {
            assert_eq!(one(line), Command::Unknown, "{line:?}");
        }
        for line in ["PRESS  F1\n", "SEND\n", "GET SPEEDY\n", "GET speed\n", "SET SPEED\n", "SET SPEEDY 20\n"] {
            assert_eq!(one(line), Command::Unknown, "{line:?}");
        }
    }

    #[test]
    fn press_names_each_button() {
        for (name, button) in BUTTON_NAMES {
            let mut line = CommandLine::default();
            let found = [b"PRESS ".as_slice(), name, b"\n"].concat().iter().find_map(|&b| line.push(b));
            assert_eq!(found, Some(Command::Press(button)));
        }
    }

    #[test]
    fn hold_takes_only_the_f_keys() {
        assert_eq!(one("HOLD F1\n"), Command::Hold(Button::F1));
        assert_eq!(one("HOLD F4\n"), Command::Hold(Button::F4));
        for line in ["HOLD CLICK\n", "HOLD LEFT\n", "HOLD F5\n", "HOLD\n"] {
            assert_eq!(one(line), Command::Unknown, "{line:?}");
        }
    }

    #[test]
    fn send_and_play_take_their_text_in_upper_case() {
        assert_eq!(one("SEND cq de K5MME <ar>\n"), Command::Send(text("CQ DE K5MME <AR>")));
        assert_eq!(one("SEND \n"), Command::Send(text("")));
        assert_eq!(one("PLAY cq\n"), Command::Play(text("CQ")));
        assert_eq!(one("SAY qth\n"), Command::Say(text("QTH")));
    }

    #[test]
    fn get_and_set_name_a_setting_or_memory() {
        assert_eq!(one("GET SPEED\n"), Command::Get(Some(Name::Setting(Field::Speed))));
        assert_eq!(one("SET MODE iambic a\n"), Command::Set(Name::Setting(Field::Mode), text("IAMBIC A")));
        assert_eq!(one("SET M1 \n"), Command::Set(Name::Memory(0), text("")));
    }

    #[test]
    fn a_long_line_or_text_is_too_long_and_the_next_line_still_counts() {
        let long = [b'X'; LINE_LEN + 1];
        assert_eq!(commands(&[&long[..], b"\nSHOT\n"].concat()), [Some(Command::TooLong), Some(Command::Shot), None, None]);
        let full_set = [b"SET M4 ".as_slice(), &[b'E'; MAX_TEXT], b"\n"].concat();
        assert!(matches!(commands(&full_set)[0], Some(Command::Set(Name::Memory(3), t)) if t.len() == MAX_TEXT));
        let over = [b"SEND ".as_slice(), &[b'E'; MAX_TEXT + 1], b"\n"].concat();
        assert_eq!(commands(&over)[0], Some(Command::TooLong));
    }
}
