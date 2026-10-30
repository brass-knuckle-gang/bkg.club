//! Small pseudo-random generator for drills and bots: xorshift32.

#[derive(Clone, Copy)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Rng {
        Rng(seed | 1)
    }

    /// A number in 0..n.
    pub fn below(&mut self, n: u32) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 % n.max(1)
    }
}
