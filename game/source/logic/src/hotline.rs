//! The keyed line as VBand's CW Hotline sends it: each mark with the space before it.

/// VBand clamps marks and spaces to this, and plays a space of this length as the start of an
/// over after its own short wait. A longer space would hold every listener for its whole length.
pub const MAX_MS: u16 = 3000;

/// Times the keyed line one 1 ms tick at a time.
pub struct Marks {
    down: bool,
    /// Ticks since the line last changed.
    run_ms: u32,
    /// The space before the mark now down.
    space_ms: u16,
    /// A mark has ended since the start, so a space has a real length.
    marked: bool,
}

impl Marks {
    pub const fn new() -> Marks {
        Marks { down: false, run_ms: 0, space_ms: 0, marked: false }
    }

    /// The space and mark, in ms, when a mark ends on this tick.
    pub fn tick(&mut self, keyed: bool) -> Option<(u16, u16)> {
        self.run_ms = self.run_ms.saturating_add(1);
        if keyed == self.down {
            return None;
        }
        let run_ms = self.run_ms.min(MAX_MS as u32) as u16;
        (self.down, self.run_ms) = (keyed, 0);
        if keyed {
            self.space_ms = if self.marked { run_ms } else { MAX_MS };
            None
        } else {
            self.marked = true;
            Some((self.space_ms, run_ms))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lines that keying `runs` gives: (down, ms) pairs.
    fn lines(runs: &[(bool, u32)]) -> [Option<(u16, u16)>; 4] {
        let mut marks = Marks::new();
        let mut found = [None; 4];
        let mut n = 0;
        for &(down, ms) in runs {
            for _ in 0..ms {
                if let Some(line) = marks.tick(down) {
                    found[n] = Some(line);
                    n += 1;
                }
            }
        }
        found
    }

    #[test]
    fn each_mark_comes_at_its_end_with_the_space_before_it() {
        let found = lines(&[(false, 500), (true, 60), (false, 60), (true, 180), (false, 1)]);
        assert_eq!(found, [Some((MAX_MS, 60)), Some((60, 180)), None, None]);
    }

    #[test]
    fn the_first_mark_starts_an_over() {
        assert_eq!(lines(&[(true, 60), (false, 1)])[0], Some((MAX_MS, 60)));
    }

    #[test]
    fn long_spaces_and_marks_clamp() {
        let found = lines(&[(true, 60), (false, 60_000), (true, 5000), (false, 1)]);
        assert_eq!(found[1], Some((MAX_MS, MAX_MS)));
    }
}
