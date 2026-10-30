//! UI looks: a style (shapes) times a palette (colors). Screens draw their frame, header, soft
//! keys, bars and selections through `Theme`, so they never depend on the style.

use embedded_graphics::pixelcolor::{Rgb565, RgbColor};
use pocketcw_logic::settings::{PALETTE_COUNT, STYLE_COUNT};
use pocketcw_logic::timing::Quality;

use crate::gfx::{self, CHAR_W, TINY_W, ink, mix, para, rect, rgb, text, tiny};
use crate::lcd::{HEIGHT, Lcd, WIDTH};

/// Width of the main area; the soft key column fills the rest, under the header.
pub const MAIN: i32 = 272;
/// The header spans the full width; everything else starts below it.
pub const HEADER_H: i32 = 23;
const KEYS_W: i32 = WIDTH as i32 - MAIN;
const KEY_SLOT: i32 = (HEIGHT as i32 - HEADER_H) / 4;
/// The battery icon at the right end of the header: body, then the terminal nub.
const BATTERY_W: i32 = 21;
const BATTERY_X: i32 = WIDTH as i32 - 6 - BATTERY_W;
/// Right end of the header chips, left of the battery icon.
pub const CHIPS_RIGHT: i32 = BATTERY_X - 8;
/// Space between header chips.
pub const CHIP_GAP: i32 = 8;
const BATTERY_Y: i32 = 6;
pub const BATTERY_SEGMENTS: u8 = 4;

/// Timing colors, the same in every theme: good, off and bad timing as `Quality` grades it.
pub const GOOD: Rgb565 = rgb(0x3DDC84);
pub const WARN: Rgb565 = rgb(0xFFC233);
pub const BAD: Rgb565 = rgb(0xFF5A5A);

/// What the header's battery icon shows. No filled segments draws a red outline.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Battery {
    Usb,
    Segments(u8),
}

/// Every mark of timing quality has a shape as well as this color, so it reads without color
/// vision.
pub fn quality_color(q: Quality) -> Rgb565 {
    match q {
        Quality::Good => GOOD,
        Quality::Off => WARN,
        Quality::Bad => BAD,
    }
}

pub const UNDERLINE_H: i32 = 3;

/// An underline `UNDERLINE_H` pixels tall: good is a thin line, off is dashed, bad is a
/// thick bar.
pub fn underline(lcd: &mut Lcd, x: i32, y: i32, w: i32, q: Quality, bg: Rgb565) {
    rect(lcd, x, y, w, UNDERLINE_H, bg);
    let color = quality_color(q);
    match q {
        Quality::Good => rect(lcd, x, y + UNDERLINE_H - 1, w, 1, color),
        Quality::Off => {
            for dx in (0..w).step_by(4) {
                rect(lcd, x + dx, y + 1, 2.min(w - dx), 2, color);
            }
        }
        Quality::Bad => rect(lcd, x, y, w, UNDERLINE_H, color),
    }
}

/// A timed block: good is solid, off has a hollow stripe through it, bad is an outline.
pub fn quality_block(lcd: &mut Lcd, x: i32, y: i32, w: i32, h: i32, q: Quality, bg: Rgb565) {
    let color = quality_color(q);
    rect(lcd, x, y, w, h, color);
    match q {
        Quality::Good => {}
        Quality::Off => rect(lcd, x, y + h / 2 - 1, w, 2, bg),
        Quality::Bad => rect(lcd, x + 1, y + 1, w - 2, h - 2, bg),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Hazard,
    Hud,
    Block,
}

const STYLES: [(Style, &str); 3] = [(Style::Hazard, "HAZARD"), (Style::Hud, "HUD"), (Style::Block, "BLOCK")];
const _: () = assert!(STYLES.len() == STYLE_COUNT as usize);

pub fn style_name(index: u8) -> &'static str {
    STYLES[index as usize % STYLES.len()].1
}

