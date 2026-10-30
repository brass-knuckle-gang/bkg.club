//! Brass Knuckles Knock Out: a boxing game that teaches Morse.
//!
//! The opponent telegraphs each punch by sending a character or two. Key the same back before
//! the punch lands and you dodge it; the opponent is then open, and every character you send is
//! a punch: all dits jab, all dahs hook, and a mix of both throws a combo. Clean timing hits
//! harder. Clean counters earn stars, and a star buys an uppercut: send a whole word or callsign
//! without an error.
//!
//! The ladder adds Koch characters fighter by fighter, sends faster, takes away the on-screen
//! hint and adds static, fading and longer tells. Tells lean on the characters you copy worst.
//!
//! Everything runs on the millisecond tick count. The screen calls `update` at `deadline`, and
//! reads the state to draw it.

use crate::letters::{self, KOCH, Letters};
use crate::morse;
use crate::qso::us_call;
use crate::record::{self, body_mut, frame, unframe};
use crate::rng::Rng;
use crate::text::Text;
use crate::ticks;
use crate::timing::Quality;

pub const FIGHTERS: usize = 8;
pub const ROUNDS: u8 = 3;
pub const ROUND_MS: u32 = 90_000;
pub const MAX_STARS: u8 = 3;
/// Clean counters that earn a star.
pub const CLEAN_PER_STAR: u8 = 3;
pub const PLAYER_HP: i16 = 100;
/// Knockdowns in one fight that end it.
pub const KNOCKDOWNS_TO_KO: u8 = 3;
pub const MAX_TELL: usize = 3;
pub const MAX_WORD: usize = 10;
/// One count of the referee over a fighter on the canvas.
pub const COUNT_MS: u32 = 1000;
const ROUND_BANNER_MS: u32 = 1800;
const ROUND_END_MS: u32 = 2600;
/// How long the punch takes to land once it is thrown, and how long the opponent stays off
/// balance after you dodge it.
const PUNCH_MS: u32 = 260;
const WHIFF_MS: u32 = 420;
/// The pause after you take a punch, before the next attack can start.
const STAGGER_MS: u32 = 700;
/// Time to send the uppercut word: this per character, after this much to start.
const STAR_START_MS: u32 = 2500;
const STAR_CHAR_MS: u32 = 1200;
const UPPERCUT_DAMAGE: i16 = 34;
const GET_UP_COUNT: u8 = 10;

/// One opponent on the ladder.
pub struct Fighter {
    pub name: &'static str,
    /// Shown on the intro card.
    pub from: &'static str,
    pub blurb: &'static str,
    /// Koch characters this fighter's tells use, counted from the start of `KOCH`.
    pub koch: u8,
    /// Added to the receive speed.
    pub speed_bump: u8,
    /// Farnsworth speed as a percent of the character speed.
    pub spacing_pct: u8,
    /// The tell shows on screen while it plays.
    pub hint: bool,
    /// Band noise and fading, 0..=`synth::MAX_LEVEL`, on top of your own settings.
    pub noise: u8,
    pub fading: u8,
    /// Longest tell, in characters.
    pub max_tell: u8,
    pub hp: i16,
    /// Damage one landed punch does to you.
    pub power: i16,
    /// Time after the tell ends to start keying it back, before your keying time is added.
    pub react_ms: u32,
    /// Pause between attacks: shortest, then the random extra.
    pub rest_ms: (u32, u32),
    /// How long the opponent stays open after you dodge.
    pub open_ms: u32,
    /// Offset of the opponent's tone from yours.
    pub pitch_offset_hz: i16,
    /// How unevenly the tells are keyed, in percent: a lid's sloppy fist, a sideswiper's swing.
    pub fist: u8,
}

pub static LADDER: [Fighter; FIGHTERS] = [
    Fighter {
        name: "LID LARRY",
        from: "THE BOTTOM OF THE BAND",
        blurb: "CALLS CQ ON TOP OF EVERYONE",
        koch: 5,
        speed_bump: 0,
        spacing_pct: 40,
        hint: true,
        noise: 0,
        fading: 0,
        max_tell: 1,
        hp: 60,
        power: 12,
        react_ms: 2600,
        rest_ms: (1800, 1400),
        open_ms: 4800,
        pitch_offset_hz: -60,
        fist: 14,
    },
    Fighter {
        name: "QRM QUEEN",
        from: "RIGHT ON YOUR FREQUENCY",
        blurb: "SHE SPLATTERS AND SHE KNOWS IT",
        koch: 10,
        speed_bump: 0,
        spacing_pct: 50,
        hint: true,
        noise: 1,
        fading: 0,
        max_tell: 1,
        hp: 75,
        power: 13,
        react_ms: 2300,
        rest_ms: (1600, 1300),
        open_ms: 4500,
        pitch_offset_hz: 90,
        fist: 4,
    },
    Fighter {
        name: "PILEUP PETE",
        from: "THE FRONT OF EVERY PILEUP",
        blurb: "NEVER WAITS HIS TURN",
        koch: 15,
        speed_bump: 1,
        spacing_pct: 60,
        hint: true,
        noise: 2,
        fading: 0,
        max_tell: 1,
        hp: 85,
        power: 14,
        react_ms: 2000,
        rest_ms: (1400, 1200),
        open_ms: 4200,
        pitch_offset_hz: -40,
        fist: 6,
    },
    Fighter {
        name: "BAREFOOT BOB",
        from: "100 WATTS AND A WIRE",
        blurb: "NO AMP, NO HINTS, NO MERCY",
        koch: 20,
        speed_bump: 2,
        spacing_pct: 70,
        hint: false,
        noise: 3,
        fading: 0,
        max_tell: 1,
        hp: 95,
        power: 15,
        react_ms: 1800,
        rest_ms: (1300, 1100),
        open_ms: 3900,
        pitch_offset_hz: 50,
        fist: 0,
    },
    Fighter {
        name: "CONTEST CARL",
        from: "A 48 HOUR CONTEST",
        blurb: "599 TU. NEXT!",
        koch: 25,
        speed_bump: 3,
        spacing_pct: 80,
        hint: false,
        noise: 3,
        fading: 0,
        max_tell: 2,
        hp: 105,
        power: 17,
        react_ms: 1600,
        rest_ms: (1100, 1000),
        open_ms: 3600,
        pitch_offset_hz: -80,
        fist: 0,
    },
    Fighter {
        name: "SKIP SADIE",
        from: "THE F2 LAYER",
        blurb: "HERE ONE SECOND, GONE THE NEXT",
        koch: 30,
        speed_bump: 4,
        spacing_pct: 90,
        hint: false,
        noise: 4,
        fading: 5,
        max_tell: 2,
        hp: 115,
        power: 18,
        react_ms: 1450,
        rest_ms: (1000, 900),
        open_ms: 3300,
        pitch_offset_hz: 110,
        fist: 3,
    },
    Fighter {
        name: "SIDESWIPER SID",
        from: "A KEY CUT FROM A HACKSAW BLADE",
        blurb: "WAGS BOTH WAYS, FAST",
        koch: 35,
        speed_bump: 5,
        spacing_pct: 100,
        hint: false,
        noise: 5,
        fading: 6,
        max_tell: 3,
        hp: 125,
        power: 20,
        react_ms: 1300,
        rest_ms: (900, 800),
        open_ms: 3000,
        pitch_offset_hz: -100,
        fist: 10,
    },
    Fighter {
        name: "BARON VON KILOWATT",
        from: "A CASTLE FULL OF AMPLIFIERS",
        blurb: "THE LEGAL LIMIT IS A SUGGESTION",
        koch: 41,
        speed_bump: 6,
        spacing_pct: 100,
        hint: false,
        noise: 6,
        fading: 7,
        max_tell: 3,
        hp: 140,
        power: 23,
        react_ms: 1150,
        rest_ms: (800, 700),
        open_ms: 2800,
        pitch_offset_hz: 0,
        fist: 0,
    },
];

