//! Drawing primitives for the UI: rectangles, slanted blocks, block letters and text.

use embedded_graphics::{
    pixelcolor::{Rgb565, Rgb888, RgbColor},
    prelude::*,
    primitives::Rectangle,
};

use crate::fonts::{SMALL, TINY};
use crate::lcd::Lcd;

pub const CHAR_W: i32 = 10;
pub const CHAR_H: i32 = 16;
pub const TINY_W: i32 = 7;
pub const TINY_H: i32 = 12;
const _: () = assert!(SMALL.width as i32 == CHAR_W && SMALL.height as i32 == CHAR_H);
const _: () = assert!(TINY.width as i32 == TINY_W && TINY.height as i32 == TINY_H);

pub const fn rgb(hex: u32) -> Rgb565 {
    Rgb565::new((hex >> 19) as u8 & 0x1F, (hex >> 10) as u8 & 0x3F, (hex >> 3) as u8 & 0x1F)
}

/// The color `percent` of the way from `a` to `b`.
pub fn mix(a: Rgb565, b: Rgb565, percent: u32) -> Rgb565 {
    let (a, b) = (Rgb888::from(a), Rgb888::from(b));
    let m = |x: u8, y: u8| ((x as u32 * (100 - percent) + y as u32 * percent) / 100) as u8;
    Rgb565::from(Rgb888::new(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b())))
}

/// Black or white, whichever reads better on `background`.
pub fn ink(background: Rgb565) -> Rgb565 {
    let c = Rgb888::from(background);
    let luma = 299 * c.r() as u32 + 587 * c.g() as u32 + 114 * c.b() as u32;
    if luma > 150_000 { Rgb565::BLACK } else { Rgb565::WHITE }
}

pub fn rect(lcd: &mut Lcd, x: i32, y: i32, w: i32, h: i32, color: Rgb565) {
    if w > 0 && h > 0 {
        let _ = lcd.fill_solid(&Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32)), color);
    }
}

/// A parallelogram `w` wide and `h` tall whose top edge sits `skew` pixels right of its bottom
/// edge. Rows with the same offset go out as one rectangle: each fill costs an LCD window setup.
pub fn para(lcd: &mut Lcd, x: i32, y: i32, w: i32, h: i32, skew: i32, color: Rgb565) {
    if skew == 0 || h < 2 {
        return rect(lcd, x, y, w, h, color);
    }
    let shift = |row: i32| (2 * skew * (h - 1 - row) + h - 1).div_euclid(2 * (h - 1));
    let mut top = 0;
    while top < h {
        let mut end = top + 1;
        while end < h && shift(end) == shift(top) {
            end += 1;
        }
        rect(lcd, x + shift(top), y + top, w, end - top, color);
        top = end;
    }
}

pub fn text(lcd: &mut Lcd, x: i32, y: i32, s: &str, fg: Rgb565, bg: Rgb565) {
    let _ = SMALL.draw(lcd, Point::new(x, y), s, fg, bg);
}

/// Text in the label font.
pub fn tiny(lcd: &mut Lcd, x: i32, y: i32, s: &str, fg: Rgb565, bg: Rgb565) {
    let _ = TINY.draw(lcd, Point::new(x, y), s, fg, bg);
}

/// Text ending at `right`.
pub fn text_right(lcd: &mut Lcd, right: i32, y: i32, s: &str, fg: Rgb565, bg: Rgb565) {
    text(lcd, right - s.len() as i32 * CHAR_W, y, s, fg, bg);
}

/// Text followed by background up to `cols` characters, so it replaces a longer old text.
pub fn text_cols(lcd: &mut Lcd, x: i32, y: i32, s: &str, cols: usize, fg: Rgb565, bg: Rgb565) {
    let s = &s[..s.len().min(cols)];
    text(lcd, x, y, s, fg, bg);
    rect(lcd, x + s.len() as i32 * CHAR_W, y, (cols - s.len()) as i32 * CHAR_W, CHAR_H, bg);
}

/// 5 x 7 block letters of `px`-pixel blocks, `gap` pixels apart. `slant` leans them right by
/// tenths of a pixel per block row, sheared one pixel row at a time so upright strokes stay
/// straight. With `bg`, empty blocks are painted too, so new letters replace old ones in place.
/// Returns the x after the last letter.
#[allow(clippy::too_many_arguments)]
pub fn blocks(lcd: &mut Lcd, x: i32, y: i32, s: &str, px: i32, gap: i32, slant: i32, fg: Rgb565, bg: Option<Rgb565>) -> i32 {
    let rows = 7 * px;
    let shift = |py: i32| ((rows - 1 - py) * slant + 5 * px) / (10 * px);
    let mut left = x;
    for c in s.chars() {
        let glyph = glyph(c);
        let mut top = 0;
        while top < rows {
            // Pixel rows of one block row with the same shift go out together: each fill costs an
            // LCD window setup.
            let mut end = top + 1;
            while end < rows && end / px == top / px && shift(end) == shift(top) {
                end += 1;
            }
            let bits = glyph[(top / px) as usize];
            let on = |col: i32| bits & (0b10000 >> col) != 0;
            let mut col = 0;
            while col < 5 {
                let mut last = col + 1;
                while last < 5 && on(last) == on(col) {
                    last += 1;
                }
                if let Some(color) = if on(col) { Some(fg) } else { bg } {
                    rect(lcd, left + col * px + shift(top), y + top, (last - col) * px, end - top, color);
                }
                col = last;
            }
            top = end;
        }
        left += 5 * px + gap;
    }
    left - gap
}

pub const fn blocks_width(len: usize, px: i32, gap: i32) -> i32 {
    len as i32 * (5 * px + gap) - gap
}

/// One row per byte, top first; bit 4 is the left column. Unknown characters are solid.
pub fn glyph(c: char) -> [u8; 7] {
    match c.to_ascii_uppercase() {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [14, 4, 4, 4, 4, 4, 14],
        'J' => [7, 2, 2, 2, 2, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 17, 25, 21, 19, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [31, 2, 4, 2, 1, 17, 14],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 30, 1, 1, 17, 14],
        '6' => [6, 8, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 2, 12],
        '/' => [1, 1, 2, 4, 8, 16, 16],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        '.' => [0, 0, 0, 0, 0, 12, 12],
        '!' => [4, 4, 4, 4, 4, 0, 4],
        '+' => [0, 4, 4, 31, 4, 4, 0],
        '?' => [14, 17, 1, 2, 4, 0, 4],
        ':' => [0, 12, 12, 0, 12, 12, 0],
        '\'' => [4, 4, 8, 0, 0, 0, 0],
        ' ' => [0; 7],
        _ => [31; 7],
    }
}