/// Black stripes use dark gray so they show on the black screen.
pub const INK: Rgb565 = rgb(0x2C2C2C);
pub const FLAGS: [&[Rgb565]; 11] = [
    &[rgb(0xE40303), rgb(0xFF8C00), rgb(0xFFED00), rgb(0x008026), rgb(0x004DFF), rgb(0x750787)],
    &[rgb(0x5BCEFA), rgb(0xF5A9B8), rgb(0xFFFFFF), rgb(0xF5A9B8), rgb(0x5BCEFA)],
    &[rgb(0xD60270), rgb(0xD60270), rgb(0x9B4F96), rgb(0x0038A8), rgb(0x0038A8)],
    &[rgb(0xD52D00), rgb(0xFF9A56), rgb(0xFFFFFF), rgb(0xD362A4), rgb(0xA30262)],
    &[rgb(0xFF218C), rgb(0xFFD800), rgb(0x21B1FF)],
    &[rgb(0xFCF434), rgb(0xFFFFFF), rgb(0x9C59D1), INK],
    &[INK, rgb(0xA3A3A3), rgb(0xFFFFFF), rgb(0x800080)],
    &[rgb(0x3DA542), rgb(0xA7D379), rgb(0xFFFFFF), rgb(0xA9A9A9), INK],
    &[rgb(0xFF76A4), rgb(0xFFFFFF), rgb(0xC011D7), INK, rgb(0x2F3CBE)],
    &[INK, rgb(0xBCC4C7), rgb(0xFFFFFF), rgb(0xB7F684), rgb(0xFFFFFF), rgb(0xBCC4C7), INK],
    &[rgb(0xB57EDC), rgb(0xFFFFFF), rgb(0x4A8123)],
];
/// Name, accent and echo color for each flag in `FLAGS`.
const FLAG_UI: [(&str, u32, u32); 11] = [
    ("RAINBOW", 0xFF8C00, 0x3D7BFF),
    ("TRANS", 0x5BCEFA, 0xF5A9B8),
    ("BI", 0xD60270, 0x6F8FE0),
    ("LESBIAN", 0xEF7627, 0xD162A4),
    ("PAN", 0xFF218C, 0x21B1FF),
    ("NONBIN", 0xFCF434, 0x9C59D1),
    ("ACE", 0xB04DB0, 0xA3A3A3),
    ("ARO", 0x3DA542, 0xA7D379),
    ("FLUID", 0xFF76A4, 0xD24DE6),
    ("AGENDER", 0xB7F684, 0xBCC4C7),
    ("QUEER", 0xB57EDC, 0x6FA84A),
];
/// Coprime with the flag count, so consecutive boots never repeat a flag.
const FLAG_STEP: usize = 7;

/// The flag of the day: the boot screen's flag, which changes every power-on.
pub fn daily_flag(boot_count: u16) -> usize {
    boot_count as usize * FLAG_STEP % FLAGS.len()
}

struct Palette {
    name: &'static str,
    bg: u32,
    fg: u32,
    muted: u32,
    accent: u32,
    echo: u32,
    stripes: &'static [Rgb565],
}

const SOLID: [Palette; 4] = [
    Palette { name: "ORANGE", bg: 0x000000, fg: 0xF2F2F2, muted: 0x707070, accent: 0xFF5F00, echo: 0xFFFFFF, stripes: &[rgb(0xFF5F00)] },
    Palette {
        name: "CYAN",
        bg: 0x040A16,
        fg: 0xDDF7FF,
        muted: 0x4D6A85,
        accent: 0x00E5FF,
        echo: 0xFF2E88,
        stripes: &[rgb(0x00E5FF), rgb(0xFF2E88)],
    },
    Palette {
        name: "AMBER",
        bg: 0x080500,
        fg: 0xFFD9A0,
        muted: 0x7A5A2E,
        accent: 0xFFB000,
        echo: 0xFF6A00,
        stripes: &[rgb(0xFFB000), rgb(0xFF6A00)],
    },
    Palette { name: "LIME", bg: 0x000000, fg: 0xF2F2F2, muted: 0x707070, accent: 0x8CFF00, echo: 0xFFFFFF, stripes: &[rgb(0x8CFF00)] },
];
/// Palette index of the flag that changes every boot. The flags follow it in `FLAGS` order.
pub const DAILY: u8 = SOLID.len() as u8;
const FIRST_FLAG: usize = DAILY as usize + 1;
const _: () = assert!(FIRST_FLAG + FLAGS.len() == PALETTE_COUNT as usize);

