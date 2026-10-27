//! The content pack, `F000400.bin` at SPI flash 0x100000, built by `tools/pack/pack.py`. All numbers
//! are little endian.
//!
//! Header: `PocketCW`, format u8, section count u8, two zero bytes, then one 16 byte table entry
//! per section (kind [u8; 4], offset u32 from the pack start, length u32, CRC-32 u32), then the
//! CRC-32 of everything before it.
//!
//! `WORD`: entry count u16, two zero bytes, then one `WORD_ENTRY` byte entry each: text length
//! u8, flags u8, text padded with zeros to `WORD_TEXT` bytes, then two clip offsets u32 in
//! `CLIP` or `NO_CLIP`: the word, and its meaning, which follows the word when it is said on its own. A word with no clip of its own, such as `QTH`, is spelled from the characters' clips. The characters come first, for spelling, then ham words, then common words, each
//! group of words most frequent first. A character's text can also be a word's: `K` is a letter
//! and "over".
//!
//! `CLIP`: the output sample rate divided by the clips' rate u8, three zero bytes, then the
//! clips. Each clip is its length u16, then IMA ADPCM bytes (`adpcm.rs`).

pub const MAGIC: &[u8; 8] = b"PocketCW";
pub const FORMAT: u8 = 4;
pub const MAX_SECTIONS: usize = 8;
const TABLE: usize = 12;
const ENTRY: usize = 16;
/// Bytes that hold the largest header.
pub const HEADER_MAX: usize = TABLE + MAX_SECTIONS * ENTRY + 4;