impl Fighter {
    /// The Koch characters this fighter sends.
    pub fn set(&self) -> &'static [u8] {
        &KOCH[..self.koch as usize]
    }

    /// The characters this fighter adds to the one before.
    pub fn new_chars(&self, index: usize) -> &'static [u8] {
        let from = if index == 0 { 0 } else { LADDER[index - 1].koch as usize };
        &KOCH[from..self.koch as usize]
    }

    /// Character and Farnsworth speed of the tells, from your receive speed.
    pub fn speeds(&self, base_wpm: u8, farnsworth: u8) -> (u8, u8) {
        let wpm = (base_wpm as u16 + self.speed_bump as u16).min(60) as u8;
        let mut effective = ((wpm as u16 * self.spacing_pct as u16 / 100) as u8).max(5);
        if farnsworth > 0 {
            effective = effective.min(farnsworth);
        }
        (wpm, effective.min(wpm))
    }
}

/// What a character you send throws.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Punch {
    /// All dits.
    Jab,
    /// All dahs.
    Hook,
    /// Dits and dahs: one punch per run of the same element.
    Combo(u8),
}

/// The punch character `c` throws, from its code.
pub fn punch_of(c: u8) -> Option<Punch> {
    let code = morse::code(c)?.as_bytes();
    if code.iter().all(|&e| e == b'.') {
        Some(Punch::Jab)
    } else if code.iter().all(|&e| e == b'-') {
        Some(Punch::Hook)
    } else {
        let runs = 1 + code.windows(2).filter(|w| w[0] != w[1]).count();
        Some(Punch::Combo(runs.min(4) as u8))
    }
}

/// Damage of `c` thrown with `quality` timing.
pub fn damage(c: u8, quality: Quality) -> i16 {
    let Some(code) = morse::code(c) else { return 0 };
    let len = code.len() as i16;
    let base = match punch_of(c) {
        Some(Punch::Jab) => 3 + len,
        Some(Punch::Hook) => 4 + len,
        Some(Punch::Combo(_)) => 3 + 2 * len,
        None => 0,
    };
    base * quality_percent(quality) / 100
}

fn quality_percent(q: Quality) -> i16 {
    match q {
        Quality::Good => 100,
        Quality::Off => 65,
        Quality::Bad => 35,
    }
}

/// The ideal length of character `c` in dots, with no gap after it.
pub fn units(c: u8) -> u32 {
    morse::code(c).map_or(0, |code| code.bytes().map(|e| if e == b'-' { 3 } else { 1 }).sum::<u32>() + code.len() as u32 - 1)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    pub fn other(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        }
    }
}

/// What the opponent is doing, for the screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Foe {
    Idle,
    /// Sending the tell, arm drawn back on `Side`.
    Windup(Side),
    Punch(Side),
    /// Swung at air after your dodge.
    Whiff(Side),
    /// Off balance: your counters land.
    Open,
    Hurt(Side),
    Block,
    /// Stunned by a star punch while you send the word.
    Stunned,
    Down,
    Out,
    Rise,
    Victory,
}

/// What you are doing, for the screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Me {
    Idle,
    Dodge(Side),
    Jab(Side),
    Hook(Side),
    /// The `u8`th of `Combo` punches.
    Combo(u8),
    Uppercut,
    /// Your punch hit gloves.
    Blocked(Side),
    /// A pattern no character uses: a swing at air.
    Miss,
    Hurt,
    Down,
    Rise,
    Victory,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    Ko,
    Tko,
    Decision,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// The fighter's card, before the first bell.
    Intro,
    RoundStart,
    Fight,
    /// The opponent is down; the count is `count`.
    FoeDown,
    /// You are down: key `get_up` before the count reaches ten.
    MeDown,
    RoundEnd,
    /// The fight is over: you won or lost, and how.
    Over { won: bool, how: Outcome },
}

/// What happens between the bells.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Duel {
    /// The next attack starts at the deadline.
    Neutral,
    /// The tell is playing; you can already key it back.
    Tell,
    /// The tell ended; the punch lands at the deadline.
    Window,
    /// The punch is on its way: it lands, or misses you after a dodge, at the deadline.
    Punch { dodged: bool },
    /// Your counters land until the deadline.
    Open,
    /// You armed a star: send `word` before the deadline.
    Star,
    /// The pause after a punch landed on you.
    Stagger,
}

/// A short message for the screen, with the time it happened.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Callout {
    Dodge,
    /// Your keying did not match the tell: you sent this, it was that.
    Wrong { sent: u8 },
    TooSlow,
    Clean,
    Star,
    Uppercut,
    Blocked,
    Missed,
    Down,
    Up,
}

/// Counts for the results screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FightStats {
    pub tells: u16,
    pub dodged: u16,
    pub punches: u16,
    pub clean: u16,
    pub damage: u16,
    pub taken: u16,
    pub uppercuts: u8,
    pub knockdowns: u8,
    pub knocked: u8,
    /// Tries and right per character of `KOCH`, this fight only.
    pub tries: [u8; KOCH.len()],
    pub right: [u8; KOCH.len()],
}

impl Default for FightStats {
    fn default() -> Self {
        FightStats {
            tells: 0,
            dodged: 0,
            punches: 0,
            clean: 0,
            damage: 0,
            taken: 0,
            uppercuts: 0,
            knockdowns: 0,
            knocked: 0,
            tries: [0; KOCH.len()],
            right: [0; KOCH.len()],
        }
    }
}

impl FightStats {
    pub fn accuracy(&self) -> Option<u8> {
        (self.tells > 0).then(|| (self.dodged as u32 * 100 / self.tells as u32) as u8)
    }

    pub fn clean_percent(&self) -> Option<u8> {
        (self.punches > 0).then(|| (self.clean as u32 * 100 / self.punches as u32) as u8)
    }

    fn count(&mut self, c: u8, right: bool) {
        if let Some(i) = letters::index(c) {
            self.tries[i] = self.tries[i].saturating_add(1);
            self.right[i] = self.right[i].saturating_add(right as u8);
        }
    }

    /// Characters sent this fight, worst first: (character, right, tries).
    pub fn worst(&self) -> impl Iterator<Item = (u8, u8, u8)> + '_ {
        let mut order: [usize; KOCH.len()] = core::array::from_fn(|i| i);
        // Misses first, then the lowest share right, then the most tries.
        crate::math::sort_by_key(&mut order, |&i| {
            let (r, t) = (self.right[i] as u32, self.tries[i] as u32);
            (t == 0, r * 1000 / t.max(1), u32::MAX - t)
        });
        order.into_iter().filter(|&i| self.tries[i] > 0).map(|i| (KOCH[i], self.right[i], self.tries[i]))
    }
}

/// Words for uppercuts. Each fighter uses the ones made only of its characters.
const WORDS: [&str; 72] = [
    "MURK", "EMU", "RUE", "SURE", "USER", "MUSE", "RUSE", "SUM", "SKA", "TREK", "MARK", "PARK", "SNAP", "STAMP", "MASTER",
    "PAUSE", "SPARK", "TRAMP", "ANTS", "RANK", "TUNE", "NAME", "UR", "RST", "TNX", "TU", "ES", "PSE", "QSL", "QRP", "QRM",
    "QRZ", "QTH", "SKED", "POTA", "SOTA", "CQ", "DE", "73", "5NN", "599", "WX", "RIG", "ANT", "KEY", "DIPOLE", "WATTS",
    "BAND", "WIRE", "LOOP", "YAGI", "BEAM", "FIST", "SPLIT", "PILEUP", "LID", "DX", "CW", "WPM", "FB", "OM", "YL", "GM",
    "HAM", "RADIO", "MORSE", "BOXING", "PUNCH", "JAB", "HOOK", "KNOCKOUT", "CHAMP",
];