pub fn palette_name(index: u8) -> &'static str {
    match index as usize {
        i if i < SOLID.len() => SOLID[i].name,
        i if i == DAILY as usize => "DAILY",
        i => FLAG_UI[(i - FIRST_FLAG) % FLAGS.len()].0,
    }
}

/// Stripes of palette `index`, for its swatch.
pub fn palette_stripes(index: u8, boot_count: u16) -> &'static [Rgb565] {
    match index as usize {
        i if i < SOLID.len() => SOLID[i].stripes,
        i if i == DAILY as usize => FLAGS[daily_flag(boot_count)],
        i => FLAGS[(i - FIRST_FLAG) % FLAGS.len()],
    }
}

/// Accent and echo colors of palette `index`.
pub fn palette_pair(index: u8, boot_count: u16) -> (Rgb565, Rgb565) {
    let (accent, echo) = match index as usize {
        i if i < SOLID.len() => (SOLID[i].accent, SOLID[i].echo),
        i if i == DAILY as usize => (FLAG_UI[daily_flag(boot_count)].1, FLAG_UI[daily_flag(boot_count)].2),
        i => (FLAG_UI[(i - FIRST_FLAG) % FLAGS.len()].1, FLAG_UI[(i - FIRST_FLAG) % FLAGS.len()].2),
    };
    (rgb(accent), rgb(echo))
}

const MAX_STRIPES: usize = 7;

pub struct Theme {
    pub style: Style,
    pub bg: Rgb565,
    pub fg: Rgb565,
    pub muted: Rgb565,
    pub accent: Rgb565,
    pub echo: Rgb565,
    /// The accent faded toward the background, for unlit bar segments and HUD lines.
    pub dim: Rgb565,
    /// Flags color bars and keys stripe by stripe; the solid palettes use the accent.
    flag: bool,
    stripes: [Rgb565; MAX_STRIPES],
    stripe_count: usize,
}

impl Theme {
    pub fn new(style: u8, palette: u8, boot_count: u16) -> Theme {
        let (bg, fg, muted) = match SOLID.get(palette as usize) {
            Some(p) => (rgb(p.bg), rgb(p.fg), rgb(p.muted)),
            None => (Rgb565::BLACK, Rgb565::WHITE, rgb(0x7C7C7C)),
        };
        let (accent, echo) = palette_pair(palette, boot_count);
        let mut stripes = [accent; MAX_STRIPES];
        let mut stripe_count = 0;
        for &color in palette_stripes(palette, boot_count).iter().filter(|&&c| c != INK) {
            stripes[stripe_count] = color;
            stripe_count += 1;
        }
        Theme {
            style: STYLES[style as usize % STYLES.len()].0,
            bg,
            fg,
            muted,
            accent,
            echo,
            dim: mix(accent, bg, 80),
            flag: palette >= DAILY,
            stripes,
            stripe_count,
        }
    }

    /// Stripe for item `i` of `n`, spreading the stripes evenly over the items.
    fn stripe(&self, i: usize, n: usize) -> Rgb565 {
        self.stripes[i * self.stripe_count / n % self.stripe_count]
    }

    /// Key `i`'s color: flag stripes, or accent and echo in turn.
    fn alternate(&self, i: usize, n: usize) -> Rgb565 {
        match (self.flag, i % 2) {
            (true, _) => self.stripe(i, n),
            (false, 0) => self.accent,
            (false, _) => self.echo,
        }
    }

    /// Tenths of a pixel per block row that block letters lean.
    pub fn slant(&self) -> i32 {
        if self.style == Style::Block { 0 } else { 6 }
    }

    /// Skew for small slanted shapes: underlines and markers.
    pub fn skew(&self) -> i32 {
        if self.style == Style::Block { 0 } else { 2 }
    }

    /// Color for dividers, grid lines and unit ticks.
    pub fn rule(&self) -> Rgb565 {
        match self.style {
            Style::Hud => self.dim,
            _ => mix(self.fg, self.bg, 82),
        }
    }

