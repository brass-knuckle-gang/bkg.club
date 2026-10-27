//! Hand timing: what the contacts do around the keyer. With an iambic keyer, how long before the
//! keyer's decision you release at the end of a character, and how often you press too late to
//! continue one. With a straight key or sideswiper, each side's dits and dahs, the weight of a
//! dit against the gap after it, and how often both sides close in one mark.

use crate::text::Text;
use crate::ticks;
use crate::timing::{Kind, Line, Moments, moments_line};

/// A release this few dots before the keyer's decision nearly sent one more element.
pub const CLOSE_DOTS: f32 = 0.5;
pub const RELEASE_BINS: usize = 12;
/// Release histogram range in dots.
pub const RELEASE_RANGE: f32 = 3.0;

/// Mark lengths in ms, as the decoder reads them.
#[derive(Clone, Copy, Default)]
pub struct Side {
    pub dit: Moments,
    pub dah: Moments,
}

impl Side {
    /// The dah length over the dit length; 3 is standard.
    pub fn ratio(&self) -> Option<f32> {
        Some(self.dah.mean()? / self.dit.mean()?)
    }
}

#[derive(Clone, Copy, Default)]
pub struct Hand {
    /// Dots from your last release to the keyer's decision, once per iambic character.
    pub release: Moments,
    pub release_bins: [u16; RELEASE_BINS],
    /// Releases under CLOSE_DOTS.
    pub close: u32,
    /// Iambic element gaps, late or not.
    pub element_gaps: u32,
    /// Dots past the keyer's decision, for each element gap you pressed late.
    pub late: Moments,
    /// Straight keying by the contact that closed the key: tip, then ring.
    pub sides: [Side; 2],
    /// A straight-keyed dit over the element gap after it: 1 is even, more is heavy.
    pub weight: Moments,
    /// Straight marks in `sides` during which the other contact closed too.
    pub overlap: u32,
    /// The other contact closed during the mark now down.
    squeezed: bool,
    /// Tick time each contact closed, while closed.
    closed: [Option<u32>; 2],
    released_at: Option<u32>,
    /// The last contact to close while the other was open, and when.
    first_closed: Option<(usize, u32)>,
    down_at: Option<u32>,
    up_at: Option<u32>,
    /// The keyer's decision after an iambic element, and the dot length, until it is known
    /// whether the character ended there.
    pending_stop: Option<(u32, u16)>,
    char_start: u32,
    /// A straight-keyed dit's length, until the gap after it.
    last_dit_ms: Option<u32>,
}

impl Hand {
    /// Contact `line`, Tip or Ring, closed or opened at `at_ms`.
    pub fn contact(&mut self, line: Line, down: bool, at_ms: u32) {
        let i = (line == Line::Ring) as usize;
        if down {
            if self.closed[1 - i].is_none() {
                self.first_closed = Some((i, at_ms));
            } else if self.down_at.is_some() {
                self.squeezed = true;
            }
            self.closed[i] = Some(at_ms);
        } else {
            self.closed[i] = None;
            self.released_at = Some(at_ms);
        }
    }

    /// The keyer output went down or up at `at_ms`. Give contact edges of the same tick first.
    pub fn key(&mut self, down: bool, at_ms: u32, dit_ms: u16, iambic: bool) {
        let dots = |ms: u32| ms as f32 / dit_ms as f32;
        if down {
            self.down_at = Some(at_ms);
            self.squeezed = false;
            let dit = self.last_dit_ms.take();
            match self.up_at.take().map(|up| at_ms.wrapping_sub(up)) {
                Some(gap) if Kind::gap(dots(gap)) == Kind::ElementGap => {
                    self.pending_stop = None;
                    if iambic && self.touched() {
                        self.element_gaps += 1;
                        if gap > dit_ms as u32 {
                            self.late.add(dots(gap - dit_ms as u32));
                        }
                    } else if let Some(dit) = dit {
                        self.weight.add(dit as f32 / gap as f32);
                    }
                }
                _ => {
                    self.stop();
                    self.char_start = at_ms;
                }
            }
        } else if let Some(start) = self.down_at.take() {
            self.up_at = Some(at_ms);
            let len = at_ms.wrapping_sub(start);
            if iambic {
                self.pending_stop = Some((at_ms.wrapping_add(dit_ms as u32), dit_ms));
            } else if let Some((side, _)) = self.first_closed.filter(|&(_, at)| at == start) {
                self.overlap += self.squeezed as u32;
                let side = &mut self.sides[side];
                if Kind::mark(dots(len)) == Kind::Dit {
                    side.dit.add(len as f32);
                    self.last_dit_ms = Some(len);
                } else {
                    side.dah.add(len as f32);
                }
            }
        }
    }

