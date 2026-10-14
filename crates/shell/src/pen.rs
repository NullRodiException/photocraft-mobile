//! Pen and memory helpers for touch platforms, kept here (pure and tested) rather than in the
//! Android crate, which only builds for Android.

/// Android's stylus angles as PhotoCraft's tilt (Pointer Events' `tiltX`/`tiltY`, degrees in
/// −90…90): `tilt` is `AXIS_TILT` (radians from perpendicular) and `orientation` is
/// `AXIS_ORIENTATION` (radians, 0 = pointing up the screen, clockwise positive). Same conversion
/// as Chromium's Android pointer events.
pub fn tilt_from_android(tilt: f32, orientation: f32) -> (f32, f32) {
    if !(tilt.is_finite() && orientation.is_finite()) {
        return (0.0, 0.0);
    }
    let tilt = tilt.clamp(0.0, std::f32::consts::FRAC_PI_2);
    let (r, z) = (tilt.sin(), tilt.cos());
    let tilt_x = ((-orientation).sin() * r).atan2(z).to_degrees();
    let tilt_y = ((-orientation).cos() * r).atan2(z).to_degrees();
    (tilt_x.clamp(-90.0, 90.0), tilt_y.clamp(-90.0, 90.0))
}

/// Preferences › Performance › Memory Usage for a device with `total_mb` of RAM: a third of it,
/// between 1 and 4 GB in 256 MB steps. A phone or tablet shares that RAM with the system and other
/// apps, and Android ends an app that takes too much; PhotoCraft's desktop default is 8 GB.
pub fn memory_budget_mb(total_mb: u64) -> u32 {
    let third = total_mb / 3;
    let stepped = third / 256 * 256;
    stepped.clamp(1024, 4096) as u32
}

/// `MemTotal` from `/proc/meminfo` text, in MB.
pub fn mem_total_mb(meminfo: &str) -> Option<u64> {
    let line = meminfo.lines().find(|l| l.starts_with("MemTotal:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};

    #[test]
    fn upright_pen_has_no_tilt() {
        assert_eq!(tilt_from_android(0.0, 1.0), (0.0, 0.0));
    }

    #[test]
    fn tilt_follows_the_orientation() {
        // Leaning 45° with orientation 0 (up the screen): all of it on the y axis.
        let (x, y) = tilt_from_android(FRAC_PI_4, 0.0);
        assert!(x.abs() < 1e-3 && (y - 45.0).abs() < 1e-3, "{x} {y}");
        // Orientation a quarter turn clockwise: all of it on the x axis, the other sign.
        let (x, y) = tilt_from_android(FRAC_PI_4, FRAC_PI_2);
        assert!((x + 45.0).abs() < 1e-3 && y.abs() < 1e-3, "{x} {y}");
    }

    #[test]
    fn nonsense_angles_are_upright() {
        assert_eq!(tilt_from_android(f32::NAN, 0.0), (0.0, 0.0));
        let (x, y) = tilt_from_android(10.0, 0.0);
        assert!(x.abs() <= 90.0 && y.abs() <= 90.0);
    }

    #[test]
    fn memory_budget_is_a_third_within_limits() {
        assert_eq!(memory_budget_mb(12 * 1024), 4096);
        assert_eq!(memory_budget_mb(8 * 1024), 2560);
        assert_eq!(memory_budget_mb(2 * 1024), 1024);
        assert_eq!(memory_budget_mb(64 * 1024), 4096);
    }

    #[test]
    fn reads_mem_total() {
        assert_eq!(mem_total_mb("MemTotal:       11534336 kB\nMemFree: 1 kB\n"), Some(11264));
        assert_eq!(mem_total_mb("nothing"), None);
    }
}