    /// Background of the big digits: Block sets them on a tile.
    pub fn panel(&self) -> Rgb565 {
        if self.style == Style::Block { mix(self.fg, self.bg, 92) } else { self.bg }
    }

    /// Clears the screen.
    pub fn frame(&self, lcd: &mut Lcd) {
        rect(lcd, 0, 0, WIDTH as i32, HEIGHT as i32, self.bg);
    }

    /// Title bar across the full width, `HEADER_H` pixels: the title in block letters. Chips
    /// go on it afterwards. Returns the right edge of the title, its slant included.
    pub fn header(&self, lcd: &mut Lcd, title: &str) -> i32 {
        const W: i32 = WIDTH as i32;
        let end = match self.style {
            Style::Hazard => {
                for (k, x) in (-4..W).step_by(8).enumerate() {
                    para(lcd, x, 0, 4, 3, -3, self.stripes[k % self.stripe_count]);
                }
                rect(lcd, 0, 22, W, 1, mix(self.fg, self.bg, 80));
                gfx::blocks(lcd, 6, 5, title, 2, 2, 4, self.accent, None)
            }
            Style::Hud => {
                rect(lcd, 0, 21, W - 40, 1, self.accent);
                rect(lcd, W - 36, 21, 30, 1, self.echo);
                gfx::blocks(lcd, 7, 5, title, 2, 2, 6, self.echo, None);
                gfx::blocks(lcd, 6, 4, title, 2, 2, 6, self.accent, None) + 1
            }
            Style::Block => {
                rect(lcd, 0, 0, W, 20, self.accent);
                const SEGMENTS: i32 = 20;
                for i in 0..SEGMENTS {
                    let x = i * W / SEGMENTS;
                    rect(lcd, x, 20, (i + 1) * W / SEGMENTS - x, 2, self.alternate(i as usize, SEGMENTS as usize));
                }
                gfx::blocks(lcd, 6, 3, title, 2, 2, 0, ink(self.accent), None)
            }
        };
        // The slant leans the top row up to 4 pixels right.
        end + 4
    }

    /// Top row and background of header chips.
    fn chip_row(&self) -> (i32, Rgb565) {
        match self.style {
            Style::Block => (2, self.accent),
            _ => (3, self.bg),
        }
    }

    /// A dial chip ending at `right`: a tiny label, then one number, or two split by a slash.
    /// The number at `focus` is inverted. Returns the chip's left edge.
    pub fn dial_chip(&self, lcd: &mut Lcd, right: i32, label: &str, numbers: &[&str], focus: Option<usize>) -> i32 {
        const LABEL_GAP: i32 = 2;
        let (y, bg) = self.chip_row();
        let (fg, focus_bg, focus_fg) = match self.style {
            Style::Block => (ink(self.accent), self.bg, self.fg),
            _ => (self.fg, self.accent, ink(self.accent)),
        };
        let boxes: i32 = numbers.iter().map(|n| n.len() as i32 * CHAR_W + 2).sum();
        let w = label.len() as i32 * TINY_W + LABEL_GAP + boxes + (numbers.len() as i32 - 1) * CHAR_W;
        let left = right - w;
        rect(lcd, left, y, w, 16, bg);
        tiny(lcd, left, y + 4, label, fg, bg);
        let mut x = left + label.len() as i32 * TINY_W + LABEL_GAP;
        for (i, number) in numbers.iter().enumerate() {
            if i > 0 {
                text(lcd, x, y, "/", fg, bg);
                x += CHAR_W;
            }
            let box_w = number.len() as i32 * CHAR_W + 2;
            let (on, under) = if focus == Some(i) { (focus_fg, focus_bg) } else { (fg, bg) };
            rect(lcd, x, y, box_w, 16, under);
            text(lcd, x + 1, y, number, on, under);
            x += box_w;
        }
        left
    }

