use embedded_graphics::{
    pixelcolor::{Rgb565, RgbColor},
    prelude::*,
    primitives::Rectangle,
};

/// Monospace glyph table made by `tools/fontgen`.
pub struct Font {
    pub width: u16,
    pub height: u16,
    /// Sorted by byte value.
    pub chars: &'static [u8],
    /// Per glyph: left, top, width and height of its inked pixels inside the cell.
    pub boxes: &'static [[u8; 4]],
    /// Per glyph: where its pixels start in `pixels`; one more entry ends the last glyph.
    pub offsets: &'static [u16],
    /// Coverage 0..=15, 4 bits per pixel, high nibble first, each glyph's box row by row.
    pub pixels: &'static [u8],
}

impl Font {
    /// A lowercase letter missing from the font finds its capital.
    fn glyph(&self, c: u8) -> Option<([u8; 4], &'static [u8])> {
        let index = self.chars.binary_search(&c).or_else(|_| self.chars.binary_search(&c.to_ascii_uppercase())).ok()?;
        let pixels = self.pixels.get(self.offsets[index] as usize..self.offsets[index + 1] as usize)?;
        Some((self.boxes[index], pixels))
    }

    /// The coverage, 0..=15, of `c` at (`x`, `y`) in its cell.
    pub fn coverage(&self, c: u8, x: i32, y: i32) -> u8 {
        let Some(([left, top, w, h], pixels)) = self.glyph(c) else { return 0 };
        let (bx, by) = (x - left as i32, y - top as i32);
        if bx < 0 || by < 0 || bx >= w as i32 || by >= h as i32 {
            return 0;
        }
        let p = (by * w as i32 + bx) as usize;
        (pixels[p / 2] >> if p % 2 == 0 { 4 } else { 0 }) & 0x0F
    }

    pub fn size(&self) -> Size {
        Size::new(self.width as u32, self.height as u32)
    }

    /// Draws `text` with its top left corner at `origin`, blending from `bg` to `fg` by coverage.
    /// A character missing from the font draws as a solid `fg` cell, so the gap shows on screen.
    pub fn draw<D: DrawTarget<Color = Rgb565>>(
        &self,
        target: &mut D,
        origin: Point,
        text: &str,
        fg: Rgb565,
        bg: Rgb565,
    ) -> Result<(), D::Error> {
        let palette = blend_palette(fg, bg);
        let (width, height) = (self.width as usize, self.height as usize);
        for (i, c) in text.bytes().enumerate() {
            let cell = Rectangle::new(origin + Point::new(i as i32 * self.width as i32, 0), self.size());
            match self.glyph(c) {
                Some((ink, glyph)) => {
                    let [left, top, w, h] = ink.map(usize::from);
                    let pixels = (0..height).flat_map(|y| {
                        (0..width).map(move |x| {
                            // Wrapping: a pixel left of or above the box lands far past its end.
                            let (bx, by) = (x.wrapping_sub(left), y.wrapping_sub(top));
                            if bx >= w || by >= h {
                                return palette[0];
                            }
                            let p = by * w + bx;
                            let shift = if p % 2 == 0 { 4 } else { 0 };
                            palette[((glyph[p / 2] >> shift) & 0x0F) as usize]
                        })
                    });
                    target.fill_contiguous(&cell, pixels)?
                }
                None => target.fill_solid(&cell, fg)?,
            }
        }
        Ok(())
    }
}

/// The 16 coverage levels from `bg` (0) to `fg` (15).
fn blend_palette(fg: Rgb565, bg: Rgb565) -> [Rgb565; 16] {
    core::array::from_fn(|level| {
        let level = level as u32;
        let mix = |f: u8, b: u8| ((b as u32 * (15 - level) + f as u32 * level + 7) / 15) as u8;
        Rgb565::new(mix(fg.r(), bg.r()), mix(fg.g(), bg.g()), mix(fg.b(), bg.b()))
    })
}
