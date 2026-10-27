//! The ring for Brass Knuckles Knock Out: the arena, the opponent facing you and you seen from
//! behind, drawn as pixel art at twice its size.
//!
//! A frame is a scene: a fixed list of slots, one per body part or effect, drawn in order over
//! the arena. Each frame redraws only the rectangles whose slots changed. A rectangle goes to
//! the panel in one window: every art row is composed into a buffer, back to front, and sent
//! twice, each pixel doubled, so nothing flickers.

use embedded_graphics::{pixelcolor::Rgb565, prelude::*, primitives::Rectangle};
use pocketcw_logic::boxing::{Foe, Me, Side};
use pocketcw_logic::text::Text;

use crate::fonts::SMALL;
use crate::art::MAX_W;
use crate::gfx::{glyph, mix, rgb};
use crate::lcd::Lcd;
use crate::sprites::{
    ARM, EXTRA, FACE, FIGHTER_PALETTES, FX, FX_PALETTE, GEAR, GLYPH, HEAD, HERO_ARM, HERO_BODY, LEGS, LOGO, LOGO_PALETTE,
    LOOKS, PLAYER_PALETTE, Palette, STAGE_PALETTES, STAGE_TILE, SWEAT_PALETTE, Sprite, TORSO,
};

/// The art is half the panel's size in each direction.
pub const ART_W: i32 = 160;
pub const ART_H: i32 = 120;
/// The ring starts below the scoreboard.
pub const STAGE_Y: i32 = 16;
/// The side panels cover the arena above this row, outside `WINDOW`.
pub const PANEL_BOTTOM: i32 = 59;
pub const WINDOW: (i32, i32) = (37, 123);

const FOE_X: i32 = 80;
const FOE_Y: i32 = 110;
const ME_X: i32 = 80;
const ME_Y: i32 = 117;
/// Your shoulder joints from between your feet, as tools/boxing_art.py draws them.
const MY_SHOULDER: (i32, i32) = (12, -32);
/// From the opponent's feet: the top of his torso, his neck, his shoulders, and the middle of
/// his face from the neck. These follow the grids in tools/boxing_art.py.
const TORSO_Y: i32 = -63;
const NECK_Y: i32 = -60;
const SHOULDER: (i32, i32) = (23, -51);
const FACE_Y: i32 = -15;
/// How far the kneeling body sits below the standing one.
const KNEEL_DROP: i32 = 18;
/// How far you slip to the side.
const DODGE_X: i32 = 32;

/// A place to fight: the backdrop tile behind the ring, and the ring's colors.
pub struct Stage {
    pub name: &'static str,
    wall: Rgb565,
    wall_low: Rgb565,
    barrier: Rgb565,
    barrier_edge: Rgb565,
    ropes: [Rgb565; 3],
    rope_shade: [Rgb565; 3],
    post: Rgb565,
    post_dark: Rgb565,
    pad: Rgb565,
    apron: Rgb565,
    apron_edge: Rgb565,
    mat_back: Rgb565,
    mat_front: Rgb565,
}

/// The stages, in the order of `STAGE_TILE` and `STAGE_PALETTES`.
pub static STAGES: [Stage; 4] = [
    Stage {
        name: "THE ARENA",
        wall: rgb(0x1A1426),
        wall_low: rgb(0x2A2038),
        barrier: rgb(0x0E0A14),
        barrier_edge: rgb(0x4A3C5A),
        ropes: [rgb(0xD83A3A), rgb(0xECECEC), rgb(0x3A5AD8)],
        rope_shade: [rgb(0x8A1E1E), rgb(0x9A9AA4), rgb(0x22348A)],
        post: rgb(0x9A9AA8),
        post_dark: rgb(0x4A4A58),
        pad: rgb(0xE8B020),
        apron: rgb(0x2E3E78),
        apron_edge: rgb(0xE8E8F0),
        mat_back: rgb(0x4C6CA2),
        mat_front: rgb(0x86A4D2),
    },
    Stage {
        name: "THE HAM SHACK",
        wall: rgb(0x2E1C10),
        wall_low: rgb(0x3D2616),
        barrier: rgb(0x1E120A),
        barrier_edge: rgb(0x9A6A3A),
        ropes: [rgb(0xF0C020), rgb(0x202020), rgb(0xF0C020)],
        rope_shade: [rgb(0x9A7810), rgb(0x080808), rgb(0x9A7810)],
        post: rgb(0xB08850),
        post_dark: rgb(0x5A3818),
        pad: rgb(0xD83A3A),
        apron: rgb(0x5A3818),
        apron_edge: rgb(0xE8D8B0),
        mat_back: rgb(0x8A6A40),
        mat_front: rgb(0xD2B482),
    },
    Stage {
        name: "FIELD DAY",
        wall: rgb(0x2E6A24),
        wall_low: rgb(0x3E8A30),
        barrier: rgb(0x6A5030),
        barrier_edge: rgb(0xD8B860),
        ropes: [rgb(0xF07A20), rgb(0xFFFFFF), rgb(0x2E6AD8)],
        rope_shade: [rgb(0x904A10), rgb(0xA0A0A8), rgb(0x1A3E80)],
        post: rgb(0xD0D0D8),
        post_dark: rgb(0x6A6A74),
        pad: rgb(0x3E8A30),
        apron: rgb(0x2E6AD8),
        apron_edge: rgb(0xFFFFFF),
        mat_back: rgb(0xA8ACB4),
        mat_front: rgb(0xF0F0F0),
    },
    Stage {
        name: "ROOFTOP DX",
        wall: rgb(0x14141E),
        wall_low: rgb(0x22222E),
        barrier: rgb(0x0A0A10),
        barrier_edge: rgb(0x6A6A7A),
        ropes: [rgb(0xFF2EA0), rgb(0x30F0FF), rgb(0xA0FF30)],
        rope_shade: [rgb(0x901860), rgb(0x188890), rgb(0x589018)],
        post: rgb(0x6A6A7A),
        post_dark: rgb(0x34343E),
        pad: rgb(0xFF2EA0),
        apron: rgb(0x101018),
        apron_edge: rgb(0x30F0FF),
        mat_back: rgb(0x26263A),
        mat_front: rgb(0x4A4A66),
    },
];