    /// A contact is closed, or opened during this character. Serial keying has neither.
    fn touched(&self) -> bool {
        self.closed.iter().any(Option::is_some) || self.released_at.is_some_and(|r| ticks::reached(r, self.char_start))
    }

    /// The decoder ended a character: the keyer's last decision passed with no element.
    pub fn char_ended(&mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        let Some((decision, dit_ms)) = self.pending_stop.take() else { return };
        // A contact closed at the decision is stuck. No release in the character: serial keying.
        if self.closed.iter().flatten().any(|&since| ticks::reached(decision, since)) {
            return;
        }
        let in_char = |r: u32| ticks::reached(r, self.char_start) && ticks::reached(decision, r);
        let Some(released) = self.released_at.filter(|&r| in_char(r)) else { return };
        let dots = decision.wrapping_sub(released) as f32 / dit_ms as f32;
        self.release.add(dots);
        let bin = ((dots / RELEASE_RANGE * RELEASE_BINS as f32) as usize).min(RELEASE_BINS - 1);
        self.release_bins[bin] = self.release_bins[bin].saturating_add(1);
        self.close += (dots < CLOSE_DOTS) as u32;
    }

    /// The hand lines of `STATS TIMING` (docs/dev/serial.md), each ending in `\n`.
    pub fn report<const N: usize>(&self) -> impl Iterator<Item = Text<N>> + '_ {
        // The longest line: "BINS RELEASE 3", then 12 counts of up to " 65535", then "\n".
        const { assert!(N >= 14 + RELEASE_BINS * 6 + 1) };
        let on_time = self.element_gaps - self.late.count;
        let iambic = [
            moments_line("RELEASE", &self.release),
            Text::format(format_args!("CLOSE {}\n", self.close)),
            moments_line("LATE", &self.late),
            Text::format(format_args!("ON_TIME {on_time}\n")),
        ];
        let [tip, ring] = &self.sides;
        let sides = [("TIP DIT", &tip.dit), ("TIP DAH", &tip.dah), ("RING DIT", &ring.dit), ("RING DAH", &ring.dah)]
            .into_iter()
            .map(|(name, m)| moments_line(name, m));
        let mut bins = Text::format(format_args!("BINS RELEASE {}", RELEASE_RANGE as u32));
        for n in self.release_bins {
            let _ = core::fmt::Write::write_fmt(&mut bins, format_args!(" {n}"));
        }
        let _ = core::fmt::Write::write_str(&mut bins, "\n");
        let overlap = Text::format(format_args!("OVERLAP {}\n", self.overlap));
        iambic.into_iter().chain(sides).chain([moments_line("WEIGHT", &self.weight), overlap, bins])
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::keyer::{Contacts, Keyer, Mode};
    use std::vec::Vec;

    const TIP: Contacts = Contacts { tip: true, ring: false };
    const RING: Contacts = Contacts { tip: false, ring: true };
    const NONE: Contacts = Contacts { tip: false, ring: false };
    /// 20 WPM.
    const DIT: u16 = 60;

    /// Ticks `keyer` through `steps` from time 0, giving `hand` contact edges then key edges as
    /// the firmware does. A character ends once the key has been up two dots, as the decoder
    /// reads it.
    fn run(hand: &mut Hand, mode: Mode, steps: &[(Contacts, u32)]) {
        let mut keyer = Keyer::new(mode, false, 20);
        let iambic = mode != Mode::Straight;
        let (mut was, mut down, mut up_at, mut t) = (NONE, false, None, 0u32);
        for &(contacts, ms) in steps {
            for _ in 0..ms {
                for (line, a, b) in [(Line::Tip, was.tip, contacts.tip), (Line::Ring, was.ring, contacts.ring)] {
                    if a != b {
                        hand.contact(line, b, t);
                    }
                }
                was = contacts;
                if keyer.tick(contacts) != down {
                    down = !down;
                    hand.key(down, t, DIT, iambic);
                    up_at = (!down).then_some(t);
                }
                if up_at.is_some_and(|up| t - up == 2 * DIT as u32) {
                    hand.char_ended();
                    up_at = None;
                }
                t += 1;
            }
        }
    }

    fn near(a: Option<f32>, b: f32) -> bool {
        a.is_some_and(|a| (a - b).abs() < 1e-4)
    }

    #[test]
    fn release_is_measured_to_the_decision_one_dot_after_the_element() {
        let mut hand = Hand::default();
        // A dit ends at 60 and the keyer decides at 120. A release at 100 is a third of a dot
        // early; one at 20 is five thirds.
        run(&mut hand, Mode::IambicB, &[(TIP, 100), (NONE, 400), (TIP, 20), (NONE, 400)]);
        assert_eq!((hand.release.count, hand.close), (2, 1));
        assert!(near(hand.release.mean(), 1.0), "{:?}", hand.release.mean());
        assert_eq!((hand.release_bins[1], hand.release_bins[6]), (1, 1));
    }