/// How long each reaction shows before the fighter settles back.
fn foe_event_ms(pose: Foe) -> u32 {
    match pose {
        Foe::Hurt(_) => 380,
        Foe::Block => 320,
        Foe::Rise => 700,
        _ => 0,
    }
}

fn me_event_ms(pose: Me) -> u32 {
    match pose {
        Me::Jab(_) => 240,
        Me::Hook(_) => 320,
        Me::Combo(n) => 150 * n as u32 + 120,
        Me::Uppercut => 760,
        Me::Blocked(_) => 280,
        Me::Miss => 300,
        Me::Hurt => 520,
        Me::Rise => 700,
        _ => 0,
    }
}

pub struct Fight {
    pub fighter: usize,
    pub phase: Phase,
    duel: Duel,
    /// When the current phase or duel step started, and when it ends.
    since: u32,
    due: Option<u32>,
    pub round: u8,
    round_ends: u32,
    /// Round time left while the clock is stopped: before the bell, on the canvas.
    round_left: u32,
    pub foe_hp: i16,
    pub my_hp: i16,
    pub stars: u8,
    /// Clean counters toward the next star.
    pub clean_run: u8,
    pub score: u32,
    pub tell: Text<MAX_TELL>,
    /// Your keying of the tell so far.
    pub answer: Text<MAX_TELL>,
    /// The tell is shown: while it plays with hints, and once it is over.
    pub tell_shown: bool,
    /// The uppercut word, while a star is armed, and what you have sent of it.
    pub word: Text<MAX_WORD>,
    pub typed: Text<MAX_WORD>,
    /// The referee's count over a fighter on the canvas.
    pub count: u8,
    /// The count the opponent gets up at; GET_UP_COUNT stays down.
    get_up_at: u8,
    /// What you key to get up.
    pub get_up: u8,
    /// The newest reaction of each fighter, and when it started.
    foe_event: Option<(Foe, u32)>,
    me_event: Option<(Me, u32)>,
    pub callout: Option<(Callout, u32)>,
    /// The punching hand of the attack.
    side: Side,
    /// Your last punching hand, so jabs alternate.
    my_side: Side,
    pub stats: FightStats,
    rng: Rng,
    /// A tell waiting for the screen to play it.
    play: Option<Text<MAX_TELL>>,
    /// The screen should stop the tell now.
    pub stop: bool,
    hints: bool,
    /// Your dot length: the reply window adds the time to key the tell at it.
    my_dit_ms: u16,
    paused_at: Option<u32>,
    callsign: Text<MAX_WORD>,
    /// Drill weight per Koch character, from your letter statistics.
    weights: [u32; KOCH.len()],
    /// Characters of the uppercut word keyed with timing better than bad.
    star_clean: u8,
    copies: Copies,
}

/// Characters judged since the screen last took them: (sent, copied), copied None when missed.
#[derive(Clone, Copy)]
struct Copies {
    items: [(u8, Option<u8>); 8],
    len: usize,
}

impl Copies {
    fn push(&mut self, item: (u8, Option<u8>)) {
        if self.len == self.items.len() {
            self.items.copy_within(1.., 0);
            self.len -= 1;
        }
        self.items[self.len] = item;
        self.len += 1;
    }
}

impl Fight {
    /// A fight against ladder fighter `fighter`. `my_dit_ms` is your keyer's dot length.
    pub fn new(fighter: usize, seed: u32, hints: bool, my_dit_ms: u16, callsign: &str) -> Fight {
        let fighter = fighter.min(FIGHTERS - 1);
        let mut call = Text::new();
        for b in callsign.bytes().take(MAX_WORD) {
            call.push(b);
        }
        Fight {
            fighter,
            phase: Phase::Intro,
            duel: Duel::Neutral,
            since: 0,
            due: None,
            round: 1,
            round_ends: 0,
            round_left: ROUND_MS,
            foe_hp: LADDER[fighter].hp,
            my_hp: PLAYER_HP,
            stars: 0,
            clean_run: 0,
            score: 0,
            tell: Text::new(),
            answer: Text::new(),
            tell_shown: false,
            word: Text::new(),
            typed: Text::new(),
            count: 0,
            get_up_at: 0,
            get_up: KOCH[0],
            foe_event: None,
            me_event: None,
            callout: None,
            side: Side::Right,
            my_side: Side::Left,
            stats: FightStats::default(),
            rng: Rng::new(seed),
            play: None,
            stop: false,
            hints,
            my_dit_ms,
            paused_at: None,
            callsign: call,
            weights: [8; KOCH.len()],
            star_clean: 0,
            copies: Copies { items: [(0, None); 8], len: 0 },
        }
    }