const ROPE_Y: [i32; 3] = [46, 50, 54];
const MAT_Y: i32 = 59;
const SHADOW_PERCENT: u32 = 45;

/// What one slot draws.
#[derive(Clone, Copy)]
pub(super) struct Layer {
    sprite: &'static Sprite,
    /// Where the sprite's pivot goes.
    x: i16,
    y: i16,
    flip: bool,
    palette: &'static Palette,
    /// Every pixel one color: white for the flash of a landed punch, dark for a fighter you
    /// have not met.
    tint: Option<Rgb565>,
    /// Art pixels per sprite pixel.
    scale: u8,
}

/// Equal when it draws the same: the same sprite and palette, by address.
impl PartialEq for Layer {
    fn eq(&self, o: &Layer) -> bool {
        core::ptr::eq(self.sprite, o.sprite)
            && core::ptr::eq(self.palette, o.palette)
            && (self.x, self.y, self.flip, self.tint, self.scale) == (o.x, o.y, o.flip, o.tint, o.scale)
    }
}

impl Layer {
    pub(super) fn new(sprite: &'static Sprite, x: i32, y: i32, palette: &'static Palette) -> Layer {
        Layer { sprite, x: x as i16, y: y as i16, flip: false, palette, tint: None, scale: 1 }
    }

    pub(super) fn flipped(self, flip: bool) -> Layer {
        Layer { flip, ..self }
    }

    fn flashing(self, flash: bool) -> Layer {
        Layer { tint: flash.then_some(Rgb565::WHITE), ..self }
    }

    pub(super) fn tinted(self, tint: Option<Rgb565>) -> Layer {
        Layer { tint, ..self }
    }

    pub(super) fn scaled(self, scale: u8) -> Layer {
        Layer { scale, ..self }
    }

    pub(super) fn bounds(&self) -> Rect {
        let (s, k) = (self.sprite, self.scale as i32);
        let (w, h, ox, oy) = (s.w as i32 * k, s.h as i32 * k, s.ox as i32 * k, s.oy as i32 * k);
        let left = if self.flip { self.x as i32 + ox + 1 - w } else { self.x as i32 - ox };
        let top = self.y as i32 - oy;
        Rect { x0: left, y0: top, x1: left + w, y1: top + h }
    }

    /// Draws art row `y` of the sprite into `row`, which starts at art column `x0`.
    pub(super) fn compose(&self, y: i32, x0: i32, row: &mut [Rgb565]) {
        let s = self.sprite;
        let b = self.bounds();
        if y < b.y0 || y >= b.y1 {
            return;
        }
        let k = self.scale as i32;
        let from = b.x0.max(x0);
        let to = b.x1.min(x0 + row.len() as i32);
        if from >= to {
            return;
        }
        let mut px = [0u8; MAX_W];
        s.row(((y - b.y0) / k) as usize, &mut px);
        for x in from..to {
            let col = (if self.flip { b.x1 - 1 - x } else { x - b.x0 } / k) as usize;
            let v = px[col];
            if v != 0 {
                row[(x - x0) as usize] = self.tint.unwrap_or(self.palette[v as usize]);
            }
        }
    }
}

/// A rectangle in art pixels, end exclusive.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Rect {
    pub(super) fn union(self, o: Rect) -> Rect {
        Rect { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
    }

    pub(super) fn area(&self) -> i32 {
        (self.x1 - self.x0).max(0) * (self.y1 - self.y0).max(0)
    }

    /// Close enough that one rectangle around both costs little more than two.
    fn near(&self, o: &Rect) -> bool {
        self.union(*o).area() <= self.area() + o.area() + 64
    }

    pub(super) fn clip(self, to: Rect) -> Rect {
        Rect { x0: self.x0.max(to.x0), y0: self.y0.max(to.y0), x1: self.x1.min(to.x1), y1: self.y1.min(to.y1) }
    }
}

/// Big block letters across the ring.
#[derive(Clone, Copy, PartialEq)]
pub struct Banner {
    pub text: Text<24>,
    pub color: Rgb565,
    /// Top row, in art pixels.
    pub y: i32,
    /// Art pixels per block.
    pub px: i32,
    /// The outline.
    pub edge: Rgb565,
}

impl Banner {
    fn width(&self) -> i32 {
        self.text.len() as i32 * 6 * self.px - self.px
    }

    pub(super) fn bounds(&self) -> Rect {
        let x0 = (ART_W - self.width()) / 2;
        Rect { x0: x0 - 1, y0: self.y - 1, x1: x0 + self.width() + 3, y1: self.y + 7 * self.px + 3 }
    }

