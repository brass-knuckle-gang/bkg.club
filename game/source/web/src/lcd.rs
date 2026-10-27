//! The panel, as a frame buffer the page copies to a canvas.

use embedded_graphics::{pixelcolor::{Rgb565, raw::RawU16}, prelude::*, primitives::Rectangle};

pub const WIDTH: u32 = 320;
pub const HEIGHT: u32 = 240;
pub const ROW_BYTES: usize = WIDTH as usize * 3;

pub struct Lcd {
    pub fb: &'static mut [u16; (WIDTH * HEIGHT) as usize],
    /// Something changed since the page last copied the frame.
    pub dirty: bool,
}

impl Lcd {
    fn put(&mut self, p: Point, c: Rgb565) {
        if p.x >= 0 && p.y >= 0 && (p.x as u32) < WIDTH && (p.y as u32) < HEIGHT {
            self.fb[(p.y as u32 * WIDTH + p.x as u32) as usize] = RawU16::from(c).into_inner();
        }
    }
}

impl OriginDimensions for Lcd {
    fn size(&self) -> Size {
        Size::new(WIDTH, HEIGHT)
    }
}

impl DrawTarget for Lcd {
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    fn draw_iter<I: IntoIterator<Item = Pixel<Rgb565>>>(&mut self, pixels: I) -> Result<(), Self::Error> {
        self.dirty = true;
        for Pixel(p, c) in pixels {
            self.put(p, c);
        }
        Ok(())
    }

    fn fill_contiguous<I: IntoIterator<Item = Rgb565>>(&mut self, area: &Rectangle, colors: I) -> Result<(), Self::Error> {
        self.dirty = true;
        for (p, c) in area.points().zip(colors) {
            self.put(p, c);
        }
        Ok(())
    }

    fn fill_solid(&mut self, area: &Rectangle, color: Rgb565) -> Result<(), Self::Error> {
        self.dirty = true;
        let v = area.intersection(&self.bounding_box());
        let raw = RawU16::from(color).into_inner();
        for y in v.top_left.y..v.top_left.y + v.size.height as i32 {
            let row = (y as u32 * WIDTH) as usize;
            let x0 = v.top_left.x as usize;
            self.fb[row + x0..row + x0 + v.size.width as usize].fill(raw);
        }
        Ok(())
    }
}
