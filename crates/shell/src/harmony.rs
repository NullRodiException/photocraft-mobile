//! Colour harmonies: the palettes the colour popover makes from one base colour.
//!
//! - **Analogous:** the base and its neighbours on the colour wheel, [`ANALOGOUS_STEP`] degrees
//!   apart, two on each side.
//! - **Complementary:** the base and the hue opposite it, each with a darker and a lighter tone.
//! - **Monochromatic:** the base's hue in five tones, from dark to light.
//!
//! Colours are HSV, in PhotoCraft's colour picker units: hue in degrees, saturation and value
//! 0..1. A tone is darker (a shade: less value) or lighter toward white (a tint: less saturation,
//! more value). [`tones`] splits the steps between the two by how much room the base leaves on
//! each side, so black, white and the colours near them get distinct tones too.

/// Degrees between neighbouring analogous colours (one step of a 12-hue wheel).
pub const ANALOGOUS_STEP: f32 = 30.0;

/// How the colour popover makes a palette from the current colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Harmony {
    #[default]
    Analogous,
    Complementary,
    Monochromatic,
}

impl Harmony {
    pub const ALL: [Harmony; 3] = [Harmony::Analogous, Harmony::Complementary, Harmony::Monochromatic];

    /// The English and Portuguese names (a saved palette is named after them).
    pub fn names(self) -> (&'static str, &'static str) {
        match self {
            Harmony::Analogous => ("Analogous", "Análogas"),
            Harmony::Complementary => ("Complementary", "Complementares"),
            Harmony::Monochromatic => ("Monochromatic", "Monocromáticas"),
        }
    }

    /// The palette this harmony makes from `base` (HSV).
    pub fn palette(self, base: [f32; 3]) -> Palette {
        let [h, s, v] = clean(base);
        match self {
            Harmony::Analogous => Palette { colors: [-2.0, -1.0, 0.0, 1.0, 2.0].map(|k| [wrap(h + k * ANALOGOUS_STEP), s, v]).to_vec(), base: 2 },
            Harmony::Complementary => {
                let own = tones([h, s, v], 3);
                let other = tones([wrap(h + 180.0), s, v], 3);
                Palette { base: own.base, colors: own.colors.into_iter().chain(other.colors).collect() }
            }
            Harmony::Monochromatic => tones([h, s, v], 5),
        }
    }
}

/// Colours made from a base colour, and where the base sits among them.
#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    /// HSV colours, in the order shown.
    pub colors: Vec<[f32; 3]>,
    /// The base colour's position in `colors`.
    pub base: usize,
}

/// `n` tones of `hsv`'s hue from dark to light, `hsv` among them (at least `hsv` alone).
pub fn tones(hsv: [f32; 3], n: usize) -> Palette {
    let [h, s, v] = clean(hsv);
    let steps = n.saturating_sub(1);
    // Room to darken (value down to 0) and to lighten (toward white); together at least 0.5.
    let (down, up) = (v, s.max(1.0 - v));
    let below = ((steps as f32 * down / (down + up)).round() as usize).min(steps);
    let above = steps - below;
    let mut colors = Vec::with_capacity(steps + 1);
    for k in (1..=below).rev() {
        let f = k as f32 / (below + 1) as f32;
        colors.push([h, s, v * (1.0 - f)]);
    }
    colors.push([h, s, v]);
    for k in 1..=above {
        let f = k as f32 / (above + 1) as f32;
        colors.push([h, s * (1.0 - f), v + (1.0 - v) * f]);
    }
    Palette { colors, base: below }
}

/// The name of a new palette: `base` and the first number no taken name has ("Analogous 1").
pub fn palette_name(base: &str, taken: impl Fn(&str) -> bool) -> String {
    (1..).map(|i| format!("{base} {i}")).find(|n| !taken(n)).unwrap_or_else(|| base.to_string())
}

/// A hue in 0..360 (what `rem_euclid` rounds up to 360 wraps to 0).
fn wrap(h: f32) -> f32 {
    let h = h.rem_euclid(360.0);
    if h >= 360.0 { 0.0 } else { h }
}

