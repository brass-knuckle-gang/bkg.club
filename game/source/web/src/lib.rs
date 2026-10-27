//! Brass Knuckles Knock Out in a web browser. The HamCube's own code, the screens in
//! `firmware/src/ui` and the rules in `logic`, built for WebAssembly. This file does what the
//! device's 1 ms tick and UI task do around them: it reads the keys the page passes in, runs
//! the keyer, decoder and players, and makes the sound with the device's synth, one
//! millisecond at a time, paced by the page's audio.
//!
//! The page (`web/index.html`) calls `init`, then `run` from its audio callback, and copies
//! the frame buffer to a canvas when `frame_dirty` says it changed.

#![no_std]
#![allow(dead_code, static_mut_refs)]

#[path = "../../firmware/src/art.rs"]
mod art;
#[path = "../../firmware/src/font.rs"]
mod font;
#[path = "../../firmware/src/fonts.rs"]
mod fonts;
#[path = "../../firmware/src/gfx.rs"]
mod gfx;
mod lcd;
mod mocks;
#[path = "../../firmware/src/sprites.rs"]
mod sprites;
#[path = "../../firmware/src/text.rs"]
mod text;
#[path = "../../firmware/src/theme.rs"]
mod theme;
#[path = "../../firmware/src/ui/mod.rs"]
mod ui;

use mocks::{audio, battery, clock, codec, link, pack};

use pocketcw_logic::{
    battery::Gauge,
    boxing::{Career, RECORD_LEN as CAREER_LEN},
    buttons::{Buttons, COUNT},
    decoder::KeyDecoder,
    input::{Action, Input},
    keyer::{Contacts, Jack, Keyer, Mode},
    letters::{Letters, RECORD_LEN as LETTERS_LEN},
    memories::Memories,
    panel::{IdRead, Panel, Source},
    player::Player,
    progress::{History, RECORD_LEN as HISTORY_LEN},
    settings::{RECORD_LEN as SETTINGS_LEN, Settings},
    synth::{PLAYERS, SIDETONE, Synth},
    ticks,
    timing::Line,
};

use lcd::Lcd;
use ui::{Command, PanelFrom, SystemInfo, Ui};

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

/// Keys the page passes in, by number.
const DIT: usize = 0;
const DAH: usize = 1;
/// F1..F4 are 2..5, then the dial: click, left, right.
const KEYS: usize = 9;

struct Engine {
    ui: Ui,
    lcd: Lcd,
    s: Settings,
    jack: Jack,
    keyer: Keyer,
    decoder: KeyDecoder,
    buttons: Buttons,
    input: Input,
    players: [Player; PLAYERS],
    synth: Synth,
    sound: pocketcw_logic::synth::Sound,
    now: u32,
    keys: [bool; KEYS],
    contacts: Contacts,
    key_down: bool,
    playing: bool,
    /// Samples per millisecond, in thousandths, and how far into the next millisecond.
    rate: u32,
    phase: u32,
    seed: u32,
}

static mut ENGINE: Option<Engine> = None;
static mut FB: [u16; 320 * 240] = [0; 320 * 240];
static mut AUDIO: [f32; 4096] = [0.0; 4096];
/// Records the page keeps in local storage: written by the page before `init`, by the engine
/// when they change.
static mut CAREER: [u8; CAREER_LEN] = [0xFF; CAREER_LEN];
static mut LETTERS: [u8; LETTERS_LEN] = [0xFF; LETTERS_LEN];
static mut SETTINGS: [u8; SETTINGS_LEN] = [0xFF; SETTINGS_LEN];
static mut SAVED: u32 = 0;

fn engine() -> &'static mut Engine {
    // SAFETY: the page is single threaded and calls `init` first.
    unsafe { ENGINE.as_mut().unwrap() }
}

#[unsafe(no_mangle)]
pub extern "C" fn frame_ptr() -> *const u16 {
    &raw const FB as *const u16
}

#[unsafe(no_mangle)]
pub extern "C" fn audio_ptr() -> *const f32 {
    &raw const AUDIO as *const f32
}

#[unsafe(no_mangle)]
pub extern "C" fn career_ptr() -> *mut u8 {
    &raw mut CAREER as *mut u8
}

#[unsafe(no_mangle)]
pub extern "C" fn letters_ptr() -> *mut u8 {
    &raw mut LETTERS as *mut u8
}

