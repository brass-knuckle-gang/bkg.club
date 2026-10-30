//! Pixel art sprites: 4-bit palette indices, raw or run-length coded, for the games.

use embedded_graphics::pixelcolor::Rgb565;

/// A sprite's pixels, 4 bits each; index 0 is clear.
///
/// With `marks` empty, `data` holds the rows packed two pixels a byte, high nibble first, each
/// row a whole number of bytes. Otherwise `data` holds runs, one byte each: the length less one
/// in the high nibble, the index in the low, and no run crosses a row. `marks[k]` is where row
/// `8 * k` starts in `data`.
pub struct Sprite {
    pub w: u8,
    pub h: u8,
    /// The pivot, from the top left corner.
    pub ox: i8,
    pub oy: i8,
    pub data: &'static [u8],
    pub marks: &'static [u16],
}

pub type Palette = [Rgb565; 16];

/// The widest sprite.
pub const MAX_W: usize = 160;

impl Sprite {
    /// Unpacks row `y` into `out[..w]`.
    pub fn row(&self, y: usize, out: &mut [u8; MAX_W]) {
        let w = self.w as usize;
        if self.marks.is_empty() {
            let stride = w.div_ceil(2);
            let data = &self.data[y * stride..][..stride];
            for (x, p) in out[..w].iter_mut().enumerate() {
                *p = (data[x / 2] >> if x % 2 == 0 { 4 } else { 0 }) & 0x0F;
            }
            return;
        }
        let mut i = self.marks[y / 8] as usize;
        // Skip the rows before `y` in its group of eight.
        for _ in 0..y % 8 {
            let mut x = 0;
            while x < w {
                x += (self.data[i] >> 4) as usize + 1;
                i += 1;
            }
        }
        let mut x = 0;
        while x < w {
            let run = self.data[i];
            i += 1;
            let end = (x + (run >> 4) as usize + 1).min(w);
            out[x..end].fill(run & 0x0F);
            x = end;
        }
    }
}
