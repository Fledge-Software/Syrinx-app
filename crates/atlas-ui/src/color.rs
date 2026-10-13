//! An 8-bit sRGB colour with the conversions and WCAG contrast maths the tokens need.

/// One sRGB colour with straight (not premultiplied) alpha.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Color {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

impl Color {
    /// An opaque colour written as CSS `#RRGGBB`, given as `0xRRGGBB`.
    pub const fn hex(rgb: u32) -> Self {
        Self {
            red: (rgb >> 16) as u8,
            green: (rgb >> 8) as u8,
            blue: rgb as u8,
            alpha: 0xFF,
        }
    }

    /// A colour written as CSS `rgba(r, g, b, alpha)`.
    pub const fn rgba(red: u8, green: u8, blue: u8, alpha: f32) -> Self {
        Self {
            red,
            green,
            blue,
            alpha: unit_to_byte(alpha),
        }
    }

    /// The same colour with an 8-bit alpha, as in the canvas's `hex + 'C7'` suffixes.
    pub const fn with_alpha_byte(self, alpha: u8) -> Self {
        Self { alpha, ..self }
    }

    /// A colour written as CSS `hsl(h, s%, l%)`.
    pub fn hsl(hue_degrees: f64, saturation_percent: f64, lightness_percent: f64) -> Self {
        let saturation = (saturation_percent / 100.).clamp(0., 1.);
        let lightness = (lightness_percent / 100.).clamp(0., 1.);
        let hue = hue_degrees.rem_euclid(360.);

        // CSS Color 4's hsl-to-rgb: each channel samples a shifted triangle wave.
        let chroma_span = saturation * lightness.min(1. - lightness);
        let channel = |offset: f64| {
            let k = (offset + hue / 30.).rem_euclid(12.);
            let wave = (k - 3.).min(9. - k).clamp(-1., 1.);
            unit_to_byte_f64(lightness - chroma_span * wave)
        };
        Self {
            red: channel(0.),
            green: channel(8.),
            blue: channel(4.),
            alpha: 0xFF,
        }
    }

    /// Packs the colour as `0xRRGGBBAA`, the form GPUI's `rgba()` takes.
    pub const fn to_rgba_u32(self) -> u32 {
        (self.red as u32) << 24
            | (self.green as u32) << 16
            | (self.blue as u32) << 8
            | self.alpha as u32
    }

    /// Whether the colour lets anything behind it show through.
    pub const fn is_opaque(self) -> bool {
        self.alpha == 0xFF
    }

    /// Paints this colour over `background` with ordinary source-over blending.
    pub fn over(self, background: Color) -> Color {
        let top = f64::from(self.alpha) / 255.;
        let bottom = f64::from(background.alpha) / 255.;
        let alpha = top + bottom * (1. - top);
        if alpha == 0. {
            return Color::rgba(0, 0, 0, 0.);
        }
        let blend = |front: u8, back: u8| {
            let mixed = (f64::from(front) * top + f64::from(back) * bottom * (1. - top)) / alpha;
            mixed.round().clamp(0., 255.) as u8
        };
        Color {
            red: blend(self.red, background.red),
            green: blend(self.green, background.green),
            blue: blend(self.blue, background.blue),
            alpha: unit_to_byte_f64(alpha),
        }
    }

    /// WCAG 2.x relative luminance of the colour's RGB channels (alpha ignored).
    pub fn relative_luminance(self) -> f64 {
        let linear = |channel: u8| {
            let value = f64::from(channel) / 255.;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(self.red) + 0.7152 * linear(self.green) + 0.0722 * linear(self.blue)
    }

    /// The (hue degrees, saturation percent) pair the canvas derives from an accent.
    ///
    /// Both are rounded to whole numbers, matching the canvas's `tokens()` method.
    pub fn hue_and_saturation(self) -> (f64, f64) {
        let red = f64::from(self.red) / 255.;
        let green = f64::from(self.green) / 255.;
        let blue = f64::from(self.blue) / 255.;
        let max = red.max(green).max(blue);
        let min = red.min(green).min(blue);
        let delta = max - min;
        if delta == 0. {
            return (0., 0.);
        }

        // JavaScript's `%` truncates like Rust's, so negative sectors match the canvas.
        let sector = if max == red {
            ((green - blue) / delta) % 6.
        } else if max == green {
            (blue - red) / delta + 2.
        } else {
            (red - green) / delta + 4.
        };
        let hue = ((sector * 60. + 360.) % 360.).round();
        let lightness = (max + min) / 2.;
        let saturation = (delta / (1. - (2. * lightness - 1.).abs()) * 100.).round();
        (hue, saturation)
    }
}

/// WCAG 2.x contrast ratio between two opaque colours, from 1 to 21.
pub fn contrast_ratio(first: Color, second: Color) -> f64 {
    let first = first.relative_luminance();
    let second = second.relative_luminance();
    let (lighter, darker) = if first > second {
        (first, second)
    } else {
        (second, first)
    };
    (lighter + 0.05) / (darker + 0.05)
}

const fn unit_to_byte(unit: f32) -> u8 {
    // Const-friendly rounding of a 0–1 fraction to 0–255.
    let scaled = unit * 255. + 0.5;
    if scaled <= 0. {
        0
    } else if scaled >= 255. {
        255
    } else {
        scaled as u8
    }
}

fn unit_to_byte_f64(unit: f64) -> u8 {
    (unit * 255.).round().clamp(0., 255.) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_and_packing_round_trip() {
        let color = Color::hex(0xFF8A65);
        assert_eq!(
            (color.red, color.green, color.blue, color.alpha),
            (0xFF, 0x8A, 0x65, 0xFF)
        );
        assert_eq!(color.to_rgba_u32(), 0xFF8A65FF);
    }

    #[test]
    fn css_alpha_rounds_like_a_browser() {
        assert_eq!(Color::rgba(255, 255, 255, 0.09).alpha, 23);
        assert_eq!(Color::rgba(0, 0, 0, 0.5).alpha, 128);
        assert_eq!(Color::rgba(0, 0, 0, 1.).alpha, 255);
    }

    #[test]
    fn hsl_matches_known_css_values() {
        assert_eq!(Color::hsl(0., 100., 50.), Color::hex(0xFF0000));
        assert_eq!(Color::hsl(120., 100., 25.), Color::hex(0x008000));
        assert_eq!(Color::hsl(240., 100., 50.), Color::hex(0x0000FF));
        assert_eq!(Color::hsl(0., 0., 100.), Color::hex(0xFFFFFF));
    }

    #[test]
    fn default_accent_hue_and_saturation_match_the_canvas_formula() {
        assert_eq!(Color::hex(0xFF8A65).hue_and_saturation(), (14., 100.));
        assert_eq!(Color::hex(0x808080).hue_and_saturation(), (0., 0.));
    }

    #[test]
    fn blending_over_an_opaque_ground_is_opaque() {
        let row = Color::rgba(255, 255, 255, 0.5).over(Color::hex(0x000000));
        assert!(row.is_opaque());
        assert_eq!(row.red, 128);
    }

    #[test]
    fn contrast_ratio_spans_one_to_twenty_one() {
        let black = Color::hex(0x000000);
        let white = Color::hex(0xFFFFFF);
        assert!((contrast_ratio(black, white) - 21.).abs() < 1e-9);
        assert!((contrast_ratio(white, white) - 1.).abs() < 1e-9);
    }
}