pub const WORDS: [u8; 4] = *b"WORD";
pub const CLIPS: [u8; 4] = *b"CLIP";
/// Bytes before the first entry or clip of a section.
pub const SECTION_START: u32 = 4;
pub const WORD_TEXT: usize = 18;
pub const WORD_ENTRY: usize = 2 + WORD_TEXT + 2 * 4;
pub const NO_CLIP: u32 = u32::MAX;
const FLAG_HAM: u8 = 1;
const FLAG_CHAR: u8 = 2;
/// Bytes before a clip's ADPCM data.
pub const CLIP_START: u32 = 2;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Error {
    /// No pack: the bytes do not start with `MAGIC`.
    Missing,
    /// A pack of another format, such as the 4 KB format 1 file of releases before packs.
    Format(u8),
    Corrupt,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Section {
    pub kind: [u8; 4],
    pub offset: u32,
    pub len: u32,
    pub crc: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Header {
    sections: [Option<Section>; MAX_SECTIONS],
}

impl Header {
    /// Reads the header from the first bytes of the pack, `HEADER_MAX` of them or all of a
    /// shorter pack.
    pub fn parse(bytes: &[u8]) -> Result<Header, Error> {
        if !bytes.starts_with(MAGIC) {
            return Err(Error::Missing);
        }
        match bytes.get(8) {
            Some(&FORMAT) => {}
            Some(&format) => return Err(Error::Format(format)),
            None => return Err(Error::Corrupt),
        }
        let count = bytes[9] as usize;
        let end = TABLE + count * ENTRY;
        let (Some(table), Some(crc)) = (bytes.get(..end), bytes.get(end..end + 4)) else {
            return Err(Error::Corrupt);
        };
        if count > MAX_SECTIONS || crc32(table) != u32_at(crc, 0) {
            return Err(Error::Corrupt);
        }
        let mut sections = [None; MAX_SECTIONS];
        for (slot, entry) in sections.iter_mut().zip(table[TABLE..].chunks_exact(ENTRY)) {
            *slot = Some(Section { kind: entry[..4].try_into().unwrap(), offset: u32_at(entry, 4), len: u32_at(entry, 8), crc: u32_at(entry, 12) });
        }
        Ok(Header { sections })
    }

    pub fn section(&self, kind: [u8; 4]) -> Option<Section> {
        self.sections.iter().flatten().find(|s| s.kind == kind).copied()
    }
}

/// The word count, from the first bytes of the `WORD` section.
pub fn word_count(start: &[u8; SECTION_START as usize]) -> u16 {
    u16::from_le_bytes([start[0], start[1]])
}

/// Where word `index` sits in the `WORD` section.
pub fn word_offset(index: u16) -> u32 {
    SECTION_START + index as u32 * WORD_ENTRY as u32
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Word {
    text: [u8; WORD_TEXT],
    len: u8,
    pub ham: bool,
    /// A character for spelling, not a word.
    pub char: bool,
    /// Where the clip sits in the `CLIP` section.
    pub clip: Option<u32>,
    /// Where the clip of the meaning sits, for an abbreviation: "location" for `QTH`.
    pub meaning: Option<u32>,
}

impl Word {
    /// None when the text length runs past the entry.
    pub fn parse(entry: &[u8; WORD_ENTRY]) -> Option<Word> {
        let len = entry[0];
        let text = entry[2..2 + WORD_TEXT].try_into().unwrap();
        let offset = |at: usize| Some(u32_at(entry, at)).filter(|&o| o != NO_CLIP);
        let flags = entry[1];
        let (clip, meaning) = (offset(2 + WORD_TEXT), offset(2 + WORD_TEXT + 4));
        ((len as usize) <= WORD_TEXT).then_some(Word { text, len, ham: flags & FLAG_HAM != 0, char: flags & FLAG_CHAR != 0, clip, meaning })
    }

    /// The text as sent in Morse: capitals, digits, and prosigns in angle brackets.
    pub fn text(&self) -> &[u8] {
        &self.text[..self.len as usize]
    }
}

/// The ADPCM byte count, from the first bytes of a clip.
pub fn clip_len(start: &[u8; CLIP_START as usize]) -> u16 {
    u16::from_le_bytes(*start)
}

/// CRC-32 (IEEE, as zlib), fed in pieces so a section can be checked as it is read.
#[derive(Clone, Copy)]
pub struct Crc32(u32);

impl Default for Crc32 {
    fn default() -> Self {
        Crc32(!0)
    }
}

impl Crc32 {
    pub fn update(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (0..8).fold(self.0 ^ b as u32, |c, _| if c & 1 != 0 { c >> 1 ^ 0xEDB8_8320 } else { c >> 1 });
        }
    }

    pub fn finish(self) -> u32 {
        !self.0
    }
}

pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = Crc32::default();
    crc.update(bytes);
    crc.finish()
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Built by `python tools/pack/pack.py --fixture logic/fixtures/pack.bin`: the character `K` (clip
    /// byte 5), the words `CQ` (ham, clip bytes 1 2 3, its meaning 4 4), `<BK>` (ham, no
    /// clip, its meaning 6) and `THE` (clip bytes 9 8).
    const FIXTURE: &[u8] = include_bytes!("../fixtures/pack.bin");

    fn section(header: &Header, kind: [u8; 4]) -> &'static [u8] {
        let s = header.section(kind).unwrap();
        &FIXTURE[s.offset as usize..][..s.len as usize]
    }

    fn word(words: &[u8], index: u16) -> Word {
        let at = word_offset(index) as usize;
        Word::parse(words[at..at + WORD_ENTRY].try_into().unwrap()).unwrap()
    }

    fn clip(clips: &[u8], at: u32) -> &[u8] {
        let at = at as usize;
        let len = clip_len(clips[at..at + 2].try_into().unwrap()) as usize;
        &clips[at + 2..][..len]
    }

    #[test]
    fn crc32_matches_zlib() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn reads_the_fixture() {
        let header = Header::parse(&FIXTURE[..HEADER_MAX.min(FIXTURE.len())]).unwrap();
        let words = section(&header, WORDS);
        let clips = section(&header, CLIPS);
        assert_eq!(crc32(words), header.section(WORDS).unwrap().crc);
        assert_eq!(crc32(clips), header.section(CLIPS).unwrap().crc);
        assert_eq!(word_count(words[..4].try_into().unwrap()), 4);
        assert_eq!(clips[0], 2);

        let k = word(words, 0);
        assert_eq!((k.text(), k.char, k.ham), (&b"K"[..], true, false));
        assert_eq!(clip(clips, k.clip.unwrap()), [5]);
        let cq = word(words, 1);
        assert_eq!((cq.text(), cq.char, cq.ham), (&b"CQ"[..], false, true));
        assert_eq!(clip(clips, cq.clip.unwrap()), [1, 2, 3]);
        assert_eq!(clip(clips, cq.meaning.unwrap()), [4, 4]);
        assert_eq!(k.meaning, None);
        let bk = word(words, 2);
        assert_eq!((bk.text(), bk.ham, bk.clip), (&b"<BK>"[..], true, None));
        assert_eq!(clip(clips, bk.meaning.unwrap()), [6]);
        let the = word(words, 3);
        assert_eq!((the.text(), the.char, the.ham), (&b"THE"[..], false, false));
        assert_eq!(clip(clips, the.clip.unwrap()), [9, 8]);
    }

    #[test]
    fn tells_a_missing_pack_from_an_old_or_corrupt_one() {
        assert_eq!(Header::parse(&[0xFF; HEADER_MAX]), Err(Error::Missing));
        let mut old = [0xFF; 4096];
        old[..9].copy_from_slice(b"PocketCW\x01");
        assert_eq!(Header::parse(&old[..HEADER_MAX]), Err(Error::Format(1)));
        let mut bad = [0u8; HEADER_MAX];
        let n = HEADER_MAX.min(FIXTURE.len());
        bad[..n].copy_from_slice(&FIXTURE[..n]);
        bad[TABLE + 4] ^= 1;
        assert_eq!(Header::parse(&bad), Err(Error::Corrupt));
    }

    #[test]
    fn a_text_length_past_the_entry_is_rejected() {
        let mut entry = [0u8; WORD_ENTRY];
        entry[0] = WORD_TEXT as u8 + 1;
        assert_eq!(Word::parse(&entry), None);
    }
}