    pub fn fighter(&self) -> &'static Fighter {
        &LADDER[self.fighter]
    }

    /// Takes the drill weight of each character from your letter statistics, so tells lean on
    /// the characters you copy worst.
    pub fn learn(&mut self, letters: &Letters) {
        self.weights = core::array::from_fn(|i| letters.weight(i));
    }

    pub fn set_hints(&mut self, on: bool) {
        self.hints = on;
    }

    pub fn set_dit_ms(&mut self, dit_ms: u16) {
        self.my_dit_ms = dit_ms;
    }

    /// The tell shows while it plays.
    pub fn hinting(&self) -> bool {
        self.hints && self.fighter().hint
    }

    pub fn paused(&self) -> bool {
        self.paused_at.is_some()
    }

    /// The opponent is open to counters.
    pub fn open(&self) -> bool {
        self.phase == Phase::Fight && self.duel == Duel::Open
    }

    /// A star is armed and its word is on screen.
    pub fn starring(&self) -> bool {
        self.phase == Phase::Fight && self.duel == Duel::Star
    }

    /// The tell is playing or waiting for your answer.
    pub fn telling(&self) -> bool {
        self.phase == Phase::Fight && matches!(self.duel, Duel::Tell | Duel::Window)
    }

    /// Time left to answer the tell once it has played, or to send the uppercut word: (left,
    /// whole window).
    pub fn window(&self, now: u32) -> Option<(u32, u32)> {
        let due = self.due?;
        if self.phase != Phase::Fight || !matches!(self.duel, Duel::Window | Duel::Star) {
            return None;
        }
        Some((ticks::ms_until(self.paused_at.unwrap_or(now), due), due.wrapping_sub(self.since)))
    }

    /// The tell is sounding now.
    pub fn sounding(&self) -> bool {
        self.phase == Phase::Fight && self.duel == Duel::Tell
    }

    /// Round time left.
    pub fn clock_ms(&self, now: u32) -> u32 {
        if self.phase != Phase::Fight {
            return self.round_left;
        }
        ticks::ms_until(self.paused_at.unwrap_or(now), self.round_ends)
    }

    /// The opponent's pose at `now`, and when it started.
    pub fn foe_pose(&self, now: u32) -> (Foe, u32) {
        if let Some((pose, at)) = self.foe_event
            && now.wrapping_sub(at) < foe_event_ms(pose)
        {
            return (pose, at);
        }
        let pose = match self.phase {
            Phase::Fight => match self.duel {
                Duel::Tell | Duel::Window => Foe::Windup(self.side),
                Duel::Punch { dodged: false } => Foe::Punch(self.side),
                Duel::Punch { dodged: true } => Foe::Whiff(self.side),
                Duel::Open => Foe::Open,
                Duel::Star => Foe::Stunned,
                Duel::Neutral | Duel::Stagger => Foe::Idle,
            },
            Phase::FoeDown if self.get_up_at >= GET_UP_COUNT => Foe::Out,
            Phase::FoeDown => Foe::Down,
            Phase::MeDown => Foe::Victory,
            Phase::Over { won: true, how: Outcome::Decision } => Foe::Idle,
            Phase::Over { won: true, .. } => Foe::Out,
            Phase::Over { won: false, .. } => Foe::Victory,
            Phase::Intro | Phase::RoundStart | Phase::RoundEnd => Foe::Idle,
        };
        (pose, self.since)
    }

    /// Your pose at `now`, and when it started.
    pub fn me_pose(&self, now: u32) -> (Me, u32) {
        if let Some((pose, at)) = self.me_event
            && now.wrapping_sub(at) < me_event_ms(pose)
        {
            return (pose, at);
        }
        let pose = match self.phase {
            Phase::Fight if self.duel == (Duel::Punch { dodged: true }) => Me::Dodge(self.side.other()),
            Phase::MeDown => Me::Down,
            Phase::Over { won: true, .. } => Me::Victory,
            Phase::Over { won: false, how: Outcome::Decision } => Me::Idle,
            Phase::Over { won: false, .. } => Me::Down,
            _ => Me::Idle,
        };
        (pose, self.since)
    }

    /// The tell to start playing now.
    pub fn take_play(&mut self) -> Option<Text<MAX_TELL>> {
        self.play.take()
    }

    /// When `update` should next run.
    pub fn deadline(&self) -> Option<u32> {
        if self.paused_at.is_some() {
            return None;
        }
        let round = (self.phase == Phase::Fight).then_some(self.round_ends);
        [self.due, round].into_iter().flatten().reduce(ticks::earlier)
    }

    fn step(&mut self, duel: Duel, now: u32, due: Option<u32>) {
        self.duel = duel;
        self.since = now;
        self.due = due;
    }

    fn enter(&mut self, phase: Phase, now: u32, due: Option<u32>) {
        self.phase = phase;
        self.since = now;
        self.due = due;
    }

    /// Rings the bell: from the intro card, or between rounds. False when it is not time.
    pub fn start(&mut self, now: u32) -> bool {
        match self.phase {
            Phase::Intro => {}
            Phase::RoundEnd if self.due.is_none() => self.next_round(),
            _ => return false,
        }
        self.enter(Phase::RoundStart, now, Some(now.wrapping_add(ROUND_BANNER_MS)));
        self.foe_event = None;
        self.me_event = None;
        true
    }

    pub fn pause(&mut self, now: u32) {
        if self.paused_at.is_some() || matches!(self.phase, Phase::Over { .. } | Phase::Intro) {
            return;
        }
        self.paused_at = Some(now);
        self.stop = true;
        // A tell cut short is taken back, and a new one starts after the pause.
        if self.phase == Phase::Fight && self.duel == Duel::Tell {
            self.stats.tells -= 1;
            self.tell.clear();
            self.answer.clear();
            self.tell_shown = false;
            self.step(Duel::Neutral, now, Some(now.wrapping_add(800)));
        }
    }

    pub fn resume(&mut self, now: u32) {
        let Some(at) = self.paused_at.take() else { return };
        let gap = now.wrapping_sub(at);
        self.due = self.due.map(|d| d.wrapping_add(gap));
        self.round_ends = self.round_ends.wrapping_add(gap);
        self.since = self.since.wrapping_add(gap);
        if let Some((pose, since)) = self.foe_event {
            self.foe_event = Some((pose, since.wrapping_add(gap)));
        }
        if let Some((pose, since)) = self.me_event {
            self.me_event = Some((pose, since.wrapping_add(gap)));
        }
        if let Some((c, since)) = self.callout {
            self.callout = Some((c, since.wrapping_add(gap)));
        }
    }

    fn next_round(&mut self) {
        self.round += 1;
        self.round_left = ROUND_MS;
        // The corners patch both fighters up a little.
        let f = self.fighter();
        self.foe_hp = (self.foe_hp + (f.hp - self.foe_hp) / 4).min(f.hp);
        self.my_hp = (self.my_hp + (PLAYER_HP - self.my_hp) / 3).min(PLAYER_HP);
    }

    /// Runs whatever is due at `now`.
    pub fn update(&mut self, now: u32) {
        if self.paused_at.is_some() {
            return;
        }
        let due = self.due.is_some_and(|d| ticks::reached(now, d));
        match self.phase {
            Phase::RoundStart if due => {
                self.enter(Phase::Fight, now, None);
                self.round_ends = now.wrapping_add(self.round_left);
                self.step(Duel::Neutral, now, Some(now.wrapping_add(900)));
            }
            Phase::Fight => {
                // A punch on its way still lands after the bell.
                if ticks::reached(now, self.round_ends) && !matches!(self.duel, Duel::Punch { .. }) {
                    return self.bell(now);
                }
                if due {
                    self.duel_due(now);
                }
            }
            Phase::FoeDown if due => self.foe_count(now),
            Phase::MeDown if due => self.my_count(now),
            Phase::RoundEnd if due => self.due = None,
            _ => {}
        }
    }

    fn bell(&mut self, now: u32) {
        self.stop = true;
        self.round_left = 0;
        self.clear_texts();
        if self.round >= ROUNDS {
            let won = self.points() > self.foe_points();
            if won {
                self.score += 2000;
            }
            return self.finish(won, Outcome::Decision, now);
        }
        self.enter(Phase::RoundEnd, now, Some(now.wrapping_add(ROUND_END_MS)));
        self.duel = Duel::Neutral;
    }

    fn clear_texts(&mut self) {
        self.tell.clear();
        self.answer.clear();
        self.tell_shown = false;
        self.word.clear();
        self.typed.clear();
    }

    /// The judges' points for you: damage dealt, dodges and knockdowns.
    pub fn points(&self) -> u32 {
        self.stats.damage as u32 + 2 * self.stats.dodged as u32 + 10 * self.stats.knockdowns as u32
    }

    /// The judges' points for the opponent.
    pub fn foe_points(&self) -> u32 {
        self.stats.taken as u32 + 10 * self.stats.knocked as u32
    }

    fn duel_due(&mut self, now: u32) {
        match self.duel {
            Duel::Neutral | Duel::Stagger => self.attack(now),
            Duel::Tell => {}
            Duel::Window => {
                let tell = self.tell;
                for c in tell.as_str().bytes().skip(self.answer.len()) {
                    self.judge(c, None);
                }
                self.callout = Some((Callout::TooSlow, now));
                self.throw(now, false);
            }
            Duel::Punch { dodged: false } => self.land(now),
            Duel::Punch { dodged: true } => {
                let open = self.fighter().open_ms;
                self.step(Duel::Open, now, Some(now.wrapping_add(open)));
            }
            Duel::Open | Duel::Star => self.rest(now),
        }
    }

    fn rest(&mut self, now: u32) {
        let (least, extra) = self.fighter().rest_ms;
        let wait = least + self.rng.below(extra + 1);
        self.word.clear();
        self.typed.clear();
        self.step(Duel::Neutral, now, Some(now.wrapping_add(wait)));
    }

    /// Starts a tell.
    fn attack(&mut self, now: u32) {
        self.tell = self.pick_tell();
        self.answer.clear();
        self.tell_shown = self.hinting();
        self.side = if self.rng.below(2) == 0 { Side::Left } else { Side::Right };
        self.stats.tells += 1;
        self.play = Some(self.tell);
        self.step(Duel::Tell, now, None);
    }

    /// The tell finished playing at `now`: the reply window opens.
    pub fn tell_played(&mut self, now: u32) {
        if self.phase != Phase::Fight || self.duel != Duel::Tell || self.paused_at.is_some() {
            return;
        }
        let keying: u32 = self.tell.as_str().bytes().map(|c| units(c) + 3).sum::<u32>() * self.my_dit_ms as u32;
        self.step(Duel::Window, now, Some(now.wrapping_add(self.fighter().react_ms + keying)));
    }

    fn pick_tell(&mut self) -> Text<MAX_TELL> {
        let most = self.fighter().max_tell as u32;
        let mut len = 1 + self.rng.below(most);
        // Long tells come up less often than short ones.
        if len > 1 && self.rng.below(3) == 0 {
            len -= 1;
        }
        let mut tell = Text::new();
        for _ in 0..len {
            tell.push(self.pick_char());
        }
        tell
    }

    /// A character of this fighter's set, weighted toward weak ones and the fighter's own.
    fn pick_char(&mut self) -> u8 {
        let f = self.fighter();
        let n = f.koch as usize;
        let own_from = n - f.new_chars(self.fighter).len();
        let weights = self.weights;
        let weight = |i: usize| weights[i] * if i >= own_from { 3 } else { 1 };
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

    /// The opponent throws: you dodged, or it lands at the end of the swing.
    fn throw(&mut self, now: u32, dodged: bool) {
        self.stop = true;
        self.tell_shown = true;
        let swing = if dodged { WHIFF_MS } else { PUNCH_MS };
        self.step(Duel::Punch { dodged }, now, Some(now.wrapping_add(swing)));
    }

    fn land(&mut self, now: u32) {
        let power = self.fighter().power;
        self.my_hp = (self.my_hp - power).max(0);
        self.stats.taken += power as u16;
        self.clean_run = 0;
        self.me_event = Some((Me::Hurt, now));
        if self.my_hp == 0 {
            return self.knocked_down(now);
        }
        let wait = STAGGER_MS + self.fighter().rest_ms.0 / 2;
        self.step(Duel::Stagger, now, Some(now.wrapping_add(wait)));
    }

    /// A character you keyed, with the worst timing quality in it.
    pub fn key(&mut self, c: u8, quality: Quality, now: u32) {
        if self.paused_at.is_some() {
            return;
        }
        match self.phase {
            Phase::Fight => {}
            Phase::MeDown => return self.try_get_up(c, now),
            _ => return,
        }
        match self.duel {
            Duel::Tell | Duel::Window => self.answer_tell(c, now),
            Duel::Open => self.counter(c, quality, now),
            Duel::Star => self.star_char(c, quality, now),
            Duel::Neutral | Duel::Stagger => {
                // Into a raised guard.
                self.my_side = self.my_side.other();
                self.me_event = Some((Me::Blocked(self.my_side), now));
                self.foe_event = Some((Foe::Block, now));
                self.callout = Some((Callout::Blocked, now));
            }
            Duel::Punch { .. } => {}
        }
    }

    /// You keyed a pattern no character uses.
    pub fn key_unknown(&mut self, now: u32) {
        if self.paused_at.is_some() {
            return;
        }
        match (self.phase, self.duel) {
            (Phase::MeDown, _) => self.try_get_up(b'*', now),
            (Phase::Fight, Duel::Tell | Duel::Window) => self.answer_tell(b'*', now),
            (Phase::Fight, Duel::Open) => {
                self.me_event = Some((Me::Miss, now));
                self.callout = Some((Callout::Missed, now));
            }
            (Phase::Fight, Duel::Star) => self.star_char(b'*', Quality::Bad, now),
            _ => {}
        }
    }

    fn answer_tell(&mut self, c: u8, now: u32) {
        let at = self.answer.len();
        let Some(&want) = self.tell.as_str().as_bytes().get(at) else { return };
        self.answer.push(c);
        if c != want {
            self.judge(want, Some(c));
            let tell = self.tell;
            for rest in tell.as_str().bytes().skip(at + 1) {
                self.judge(rest, None);
            }
            self.callout = Some((Callout::Wrong { sent: c }, now));
            return self.throw(now, false);
        }
        self.judge(want, Some(want));
        if self.answer.len() == self.tell.len() {
            self.stats.dodged += 1;
            // A dodge before the tell even ends scores more.
            self.score += if self.duel == Duel::Tell { 80 } else { 50 };
            self.callout = Some((Callout::Dodge, now));
            self.throw(now, true);
        }
    }

    fn counter(&mut self, c: u8, quality: Quality, now: u32) {
        let Some(punch) = punch_of(c) else { return };
        let hit = damage(c, quality);
        self.stats.punches += 1;
        self.my_side = self.my_side.other();
        let pose = match punch {
            Punch::Jab => Me::Jab(self.my_side),
            Punch::Hook => Me::Hook(self.my_side),
            Punch::Combo(n) => Me::Combo(n),
        };
        self.me_event = Some((pose, now));
        // A hook from your left hand snaps the head to your right.
        self.foe_event = Some((Foe::Hurt(self.my_side.other()), now));
        self.score += hit as u32 * 10;
        if quality == Quality::Good {
            self.stats.clean += 1;
            self.score += 20;
            self.clean_run += 1;
            self.callout = Some((Callout::Clean, now));
            if self.clean_run >= CLEAN_PER_STAR {
                self.clean_run = 0;
                if self.stars < MAX_STARS {
                    self.stars += 1;
                    self.callout = Some((Callout::Star, now));
                }
            }
        } else {
            self.callout = None;
        }
        self.hit_foe(hit, false, now);
    }

    fn hit_foe(&mut self, hit: i16, uppercut: bool, now: u32) {
        self.foe_hp = (self.foe_hp - hit).max(0);
        self.stats.damage += hit as u16;
        if self.foe_hp == 0 {
            self.foe_knocked_down(uppercut, now);
        }
    }

    /// Arms a star, if you have one: the opponent is stunned while you send the word. False when
    /// no star can be armed now.
    pub fn star(&mut self, now: u32) -> bool {
        let ready = self.phase == Phase::Fight
            && self.stars > 0
            && self.paused_at.is_none()
            && matches!(self.duel, Duel::Neutral | Duel::Open | Duel::Stagger);
        if !ready {
            return false;
        }
        self.stars -= 1;
        self.word = self.pick_word();
        self.typed.clear();
        self.star_clean = 0;
        self.callout = None;
        self.foe_event = None;
        let time = STAR_START_MS + STAR_CHAR_MS * self.word.len() as u32;
        self.step(Duel::Star, now, Some(now.wrapping_add(time)));
        true
    }

    /// A word of this fighter's characters, your callsign, or a callsign.
    fn pick_word(&mut self) -> Text<MAX_WORD> {
        let set = self.fighter().set();
        let fits = |w: &&&str| w.bytes().all(|b| set.contains(&b));
        let count = WORDS.iter().filter(fits).count() as u32;
        let call = count == 0 || self.rng.below(4) == 0;
        let mut word = Text::new();
        if !call {
            if let Some(w) = WORDS.iter().filter(fits).nth(self.rng.below(count) as usize) {
                for b in w.bytes() {
                    word.push(b);
                }
            }
            return word;
        }
        // Early fighters know too few characters for most calls: send your own.
        let yours = !self.callsign.is_empty() && (self.fighter < 4 || self.rng.below(2) == 0);
        if yours {
            return self.callsign;
        }
        let digit = self.rng.below(10) as u8;
        for b in us_call(digit, &mut self.rng).as_str().bytes().take(MAX_WORD) {
            word.push(b);
        }
        word
    }

    fn star_char(&mut self, c: u8, quality: Quality, now: u32) {
        let at = self.typed.len();
        let Some(&want) = self.word.as_str().as_bytes().get(at) else { return };
        self.typed.push(c);
        if c != want {
            self.me_event = Some((Me::Miss, now));
            self.callout = Some((Callout::Missed, now));
            return self.rest(now);
        }
        if quality != Quality::Bad {
            self.star_clean += 1;
        }
        if self.typed.len() < self.word.len() {
            return;
        }
        let clean = self.star_clean as usize * 100 / self.word.len();
        let full = UPPERCUT_DAMAGE + 2 * self.word.len() as i16;
        let hit = if clean >= 80 { full } else { full * 3 / 4 };
        self.stats.uppercuts += 1;
        self.score += 500;
        self.me_event = Some((Me::Uppercut, now));
        self.foe_event = Some((Foe::Hurt(Side::Left), now));
        self.callout = Some((Callout::Uppercut, now));
        self.rest(now);
        self.hit_foe(hit, true, now);
    }

    fn foe_knocked_down(&mut self, uppercut: bool, now: u32) {
        self.stop = true;
        self.stats.knockdowns += 1;
        self.score += 1000;
        self.round_left = ticks::ms_until(now, self.round_ends);
        self.count = 0;
        self.callout = Some((Callout::Down, now));
        self.clear_texts();
        let kd = self.stats.knockdowns;
        // Each knockdown keeps the opponent down longer. An uppercut that empties the health bar,
        // or the third knockdown, keeps him down for good.
        self.get_up_at = if uppercut || kd >= KNOCKDOWNS_TO_KO { GET_UP_COUNT } else { 2 + 2 * kd };
        self.foe_event = None;
        self.duel = Duel::Neutral;
        self.enter(Phase::FoeDown, now, Some(now.wrapping_add(COUNT_MS)));
    }

    fn foe_count(&mut self, now: u32) {
        self.count += 1;
        if self.count >= GET_UP_COUNT {
            self.score += 5000 + self.round_left / 1000 * 10;
            let how = if self.stats.knockdowns >= KNOCKDOWNS_TO_KO { Outcome::Tko } else { Outcome::Ko };
            return self.finish(true, how, now);
        }
        if self.count < self.get_up_at {
            self.due = Some(now.wrapping_add(COUNT_MS));
            return;
        }
        let share = 60 - 15 * self.stats.knockdowns as i16;
        self.foe_hp = (self.fighter().hp * share / 100).max(10);
        self.foe_event = Some((Foe::Rise, now));
        self.callout = Some((Callout::Up, now));
        self.enter(Phase::Fight, now, None);
        self.round_ends = now.wrapping_add(self.round_left);
        self.rest(now);
    }

    fn knocked_down(&mut self, now: u32) {
        self.stop = true;
        self.stats.knocked += 1;
        self.me_event = None;
        self.round_left = ticks::ms_until(now, self.round_ends);
        self.count = 0;
        self.callout = Some((Callout::Down, now));
        self.clear_texts();
        self.duel = Duel::Neutral;
        if self.stats.knocked >= KNOCKDOWNS_TO_KO {
            return self.finish(false, Outcome::Tko, now);
        }
        self.get_up = self.weakest_known();
        self.enter(Phase::MeDown, now, Some(now.wrapping_add(COUNT_MS)));
    }

    /// The character you copy worst among the ones this fighter sends.
    fn weakest_known(&mut self) -> u8 {
        let n = self.fighter().koch as usize;
        let weights = self.weights;
        let rng = &mut self.rng;
        let worst = (0..n).max_by_key(|&i| (weights[i], rng.below(4))).unwrap_or(0);
        KOCH[worst]
    }

    fn my_count(&mut self, now: u32) {
        self.count += 1;
        if self.count >= GET_UP_COUNT {
            return self.finish(false, Outcome::Ko, now);
        }
        self.due = Some(now.wrapping_add(COUNT_MS));
    }

    fn try_get_up(&mut self, c: u8, now: u32) {
        self.judge(self.get_up, Some(c));
        if c != self.get_up {
            self.callout = Some((Callout::Wrong { sent: c }, now));
            return;
        }
        let share = 70 - 20 * self.stats.knocked as i16;
        self.my_hp = (PLAYER_HP * share / 100).max(15);
        self.me_event = Some((Me::Rise, now));
        self.callout = Some((Callout::Up, now));
        self.enter(Phase::Fight, now, None);
        self.round_ends = now.wrapping_add(self.round_left);
        self.rest(now);
    }

    fn finish(&mut self, won: bool, how: Outcome, now: u32) {
        self.stop = true;
        self.foe_event = None;
        self.me_event = None;
        self.enter(Phase::Over { won, how }, now, None);
    }

    /// One judged character: `copied` is None when it was missed.
    fn judge(&mut self, sent: u8, copied: Option<u8>) {
        self.stats.count(sent, copied == Some(sent));
        // An unknown pattern is a miss, not a mixup.
        let copied = copied.filter(|&c| letters::index(c).is_some());
        self.copies.push((sent, copied));
    }

    /// Takes the characters judged since the last call, for the letter statistics: (sent,
    /// copied), copied None when missed.
    pub fn take_copies(&mut self, mut each: impl FnMut(u8, Option<u8>)) {
        for &(sent, copied) in &self.copies.items[..self.copies.len] {
            each(sent, copied);
        }
        self.copies.len = 0;
    }
}

