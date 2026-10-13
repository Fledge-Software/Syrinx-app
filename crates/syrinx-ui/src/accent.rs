//! The owner's accent colour and the states derived from it, as the design canvas computes them.

use crate::color::Color;

/// The default accent, the logo's orange `#FF612E` (2026-10-13; the spec's `#FF8A65` before).
pub const DEFAULT_ACCENT: Color = Color::hex(0xFF612E);

/// The accent swatches the canvas's Customize panel offers.
pub const ACCENT_PRESETS: [Color; 4] = [
    DEFAULT_ACCENT,
    Color::hex(0x6EA8FF),
    Color::hex(0x6FD3A8),
    Color::hex(0xE8C16A),
];

/// The lowest accent-ink lightness, in percent, on the see-through Haze and Glass palettes.
const SEE_THROUGH_INK_LIGHTNESS: f64 = 70.;

/// Dark text on accent-filled badges (the canvas's `badgeText`).
pub const BADGE_TEXT: Color = Color::hex(0x1F120D);

/// A user-chosen accent. Only opaque colours are accepted, as in the canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Accent(Color);

impl Default for Accent {
    fn default() -> Self {
        Self(DEFAULT_ACCENT)
    }
}

impl Accent {
    /// Accepts an opaque colour; a translucent one falls back to the default, like the canvas.
    pub fn new(color: Color) -> Self {
        if color.is_opaque() {
            Self(color)
        } else {
            Self::default()
        }
    }

    /// The colour as chosen: badge fills and selection marks.
    pub const fn fill(self) -> Color {
        self.0
    }

    /// The accent's hue and saturation, which the Tint palette and Paper's ink reuse.
    pub fn hue_and_saturation(self) -> (f64, f64) {
        self.0.hue_and_saturation()
    }

    /// An HSL colour in the accent's hue, its saturation capped at the accent's own.
    pub(crate) fn tone(self, saturation_percent: f64, lightness_percent: f64) -> Color {
        let (hue, saturation) = self.hue_and_saturation();
        Color::hsl(hue, saturation_percent.min(saturation), lightness_percent)
    }
}

/// The accent's derived states for one palette (canvas `ink`, `soft`, `softLine`, `ring`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccentColors {
    /// The chosen colour itself: badges and selection marks.
    pub fill: Color,
    /// Accent-coloured text and icons; darkened on Paper so it stays legible.
    pub ink: Color,
    /// A faint accent wash behind the selected or default (Enter) choice.
    pub soft: Color,
    /// The outline drawn around `soft`.
    pub soft_line: Color,
    /// A focus ring.
    pub ring: Color,
    /// Text drawn on `fill`.
    pub on_fill: Color,
}

impl AccentColors {
    /// The dark-palette derivation: `hex`, `hex + 26`, `hex + 8C`, `hex + 33`.
    pub fn dark(accent: Accent) -> Self {
        let fill = accent.fill();
        Self {
            fill,
            ink: fill,
            soft: fill.with_alpha_byte(0x26),
            soft_line: fill.with_alpha_byte(0x8C),
            ring: fill.with_alpha_byte(0x33),
            on_fill: BADGE_TEXT,
        }
    }

    /// Haze and Glass: as dark, but ink is lifted to at least 70% lightness in the accent's hue.
    ///
    /// Their see-through grounds are lighter than Graphite's, and the logo orange `#FF612E`
    /// (59% lightness) fell below 4.5:1 as text on Haze and Glass's selected palette rows and
    /// below 3:1 as a mark on Glass. 70% rounds the former default `#FF8A65`'s 69.8%, which
    /// passed; accents at least that light keep their own colour (2026-10-13 decision).
    pub fn see_through(accent: Accent) -> Self {
        let fill = accent.fill();
        let ink = if fill.lightness_percent() >= SEE_THROUGH_INK_LIGHTNESS {
            fill
        } else {
            accent.tone(100., SEE_THROUGH_INK_LIGHTNESS)
        };
        Self {
            ink,
            ..Self::dark(accent)
        }
    }

    /// Paper's derivation: ink becomes `hsl(h, min(85, s)%, 36%)` and the wash `hex + 24`.
    pub fn light(accent: Accent) -> Self {
        Self {
            ink: accent.tone(85., 36.),
            soft: accent.fill().with_alpha_byte(0x24),
            ..Self::dark(accent)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translucent_accent_falls_back_to_default() {
        let translucent = Color::rgba(10, 20, 30, 0.5);
        assert_eq!(Accent::new(translucent), Accent::default());
        assert_eq!(Accent::default().fill(), DEFAULT_ACCENT);
    }

    #[test]
    fn dark_states_use_the_canvas_alpha_suffixes() {
        let colors = AccentColors::dark(Accent::default());
        assert_eq!(colors.soft.to_rgba_u32(), 0xFF612E26);
        assert_eq!(colors.soft_line.to_rgba_u32(), 0xFF612E8C);
        assert_eq!(colors.ring.to_rgba_u32(), 0xFF612E33);
        assert_eq!(colors.ink, DEFAULT_ACCENT);
    }

    #[test]
    fn see_through_ink_lifts_only_dark_accents() {
        let lifted = AccentColors::see_through(Accent::default());
        assert_eq!(lifted.ink, Color::hsl(15., 100., 70.));
        assert_eq!(lifted.fill, DEFAULT_ACCENT);
        assert_eq!(
            lifted,
            AccentColors {
                ink: lifted.ink,
                ..AccentColors::dark(Accent::default())
            }
        );
        let blue = Accent::new(Color::hex(0x6EA8FF));
        assert_eq!(AccentColors::see_through(blue), AccentColors::dark(blue));
    }

    #[test]
    fn paper_ink_is_a_darker_tone_of_the_same_hue() {
        let colors = AccentColors::light(Accent::default());
        assert_eq!(colors.ink, Color::hsl(15., 85., 36.));
        assert_eq!(colors.soft.alpha, 0x24);
        assert!(colors.ink.relative_luminance() < DEFAULT_ACCENT.relative_luminance());
    }
}