    #[test]
    fn a_press_after_the_decision_is_late_and_does_not_end_the_character() {
        let mut hand = Hand::default();
        // The dit's decision at 120 finds no paddle; the dah pressed at 130 starts at once.
        run(&mut hand, Mode::IambicB, &[(TIP, 100), (NONE, 30), (RING, 50), (NONE, 400)]);
        assert_eq!((hand.element_gaps, hand.late.count), (1, 1));
        assert!(near(hand.late.mean(), 10.0 / 60.0));
        // The dah ends at 310 and its decision is at 370, 190 ms after the release at 180.
        assert_eq!(hand.release.count, 1);
        assert!(near(hand.release.mean(), 190.0 / 60.0));
        assert_eq!(hand.release_bins[RELEASE_BINS - 1], 1, "past the range counts in the last bin");
    }

    #[test]
    fn a_held_paddle_continues_on_time() {
        let mut hand = Hand::default();
        // Dits at 0 and 120; the second ends at 180 and its decision is at 240.
        run(&mut hand, Mode::IambicA, &[(TIP, 150), (NONE, 400)]);
        assert_eq!((hand.element_gaps, hand.late.count, hand.release.count), (1, 0, 1));
        assert!(near(hand.release.mean(), 1.5));
    }

    #[test]
    fn keying_with_no_contacts_measures_nothing() {
        let mut hand = Hand::default();
        run(&mut hand, Mode::IambicB, &[(TIP, 20), (NONE, 400)]);
        for (down, at) in [(true, 1000), (false, 1060), (true, 1120), (false, 1300)] {
            hand.key(down, at, DIT, true);
        }
        hand.char_ended();
        assert_eq!((hand.release.count, hand.element_gaps), (1, 0));
    }

    #[test]
    fn straight_keying_counts_each_side_and_the_weight_of_dits() {
        let mut hand = Hand::default();
        // Tip dit 70 ms with a 50 ms gap, ring dah 180, a character gap, then a tip dah.
        let steps = [(TIP, 70), (NONE, 50), (RING, 180), (NONE, 300), (TIP, 200), (NONE, 300)];
        run(&mut hand, Mode::Straight, &steps);
        let [tip, ring] = hand.sides;
        assert_eq!([tip.dit.count, tip.dah.count, ring.dit.count, ring.dah.count], [1, 1, 0, 1]);
        assert!(near(tip.dit.mean(), 70.0) && near(ring.dah.mean(), 180.0) && near(tip.dah.mean(), 200.0));
        assert_eq!(hand.weight.count, 1);
        assert!(near(hand.weight.mean(), 1.4));
        assert!(near(tip.ratio(), 200.0 / 70.0) && ring.ratio().is_none());
        assert_eq!(hand.overlap, 0);
        assert_eq!((hand.release.count, hand.element_gaps), (0, 0), "no keyer decisions");
    }

    #[test]
    fn a_sideswiper_mark_belongs_to_the_contact_that_closed_the_key() {
        let mut hand = Hand::default();
        // The ring closes while the tip still holds the key down: one mark, from the tip.
        let both = Contacts { tip: true, ring: true };
        run(&mut hand, Mode::Straight, &[(TIP, 40), (both, 20), (RING, 40), (NONE, 300)]);
        assert_eq!([hand.sides[0].dit.count, hand.sides[1].dit.count], [1, 0]);
        assert_eq!(hand.overlap, 1);
        // The next mark from one side alone does not overlap.
        run(&mut hand, Mode::Straight, &[(RING, 60), (NONE, 300)]);
        assert_eq!(hand.overlap, 1);
    }

    #[test]
    fn the_report_gives_each_line() {
        let mut hand = Hand::default();
        run(&mut hand, Mode::IambicB, &[(TIP, 100), (NONE, 400)]);
        let lines: Vec<std::string::String> = hand.report::<96>().map(|l| l.as_str().into()).collect();
        assert_eq!(
            lines,
            [
                "RELEASE 1 0.33 0.00\n",
                "CLOSE 1\n",
                "LATE 0 - -\n",
                "ON_TIME 0\n",
                "TIP DIT 0 - -\n",
                "TIP DAH 0 - -\n",
                "RING DIT 0 - -\n",
                "RING DAH 0 - -\n",
                "WEIGHT 0 - -\n",
                "OVERLAP 0\n",
                "BINS RELEASE 3 0 1 0 0 0 0 0 0 0 0 0 0\n",
            ]
        );
    }
}