#[unsafe(no_mangle)]
pub extern "C" fn settings_ptr() -> *mut u8 {
    &raw mut SETTINGS as *mut u8
}

#[unsafe(no_mangle)]
pub extern "C" fn record_lens() -> u32 {
    (CAREER_LEN as u32) | (LETTERS_LEN as u32) << 10 | (SETTINGS_LEN as u32) << 20
}

/// Starts the game at the startup screen. `sample_hz` is the page's audio rate.
#[unsafe(no_mangle)]
pub extern "C" fn init(seed: u32, sample_hz: u32) {
    // SAFETY: single threaded; the page wrote the records before this call.
    let (career, letters, settings) = unsafe {
        (
            Career::decode(&CAREER).unwrap_or_default(),
            Letters::decode(&LETTERS).unwrap_or_default(),
            Settings::decode(&SETTINGS).unwrap_or_default(),
        )
    };
    let s = settings;
    let system = SystemInfo {
        flash_kb: 256,
        board: 0x64,
        panel: Panel::Ips,
        panel_from: PanelFrom::Chosen(Source::Saved),
        panel_id: [IdRead { pulled_up: 0, pulled_down: 0 }; 2],
        uid: [seed, 0, 0],
        clock: None,
        codec: None,
        codec_ok: true,
        eeprom_ok: true,
        adc_ok: true,
    };
    let ui = Ui::new(
        &s,
        system,
        letters,
        History::decode(&[0xFF; HISTORY_LEN]),
        Memories::new(s.callsign()),
        Gauge::new(None, 0),
        Err(pocketcw_logic::pack::Error::Missing),
        career,
    );
    // SAFETY: as above.
    let fb = unsafe { &mut FB };
    let keyer = Keyer::new(s.mode, s.swap, s.wpm);
    let decoder = KeyDecoder::new(keyer.dit_ms());
    let sound = s.sound();
    let mut synth = Synth::new(sample_hz as f32, sound);
    synth.set_gain(0.6);
    let e = Engine {
        ui,
        lcd: Lcd { fb, dirty: true },
        s,
        jack: Jack::new(Contacts { tip: false, ring: false }),
        keyer,
        decoder,
        buttons: Buttons::default(),
        input: Input::new(false),
        players: [Player::new(); PLAYERS],
        synth,
        sound,
        now: 1,
        keys: [false; KEYS],
        contacts: Contacts { tip: false, ring: false },
        key_down: false,
        playing: false,
        rate: sample_hz,
        phase: 0,
        seed,
    };
    // SAFETY: as above.
    unsafe { ENGINE = Some(e) };
    let e = engine();
    e.ui.draw(&mut e.lcd, &e.s);
    e.ui.open_boxing(&mut e.lcd, &e.s);
    after(e);
}

/// A key on the page went down or up: 0 dit, 1 dah, 2..5 F1..F4, 6 dial click, 7 and 8 the
/// dial turned left and right.
#[unsafe(no_mangle)]
pub extern "C" fn key(which: u32, down: bool) {
    if let Some(k) = engine().keys.get_mut(which as usize) {
        *k = down;
    }
}

/// Runs the device `n` audio samples forward, at most 4096, and leaves the sound in the audio
/// buffer: a millisecond tick every `sample_hz / 1000` samples.
#[unsafe(no_mangle)]
pub extern "C" fn run(n: u32) {
    let e = engine();
    for k in 0..(n as usize).min(4096) {
        e.phase += 1000;
        if e.phase >= e.rate {
            e.phase -= e.rate;
            tick(e);
        }
        // SAFETY: single threaded.
        unsafe { AUDIO[k] = e.synth.sample() };
    }
}

/// Runs `ms` ticks with no sound, for when the page has no audio.
#[unsafe(no_mangle)]
pub extern "C" fn advance(ms: u32) {
    let e = engine();
    for _ in 0..ms.min(1000) {
        tick(e);
    }
}

/// Whether the screen changed since the last call.
#[unsafe(no_mangle)]
pub extern "C" fn frame_dirty() -> bool {
    core::mem::take(&mut engine().lcd.dirty)
}

/// Which records changed since the last call, to store: bit 0 career, 1 letters, 2 settings.
#[unsafe(no_mangle)]
pub extern "C" fn take_saved() -> u32 {
    // SAFETY: single threaded.
    unsafe { core::mem::take(&mut SAVED) }
}