    /// The letters with a dark outline and a lighter top half.
    pub(super) fn compose(&self, y: i32, x0: i32, row: &mut [Rgb565]) {
        let b = self.bounds();
        if y < b.y0 || y >= b.y1 {
            return;
        }
        let left = b.x0 + 1;
        let on = |x: i32, y: i32| -> bool {
            let (dx, dy) = (x - left, y - self.y);
            if dx < 0 || dy < 0 || dy >= 7 * self.px {
                return false;
            }
            let (cell, col) = (dx / (6 * self.px), dx % (6 * self.px) / self.px);
            if col >= 5 {
                return false;
            }
            let Some(c) = self.text.as_str().as_bytes().get(cell as usize) else { return false };
            glyph(*c as char)[(dy / self.px) as usize] & (0b10000 >> col) != 0
        };
        let top = mix(self.color, Rgb565::WHITE, 45);
        for x in b.x0.max(x0)..b.x1.min(x0 + row.len() as i32) {
            let i = (x - x0) as usize;
            if on(x, y) {
                row[i] = if y - self.y < 3 * self.px { top } else { self.color };
            } else if on(x - 1, y) || on(x + 1, y) || on(x, y - 1) || on(x, y + 1) || on(x - 1, y - 1) || on(x - 2, y - 2) {
                row[i] = self.edge;
            }
        }
    }
}

/// Every part, in drawing order.
#[derive(Clone, Copy)]
enum Slot {
    FoeLegs,
    FoeTorso,
    Emblem,
    FoeHead,
    FoeFace,
    FoeGear,
    FoeArmBack,
    FoeArmFront,
    FoeFx1,
    FoeFx2,
    FoeFx3,
    // Seen from behind, your arms are in front of you: your head, back and legs cover them.
    MyArmLeft,
    MyArmRight,
    MyBody,
    MyFx,
}
const SLOTS: usize = Slot::MyFx as usize + 1;

/// One frame of the ring.
#[derive(Clone, Copy, PartialEq)]
pub struct Scene {
    layers: [Option<Layer>; SLOTS],
    /// The middle of the opponent's shadow, when he stands.
    shadow: Option<i16>,
    banner: Option<Banner>,
    stage: u8,
}

/// Who stands where, and what they are doing.
pub struct Fighters {
    /// The opponent's ladder index.
    pub fighter: usize,
    pub foe: (Foe, u32),
    pub me: (Me, u32),
    /// Low health: the opponent looks it.
    pub foe_hurting: bool,
    pub now: u32,
    /// Which of `STAGES`.
    pub stage: usize,
}

impl Default for Scene {
    fn default() -> Self {
        Scene::empty()
    }
}

impl Scene {
    pub const fn empty() -> Scene {
        Scene { layers: [None; SLOTS], shadow: None, banner: None, stage: 0 }
    }

    fn put(&mut self, slot: Slot, layer: Layer) {
        self.layers[slot as usize] = Some(layer);
    }

    pub fn set_banner(&mut self, banner: Option<Banner>) {
        self.banner = banner;
    }

    /// Lays out both fighters.
    pub fn new(f: &Fighters) -> Scene {
        let mut scene = Scene::empty();
        scene.stage = f.stage as u8;
        scene.foe(f);
        scene.me(f);
        scene
    }

