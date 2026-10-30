//! The log ring in SPI flash: a raw transcript of what went out on the air, in 4 KB sectors. The
//! format is in `docs/dev/roadmap.md`, "Log format". The ring makes every write through `Flash`,
//! so host tests run it on a model of the chip.

use core::ops::Range;

use crate::record::{self, BODY_START, OVERHEAD};
use crate::text::Text;

/// The only range PocketCW erases or programs.
pub const LOG: Range<u32> = 0xC0_0000..0xE0_0000;
pub const SECTOR: u32 = 4096;
pub const PAGE: u32 = 256;
pub const SECTORS: u16 = ((LOG.end - LOG.start) / SECTOR) as u16;
pub const MAX_TEXT: usize = 242;
pub const CALL_LEN: usize = 10;
/// The highest part number, since bit 7 of the part byte is the more flag.
pub const LAST_PART: u8 = 127;
/// A frame with a 255-byte body.
pub const MAX_FRAME: usize = 255 + OVERHEAD;

/// Every kind stays at version 1 while its fields only append. A sector header must never change
/// version: older firmware erases a sector whose header it cannot read.
const VERSION: u8 = 1;
const HEADER: [u8; 2] = *b"LH";
const SESSION: [u8; 2] = *b"LS";
const RUN: [u8; 2] = *b"LT";
const MORE: u8 = 0x80;
const RUN_TEXT_AT: usize = 13;
const SESSION_CALL_AT: usize = 12;

/// Whether an erase or program of `len` bytes at `address` stays inside `LOG`.
pub fn allowed(address: u32, len: u32) -> bool {
    len > 0 && address >= LOG.start && address.checked_add(len).is_some_and(|end| end <= LOG.end)
}

/// The chip, as the ring uses it. Addresses are absolute.
pub trait Flash {
    fn read(&mut self, address: u32, buf: &mut [u8]);
    /// Erases the 4 KB sector at `address`.
    fn erase(&mut self, address: u32);
    /// Programs `data`, which does not cross a page.
    fn program(&mut self, address: u32, data: &[u8]);
}

/// The values are persisted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    You = 0,
    Memory = 1,
    Note = 2,
    Them = 3,
}

const SOURCES: [Source; 4] = [Source::You, Source::Memory, Source::Note, Source::Them];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Run {
    pub source: Source,
    /// Counted per session.
    pub run: u16,
    pub part: u8,
    /// More parts of this run follow.
    pub more: bool,
    /// This part's own span, in ms since boot.
    pub start: u32,
    pub end: u32,
    pub text: Text<MAX_TEXT>,
}

/// One record at a time is on the stack, so the large `Run` costs nothing a box would save.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Record {
    /// The first record of every sector: its place in the ring, and the session open then.
    Header { sequence: u32, session: u32 },
    Session { session: u32, start: u32, version: [u8; 3], call: Text<CALL_LEN> },
    Run(Run),
}

impl Record {
    /// Frames the record into `out` and returns the frame.
    pub fn encode<'a>(&self, out: &'a mut [u8; MAX_FRAME]) -> &'a [u8] {
        let mut body = [0; MAX_FRAME - OVERHEAD];
        let mut len = 0;
        let mut put = |bytes: &[u8]| {
            body[len..len + bytes.len()].copy_from_slice(bytes);
            len += bytes.len();
        };
        let magic = match self {
            Record::Header { sequence, session } => {
                put(&sequence.to_le_bytes());
                put(&session.to_le_bytes());
                HEADER
            }
            Record::Session { session, start, version, call } => {
                put(&session.to_le_bytes());
                put(&start.to_le_bytes());
                put(version);
                put(&[call.len() as u8]);
                put(call.as_str().as_bytes());
                SESSION
            }
            Record::Run(r) => {
                put(&[r.source as u8]);
                put(&r.run.to_le_bytes());
                put(&[r.part & !MORE | if r.more { MORE } else { 0 }]);
                put(&r.start.to_le_bytes());
                put(&r.end.to_le_bytes());
                put(&[r.text.len() as u8]);
                put(r.text.as_str().as_bytes());
                RUN
            }
        };
        let frame = &mut out[..len + OVERHEAD];
        record::body_mut(frame).copy_from_slice(&body[..len]);
        record::frame(frame, magic, VERSION);
        frame
    }

    /// The record in a valid frame; None for an unknown kind or version, or a body too short for
    /// its fields and text.
    pub fn decode(frame: &[u8]) -> Option<Record> {
        let (magic, version, body) = record::read_frame(frame)?;
        if version != VERSION {
            return None;
        }
        let u32_at = |i: usize| body.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        let text_at = |i: usize| body.get(i + 1..i + 1 + *body.get(i)? as usize);
        match magic {
            HEADER => Some(Record::Header { sequence: u32_at(0)?, session: u32_at(4)? }),
            SESSION => Some(Record::Session {
                session: u32_at(0)?,
                start: u32_at(4)?,
                version: body.get(8..11)?.try_into().ok()?,
                call: text(text_at(SESSION_CALL_AT - 1)?)?,
            }),
            RUN => Some(Record::Run(Run {
                source: *SOURCES.get(*body.first()? as usize)?,
                run: u16::from_le_bytes([*body.get(1)?, *body.get(2)?]),
                part: body.get(3)? & !MORE,
                more: body.get(3)? & MORE != 0,
                start: u32_at(4)?,
                end: u32_at(8)?,
                text: text(text_at(RUN_TEXT_AT - 1)?)?,
            })),
            _ => None,
        }
    }
}