/// Saved progress: how far up the ladder you are, the hint setting, and your best score and
/// wins against each fighter.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Career {
    /// Fighters you can pick: the first is always open, and each win opens the next.
    pub unlocked: u8,
    pub hints: bool,
    pub best: [u16; FIGHTERS],
    pub wins: [u8; FIGHTERS],
}

impl Default for Career {
    fn default() -> Self {
        Career { unlocked: 1, hints: true, best: [0; FIGHTERS], wins: [0; FIGHTERS] }
    }
}

const MAGIC: [u8; 2] = *b"Rx";
const VERSION: u8 = 1;
/// Body: unlocked, flags, best score per fighter LE, wins per fighter.
const BODY_LEN: usize = 2 + 2 * FIGHTERS + FIGHTERS;
pub const RECORD_LEN: usize = BODY_LEN + record::OVERHEAD;
/// Best scores are stored in tens of points, so a long fight still fits.
const SCORE_UNIT: u32 = 10;

impl Career {
    /// Records a finished fight; true when it opened the next fighter.
    pub fn finished(&mut self, fighter: usize, won: bool, score: u32) -> bool {
        let Some(best) = self.best.get_mut(fighter) else { return false };
        *best = (*best).max((score / SCORE_UNIT).min(u16::MAX as u32) as u16);
        if !won {
            return false;
        }
        self.wins[fighter] = self.wins[fighter].saturating_add(1);
        let opens = fighter + 1 == self.unlocked as usize && fighter + 1 < FIGHTERS;
        if opens {
            self.unlocked += 1;
        }
        opens
    }