    fn foe(&mut self, f: &Fighters) {
        let look = &LOOKS[f.fighter];
        let pal = &FIGHTER_PALETTES[f.fighter];
        let (pose, since) = f.foe;
        let t = f.now.wrapping_sub(since);
        let x = FOE_X;
        let y = FOE_Y;
        if pose == Foe::Out {
            self.put(Slot::FoeTorso, Layer::new(&EXTRA[1], x, y - 2, pal));
            self.dizzy(x, y - 34, f.now);
            return;
        }
        let rise = pose == Foe::Rise && t < 360;
        let down = pose == Foe::Down || rise;
        self.shadow = Some(x as i16);
        // Offsets of the body, the head and each arm from where they stand, and what they show.
        let bob = if (f.now / 260) % 4 < 2 { 0 } else { 1 };
        let (mut body, mut headd) = ((0, bob), (0, bob));
        let mut arms = [(0usize, (0, bob)), (0usize, (0, 1 - bob))];
        let mut face = if f.foe_hurting { 3 } else { 0 };
        let mut front = 1;
        let mut flash = false;
        let side = |s: Side| if s == Side::Left { 0 } else { 1 };
        let toward = |s: Side, n: i32| if s == Side::Left { -n } else { n };
        match pose {
            Foe::Idle | Foe::Out => {}
            Foe::Rise => {
                face = 0;
                body.1 = 0;
                headd.1 = 0;
            }
            Foe::Windup(s) => {
                // Leans away from the cocked arm, and shakes once the tell gets going.
                let shake = if t > 300 && (t / 90) % 2 == 0 { 1 } else { 0 };
                body = (toward(s, -2) + shake, 0);
                headd = (toward(s, -3) + shake, -1);
                arms[side(s)] = (1, (toward(s, -2), -1));
                arms[1 - side(s)].1 = (toward(s, -2), 0);
                front = side(s);
                face = 1;
            }
            Foe::Punch(s) => {
                body = (toward(s, -2), 2);
                headd = (toward(s, -2), 3);
                arms[side(s)] = (2, (toward(s, -2), 2));
                arms[1 - side(s)].1 = (toward(s, -2), 2);
                front = side(s);
                face = 1;
            }
            Foe::Whiff(s) => {
                let k = (t as i32 / 40).min(4);
                body = (toward(s, -k - 2), 2);
                headd = (toward(s, -2 * k - 2), 3);
                arms[side(s)] = (3, (toward(s, -k - 2), 2));
                arms[1 - side(s)] = (4, (toward(s, -k), 2));
                front = side(s);
                face = 2;
            }
            Foe::Open => {
                let sway = [0, 1, 2, 1, 0, -1, -2, -1][(f.now / 140 % 8) as usize];
                body = (sway / 2, 2);
                headd = (sway, 3);
                arms = [(4, (sway / 2, 2)), (4, (sway / 2, 2))];
                face = 3;
                self.sweat(x + headd.0, y + NECK_Y + headd.1 + FACE_Y, f.now);
            }
            Foe::Hurt(s) => {
                // The head snaps away from the punch and eases back.
                let snap = if t < 120 { 5 } else if t < 240 { 3 } else { 1 };
                body = (toward(s, -1), 1);
                headd = (toward(s, -snap), 2);
                arms = [(4, (toward(s, -1), 1)), (4, (toward(s, -1), 1))];
                face = 2;
                flash = t < 70;
                if t < 160 {
                    let hx = x + headd.0 + toward(s, 9);
                    self.put(Slot::FoeFx1, Layer::new(&FX[2], hx, y + NECK_Y + FACE_Y + 2, &FX_PALETTE));
                }
            }
            Foe::Block => {
                body = (0, 1);
                headd = (0, 3);
                arms = [(5, (0, 1)), (5, (0, 1))];
                face = 1;
            }
            Foe::Stunned => {
                let sway = [0, 1, 1, 0, -1, -1][(f.now / 160 % 6) as usize];
                body = (0, 2);
                headd = (sway, 3);
                arms = [(4, (0, 2)), (4, (0, 2))];
                face = 3;
            }
            Foe::Down => {}
            Foe::Victory => {
                let hop = if (t / 180) % 2 == 0 { 0 } else { -2 };
                body = (0, hop);
                headd = (0, hop);
                arms = [(6, (0, hop)), (6, (0, hop))];
                face = 4;
            }
        }
        if down {
            body = (0, KNEEL_DROP);
            headd = (3, KNEEL_DROP + 3);
            arms = [(4, (0, KNEEL_DROP)), (4, (1, KNEEL_DROP))];
            face = 3;
        }
        let legs = if down { look.kneel } else { look.legs };
        self.put(Slot::FoeLegs, Layer::new(&LEGS[legs as usize], x, y, pal));
        let (bx, by) = (x + body.0, y + TORSO_Y + body.1);
        self.put(Slot::FoeTorso, Layer::new(&TORSO[look.torso as usize], bx, by, pal));
        if look.emblem {
            self.put(Slot::Emblem, Layer::new(&EXTRA[0], bx, by + 17, pal));
        }
        let (hx, hy) = (x + headd.0, y + NECK_Y + headd.1);
        self.put(Slot::FoeHead, Layer::new(&HEAD[f.fighter], hx, hy, pal).flashing(flash));
        self.put(Slot::FoeFace, Layer::new(&FACE[face], hx, hy, pal));
        self.put(Slot::FoeGear, Layer::new(&GEAR[f.fighter], hx, hy, pal));
        for (k, &(art, (dx, dy))) in arms.iter().enumerate() {
            let sx = if k == 0 { -SHOULDER.0 } else { SHOULDER.0 - 1 };
            let layer = Layer::new(&ARM[art], x + sx + dx, y + SHOULDER.1 + dy, pal).flipped(k == 1);
            self.put(if k == front { Slot::FoeArmFront } else { Slot::FoeArmBack }, layer);
        }
        if down || pose == Foe::Stunned {
            self.dizzy(hx, hy + FACE_Y - 18, f.now);
        }
    }

    /// Three stars circling over a head.
    fn dizzy(&mut self, x: i32, y: i32, now: u32) {
        const ORBIT: [(i8, i8); 12] =
            [(12, 0), (10, 2), (6, 3), (0, 4), (-6, 3), (-10, 2), (-12, 0), (-10, -2), (-6, -3), (0, -4), (6, -3), (10, -2)];
        for (k, slot) in [Slot::FoeFx1, Slot::FoeFx2, Slot::FoeFx3].into_iter().enumerate() {
            let (dx, dy) = ORBIT[((now / 90) as usize + k * 4) % ORBIT.len()];
            self.put(slot, Layer::new(&FX[0], x + dx as i32, y + dy as i32, &FX_PALETTE));
        }
    }

    /// Sweat flying off a dazed head.
    fn sweat(&mut self, x: i32, y: i32, now: u32) {
        let t = (now % 600) as i32;
        let d = t / 60;
        self.put(Slot::FoeFx2, Layer::new(&FX[3], x - 12 - d, y - 6 + d / 2, &SWEAT_PALETTE));
        self.put(Slot::FoeFx3, Layer::new(&FX[3], x + 12 + d, y - 8 + d / 3, &SWEAT_PALETTE));
    }

