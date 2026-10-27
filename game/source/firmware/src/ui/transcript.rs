//! The QSO, hunt and ragchew transcripts: one entry per transmission, word-wrapped, newest at the
//! bottom once the rows fill. A hidden entry takes one row of dashes until revealed, so a long
//! one doesn't push what you can read off the screen.

use embedded_graphics::pixelcolor::Rgb565;
use pocketcw_logic::text::wrap;

use crate::gfx::{CHAR_W, rect, text_cols, tiny};
use crate::lcd::Lcd;
use crate::theme::{MAIN, Theme};

pub(super) const ROWS: usize = 7;
pub(super) const MAX_ROWS: usize = 10;
const HIDDEN: &str = "---";
const ROW_Y: i32 = 26;
const ROW_STEP: i32 = 18;

pub(super) struct Entry<'a> {
    pub who: &'a str,
    pub who_color: Rgb565,
    pub color: Rgb565,
    pub text: &'a str,
    pub hidden: bool,
}

/// One screen row: the entry it belongs to, its line of text, and whether it is the first.
type Row<'a> = (usize, &'a str, bool);

/// Draws the last `count` rows of `entries`, at most `MAX_ROWS`, with their text starting at `x`.
pub(super) fn draw(lcd: &mut Lcd, t: &Theme, entries: &[Option<Entry>], x: i32, count: usize) {
    let cols = ((MAIN - x) / CHAR_W) as usize;
    let count = count.min(MAX_ROWS);
    let mut rows: [Option<Row>; MAX_ROWS] = [None; MAX_ROWS];
    let mut n = 0;
    let mut after_hidden = false;
    for (i, entry) in entries.iter().enumerate() {
        let Some(entry) = entry else { continue };
        // Hidden lines that continue a hidden entry share its row.
        let joins = entry.hidden && after_hidden && entry.who.is_empty();
        after_hidden = entry.hidden;
        if joins {
            continue;
        }
        let text = if entry.hidden { HIDDEN } else { entry.text };
        for (k, line) in wrap(text, cols).enumerate() {
            if n == count {
                rows[..count].copy_within(1.., 0);
                n -= 1;
            }
            rows[n] = Some((i, line, k == 0));
            n += 1;
        }
    }
    for (r, row) in rows[..count].iter().enumerate() {
        let y = ROW_Y + r as i32 * ROW_STEP;
        rect(lcd, 0, y, MAIN, ROW_STEP, t.bg);
        let Some((i, line, first)) = *row else { continue };
        let Some(entry) = &entries[i] else { continue };
        if first {
            tiny(lcd, 2, y + 3, entry.who, entry.who_color, t.bg);
        }
        let color = if entry.hidden { t.muted } else { entry.color };
        text_cols(lcd, x, y, line, cols, color, t.bg);
    }
}
