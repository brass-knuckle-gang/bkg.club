//! International Morse code, ITU-R M.1677-1.
//!
//! A character is one byte. Prosigns are two letters sent as one character with no gap between
//! them: text writes them between angle brackets, as `<AR>`, and where each character must be
//! one byte, such as drill scoring, prosign `i` in `PROSIGNS` is the control byte
//! `FIRST_PROSIGN + i`.

const LETTERS: [&str; 26] = [
    ".-", "-...", "-.-.", "-..", ".", "..-.", "--.", "....", "..", ".---", "-.-", ".-..", "--", "-.", "---", ".--.", "--.-",
    ".-.", "...", "-", "..-", "...-", ".--", "-..-", "-.--", "--..",
];
const DIGITS: [&str; 10] = ["-----", ".----", "..---", "...--", "....-", ".....", "-....", "--...", "---..", "----."];
const PROSIGNS: [(&str, &str); 9] = [
    ("<AR>", ".-.-."),
    ("<AS>", ".-..."),
    ("<BK>", "-...-.-"),
    ("<CL>", "-.-..-.."),
    ("<CT>", "-.-.-"),
    ("<KN>", "-.--."),
    ("<SK>", "...-.-"),
    ("<SN>", "...-."),
    ("<HH>", "........"),
];
const FIRST_PROSIGN: u8 = 0x01;
/// Length of a prosign written in text: `<AR>`.
const PROSIGN_TEXT_LEN: usize = 4;

fn prosign(c: u8) -> Option<(&'static str, &'static str)> {
    PROSIGNS.get(c.wrapping_sub(FIRST_PROSIGN) as usize).copied()
}

/// Dots and dashes for a character, case-insensitive.
pub fn code(c: u8) -> Option<&'static str> {
    match c.to_ascii_uppercase() {
        c @ b'A'..=b'Z' => Some(LETTERS[(c - b'A') as usize]),
        c @ b'0'..=b'9' => Some(DIGITS[(c - b'0') as usize]),
        b'/' => Some("-..-."),
        b'?' => Some("..--.."),
        b'.' => Some(".-.-.-"),
        b',' => Some("--..--"),
        b'=' => Some("-...-"),
        b':' => Some("---..."),
        b'\'' => Some(".----."),
        b'-' => Some("-....-"),
        b')' => Some("-.--.-"),
        b'"' => Some(".-..-."),
        b'@' => Some(".--.-."),
        // Same codes as <KN> and <AR>, which `decode` returns for them.
        b'(' => Some("-.--."),
        b'+' => Some(".-.-."),
        c => prosign(c).map(|(_, code)| code),
    }
}

/// Every character `code` knows that is not a prosign and has a code of its own, for reverse lookup.
const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789/?.,=:'-)\"@";

/// The character sent as `pattern` of dots and dashes.
pub fn decode(pattern: &str) -> Option<u8> {
    let prosigns = (0..PROSIGNS.len() as u8).map(|i| FIRST_PROSIGN + i);
    CHARS.iter().copied().chain(prosigns).find(|&c| code(c) == Some(pattern))
}

/// How text writes character `c`: a prosign as `<AR>`, anything else as itself.
pub fn text(c: &u8) -> &str {
    match prosign(*c) {
        Some((text, _)) => text,
        None => core::str::from_utf8(core::slice::from_ref(c)).unwrap_or("?"),
    }
}

/// A prosign's two letters, for showing it with a bar over them.
pub fn prosign_letters(c: u8) -> Option<&'static str> {
    prosign(c).map(|(text, _)| &text[1..3])
}

/// The first character of `text` and how many bytes it takes there.
pub fn first(text: &[u8]) -> Option<(u8, usize)> {
    let &c = text.first()?;
    let written = text.get(..PROSIGN_TEXT_LEN).unwrap_or_default();
    match PROSIGNS.iter().position(|(p, _)| p.as_bytes() == written) {
        Some(i) => Some((FIRST_PROSIGN + i as u8, PROSIGN_TEXT_LEN)),
        None => Some((c, 1)),
    }
}

/// The characters of `text`, one byte each.
pub fn chars(mut text: &[u8]) -> impl Iterator<Item = u8> + '_ {
    core::iter::from_fn(move || {
        let (c, len) = first(text)?;
        text = &text[len..];
        Some(c)
    })
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    #[test]
    fn callsign() {
        let codes: Vec<_> = b"k5mme".iter().map(|&c| code(c)).collect();
        assert_eq!(codes, [Some("-.-"), Some("....."), Some("--"), Some("--"), Some(".")]);
    }

    #[test]
    fn decode_reverses_code() {
        for &c in CHARS {
            assert_eq!(decode(code(c).unwrap()), Some(c));
        }
        assert_eq!(decode("........."), None);
    }

    #[test]
    fn itu_punctuation() {
        assert_eq!(decode("-....-"), Some(b'-'));
        assert_eq!(decode(".--.-."), Some(b'@'));
    }

    #[test]
    fn open_bracket_and_plus_send_as_prosigns() {
        assert_eq!(code(b'('), code(chars(b"<KN>").next().unwrap()));
        assert_eq!(code(b'+'), code(chars(b"<AR>").next().unwrap()));
        assert_eq!(text(&decode("-.--.").unwrap()), "<KN>");
        assert_eq!(text(&decode(".-.-.").unwrap()), "<AR>");
    }

    #[test]
    fn error_signal_is_eight_dots() {
        let hh = decode("........").unwrap();
        assert_eq!((text(&hh), prosign_letters(hh)), ("<HH>", Some("HH")));
    }

    #[test]
    fn prosigns_read_and_write_as_text() {
        let sk = decode("...-.-").unwrap();
        assert_eq!((text(&sk), prosign_letters(sk)), ("<SK>", Some("SK")));
        let read: Vec<u8> = chars(b"73 <SK> <X>").collect();
        assert_eq!(read, [b'7', b'3', b' ', sk, b' ', b'<', b'X', b'>']);
        for (written, pattern) in PROSIGNS {
            assert_eq!(text(&decode(pattern).unwrap()), written);
            assert!(!CHARS.iter().any(|&c| code(c) == Some(pattern)), "{written} is also a character");
        }
    }
}