    fn me(&mut self, f: &Fighters) {
        // Arms in HERO_ARM.
        const GUARD: usize = 0;
        const JAB: usize = 1;
        const HOOK: usize = 2;
        const UPPER: usize = 3;
        const RAISE: usize = 4;
        const LOW: usize = 5;
        // The other hand, up in front of the face while one punches.
        const TUCK: usize = 6;
        let (pose, since) = f.me;
        let t = f.now.wrapping_sub(since);
        let bob = if (f.now / 300) % 2 == 0 { 0 } else { 1 };
        let (mut x, mut y) = (ME_X, ME_Y);
        // Each arm, left then right: its drawing and its offset from the shoulder.
        let mut arms = [(GUARD, (0, bob)), (GUARD, (0, bob))];
        let mut fx = None;
        let toward = |s: Side, n: i32| if s == Side::Left { -n } else { n };
        let ease = |t: u32, ms: u32, n: i32| n * t.min(ms) as i32 / ms as i32;
        let arm = |s: Side| if s == Side::Left { 0 } else { 1 };
        match pose {
            Me::Idle => {}
            Me::Dodge(s) => {
                x += toward(s, ease(t, 90, DODGE_X));
                y += 2;
            }
            Me::Jab(s) if t < 170 => {
                arms[arm(s)] = (JAB, (0, 0));
                arms[1 - arm(s)] = (TUCK, (0, 0));
                y -= 1;
            }
            Me::Blocked(s) => {
                arms[arm(s)] = (JAB, (0, if t < 120 { 0 } else { 3 }));
                arms[1 - arm(s)] = (TUCK, (0, 0));
                y += if t < 120 { 0 } else { 2 };
            }
            Me::Hook(s) if t < 240 => {
                // Out wide and round, clear of your own head.
                arms[arm(s)] = (HOOK, (if s == Side::Left { -4 } else { 4 }, -2));
                arms[1 - arm(s)] = (TUCK, (0, 0));
                x += toward(s, -2);
            }
            Me::Combo(n) => {
                // A jab, then hooks, hand after hand.
                let k = t / 150;
                if k < n as u32 {
                    let s = if k % 2 == 0 { Side::Left } else { Side::Right };
                    let out = if k == 0 { 0 } else if s == Side::Left { -4 } else { 4 };
                    arms[arm(s)] = (if k == 0 { JAB } else { HOOK }, (out, if k == 0 { 0 } else { -2 }));
                    arms[1 - arm(s)] = (TUCK, (0, 0));
                    x += toward(s, if k == 0 { 1 } else { -2 });
                }
            }
            Me::Uppercut => {
                if t < 160 {
                    y += 3;
                    arms = [(GUARD, (0, 3)), (GUARD, (0, 4))];
                } else if t < 560 {
                    y -= 2;
                    arms = [(TUCK, (0, 0)), (UPPER, (0, 0))];
                    if (200..380).contains(&t) {
                        fx = Some(Layer::new(&FX[2], FOE_X + 4, FOE_Y + NECK_Y - 4, &FX_PALETTE));
                    }
                }
            }
            Me::Miss => {
                arms = [(TUCK, (0, 0)), (HOOK, (3, 0))];
                x += 4;
            }
            Me::Hurt => {
                y += 3;
                if t < 240 {
                    x += if (t / 60) % 2 == 0 { -2 } else { 2 };
                }
                arms = [(LOW, (0, 0)), (LOW, (0, 0))];
                if t < 150 {
                    fx = Some(Layer::new(&FX[2], x, y - 46, &FX_PALETTE));
                }
            }
            Me::Down => {
                y += ease(t, 300, 30);
                arms = [(LOW, (0, 0)), (LOW, (0, 0))];
            }
            Me::Rise => {
                y += 30 - ease(t, 600, 30);
                if t < 400 {
                    arms = [(LOW, (0, 0)), (LOW, (0, 0))];
                }
            }
            Me::Victory => {
                y -= if (t / 200) % 2 == 0 { 0 } else { 2 };
                arms = [(RAISE, (0, 0)), (RAISE, (0, 0))];
            }
            Me::Jab(_) | Me::Hook(_) => {}
        }
        let pal = &PLAYER_PALETTE;
        self.put(Slot::MyBody, Layer::new(&HERO_BODY[0], x, y, pal));
        for (k, &(art, (dx, dy))) in arms.iter().enumerate() {
            let sx = if k == 0 { -MY_SHOULDER.0 } else { MY_SHOULDER.0 - 1 };
            let layer = Layer::new(&HERO_ARM[art], x + sx + dx, y + MY_SHOULDER.1 + dy, pal).flipped(k == 1);
            self.put(if k == 0 { Slot::MyArmLeft } else { Slot::MyArmRight }, layer);
        }
        if let Some(fx) = fx {
            self.put(Slot::MyFx, fx);
        }
    }

    fn shadow_bounds(x: i16) -> Rect {
        Rect { x0: x as i32 - 23, y0: FOE_Y - 4, x1: x as i32 + 24, y1: FOE_Y + 4 }
    }

    /// Draws `area` of the ring as this scene shows it. Keeps out of the scoreboard and the side
    /// panels.
    pub fn draw(&self, lcd: &mut Lcd, area: Rect) {
        let upper = Rect { x0: WINDOW.0, y0: STAGE_Y, x1: WINDOW.1, y1: PANEL_BOTTOM };
        let lower = Rect { x0: 0, y0: PANEL_BOTTOM, x1: ART_W, y1: ART_H };
        for part in [area.clip(upper), area.clip(lower)] {
            if part.area() > 0 {
                self.render(lcd, part);
            }
        }
    }

    /// Draws the whole ring.
    pub fn draw_all(&self, lcd: &mut Lcd) {
        self.draw(lcd, Rect { x0: 0, y0: STAGE_Y, x1: ART_W, y1: ART_H });
    }

    /// Redraws what changed since `old`.
    pub fn update(&self, lcd: &mut Lcd, old: &Scene) {
        let mut dirty = Dirty::<4>::new(Rect { x0: 0, y0: STAGE_Y, x1: ART_W, y1: ART_H });
        for (new, old) in self.layers.iter().zip(&old.layers) {
            if new != old {
                for layer in [new, old].into_iter().flatten() {
                    dirty.add(layer.bounds());
                }
            }
        }
        if self.shadow != old.shadow {
            for x in [self.shadow, old.shadow].into_iter().flatten() {
                dirty.add(Self::shadow_bounds(x));
            }
        }
        if self.banner != old.banner {
            for b in [self.banner, old.banner].into_iter().flatten() {
                dirty.add(b.bounds());
            }
        }
        for rect in &dirty.rects[..dirty.len] {
            self.draw(lcd, *rect);
        }
    }

    fn render(&self, lcd: &mut Lcd, r: Rect) {
        stream(lcd, r, 2 * r.x0, 2 * r.y0, |y, x0, row| self.compose(y, x0, row));
    }