/// `hsv` in range; what isn't finite becomes 0.
fn clean([h, s, v]: [f32; 3]) -> [f32; 3] {
    let unit = |x: f32| if x.is_finite() { x.clamp(0.0, 1.0) } else { 0.0 };
    [if h.is_finite() { wrap(h) } else { 0.0 }, unit(s), unit(v)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
    }

    #[test]
    fn analogous_colours_are_the_neighbours_on_the_wheel() {
        let p = Harmony::Analogous.palette([10.0, 0.8, 0.6]);
        let hues: Vec<f32> = p.colors.iter().map(|c| c[0]).collect();
        assert_eq!(hues, [310.0, 340.0, 10.0, 40.0, 70.0], "across 0°");
        assert_eq!(p.base, 2);
        assert!(p.colors.iter().all(|c| c[1] == 0.8 && c[2] == 0.6), "saturation and value stay the base's");
    }

    #[test]
    fn complementary_colours_pair_the_base_with_the_opposite_hue() {
        let p = Harmony::Complementary.palette([200.0, 1.0, 1.0]);
        assert_eq!(p.colors.len(), 6);
        assert!(near(p.colors[p.base], [200.0, 1.0, 1.0]));
        assert!(p.colors[..3].iter().all(|c| c[0] == 200.0) && p.colors[3..].iter().all(|c| c[0] == 20.0), "{:?}", p.colors);
        assert!(near(p.colors[3 + p.base], [20.0, 1.0, 1.0]), "the complement in the same place");
        assert!(near(p.colors[0], [200.0, 1.0, 0.5]) && near(p.colors[2], [200.0, 0.5, 1.0]), "a shade and a tint of each");
    }

    #[test]
    fn monochromatic_tones_go_from_dark_to_light_around_the_base() {
        let p = Harmony::Monochromatic.palette([0.0, 1.0, 1.0]);
        assert_eq!(p.base, 2, "pure red has as much room each way");
        let expect = [[0.0, 1.0, 1.0 / 3.0], [0.0, 1.0, 2.0 / 3.0], [0.0, 1.0, 1.0], [0.0, 2.0 / 3.0, 1.0], [0.0, 1.0 / 3.0, 1.0]];
        assert!(p.colors.iter().zip(expect).all(|(a, b)| near(*a, b)), "{:?}", p.colors);
        // A dark navy has little room to darken: one shade, three tints.
        let navy = Harmony::Monochromatic.palette([240.0, 0.8, 0.3]);
        assert_eq!(navy.base, 1);
        assert!(navy.colors.iter().all(|c| c[0] == 240.0));
        let value: Vec<f32> = navy.colors.iter().map(|c| c[2]).collect();
        assert!(value.windows(2).all(|w| w[0] < w[1]), "lighter each step: {value:?}");
    }

    #[test]
    fn black_and_white_still_make_distinct_tones() {
        let black = tones([0.0, 0.0, 0.0], 5);
        assert_eq!(black.base, 0, "black can only lighten");
        let white = tones([0.0, 0.0, 1.0], 5);
        assert_eq!(white.base, 4, "white can only darken");
        for p in [black, white] {
            let value: Vec<f32> = p.colors.iter().map(|c| c[2]).collect();
            assert!(value.windows(2).all(|w| w[0] < w[1]), "{value:?}");
        }
    }

    #[test]
    fn nonsense_stays_in_range() {
        for base in [[f32::NAN, 2.0, -1.0], [-30.0, f32::INFINITY, 0.5], [-1e-8, 0.5, 0.5], [7200.0, 0.5, f32::NEG_INFINITY]] {
            for h in Harmony::ALL {
                let p = h.palette(base);
                assert!(p.base < p.colors.len());
                for c in &p.colors {
                    assert!((0.0..360.0).contains(&c[0]) && (0.0..=1.0).contains(&c[1]) && (0.0..=1.0).contains(&c[2]), "{h:?} {base:?} → {c:?}");
                }
            }
        }
        assert_eq!(tones([0.0, 1.0, 1.0], 0).colors.len(), 1, "the base alone");
    }

    #[test]
    fn a_new_palette_takes_the_first_free_number() {
        assert_eq!(palette_name("Analogous", |_| false), "Analogous 1");
        let taken = ["Analogous 1", "Analogous 3"];
        assert_eq!(palette_name("Analogous", |n| taken.contains(&n)), "Analogous 2");
    }
}
