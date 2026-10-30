//! Math that `core` lacks for `f32`, and a sort smaller than `core`'s.

/// Square root by Newton's method.
pub fn sqrt(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    let mut g = x.max(1.0);
    for _ in 0..20 {
        g = 0.5 * (g + x / g);
    }
    g
}

/// Stable insertion sort, for slices of a few dozen items. `core`'s sorts add about 0.8 KB.
pub fn sort_by_key<T: Copy, K: Ord>(items: &mut [T], key: impl Fn(&T) -> K) {
    for i in 1..items.len() {
        let item = items[i];
        let mut j = i;
        while j > 0 && key(&items[j - 1]) > key(&item) {
            items[j] = items[j - 1];
            j -= 1;
        }
        items[j] = item;
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::rng::Rng;

    #[test]
    fn sort_matches_std_stable_sort() {
        let mut rng = Rng::new(7);
        for len in 0..40 {
            let items: std::vec::Vec<(u32, usize)> = (0..len).map(|i| (rng.below(5), i)).collect();
            let (mut ours, mut std) = (items.clone(), items);
            sort_by_key(&mut ours, |p| p.0);
            std.sort_by_key(|p| p.0);
            assert_eq!(ours, std);
        }
    }

    #[test]
    fn sqrt_matches_std_across_the_range_used() {
        for x in [1e-6f32, 0.25, 2.0, 9.0, 1e4, 1e6] {
            assert!((sqrt(x) - x.sqrt()).abs() <= x.sqrt() * 1e-6, "{x}");
        }
        assert_eq!(sqrt(-1.0), 0.0);
    }
}
