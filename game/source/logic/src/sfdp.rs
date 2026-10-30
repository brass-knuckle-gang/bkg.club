//! SFDP (JEDEC JESD216): the table a serial flash chip reads out about itself. Only the fields the
//! driver needs: 4 KB erase, the erase types and their times, the size, and the page size.

/// Bytes the SFDP header and the first parameter header take.
pub const HEADER_LEN: usize = 16;

/// Where the basic flash parameter table sits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Header {
    pub major: u8,
    pub minor: u8,
    pub table_address: u32,
    pub table_dwords: u8,
}

/// The SFDP header and the first parameter header; None without the `SFDP` signature or when
/// the first table is not the JEDEC basic table.
pub fn header(bytes: &[u8; HEADER_LEN]) -> Option<Header> {
    let jedec_basic = bytes[8] == 0x00 && bytes[15] == 0xFF;
    (bytes[..4] == *b"SFDP" && jedec_basic).then(|| Header {
        major: bytes[5],
        minor: bytes[4],
        table_address: u32::from_le_bytes([bytes[12], bytes[13], bytes[14], 0]),
        table_dwords: bytes[11],
    })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EraseType {
    pub bytes: u32,
    pub opcode: u8,
    /// Typical and maximum erase times, when the table gives them.
    pub ms: Option<(u32, u32)>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Basic {
    /// The 4 KB erase command, when the chip has one.
    pub erase_4k: Option<u8>,
    pub bytes: u64,
    pub erase_types: [Option<EraseType>; 4],
    pub page_bytes: Option<u32>,
}

/// Units of the erase time fields, in ms.
const ERASE_UNIT_MS: [u32; 4] = [1, 16, 128, 1000];

/// The basic flash parameter table, as little-endian dwords from its start. Fields past the end
/// of a short (older) table read as absent.
pub fn basic(dwords: &[u32]) -> Basic {
    let dword = |n: usize| dwords.get(n - 1).copied();
    let mut basic = Basic::default();
    if let Some(d) = dword(1) {
        basic.erase_4k = (d & 0b11 == 0b01).then_some((d >> 8) as u8);
    }
    if let Some(d) = dword(2) {
        let bits = if d & 0x8000_0000 == 0 { d as u64 + 1 } else { 1u64 << (d & 0x7FFF_FFFF).min(63) };
        basic.bytes = bits / 8;
    }
    let types = [dword(8), dword(9)];
    for (i, slot) in basic.erase_types.iter_mut().enumerate() {
        let Some(d) = types[i / 2] else { continue };
        let field = d >> (16 * (i % 2));
        let exponent = field & 0xFF;
        *slot = (exponent != 0 && exponent < 32).then(|| EraseType { bytes: 1 << exponent, opcode: (field >> 8) as u8, ms: None });
    }
    if let Some(d) = dword(10) {
        let multiplier = d & 0xF;
        for (i, slot) in basic.erase_types.iter_mut().enumerate() {
            let Some(erase) = slot else { continue };
            let count = d >> (4 + 7 * i) & 0x1F;
            let unit = ERASE_UNIT_MS[(d >> (9 + 7 * i) & 0b11) as usize];
            let typical = (count + 1) * unit;
            erase.ms = Some((typical, 2 * (multiplier + 1) * typical));
        }
    }
    if let Some(d) = dword(11) {
        basic.page_bytes = Some(1 << (d >> 4 & 0xF));
    }
    basic
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_first_parameter_header() {
        let bytes = [b'S', b'F', b'D', b'P', 0x06, 0x01, 0x00, 0xFF, 0x00, 0x06, 0x01, 0x10, 0x80, 0x00, 0x00, 0xFF];
        assert_eq!(header(&bytes), Some(Header { major: 1, minor: 6, table_address: 0x80, table_dwords: 16 }));
        let mut vendor_first = bytes;
        vendor_first[8] = 0x84;
        assert_eq!(header(&vendor_first), None);
        assert_eq!(header(&[0xFF; HEADER_LEN]), None);
    }

    /// Fields set by their JESD216 bit positions: 4 KB erase 0x20, 128 Mbit, erase types
    /// 4 KB 0x20, 32 KB 0x52, 64 KB 0xD8, 4 KB erase 48 ms typical with multiplier 2, 64 KB erase
    /// 160 ms typical, 256 byte pages.
    #[test]
    fn reads_the_basic_table() {
        let mut dwords = [0u32; 11];
        dwords[0] = 0xFFF9_20E5;
        dwords[1] = 0x07FF_FFFF;
        dwords[7] = 0x520F_200C;
        dwords[8] = 0x0000_D810;
        dwords[9] = 2 | 2 << 4 | 1 << 9 | 9 << 18 | 1 << 23;
        dwords[10] = 8 << 4;
        let basic = basic(&dwords);
        assert_eq!(basic.erase_4k, Some(0x20));
        assert_eq!(basic.bytes, 16 << 20);
        assert_eq!(basic.erase_types[0], Some(EraseType { bytes: 4096, opcode: 0x20, ms: Some((48, 288)) }));
        assert_eq!(basic.erase_types[1].map(|e| (e.bytes, e.opcode)), Some((32 << 10, 0x52)));
        assert_eq!(basic.erase_types[2], Some(EraseType { bytes: 64 << 10, opcode: 0xD8, ms: Some((160, 960)) }));
        assert_eq!(basic.erase_types[3], None);
        assert_eq!(basic.page_bytes, Some(256));
    }

    #[test]
    fn a_short_table_leaves_later_fields_absent() {
        let basic = basic(&[0xFFF9_20E5, 0x07FF_FFFF]);
        assert_eq!(basic.erase_4k, Some(0x20));
        assert_eq!(basic.erase_types, [None; 4]);
        assert_eq!(basic.page_bytes, None);
    }

    #[test]
    fn no_4k_erase_reads_as_none() {
        assert_eq!(basic(&[0xFFF9_20E7]).erase_4k, None);
    }
}
