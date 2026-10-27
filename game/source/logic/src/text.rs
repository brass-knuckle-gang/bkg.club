use core::fmt;

/// Fixed-capacity string for formatting without a heap. Text past the capacity is an error.
#[derive(Clone, Copy)]
pub struct Text<const N: usize> {
    buf: [u8; N],
    len: usize,
}

/// Equal by contents: bytes past `len` are left over from `pop` and `clear`.
impl<const N: usize> PartialEq for Text<N> {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl<const N: usize> Eq for Text<N> {}

impl<const N: usize> fmt::Debug for Text<N> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl<const N: usize> Default for Text<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Text<N> {
    pub const fn new() -> Self {
        Text { buf: [0; N], len: 0 }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// Appends one ASCII byte; false when full.
    pub fn push(&mut self, byte: u8) -> bool {
        if self.len == N || !byte.is_ascii() {
            return false;
        }
        self.buf[self.len] = byte;
        self.len += 1;
        true
    }

    /// Removes the last character, all of its bytes.
    pub fn pop(&mut self) -> Option<char> {
        let c = self.as_str().chars().next_back()?;
        self.len -= c.len_utf8();
        Some(c)
    }

    pub fn as_str(&self) -> &str {
        str_of(&self.buf, self.len)
    }

    fn sink(&mut self, cut: bool) -> Sink<'_> {
        Sink { buf: &mut self.buf, len: &mut self.len, cut }
    }
}

impl<const N: usize> Text<N> {
    /// Formats `args`, cut at the capacity: too-long text shows cut off, not missing.
    pub fn format(args: fmt::Arguments) -> Self {
        let mut text = Self::new();
        // A cutting `Sink` never fails, so an error here can only come from a `Display` impl.
        let _ = fmt::Write::write_fmt(&mut text.sink(true), args);
        text
    }
}

impl<const N: usize> fmt::Write for Text<N> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        fmt::Write::write_str(&mut self.sink(false), s)
    }

    fn write_fmt(&mut self, args: fmt::Arguments) -> fmt::Result {
        fmt::Write::write_fmt(&mut self.sink(false), args)
    }
}

// The code below is not generic over `N`: one copy serves every `Text` size, about 4.8 KB less.

#[inline(never)]
fn str_of(buf: &[u8], len: usize) -> &str {
    // Only whole `&str` values are ever copied in, so the bytes are valid UTF-8.
    core::str::from_utf8(&buf[..len]).unwrap()
}

/// Writes into a `Text`'s buffer. Text that does not fit is an error, or with `cut`, is dropped
/// from the last whole character that fits.
struct Sink<'a> {
    buf: &'a mut [u8],
    len: &'a mut usize,
    cut: bool,
}

impl fmt::Write for Sink<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let room = self.buf.len() - *self.len;
        let mut end = s.len();
        if end > room {
            if !self.cut {
                return Err(fmt::Error);
            }
            end = room;
            while !s.is_char_boundary(end) {
                end -= 1;
            }
        }
        self.buf[*self.len..*self.len + end].copy_from_slice(&s.as_bytes()[..end]);
        *self.len += end;
        Ok(())
    }
}

/// A number with one decimal, rounded half up. It writes integers, so it needs no float
/// formatting code. Negative numbers write as 0.0.
pub struct Tenths(pub f32);

impl fmt::Display for Tenths {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let tenths = (self.0 * 10.0 + 0.5) as u32;
        write!(f, "{}.{}", tenths / 10, tenths % 10)
    }
}

/// A number with two decimals, rounded. It writes integers, so it needs no float formatting
/// code. Negative numbers write as 0.00.
pub struct Hundredths(pub f32);

impl fmt::Display for Hundredths {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let hundredths = (self.0 * 100.0 + 0.5) as u32;
        write!(f, "{}.{:02}", hundredths / 100, hundredths % 100)
    }
}

// `str::contains`, `find` and `trim_end_matches` with a `&str` pattern use the Two-Way search,
// about 2.6 KB. The bots search a line or less, where byte compares at each offset are enough.

/// Byte index of the first `needle` in `hay`.
pub fn find(hay: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    hay.as_bytes().windows(needle.len()).position(|w| w == needle.as_bytes())
}