/// One millisecond of the device, as its tick and UI tasks run it.
fn tick(e: &mut Engine) {
    e.now = e.now.wrapping_add(1);
    let now = e.now;
    let dit_on_ring = e.s.mode.dit_on_ring(e.s.swap);
    let (dit, dah) = (e.keys[DIT], e.keys[DAH]);
    let raw = if dit_on_ring { Contacts { tip: dah, ring: dit } } else { Contacts { tip: dit, ring: dah } };
    e.jack.tick(raw);
    let (contacts, keying) = (e.jack.closed(), e.jack.keying());
    let down = e.keyer.tick(keying);
    let copy_dit = e.decoder.dit_ms();
    let old = e.contacts;
    for (line, was, is) in [(Line::Tip, old.tip, contacts.tip), (Line::Ring, old.ring, contacts.ring)] {
        if was != is {
            e.ui.edge(&mut e.lcd, &e.s, line, is, now, copy_dit);
        }
    }
    e.contacts = contacts;
    if down != e.key_down {
        e.key_down = down;
        e.ui.edge(&mut e.lcd, &e.s, Line::Key, down, now, copy_dit);
    }
    let mut played = [None; PLAYERS];
    for (i, p) in e.players.iter_mut().enumerate() {
        played[i] = p.tick();
        e.synth.key(SIDETONE + 1 + i, played[i] == Some(true), p.pitch_hz());
    }
    e.synth.key(SIDETONE, down, e.s.tone_hz);
    let playing = played.iter().any(Option::is_some);
    if e.playing && !playing {
        e.ui.played(&mut e.lcd, &e.s, now);
    }
    e.playing = playing;
    if let Some(decoded) = e.decoder.tick(down, e.keyer.dit_ms(), e.s.mode == Mode::Straight) {
        e.ui.decoded(&mut e.lcd, &mut e.s, decoded);
    }
    let mut raw = [false; COUNT];
    raw[..4].copy_from_slice(&e.keys[2..6]);
    // Buttons: F1..F4, Left, Right, Click.
    raw[4] = e.keys[7];
    raw[5] = e.keys[8];
    raw[6] = e.keys[6];
    let mut events = [None; 8];
    let mut n = 0;
    e.buttons.tick(raw, |event| {
        if n < events.len() {
            events[n] = Some(event);
            n += 1;
        }
    });
    for event in events.into_iter().flatten() {
        match e.input.map(event) {
            None | Some(Action::PowerHold | Action::PowerCancel | Action::PowerOff) => {}
            Some(action) => e.ui.act(&mut e.lcd, &mut e.s, action, now),
        }
    }
    if e.ui.deadline().is_some_and(|at| ticks::reached(now, at)) {
        e.ui.timeout(&mut e.lcd, &mut e.s, now);
    }
    after(e);
}

/// What the UI task does after each event: start and stop the players, apply new settings,
/// set the sound, keep the game on screen, and note records to save.
fn after(e: &mut Engine) {
    let before = (e.keyer.mode(), e.keyer.swap, e.keyer.dit_ms());
    while let Some(command) = e.ui.take_command(&e.s) {
        match command {
            Command::Play(p) => {
                let i = e.players.iter().position(|p| !p.is_active()).unwrap_or(0);
                e.players[i].start(p.text.as_str(), p.keying, p.seed, p.delay_ms, p.pitch_hz);
            }
            Command::Stop => e.players.iter_mut().for_each(Player::stop),
        }
    }
    if (e.s.mode, e.s.swap, pocketcw_logic::keyer::dit_ms(e.s.wpm)) != before {
        e.keyer.set_mode(e.s.mode);
        e.keyer.swap = e.s.swap;
        e.keyer.set_wpm(e.s.wpm);
    }
    e.synth.set_band(e.ui.band());
    let sound = e.ui.sound(&e.s);
    if sound != e.sound {
        e.sound = sound;
        e.synth.set_sound(sound);
    }
    // Only the game and the screens it opens: anything else goes back to the game.
    if !e.ui.in_boxing() {
        e.ui.open_boxing(&mut e.lcd, &e.s);
    }
    // SAFETY: single threaded.
    unsafe {
        if let Some(r) = e.ui.take_career(false) {
            CAREER = r;
            SAVED |= 1;
        }
        if let Some(r) = e.ui.take_letters(true) {
            LETTERS = r;
            SAVED |= 2;
        }
        let settings = e.s.encode(0);
        if settings[..] != SETTINGS[..] {
            SETTINGS = settings;
            SAVED |= 4;
        }
    }
}
