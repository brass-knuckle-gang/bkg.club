pub mod battery {
    #[derive(Clone, Copy, Default)]
    pub struct Reading { pub raw: u16, pub supply_mv: u16 }
}
pub mod codec {
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub enum Codec { Es8388, Wm8978 }
    impl Codec { pub fn name(self) -> &'static str { "ES8388" } }
}
pub mod clock {
    pub const SYSCLK_HZ: u32 = 72_000_000;
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub enum Source { Crystal, Internal }
    impl Source { pub fn name(self) -> &'static str { "XTAL" } }
}
pub mod link {
    use core::sync::atomic::AtomicU16;
    pub static ERRORS: AtomicU16 = AtomicU16::new(0);
}
pub mod audio {
    pub const SAMPLE_CYCLES: u32 = 4096;
    pub fn take_worst_cycles() -> u32 { 0 }
}
pub mod pack {
    #![allow(dead_code)]
    use crate::lcd::Lcd;
    use pocketcw_logic::pack::Word;
    pub struct Pack { pub factor: u8, pub voiced: bool }
    impl Pack {
        pub fn count(&self) -> u16 { 0 }
        pub fn words(&self) -> (u16, u16) { (0, 0) }
        pub fn word(&self, _: &mut Lcd, _: u16) -> Option<Word> { None }
        pub fn find(&self, _: &mut Lcd, _: &[u8]) -> Option<Word> { None }
        pub fn char(&self, _: &mut Lcd, _: u8) -> Option<Word> { None }
    }
}
