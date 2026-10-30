//! What your keying decodes to, as lines of text that scroll up when the last one fills. A word
//! that reaches the end of a line moves whole to the next one. Each character keeps the timing
//! quality of its keying.

use crate::decoder::Decoded;
use crate::morse;
use crate::timing::Quality;

pub struct DecodeLines<const COLS: usize, const ROWS: usize> {
    lines: [[u8; COLS]; ROWS],
    quality: [[Option<Quality>; COLS]; ROWS],
    row: usize,
    col: usize,
    scrolled: bool,
}

impl<const COLS: usize, const ROWS: usize> Default for DecodeLines<COLS, ROWS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const COLS: usize, const ROWS: usize> DecodeLines<COLS, ROWS> {
    pub const fn new() -> Self {
        DecodeLines { lines: [[b' '; COLS]; ROWS], quality: [[None; COLS]; ROWS], row: 0, col: 0, scrolled: false }
    }

    /// Adds a character, a prosign as `<AR>`; leading and repeated word spaces are dropped.
    /// `worst` is the worst timing in its keying.
    pub fn push(&mut self, decoded: Decoded, worst: Quality) {
        let (written, quality) = match &decoded {
            Decoded::Char(c) => (morse::text(c), Some(worst)),
            Decoded::Unknown => ("*", Some(Quality::Bad)),
            Decoded::Space => (" ", None),
        };
        self.scrolled = written.len() > 1;
        for &b in written.as_bytes() {
            self.push_byte(b, quality);
        }
    }

    pub fn line(&self, row: usize) -> &str {
        // Every byte comes from `morse::text` or a literal, so the line is ASCII.
        core::str::from_utf8(&self.lines[row]).unwrap()
    }

    pub fn quality(&self, row: usize) -> &[Option<Quality>; COLS] {
        &self.quality[row]
    }

    /// Row and column where the next character goes; the column is `COLS` when the row is full.
    pub fn cursor(&self) -> (usize, usize) {
        (self.row, self.col)
    }

    /// The last push changed more than its own cell, so every line needs drawing.
    pub fn scrolled(&self) -> bool {
        self.scrolled
    }

    fn push_byte(&mut self, c: u8, quality: Option<Quality>) {
        if c == b' ' && (self.col == 0 || self.lines[self.row][self.col - 1] == b' ') {
            return;
        }
        if self.col == COLS {
            if c == b' ' {
                return self.new_line(COLS);
            }
            // The word in progress starts after the line's last space; one with no space fills
            // the line, so it breaks there instead.
            let start = self.lines[self.row].iter().rposition(|&b| b == b' ').map_or(COLS, |at| at + 1);
            self.new_line(start);
        }
        self.lines[self.row][self.col] = c;
        self.quality[self.row][self.col] = quality;
        self.col += 1;
    }

    /// Starts the next line, scrolling when there is none, and moves the current line's
    /// characters from `start` onto it.
    fn new_line(&mut self, start: usize) {
        let (word, quality) = (self.lines[self.row], self.quality[self.row]);
        self.lines[self.row][start..].fill(b' ');
        self.quality[self.row][start..].fill(None);
        if self.row + 1 < ROWS {
            self.row += 1;
        } else {
            self.lines.copy_within(1.., 0);
            self.quality.copy_within(1.., 0);
        }
        let moved = COLS - start;
        self.lines[self.row] = [b' '; COLS];
        self.quality[self.row] = [None; COLS];
        self.lines[self.row][..moved].copy_from_slice(&word[start..]);
        self.quality[self.row][..moved].copy_from_slice(&quality[start..]);
        self.col = moved;
        self.scrolled = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_str<const C: usize, const R: usize>(d: &mut DecodeLines<C, R>, s: &str) {
        for b in s.bytes() {
            d.push(if b == b' ' { Decoded::Space } else { Decoded::Char(b) }, Quality::Good);
        }
    }

    #[test]
    fn drops_leading_and_repeated_spaces() {
        let mut d = DecodeLines::<6, 2>::new();
        push_str(&mut d, " A  B");
        assert_eq!(d.line(0), "A B   ");
        assert_eq!(d.cursor(), (0, 3));
    }

    #[test]
    fn moves_a_word_whole_to_the_next_line() {
        let mut d = DecodeLines::<6, 2>::new();
        push_str(&mut d, "CQ DE K");
        assert!(d.scrolled());
        push_str(&mut d, "1");
        assert!(!d.scrolled());
        assert_eq!(d.line(0), "CQ DE ");
        assert_eq!(d.line(1), "K1    ");
        push_str(&mut d, "ABC W1");
        assert_eq!(d.line(0), "K1ABC ");
        assert_eq!(d.line(1), "W1    ");
    }

    #[test]
    fn breaks_a_word_with_no_space_and_scrolls() {
        let mut d = DecodeLines::<4, 2>::new();
        push_str(&mut d, "ABCDEFGHI");
        assert_eq!(d.line(0), "EFGH");
        assert_eq!(d.line(1), "I   ");
        assert_eq!(d.cursor(), (1, 1));
    }

    #[test]
    fn keeps_quality_and_redraws_after_a_prosign() {
        let mut d = DecodeLines::<8, 2>::new();
        d.push(Decoded::Char(b'E'), Quality::Bad);
        assert!(!d.scrolled());
        d.push(Decoded::Unknown, Quality::Good);
        assert_eq!(d.quality(0)[..2], [Some(Quality::Bad), Some(Quality::Bad)]);
        let ar = morse::decode(".-.-.").unwrap();
        d.push(Decoded::Char(ar), Quality::Good);
        assert!(d.scrolled());
        assert_eq!(d.line(0), "E*<AR>  ");
        assert_eq!(d.quality(0)[2..6], [Some(Quality::Good); 4]);
    }
}
