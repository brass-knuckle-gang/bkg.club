//! Receive drills: the device sends a group, you key back what you copied, and each character
//! is scored against what was sent.

use core::fmt::Write;

use crate::letters::{self, FIRST_LEVEL, KOCH, Letters};
use crate::morse;
use crate::qso::home;
use crate::ragchew;
use crate::rng::Rng;
use crate::text::Text;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Five-character groups from the first `level` Koch characters.
    Koch,
    Words,
    Calls,
    /// A content pack word, then the voice says it; see `Drill::pick_word`.
    Ninja,
    /// Head copy: one item at the current rung of `RUNGS`, keyed back after it ends.
    Head,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Koch => "KOCH",
            Kind::Words => "WORDS",
            Kind::Calls => "CALLS",
            Kind::Ninja => "NINJA",
            Kind::Head => "HEAD",
        }
    }
}

pub const GROUP_LEN: usize = 5;
/// Characters in a group, spaces included. `ragchew::sentence` lines fit, with room to spare.
pub const MAX_GROUP: usize = 20;
/// Copies longer than this are cut: 12 past the longest group leaves room for extra keying.
pub const MAX_COPY: usize = 32;
pub const MIN_LEVEL: u8 = FIRST_LEVEL;
pub const MAX_LEVEL: u8 = KOCH.len() as u8;
/// Koch's rule: add a character once you copy 90% of the current set. Accuracy here is one minus
/// the character error rate, (substituted + missed + extra) / sent, so keying extra characters
/// costs as much as missing them.
const LEVEL_WINDOW: u16 = 50;
const LEVEL_PERCENT: u16 = 90;

/// What one HEAD item holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Item {
    /// `count` pack words of `min` to `max` characters each, a prosign counting as one.
    Words { count: u8, min: u8, max: u8 },
    /// One line of a QSO exchange: `ragchew::sentence`.
    Sentence,
}

/// The head-copy ladder: rung 1 is the first.
pub const RUNGS: [Item; 8] = [
    Item::Words { count: 1, min: 2, max: 3 },
    Item::Words { count: 1, min: 4, max: 4 },
    Item::Words { count: 1, min: 5, max: 5 },
    Item::Words { count: 1, min: 6, max: 6 },
    Item::Words { count: 1, min: 7, max: 10 },
    Item::Words { count: 2, min: 2, max: 4 },
    Item::Words { count: 2, min: 3, max: 6 },
    Item::Sentence,
];
pub const FIRST_RUNG: u8 = 1;
/// HEAD copies in a row that move the rung: all right moves it up, under half right moves it
/// down.
const RUNG_STREAK: u8 = 3;

/// Common words and abbreviations heard on the air.
const WORDS: [&str; 64] = [
    "CQ", "DE", "TU", "73", "72", "5NN", "RST", "UR", "NAME", "QTH", "RIG", "ANT", "WX", "FB", "OP", "RPT", "GM", "GA", "GE",
    "ES", "HR", "TNX", "AGN", "PSE", "QRS", "QRZ", "QSL", "QSO", "QRP", "POTA", "SOTA", "PARK", "<BK>", "<KN>", "<SK>", "HW",
    "CPY", "CUL", "GL", "BEST", "KEY", "THE", "AND", "FOR", "YOU", "ARE", "WITH", "THAT", "HAVE", "THIS", "FROM", "WILL",
    "WORK", "GOOD", "SUN", "RAIN", "COLD", "HOT", "WATTS", "DIPOLE", "VERT", "MILES", "TODAY", "AGAIN",
];

/// How one sent character was copied.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mark {
    Right,
    Wrong(u8),
    Missed,
}

#[derive(Clone, Copy, Debug)]
pub struct Score {
    /// One mark per sent character.
    pub marks: [Mark; MAX_GROUP],
    pub len: usize,
    pub right: u8,
    /// Copied characters that match nothing sent.
    pub extra: u8,
    /// The Koch level went up after this group.
    pub level_up: bool,
}

pub struct Drill {
    pub kind: Kind,
    pub level: u8,
    rng: Rng,
    window_right: u16,
    window_total: u16,
    /// The level a hand change left and its window.
    left: Option<(u8, (u16, u16))>,
    /// The HEAD rung, from `FIRST_RUNG`.
    rung: u8,
    /// HEAD copies in a row all right, and under half right.
    streaks: (u8, u8),
}

impl Drill {
    pub fn new(kind: Kind, level: u8, seed: u32) -> Drill {
        let level = level.clamp(MIN_LEVEL, MAX_LEVEL);
        Drill { kind, level, rng: Rng::new(seed), window_right: 0, window_total: 0, left: None, rung: FIRST_RUNG, streaks: (0, 0) }
    }

