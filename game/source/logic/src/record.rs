//! EEPROM records. A record is a frame: magic (2), version, body length, body, then the
//! CRC-16/CCITT-FALSE of everything before it, high byte first. A reader takes the fields it knows
//! from the front of the body, so later firmware can append fields and earlier firmware still
//! reads the record. A version change means the body no longer starts the same way.
//!
//! History slots, and records written before frames, end in an 8-bit checksum instead: `seal`.

/// Bytes a frame adds to its body: magic, version and length before it, CRC after it.
pub const OVERHEAD: usize = 6;
/// Magic, version and body length.
pub const BODY_START: usize = 4;

/// The body of a frame being built: every byte between the header and the CRC.
pub fn body_mut(record: &mut [u8]) -> &mut [u8] {
    let end = record.len() - 2;
    &mut record[BODY_START..end]
}

/// Writes the header and CRC around the body already in place.
pub fn frame(record: &mut [u8], magic: [u8; 2], version: u8) {
    let end = record.len() - 2;
    record[..BODY_START].copy_from_slice(&[magic[0], magic[1], version, (end - BODY_START) as u8]);
    let crc = crc16(&record[..end]);
    record[end..].copy_from_slice(&crc.to_be_bytes());
}

/// The body of the frame at the start of `slot`; None for another magic or version, a length
/// past the slot, or a bad CRC.
pub fn unframe(slot: &[u8], magic: [u8; 2], version: u8) -> Option<&[u8]> {
    read_frame(slot).and_then(|(m, v, body)| (m == magic && v == version).then_some(body))
}

/// The magic, version and body of the frame at the start of `slot`, whatever its magic; None for
/// a length past the slot or a bad CRC.
pub fn read_frame(slot: &[u8]) -> Option<([u8; 2], u8, &[u8])> {
    let &[m0, m1, version, len] = slot.first_chunk::<BODY_START>()?;
    let end = BODY_START + len as usize;
    let crc = slot.get(end..end + 2)?;
    (crc == crc16(&slot[..end]).to_be_bytes()).then(|| ([m0, m1], version, &slot[BODY_START..end]))
}

/// Bytes in the frame that starts with `header`.
pub fn frame_len(header: [u8; BODY_START]) -> usize {
    BODY_START + header[3] as usize + 2
}

fn crc16(bytes: &[u8]) -> u16 {
    bytes.iter().fold(0xFFFF, |crc, &b| {
        (0..8).fold(crc ^ (b as u16) << 8, |c, _| if c & 0x8000 != 0 { c << 1 ^ 0x1021 } else { c << 1 })
    })
}

/// Sets the last byte to the checksum of the rest.
pub fn seal(record: &mut [u8]) {
    let (body, last) = record.split_at_mut(record.len() - 1);
    last[0] = checksum(body);
}

/// Whether the last byte is the checksum of the rest.
pub fn is_sealed(record: &[u8]) -> bool {
    let (body, last) = record.split_at(record.len() - 1);
    last[0] == checksum(body)
}

fn checksum(bytes: &[u8]) -> u8 {
    !bytes.iter().fold(0u8, |sum, b| sum.wrapping_add(*b))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAGIC: [u8; 2] = *b"Rt";

    #[test]
    fn crc_matches_the_standard_check_value() {
        assert_eq!(crc16(b"123456789"), 0x29B1);
    }

    #[test]
    fn a_frame_reads_back_and_any_change_breaks_it() {
        let mut record = [0; 3 + OVERHEAD];
        body_mut(&mut record).copy_from_slice(&[1, 2, 3]);
        frame(&mut record, MAGIC, 1);
        assert_eq!(unframe(&record, MAGIC, 1), Some(&[1, 2, 3][..]));
        assert_eq!(unframe(&record, MAGIC, 2), None);
        assert_eq!(unframe(&record, *b"Ru", 1), None);
        let mut swapped = record;
        swapped.swap(4, 5);
        assert_eq!(unframe(&swapped, MAGIC, 1), None, "swapped bytes keep an additive sum");
        assert_eq!(unframe(&record[..record.len() - 1], MAGIC, 1), None, "cut short");
        assert_eq!(unframe(&[0xFF; 16], MAGIC, 1), None, "erased EEPROM");
    }

    #[test]
    fn a_longer_body_from_later_firmware_still_reads() {
        let mut slot = [0xFF; 16];
        let record = &mut slot[..5 + OVERHEAD];
        body_mut(record).copy_from_slice(&[1, 2, 3, 4, 5]);
        frame(record, MAGIC, 1);
        assert_eq!(unframe(&slot, MAGIC, 1).map(|body| &body[..3]), Some(&[1, 2, 3][..]));
    }

    #[test]
    fn seal_marks_a_record_and_any_change_breaks_it() {
        let mut record = [1, 2, 3, 0];
        assert!(!is_sealed(&record));
        seal(&mut record);
        assert!(is_sealed(&record));
        record[1] ^= 0x10;
        assert!(!is_sealed(&record));
        assert!(!is_sealed(&[0xFF; 6]), "erased EEPROM");
    }
}