/// `bytes` as text; None when it holds a byte that is not ASCII or does not fit.
fn text<const N: usize>(bytes: &[u8]) -> Option<Text<N>> {
    let mut t = Text::new();
    bytes.iter().all(|&b| t.push(b)).then_some(t)
}

/// What a sector holds at an offset.
enum Found {
    /// A valid frame of this many bytes, now in the buffer.
    Frame(usize),
    End,
    Torn,
}

fn read_at(flash: &mut impl Flash, sector: u32, offset: u32, buf: &mut [u8; MAX_FRAME]) -> Found {
    if offset + BODY_START as u32 > SECTOR {
        return Found::End;
    }
    let mut header = [0; BODY_START];
    flash.read(sector + offset, &mut header);
    if header[0] == 0xFF {
        return Found::End;
    }
    let len = record::frame_len(header);
    if offset + len as u32 > SECTOR {
        return Found::Torn;
    }
    flash.read(sector + offset, &mut buf[..len]);
    match record::read_frame(&buf[..len]) {
        Some(_) => Found::Frame(len),
        None => Found::Torn,
    }
}

/// The sequence and session in the header of the sector at `sector`.
fn header(flash: &mut impl Flash, sector: u32) -> Option<(u32, u32)> {
    let mut buf = [0; MAX_FRAME];
    let Found::Frame(len) = read_at(flash, sector, 0, &mut buf) else { return None };
    match Record::decode(&buf[..len])? {
        Record::Header { sequence, session } => Some((sequence, session)),
        _ => None,
    }
}

/// The session a header or session start frame names. A session start that fails to decode in
/// full, such as one with a longer call, still counts, so its number is not used again.
fn session_of(frame: &[u8]) -> Option<u32> {
    let (magic, version, body) = record::read_frame(frame)?;
    let session = body.get(4 * (magic == HEADER) as usize..)?.first_chunk::<4>()?;
    (version == VERSION && (magic == HEADER || magic == SESSION)).then(|| u32::from_le_bytes(*session))
}

/// Whether the sector at `sector` reads 0xFF from `offset` on. A program cut off by power loss
/// can leave a first byte of 0xFF with bits cleared after it, which a later program would garble.
fn blank_from(flash: &mut impl Flash, sector: u32, offset: u32) -> bool {
    let mut buf = [0; PAGE as usize];
    (offset..SECTOR).step_by(PAGE as usize).all(|at| {
        let piece = &mut buf[..(SECTOR - at).min(PAGE) as usize];
        flash.read(sector + at, piece);
        piece.iter().all(|&b| b == 0xFF)
    })
}

fn sector_address(sector: u16) -> u32 {
    LOG.start + sector as u32 * SECTOR
}

/// Programs `data` a page at a time.
fn program(flash: &mut impl Flash, mut address: u32, mut data: &[u8]) {
    while !data.is_empty() {
        let room = (PAGE - address % PAGE) as usize;
        let (now, rest) = data.split_at(room.min(data.len()));
        flash.program(address, now);
        address += now.len() as u32;
        data = rest;
    }
}

/// Where a walk of the ring is: a sector, counted on from the head, and an offset in it.
pub struct Cursor {
    after_head: u16,
    offset: u32,
}

