//! Brass Knuckles Knock Out: the title screen and fighter select, the fighter's intro card, the
//! fight, and the results. The rules live in `pocketcw_logic::boxing`; the ring is `ring`.
//!
//! Opening the game plays the startup screen: the brass knuckles logo, BRASS KNUCKLES GANG and
//! the motto, with impact stars going off all over. Each fight is on one of four stages, picked
//! at random.
//!
//! The fight fills the whole panel: a scoreboard across the top, the tell on the left, your
//! keying on the right, and the ring between and below them. Every part redraws only when what
//! it shows changes, about 25 times a second while the fight runs.

use embedded_graphics::pixelcolor::{Rgb565, RgbColor};
use pocketcw_logic::{
    boxing::{
        CLEAN_PER_STAR, Callout, FIGHTERS, Fight, LADDER, MAX_STARS, Outcome, PLAYER_HP, Phase, Punch, ROUNDS, punch_of,
    },
    decoder::Decoded,
    input::Action,
    keyer,
    morse,
    player::{Keying, MAX_TEXT},
    settings::Settings,
    synth::{MAX_LEVEL, Sound},
    text::{Text, wrap},
    ticks,
    timing::{Interval, Kind, Quality},
};

use super::ring::{self, Banner, Fighters, STAGES, Scene, Splash};
use super::settings::Section;
use super::{Play, Screen, Ui};
use crate::gfx::{CHAR_W, TINY_W, blocks, blocks_width, mix, rect, rgb, text, tiny};
use crate::lcd::{HEIGHT, Lcd, WIDTH};
use crate::theme::{BAD, GOOD, MAIN, WARN, quality_color, underline};

/// How often the fight redraws.
const FRAME_MS: u32 = 40;
/// Title and results only blink.
const IDLE_FRAME_MS: u32 = 250;
/// How long the result shows in the ring before the results screen.
const OVER_MS: u32 = 3200;
const TELL_DELAY_MS: u32 = 150;
/// The startup screen moves on by itself after this long.
const SPLASH_MS: u32 = 7000;

// The fight's layout, in panel pixels.
const HUD_H: i32 = 2 * ring::STAGE_Y;
const PANEL_W: i32 = 2 * ring::WINDOW.0;
const RIGHT_X: i32 = 2 * ring::WINDOW.1;
const PANEL_TOP: i32 = HUD_H;
const PANEL_BOTTOM: i32 = 2 * ring::PANEL_BOTTOM;
const BAR_W: i32 = 112;
const BANNER_Y: i32 = 62;

// The game's own colors, the same in every theme: it has its own look.
const HUD_BG: Rgb565 = rgb(0x120E1C);
const HUD_EDGE: Rgb565 = rgb(0xE8B020);
const PANEL_BG: Rgb565 = rgb(0x0E0C16);
const PANEL_EDGE: Rgb565 = rgb(0x4A3C5A);
const LABEL: Rgb565 = rgb(0x9A8CB0);
const BRASS: Rgb565 = rgb(0xE8B020);
const RED: Rgb565 = rgb(0xE83A3A);
const PALE: Rgb565 = rgb(0xF4EEE0);
const DIM: Rgb565 = rgb(0x5A4E6A);
const BAR_EMPTY: Rgb565 = rgb(0x2A2238);

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Splash,
    Title,
    Card,
    Fight,
    Results,
}

/// What the scoreboard and panels show, to redraw only what changed.
#[derive(Clone, Copy, PartialEq, Default)]
struct Shown {
    my_hp: i16,
    foe_hp: i16,
    stars: u8,
    clean_run: u8,
    score: u32,
    clock_s: u32,
    round: u8,
    tell: TellPanel,
    you: YouPanel,
    window_px: i32,
}

#[derive(Clone, Copy, PartialEq, Default)]
struct TellPanel {
    mode: u8,
    text: Text<12>,
    typed: Text<12>,
    shown: bool,
    sounding: bool,
    count: u8,
    resolved: Option<bool>,
    hints: bool,
}

#[derive(Clone, Copy, PartialEq, Default)]
struct YouPanel {
    marks: Text<10>,
    last: Option<(u8, Quality)>,
    /// What your last character did: JAB, HOOK, COMBO, or nothing for a tell's answer.
    label: &'static str,
    callout: Option<Callout>,
    note: &'static str,
    paused: bool,
}

pub(super) struct Boxing {
    view: View,
    /// The startup screen: when it began, its stars' seed, what it shows, and which of its
    /// parts at full resolution are drawn.
    splash_at: u32,
    splash_seed: u32,
    splash: Splash,
    motto: bool,
    prompt: Option<bool>,
    /// The stage of this fight, in `ring::STAGES`.
    stage: usize,
    /// The fighter picked on the title screen.
    pick: usize,
    fight: Option<Fight>,
    scene: Scene,
    shown: Shown,
    /// The scoreboard and panels have been drawn once since the last full redraw.
    drawn: bool,
    /// Dots and dashes of the character you are keying now, and the worst timing in them.
    marks: Text<10>,
    worst: Quality,
    /// Your last character, with its timing, and what it did.
    last: Option<(u8, Quality)>,
    label: &'static str,
    /// A short message on the YOU panel, and when it started.
    note: Option<(&'static str, u32)>,
    /// The tick count, as of the newest event, and of the last key edge.
    now: u32,
    edge: u32,
    next_frame: u32,
    over_at: Option<u32>,
    /// The results screen: the fight opened the next fighter, and set a new best.
    opened: bool,
    best: bool,
    blink: bool,
}

impl Boxing {
    fn new(pick: usize, now: u32, seed: u32) -> Boxing {
        Boxing {
            view: View::Splash,
            splash_at: now,
            splash_seed: seed,
            splash: Splash::empty(),
            motto: false,
            prompt: None,
            stage: seed as usize % STAGES.len(),
            pick,
            fight: None,
            scene: Scene::empty(),
            shown: Shown::default(),
            drawn: false,
            marks: Text::new(),
            worst: Quality::Good,
            last: None,
            label: "",
            note: None,
            now,
            edge: now,
            next_frame: now,
            over_at: None,
            opened: false,
            best: false,
            blink: false,
        }
    }

    /// Every view draws the whole panel in the game's own look.
    pub(super) fn full_screen(&self) -> bool {
        true
    }

    /// The fight is on screen and running: noise plays, frames come fast.
    pub(super) fn fighting(&self) -> bool {
        self.view == View::Fight && self.fight.as_ref().is_some_and(|f| !f.paused())
    }

    pub(super) fn deadline(&self) -> Option<u32> {
        let frame = Some(self.next_frame);
        let fight = self.fight.as_ref().filter(|_| self.view == View::Fight).and_then(Fight::deadline);
        [frame, fight].into_iter().flatten().reduce(ticks::earlier)
    }
}

impl Ui {
    fn boxing(&mut self) -> Option<&mut Boxing> {
        self.boxing.as_mut()
    }

