//! The two LCD panel types, which one a board gets, and the saved choice that overrides it.
//! The board gets the type its controller names when asked for its ID, else the type its
//! revision byte implies.

use crate::record::{self, body_mut, frame, unframe};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Panel {
    /// ST7789-style IPS, from board 2.63.
    Ips,
    Ili9341,
}

impl Panel {
    /// From option byte Data1, the board revision after "2.". An erased byte (0xFF) gets the
    /// ILI9341, as in the vendor firmware. The vendor activation writes 0x60 on any board, so an
    /// IPS board can read below 0x63: the ID read and the saved choice cover it.
    pub fn for_board(board: u8) -> Panel {
        if (0x63..0xFF).contains(&board) { Panel::Ips } else { Panel::Ili9341 }
    }

    pub fn other(self) -> Panel {
        match self {
            Panel::Ips => Panel::Ili9341,
            Panel::Ili9341 => Panel::Ips,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Panel::Ips => "IPS",
            Panel::Ili9341 => "ILI9341",
        }
    }
}

/// The panel's answer to RDDID (0x04) on one data pin: 32 bits, first bit in bit 31, read once
/// with the pin pulled up and once pulled down.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IdRead {
    pub pulled_up: u32,
    pub pulled_down: u32,
}

impl IdRead {
    /// The pulled-up answer when the panel drove the pin: both pulls read the same ID bits. The
    /// dummy clock and the bits after the ID can float, so they may differ.
    pub fn driven(self) -> Option<u32> {
        ((self.pulled_up ^ self.pulled_down) & ID_BITS == 0).then_some(self.pulled_up)
    }
}

/// The ST7789V's RDDID answer as shipped. An ILI9341 answers 00 80 00 or a module maker's ID, and
/// its ID2 is always 0x80 or more, so it cannot match.
const ST7789V_ID: u32 = 0x85_85_52;
/// Both datasheets put one dummy clock before the 24 ID bits, so the ID ends 7 bits above the
/// bottom of the 32 read.
const ID_SHIFT: u32 = 7;
const ID_BITS: u32 = 0xFF_FFFF << ID_SHIFT;

/// The panel type a driven answer names exactly, on any pin.
pub fn detect(reads: &[IdRead]) -> Option<Panel> {
    reads.iter().filter_map(|read| read.driven()).find(|answer| answer >> ID_SHIFT & 0xFF_FFFF == ST7789V_ID).map(|_| Panel::Ips)
}

/// How a panel sends its memory back for RAMRD (0x2E): leading bits to skip, the pixel format,
/// and the channel order. Neither datasheet pins these down for the serial interface.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Framing {
    pub skip_bits: usize,
    pub format: PixelFormat,
    /// Blue comes first.
    pub bgr: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PixelFormat {
    /// Two bytes a pixel, as written.
    Rgb565,
    /// Three bytes a pixel, each channel in the top 6 bits of its byte.
    Rgb666,
}

impl PixelFormat {
    pub fn bits(self) -> usize {
        match self {
            PixelFormat::Rgb565 => 16,
            PixelFormat::Rgb666 => 24,
        }
    }
}

/// Both datasheets put one dummy clock or one dummy byte before the data; the search allows two
/// bytes.
pub const MAX_SKIP_BITS: usize = 16;

/// RGB565 pixels to write and read back. Red and blue differ in every pixel but black and white,
/// so a swapped channel order cannot match.
pub const READBACK_PATTERN: [u16; 8] = [0xF800, 0x07E0, 0x001F, 0xFFFF, 0x0000, 0x8410, 0x4A69, 0xB5B6];
/// Words that hold the pattern read back at the widest format and the most skipped bits.
pub const READBACK_WORDS: usize = (MAX_SKIP_BITS + READBACK_PATTERN.len() * 24).div_ceil(32);

impl Framing {
    /// Pixel `i` of a read whose first bit is bit 31 of `bits[0]`, as RGB565; None past the end.
    /// RGB666 keeps the top 5 bits of red and blue, which hold all of what was written.
    pub fn pixel(self, bits: &[u32], i: usize) -> Option<u16> {
        let at = self.skip_bits + i * self.format.bits();
        if at + self.format.bits() > bits.len() * 32 {
            return None;
        }
        let field = |offset: usize, len: usize| (0..len).fold(0u16, |v, k| v << 1 | bit(bits, at + offset + k) as u16);
        let (first, green, last) = match self.format {
            PixelFormat::Rgb565 => (field(0, 5), field(5, 6), field(11, 5)),
            PixelFormat::Rgb666 => (field(0, 5), field(8, 6), field(16, 5)),
        };
        let (red, blue) = if self.bgr { (last, first) } else { (first, last) };
        Some(red << 11 | green << 5 | blue)
    }
}