    pub fn best_score(&self, fighter: usize) -> u32 {
        self.best[fighter] as u32 * SCORE_UNIT
    }

    /// Every fighter beaten at least once.
    pub fn champion(&self) -> bool {
        self.wins[FIGHTERS - 1] > 0
    }

    pub fn encode(&self) -> [u8; RECORD_LEN] {
        let mut record = [0; RECORD_LEN];
        let body = body_mut(&mut record);
        body[0] = self.unlocked;
        body[1] = self.hints as u8;
        for (k, best) in self.best.iter().enumerate() {
            body[2 + 2 * k..4 + 2 * k].copy_from_slice(&best.to_le_bytes());
        }
        body[2 + 2 * FIGHTERS..].copy_from_slice(&self.wins);
        frame(&mut record, MAGIC, VERSION);
        record
    }

    /// The record at the start of `slot`; None for a blank or foreign record or a bad check.
    pub fn decode(slot: &[u8]) -> Option<Career> {
        let body = unframe(slot, MAGIC, VERSION)?.get(..BODY_LEN)?;
        let career = Career {
            unlocked: body[0],
            hints: body[1] & 1 != 0,
            best: core::array::from_fn(|k| u16::from_le_bytes([body[2 + 2 * k], body[3 + 2 * k]])),
            wins: core::array::from_fn(|k| body[2 + 2 * FIGHTERS + k]),
        };
        (1..=FIGHTERS as u8).contains(&career.unlocked).then_some(career)
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    const DIT: u16 = 60;

    fn fight(fighter: usize) -> Fight {
        Fight::new(fighter, 7, true, DIT, "K5MME")
    }

    /// Runs the fight from the bell to its first tell. Returns the time and the tell.
    fn to_tell(f: &mut Fight, mut now: u32) -> (u32, Text<MAX_TELL>) {
        if f.phase == Phase::Intro {
            assert!(f.start(now));
        }
        for _ in 0..100 {
            if let Some(tell) = f.take_play() {
                return (now, tell);
            }
            now = f.deadline().expect("something is due");
            f.update(now);
        }
        panic!("no tell");
    }

    /// Plays the tell, then keys it back right.
    fn dodge(f: &mut Fight, now: u32) -> u32 {
        let (now, tell) = to_tell(f, now);
        f.tell_played(now + 500);
        for c in tell.as_str().bytes() {
            f.key(c, Quality::Good, now + 700);
        }
        assert_eq!(f.me_pose(now + 700).0, Me::Dodge(f.side.other()));
        let now = f.deadline().unwrap();
        f.update(now);
        assert!(f.open());
        now
    }

    #[test]
    fn dits_jab_dahs_hook_and_mixes_combo() {
        for c in *b"EISH5" {
            assert_eq!(punch_of(c), Some(Punch::Jab), "{}", c as char);
        }
        for c in *b"TMO0" {
            assert_eq!(punch_of(c), Some(Punch::Hook), "{}", c as char);
        }
        assert_eq!(punch_of(b'A'), Some(Punch::Combo(2)));
        assert_eq!(punch_of(b'K'), Some(Punch::Combo(3)));
        assert_eq!(punch_of(b'C'), Some(Punch::Combo(4)));
        assert_eq!(punch_of(b'#'), None);
    }

    #[test]
    fn clean_timing_and_combos_hit_harder() {
        assert!(damage(b'H', Quality::Good) > damage(b'H', Quality::Off));
        assert!(damage(b'H', Quality::Off) > damage(b'H', Quality::Bad));
        assert!(damage(b'R', Quality::Good) > damage(b'S', Quality::Good));
        assert!(damage(b'M', Quality::Good) > damage(b'I', Quality::Good));
        assert!(damage(b'E', Quality::Bad) > 0);
    }

    #[test]
    fn the_ladder_adds_koch_characters_and_speed() {
        let mut all = Vec::new();
        for (i, f) in LADDER.iter().enumerate() {
            all.extend_from_slice(f.new_chars(i));
            if i > 0 {
                let before = &LADDER[i - 1];
                assert!(f.koch > before.koch && f.speed_bump >= before.speed_bump && f.hp > before.hp);
                assert!(f.react_ms < before.react_ms && f.spacing_pct >= before.spacing_pct);
                assert!(!f.hint || before.hint, "hints only early on");
            }
        }
        assert_eq!(&all[..], b"KMURESNAPTLWI.JZ=FOY,VG5/Q92H38B?47C1D60X");
        assert!(LADDER[0].hint && !LADDER[FIGHTERS - 1].hint);
        assert!(LADDER[0].max_tell == 1 && LADDER[FIGHTERS - 1].max_tell as usize == MAX_TELL);
        assert!(LADDER[FIGHTERS - 1].noise > 0 && LADDER[FIGHTERS - 1].fading > 0);
        assert_eq!(LADDER[0].speeds(20, 0), (20, 8));
        assert_eq!(LADDER[FIGHTERS - 1].speeds(20, 0), (26, 26));
        assert_eq!(LADDER[FIGHTERS - 1].speeds(20, 15), (26, 15));
    }

    #[test]
    fn tells_use_only_the_fighters_characters() {
        for (k, fighter) in LADDER.iter().enumerate() {
            let mut f = fight(k);
            let set = fighter.set();
            for _ in 0..200 {
                let tell = f.pick_tell();
                assert!((1..=fighter.max_tell as usize).contains(&tell.len()));
                assert!(tell.as_str().bytes().all(|c| set.contains(&c)), "{}", tell.as_str());
            }
        }
    }

    #[test]
    fn tells_lean_on_weak_characters() {
        let mut letters = Letters::default();
        for _ in 0..20 {
            letters.record(b'U', Some(b'U'));
            letters.record(b'K', Some(b'K'));
            letters.record(b'M', Some(b'M'));
            letters.record(b'E', Some(b'E'));
            letters.record(b'R', Some(b'K'));
        }
        let mut f = fight(0);
        f.learn(&letters);
        let picks: Vec<u8> = (0..2000).map(|_| f.pick_char()).collect();
        let count = |c: u8| picks.iter().filter(|&&p| p == c).count();
        assert!(count(b'R') > 3 * count(b'U'), "R {} U {}", count(b'R'), count(b'U'));
    }

    #[test]
    fn keying_the_tell_back_dodges_and_opens_the_opponent() {
        let mut f = fight(0);
        let now = dodge(&mut f, 1000);
        assert_eq!(f.stats.dodged, 1);
        assert_eq!(f.foe_pose(now).0, Foe::Open);
        let hp = f.foe_hp;
        f.key(b'E', Quality::Good, now + 100);
        assert_eq!(f.foe_hp, hp - damage(b'E', Quality::Good));
        assert_eq!(f.me_pose(now + 100).0, Me::Jab(Side::Right));
        assert!(matches!(f.foe_pose(now + 100).0, Foe::Hurt(_)));
        f.key(b'K', Quality::Off, now + 900);
        assert_eq!(f.me_pose(now + 900).0, Me::Combo(3));
        assert_eq!((f.stats.punches, f.stats.clean, f.clean_run), (2, 1, 1));
        // The window closes and the next attack comes.
        let close = f.deadline().unwrap();
        f.update(close);
        assert!(!f.open());
        f.key(b'T', Quality::Good, close + 10);
        assert_eq!(f.foe_hp, hp - damage(b'E', Quality::Good) - damage(b'K', Quality::Off), "blocked");
        assert_eq!(f.foe_pose(close + 10).0, Foe::Block);
    }

    #[test]
    fn a_wrong_character_takes_the_punch_at_once() {
        let mut f = fight(0);
        let (now, tell) = to_tell(&mut f, 1000);
        let want = tell.as_str().as_bytes()[0];
        let wrong = if want == b'E' { b'T' } else { b'E' };
        f.key(wrong, Quality::Good, now + 300);
        assert!(f.stop && f.tell_shown);
        assert_eq!(f.callout.map(|c| c.0), Some(Callout::Wrong { sent: wrong }));
        assert_eq!(f.foe_pose(now + 300).0, Foe::Punch(f.side));
        let lands = f.deadline().unwrap();
        f.update(lands);
        assert_eq!(f.my_hp, PLAYER_HP - LADDER[0].power);
        assert_eq!(f.me_pose(lands).0, Me::Hurt);
        let mut copies = Vec::new();
        f.take_copies(|s, c| copies.push((s, c)));
        assert_eq!(copies, [(want, Some(wrong))]);
    }

    #[test]
    fn too_slow_takes_the_punch_and_counts_a_miss() {
        let mut f = fight(0);
        let (now, tell) = to_tell(&mut f, 1000);
        f.tell_played(now + 400);
        let window = f.deadline().unwrap() - (now + 400);
        let keying = (units(tell.as_str().as_bytes()[0]) + 3) * DIT as u32;
        assert_eq!(window, LADDER[0].react_ms + keying);
        let late = f.deadline().unwrap();
        f.update(late);
        assert_eq!(f.callout.map(|c| c.0), Some(Callout::TooSlow));
        f.update(f.deadline().unwrap());
        assert!(f.my_hp < PLAYER_HP);
        let mut copies = Vec::new();
        f.take_copies(|s, c| copies.push((s, c)));
        assert_eq!(copies, [(tell.as_str().as_bytes()[0], None)]);
    }

    #[test]
    fn clean_counters_earn_a_star_and_a_word_lands_an_uppercut() {
        let mut f = fight(1);
        let now = dodge(&mut f, 1000);
        for k in 0..CLEAN_PER_STAR as u32 {
            f.key(b'S', Quality::Good, now + 100 * k);
        }
        assert_eq!((f.stars, f.callout.map(|c| c.0)), (1, Some(Callout::Star)));
        assert!(f.star(now + 400));
        assert!(f.starring() && !f.word.is_empty());
        assert_eq!(f.foe_pose(now + 400).0, Foe::Stunned);
        let hp = f.foe_hp;
        let word = f.word;
        for c in word.as_str().bytes() {
            f.key(c, Quality::Good, now + 500);
        }
        assert_eq!(f.stats.uppercuts, 1);
        assert_eq!(f.me_pose(now + 500).0, Me::Uppercut);
        assert!(hp - f.foe_hp >= UPPERCUT_DAMAGE || f.phase == Phase::FoeDown);
        assert!(!f.star(now + 600), "no stars left");
    }

    #[test]
    fn a_wrong_character_wastes_the_star() {
        let mut f = fight(1);
        let now = dodge(&mut f, 1000);
        f.stars = 1;
        assert!(f.star(now));
        let hp = f.foe_hp;
        f.key(b'#', Quality::Good, now + 100);
        assert_eq!((f.stars, f.foe_hp, f.starring()), (0, hp, false));
    }

    #[test]
    fn uppercut_words_use_known_characters_or_a_callsign() {
        let mut f = fight(2);
        let set = LADDER[2].set();
        for _ in 0..100 {
            let w = f.pick_word();
            assert!(!w.is_empty());
            assert!(w.as_str() == "K5MME" || w.as_str().bytes().all(|b| set.contains(&b)), "{}", w.as_str());
        }
    }

    #[test]
    fn knockdowns_count_and_the_third_one_ends_it() {
        let mut f = fight(0);
        let mut now = 1000;
        for kd in 1..=KNOCKDOWNS_TO_KO {
            now = dodge(&mut f, now);
            f.foe_hp = 1;
            f.key(b'E', Quality::Good, now + 50);
            assert_eq!((f.phase, f.stats.knockdowns), (Phase::FoeDown, kd));
            assert_eq!(f.foe_pose(now + 50).0, if kd < KNOCKDOWNS_TO_KO { Foe::Down } else { Foe::Out });
            while f.phase == Phase::FoeDown {
                now = f.deadline().unwrap();
                f.update(now);
            }
            if kd < KNOCKDOWNS_TO_KO {
                assert_eq!(f.count, 2 + 2 * kd);
                assert_eq!(f.foe_pose(now).0, Foe::Rise);
                assert!(f.foe_hp > 0);
            }
        }
        assert_eq!(f.phase, Phase::Over { won: true, how: Outcome::Tko });
        assert_eq!(f.foe_pose(now).0, Foe::Out);
        assert_eq!(f.me_pose(now).0, Me::Victory);
    }

    #[test]
    fn an_uppercut_that_empties_the_bar_is_a_ko() {
        let mut f = fight(0);
        let mut now = dodge(&mut f, 1000);
        f.foe_hp = 5;
        f.stars = 1;
        assert!(f.star(now));
        let word = f.word;
        for c in word.as_str().bytes() {
            f.key(c, Quality::Good, now + 10);
        }
        assert_eq!(f.foe_pose(now + 10).0, Foe::Out);
        while f.phase == Phase::FoeDown {
            now = f.deadline().unwrap();
            f.update(now);
        }
        assert_eq!((f.phase, f.count), (Phase::Over { won: true, how: Outcome::Ko }, 10));
    }

    #[test]
    fn knocked_down_you_key_a_character_to_get_up() {
        let mut f = fight(0);
        f.my_hp = 1;
        let (now, tell) = to_tell(&mut f, 1000);
        let want = tell.as_str().as_bytes()[0];
        f.key(if want == b'E' { b'T' } else { b'E' }, Quality::Good, now + 10);
        let lands = f.deadline().unwrap();
        f.update(lands);
        assert_eq!((f.phase, f.my_hp), (Phase::MeDown, 0));
        assert_eq!(f.me_pose(lands).0, Me::Down);
        assert!(LADDER[0].set().contains(&f.get_up));
        let wrong = if f.get_up == b'E' { b'T' } else { b'E' };
        f.update(lands + COUNT_MS);
        f.key(wrong, Quality::Good, lands + COUNT_MS + 10);
        assert_eq!((f.phase, f.count), (Phase::MeDown, 1));
        f.key(f.get_up, Quality::Bad, lands + COUNT_MS + 20);
        assert_eq!(f.phase, Phase::Fight);
        assert!(f.my_hp > 0);
        assert_eq!(f.me_pose(lands + COUNT_MS + 20).0, Me::Rise);
    }

    #[test]
    fn staying_down_loses_by_ko() {
        let mut f = fight(0);
        f.my_hp = 1;
        let (now, _) = to_tell(&mut f, 1000);
        f.tell_played(now);
        while f.phase == Phase::Fight {
            f.update(f.deadline().unwrap());
        }
        while f.phase == Phase::MeDown {
            f.update(f.deadline().unwrap());
        }
        assert_eq!(f.phase, Phase::Over { won: false, how: Outcome::Ko });
    }

    #[test]
    fn the_bell_ends_rounds_and_the_judges_decide() {
        let mut f = fight(0);
        let now = dodge(&mut f, 1000);
        f.key(b'S', Quality::Good, now);
        let bell = now + ROUND_MS;
        f.update(bell);
        assert_eq!(f.phase, Phase::RoundEnd);
        assert!(f.stop);
        assert!(!f.start(bell), "the corner needs its time");
        f.update(f.deadline().unwrap());
        for round in 2..=ROUNDS {
            assert!(f.start(bell));
            assert_eq!(f.round, round);
            f.update(bell + ROUND_BANNER_MS);
            assert_eq!(f.clock_ms(bell + ROUND_BANNER_MS), ROUND_MS);
            f.update(bell + ROUND_BANNER_MS + ROUND_MS);
            if round < ROUNDS {
                f.update(f.deadline().unwrap());
            }
        }
        assert_eq!(f.phase, Phase::Over { won: true, how: Outcome::Decision });
    }

    #[test]
    fn pause_stops_every_clock() {
        let mut f = fight(0);
        let (now, _) = to_tell(&mut f, 1000);
        f.tell_played(now);
        let due = f.deadline().unwrap();
        let clock = f.clock_ms(now);
        f.pause(now);
        assert_eq!(f.deadline(), None);
        f.update(due + 50_000);
        assert!(f.telling());
        f.resume(now + 60_000);
        assert_eq!(f.deadline(), Some(due + 60_000));
        assert_eq!(f.clock_ms(now + 60_000), clock);
    }

    #[test]
    fn pausing_mid_tell_takes_it_back() {
        let mut f = fight(0);
        let (now, _) = to_tell(&mut f, 1000);
        f.pause(now + 100);
        assert!(!f.telling() && f.tell.is_empty());
        assert_eq!(f.stats.tells, 0);
        f.resume(now + 5000);
        f.tell_played(now + 5100);
        assert!(!f.telling(), "the old tell's end does nothing");
        let (_, tell) = to_tell(&mut f, now + 5200);
        assert!(!tell.is_empty());
    }

    #[test]
    fn fight_stats_list_the_worst_characters_first() {
        let mut s = FightStats::default();
        s.count(b'K', true);
        s.count(b'K', true);
        s.count(b'M', false);
        s.count(b'R', true);
        s.count(b'R', false);
        let worst: Vec<u8> = s.worst().map(|w| w.0).collect();
        assert_eq!(worst, [b'M', b'R', b'K']);
    }

    #[test]
    fn career_opens_fighters_and_round_trips() {
        let mut c = Career::default();
        assert!(!c.finished(0, false, 1234));
        assert_eq!((c.unlocked, c.best_score(0)), (1, 1230));
        assert!(c.finished(0, true, 900));
        assert_eq!((c.unlocked, c.wins[0], c.best_score(0)), (2, 1, 1230));
        assert!(!c.finished(0, true, 99_999), "an old fighter opens nothing");
        assert_eq!(c.best_score(0), 99_990);
        c.hints = false;
        assert_eq!(Career::decode(&c.encode()), Some(c));
        assert_eq!(Career::decode(&[0xFF; RECORD_LEN]), None);
        let mut champ = Career { unlocked: FIGHTERS as u8, ..Career::default() };
        assert!(!champ.finished(FIGHTERS - 1, true, 10) && champ.champion());
    }
}