    /// Leaving the game for `to`. A fight waits, paused, for your return; anything else starts
    /// over at the startup screen next time.
    pub(super) fn boxing_leave(&mut self, to: Screen) {
        let Some(b) = self.boxing() else { return };
        let now = b.now;
        let keep = match (&mut b.fight, b.view) {
            (Some(f), View::Fight) if !matches!(f.phase, Phase::Over { .. }) => {
                if !f.paused() {
                    f.pause(now);
                }
                true
            }
            _ => matches!(to, Screen::Settings | Screen::Letters),
        };
        if !keep {
            self.boxing = None;
        }
    }

    /// The sound while the fight runs: the opponent's static and fading on top of yours.
    pub(super) fn boxing_sound(&self, s: &Settings) -> Option<Sound> {
        let b = self.boxing.as_ref()?;
        let f = b.fight.as_ref().filter(|_| b.view == View::Fight)?.fighter();
        let mut sound = s.sound();
        sound.noise = sound.noise.max(f.noise).min(MAX_LEVEL);
        sound.fading = sound.fading.max(f.fading).min(MAX_LEVEL);
        Some(sound)
    }

    pub(super) fn draw_boxing(&mut self, lcd: &mut Lcd, s: &Settings) {
        if self.boxing.is_none() {
            let pick = self.career.unlocked.saturating_sub(1) as usize;
            let seed = self.next_seed();
            self.boxing = Some(Boxing::new(pick, self.now_ms, seed));
        }
        let dit = keyer::dit_ms(s.wpm);
        let Some(b) = self.boxing() else { return };
        // Back from the settings, the fight takes the new keyer speed.
        if let Some(f) = &mut b.fight {
            f.set_dit_ms(dit);
        }
        match b.view {
            View::Splash => self.draw_splash(lcd),
            View::Title => self.draw_title(lcd, s),
            View::Card => self.draw_card(lcd, s),
            View::Fight => self.draw_fight(lcd),
            View::Results => self.draw_results(lcd),
        }
    }

    // --- Title and fighter select ---------------------------------------------------------

    fn draw_title(&mut self, lcd: &mut Lcd, s: &Settings) {
        rect(lcd, 0, 0, WIDTH as i32, HEIGHT as i32, HUD_BG);
        self.softkeys(lcd, ["SET", "HINT", "LTRS", "BACK"]);
        tiny(lcd, MAIN + 6, 6, "BKG", BRASS, HUD_BG);
        // The logo: brass over red, with a dark drop shadow.
        let brass_w = blocks_width(14, 3, 2);
        let ko_w = blocks_width(9, 5, 3);
        for (dx, color) in [(2, rgb(0x050308)), (0, BRASS)] {
            blocks(lcd, (MAIN - brass_w) / 2 + dx, 8 + dx, "BRASS KNUCKLES", 3, 2, 4, color, None);
        }
        for (dx, color) in [(3, rgb(0x050308)), (0, RED)] {
            blocks(lcd, (MAIN - ko_w) / 2 + dx, 36 + dx, "KNOCK OUT", 5, 3, 5, color, None);
        }
        rect(lcd, 10, 78, MAIN - 20, 1, PANEL_EDGE);
        self.draw_pick(lcd, s);
    }

    fn draw_pick(&mut self, lcd: &mut Lcd, s: &Settings) {
        let career = self.career;
        let Some(b) = self.boxing() else { return };
        let k = b.pick;
        let locked = k >= career.unlocked as usize;
        let f = &LADDER[k];
        let (top, h) = (84, 128);
        rect(lcd, 0, top, MAIN, h, HUD_BG);
        // The portrait in its spotlight, with arrows for the dial.
        let (px, pw) = (8, 92);
        rect(lcd, px, top + 2, pw, h - 6, PANEL_BG);
        rect(lcd, px, top + 2, pw, 1, PANEL_EDGE);
        rect(lcd, px, top + h - 5, pw, 1, PANEL_EDGE);
        ring::portrait(lcd, k, 0, px + pw / 2, top + 88, PANEL_BG, locked);
        if k > 0 {
            text(lcd, px + 2, top + 40, "<", LABEL, PANEL_BG);
        }
        if k + 1 < FIGHTERS {
            text(lcd, px + pw - CHAR_W - 2, top + 40, ">", LABEL, PANEL_BG);
        }
        let number = Text::<8>::format(format_args!("{} OF {}", k + 1, FIGHTERS));
        tiny(lcd, px + (pw - number.len() as i32 * TINY_W) / 2, top + 96, number.as_str(), LABEL, PANEL_BG);
        let x = px + pw + 10;
        let cols = ((MAIN - x - 4) / TINY_W) as usize;
        if locked {
            text(lcd, x, top + 6, "? ? ?", DIM, HUD_BG);
            let line = Text::<40>::format(format_args!("BEAT {} TO", LADDER[k - 1].name));
            let mut y = top + 30;
            for part in wrap(line.as_str(), cols).chain(["UNLOCK"]) {
                tiny(lcd, x, y, part, LABEL, HUD_BG);
                y += 13;
            }
        } else {
            let name_fits = f.name.len() as i32 * CHAR_W <= MAIN - x - 4;
            if name_fits {
                text(lcd, x, top + 4, f.name, PALE, HUD_BG);
            } else {
                tiny(lcd, x, top + 6, f.name, PALE, HUD_BG);
            }
            let mut y = top + 24;
            for part in wrap(f.from, cols) {
                tiny(lcd, x, y, part, LABEL, HUD_BG);
                y += 12;
            }
            y += 4;
            let (wpm, effective) = self.tell_speeds(s, k);
            let speed = if effective < wpm {
                Text::<24>::format(format_args!("{wpm} WPM, SPACED {effective}"))
            } else {
                Text::format(format_args!("{wpm} WPM"))
            };
            tiny(lcd, x, y, speed.as_str(), PALE, HUD_BG);
            y += 13;
            let chars = Text::<24>::format(format_args!("{} CHARACTERS", f.koch));
            tiny(lcd, x, y, chars.as_str(), PALE, HUD_BG);
            y += 13;
            let record = if career.wins[k] > 0 {
                Text::<24>::format(format_args!("WON {}  BEST {}", career.wins[k], career.best_score(k)))
            } else if career.best[k] > 0 {
                Text::format(format_args!("BEST {}", career.best_score(k)))
            } else {
                Text::format(format_args!("NOT FOUGHT YET"))
            };
            tiny(lcd, x, y, record.as_str(), if career.wins[k] > 0 { GOOD } else { LABEL }, HUD_BG);
        }
        self.draw_title_foot(lcd);
    }

    fn draw_title_foot(&mut self, lcd: &mut Lcd) {
        let hints = self.career.hints;
        let champ = self.career.champion();
        let Some(b) = self.boxing() else { return };
        let note = b.note.filter(|n| !ticks::reached(b.now, n.1.wrapping_add(2000))).map(|n| n.0);
        let blink = b.blink;
        let y = 216;
        rect(lcd, 0, y, MAIN, HEIGHT as i32 - y, HUD_BG);
        match note {
            Some(note) => tiny(lcd, 10, y + 4, note, WARN, HUD_BG),
            None if !blink => tiny(lcd, 10, y + 4, "CLICK TO FIGHT", BRASS, HUD_BG),
            None => {}
        }
        if note.is_some() {
            return;
        }
        let champ = if champ { "CHAMP  " } else { "" };
        let flags = Text::<24>::format(format_args!("{champ}HINTS {}", if hints { "ON" } else { "OFF" }));
        tiny(lcd, MAIN - 8 - flags.len() as i32 * TINY_W, y + 4, flags.as_str(), LABEL, HUD_BG);
    }