fn bit(bits: &[u32], i: usize) -> bool {
    bits[i / 32] >> (31 - i % 32) & 1 != 0
}

/// The framing under which `bits` read back as `expected`, fewest skipped bits first.
pub fn find_framing(bits: &[u32], expected: &[u16]) -> Option<Framing> {
    (0..=MAX_SKIP_BITS)
        .flat_map(|skip_bits| {
            [PixelFormat::Rgb666, PixelFormat::Rgb565]
                .into_iter()
                .flat_map(move |format| [false, true].map(move |bgr| Framing { skip_bits, format, bgr }))
        })
        .find(|framing| expected.iter().enumerate().all(|(i, &want)| framing.pixel(bits, i) == Some(want)))
}

/// What picked the panel type.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    /// A choice saved with F2 at power-on.
    Saved,
    /// The controller's ID.
    Id,
    /// The board revision.
    Board,
}

/// The saved choice wins, then the controller's ID, then the board revision.
pub fn choose(saved: Option<Panel>, detected: Option<Panel>, board: u8) -> (Panel, Source) {
    match (saved, detected) {
        (Some(panel), _) => (panel, Source::Saved),
        (None, Some(panel)) => (panel, Source::Id),
        (None, None) => (Panel::for_board(board), Source::Board),
    }
}

/// What to save so `panel` shows: None when `auto`, the type chosen without a saved choice,
/// already is `panel`.
pub fn saved_choice(panel: Panel, auto: Panel) -> Option<Panel> {
    (panel != auto).then_some(panel)
}

const MAGIC: [u8; 2] = *b"Rp";
const VERSION: u8 = 1;
/// Body: choice (0 none, 1 IPS, 2 ILI9341).
pub const RECORD_LEN: usize = 1 + record::OVERHEAD;

pub fn encode(saved: Option<Panel>) -> [u8; RECORD_LEN] {
    let choice = match saved {
        None => 0,
        Some(Panel::Ips) => 1,
        Some(Panel::Ili9341) => 2,
    };
    let mut record = [0; RECORD_LEN];
    body_mut(&mut record)[0] = choice;
    frame(&mut record, MAGIC, VERSION);
    record
}