    /// Sets the level by hand. Turning back to the level just left restores its window, so a
    /// stray dial turn costs no progress.
    pub fn set_level(&mut self, level: u8) {
        let level = level.clamp(MIN_LEVEL, MAX_LEVEL);
        if level == self.level {
            return;
        }
        let window = match self.left {
            Some((left, window)) if left == level => window,
            _ => (0, 0),
        };
        self.left = Some((self.level, self.window()));
        self.level = level;
        self.resume(window);
    }

    /// Progress toward the next level over the current window: (right less extra, total sent).
    pub fn window(&self) -> (u16, u16) {
        (self.window_right, self.window_total)
    }

    /// Continues the window of an earlier drill at this level.
    pub fn resume(&mut self, (right, total): (u16, u16)) {
        (self.window_right, self.window_total) = (right, total);
    }

    /// The index of a pack word among the first `limit` of the `count` pack words that start at
    /// `first`.
    pub fn pick_word(&mut self, first: u16, count: u16, limit: u16) -> u16 {
        first + self.rng.below(count.min(limit).max(1) as u32) as u16
    }

    /// Sets the HEAD rung and starts its streaks over.
    pub fn set_rung(&mut self, rung: u8) {
        self.rung = rung.clamp(FIRST_RUNG, RUNGS.len() as u8);
        self.streaks = (0, 0);
    }

    pub fn rung(&self) -> u8 {
        self.rung
    }

    /// The HEAD item at the current rung.
    pub fn item(&self) -> Item {
        RUNGS[(self.rung - FIRST_RUNG) as usize]
    }

    /// The next group to send. NINJA groups, and HEAD groups of words, come from the pack
    /// through `pick_word`, so this gives an empty one for them.
    pub fn next(&mut self, letters: &Letters) -> Text<MAX_GROUP> {
        let mut group = Text::new();
        match self.kind {
            Kind::Koch => {
                for _ in 0..GROUP_LEN {
                    group.push(self.pick(letters));
                }
            }
            Kind::Words => {
                for c in morse::chars(WORDS[self.rng.below(WORDS.len() as u32) as usize].as_bytes()) {
                    group.push(c);
                }
            }
            Kind::Calls => {
                let call = home(&mut self.rng).call(&mut self.rng);
                let _ = write!(group, "{}", call.as_str());
            }
            Kind::Head if self.item() == Item::Sentence => group = ragchew::sentence(&mut self.rng),
            Kind::Ninja | Kind::Head => {}
        }
        group
    }

    /// A Koch character, weighted toward weak ones and the newest.
    fn pick(&mut self, letters: &Letters) -> u8 {
        let n = self.level as usize;
        let weight = |i: usize| letters.weight(i) * if i == n - 1 { 2 } else { 1 };
        let total: u32 = (0..n).map(weight).sum();
        let mut r = self.rng.below(total);
        for (i, &c) in KOCH.iter().enumerate().take(n) {
            if r < weight(i) {
                return c;
            }
            r -= weight(i);
        }
        KOCH[n - 1]
    }

    /// Scores `copied` against `sent` as `score_copy` does, and moves the Koch level or the HEAD
    /// rung.
    pub fn score(&mut self, sent: &str, copied: &str, letters: &mut Letters) -> Score {
        let mut score = score_copy(sent, copied, letters);
        if self.kind == Kind::Koch {
            self.window_right += score.right.saturating_sub(score.extra) as u16;
            self.window_total += score.len as u16;
            let passed = self.window_right * 100 >= self.window_total * LEVEL_PERCENT;
            if self.window_total >= LEVEL_WINDOW && passed && self.level < MAX_LEVEL {
                self.level += 1;
                self.resume((0, 0));
                self.left = None;
                score.level_up = true;
            } else if self.window_total >= LEVEL_WINDOW {
                self.window_right = 0;
                self.window_total = 0;
            }
        }
        if self.kind == Kind::Head {
            self.climb(&score);
        }
        score
    }

    fn climb(&mut self, score: &Score) {
        let all_right = score.right as usize == score.len && score.extra == 0;
        let under_half = (score.right as usize) * 2 < score.len;
        let (right, wrong) = self.streaks;
        self.streaks = (if all_right { right + 1 } else { 0 }, if under_half { wrong + 1 } else { 0 });
        if self.streaks.0 == RUNG_STREAK {
            self.set_rung(self.rung + 1);
        } else if self.streaks.1 == RUNG_STREAK {
            self.set_rung(self.rung - 1);
        }
    }
}