    fn title_act(&mut self, lcd: &mut Lcd, s: &mut Settings, action: Action) {
        let unlocked = self.career.unlocked as usize;
        let Some(b) = self.boxing() else { return };
        match action {
            Action::Turn(d) => {
                let next = (b.pick as i32 + d as i32).clamp(0, FIGHTERS as i32 - 1) as usize;
                if next != b.pick {
                    b.pick = next;
                    self.draw_pick(lcd, s);
                }
            }
            Action::Click if b.pick < unlocked => {
                let pick = b.pick;
                self.new_fight(pick, s);
                self.draw(lcd, s);
            }
            Action::Click => {
                b.note = Some(("LOCKED: WIN THE ONE BEFORE", b.now));
                self.draw_title_foot(lcd);
            }
            Action::Soft(0) => self.open_settings_at(lcd, s, Section::Keyer),
            Action::Soft(1) => {
                self.career.hints = !self.career.hints;
                self.draw_title_foot(lcd);
            }
            Action::Soft(2) => self.open(lcd, s, Screen::Letters),
            Action::Soft(3) => self.open(lcd, s, Screen::Menu),
            _ => {}
        }
    }

    // --- Intro card ------------------------------------------------------------------------

    fn new_fight(&mut self, k: usize, s: &Settings) {
        let seed = self.next_seed();
        let hints = self.career.hints;
        let mut fight = Fight::new(k, seed, hints, keyer::dit_ms(s.wpm), s.callsign());
        fight.learn(&self.letters);
        let roll = self.next_seed() >> 16;
        let Some(b) = self.boxing() else { return };
        // A different stage each fight, at random among the others.
        b.stage = (b.stage + 1 + roll as usize % (STAGES.len() - 1)) % STAGES.len();
        b.fight = Some(fight);
        b.view = View::Card;
        b.pick = k;
        b.over_at = None;
    }

    fn tell_speeds(&self, s: &Settings, k: usize) -> (u8, u8) {
        let (base, _) = s.receive_speed(s.wpm);
        LADDER[k].speeds(base, s.rx_effective)
    }

    fn draw_card(&mut self, lcd: &mut Lcd, s: &Settings) {
        let Some((k, stage)) = self.boxing().map(|b| (b.pick, b.stage)) else { return };
        let f = &LADDER[k];
        rect(lcd, 0, 0, WIDTH as i32, HEIGHT as i32, HUD_BG);
        self.softkeys(lcd, ["SET", "HINT", "", "BACK"]);
        let stage = Text::<24>::format(format_args!("AT {}", STAGES[stage].name));
        tiny(lcd, MAIN - 6 - stage.len() as i32 * TINY_W, 6, stage.as_str(), BRASS, HUD_BG);
        // "VS", the portrait, and the name in brass.
        let label = Text::<16>::format(format_args!("FIGHT {} OF {}", k + 1, FIGHTERS));
        tiny(lcd, 10, 6, label.as_str(), LABEL, HUD_BG);
        rect(lcd, 8, 22, 100, 100, PANEL_BG);
        rect(lcd, 8, 22, 100, 1, BRASS);
        rect(lcd, 8, 121, 100, 1, BRASS);
        ring::portrait(lcd, k, 1, 58, 112, PANEL_BG, false);
        let x = 118;
        let cols = ((MAIN - x - 4) / TINY_W) as usize;
        let mut y = 24;
        for word in f.name.split(' ') {
            if blocks_width(word.len(), 3, 2) <= MAIN - x - 4 {
                blocks(lcd, x + 2, y + 2, word, 3, 2, 4, rgb(0x050308), None);
                blocks(lcd, x, y, word, 3, 2, 4, BRASS, None);
            } else {
                blocks(lcd, x, y, word, 2, 2, 4, BRASS, None);
            }
            y += 26;
        }
        y += 2;
        for part in wrap(f.from, cols) {
            tiny(lcd, x, y, part, LABEL, HUD_BG);
            y += 12;
        }
        let quote = Text::<40>::format(format_args!("\"{}\"", f.blurb));
        y = y.max(128);
        for part in wrap(quote.as_str(), 37) {
            tiny(lcd, 10, y, part, PALE, HUD_BG);
            y += 12;
        }
        y += 4;
        // What this fighter brings: new characters, speed, the hint, the band.
        tiny(lcd, 10, y, "NEW", LABEL, HUD_BG);
        let mut cx = 44;
        for &c in f.new_chars(k) {
            cx += super::draw_char(lcd, &crate::fonts::SMALL, cx, y - 3, c, BRASS, HUD_BG) + 4;
        }
        y += 18;
        let (wpm, effective) = self.tell_speeds(s, k);
        let hint = f.hint && self.career.hints;
        let mut line = Text::<48>::format(format_args!("{wpm} WPM"));
        if effective < wpm {
            line = Text::format(format_args!("{wpm} WPM SPACED TO {effective}"));
        }
        tiny(lcd, 10, y, line.as_str(), PALE, HUD_BG);
        y += 13;
        let tells = if f.max_tell > 1 {
            Text::<24>::format(format_args!("TELLS UP TO {} LONG", f.max_tell))
        } else {
            Text::format(format_args!("ONE-CHARACTER TELLS"))
        };
        tiny(lcd, 10, y, tells.as_str(), PALE, HUD_BG);
        y += 13;
        let band = match (f.noise, f.fading) {
            (0, _) => "CLEAN BAND",
            (_, 0) => "STATIC",
            _ => "STATIC AND FADING",
        };
        tiny(lcd, 10, y, if hint { "HINTS SHOWN" } else { "AUDIO ONLY" }, if hint { GOOD } else { WARN }, HUD_BG);
        tiny(lcd, 110, y, band, if f.noise > 0 { WARN } else { GOOD }, HUD_BG);
        self.draw_card_prompt(lcd);
    }

    fn draw_card_prompt(&mut self, lcd: &mut Lcd) {
        let blink = self.boxing().is_some_and(|b| b.blink);
        let y = 224;
        rect(lcd, 0, y - 2, MAIN, 16, HUD_BG);
        if !blink {
            let line = "CLICK: RING THE BELL";
            tiny(lcd, (MAIN - line.len() as i32 * TINY_W) / 2, y, line, BRASS, HUD_BG);
        }
    }

    fn card_act(&mut self, lcd: &mut Lcd, s: &mut Settings, action: Action) {
        let now = self.boxing().map_or(0, |b| b.now);
        match action {
            Action::Click => {
                if let Some(b) = self.boxing()
                    && let Some(f) = &mut b.fight
                {
                    f.start(now);
                    b.view = View::Fight;
                    b.drawn = false;
                    b.marks.clear();
                    b.last = None;
                }
                self.draw(lcd, s);
            }
            Action::Soft(0) => self.open_settings_at(lcd, s, Section::Keyer),
            Action::Soft(1) => {
                self.career.hints = !self.career.hints;
                let hints = self.career.hints;
                if let Some(f) = self.boxing().and_then(|b| b.fight.as_mut()) {
                    f.set_hints(hints);
                }
                self.draw(lcd, s);
            }
            Action::Soft(3) => self.boxing_title(lcd, s),
            _ => {}
        }
    }