    fn compose(&self, y: i32, x0: i32, row: &mut [Rgb565]) {
        arena(y, x0, row, self.shadow, &STAGES[self.stage as usize], self.stage as usize);
        for layer in self.layers.iter().flatten() {
            layer.compose(y, x0, row);
        }
        if let Some(b) = &self.banner {
            b.compose(y, x0, row);
        }
    }
}

/// Sends art rectangle `r` to the panel at (`sx`, `sy`), each art pixel a 2 x 2 block, in one
/// window. `compose` fills one art row from a column.
pub(super) fn stream(lcd: &mut Lcd, r: Rect, sx: i32, sy: i32, mut compose: impl FnMut(i32, i32, &mut [Rgb565])) {
    let w = (r.x1 - r.x0) as usize;
    if w == 0 || r.y1 <= r.y0 {
        return;
    }
    let mut row = [Rgb565::BLACK; ART_W as usize];
    let mut y = r.y0;
    compose(y, r.x0, &mut row[..w]);
    // Each art row goes out twice, each pixel doubled.
    let (mut i, mut repeat) = (0, 0);
    let pixels = core::iter::from_fn(|| {
        if i == 2 * w {
            i = 0;
            repeat += 1;
            if repeat == 2 {
                repeat = 0;
                y += 1;
                if y >= r.y1 {
                    return None;
                }
                compose(y, r.x0, &mut row[..w]);
            }
        }
        let c = row[i / 2];
        i += 1;
        Some(c)
    });
    let area = Rectangle::new(Point::new(sx, sy), Size::new(2 * w as u32, 2 * (r.y1 - r.y0) as u32));
    let _ = lcd.fill_contiguous(&area, pixels);
}

/// A fighter's head on `bg`, centered on screen column `cx`, its bottom at screen row `bottom`.
/// Locked fighters show as a dark silhouette.
pub fn portrait(lcd: &mut Lcd, fighter: usize, face: usize, cx: i32, bottom: i32, bg: Rgb565, locked: bool) {
    let pal = &FIGHTER_PALETTES[fighter];
    let tint = locked.then_some(rgb(0x2A2434));
    let head = Layer::new(&HEAD[fighter], 0, 0, pal).tinted(tint);
    let face = (!locked).then(|| Layer::new(&FACE[face], 0, 0, pal));
    let gear = (!locked).then(|| Layer::new(&GEAR[fighter], 0, 0, pal));
    let b = [Some(head), face, gear].into_iter().flatten().fold(head.bounds(), |b, l| b.union(l.bounds()));
    let (sx, sy) = (cx - (b.x1 - b.x0), bottom - 2 * (b.y1 - b.y0));
    stream(lcd, b, sx, sy, |y, x0, row| {
        row.iter_mut().for_each(|p| *p = bg);
        for layer in [Some(head), face, gear].into_iter().flatten() {
            layer.compose(y, x0, row);
        }
    });
}

/// Up to `N` rectangles to redraw, inside `clip`; close ones merge.
pub(super) struct Dirty<const N: usize = 4> {
    pub(super) rects: [Rect; N],
    pub(super) len: usize,
    clip: Rect,
}

impl Default for Rect {
    fn default() -> Self {
        Rect { x0: 0, y0: 0, x1: 0, y1: 0 }
    }
}

impl<const N: usize> Dirty<N> {
    pub(super) fn new(clip: Rect) -> Dirty<N> {
        Dirty { rects: [Rect::default(); N], len: 0, clip }
    }

    pub(super) fn rects(&self) -> &[Rect] {
        &self.rects[..self.len]
    }

    pub(super) fn add(&mut self, mut r: Rect) {
        r = r.clip(self.clip);
        if r.area() == 0 {
            return;
        }
        // Merge with anything near, until nothing is.
        let mut k = 0;
        while k < self.len {
            if self.rects[k].near(&r) {
                r = r.union(self.rects[k]);
                self.len -= 1;
                self.rects[k] = self.rects[self.len];
                k = 0;
            } else {
                k += 1;
            }
        }
        if self.len == self.rects.len() {
            // Full: fold the new one into the rectangle it grows least.
            let best = (0..self.len).min_by_key(|&k| self.rects[k].union(r).area() - self.rects[k].area()).unwrap_or(0);
            let merged = self.rects[best].union(r);
            self.len -= 1;
            self.rects[best] = self.rects[self.len];
            return self.add(merged);
        }
        self.rects[self.len] = r;
        self.len += 1;
    }
}

