//! The timing tape: keying drawn to scale against time, one page at a time. A new page starts
//! when an element starts after a pause, or too near the right edge to fit. Pages start with
//! the element, not when it ends: a contact tap ends first, and would be drawn on the old page.

use pocketcw_logic::ticks;
use pocketcw_logic::timing::{Interval, Kind, Quality};

use crate::fonts::{SMALL, TINY};
use crate::gfx::{TINY_W, rect, tiny};
use crate::text::{Tenths, Text};
use crate::theme::{MAIN, Theme, quality_block, quality_color, underline};

/// Home: keyer output above the two contacts. Timing: ideal and actual marks, zoomed.
struct Layout {
    x0: i32,
    px_per_unit: i32,
    top: i32,
    bottom: i32,
    key_y: i32,
    key_h: i32,
}

const HOME: Layout = Layout { x0: 28, px_per_unit: 4, top: 72, bottom: 129, key_y: 74, key_h: 8 };
const ZOOM: Layout = Layout { x0: 26, px_per_unit: 8, top: 26, bottom: 108, key_y: 44, key_h: 14 };
const X1: i32 = MAIN - 2;
/// Dots past a page's origin that `Tape::x` tells apart.
const MAX_UNITS: u32 = 1024;
/// A dah and the gap after it: what must fit for an element to start on this page.
const ELEMENT_ROOM_UNITS: u32 = 4;
/// Home lanes for the dit and dah contacts, 12 px apart so their labels fit beside them.
const CONTACT_Y: [i32; 2] = [88, 100];
/// Home rows under the lanes: decoded letters, then gap lengths.
const HOME_LETTER_Y: i32 = 105;
const HOME_GAP_Y: i32 = 117;
const ZOOM_SET_Y: i32 = 30;
const ZOOM_PERCENT_Y: [i32; 2] = [64, 75];
const ZOOM_LETTER_Y: i32 = 88;

/// Tick time at the page's left edge, and the dot length the page is drawn at.
#[derive(Clone, Copy)]
struct Page {
    origin: u32,
    dit_ms: u16,
}

pub struct Tape {
    zoom: bool,
    /// None until the first mark.
    page: Option<Page>,
    /// Alternates the percent row so neighboring labels do not overlap.
    marks: u32,
}

impl Tape {
    pub fn new(zoom: bool) -> Tape {
        Tape { zoom, page: None, marks: 0 }
    }

