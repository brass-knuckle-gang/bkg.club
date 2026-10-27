//! Battery gauge from ADC counts on PA1: the battery through a 2:1 divider, read against the
//! 3.3 V supply. Divider and regulator tolerances move the scale by a few percent per unit, so
//! the gauge learns each unit's reading at full charge and scales to that.

use crate::record::{self, body_mut, frame, is_sealed, unframe};

/// Battery millivolts per 10,000 counts: the vendor firmware's scale.
const MV_PER_10K_COUNTS: u32 = 16_169;
const FULL_MV: u32 = 4200;
/// A full cell in counts at the vendor scale.
pub const DESIGN_FULL: u16 = (FULL_MV * 10_000 / MV_PER_10K_COUNTS) as u16;
/// The learned full point stays within these, about 3% either side of the design value.
const FULL_MIN: u16 = DESIGN_FULL - DESIGN_FULL / 33;
const FULL_MAX: u16 = DESIGN_FULL + DESIGN_FULL / 33;
/// With USB in, PA1 reads about 4.5% above a full cell, so a reading this far above the full
/// point at startup means USB.
const USB_ABOVE: u16 = 75;
/// Plugging USB in or out moves the reading by about 120 counts within one poll; the load
/// moves a battery by under 30.
const JUMP: u16 = 60;
/// Surface charge from the charger lifts the reading for a while after USB comes out.
const SETTLE_MS: u32 = 120_000;
/// Percent at each battery voltage, for a lithium cell under light load.
const CURVE: [(u32, u8); 11] = [
    (3400, 0),
    (3500, 5),
    (3600, 12),
    (3700, 25),
    (3750, 35),
    (3800, 45),
    (3850, 55),
    (3900, 63),
    (4000, 77),
    (4100, 90),
    (4200, 100),
];
/// Below this the device saves and powers off.
const EMPTY_MV: u32 = 3200;
/// Below this supply the regulator has dropped out, and readings against the supply read high.
const EMPTY_SUPPLY_MV: u16 = 3100;
/// A supply this far below the highest seen, in percent, means the regulator has dropped out;
/// smaller dips are reading noise.
const DROPOUT_PERCENT: u32 = 98;
/// Consecutive empty readings before `empty`, so one sag under load doesn't power off.
const EMPTY_POLLS: u8 = 10;

const MAGIC: [u8; 2] = *b"Rb";
const VERSION: u8 = 1;
/// Body: full point LE.
pub const RECORD_LEN: usize = 2 + record::OVERHEAD;
/// Before frames: magic, version 1, full point LE, checksum.
const LEGACY: [u8; 3] = [b'R', b'B', 1];
const LEGACY_LEN: usize = 6;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Charge {
    Usb,
    Percent(u8),
}

pub struct Gauge {
    /// Learned full-charge reading; None until the first charge.
    full: Option<u16>,
    /// The last reading, scaled back up when the supply sagged.
    raw: Option<u16>,
    /// The highest supply reading so far: the regulator's normal output on this unit's
    /// internal reference.
    supply_top: u16,
    usb: bool,
    /// Readings from `quiet_at + SETTLE_MS` on carry no surface charge: startup or the last USB
    /// reading.
    quiet_at: u32,
    empty_polls: u8,
    unsaved: bool,
}

impl Gauge {
    /// `full`: the full point from `decode`.
    pub fn new(full: Option<u16>, now_ms: u32) -> Gauge {
        Gauge { full, raw: None, supply_top: 0, usb: false, quiet_at: now_ms, empty_polls: 0, unsaved: false }
    }

    pub fn update(&mut self, raw: u16, supply_mv: u16, now_ms: u32) {
        self.supply_top = self.supply_top.max(supply_mv);
        // The ADC measures against the supply, so in dropout a falling battery reads steady. The
        // ratio of two internal-reference readings cancels that reference's own error.
        let raw = if (supply_mv as u32) * 100 < self.supply_top as u32 * DROPOUT_PERCENT {
            (raw as u32 * supply_mv as u32 / self.supply_top as u32) as u16
        } else {
            raw
        };
        self.usb = match self.raw {
            None => raw >= self.full_point() + USB_ABOVE,
            Some(last) if raw >= last + JUMP => true,
            Some(last) if raw + JUMP <= last => false,
            Some(_) => self.usb,
        };
        self.raw = Some(raw);
        // Counted whatever `usb` says: a load coming off a nearly empty cell can lift it by JUMP,
        // and real USB reads far above empty.
        let empty = self.battery_mv().is_some_and(|mv| (mv as u32) < EMPTY_MV) || supply_mv < EMPTY_SUPPLY_MV;
        self.empty_polls = if empty { self.empty_polls.saturating_add(1) } else { 0 };
        if self.usb {
            self.quiet_at = now_ms;
            return;
        }
        let settled = now_ms.wrapping_sub(self.quiet_at) >= SETTLE_MS;
        let full = raw.min(FULL_MAX);
        if settled && raw >= FULL_MIN && self.full.is_none_or(|old| full > old) {
            self.full = Some(full);
            self.unsaved = true;
        }
    }