pub struct Ring {
    sectors: u16,
    head: u16,
    /// Where the next record goes in the head sector. `SECTOR` closes the head.
    offset: u32,
    sequence: u32,
    /// This power-on's session.
    session: u32,
    /// The sector after the head was erased since startup.
    next_erased: bool,
}

impl Ring {
    /// Finds the head among the first `sectors` sectors of `LOG`, and where its data ends. With no
    /// valid header anywhere, the first write goes to sector 0 as sequence 1 of session 1.
    pub fn open(flash: &mut impl Flash, sectors: u16) -> Ring {
        assert!((2..=SECTORS).contains(&sectors), "a ring needs a sector to erase ahead and must stay in LOG");
        let head = (0..sectors)
            .filter_map(|s| header(flash, sector_address(s)).map(|(sequence, _)| (s, sequence)))
            .max_by_key(|&(_, sequence)| sequence);
        let Some((head, sequence)) = head else {
            return Ring { sectors, head: sectors - 1, offset: SECTOR, sequence: 0, session: 1, next_erased: false };
        };
        let mut buf = [0; MAX_FRAME];
        let (mut offset, mut last_session) = (0, 0);
        loop {
            match read_at(flash, sector_address(head), offset, &mut buf) {
                Found::Frame(len) => {
                    last_session = last_session.max(session_of(&buf[..len]).unwrap_or(0));
                    offset += len as u32;
                }
                Found::End if blank_from(flash, sector_address(head), offset) => break,
                Found::End | Found::Torn => {
                    offset = SECTOR;
                    break;
                }
            }
        }
        Ring { sectors, head, offset, sequence, session: last_session + 1, next_erased: false }
    }

    pub fn session(&self) -> u32 {
        self.session
    }

    /// Writes `record` after the others, moving to the next sector when it does not fit.
    pub fn append(&mut self, flash: &mut impl Flash, record: &Record) {
        let mut buf = [0; MAX_FRAME];
        let frame = record.encode(&mut buf);
        if self.offset + frame.len() as u32 > SECTOR {
            self.advance(flash);
        }
        program(flash, sector_address(self.head) + self.offset, frame);
        self.offset += frame.len() as u32;
    }

    /// Erases the sector after the head once the head is half full, so the move to it needs only
    /// page programs. True when it started an erase.
    pub fn erase_ahead(&mut self, flash: &mut impl Flash) -> bool {
        if self.next_erased || self.offset < SECTOR / 2 {
            return false;
        }
        flash.erase(sector_address(self.next()));
        self.next_erased = true;
        true
    }

    /// A walk of every valid frame, of known kinds or not, from the oldest sector to the head.
    pub fn cursor(&self) -> Cursor {
        Cursor { after_head: 1, offset: 0 }
    }