    fn layout(&self) -> &'static Layout {
        if self.zoom { &ZOOM } else { &HOME }
    }

    pub fn new_page(&mut self) {
        self.page = None;
    }

    /// Starts a page one dot before `at_ms`, drawn at dots of `dit_ms`.
    fn start_page(&mut self, lcd: &mut crate::lcd::Lcd, theme: &Theme, at_ms: u32, dit_ms: u16, lanes: [&str; 3]) -> Page {
        let page = Page { origin: at_ms.wrapping_sub(dit_ms as u32), dit_ms };
        self.page = Some(page);
        self.clear(lcd, theme, lanes);
        page
    }

    /// The keyer started an element at `at_ms`: starts a new page after a pause, or when the
    /// element and its gap may not fit.
    pub fn key_down(&mut self, lcd: &mut crate::lcd::Lcd, theme: &Theme, at_ms: u32, dit_ms: u16, paused: bool, lanes: [&str; 3]) {
        if paused || self.page.is_none() || self.x(at_ms + ELEMENT_ROOM_UNITS * dit_ms as u32) > X1 {
            self.start_page(lcd, theme, at_ms, dit_ms, lanes);
        }
    }

    fn x(&self, t: u32) -> i32 {
        let l = self.layout();
        let Some(page) = self.page else { return l.x0 };
        let dit_ms = page.dit_ms as u32;
        // Past MAX_UNITS dots the x is far off screen either way. The cap keeps the product in
        // u32 for px_per_unit up to 64: a 64-bit divide costs about 0.9 KB.
        let elapsed_ms = t.wrapping_sub(page.origin).min(MAX_UNITS * dit_ms);
        l.x0 + (elapsed_ms * l.px_per_unit as u32 / dit_ms) as i32
    }

    /// Clears the tape and draws the lane labels and unit ticks.
    pub fn clear(&self, lcd: &mut crate::lcd::Lcd, theme: &Theme, lanes: [&str; 3]) {
        let l = self.layout();
        rect(lcd, 0, l.top, MAIN, l.bottom - l.top, theme.bg);
        if self.zoom {
            tiny(lcd, 2, ZOOM_SET_Y, "SET", theme.muted, theme.bg);
            tiny(lcd, 2, l.key_y + 2, "YOU", theme.muted, theme.bg);
            return;
        }
        tiny(lcd, 2, l.key_y - 2, lanes[0], theme.muted, theme.bg);
        tiny(lcd, 2, CONTACT_Y[0] - 4, lanes[1], theme.muted, theme.bg);
        tiny(lcd, 2, CONTACT_Y[1] - 4, lanes[2], theme.muted, theme.bg);
        let mut u = 0;
        while l.x0 + u * l.px_per_unit < X1 {
            rect(lcd, l.x0 + u * l.px_per_unit, l.key_y + l.key_h + 2, 1, if u % 5 == 0 { 3 } else { 1 }, theme.rule());
            u += 1;
        }
    }

    pub fn mark(&mut self, lcd: &mut crate::lcd::Lcd, theme: &Theme, iv: &Interval, iambic: bool, lanes: [&str; 3]) {
        if self.page.is_none() {
            self.start_page(lcd, theme, iv.start_ms, iv.dit_ms, lanes);
        }
        let l = self.layout();
        let x = self.x(iv.start_ms);
        if x >= X1 {
            return;
        }
        // A long straight-key mark is cut at the edge; the next element starts a new page.
        let w = self.x(iv.start_ms + iv.len_ms).min(X1) - x;
        let quality = (!iambic || self.zoom).then(|| iv.quality());
        let color = quality.map_or(theme.fg, quality_color);
        if self.zoom {
            let ideal = self.x(iv.start_ms + iv.ideal_ms()) - x;
            brackets(lcd, x, ZOOM_SET_Y, ideal, 10, theme.muted);
            quality_block(lcd, x, l.key_y, w.max(3), l.key_h, iv.quality(), theme.bg);
            let percent = (iv.error() * 100.0) as i32;
            let pct = if percent > 0 { Text::<5>::format(format_args!("+{percent}")) } else { Text::format(format_args!("{percent}")) };
            let y = ZOOM_PERCENT_Y[(self.marks % 2) as usize];
            tiny(lcd, x + w / 2 - pct.len() as i32 * TINY_W / 2, y, pct.as_str(), color, theme.bg);
            self.marks += 1;
        } else if let Some(q) = quality {
            quality_block(lcd, x, l.key_y, w.max(3), l.key_h, q, theme.bg);
        } else {
            rect(lcd, x, l.key_y, w.max(1), l.key_h, color);
        }
    }

    pub fn gap(&mut self, lcd: &mut crate::lcd::Lcd, theme: &Theme, iv: &Interval) {
        if self.page.is_none() || self.x(iv.start_ms + iv.len_ms) > X1 {
            return;
        }
        let l = self.layout();
        let x = self.x(iv.start_ms);
        let end = self.x(iv.start_ms + iv.len_ms);
        let color = quality_color(iv.quality());
        if iv.kind == Kind::ElementGap {
            let quality = iv.quality();
            if self.zoom && quality != Quality::Good {
                underline(lcd, x, l.key_y + l.key_h + 1, end - x, quality, theme.bg);
            }
            return;
        }
        if !self.zoom {
            let tick = self.x(iv.start_ms + iv.ideal_ms());
            if tick < X1 {
                rect(lcd, tick, l.key_y - 2, 1, l.key_h + 4, theme.muted);
            }
        }
        let label = if self.zoom { Text::<6>::format(format_args!("{}U", Tenths(iv.units))) } else { Text::format(format_args!("{}", Tenths(iv.units))) };
        let y = if self.zoom { ZOOM_LETTER_Y + 4 } else { HOME_GAP_Y };
        tiny(lcd, (x + end) / 2 - label.len() as i32 * TINY_W / 2, y, label.as_str(), color, theme.bg);
    }

    /// One contact closed from `since` to `until`, on lane 0 (dit or tip) or 1. A tap often
    /// ends before the keyer finishes its element, so the contact can open the page.
    #[allow(clippy::too_many_arguments)]
    pub fn contact(&mut self, lcd: &mut crate::lcd::Lcd, theme: &Theme, lane: usize, since: u32, until: u32, dit_ms: u16, lanes: [&str; 3]) {
        if self.zoom {
            return;
        }
        let page = match self.page {
            Some(page) => page,
            None => self.start_page(lcd, theme, since, dit_ms, lanes),
        };
        if let Some((x, end)) = self.span(page, since, until) {
            rect(lcd, x, CONTACT_Y[lane], end - x, 4, theme.accent);
        }
    }

    /// Both contacts closed from `since` to `until`: a squeeze, drawn as one bar across both lanes.
    pub fn squeeze(&self, lcd: &mut crate::lcd::Lcd, theme: &Theme, since: u32, until: u32) {
        let Some(page) = self.page.filter(|_| !self.zoom) else { return };
        if let Some((x, end)) = self.span(page, since, until) {
            rect(lcd, x, CONTACT_Y[0] + 4, end - x, CONTACT_Y[1] - CONTACT_Y[0] - 4, theme.accent);
        }
    }

    /// The x range of `since` to `until` on `page`, cut to the page.
    fn span(&self, page: Page, since: u32, until: u32) -> Option<(i32, i32)> {
        let since = if ticks::reached(since, page.origin) { since } else { page.origin };
        let x = self.x(since).max(HOME.x0);
        let end = self.x(until).min(X1);
        (end > x).then_some((x, end))
    }

    /// Labels a decoded letter under its marks.
    pub fn letter(&mut self, lcd: &mut crate::lcd::Lcd, theme: &Theme, c: u8, (start, end): (u32, u32)) {
        if self.page.is_none_or(|page| !ticks::reached(start, page.origin)) {
            return;
        }
        let center = (self.x(start) + self.x(end)) / 2;
        if center > X1 {
            return;
        }
        let (font, y) = if self.zoom { (&SMALL, ZOOM_LETTER_Y) } else { (&TINY, HOME_LETTER_Y) };
        let w = super::char_cells(c) as i32 * font.width as i32;
        super::draw_char(lcd, font, center - w / 2, y, c, theme.fg, theme.bg);
    }
}

fn brackets(lcd: &mut crate::lcd::Lcd, x: i32, y: i32, w: i32, h: i32, color: embedded_graphics::pixelcolor::Rgb565) {
    const K: i32 = 3;
    rect(lcd, x, y, K, 1, color);
    rect(lcd, x, y, 1, h, color);
    rect(lcd, x, y + h - 1, K, 1, color);
    rect(lcd, x + w - K, y, K, 1, color);
    rect(lcd, x + w - 1, y, 1, h, color);
    rect(lcd, x + w - K, y + h - 1, K, 1, color);
}
