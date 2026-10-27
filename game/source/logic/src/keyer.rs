//! Paddle keyer, stepped once per millisecond.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Squeeze alternates elements; releasing both after a squeeze stops after the current
    /// element.
    IambicA,
    /// Like A, but releasing both after a squeeze still sends the element the squeeze asked for.
    IambicB,
    /// Either contact keys directly: straight key on the tip, or a sideswiper on both.
    Straight,
}

impl Mode {
    /// Whether the dit is the ring contact: paddles reversed, which a straight key ignores.
    pub fn dit_on_ring(self, swap: bool) -> bool {
        swap && self != Mode::Straight
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Element {
    Dit,
    Dah,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Idle,
    /// Key down; `left` milliseconds remain, including this one.
    Mark { element: Element, left: u16 },
    /// The one-dit gap after an element.
    Space { element: Element, left: u16 },
}

/// What the radio jack expects.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Output {
    /// One line closes while the keyer sends.
    Straight,
    /// Tip and ring follow your dit and dah contacts, for the radio's own keyer.
    Paddle,
}

/// Tip and ring levels for the radio jack. A straight output keys the tip with `keyed`; paddle
/// output puts the dit contact on the tip and dah on the ring. `swap` exchanges tip and ring.
pub fn output_lines(output: Output, swap: bool, keyed: bool, dit: bool, dah: bool) -> (bool, bool) {
    let (tip, ring) = match output {
        Output::Straight => (keyed, false),
        Output::Paddle => (dit, dah),
    };
    if swap { (ring, tip) } else { (tip, ring) }
}

/// Contact state of the "key" jack, true while closed.
#[derive(Clone, Copy, Default, Debug)]
pub struct Contacts {
    pub tip: bool,
    pub ring: bool,
}

impl Contacts {
    /// (dit, dah).
    pub fn paddles(self, dit_on_ring: bool) -> (bool, bool) {
        if dit_on_ring { (self.ring, self.tip) } else { (self.tip, self.ring) }
    }
}

/// After a contact changes, changes are ignored this long. Contacts bounce for up to 6.2 ms
/// (Ganssle, 16 switches) or 5 to 10 ms (Curtis 8044 application note); 8 ms is under half the
/// 20 ms dot at 60 WPM.
const DEBOUNCE_MS: u8 = 8;

/// A ring closed this long is a mono plug or a short, not keying: longer than the 4.56 s "0" at
/// MIN_WPM, the longest character.
pub const RING_STUCK_MS: u16 = 5000;
/// A tip closed this long is stuck or shorted: long enough to hold a straight key down to tune.
pub const TIP_STUCK_MS: u16 = 30_000;

/// One contact of the key jack, debounced.
#[derive(Clone, Copy, Default)]
struct Contact {
    closed: bool,
    /// Milliseconds left in which changes are ignored.
    hold_ms: u8,
    /// Milliseconds closed without a break, up to the stuck time.
    closed_ms: u16,
    /// May not key until it opens.
    held: bool,
}

impl Contact {
    /// Closed for `stuck_ms`, the contact is held.
    fn tick(&mut self, raw: bool, stuck_ms: u16) {
        if self.hold_ms > 0 {
            self.hold_ms -= 1;
        } else if raw != self.closed {
            self.closed = raw;
            self.hold_ms = DEBOUNCE_MS - 1;
        }
        self.closed_ms = if self.closed { (self.closed_ms + 1).min(stuck_ms) } else { 0 };
        self.held = (self.held || self.closed_ms == stuck_ms) && self.closed;
    }
}

/// The key jack's contacts, stepped once per millisecond before anything reads them. The first
/// change of a contact counts at once; changes in the next DEBOUNCE_MS are bounce.
///
/// A contact closed at power on, or for its stuck time, stops keying until it opens. A mono plug
/// shorts the ring, so the ring's stuck time is short and a mono plug keys only on the tip
/// whenever it goes in.
pub struct Jack {
    tip: Contact,
    ring: Contact,
}

impl Jack {
    /// `raw` is the jack at power on: a contact already closed may not key until it opens.
    pub fn new(raw: Contacts) -> Jack {
        let contact = |closed| Contact { closed, hold_ms: 0, closed_ms: 0, held: closed };
        Jack { tip: contact(raw.tip), ring: contact(raw.ring) }
    }

    pub fn tick(&mut self, raw: Contacts) {
        self.tip.tick(raw.tip, TIP_STUCK_MS);
        self.ring.tick(raw.ring, RING_STUCK_MS);
    }