    /// The header's battery icon; None draws nothing there.
    pub fn battery(&self, lcd: &mut Lcd, icon: Option<Battery>) {
        let (fg, bg) = match self.style {
            Style::Block => (ink(self.accent), self.accent),
            _ => (self.fg, self.bg),
        };
        let (x, y) = (BATTERY_X, BATTERY_Y);
        rect(lcd, x, y, BATTERY_W, 10, bg);
        let Some(icon) = icon else { return };
        let outline = if icon == Battery::Segments(0) { BAD } else { fg };
        rect(lcd, x, y, 19, 1, outline);
        rect(lcd, x, y + 9, 19, 1, outline);
        rect(lcd, x, y, 1, 10, outline);
        rect(lcd, x + 18, y, 1, 10, outline);
        rect(lcd, x + 19, y + 3, 2, 4, outline);
        match icon {
            Battery::Usb => {
                rect(lcd, x + 6, y + 4, 7, 2, fg);
                rect(lcd, x + 8, y + 2, 3, 6, fg);
            }
            Battery::Segments(filled) => {
                for i in 0..filled.min(BATTERY_SEGMENTS) as i32 {
                    rect(lcd, x + 2 + i * 4, y + 2, 3, 6, fg);
                }
            }
        }
    }

    /// Text chips right to left from `right`, while they stay right of `left`; chip 0 is the
    /// rightmost and brightest.
    pub fn chips(&self, lcd: &mut Lcd, chips: &[&str], left: i32, right: i32) {
        let (y, bg) = self.chip_row();
        let mut right = right;
        for (i, chip) in chips.iter().rev().enumerate() {
            if right - (chip.len() as i32 * CHAR_W) < left {
                return;
            }
            let color = match self.style {
                Style::Block => ink(self.accent),
                _ if i == 0 => self.fg,
                _ => self.muted,
            };
            gfx::text_right(lcd, right, y, chip, color, bg);
            right -= chip.len() as i32 * CHAR_W + 12;
        }
    }

    /// The F1..F4 labels, up to four characters, in the column beside the keys and under the
    /// header. An empty label marks an unused key. `holds` names what holding each key does,
    /// up to six characters, in place of or under the key's name.
    pub fn softkeys(&self, lcd: &mut Lcd, labels: [&str; 4], holds: [&str; 4]) {
        const KEY_NAMES: [&str; 4] = ["F1", "F2", "F3", "F4"];
        let x = MAIN;
        match self.style {
            Style::Hazard => {
                let column = mix(self.bg, Rgb565::WHITE, 4);
                rect(lcd, x, HEADER_H, KEYS_W, HEIGHT as i32 - HEADER_H, column);
                rect(lcd, x, HEADER_H, 1, HEIGHT as i32 - HEADER_H, mix(self.fg, self.bg, 80));
                for (i, (label, hold)) in labels.iter().zip(holds).enumerate() {
                    let y = HEADER_H + i as i32 * KEY_SLOT;
                    if i > 0 {
                        rect(lcd, x + 1, y, KEYS_W - 1, 1, mix(self.fg, self.bg, 86));
                    }
                    tiny(lcd, x + 32, y + 9, KEY_NAMES[i], mix(self.fg, self.bg, 60), column);
                    tiny(lcd, x + 4, y + 41, hold, self.muted, column);
                    if label.is_empty() {
                        text(lcd, x + 14, y + 25, "--", mix(self.fg, self.bg, 75), column);
                        continue;
                    }
                    let mark = if self.flag { self.stripe(i, 4) } else { self.accent };
                    para(lcd, x + 5, y + 12, 14, 4, 2, mark);
                    text(lcd, x + 4, y + 25, label, self.fg, column);
                }
            }
            Style::Hud => {
                rect(lcd, x, HEADER_H, KEYS_W, HEIGHT as i32 - HEADER_H, self.bg);
                rect(lcd, x, HEADER_H + 4, 1, HEIGHT as i32 - HEADER_H - 8, self.dim);
                for (i, (label, hold)) in labels.iter().zip(holds).enumerate() {
                    let y = HEADER_H + i as i32 * KEY_SLOT;
                    if label.is_empty() {
                        text(lcd, x + 14, y + 18, "--", self.dim, self.bg);
                        tiny(lcd, x + 4, y + 35, hold, self.muted, self.bg);
                        continue;
                    }
                    let color = if self.flag { self.stripe(i, 4) } else { self.accent };
                    brackets(lcd, x + 3, y + 3, 43, 48, color, 5);
                    text(lcd, x + 4, y + 18, label, self.fg, self.bg);
                    match hold {
                        "" => tiny(lcd, x + 18, y + 35, KEY_NAMES[i], self.muted, self.bg),
                        hold => tiny(lcd, x + 4, y + 35, hold, self.muted, self.bg),
                    }
                }
            }
            Style::Block => {
                rect(lcd, x, HEADER_H, KEYS_W, HEIGHT as i32 - HEADER_H, self.bg);
                for (i, (label, hold)) in labels.iter().zip(holds).enumerate() {
                    let y = HEADER_H + i as i32 * KEY_SLOT;
                    if label.is_empty() {
                        let unused = mix(self.fg, self.bg, 90);
                        rect(lcd, x + 3, y + 3, 44, KEY_SLOT - 6, unused);
                        tiny(lcd, x + 5, y + 6, hold, self.muted, unused);
                        continue;
                    }
                    let color = self.alternate(i, 4);
                    let on = ink(color);
                    rect(lcd, x + 3, y + 3, 44, KEY_SLOT - 6, color);
                    match hold {
                        "" => tiny(lcd, x + 6, y + 6, KEY_NAMES[i], mix(color, on, 55), color),
                        hold => tiny(lcd, x + 5, y + 6, hold, mix(color, on, 55), color),
                    }
                    text(lcd, x + 4, y + 28, label, on, color);
                }
            }
        }
    }