    /// None before the first reading.
    pub fn charge(&self) -> Option<Charge> {
        if self.usb {
            return Some(Charge::Usb);
        }
        let mv = self.battery_mv()? as u32;
        let percent = match CURVE.iter().position(|&(at, _)| mv < at) {
            Some(0) => 0,
            None => 100,
            Some(i) => {
                let ((mv0, p0), (mv1, p1)) = (CURVE[i - 1], CURVE[i]);
                p0 + ((mv - mv0) * (p1 - p0) as u32 / (mv1 - mv0)) as u8
            }
        };
        Some(Charge::Percent(percent))
    }

    /// Battery voltage on the learned scale; None before the first reading.
    pub fn battery_mv(&self) -> Option<u16> {
        Some((self.raw? as u32 * FULL_MV / self.full_point() as u32) as u16)
    }

    pub fn raw(&self) -> Option<u16> {
        self.raw
    }

    /// Below the bottom of the curve.
    pub fn low(&self) -> bool {
        !self.usb && self.battery_mv().is_some_and(|mv| (mv as u32) < CURVE[0].0)
    }

    /// Empty for long enough that the device should save and power off.
    pub fn empty(&self) -> bool {
        self.empty_polls >= EMPTY_POLLS
    }

    fn full_point(&self) -> u16 {
        self.full.unwrap_or(DESIGN_FULL)
    }

    /// The record to save, once after each change of the full point.
    pub fn take_record(&mut self) -> Option<[u8; RECORD_LEN]> {
        let full = self.full.filter(|_| self.unsaved)?;
        self.unsaved = false;
        let mut record = [0; RECORD_LEN];
        body_mut(&mut record).copy_from_slice(&full.to_le_bytes());
        frame(&mut record, MAGIC, VERSION);
        Some(record)
    }