/// Scores `copied` against `sent`, ignoring spaces and case, and records each sent character.
pub fn score_copy(sent: &str, copied: &str, letters: &mut Letters) -> Score {
    let sent = squash::<MAX_GROUP>(sent);
    let copied = squash::<MAX_COPY>(copied);
    let score = align(sent.as_str().as_bytes(), copied.as_str().as_bytes());
    for (&s, mark) in sent.as_str().as_bytes().iter().zip(&score.marks) {
        match *mark {
            Mark::Right => letters.record(s, Some(s)),
            Mark::Wrong(c) if letters::index(c).is_some() => letters.record(s, Some(c)),
            Mark::Wrong(_) | Mark::Missed => letters.record(s, None),
        }
    }
    score
}

pub(crate) fn squash<const N: usize>(s: &str) -> Text<N> {
    let mut out = Text::new();
    for c in morse::chars(s.as_bytes()).filter(|c| *c != b' ') {
        out.push(c.to_ascii_uppercase());
    }
    out
}

/// Edit-distance alignment of the copy against what was sent.
fn align(sent: &[u8], copied: &[u8]) -> Score {
    let (n, m) = (sent.len().min(MAX_GROUP), copied.len().min(MAX_COPY));
    let mut d = [[0u8; MAX_COPY + 1]; MAX_GROUP + 1];
    for (i, row) in d.iter_mut().enumerate().take(n + 1) {
        row[0] = i as u8;
    }
    for (j, cell) in d[0].iter_mut().enumerate().take(m + 1) {
        *cell = j as u8;
    }
    for i in 1..=n {
        for j in 1..=m {
            let change = d[i - 1][j - 1] + (sent[i - 1] != copied[j - 1]) as u8;
            d[i][j] = change.min(d[i - 1][j] + 1).min(d[i][j - 1] + 1);
        }
    }
    let mut score = Score { marks: [Mark::Missed; MAX_GROUP], len: n, right: 0, extra: 0, level_up: false };
    let (mut i, mut j) = (n, m);
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && d[i][j] == d[i - 1][j - 1] + (sent[i - 1] != copied[j - 1]) as u8 {
            score.marks[i - 1] = if sent[i - 1] == copied[j - 1] { Mark::Right } else { Mark::Wrong(copied[j - 1]) };
            (i, j) = (i - 1, j - 1);
        } else if i > 0 && d[i][j] == d[i - 1][j] + 1 {
            score.marks[i - 1] = Mark::Missed;
            i -= 1;
        } else {
            score.extra += 1;
            j -= 1;
        }
    }
    score.right = score.marks[..n].iter().filter(|m| **m == Mark::Right).count() as u8;
    score
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    fn marks(sent: &str, copied: &str) -> std::vec::Vec<Mark> {
        let score = align(sent.as_bytes(), copied.as_bytes());
        score.marks[..score.len].to_vec()
    }

    #[test]
    fn alignment_finds_wrong_missed_and_extra() {
        use Mark::*;
        assert_eq!(marks("KMRSE", "KMRSE"), [Right; 5]);
        assert_eq!(marks("KMRSE", "KMXSE"), [Right, Right, Wrong(b'X'), Right, Right]);
        assert_eq!(marks("KMRSE", "KMSE"), [Right, Right, Missed, Right, Right]);
        let score = align(b"KMRSE", b"KMRRSE");
        assert_eq!((score.right, score.extra), (5, 1));
        assert_eq!(marks("KMRSE", ""), [Missed; 5]);
    }

    #[test]
    fn three_right_in_a_row_climbs_a_rung_and_three_under_half_steps_down() {
        let mut drill = Drill::new(Kind::Head, MIN_LEVEL, 1);
        let mut letters = Letters::default();
        drill.score("THE", "THE", &mut letters);
        drill.score("THE", "THE", &mut letters);
        drill.score("THE", "THEE", &mut letters);
        assert_eq!(drill.rung, 1, "an extra character breaks the streak");
        for _ in 0..3 {
            drill.score("THE", "THE", &mut letters);
        }
        assert_eq!(drill.rung, 2);
        for _ in 0..2 {
            drill.score("TIME", "T", &mut letters);
        }
        drill.score("TIME", "TI", &mut letters);
        assert_eq!(drill.rung, 2, "half right is not under half");
        for _ in 0..3 {
            drill.score("TIME", "", &mut letters);
        }
        assert_eq!(drill.rung, 1);
        for _ in 0..3 {
            drill.score("AT", "", &mut letters);
        }
        assert_eq!(drill.rung, 1, "rung 1 is the bottom");
        drill.set_rung(RUNGS.len() as u8);
        for _ in 0..3 {
            drill.score("NAME BOB", "NAMEBOB", &mut letters);
        }
        assert_eq!(drill.rung, RUNGS.len() as u8, "the last rung is the top");
        drill.set_rung(FIRST_RUNG);
        for _ in 0..6 {
            drill.score("AT", "AT", &mut letters);
        }
        assert_eq!(drill.rung, 3, "a climb starts the streak over");
        assert_eq!(drill.level, MIN_LEVEL, "HEAD leaves the Koch level alone");
    }

    #[test]
    fn every_rung_fits_a_group() {
        for item in RUNGS {
            if let Item::Words { count, min, max } = item {
                assert!(min <= max && count * max + count - 1 <= MAX_GROUP as u8, "{item:?}");
            }
        }
        let mut drill = Drill::new(Kind::Head, MIN_LEVEL, 1);
        drill.set_rung(RUNGS.len() as u8);
        assert!(!drill.next(&Letters::default()).as_str().is_empty());
    }

    #[test]
    fn ninja_words_come_from_the_first_of_the_pack_words() {
        let mut drill = Drill::new(Kind::Ninja, MIN_LEVEL, 9);
        let limit = 500;
        let picks: std::vec::Vec<u16> = (0..2000).map(|_| drill.pick_word(49, 2979, limit)).collect();
        assert!(picks.iter().all(|&i| (49..49 + limit).contains(&i)));
        assert!(picks.contains(&49) && picks.contains(&(48 + limit)));
        assert!((0..50).all(|_| drill.pick_word(1, 3, limit) < 4));
    }

    #[test]
    fn koch_groups_use_only_the_level_set() {
        let mut drill = Drill::new(Kind::Koch, 4, 3);
        let letters = Letters::default();
        for _ in 0..50 {
            let group = drill.next(&letters);
            assert_eq!(group.len(), GROUP_LEN);
            assert!(group.as_str().bytes().all(|b| KOCH[..4].contains(&b)), "{}", group.as_str());
        }
    }

    #[test]
    fn ninety_percent_over_fifty_characters_adds_a_character() {
        let mut drill = Drill::new(Kind::Koch, 2, 5);
        let mut letters = Letters::default();
        for _ in 0..9 {
            assert!(!drill.score("KMKMK", "KMKMK", &mut letters).level_up);
        }
        assert!(drill.score("KMKMK", "KMKMM", &mut letters).level_up);
        assert_eq!((drill.level, drill.window()), (3, (0, 0)));
        let k = letters::index(b'K').unwrap();
        assert_eq!(letters.accuracy(k), Some((96, 30)));
    }

    #[test]
    fn turning_back_to_a_level_keeps_its_progress() {
        let mut drill = Drill::new(Kind::Koch, 4, 5);
        let mut letters = Letters::default();
        drill.score("KMRSK", "KMRSK", &mut letters);
        drill.set_level(5);
        assert_eq!(drill.window(), (0, 0));
        drill.set_level(4);
        assert_eq!(drill.window(), (5, 5));
        drill.set_level(5);
        drill.set_level(6);
        drill.set_level(4);
        assert_eq!(drill.window(), (0, 0));
    }

    #[test]
    fn a_level_up_leaves_no_window_to_return_to() {
        let mut drill = Drill::new(Kind::Koch, 2, 5);
        let mut letters = Letters::default();
        for _ in 0..10 {
            drill.score("KMKMK", "KMKMK", &mut letters);
        }
        assert_eq!(drill.level, 3);
        drill.set_level(2);
        assert_eq!(drill.window(), (0, 0));
    }

    #[test]
    fn extra_characters_count_as_errors() {
        let mut drill = Drill::new(Kind::Koch, 2, 1);
        let mut letters = Letters::default();
        for _ in 0..10 {
            assert!(!drill.score("KMKMK", "KMKMKMKMKM", &mut letters).level_up);
        }
        assert_eq!(drill.level, 2);
    }

    #[test]
    fn spaces_and_case_do_not_count() {
        let mut drill = Drill::new(Kind::Words, 2, 1);
        let score = drill.score("POTA", "po ta", &mut Letters::default());
        assert_eq!(score.right, 4);
    }

    #[test]
    fn a_prosign_scores_as_one_character() {
        let mut drill = Drill::new(Kind::Koch, MAX_LEVEL, 1);
        let mut letters = Letters::default();
        let sent: Text<MAX_GROUP> = morse::chars(b"K<SK>").fold(Text::new(), |mut t, c| {
            t.push(c);
            t
        });
        let score = drill.score(sent.as_str(), "K <SK>", &mut letters);
        assert_eq!((score.len, score.right), (2, 2));
        let sk = letters::index(sent.as_str().as_bytes()[1]).unwrap();
        assert_eq!(letters.accuracy(sk), Some((100, 1)));
    }
}
