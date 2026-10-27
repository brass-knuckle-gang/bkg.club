//! Keys text that streams in from the serial port, stepped once per millisecond. Characters wait
//! in a queue as they arrive, and each one reports its echo once its last mark ends.

use crate::morse;

/// Characters and buffered commands that can wait at once.
pub const QUEUE_LEN: usize = 128;
/// Longest code `Merge` can make: two prosigns of up to 8 elements.
const PATTERN_LEN: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Item {
    /// A character as `morse::code` knows it, and the byte to report once it is keyed. A space
    /// keys a word gap.
    Char { c: u8, echo: Option<u8> },
    /// Two characters keyed as one, with no gap between them.
    Merge { first: u8, second: u8, echo: Option<u8> },
    /// Speed for the items after it, until `CancelSpeed`.
    Speed(u8),
    CancelSpeed,
    /// Half a dot of silence.
    HalfDot,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Tick {
    pub down: bool,
    /// The echo of a character whose last mark just ended.
    pub sent: Option<u8>,
}

#[derive(Clone, Copy)]
pub struct Sender {
    queue: [Item; QUEUE_LEN],
    head: usize,
    len: usize,
    /// Dots and dashes of the character being keyed, and the next one.
    pattern: [u8; PATTERN_LEN],
    pattern_len: usize,
    element: usize,
    echo: Option<u8>,
    /// An echo that came in the same millisecond as another, reported on the next tick.
    late: Option<u8>,
    /// Silence owed before the next mark, in dots.
    gap: u8,
    down: bool,
    remaining_ms: u32,
    buffered_wpm: Option<u8>,
    paused: bool,
    tune: bool,
}

impl Default for Sender {
    fn default() -> Self {
        Self::new()
    }
}

impl Sender {
    pub const fn new() -> Self {
        Sender {
            queue: [Item::CancelSpeed; QUEUE_LEN],
            head: 0,
            len: 0,
            pattern: [0; PATTERN_LEN],
            pattern_len: 0,
            element: 0,
            echo: None,
            late: None,
            gap: 0,
            down: false,
            remaining_ms: 0,
            buffered_wpm: None,
            paused: false,
            tune: false,
        }
    }

    /// False when the queue is full.
    pub fn push(&mut self, item: Item) -> bool {
        if self.len == QUEUE_LEN {
            return false;
        }
        self.queue[(self.head + self.len) % QUEUE_LEN] = item;
        self.len += 1;
        true
    }

    /// Queues `text`, with prosigns written as `<AR>`, each character echoing itself. False when
    /// the queue had no room for all of it.
    pub fn push_text(&mut self, text: &str) -> bool {
        morse::chars(text.as_bytes()).all(|c| self.push(Item::Char { c, echo: Some(c) }))
    }

    /// Drops the newest item not yet started. False when there is none.
    pub fn backspace(&mut self) -> bool {
        let had = self.len > 0;
        self.len = self.len.saturating_sub(1);
        had
    }

    /// Stops at once and drops everything queued.
    pub fn clear(&mut self) {
        *self = Sender { paused: self.paused, ..Sender::new() };
    }

    /// While paused, the character being keyed finishes and the next one waits.
    pub fn set_pause(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// Holds the key down, ahead of everything else.
    pub fn set_tune(&mut self, tune: bool) {
        self.tune = tune;
    }

    pub fn queued(&self) -> usize {
        self.len
    }

    /// Keying, owing a gap, or holding items.
    pub fn is_busy(&self) -> bool {
        self.tune
            || self.remaining_ms > 0
            || self.element < self.pattern_len
            || self.echo.is_some()
            || self.late.is_some()
            || self.gap > 0
            || self.len > 0
    }

    /// Steps one millisecond, at `dit_ms` unless a buffered speed is in force.
    pub fn tick(&mut self, dit_ms: u16) -> Tick {
        if self.tune {
            return Tick { down: true, sent: None };
        }
        let mut sent = self.late.take();
        while self.remaining_ms == 0 {
            // Echoes come only between characters, so the key is up while one waits.
            if self.late.is_some() {
                self.down = false;
                return Tick { down: false, sent };
            }
            let dot = match self.buffered_wpm {
                Some(wpm) => crate::keyer::dit_ms(wpm),
                None => dit_ms,
            } as u32;
            match self.next(dot) {
                Step::Key(down, ms) => {
                    self.down = down;
                    self.remaining_ms = ms;
                }
                Step::Sent(None) => {}
                Step::Sent(Some(echo)) if sent.is_none() => sent = Some(echo),
                Step::Sent(late) => self.late = late,
                Step::Idle => {
                    self.down = false;
                    return Tick { down: false, sent };
                }
            }
        }
        self.remaining_ms -= 1;
        Tick { down: self.down, sent }
    }

    fn next(&mut self, dot: u32) -> Step {
        if self.element < self.pattern_len {
            if self.gap > 0 {
                return Step::Key(false, core::mem::take(&mut self.gap) as u32 * dot);
            }
            let dash = self.pattern[self.element] == b'-';
            self.element += 1;
            self.gap = if self.element == self.pattern_len { 3 } else { 1 };
            return Step::Key(true, if dash { 3 * dot } else { dot });
        }
        if self.pattern_len > 0 {
            self.pattern_len = 0;
            self.element = 0;
            return Step::Sent(self.echo.take());
        }
        let item = if self.paused || self.len == 0 {
            None
        } else {
            let item = self.queue[self.head];
            self.head = (self.head + 1) % QUEUE_LEN;
            self.len -= 1;
            Some(item)
        };
        match item {
            None if self.gap > 0 => Step::Key(false, core::mem::take(&mut self.gap) as u32 * dot),
            None => Step::Idle,
            Some(Item::Char { c: b' ', echo }) => {
                if self.gap > 0 {
                    self.gap = 7;
                }
                Step::Sent(echo)
            }
            Some(Item::Char { c, echo }) => self.load(&[c], echo),
            Some(Item::Merge { first, second, echo }) => self.load(&[first, second], echo),
            Some(Item::Speed(wpm)) => {
                self.buffered_wpm = Some(wpm);
                Step::Sent(None)
            }
            Some(Item::CancelSpeed) => {
                self.buffered_wpm = None;
                Step::Sent(None)
            }
            Some(Item::HalfDot) => Step::Key(false, dot / 2),
        }
    }

    /// Starts keying `chars` as one character. One with no code is skipped, echo and all.
    fn load(&mut self, chars: &[u8], echo: Option<u8>) -> Step {
        let mut len = 0;
        for &c in chars {
            let Some(code) = morse::code(c) else { return Step::Sent(None) };
            self.pattern[len..len + code.len()].copy_from_slice(code.as_bytes());
            len += code.len();
        }
        self.pattern_len = len;
        self.element = 0;
        self.echo = echo;
        Step::Sent(None)
    }
}

enum Step {
    /// Key down or up for this many milliseconds.
    Key(bool, u32),
    /// Nothing to key; report this echo, if any, and look again.
    Sent(Option<u8>),
    Idle,
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    const DOT: u16 = 60;

    /// (down, ms) for each run of the key.
    type Runs = Vec<(bool, u32)>;
    /// (echo, ms) for each echo, at the millisecond it came.
    type Echoes = Vec<(u8, u32)>;

    /// The key's runs and the echoes until the sender idles.
    fn run(s: &mut Sender) -> (Runs, Echoes) {
        let (mut runs, mut echoes) = (Runs::new(), Echoes::new());
        let mut ms = 0;
        while s.is_busy() {
            let t = s.tick(DOT);
            if let Some(e) = t.sent {
                echoes.push((e, ms));
            }
            match runs.last_mut() {
                Some((d, n)) if *d == t.down => *n += 1,
                _ => runs.push((t.down, 1)),
            }
            ms += 1;
            assert!(ms < 100_000, "never idles");
        }
        (runs, echoes)
    }

    #[test]
    fn keys_text_with_letter_and_word_gaps_then_idles() {
        let mut s = Sender::new();
        assert!(s.push_text("EE T"));
        let (runs, echoes) = run(&mut s);
        assert_eq!(runs, [(true, 60), (false, 180), (true, 60), (false, 421), (true, 180), (false, 180)]);
        assert_eq!(runs.iter().map(|r| r.1).sum::<u32>(), 1081, "one late millisecond for the space echo");
        assert_eq!(echoes, [(b'E', 60), (b'E', 300), (b' ', 301), (b'T', 901)]);
        assert_eq!(s.tick(DOT), Tick { down: false, sent: None });
    }

    #[test]
    fn prosigns_merges_and_unknown_characters() {
        let mut s = Sender::new();
        s.push_text("<AR>~");
        s.push(Item::Merge { first: b'S', second: b'K', echo: Some(b'>') });
        let (runs, echoes) = run(&mut s);
        let marks: Vec<u32> = runs.iter().filter(|r| r.0).map(|r| r.1 / 60).collect();
        assert_eq!(marks, [1, 3, 1, 3, 1, 1, 1, 1, 3, 1, 3], ".-.-. then ...-.-");
        assert_eq!(echoes.iter().map(|e| e.0).collect::<Vec<_>>(), [0x01, b'>'], "~ is skipped");
    }

    #[test]
    fn buffered_speed_holds_until_cancelled() {
        let mut s = Sender::new();
        s.push(Item::Speed(12));
        s.push_text("E");
        s.push(Item::CancelSpeed);
        s.push_text("E");
        let (runs, _) = run(&mut s);
        assert_eq!(runs[0], (true, 100), "12 WPM");
        assert_eq!(runs[2], (true, 60), "back to the base speed");
    }

    #[test]
    fn backspace_clear_pause_and_tune() {
        let mut s = Sender::new();
        s.push_text("EI");
        assert!(s.backspace());
        let (_, echoes) = run(&mut s);
        assert_eq!(echoes, [(b'E', 60)]);
        assert!(!s.backspace());

        s.push_text("TT");
        s.tick(DOT);
        s.clear();
        assert!(!s.is_busy());
        assert!(!s.tick(DOT).down);

        s.set_pause(true);
        s.push_text("E");
        assert_eq!(s.tick(DOT), Tick { down: false, sent: None });
        s.set_pause(false);
        assert!(s.tick(DOT).down);

        s.set_tune(true);
        assert!(s.is_busy() && s.tick(DOT).down);
    }

    #[test]
    fn a_full_queue_refuses_more() {
        let mut s = Sender::new();
        for _ in 0..QUEUE_LEN {
            assert!(s.push(Item::HalfDot));
        }
        assert!(!s.push(Item::HalfDot));
        assert_eq!(s.queued(), QUEUE_LEN);
    }
}
