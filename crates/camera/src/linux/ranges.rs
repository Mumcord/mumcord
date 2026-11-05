//! Mode candidates inside the size and frame-rate ranges that PipeWire and V4L2 report as
//! continuous or stepped instead of as a list.

use crate::mode::Rate;

/// Sizes worth trying inside a continuous or stepped size range: the common camera sizes it
/// contains, and its largest size.
pub fn sizes_in_range(
    min: (u32, u32),
    max: (u32, u32),
    step: (u32, u32),
) -> impl Iterator<Item = (u32, u32)> {
    const COMMON: [(u32, u32); 7] = [
        (1280, 720),
        (960, 540),
        (640, 360),
        (1280, 960),
        (960, 720),
        (640, 480),
        (320, 240),
    ];
    let on_grid = move |value: u32, low: u32, high: u32, step: u32| {
        (low..=high).contains(&value) && (step <= 1 || (value - low).is_multiple_of(step))
    };
    COMMON
        .into_iter()
        .chain([max])
        .filter(move |&(w, h)| on_grid(w, min.0, max.0, step.0) && on_grid(h, min.1, max.1, step.1))
}

/// The rate to ask for inside a frame-rate range: the request's rate if the range has it,
/// otherwise the fastest one.
pub fn rate_in_range(min: Rate, max: Rate, request_fps: u32) -> Rate {
    let wanted = Rate::fps(request_fps);
    let at_least = |a: Rate, b: Rate| {
        u64::from(a.num) * u64::from(b.den) >= u64::from(b.num) * u64::from(a.den)
    };
    if at_least(max, wanted) && at_least(wanted, min) {
        wanted
    } else {
        max
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_offer_common_sizes_and_the_requested_rate() {
        let sizes: Vec<_> = sizes_in_range((320, 240), (1280, 720), (16, 8)).collect();
        assert!(sizes.contains(&(1280, 720)));
        assert!(sizes.contains(&(640, 480)));
        assert!(!sizes.contains(&(1280, 960)));
        assert_eq!(
            rate_in_range(Rate::fps(1), Rate::fps(60), 30),
            Rate::fps(30)
        );
        assert_eq!(
            rate_in_range(Rate::fps(1), Rate::fps(15), 30),
            Rate::fps(15)
        );
    }
}