    fn boxing_title(&mut self, lcd: &mut Lcd, s: &Settings) {
        self.stop_players();
        if let Some(b) = self.boxing() {
            b.view = View::Title;
            b.fight = None;
        }
        self.draw(lcd, s);
    }

    // --- The fight -------------------------------------------------------------------------

    fn draw_fight(&mut self, lcd: &mut Lcd) {
        rect(lcd, 0, 0, WIDTH as i32, HUD_H, HUD_BG);
        rect(lcd, 0, HUD_H - 2, WIDTH as i32, 1, HUD_EDGE);
        rect(lcd, 0, HUD_H - 1, WIDTH as i32, 1, rgb(0x050308));
        for x in [0, RIGHT_X] {
            rect(lcd, x, PANEL_TOP, PANEL_W, PANEL_BOTTOM - PANEL_TOP, PANEL_BG);
            rect(lcd, x + if x == 0 { PANEL_W - 1 } else { 0 }, PANEL_TOP, 1, PANEL_BOTTOM - PANEL_TOP, PANEL_EDGE);
            rect(lcd, x, PANEL_BOTTOM - 1, PANEL_W, 1, PANEL_EDGE);
        }
        tiny(lcd, 4, 2, "YOU", LABEL, HUD_BG);
        let Some(b) = self.boxing() else { return };
        // Values nothing shows, so every part draws.
        b.shown = Shown {
            round: u8::MAX,
            clock_s: u32::MAX,
            my_hp: -1,
            foe_hp: -1,
            stars: u8::MAX,
            score: u32::MAX,
            window_px: -1,
            ..Shown::default()
        };
        b.shown.tell.mode = u8::MAX;
        b.shown.you.paused = true;
        b.shown.you.marks.push(b'#');
        let name = b.fight.as_ref().map_or("", |f| f.fighter().name);
        tiny(lcd, WIDTH as i32 - 4 - name.len() as i32 * TINY_W, 2, name, LABEL, HUD_BG);
        b.scene = Scene::empty();
        let scene = self.fight_scene();
        scene.draw_all(lcd);
        if let Some(b) = self.boxing() {
            b.scene = scene;
            b.drawn = true;
        }
        self.update_hud(lcd);
    }

    fn fight_scene(&mut self) -> Scene {
        let Some(b) = &self.boxing else { return Scene::empty() };
        let Some(f) = &b.fight else { return Scene::empty() };
        let now = b.now;
        let foe_hurting = f.foe_hp * 4 < f.fighter().hp;
        let fighters =
            Fighters { fighter: f.fighter, foe: f.foe_pose(now), me: f.me_pose(now), foe_hurting, now, stage: b.stage };
        let mut scene = Scene::new(&fighters);
        scene.set_banner(banner(f, now));
        scene
    }

    /// Runs the fight to `now`: logic, sound, then whatever on screen changed.
    fn fight_frame(&mut self, lcd: &mut Lcd, s: &Settings) {
        let Some(b) = self.boxing() else { return };
        let now = b.now;
        let Some(f) = &mut b.fight else { return };
        f.update(now);
        let over = matches!(f.phase, Phase::Over { .. });
        if over && b.over_at.is_none() {
            b.over_at = Some(now);
        }
        if let Some(at) = b.over_at
            && ticks::reached(now, at.wrapping_add(OVER_MS))
        {
            return self.finish_fight(lcd, s);
        }
        self.fight_sound(s);
        self.take_copies();
        let scene = self.fight_scene();
        let Some(b) = self.boxing() else { return };
        if b.drawn && scene != b.scene {
            scene.update(lcd, &b.scene);
            b.scene = scene;
        }
        self.update_hud(lcd);
    }

    /// Stops the tell, or starts the next one.
    fn fight_sound(&mut self, s: &Settings) {
        let Some(b) = self.boxing() else { return };
        let Some(f) = &mut b.fight else { return };
        let stop = core::mem::take(&mut f.stop);
        let tell = f.take_play();
        let fighter = f.fighter();
        let k = f.fighter;
        if stop {
            self.stop_players();
        }
        if let Some(tell) = tell {
            let (wpm, effective) = self.tell_speeds(s, k);
            let mut text = Text::<MAX_TEXT>::new();
            for c in tell.as_str().bytes() {
                let _ = core::fmt::Write::write_str(&mut text, morse::text(&c));
            }
            let seed = self.next_seed();
            let keying = Keying { wpm, effective, fist: fighter.fist };
            let pitch_hz = s.station_pitch(fighter.pitch_offset_hz);
            self.play = Some(Play { text, keying, seed, delay_ms: TELL_DELAY_MS, pitch_hz, transmit: false });
        }
    }

    /// Hands the characters the fight judged to your letter statistics and history.
    fn take_copies(&mut self) {
        let Some(b) = &mut self.boxing else { return };
        let Some(f) = &mut b.fight else { return };
        let (letters, history) = (&mut self.letters, &mut self.history);
        f.take_copies(|sent, copied| {
            letters.record(sent, copied);
            history.copied((copied == Some(sent)) as u8, 1);
        });
    }

    fn finish_fight(&mut self, lcd: &mut Lcd, s: &Settings) {
        self.stop_players();
        let Some(b) = self.boxing() else { return };
        let Some(f) = &b.fight else { return };
        let (k, score) = (f.fighter, f.score);
        let won = matches!(f.phase, Phase::Over { won: true, .. });
        let best = score > self.career.best_score(k);
        let opened = self.career.finished(k, won, score);
        if let Some(b) = self.boxing() {
            b.view = View::Results;
            b.opened = opened;
            b.best = best;
            if opened {
                b.pick = k + 1;
            }
        }
        self.history.contact();
        self.draw(lcd, s);
    }

    /// Redraws the scoreboard and panels where they changed.
    fn update_hud(&mut self, lcd: &mut Lcd) {
        let Some(b) = &mut self.boxing else { return };
        let Some(f) = &b.fight else { return };
        let now = b.now;
        let old = b.shown;
        let mut new = old;
        new.my_hp = f.my_hp;
        new.foe_hp = f.foe_hp;
        new.stars = f.stars;
        new.clean_run = f.clean_run;
        new.score = f.score;
        new.round = f.round;
        new.clock_s = f.clock_ms(now).div_ceil(1000);
        new.tell = tell_panel(f);
        let fresh = |at: u32| !ticks::reached(now, at.wrapping_add(1500));
        new.you = YouPanel {
            marks: b.marks,
            last: b.last,
            label: b.label,
            callout: f.callout.filter(|c| fresh(c.1)).map(|c| c.0),
            note: b.note.filter(|n| fresh(n.1)).map_or("", |n| n.0),
            paused: f.paused(),
        };
        let bar = (PANEL_W - 12) as u64;
        new.window_px = f.window(now).map_or(-1, |(left, whole)| (left as u64 * bar / whole.max(1) as u64) as i32);
        if new.my_hp != old.my_hp {
            health(lcd, 4, new.my_hp, PLAYER_HP, false);
        }
        if new.foe_hp != old.foe_hp {
            health(lcd, WIDTH as i32 - 4 - BAR_W, new.foe_hp, f.fighter().hp, true);
        }
        if (new.round, new.clock_s) != (old.round, old.clock_s) {
            clock(lcd, new.round, new.clock_s);
        }
        if new.score != old.score {
            let score = Text::<12>::format(format_args!("{:>7}", new.score));
            tiny(lcd, 160 - 7 * TINY_W / 2 - 3, 19, score.as_str(), PALE, HUD_BG);
        }
        if (new.stars, new.clean_run) != (old.stars, old.clean_run) {
            stars(lcd, new.stars, new.clean_run);
        }
        if new.tell != old.tell {
            draw_tell(lcd, &new.tell);
            new.window_px = -2;
        }
        if new.window_px != old.window_px {
            window_bar(lcd, new.window_px.max(-1));
        }
        if new.you != old.you {
            draw_you(lcd, &new.you);
        }
        b.shown = new;
    }

