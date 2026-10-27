//! Times on the millisecond tick count, which wraps every 49 days. Two times compare correctly
//! while they are under 24 days apart.

const HALF: u32 = u32::MAX / 2;

/// Whether `now` is at or after `at`.
pub fn reached(now: u32, at: u32) -> bool {
    now.wrapping_sub(at) < HALF
}

/// Milliseconds from `now` until `at`; 0 once it is reached.
pub fn ms_until(now: u32, at: u32) -> u32 {
    if reached(now, at) { 0 } else { at.wrapping_sub(now) }
}

pub fn earlier(a: u32, b: u32) -> u32 {
    if reached(b, a) { a } else { b }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_across_the_wrap() {
        let before_wrap = u32::MAX - 10;
        let after_wrap = 5;
        assert!(reached(after_wrap, before_wrap) && !reached(before_wrap, after_wrap));
        assert!(reached(7, 7));
        assert_eq!(earlier(after_wrap, before_wrap), before_wrap);
        assert_eq!(earlier(before_wrap, after_wrap), before_wrap);
        assert_eq!(ms_until(before_wrap, after_wrap), 16);
        assert_eq!(ms_until(after_wrap, before_wrap), 0);
    }
}