    /// The next frame of the walk, read into `buf`; None after the head. A sector with no valid
    /// header is passed over.
    pub fn next_frame<'a>(&self, flash: &mut impl Flash, cursor: &mut Cursor, buf: &'a mut [u8; MAX_FRAME]) -> Option<&'a [u8]> {
        while cursor.after_head <= self.sectors {
            let sector = sector_address((self.head + cursor.after_head) % self.sectors);
            if (cursor.offset > 0 || header(flash, sector).is_some())
                && let Found::Frame(len) = read_at(flash, sector, cursor.offset, buf)
            {
                cursor.offset += len as u32;
                return Some(&buf[..len]);
            }
            (cursor.after_head, cursor.offset) = (cursor.after_head + 1, 0);
        }
        None
    }

    fn next(&self) -> u16 {
        (self.head + 1) % self.sectors
    }

    /// Moves the head to the next sector: erased unless erased ahead, then headed.
    fn advance(&mut self, flash: &mut impl Flash) {
        self.head = self.next();
        self.sequence += 1;
        let sector = sector_address(self.head);
        if !core::mem::take(&mut self.next_erased) {
            flash.erase(sector);
        }
        let mut buf = [0; MAX_FRAME];
        let header = Record::Header { sequence: self.sequence, session: self.session }.encode(&mut buf);
        program(flash, sector, header);
        self.offset = header.len() as u32;
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    const SMALL: u16 = 4;

    /// NOR flash: erase sets bytes to 0xFF, and a program can only clear bits.
    struct Chip {
        bytes: Vec<u8>,
        erases: usize,
    }

    impl Chip {
        fn new() -> Chip {
            Chip { bytes: std::vec![0xFF; SECTORS as usize * SECTOR as usize], erases: 0 }
        }

        fn at(address: u32) -> usize {
            (address - LOG.start) as usize
        }
    }

    impl Flash for Chip {
        fn read(&mut self, address: u32, buf: &mut [u8]) {
            let at = Chip::at(address);
            buf.copy_from_slice(&self.bytes[at..at + buf.len()]);
        }

        fn erase(&mut self, address: u32) {
            assert!(allowed(address, SECTOR) && address.is_multiple_of(SECTOR));
            let at = Chip::at(address);
            self.bytes[at..at + SECTOR as usize].fill(0xFF);
            self.erases += 1;
        }

        fn program(&mut self, address: u32, data: &[u8]) {
            assert!(allowed(address, data.len() as u32));
            assert_eq!(address / PAGE, (address + data.len() as u32 - 1) / PAGE, "crosses a page");
            let at = Chip::at(address);
            for (byte, &d) in self.bytes[at..].iter_mut().zip(data) {
                *byte &= d;
            }
        }
    }

    fn run(n: u16, part: u8, more: bool, text: &str) -> Record {
        let text = Text::format(format_args!("{text}"));
        Record::Run(Run { source: Source::You, run: n, part, more, start: 1000 * n as u32, end: 1000 * n as u32 + 500, text })
    }

    fn session(n: u32) -> Record {
        Record::Session { session: n, start: 42, version: [0, 3, 0], call: Text::format(format_args!("K5MME")) }
    }

    fn records(ring: &Ring, chip: &mut Chip) -> Vec<Option<Record>> {
        let (mut out, mut cursor, mut buf) = (Vec::new(), ring.cursor(), [0; MAX_FRAME]);
        while let Some(frame) = ring.next_frame(chip, &mut cursor, &mut buf) {
            out.push(Record::decode(frame));
        }
        out
    }

    fn runs(ring: &Ring, chip: &mut Chip) -> Vec<(u16, u8)> {
        records(ring, chip)
            .into_iter()
            .filter_map(|r| match r {
                Some(Record::Run(r)) => Some((r.run, r.part)),
                _ => None,
            })
            .collect()
    }

    /// A run of `MAX_TEXT` characters: 261 bytes framed, so 15 fill a sector after its header.
    fn long(n: u16, part: u8) -> Record {
        run(n, part, true, &"E".repeat(MAX_TEXT))
    }

    #[test]
    fn allowed_is_exactly_the_log_range() {
        assert!(allowed(LOG.start, SECTOR));
        assert!(allowed(LOG.end - 1, 1));
        assert!(!allowed(LOG.start - 1, 2));
        assert!(!allowed(LOG.end - 1, 2));
        assert!(!allowed(LOG.start, 0));
        assert!(!allowed(u32::MAX, 2));
        assert!(!allowed(0x10_0000, SECTOR), "the content pack");
    }

    #[test]
    fn records_read_back() {
        let mut buf = [0; MAX_FRAME];
        for record in [Record::Header { sequence: 7, session: 3 }, session(3), run(2, 5, true, "CQ POTA DE K5MME <AR> *"), long(1, 0)] {
            assert_eq!(Record::decode(record.encode(&mut buf)), Some(record));
        }
        assert_eq!(long(1, 0).encode(&mut buf).len(), MAX_FRAME);
        let frame = run(1, 0, false, "TU").encode(&mut buf).to_vec();
        assert_eq!(&frame[..2], b"LT");
        let mut bad = frame.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert_eq!(Record::decode(&bad), None, "a bad CRC");
        assert_eq!(run(0, 200, false, "").encode(&mut buf)[BODY_START + 3], 200 & !MORE, "a part never sets the more bit");
    }

    #[test]
    fn longer_bodies_from_later_firmware_still_decode() {
        let mut buf = [0; MAX_FRAME];
        for record in [Record::Header { sequence: 7, session: 3 }, session(3), run(2, 5, true, "CQ")] {
            let frame = record.encode(&mut buf);
            let body_len = frame.len() - OVERHEAD;
            let mut longer = std::vec![0; frame.len() + 3];
            record::body_mut(&mut longer)[..body_len].copy_from_slice(&frame[BODY_START..BODY_START + body_len]);
            record::frame(&mut longer, [frame[0], frame[1]], VERSION);
            assert_eq!(Record::decode(&longer), Some(record));
        }
    }

    #[test]
    fn a_body_too_short_for_its_text_is_skipped() {
        let mut frame = [0; 13 + OVERHEAD];
        record::body_mut(&mut frame)[12] = 5;
        record::frame(&mut frame, RUN, VERSION);
        assert_eq!(Record::decode(&frame), None);
    }

    #[test]
    fn the_first_write_goes_to_sector_0_as_session_1() {
        let mut chip = Chip::new();
        let mut ring = Ring::open(&mut chip, SMALL);
        assert_eq!(ring.session(), 1);
        ring.append(&mut chip, &session(1));
        assert_eq!(header(&mut chip, LOG.start), Some((1, 1)));
        let ring = Ring::open(&mut chip, SMALL);
        assert_eq!((ring.head, ring.session()), (0, 2));
        assert_eq!(records(&ring, &mut chip), [Some(Record::Header { sequence: 1, session: 1 }), Some(session(1))]);
    }

    #[test]
    fn sessions_replay_in_order() {
        let mut chip = Chip::new();
        for n in 1..=3 {
            let mut ring = Ring::open(&mut chip, SMALL);
            assert_eq!(ring.session(), n);
            ring.append(&mut chip, &session(n));
            ring.append(&mut chip, &run(0, 0, false, "CQ"));
        }
        let ring = Ring::open(&mut chip, SMALL);
        let sessions: Vec<u32> = records(&ring, &mut chip)
            .into_iter()
            .filter_map(|r| match r {
                Some(Record::Session { session, .. }) => Some(session),
                _ => None,
            })
            .collect();
        assert_eq!(sessions, [1, 2, 3]);
    }

    #[test]
    fn a_session_start_that_does_not_decode_still_takes_its_number() {
        let mut chip = Chip::new();
        let mut ring = Ring::open(&mut chip, SMALL);
        ring.append(&mut chip, &run(0, 0, false, "CQ"));
        let mut frame = [0; 12 + 11 + OVERHEAD];
        let body = record::body_mut(&mut frame);
        body[..4].copy_from_slice(&7u32.to_le_bytes());
        body[11] = 11;
        body[12..].copy_from_slice(b"K5MME/PORTA");
        record::frame(&mut frame, SESSION, VERSION);
        assert_eq!(Record::decode(&frame), None, "a call longer than 10");
        program(&mut chip, sector_address(ring.head) + ring.offset, &frame);
        assert_eq!(Ring::open(&mut chip, SMALL).session(), 8);
    }

    #[test]
    fn the_next_session_follows_the_head_header_without_a_session_record() {
        let mut chip = Chip::new();
        let mut buf = [0; MAX_FRAME];
        program(&mut chip, LOG.start, Record::Header { sequence: 9, session: 5 }.encode(&mut buf));
        assert_eq!(Ring::open(&mut chip, SMALL).session(), 6);
    }

    #[test]
    fn a_wrap_drops_the_oldest_sector_and_reads_oldest_first() {
        let mut chip = Chip::new();
        let mut ring = Ring::open(&mut chip, SMALL);
        // 15 parts a sector: 70 parts fill 4 sectors and wrap into a fifth.
        for part in 0..70 {
            ring.append(&mut chip, &long(0, part));
        }
        let ring = Ring::open(&mut chip, SMALL);
        assert_eq!((ring.head, ring.sequence), (0, 5));
        let parts = runs(&ring, &mut chip);
        assert_eq!(parts.first(), Some(&(0, 15)), "the oldest sector starts mid-run");
        assert_eq!(parts.last(), Some(&(0, 69)));
        assert!(parts.windows(2).all(|w| w[1].1 == w[0].1 + 1));
        // A torn erase took sector 2's header.
        chip.bytes[Chip::at(sector_address(2))] = 0;
        let parts: Vec<u8> = runs(&ring, &mut chip).into_iter().map(|(_, part)| part).collect();
        assert_eq!(parts, (15..30).chain(45..70).collect::<Vec<u8>>(), "a headerless sector is passed over");
    }

    #[test]
    fn a_torn_record_closes_the_head() {
        let mut chip = Chip::new();
        let mut ring = Ring::open(&mut chip, SMALL);
        ring.append(&mut chip, &run(0, 0, false, "CQ CQ"));
        ring.append(&mut chip, &run(1, 0, false, "DE K5MME"));
        // Power lost before the CRC.
        let end = Chip::at(LOG.start + ring.offset);
        chip.bytes[end - 2..end].fill(0xFF);
        let mut ring = Ring::open(&mut chip, SMALL);
        assert_eq!(ring.offset, SECTOR);
        ring.append(&mut chip, &run(2, 0, false, "TU"));
        assert_eq!(ring.head, 1);
        assert_eq!(runs(&ring, &mut chip), [(0, 0), (2, 0)]);
    }

    #[test]
    fn a_program_cut_off_after_its_first_byte_closes_the_head() {
        let mut chip = Chip::new();
        let mut ring = Ring::open(&mut chip, SMALL);
        ring.append(&mut chip, &run(0, 0, false, "CQ"));
        // The magic byte still reads 0xFF, but later bits are clear.
        chip.bytes[Chip::at(sector_address(0) + ring.offset + 10)] = 0x12;
        let mut ring = Ring::open(&mut chip, SMALL);
        assert_eq!(ring.offset, SECTOR);
        ring.append(&mut chip, &run(1, 0, false, "TU"));
        assert_eq!((ring.head, runs(&ring, &mut chip)), (1, std::vec![(0, 0), (1, 0)]));
    }

    #[test]
    fn a_sector_without_a_header_is_erased_before_use() {
        let mut chip = Chip::new();
        // A torn erase: bits left clear in sector 1, with no header.
        let at = Chip::at(sector_address(1));
        chip.bytes[at + 100..at + 200].fill(0);
        let mut ring = Ring::open(&mut chip, SMALL);
        ring.append(&mut chip, &run(0, 0, false, "CQ"));
        for part in 0..16 {
            ring.append(&mut chip, &long(1, part));
        }
        assert_eq!(ring.head, 1);
        assert_eq!(runs(&ring, &mut chip).len(), 17);
        assert!(chip.bytes[at + 100..at + 200].iter().any(|&b| b != 0), "erased, then written");
    }

    #[test]
    fn an_unknown_kind_between_known_ones_is_skipped() {
        let mut chip = Chip::new();
        let mut ring = Ring::open(&mut chip, SMALL);
        ring.append(&mut chip, &run(0, 0, false, "CQ"));
        for (magic, version) in [(*b"LX", 1), (RUN, 2)] {
            let mut frame = [0; 5 + OVERHEAD];
            record::body_mut(&mut frame).copy_from_slice(b"FUTUR");
            record::frame(&mut frame, magic, version);
            program(&mut chip, sector_address(ring.head) + ring.offset, &frame);
            ring.offset += frame.len() as u32;
        }
        ring.append(&mut chip, &run(1, 0, false, "TU"));
        let mut ring = Ring::open(&mut chip, SMALL);
        assert_eq!(records(&ring, &mut chip).len(), 5, "the header, two runs and two unknown frames");
        assert_eq!(runs(&ring, &mut chip), [(0, 0), (1, 0)]);
        ring.append(&mut chip, &run(2, 0, false, "73"));
        assert_eq!(runs(&ring, &mut chip), [(0, 0), (1, 0), (2, 0)], "writing goes on after them");
    }

    #[test]
    fn erase_ahead_saves_the_erase_at_the_move() {
        let mut chip = Chip::new();
        // Old data in sector 1, which only the erase ahead clears.
        let at = Chip::at(sector_address(1));
        chip.bytes[at..at + SECTOR as usize].fill(0x55);
        let mut ring = Ring::open(&mut chip, SMALL);
        ring.append(&mut chip, &run(0, 0, false, "CQ"));
        let erases = chip.erases;
        assert!(!ring.erase_ahead(&mut chip), "the head is not half full");
        for part in 0..8 {
            ring.append(&mut chip, &long(1, part));
        }
        assert!(ring.erase_ahead(&mut chip));
        assert!(!ring.erase_ahead(&mut chip));
        for part in 8..16 {
            ring.append(&mut chip, &long(1, part));
        }
        assert_eq!((ring.head, chip.erases), (1, erases + 1));
        assert_eq!(runs(&ring, &mut chip).len(), 17);
        let ring = Ring::open(&mut chip, SMALL);
        assert_eq!((ring.head, ring.next_erased), (1, false), "after a power-off the next sector is erased again");
    }
}