    fn fight_act(&mut self, lcd: &mut Lcd, s: &mut Settings, action: Action) {
        let Some(b) = self.boxing() else { return };
        let now = b.now;
        let Some(f) = &mut b.fight else { return };
        let paused = f.paused();
        match action {
            Action::Click if f.phase == Phase::RoundEnd => {
                f.start(now);
            }
            Action::Click if matches!(f.phase, Phase::Over { .. }) => {
                b.over_at = Some(now.wrapping_sub(OVER_MS));
            }
            Action::Click | Action::Soft(3) if !paused => f.pause(now),
            Action::Click => f.resume(now),
            Action::Soft(3) => return self.boxing_title(lcd, s),
            Action::Soft(0) if paused => return self.open_settings_at(lcd, s, Section::Keyer),
            Action::Soft(2) if paused => {
                self.career.hints = !self.career.hints;
                let hints = self.career.hints;
                if let Some(f) = self.boxing().and_then(|b| b.fight.as_mut()) {
                    f.set_hints(hints);
                }
            }
            Action::Soft(1) if !paused => {
                if !f.star(now) {
                    b.note = Some((if f.stars == 0 { "NO STARS" } else { "NOT NOW" }, now));
                }
            }
            _ => {}
        }
        self.fight_frame(lcd, s);
    }

    // --- Results ---------------------------------------------------------------------------

    fn draw_results(&mut self, lcd: &mut Lcd) {
        let Some(b) = &self.boxing else { return };
        let Some(f) = &b.fight else { return };
        rect(lcd, 0, 0, WIDTH as i32, HEIGHT as i32, HUD_BG);
        let next = b.opened || (f.fighter + 1 < self.career.unlocked as usize);
        self.softkeys(lcd, ["", "AGN", if next { "NEXT" } else { "" }, "BACK"]);
        let Some(b) = &self.boxing else { return };
        let Some(f) = &b.fight else { return };
        let Phase::Over { won, how } = f.phase else { return };
        let (title, color) = if won { ("YOU WIN", BRASS) } else { ("YOU LOSE", RED) };
        let w = blocks_width(title.len(), 4, 3);
        blocks(lcd, (MAIN - w) / 2 + 2, 10, title, 4, 3, 4, rgb(0x050308), None);
        blocks(lcd, (MAIN - w) / 2, 8, title, 4, 3, 4, color, None);
        let how = match how {
            Outcome::Ko => "BY KNOCKOUT",
            Outcome::Tko => "BY TECHNICAL KNOCKOUT",
            Outcome::Decision => "ON THE JUDGES' CARDS",
        };
        let line = Text::<48>::format(format_args!("{how}, ROUND {} OF {ROUNDS}", f.round.min(ROUNDS)));
        tiny(lcd, (MAIN - line.len() as i32 * TINY_W) / 2, 42, line.as_str(), LABEL, HUD_BG);
        rect(lcd, 10, 58, MAIN - 20, 1, PANEL_EDGE);
        // Score and the counts.
        let score = Text::<16>::format(format_args!("{}", f.score));
        tiny(lcd, 12, 66, "SCORE", LABEL, HUD_BG);
        text(lcd, 12, 78, score.as_str(), PALE, HUD_BG);
        if b.best {
            tiny(lcd, 16 + score.len() as i32 * CHAR_W, 81, "NEW BEST", GOOD, HUD_BG);
        }
        let st = &f.stats;
        let pct = |p: Option<u8>| p.map_or(Text::<6>::format(format_args!("--")), |p| Text::format(format_args!("{p}%")));
        let rows: [(&str, Text<24>); 5] = [
            ("DODGED", Text::format(format_args!("{}/{} {}", st.dodged, st.tells, pct(st.accuracy()).as_str()))),
            ("PUNCHES", Text::format(format_args!("{} CLEAN {}", st.punches, pct(st.clean_percent()).as_str()))),
            ("UPPERCUTS", Text::format(format_args!("{}", st.uppercuts))),
            ("KNOCKDOWNS", Text::format(format_args!("{} FOR, {} AGAINST", st.knockdowns, st.knocked))),
            ("DAMAGE", Text::format(format_args!("{} DEALT, {} TAKEN", st.damage, st.taken))),
        ];
        for (k, (label, value)) in rows.iter().enumerate() {
            let y = 100 + k as i32 * 14;
            tiny(lcd, 12, y, label, LABEL, HUD_BG);
            tiny(lcd, 96, y, value.as_str(), PALE, HUD_BG);
        }
        // Each character's tells this fight, worst first.
        let y = 174;
        rect(lcd, 10, y - 4, MAIN - 20, 1, PANEL_EDGE);
        tiny(lcd, 12, y, "LETTERS, WORST FIRST", LABEL, HUD_BG);
        let mut any = false;
        for (k, (c, right, tries)) in st.worst().take(8).enumerate() {
            any = true;
            let x = 12 + k as i32 * 31;
            let color = if right == tries { GOOD } else if right * 2 >= tries { WARN } else { BAD };
            let tile = mix(color, HUD_BG, 75);
            rect(lcd, x, y + 14, 29, 28, tile);
            rect(lcd, x, y + 40, 29, 2, color);
            let cells = super::char_cells(c) as i32;
            super::draw_char(lcd, &crate::fonts::SMALL, x + (29 - cells * CHAR_W) / 2, y + 14, c, PALE, tile);
            let n = Text::<6>::format(format_args!("{right}/{tries}"));
            tiny(lcd, x + (29 - n.len() as i32 * TINY_W) / 2, y + 29, n.as_str(), PALE, tile);
        }
        if !any {
            tiny(lcd, 12, y + 18, "NO TELLS THIS FIGHT", LABEL, HUD_BG);
        }
        if b.opened {
            let line = Text::<40>::format(format_args!("NEW CHALLENGER: {}", LADDER[f.fighter + 1].name));
            tiny(lcd, 12, 226, line.as_str(), BRASS, HUD_BG);
        } else if won && f.fighter + 1 == FIGHTERS {
            tiny(lcd, 12, 226, "CHAMPION OF THE AIRWAVES!", BRASS, HUD_BG);
        }
    }