/// The choice in the record at the start of `slot`; None for no choice, and for a blank,
/// foreign or damaged record.
pub fn decode(slot: &[u8]) -> Option<Panel> {
    match unframe(slot, MAGIC, VERSION)?.first()? {
        1 => Some(Panel::Ips),
        2 => Some(Panel::Ili9341),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn board_revision_picks_the_panel() {
        assert_eq!(Panel::for_board(0x60), Panel::Ili9341);
        assert_eq!(Panel::for_board(0x62), Panel::Ili9341);
        assert_eq!(Panel::for_board(0x63), Panel::Ips);
        assert_eq!(Panel::for_board(0x64), Panel::Ips);
        assert_eq!(Panel::for_board(0xFF), Panel::Ili9341, "erased option byte");
    }

    #[test]
    fn saved_choice_wins_then_id_then_board() {
        assert_eq!(choose(None, None, 0x64), (Panel::Ips, Source::Board));
        assert_eq!(choose(None, Some(Panel::Ips), 0x60), (Panel::Ips, Source::Id));
        assert_eq!(choose(Some(Panel::Ili9341), Some(Panel::Ips), 0x64), (Panel::Ili9341, Source::Saved));
    }

    #[test]
    fn flipping_twice_returns_to_no_choice() {
        for (detected, board) in [(None, 0x60), (Some(Panel::Ips), 0x60)] {
            let auto = choose(None, detected, board).0;
            let first = saved_choice(auto.other(), auto);
            assert_eq!(first, Some(auto.other()));
            let second = saved_choice(choose(first, detected, board).0.other(), auto);
            assert_eq!(second, None);
        }
    }

    /// `id` placed after `dummy_bits` leading bits, padded with ones below, as a pulled-up line
    /// reads after the panel stops driving.
    fn answer(id: u32, dummy_bits: u32) -> u32 {
        (id << (8 - dummy_bits)) | ((1 << (8 - dummy_bits)) - 1)
    }

    fn steady(bits: u32) -> IdRead {
        IdRead { pulled_up: bits, pulled_down: bits }
    }

    /// `id` after one dummy clock, with the dummy and the bits after the ID left floating.
    fn released(id: u32) -> IdRead {
        IdRead { pulled_up: 1 << 31 | id << ID_SHIFT | 0x7F, pulled_down: id << ID_SHIFT }
    }

    #[test]
    fn st7789v_id_after_one_dummy_clock_is_ips() {
        assert_eq!(detect(&[steady(answer(0x85_85_52, 1))]), Some(Panel::Ips));
        assert_eq!(detect(&[steady(0xFFFF_FFFF), steady(answer(0x85_85_52, 1))]), Some(Panel::Ips), "either pin");
    }

    #[test]
    fn other_answers_detect_nothing() {
        assert_eq!(detect(&[steady(answer(0x85_85_52, 0))]), None, "no dummy clock");
        assert_eq!(detect(&[steady(answer(0x85_85_52, 2))]), None, "two dummy clocks");
        assert_eq!(detect(&[steady(answer(0x00_80_00, 1))]), None, "ILI9341 default");
        assert_eq!(detect(&[]), None);
    }

    #[test]
    fn a_pin_nobody_drives_is_ignored() {
        let id = answer(0x85_85_52, 1);
        assert_eq!(IdRead { pulled_up: id, pulled_down: id }.driven(), Some(id));
        assert_eq!(IdRead { pulled_up: 0xFFFF_FFFF, pulled_down: 0 }.driven(), None);
        let id_bit_differs = IdRead { pulled_up: id, pulled_down: id ^ 1 << ID_SHIFT };
        assert_eq!(id_bit_differs.driven(), None, "all 24 ID bits must agree");
        assert_eq!(detect(&[id_bit_differs]), None);
    }

    #[test]
    fn floating_bits_around_the_id_still_detect() {
        assert_eq!(released(0x85_85_52).driven(), Some(released(0x85_85_52).pulled_up));
        assert_eq!(detect(&[released(0x85_85_52)]), Some(Panel::Ips));
    }

    /// `pixels` as a panel sends them under `framing`. Skipped bits and the unused low bits of
    /// RGB666 bytes read as ones, as on a pulled-up line.
    fn read_back(framing: Framing, pixels: &[u16]) -> [u32; READBACK_WORDS] {
        let mut bits = [u32::MAX; READBACK_WORDS];
        let mut at = framing.skip_bits;
        let mut put = |value: u16, len: usize| {
            for k in (0..len).rev() {
                if value >> k & 1 == 0 {
                    bits[at / 32] &= !(1 << (31 - at % 32));
                }
                at += 1;
            }
        };
        for &p in pixels {
            let (red, green, blue) = (p >> 11, p >> 5 & 0x3F, p & 0x1F);
            let (first, last) = if framing.bgr { (blue, red) } else { (red, blue) };
            match framing.format {
                PixelFormat::Rgb565 => [(first, 5), (green, 6), (last, 5)].iter().for_each(|&(v, len)| put(v, len)),
                PixelFormat::Rgb666 => {
                    let widen = |c: u16| (c << 1 | c >> 4) << 2 | 0b11;
                    [widen(first), green << 2 | 0b11, widen(last)].iter().for_each(|&byte| put(byte, 8));
                }
            }
        }
        bits
    }

    #[test]
    fn each_framing_is_found_and_no_other() {
        for skip_bits in [0, 1, 7, 8, 9, MAX_SKIP_BITS] {
            for format in [PixelFormat::Rgb565, PixelFormat::Rgb666] {
                for bgr in [false, true] {
                    let framing = Framing { skip_bits, format, bgr };
                    let bits = read_back(framing, &READBACK_PATTERN);
                    assert_eq!(find_framing(&bits, &READBACK_PATTERN), Some(framing));
                    let decoded: [Option<u16>; 8] = core::array::from_fn(|i| framing.pixel(&bits, i));
                    assert_eq!(decoded, READBACK_PATTERN.map(Some));
                }
            }
        }
    }

    #[test]
    fn a_read_without_the_pattern_has_no_framing() {
        assert_eq!(find_framing(&[u32::MAX; READBACK_WORDS], &READBACK_PATTERN), None, "nobody drives the line");
        assert_eq!(find_framing(&[0; READBACK_WORDS], &READBACK_PATTERN), None);
        let late = Framing { skip_bits: MAX_SKIP_BITS + 1, format: PixelFormat::Rgb666, bgr: false };
        assert_eq!(find_framing(&read_back(late, &READBACK_PATTERN), &READBACK_PATTERN), None, "past the most skipped bits");
    }

    #[test]
    fn record_round_trips_and_rejects_damage() {
        for saved in [None, Some(Panel::Ips), Some(Panel::Ili9341)] {
            assert_eq!(decode(&encode(saved)), saved);
        }
        assert_eq!(decode(&[0xFF; RECORD_LEN]), None);
        let mut record = encode(Some(Panel::Ips));
        record[4] = 2;
        assert_eq!(decode(&record), None, "CRC no longer matches");
    }
}