    /// Which contacts are closed, debounced: for edges and the stuck contact warning.
    pub fn closed(&self) -> Contacts {
        Contacts { tip: self.tip.closed, ring: self.ring.closed }
    }

    /// Which contacts may key: closed, and not held.
    pub fn keying(&self) -> Contacts {
        let keys = |c: Contact| c.closed && !c.held;
        Contacts { tip: keys(self.tip), ring: keys(self.ring) }
    }
}

pub struct Keyer {
    mode: Mode,
    /// Tip is dit unless swapped.
    pub swap: bool,
    dit_ms: u16,
    state: State,
    dit_memory: bool,
    dah_memory: bool,
    /// Both paddles were closed together since they were last both open.
    squeezed: bool,
}

pub const MIN_WPM: u8 = 5;
pub const MAX_WPM: u8 = 60;

/// PARIS timing: one dit is 1200 / wpm milliseconds, to the nearest millisecond.
pub fn dit_ms(wpm: u8) -> u16 {
    let wpm = wpm.clamp(MIN_WPM, MAX_WPM) as u16;
    (1200 + wpm / 2) / wpm
}

impl Keyer {
    pub fn new(mode: Mode, swap: bool, wpm: u8) -> Self {
        let mut keyer =
            Keyer { mode, swap, dit_ms: 0, state: State::Idle, dit_memory: false, dah_memory: false, squeezed: false };
        keyer.set_wpm(wpm);
        keyer
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Starts the new mode idle, with nothing remembered from the old one.
    pub fn set_mode(&mut self, mode: Mode) {
        if mode != self.mode {
            *self = Keyer { mode, state: State::Idle, dit_memory: false, dah_memory: false, squeezed: false, ..*self };
        }
    }

    pub fn set_wpm(&mut self, wpm: u8) {
        self.dit_ms = dit_ms(wpm);
    }

    pub fn dit_ms(&self) -> u16 {
        self.dit_ms
    }

    /// Advances one millisecond. Returns true while the key is down.
    pub fn tick(&mut self, contacts: Contacts) -> bool {
        if self.mode == Mode::Straight {
            self.state = State::Idle;
            return contacts.tip || contacts.ring;
        }
        let (dit, dah) = contacts.paddles(self.mode.dit_on_ring(self.swap));

        // Both modes remember an opposite paddle pressed during an element (Curtis 8044, K3NG).
        match self.state {
            State::Mark { element: Element::Dit, .. } | State::Space { element: Element::Dit, .. } if dah => {
                self.dah_memory = true
            }
            State::Mark { element: Element::Dah, .. } | State::Space { element: Element::Dah, .. } if dit => {
                self.dit_memory = true
            }
            _ => {}
        }
        self.squeezed |= dit && dah;
        if !dit && !dah && core::mem::take(&mut self.squeezed) && self.mode == Mode::IambicA {
            self.dit_memory = false;
            self.dah_memory = false;
        }

        self.state = match self.state {
            State::Idle if dit => self.start(Element::Dit),
            State::Idle if dah => self.start(Element::Dah),
            State::Idle => State::Idle,
            State::Mark { element, left } if left > 1 => State::Mark { element, left: left - 1 },
            State::Mark { element, .. } => State::Space { element, left: self.dit_ms },
            State::Space { element, left } if left > 1 => State::Space { element, left: left - 1 },
            State::Space { element, .. } => {
                let want_dit = dit || self.dit_memory;
                let want_dah = dah || self.dah_memory;
                // Prefer the opposite element, so a squeeze alternates.
                match element {
                    Element::Dit if want_dah => self.start(Element::Dah),
                    Element::Dah if want_dit => self.start(Element::Dit),
                    _ if want_dit => self.start(Element::Dit),
                    _ if want_dah => self.start(Element::Dah),
                    _ => State::Idle,
                }
            }
        };
        matches!(self.state, State::Mark { .. })
    }

    fn start(&mut self, element: Element) -> State {
        let left = match element {
            Element::Dit => {
                self.dit_memory = false;
                self.dit_ms
            }
            Element::Dah => {
                self.dah_memory = false;
                3 * self.dit_ms
            }
        };
        State::Mark { element, left }
    }
}

#[cfg(test)]
mod output_tests {
    use super::*;