    fn results_act(&mut self, lcd: &mut Lcd, s: &mut Settings, action: Action) {
        let unlocked = self.career.unlocked as usize;
        let Some(b) = self.boxing() else { return };
        let k = b.fight.as_ref().map_or(b.pick, |f| f.fighter);
        match action {
            Action::Soft(1) => {
                self.new_fight(k, s);
                self.draw(lcd, s);
            }
            Action::Soft(2) if k + 1 < unlocked => {
                self.new_fight(k + 1, s);
                self.draw(lcd, s);
            }
            Action::Click | Action::Soft(3) => self.boxing_title(lcd, s),
            _ => {}
        }
    }

    // --- Events from the UI ------------------------------------------------------------------

    pub(super) fn boxing_act(&mut self, lcd: &mut Lcd, s: &mut Settings, action: Action) {
        let Some(view) = self.boxing().map(|b| b.view) else { return };
        match view {
            View::Splash => self.splash_act(lcd, s, action),
            View::Title => self.title_act(lcd, s, action),
            View::Card => self.card_act(lcd, s, action),
            View::Fight => self.fight_act(lcd, s, action),
            View::Results => self.results_act(lcd, s, action),
        }
    }

    /// The key went down or up at `at_ms`.
    pub(super) fn boxing_key(&mut self, at_ms: u32) {
        if let Some(b) = self.boxing() {
            b.now = later(b.now, at_ms);
            b.edge = at_ms;
        }
    }

    /// A finished mark: one more dot or dash on the YOU panel.
    pub(super) fn boxing_interval(&mut self, iv: &Interval) {
        let Some(b) = self.boxing() else { return };
        if b.view != View::Fight {
            return;
        }
        // A punch is judged on its own marks and the gaps inside it, not the pause before it.
        let mark = match iv.kind {
            Kind::Dit => b'.',
            Kind::Dah => b'-',
            Kind::ElementGap => {
                b.worst = b.worst.max(iv.quality());
                return;
            }
            _ => return,
        };
        if b.marks.is_empty() {
            b.worst = Quality::Good;
        }
        b.worst = b.worst.max(iv.quality());
        if !b.marks.push(mark) {
            b.marks.clear();
            b.marks.push(mark);
        }
    }

    pub(super) fn boxing_decoded(&mut self, lcd: &mut Lcd, s: &Settings, decoded: Decoded) {
        let Some(b) = self.boxing() else { return };
        if b.view != View::Fight {
            return;
        }
        // The decoder ends a character two dots after its last mark.
        let now = later(b.now, b.edge.wrapping_add(2 * keyer::dit_ms(s.wpm) as u32));
        b.now = now;
        let worst = core::mem::replace(&mut b.worst, Quality::Good);
        let Some(f) = &mut b.fight else { return };
        // Only a counter is a punch; the rest answer a tell, spell a word, or hit gloves.
        b.label = match (decoded, f.open()) {
            (Decoded::Char(c), true) => match punch_of(c) {
                Some(Punch::Jab) => "JAB",
                Some(Punch::Hook) => "HOOK",
                Some(Punch::Combo(_)) => "COMBO",
                None => "",
            },
            (Decoded::Unknown, true) => "MISS",
            _ => "",
        };
        match decoded {
            Decoded::Char(c) => {
                b.last = Some((c, worst));
                b.marks.clear();
                f.key(c, worst, now);
            }
            Decoded::Unknown => {
                b.last = Some((b'*', Quality::Bad));
                b.marks.clear();
                f.key_unknown(now);
            }
            Decoded::Space => return,
        }
        self.fight_frame(lcd, s);
    }

    pub(super) fn boxing_played(&mut self, now: u32) {
        let Some(b) = self.boxing() else { return };
        b.now = later(b.now, now);
        if let Some(f) = &mut b.fight {
            f.tell_played(now);
        }
    }

    pub(super) fn boxing_timeout(&mut self, lcd: &mut Lcd, s: &Settings, now: u32) {
        let Some(b) = self.boxing() else { return };
        b.now = later(b.now, now);
        let fighting = b.view == View::Fight;
        let fast = fighting || b.view == View::Splash;
        b.next_frame = now.wrapping_add(if fast { FRAME_MS } else { IDLE_FRAME_MS });
        if fighting {
            return self.fight_frame(lcd, s);
        }
        if b.view == View::Splash {
            return self.splash_frame(lcd, s);
        }
        // Off the fight, only the prompt blinks.
        let blink = (now / 500) % 2 == 1;
        if b.blink == blink {
            return;
        }
        b.blink = blink;
        match b.view {
            View::Title => self.draw_title_foot(lcd),
            View::Card => self.draw_card_prompt(lcd),
            _ => {}
        }
    }

    // --- The startup screen --------------------------------------------------------------------

    fn draw_splash(&mut self, lcd: &mut Lcd) {
        let Some(b) = self.boxing() else { return };
        b.splash = Splash::empty();
        b.motto = false;
        b.prompt = None;
        ring::below_splash(lcd, 2 * ring::SPLASH_H);
        let t = b.now.wrapping_sub(b.splash_at);
        let splash = Splash::new(t, b.splash_seed);
        splash.draw_all(lcd);
        b.splash = splash;
        self.splash_parts(lcd, t);
    }

    fn splash_frame(&mut self, lcd: &mut Lcd, s: &Settings) {
        let Some(b) = self.boxing() else { return };
        let t = b.now.wrapping_sub(b.splash_at);
        if t >= SPLASH_MS {
            return self.boxing_title(lcd, s);
        }
        let splash = Splash::new(t, b.splash_seed);
        if splash != b.splash {
            splash.update(lcd, &b.splash);
            b.splash = splash;
        }
        self.splash_parts(lcd, t);
    }

    /// The motto and the blinking prompt under the animation, once their time comes.
    fn splash_parts(&mut self, lcd: &mut Lcd, t: u32) {
        let Some(b) = self.boxing() else { return };
        if t >= ring::MOTTO_MS && !b.motto {
            b.motto = true;
            ring::motto(lcd);
        }
        let prompt = (t >= ring::PROMPT_MS).then_some((t / 450) % 3 != 2);
        if prompt != b.prompt {
            b.prompt = prompt;
            let line = "CLICK OR PRESS ANY KEY";
            let x = (WIDTH as i32 - line.len() as i32 * TINY_W) / 2;
            let fg = if prompt == Some(true) { ring::SPLASH_YELLOW } else { ring::SPLASH_BAND };
            tiny(lcd, x, ring::PROMPT_Y, line, fg, ring::SPLASH_BAND);
        }
    }