    /// The full point from the record at the start of `slot`; None for a blank or foreign record,
    /// a bad check, or a value out of range.
    pub fn decode(slot: &[u8]) -> Option<u16> {
        let full = match unframe(slot, MAGIC, VERSION) {
            Some(body) => u16::from_le_bytes(*body.first_chunk()?),
            None => {
                let old = slot.get(..LEGACY_LEN).filter(|old| old[..3] == LEGACY && is_sealed(old))?;
                u16::from_le_bytes([old[3], old[4]])
            }
        };
        (FULL_MIN..=FULL_MAX).contains(&full).then_some(full)
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    const SUPPLY: u16 = 3290;

    /// Counts for `mv` at the design scale.
    fn counts(mv: u32) -> u16 {
        (mv * DESIGN_FULL as u32 / FULL_MV) as u16
    }

    fn percent(g: &Gauge) -> u8 {
        match g.charge() {
            Some(Charge::Percent(p)) => p,
            other => panic!("expected a percent, got {other:?}"),
        }
    }

    #[test]
    fn readings_from_the_bench_unit() {
        let mut g = Gauge::new(None, 0);
        assert_eq!(g.charge(), None);
        g.update(2714, SUPPLY, 0);
        assert_eq!(g.charge(), Some(Charge::Usb));
        g.update(2559, SUPPLY, 1000);
        assert_eq!(g.battery_mv(), Some(4138));
        assert_eq!(percent(&g), 93);
    }

    #[test]
    fn curve_ends_and_order() {
        let mut last = 0;
        for mv in (3300..4300).step_by(10) {
            let mut g = Gauge::new(None, 0);
            g.update(counts(mv), SUPPLY, 0);
            let p = percent(&g);
            assert!(p >= last, "{mv} mV gave {p}% after {last}%");
            assert_eq!(g.low(), g.battery_mv() < Some(3400));
            last = p;
        }
        assert_eq!(last, 100);
    }

    #[test]
    fn usb_follows_jumps() {
        let mut g = Gauge::new(None, 0);
        g.update(2400, SUPPLY, 0);
        let before = percent(&g);
        g.update(2400 + JUMP, SUPPLY, 1000);
        assert_eq!(g.charge(), Some(Charge::Usb));
        g.update(2400 + JUMP + 20, SUPPLY, 2000);
        assert_eq!(g.charge(), Some(Charge::Usb));
        g.update(2400, SUPPLY, 3000);
        assert_eq!(percent(&g), before);
        g.update(2400 - 25, SUPPLY, 4000);
        assert!(percent(&g) < before);
    }

    #[test]
    fn learns_full_once_settled_after_usb() {
        let mut g = Gauge::new(None, 0);
        g.update(2714, SUPPLY, 0);
        g.update(2600, SUPPLY, 1000);
        g.update(2590, SUPPLY, SETTLE_MS - 1);
        assert_eq!(g.take_record(), None);
        g.update(2585, SUPPLY, SETTLE_MS);
        let record = g.take_record().expect("full point learned");
        assert_eq!(Gauge::decode(&record), Some(2585));
        assert_eq!(g.take_record(), None);
        assert_eq!(percent(&g), 100);
    }

    #[test]
    fn full_only_rises_within_bounds() {
        let mut g = Gauge::new(None, 0);
        g.update(FULL_MIN - 1, SUPPLY, SETTLE_MS);
        assert_eq!(g.take_record(), None);
        g.update(2560, SUPPLY, SETTLE_MS + 1000);
        assert!(g.take_record().is_some());
        g.update(2540, SUPPLY, SETTLE_MS + 2000);
        assert_eq!(g.take_record(), None);
        let mut g = Gauge::new(Some(2630), 0);
        g.update(FULL_MAX + 10, SUPPLY, SETTLE_MS);
        assert_eq!(g.take_record().and_then(|r| Gauge::decode(&r)), Some(FULL_MAX));
    }

    #[test]
    fn readings_above_the_cap_save_once() {
        let mut g = Gauge::new(Some(2630), 0);
        g.update(FULL_MAX + 10, SUPPLY, SETTLE_MS);
        assert!(g.take_record().is_some());
        g.update(FULL_MAX + 12, SUPPLY, SETTLE_MS + 1000);
        assert_eq!(g.take_record(), None);
    }

    #[test]
    fn dropout_keeps_the_reading_falling() {
        let mut g = Gauge::new(None, 0);
        g.update(counts(3450), SUPPLY, 0);
        assert!(!g.low());
        // The regulator runs out of headroom: the supply follows the battery down, so the raw
        // count stays put.
        g.update(counts(3450), 3200, 1000);
        assert!(g.battery_mv() < Some(3400) && g.low());
        g.update(counts(3450), 3230, 2000);
        assert!(!g.low());
    }

    #[test]
    fn learned_full_rescales() {
        let mut g = Gauge::new(Some(FULL_MIN), 0);
        g.update(FULL_MIN, SUPPLY, 0);
        assert_eq!(g.battery_mv(), Some(FULL_MV as u16));
        assert_eq!(percent(&g), 100);
    }

    #[test]
    fn empty_needs_steady_readings() {
        let mut g = Gauge::new(None, 0);
        for i in 0..EMPTY_POLLS as u32 - 1 {
            g.update(counts(3150), SUPPLY, i * 1000);
        }
        assert!(!g.empty());
        g.update(counts(3230), SUPPLY, 9000);
        g.update(counts(3150), SUPPLY, 10_000);
        assert!(!g.empty());
        for i in 0..EMPTY_POLLS as u32 {
            g.update(counts(3230), 3000, 11_000 + i * 1000);
        }
        assert!(g.empty());
    }

    #[test]
    fn a_jump_near_empty_still_powers_off() {
        let mut g = Gauge::new(None, 0);
        for i in 0..5 {
            g.update(counts(3050), SUPPLY, i * 1000);
        }
        g.update(counts(3050) + JUMP, SUPPLY, 5000);
        assert_eq!(g.charge(), Some(Charge::Usb));
        for i in 0..EMPTY_POLLS as u32 {
            g.update(counts(3050) + JUMP, SUPPLY, 6000 + i * 1000);
        }
        assert!(g.empty());
    }

    #[test]
    fn record_rejects_blank_and_range() {
        assert_eq!(Gauge::decode(&[0xFF; RECORD_LEN]), None);
        let mut record = [0; RECORD_LEN];
        body_mut(&mut record).copy_from_slice(&(FULL_MAX + 1).to_le_bytes());
        frame(&mut record, MAGIC, VERSION);
        assert_eq!(Gauge::decode(&record), None);
    }

    #[test]
    fn a_record_from_before_frames_still_reads() {
        let mut old = [b'R', b'B', 1, 0x0A, 0x0A, 0];
        crate::record::seal(&mut old);
        assert_eq!(Gauge::decode(&old), Some(0x0A0A));
    }
}