/// Whether `needle` is in `hay`.
pub fn has(hay: &str, needle: &str) -> bool {
    find(hay, needle).is_some()
}

/// `s` with every trailing `pat` removed.
pub fn trim_end<'a>(mut s: &'a str, pat: &str) -> &'a str {
    if pat.is_empty() {
        return s;
    }
    while let Some(rest) = s.strip_suffix(pat) {
        s = rest;
    }
    s
}

/// Splits `s` into lines of at most `cols` characters, breaking at spaces. A word longer than a
/// line is cut where the line ends.
pub fn wrap(s: &str, cols: usize) -> impl Iterator<Item = &str> {
    let mut rest = s.trim_start_matches(' ');
    core::iter::from_fn(move || {
        if rest.is_empty() || cols == 0 {
            return None;
        }
        let line = if rest.len() <= cols {
            rest
        } else {
            match rest[..=cols].rfind(' ') {
                Some(at) if at > 0 => &rest[..at],
                _ => &rest[..cols],
            }
        };
        rest = rest[line.len()..].trim_start_matches(' ');
        Some(line.trim_end_matches(' '))
    })
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    fn lines(s: &str, cols: usize) -> Vec<&str> {
        wrap(s, cols).collect()
    }

    #[test]
    fn search_matches_std() {
        for (hay, needle) in [("ABC", ""), ("", "A"), ("AB", "ABC"), ("TU 73", "73"), ("CQ DE K5MME", " DE "), ("ÉA", "A"), ("AAB", "AB")] {
            assert_eq!(find(hay, needle), hay.find(needle), "{hay} {needle}");
            assert_eq!(has(hay, needle), hay.contains(needle), "{hay} {needle}");
        }
        for (s, pat) in [("K5MME/P/P", "/P"), ("QTH TX =", " ="), ("=", " ="), ("AB", ""), ("", "X")] {
            assert_eq!(trim_end(s, pat), s.trim_end_matches(pat), "{s} {pat}");
        }
    }

    #[test]
    fn write_is_all_or_nothing() {
        use core::fmt::Write;
        let mut t = Text::<4>::new();
        assert!(t.write_str("ABCDE").is_err());
        assert_eq!(t.as_str(), "");
        // A literal argument would fold into the format string as one piece.
        assert!(write!(t, "AB{}", core::hint::black_box("CDE")).is_err());
        assert_eq!(t.as_str(), "AB");
    }

    #[test]
    fn tenths_round_half_up() {
        let tenths = |x: f32| Text::<8>::format(format_args!("{}", Tenths(x)));
        for (x, s) in [(0.0, "0.0"), (0.04, "0.0"), (0.05, "0.1"), (1.25, "1.3"), (9.99, "10.0"), (12.3, "12.3"), (-1.0, "0.0")] {
            assert_eq!(tenths(x).as_str(), s, "{x}");
        }
    }

    #[test]
    fn format_cuts_at_capacity() {
        assert_eq!(Text::<8>::format(format_args!("LOG {}/{}", 3, 10)).as_str(), "LOG 3/10");
        assert_eq!(Text::<6>::format(format_args!("LOG {}/{}", 3, 10)).as_str(), "LOG 3/");
        assert_eq!(Text::<2>::format(format_args!("A{}", "é")).as_str(), "A");
    }

    #[test]
    fn pop_takes_a_whole_character() {
        let mut text = Text::<4>::format(format_args!("Aé"));
        assert_eq!((text.pop(), text.as_str()), (Some('é'), "A"));
        assert_eq!((text.pop(), text.pop()), (Some('A'), None));
    }

    #[test]
    fn wraps_at_spaces_and_cuts_long_words() {
        assert_eq!(lines("CQ POTA DE K5MME K5MME K", 10), ["CQ POTA DE", "K5MME", "K5MME K"]);
        assert_eq!(lines("K5MME  W1AW", 5), ["K5MME", "W1AW"]);
        assert_eq!(lines("ABCDEFGHIJKL MN", 5), ["ABCDE", "FGHIJ", "KL MN"]);
        assert_eq!(lines("  ", 5), Vec::<&str>::new());
        assert_eq!(lines("TU 72", 27), ["TU 72"]);
    }
}