    fn splash_act(&mut self, lcd: &mut Lcd, s: &mut Settings, action: Action) {
        if matches!(action, Action::Click | Action::Soft(_)) {
            self.boxing_title(lcd, s);
        }
    }
}

/// The later of two tick counts.
fn later(a: u32, b: u32) -> u32 {
    if ticks::reached(a, b) { a } else { b }
}

/// What the TELL panel shows.
fn tell_panel(f: &Fight) -> TellPanel {
    let mut p = TellPanel { hints: f.hinting(), sounding: f.sounding(), ..TellPanel::default() };
    let copy = |t: &str| Text::<12>::format(format_args!("{t}"));
    if f.paused() {
        p.mode = 4;
    } else if f.phase == Phase::MeDown {
        p.mode = 3;
        p.text.push(f.get_up);
        p.count = f.count;
    } else if f.starring() {
        p.mode = 2;
        p.text = copy(f.word.as_str());
        p.typed = copy(f.typed.as_str());
    } else if f.phase == Phase::RoundEnd {
        p.mode = 5;
        p.count = f.round;
        p.shown = f.deadline().is_none();
    } else if !f.tell.is_empty() {
        p.mode = 1;
        p.text = copy(f.tell.as_str());
        p.typed = copy(f.answer.as_str());
        p.shown = f.tell_shown;
        if !f.telling() {
            p.resolved = Some(f.answer.as_str() == f.tell.as_str());
        }
    }
    p
}

fn panel_clear(lcd: &mut Lcd, x: i32) {
    let inner = if x == 0 { 0 } else { x + 1 };
    rect(lcd, inner, PANEL_TOP, PANEL_W - 1, PANEL_BOTTOM - PANEL_TOP - 1, PANEL_BG);
}

fn draw_tell(lcd: &mut Lcd, p: &TellPanel) {
    panel_clear(lcd, 0);
    let (x, y) = (4, PANEL_TOP + 3);
    match p.mode {
        1 => {
            tiny(lcd, x, y, "TELL", LABEL, PANEL_BG);
            if p.sounding {
                speaker(lcd, PANEL_W - 14, y + 2);
            }
            let color = match p.resolved {
                Some(true) => GOOD,
                Some(false) => BAD,
                None => BRASS,
            };
            if p.shown {
                big_chars(lcd, p.text.as_str(), y + 15, color);
                patterns(lcd, p.text.as_str(), y + 55, color);
            } else {
                let unknown: Text<12> = p.text.as_str().bytes().map(|_| b'?').fold(Text::new(), |mut t, c| {
                    t.push(c);
                    t
                });
                big_chars(lcd, unknown.as_str(), y + 15, DIM);
                tiny(lcd, x, y + 52, "LISTEN!", LABEL, PANEL_BG);
            }
            typed(lcd, p.text.as_str(), p.typed.as_str(), y + 61);
        }
        2 => {
            tiny(lcd, x, y, "UPPERCUT", BRASS, PANEL_BG);
            tiny(lcd, x, y + 14, "SEND IT:", LABEL, PANEL_BG);
            let word = p.text.as_str();
            let px = if word.len() <= 4 { 2 } else { 1 };
            let w = blocks_width(word.len(), px * 2, px);
            if w <= PANEL_W - 4 {
                blocks(lcd, (PANEL_W - w) / 2, y + 30, word, px * 2, px, 0, PALE, None);
            } else {
                tiny(lcd, x, y + 32, word, PALE, PANEL_BG);
            }
            typed(lcd, word, p.typed.as_str(), y + 61);
        }
        3 => {
            tiny(lcd, x, y, "GET UP!", RED, PANEL_BG);
            tiny(lcd, x, y + 12, "KEY THIS", LABEL, PANEL_BG);
            big_chars(lcd, p.text.as_str(), y + 26, BRASS);
            patterns(lcd, p.text.as_str(), y + 62, BRASS);
        }
        4 => {
            tiny(lcd, x, y, "PAUSED", BRASS, PANEL_BG);
            for (k, line) in ["CLICK GO", "F1 SET", if p.hints { "F3 HINT:Y" } else { "F3 HINT:N" }, "F4 QUIT"]
                .iter()
                .enumerate()
            {
                tiny(lcd, x, y + 18 + k as i32 * 14, line, PALE, PANEL_BG);
            }
        }
        5 => {
            tiny(lcd, x, y, "CORNER", LABEL, PANEL_BG);
            let next = Text::<12>::format(format_args!("ROUND {}", p.count + 1));
            tiny(lcd, x, y + 24, next.as_str(), PALE, PANEL_BG);
            if p.shown {
                tiny(lcd, x, y + 40, "CLICK FOR", BRASS, PANEL_BG);
                tiny(lcd, x, y + 52, "THE BELL", BRASS, PANEL_BG);
            }
        }
        _ => tiny(lcd, x, y, "TELL", LABEL, PANEL_BG),
    }
}

/// The tell's characters in block letters, as big as fit.
fn big_chars(lcd: &mut Lcd, s: &str, y: i32, color: Rgb565) {
    let n = s.len().max(1);
    let px = match n {
        1 => 5,
        2 => 3,
        _ => 2,
    };
    let w = blocks_width(n, px, px + 1);
    // Prosigns would not fit in block letters: they show as their letters.
    let mut shown = Text::<12>::new();
    for c in s.bytes() {
        shown.push(if c.is_ascii_graphic() { c } else { b'#' });
    }
    blocks(lcd, (PANEL_W - w) / 2, y, shown.as_str(), px, px + 1, 0, color, None);
}

/// Each character's dots and dashes, side by side.
fn patterns(lcd: &mut Lcd, s: &str, y: i32, color: Rgb565) {
    let total: i32 = s.bytes().map(|c| pattern_width(c, 3)).sum::<i32>() + 6 * (s.len() as i32 - 1);
    let unit = if total <= PANEL_W - 8 { 3 } else { 2 };
    let total: i32 = s.bytes().map(|c| pattern_width(c, unit)).sum::<i32>() + 2 * unit * (s.len() as i32 - 1);
    let mut x = (PANEL_W - total) / 2;
    for c in s.bytes() {
        x = pattern(lcd, x, y, c, unit, color) + 2 * unit;
    }
}

fn pattern_width(c: u8, unit: i32) -> i32 {
    let Some(code) = morse::code(c) else { return 0 };
    code.bytes().map(|e| if e == b'-' { 3 * unit } else { unit }).sum::<i32>() + (code.len() as i32 - 1) * unit
}

/// Draws `c`'s code from `x`; returns the x after it.
fn pattern(lcd: &mut Lcd, mut x: i32, y: i32, c: u8, unit: i32, color: Rgb565) -> i32 {
    for e in morse::code(c).unwrap_or("").bytes() {
        let w = if e == b'-' { 3 * unit } else { unit };
        rect(lcd, x, y, w, unit, color);
        x += w + unit;
    }
    x - unit
}

/// What you have keyed so far, marked against `want`.
fn typed(lcd: &mut Lcd, want: &str, got: &str, y: i32) {
    let mut x = 4;
    for (k, c) in got.bytes().enumerate() {
        let right = want.as_bytes().get(k) == Some(&c);
        let color = if right { GOOD } else { BAD };
        let shown = if c.is_ascii_graphic() { c } else { b'?' };
        tiny(lcd, x, y, core::str::from_utf8(&[shown]).unwrap_or("?"), color, PANEL_BG);
        x += TINY_W;
    }
}

/// A little speaker with sound coming out, while the tell plays.
fn speaker(lcd: &mut Lcd, x: i32, y: i32) {
    rect(lcd, x, y + 2, 3, 4, PALE);
    rect(lcd, x + 3, y, 2, 8, PALE);
    rect(lcd, x + 7, y + 1, 1, 6, BRASS);
    rect(lcd, x + 9, y, 1, 8, BRASS);
}

fn window_bar(lcd: &mut Lcd, px: i32) {
    let (x, y, w) = (6, PANEL_BOTTOM - 8, PANEL_W - 12);
    rect(lcd, x, y, w, 4, if px < 0 { PANEL_BG } else { BAR_EMPTY });
    if px > 0 {
        let color = if px * 3 > w * 2 {
            GOOD
        } else if px * 3 > w {
            WARN
        } else {
            BAD
        };
        rect(lcd, x, y, px, 4, color);
    }
}

fn draw_you(lcd: &mut Lcd, p: &YouPanel) {
    panel_clear(lcd, RIGHT_X);
    let (x, y) = (RIGHT_X + 5, PANEL_TOP + 3);
    tiny(lcd, x, y, "YOU", LABEL, PANEL_BG);
    if p.paused {
        for (k, line) in ["F2 IN THE", "FIGHT:", "SPEND A", "STAR"].iter().enumerate() {
            tiny(lcd, x, y + 18 + k as i32 * 13, line, PALE, PANEL_BG);
        }
        return;
    }
    // The character you are keying, mark by mark.
    let mut mx = x;
    for &m in p.marks.as_str().as_bytes() {
        let w = if m == b'-' { 9 } else { 3 };
        if mx + w > WIDTH as i32 - 4 {
            break;
        }
        rect(lcd, mx, y + 15, w, 3, PALE);
        mx += w + 3;
    }
    if let Some((c, q)) = p.last {
        let color = if c == b'*' { BAD } else { quality_color(q) };
        let buf = [if c.is_ascii_graphic() { c } else { b'#' }];
        let s = core::str::from_utf8(&buf).unwrap_or("?");
        blocks(lcd, RIGHT_X + (PANEL_W - blocks_width(1, 3, 0)) / 2, y + 22, s, 3, 0, 0, color, None);
        underline(lcd, RIGHT_X + 20, y + 46, PANEL_W - 40, q, PANEL_BG);
        tiny(lcd, x, y + 52, p.label, LABEL, PANEL_BG);
    }
    let (call, color) = match p.callout {
        _ if !p.note.is_empty() => (p.note, WARN),
        Some(Callout::Dodge) => ("DODGE!", GOOD),
        Some(Callout::Wrong { .. }) => ("WRONG!", BAD),
        Some(Callout::TooSlow) => ("TOO SLOW", BAD),
        Some(Callout::Clean) => ("CLEAN!", GOOD),
        Some(Callout::Star) => ("STAR!", BRASS),
        Some(Callout::Uppercut) => ("POW!", BRASS),
        Some(Callout::Blocked) => ("BLOCKED", WARN),
        Some(Callout::Missed) => ("MISSED", BAD),
        Some(Callout::Down) => ("DOWN!", RED),
        Some(Callout::Up) => ("UP AGAIN", LABEL),
        None => ("", LABEL),
    };
    tiny(lcd, x, y + 66, call, color, PANEL_BG);
}

fn health(lcd: &mut Lcd, x: i32, hp: i16, max: i16, right: bool) {
    let y = 13;
    rect(lcd, x - 1, y - 1, BAR_W + 2, 9, rgb(0x050308));
    let full = (hp.max(0) as i32 * BAR_W) / max.max(1) as i32;
    let color = if hp * 2 > max {
        GOOD
    } else if hp * 4 > max {
        WARN
    } else {
        BAD
    };
    let lit = mix(color, Rgb565::WHITE, 35);
    rect(lcd, x, y, BAR_W, 7, BAR_EMPTY);
    let fx = if right { x + BAR_W - full } else { x };
    rect(lcd, fx, y, full, 7, color);
    rect(lcd, fx, y, full, 2, lit);
    // Segment notches every tenth.
    for k in 1..10 {
        rect(lcd, x + k * BAR_W / 10, y, 1, 7, rgb(0x050308));
    }
}

fn clock(lcd: &mut Lcd, round: u8, seconds: u32) {
    let line = Text::<12>::format(format_args!("R{} {}:{:02}", round.min(ROUNDS), seconds / 60, seconds % 60));
    let x = 160 - line.len() as i32 * CHAR_W / 2;
    rect(lcd, 160 - 40, 1, 80, 16, HUD_BG);
    text(lcd, x, 1, line.as_str(), PALE, HUD_BG);
}

/// Your stars, and pips toward the next.
fn stars(lcd: &mut Lcd, n: u8, run: u8) {
    let (x0, y) = (4, 23);
    rect(lcd, x0, y, 80, 7, HUD_BG);
    for k in 0..MAX_STARS {
        star(lcd, x0 + k as i32 * 10, y, if k < n { BRASS } else { DIM });
    }
    for k in 0..CLEAN_PER_STAR {
        let color = if k < run { GOOD } else { DIM };
        rect(lcd, x0 + 34 + k as i32 * 5, y + 2, 3, 3, color);
    }
}

fn star(lcd: &mut Lcd, x: i32, y: i32, color: Rgb565) {
    rect(lcd, x + 3, y, 1, 2, color);
    rect(lcd, x, y + 2, 7, 1, color);
    rect(lcd, x + 1, y + 3, 5, 1, color);
    rect(lcd, x + 2, y + 4, 3, 1, color);
    rect(lcd, x + 1, y + 5, 2, 1, color);
    rect(lcd, x + 4, y + 5, 2, 1, color);
    rect(lcd, x, y + 6, 1, 1, color);
    rect(lcd, x + 6, y + 6, 1, 1, color);
}

/// The big words across the ring.
fn banner(f: &Fight, now: u32) -> Option<Banner> {
    let make = |s: &str, color: Rgb565| {
        let mut text = Text::<24>::new();
        for b in s.bytes().take(12) {
            text.push(b);
        }
        Some(Banner { text, color, y: BANNER_Y, px: 2, edge: rgb(0x120A10) })
    };
    if f.paused() {
        return make("PAUSED", PALE);
    }
    match f.phase {
        Phase::RoundStart => {
            let left = f.deadline().map_or(0, |d| ticks::ms_until(now, d));
            if left < 600 {
                make("FIGHT!", RED)
            } else {
                let r = Text::<12>::format(format_args!("ROUND {}", f.round));
                make(r.as_str(), BRASS)
            }
        }
        Phase::RoundEnd => make("BELL!", BRASS),
        Phase::FoeDown | Phase::MeDown if f.count == 0 => make("DOWN!", RED),
        Phase::FoeDown | Phase::MeDown => {
            let n = Text::<12>::format(format_args!("{}", f.count));
            make(n.as_str(), PALE)
        }
        Phase::Over { won, how } => {
            match (won, how) {
                (true, Outcome::Ko) => make("K.O.!", BRASS),
                (true, Outcome::Tko) => make("T.K.O.!", BRASS),
                (true, Outcome::Decision) => make("YOU WIN", BRASS),
                (false, Outcome::Decision) => make("DECISION", RED),
                (false, _) => make("YOU LOSE", RED),
            }
        }
        Phase::Intro | Phase::Fight => None,
    }
}