/// One art row of the empty arena from column `x0`.
fn arena(y: i32, x0: i32, row: &mut [Rgb565], shadow: Option<i16>, st: &Stage, k: usize) {
    let n = row.len() as i32;
    let fill = |row: &mut [Rgb565], c: Rgb565| row.iter_mut().for_each(|p| *p = c);
    let crowd = &STAGE_TILE[k];
    let crowd_rows = crowd.h as i32;
    if y < STAGE_Y + crowd_rows {
        // The crowd, dimmer toward the back.
        let mut px = [0u8; MAX_W];
        crowd.row((y - STAGE_Y) as usize, &mut px);
        for (i, p) in row.iter_mut().enumerate() {
            let tx = (x0 + i as i32).rem_euclid(crowd.w as i32) as usize;
            let v = px[tx];
            *p = STAGE_PALETTES[k][v as usize];
        }
        return;
    }
    if y < MAT_Y {
        let base = match y - STAGE_Y - crowd_rows {
            0 => st.barrier_edge,
            1 | 2 => st.barrier,
            k => mix(st.wall, st.wall_low, (k * 100 / 14).clamp(0, 100) as u32),
        };
        fill(row, base);
        if y >= MAT_Y - 3 {
            fill(row, if y == MAT_Y - 3 { st.apron_edge } else { st.apron });
        }
        for (k, &ry) in ROPE_Y.iter().enumerate() {
            if y == ry || y == ry + 1 {
                let c = if y == ry { st.ropes[k] } else { st.rope_shade[k] };
                for x in 6.max(x0)..154.min(x0 + n) {
                    row[(x - x0) as usize] = c;
                }
            }
        }
        // Corner posts with their pads.
        for post in [4, 152] {
            for x in post.max(x0)..(post + 4).min(x0 + n) {
                let pad = ROPE_Y.iter().any(|&ry| (ry - 1..=ry + 2).contains(&y));
                row[(x - x0) as usize] = if pad {
                    st.pad
                } else if x == post || x == post + 3 {
                    st.post_dark
                } else {
                    st.post
                };
            }
        }
        return;
    }
    // The canvas, lighter toward you.
    let mat = mix(st.mat_back, st.mat_front, ((y - MAT_Y) * 100 / (ART_H - MAT_Y)) as u32);
    let dark = mix(mat, rgb(0x101828), SHADOW_PERCENT);
    for (i, p) in row.iter_mut().enumerate() {
        let x = x0 + i as i32;
        *p = mat;
        if let Some(sx) = shadow {
            let (dx, dy) = (x - sx as i32, y - FOE_Y);
            if dx * dx * 16 + dy * dy * 23 * 23 <= 23 * 23 * 16 {
                *p = dark;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The startup screen: the logo, BRASS KNUCKLE GANG punched in a letter line at a time, and
// impact stars going off all over the screen.

/// The art rows the splash animates; the motto band sits under them, at full resolution.
pub const SPLASH_H: i32 = 96;
/// The motto band and the prompt under it, in screen pixels.
pub const MOTTO_Y: i32 = 194;
pub const MOTTO_H: i32 = 28;
pub const PROMPT_Y: i32 = MOTTO_Y + MOTTO_H + 2;
/// When each part comes in, from the start of the splash.
pub const MOTTO_MS: u32 = 1500;
pub const PROMPT_MS: u32 = 2100;

const YELLOW: Rgb565 = rgb(0xFFE030);
const RED: Rgb565 = rgb(0xD81818);
const GREEN: Rgb565 = rgb(0x30FF50);
const BAND: Rgb565 = rgb(0x0A0306);
const STARS: usize = 12;
const LINES: [(&str, u32, i32); 3] = [("BRASS", 650, 43), ("KNUCKLE", 900, 60), ("GANG", 1150, 77)];

#[derive(Clone, Copy, PartialEq)]
pub struct Splash {
    stars: [Option<Layer>; STARS],
    logo: Option<Layer>,
    lines: [Option<Banner>; 3],
}

pub(super) fn hash(n: u32) -> u32 {
    let mut x = n.wrapping_mul(0x9E37_79B1) ^ 0x5BD1_E995;
    x ^= x >> 15;
    x = x.wrapping_mul(0x85EB_CA77);
    x ^= x >> 13;
    x = x.wrapping_mul(0xC2B2_AE3D);
    x ^ (x >> 16)
}

impl Splash {
    pub const fn empty() -> Splash {
        Splash { stars: [None; STARS], logo: None, lines: [None; 3] }
    }

    /// The splash `t` ms in; `seed` scatters the stars differently each time.
    pub fn new(t: u32, seed: u32) -> Splash {
        let mut s = Splash::empty();
        // Each star slot goes off once a cycle, at its own random place, size and moment.
        const CYCLE: u32 = 700;
        for (k, slot) in s.stars.iter_mut().enumerate() {
            let offset = k as u32 * CYCLE / STARS as u32;
            let Some(since) = t.checked_sub(offset / 3) else { continue };
            let (round, at) = (since / CYCLE, since % CYCLE);
            let h = hash(seed ^ (round << 8) ^ k as u32);
            let start = h % 200;
            if at < start || at >= start + 420 {
                continue;
            }
            let age = at - start;
            let scale = [1, 1, 1, 2, 2, 3][((h >> 8) % 6) as usize];
            let (x, y) = ((h >> 12) % 160, 4 + (h >> 20) % (SPLASH_H as u32 - 12));
            // A flash of a burst, then the star, which shrinks before it goes.
            let (sprite, scale) = if age < 90 {
                (&FX[2], scale)
            } else if age < 300 {
                (&FX[1], scale)
            } else {
                (&FX[0], scale.max(2) - 1)
            };
            *slot = Some(Layer::new(sprite, x as i32, y as i32, &FX_PALETTE).scaled(scale as u8));
        }
        // The logo drops in and bounces once.
        let logo_y = match t {
            0..=399 => -24 + 48 * t as i32 / 400,
            400..=499 => 24 - (t as i32 - 400) / 20,
            500..=599 => 19 + (t as i32 - 500) / 20,
            _ => 22,
        };
        s.logo = Some(Layer::new(&LOGO[0], 80, logo_y, &LOGO_PALETTE));
        for (k, &(text, at, y)) in LINES.iter().enumerate() {
            if t >= at {
                // Each line slams in big and settles.
                let px = if t - at < 90 { 3 } else { 2 };
                let y = if px == 3 { y - 4 } else { y };
                s.lines[k] = Some(Banner { text: Text::format(format_args!("{text}")), color: YELLOW, y, px, edge: RED });
            }
        }
        s
    }

    /// When a line has just landed: where to throw a big burst.
    pub fn slam(t: u32) -> bool {
        LINES.iter().any(|&(_, at, _)| t >= at && t - at < 90)
    }

    fn area() -> Rect {
        Rect { x0: 0, y0: 0, x1: ART_W, y1: SPLASH_H }
    }

    pub fn draw_all(&self, lcd: &mut Lcd) {
        self.render(lcd, Self::area());
    }

    pub fn update(&self, lcd: &mut Lcd, old: &Splash) {
        let mut dirty = Dirty::<4>::new(Self::area());
        let layers = |s: &Splash| s.stars.into_iter().chain([s.logo]);
        for (new, old) in layers(self).zip(layers(old)) {
            if new != old {
                for layer in [new, old].into_iter().flatten() {
                    dirty.add(layer.bounds());
                }
            }
        }
        for (new, old) in self.lines.iter().zip(&old.lines) {
            if new != old {
                for b in [new, old].into_iter().flatten() {
                    dirty.add(b.bounds());
                }
            }
        }
        for rect in &dirty.rects[..dirty.len] {
            self.render(lcd, *rect);
        }
    }

    fn render(&self, lcd: &mut Lcd, r: Rect) {
        stream(lcd, r, 2 * r.x0, 2 * r.y0, |y, x0, row| {
            splash_backdrop(y, x0, row);
            for layer in self.stars.iter().flatten() {
                layer.compose(y, x0, row);
            }
            if let Some(logo) = &self.logo {
                logo.compose(y, x0, row);
            }
            for b in self.lines.iter().flatten() {
                b.compose(y, x0, row);
            }
        });
    }
}

/// Dark red, lighter in diagonal stripes, darker toward the bottom.
fn splash_backdrop(y: i32, x0: i32, row: &mut [Rgb565]) {
    let base = mix(rgb(0x3A0A0C), rgb(0x0E0306), (y * 100 / SPLASH_H) as u32);
    let light = mix(base, rgb(0x6A1A14), 30);
    for (i, p) in row.iter_mut().enumerate() {
        let x = x0 + i as i32;
        *p = if (x + y).rem_euclid(16) < 8 { light } else { base };
    }
}

/// The motto, `彙★ 2-Meter CW or Bust!!! ★彙`, at full resolution: yellow letters with a red
/// shadow, and 彙 and ★ in bright green.
pub fn motto(lcd: &mut Lcd) {
    const TEXT: &str = " 2-Meter CW or Bust!!! ";
    let (hui, star) = (&GLYPH[0], &GLYPH[1]);
    let text_w = TEXT.len() as i32 * SMALL.width as i32;
    let width = 2 * (hui.w as i32 + 2 + star.w as i32) + text_w;
    let left = (2 * ART_W - width) / 2;
    // Each piece: its left edge, top, and what it is.
    let pieces = [
        (left, 2, Some(hui)),
        (left + hui.w as i32 + 2, 7, Some(star)),
        (left + width - hui.w as i32 - 2 - star.w as i32, 7, Some(star)),
        (left + width - hui.w as i32, 2, Some(hui)),
    ];
    let text_x = left + hui.w as i32 + 2 + star.w as i32;
    let text_y = (MOTTO_H - SMALL.height as i32) / 2;
    // Each piece's rows at y and y - 1, for its shadow, unpacked once per screen row.
    let rows = |s: &Sprite, y: i32| -> [u8; MAX_W] {
        let mut px = [0u8; MAX_W];
        if (0..s.h as i32).contains(&y) {
            s.row(y as usize, &mut px);
        }
        px
    };
    let on = |px: &[u8; MAX_W], s: &Sprite, x: i32| (0..s.w as i32).contains(&x) && px[x as usize] != 0;
    let ink = |x: i32, y: i32| -> u8 {
        let (dx, dy) = (x - text_x, y - text_y);
        if dx < 0 || dy < 0 || dx >= text_w {
            return 0;
        }
        let c = TEXT.as_bytes()[(dx / SMALL.width as i32) as usize];
        SMALL.coverage(c, dx % SMALL.width as i32, dy)
    };
    let mut row = [BAND; 2 * ART_W as usize];
    for y in 0..MOTTO_H {
        let unpacked = pieces.map(|(_, py, s)| {
            let s = s.unwrap();
            (rows(s, y - py), rows(s, y - py - 1))
        });
        for (x, p) in row.iter_mut().enumerate() {
            let x = x as i32;
            *p = if y == 0 || y == MOTTO_H - 1 { RED } else { BAND };
            for ((px, _, s), (here, above)) in pieces.iter().zip(&unpacked) {
                let s = s.unwrap();
                if on(here, s, x - px) {
                    *p = GREEN;
                } else if on(above, s, x - px - 1) {
                    *p = rgb(0x0A5A18);
                }
            }
            let shadow = ink(x - 1, y - 1);
            if shadow > 0 {
                *p = mix(*p, RED, shadow as u32 * 100 / 15);
            }
            let c = ink(x, y);
            if c > 0 {
                *p = mix(*p, YELLOW, c as u32 * 100 / 15);
            }
        }
        let area = Rectangle::new(Point::new(0, MOTTO_Y + y), Size::new(2 * ART_W as u32, 1));
        let _ = lcd.fill_contiguous(&area, row.iter().copied());
    }
}

/// The band under the motto, empty.
pub fn below_splash(lcd: &mut Lcd, top: i32) {
    let area = Rectangle::new(Point::new(0, top), Size::new(2 * ART_W as u32, (2 * ART_H - top).max(0) as u32));
    let _ = lcd.fill_solid(&area, BAND);
}

pub const SPLASH_BAND: Rgb565 = BAND;
pub const SPLASH_YELLOW: Rgb565 = YELLOW;
pub const SPLASH_RED: Rgb565 = RED;