    /// A level meter showing `value` of `max`. Segments keep their shapes, so redrawing in
    /// place needs no clear.
    #[allow(clippy::too_many_arguments)]
    pub fn bar(&self, lcd: &mut Lcd, x: i32, y: i32, w: i32, h: i32, value: i32, max: i32) {
        let n = match self.style {
            Style::Hazard => 20,
            Style::Hud => 30,
            Style::Block => 12,
        };
        let lit = (value * n * 2 + max) / (2 * max);
        let unlit = if self.style == Style::Block { mix(self.fg, self.bg, 88) } else { self.dim };
        for i in 0..n {
            let (left, right) = (x + i * w / n, x + (i + 1) * w / n);
            let color = match (i < lit, self.flag) {
                (false, _) => unlit,
                (true, true) => self.stripe(i as usize, n as usize),
                (true, false) if self.style == Style::Hud && i == lit - 1 => self.echo,
                (true, false) => self.accent,
            };
            match self.style {
                Style::Hazard => para(lcd, left, y, right - left - 2, h, 2, color),
                Style::Hud => {
                    let tall = h * (45 + 55 * i / n) / 100;
                    para(lcd, left, y + h - tall, (right - left - 2).max(2), tall, 1, color);
                }
                Style::Block => rect(lcd, left, y, right - left - 2, h, color),
            }
        }
    }

    /// Highlights a selected item. Returns the text colors on it: (main, secondary, background).
    pub fn select(&self, lcd: &mut Lcd, x: i32, y: i32, w: i32, h: i32) -> (Rgb565, Rgb565, Rgb565) {
        match self.style {
            Style::Hazard => {
                para(lcd, x, y, w - 4, h, 4, self.accent);
                let on = ink(self.accent);
                (on, mix(self.accent, on, 55), self.accent)
            }
            Style::Hud => {
                rect(lcd, x, y, w, h, self.dim);
                brackets(lcd, x, y, w, h, self.echo, 4);
                (self.fg, self.echo, self.dim)
            }
            Style::Block => {
                rect(lcd, x, y, w, h, self.accent);
                let on = ink(self.accent);
                (on, mix(self.accent, on, 55), self.accent)
            }
        }
    }
}

/// Corner brackets `k` pixels long around a box.
fn brackets(lcd: &mut Lcd, x: i32, y: i32, w: i32, h: i32, color: Rgb565, k: i32) {
    let (right, bottom) = (x + w - 1, y + h - 1);
    for (cx, cy, dx, dy) in [(x, y, 1, 1), (right, y, -1, 1), (x, bottom, 1, -1), (right, bottom, -1, -1)] {
        rect(lcd, if dx > 0 { cx } else { cx - k + 1 }, cy, k, 1, color);
        rect(lcd, cx, if dy > 0 { cy } else { cy - k + 1 }, 1, k, color);
    }
}