    #[test]
    fn a_straight_key_ignores_paddle_reversal() {
        let tip = Contacts { tip: true, ring: false };
        assert_eq!(tip.paddles(Mode::IambicB.dit_on_ring(true)), (false, true));
        assert_eq!(tip.paddles(Mode::Straight.dit_on_ring(true)), (true, false));
        assert_eq!(tip.paddles(Mode::IambicA.dit_on_ring(false)), (true, false));
    }

    #[test]
    fn output_lines_follow_type_and_swap() {
        assert_eq!(output_lines(Output::Straight, false, true, false, true), (true, false));
        assert_eq!(output_lines(Output::Straight, true, true, false, false), (false, true));
        assert_eq!(output_lines(Output::Paddle, false, false, true, false), (true, false));
        assert_eq!(output_lines(Output::Paddle, true, false, true, false), (false, true));
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    const TIP: Contacts = Contacts { tip: true, ring: false };
    const RING: Contacts = Contacts { tip: false, ring: true };
    const BOTH: Contacts = Contacts { tip: true, ring: true };
    const NONE: Contacts = Contacts { tip: false, ring: false };

    /// Runs `ms` ticks per contact step and returns the key output as runs of (down, ms).
    fn run(keyer: &mut Keyer, steps: &[(Contacts, u32)]) -> Vec<(bool, u32)> {
        let mut runs: Vec<(bool, u32)> = Vec::new();
        for &(contacts, ms) in steps {
            for _ in 0..ms {
                let down = keyer.tick(contacts);
                match runs.last_mut() {
                    Some((d, n)) if *d == down => *n += 1,
                    _ => runs.push((down, 1)),
                }
            }
        }
        runs
    }

    #[test]
    fn held_dit_repeats_at_20_wpm() {
        let mut k = Keyer::new(Mode::IambicB, false, 20);
        assert_eq!(run(&mut k, &[(TIP, 240)]), [(true, 60), (false, 60), (true, 60), (false, 60)]);
    }

    #[test]
    fn dah_is_three_dits() {
        let mut k = Keyer::new(Mode::IambicB, false, 20);
        assert_eq!(run(&mut k, &[(RING, 10), (NONE, 300)]), [(true, 180), (false, 130)]);
    }

    #[test]
    fn squeeze_alternates_dit_first() {
        let mut k = Keyer::new(Mode::IambicA, false, 20);
        let runs = run(&mut k, &[(BOTH, 480)]);
        assert_eq!(runs, [(true, 60), (false, 60), (true, 180), (false, 60), (true, 60), (false, 60)]);
    }

    #[test]
    fn iambic_b_sends_one_more_after_squeeze_release() {
        let mut k = Keyer::new(Mode::IambicB, false, 20);
        // Squeeze, then release both during the dah.
        let runs = run(&mut k, &[(BOTH, 150), (NONE, 400)]);
        assert_eq!(runs, [(true, 60), (false, 60), (true, 180), (false, 60), (true, 60), (false, 130)]);
    }

    #[test]
    fn iambic_a_stops_after_squeeze_release() {
        let mut k = Keyer::new(Mode::IambicA, false, 20);
        let runs = run(&mut k, &[(BOTH, 150), (NONE, 400)]);
        assert_eq!(runs, [(true, 60), (false, 60), (true, 180), (false, 250)]);
    }

    #[test]
    fn swap_makes_tip_dah() {
        let mut k = Keyer::new(Mode::IambicB, true, 20);
        assert_eq!(run(&mut k, &[(TIP, 10), (NONE, 230)]), [(true, 180), (false, 60)]);
    }

    #[test]
    fn iambic_a_remembers_a_tap_during_an_element() {
        let mut k = Keyer::new(Mode::IambicA, false, 20);
        let runs = run(&mut k, &[(TIP, 10), (RING, 20), (NONE, 400)]);
        assert_eq!(&runs[..3], [(true, 60), (false, 60), (true, 180)]);
    }

    #[test]
    fn iambic_a_squeeze_released_during_the_dit_sends_only_the_dit() {
        let mut k = Keyer::new(Mode::IambicA, false, 20);
        let runs = run(&mut k, &[(TIP, 10), (BOTH, 5), (RING, 15), (NONE, 400)]);
        assert_eq!(runs, [(true, 60), (false, 370)]);
    }

    #[test]
    fn a_mode_change_forgets_the_old_mode() {
        let mut k = Keyer::new(Mode::IambicB, false, 20);
        run(&mut k, &[(BOTH, 30)]);
        k.set_mode(Mode::Straight);
        run(&mut k, &[(NONE, 10)]);
        k.set_mode(Mode::IambicB);
        let runs = run(&mut k, &[(TIP, 10), (NONE, 300)]);
        assert_eq!(runs, [(true, 60), (false, 250)]);
    }

    #[test]
    fn dit_rounds_to_the_nearest_millisecond() {
        assert_eq!(dit_ms(55), 22, "21.8 ms");
        assert_eq!(dit_ms(18), 67, "66.7 ms");
        assert_eq!(dit_ms(20), 60);
    }

    /// Runs the jack, then the keyer, as the firmware does.
    fn run_jack(jack: &mut Jack, keyer: &mut Keyer, steps: &[(Contacts, u32)]) -> Vec<(bool, u32)> {
        let mut runs: Vec<(bool, u32)> = Vec::new();
        for &(raw, ms) in steps {
            for _ in 0..ms {
                jack.tick(raw);
                let down = keyer.tick(jack.keying());
                match runs.last_mut() {
                    Some((d, n)) if *d == down => *n += 1,
                    _ => runs.push((down, 1)),
                }
            }
        }
        runs
    }

    #[test]
    fn contact_bounce_makes_one_mark() {
        let mut jack = Jack::new(NONE);
        let mut k = Keyer::new(Mode::Straight, false, 20);
        let runs = run_jack(&mut jack, &mut k, &[(TIP, 1), (NONE, 1), (TIP, 1), (NONE, 1), (TIP, 100), (NONE, 20)]);
        assert_eq!(runs, [(true, 104), (false, 20)]);
    }

    #[test]
    fn a_mono_plug_does_not_key_a_straight_key() {
        // A mono plug shorts the ring from power on; the tip still keys.
        let mut jack = Jack::new(RING);
        let mut k = Keyer::new(Mode::Straight, false, 20);
        let runs = run_jack(&mut jack, &mut k, &[(RING, 3000), (BOTH, 60), (RING, 60)]);
        assert_eq!(runs, [(false, 3000), (true, 60), (false, 60)]);
        assert!(jack.closed().ring, "the stuck warning still sees the ring");
    }

    #[test]
    fn a_paddle_held_at_power_on_keys_after_it_opens() {
        let mut jack = Jack::new(TIP);
        let mut k = Keyer::new(Mode::IambicB, false, 20);
        let runs = run_jack(&mut jack, &mut k, &[(TIP, 500), (NONE, 20), (TIP, 60)]);
        assert_eq!(runs, [(false, 520), (true, 60)]);
    }

    #[test]
    fn a_mono_plug_put_in_while_on_stops_keying_the_ring() {
        let mut jack = Jack::new(NONE);
        let mut k = Keyer::new(Mode::Straight, false, 20);
        let stuck = RING_STUCK_MS as u32;
        let runs = run_jack(&mut jack, &mut k, &[(RING, stuck + 500), (BOTH, 60), (RING, 60)]);
        assert_eq!(runs, [(true, stuck - 1), (false, 501), (true, 60), (false, 60)]);
        assert!(jack.closed().ring, "the stuck warning still sees the ring");
    }

    #[test]
    fn a_ring_held_until_stuck_keys_again_after_it_opens() {
        let mut jack = Jack::new(NONE);
        let mut k = Keyer::new(Mode::Straight, false, 20);
        let stuck = RING_STUCK_MS as u32;
        let runs = run_jack(&mut jack, &mut k, &[(RING, stuck), (NONE, 20), (RING, 60)]);
        assert_eq!(runs, [(true, stuck - 1), (false, 21), (true, 60)]);
    }

    #[test]
    fn a_tip_held_to_tune_keys_until_stuck_then_again_after_it_opens() {
        let mut jack = Jack::new(NONE);
        let mut k = Keyer::new(Mode::Straight, false, 20);
        let stuck = TIP_STUCK_MS as u32;
        let runs = run_jack(&mut jack, &mut k, &[(TIP, stuck + 500), (NONE, 20), (TIP, 60)]);
        assert_eq!(runs, [(true, stuck - 1), (false, 521), (true, 60)]);
    }

    #[test]
    fn straight_follows_either_contact() {
        let mut k = Keyer::new(Mode::Straight, false, 20);
        assert_eq!(run(&mut k, &[(TIP, 5), (NONE, 5), (RING, 5)]), [(true, 5), (false, 5), (true, 5)]);
    }
}
