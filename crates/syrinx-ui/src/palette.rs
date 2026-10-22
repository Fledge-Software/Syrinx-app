//! The five base palettes, transcribed from the design canvas's shared `tokens()` method.
//!
//! Source: `design/canvas/project/Sidebar.dc.html` (identical in Main, Palette and
//! Browsing). Each field names its canvas key so the two can be compared line by line.
//! Shadows are not yet tokens; they stay in the canvas until a slice draws them.

use crate::accent::{Accent, AccentColors};
use crate::color::Color;
use crate::contrast;

/// The palette ids the canvas and Customize panel use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaletteId {
    Graphite,
    Haze,
    Glass,
    Tint,
    Paper,
}

impl PaletteId {
    /// All five, in the Customize panel's order. Graphite is the default.
    pub const ALL: [PaletteId; 5] = [
        PaletteId::Graphite,
        PaletteId::Haze,
        PaletteId::Glass,
        PaletteId::Tint,
        PaletteId::Paper,
    ];

    /// The canvas id (`graphite`, `haze`, …).
    pub const fn id(self) -> &'static str {
        match self {
            PaletteId::Graphite => "graphite",
            PaletteId::Haze => "haze",
            PaletteId::Glass => "glass",
            PaletteId::Tint => "tint",
            PaletteId::Paper => "paper",
        }
    }

    /// The name shown to the owner.
    pub const fn name(self) -> &'static str {
        match self {
            PaletteId::Graphite => "Graphite",
            PaletteId::Haze => "Haze",
            PaletteId::Glass => "Glass",
            PaletteId::Tint => "Tint",
            PaletteId::Paper => "Paper",
        }
    }

    /// Whether the window background lets the desktop show through.
    pub const fn is_see_through(self) -> bool {
        matches!(self, PaletteId::Haze | PaletteId::Glass)
    }

    /// The palette actually drawn: Reduce transparency turns Haze and Glass into Graphite.
    pub const fn resolve(self, reduce_transparency: bool) -> PaletteId {
        if reduce_transparency && self.is_see_through() {
            PaletteId::Graphite
        } else {
            self
        }
    }
}

/// The Windows 11 window material a see-through palette asks the OS to draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowMaterial {
    /// Haze: the `mica` backdrop, softly coloured by the wallpaper.
    Mica,
    /// Mica Alt: Mica with more of the wallpaper's colour, as tabbed windows use.
    MicaAlt,
    /// Glass: the transient `acrylic` backdrop plus Syrinx's glass styling.
    Acrylic,
}

/// What sits behind the chrome (the canvas's `ground`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ground {
    /// A plain colour.
    Solid(Color),
    /// Tint's diagonal gradient (`linear-gradient(155deg, …)` at 0%, 55% and 100%).
    Gradient {
        angle_degrees: f32,
        stops: [(Color, f32); 3],
    },
    /// An OS material with Syrinx's translucent tint painted over it.
    Material {
        material: WindowMaterial,
        tint: Color,
        /// The canvas's stand-in wallpaper colours, used to check contrast and draw samples.
        wallpaper_standins: &'static [Color],
    },
}

impl Ground {
    /// Every opaque colour the ground can present to text above it.
    pub fn presented_colors(&self) -> Vec<Color> {
        match self {
            Ground::Solid(color) => vec![*color],
            Ground::Gradient { stops, .. } => stops.iter().map(|(color, _)| *color).collect(),
            Ground::Material {
                tint,
                wallpaper_standins,
                ..
            } => wallpaper_standins
                .iter()
                .map(|wallpaper| tint.over(*wallpaper))
                .collect(),
        }
    }

    /// One opaque colour to fill a sample swatch with.
    pub fn representative(&self) -> Color {
        match self {
            Ground::Solid(color) => *color,
            Ground::Gradient { stops, .. } => stops[0].0,
            Ground::Material {
                tint,
                wallpaper_standins,
                ..
            } => tint.over(wallpaper_standins[0]),
        }
    }
}

/// A monogram tile's background and letter colours (the canvas's `tiles`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileColors {
    pub background: Color,
    pub foreground: Color,
}

/// The identifying colours of the built-in plugins (the canvas's `plug`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PluginColors {
    pub forge: TileColors,
    pub notes: TileColors,
    pub fledge: TileColors,
    pub neutral: TileColors,
}

/// Every colour role of one base palette with one accent applied.
#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    pub id: PaletteId,
    /// `ground`
    pub ground: Ground,
    /// `grain`: the opacity of a faint grey noise over the ground, from 0 (none) to 1.
    pub grain: f32,
    /// The glass edge: the opacity of the light line along a surface's top, 0 for none.
    pub edges: f32,
    /// `text`
    pub text: Color,
    /// `strong`
    pub text_strong: Color,
    /// `text2`
    pub text_secondary: Color,
    /// `text2b`
    pub text_secondary_bright: Color,
    /// `text3`: muted labels and hints.
    pub text_muted: Color,
    /// `icon`
    pub icon: Color,
    /// `iconDim`
    pub icon_dim: Color,
    /// `disabled`
    pub disabled: Color,
    /// `line`
    pub line: Color,
    /// `field`
    pub field: Color,
    /// `rowOn`: the selected row.
    pub row_selected: Color,
    /// `key`: keyboard-hint outlines.
    pub key: Color,
    /// `dash`
    pub dash: Color,
    /// `dash2`
    pub dash_secondary: Color,
    /// `badgeRing`
    pub badge_ring: Color,
    /// `card`: the page card around Syrinx's own pages.
    pub card: Color,
    /// `cardLine`
    pub card_line: Color,
    /// `cardRing`
    pub card_ring: Color,
    /// `surf`
    pub surface: Color,
    /// `surfLine`
    pub surface_line: Color,
    /// `pill`: the home page's search pill.
    pub pill: Color,
    /// `pillLine`
    pub pill_line: Color,
    /// `chipLine`
    pub chip_line: Color,
    /// `chipOn`
    pub chip_selected: Color,
    /// `panel`: the command palette and other near-opaque popups.
    pub panel: Color,
    /// `panelLine`
    pub panel_line: Color,
    /// `panelSide`: the palette's preview side.
    pub panel_side: Color,
    /// `panelDiv`
    pub panel_divider: Color,
    /// `input`
    pub input: Color,
    /// `inputLine`
    pub input_line: Color,
    /// `scrim`: dims the window behind a modal.
    pub scrim: Color,
    /// `tiles`: one colour pair per site index, so a site keeps its tile colour everywhere.
    pub tiles: [TileColors; 8],
    /// `plug`
    pub plugins: PluginColors,
    /// `ink`, `soft`, `softLine`, `ring` and `badgeText`.
    pub accent: AccentColors,
}

impl Palette {
    /// Whether this palette is light: Paper, or a light theme, which starts from Paper. Windows
    /// draws the material and widgets in their light form for it.
    pub fn is_light(&self) -> bool {
        self.id == PaletteId::Paper
    }
}

/// Builds a palette for `id` with the owner's accent applied.
pub fn palette(id: PaletteId, accent: Accent) -> Palette {
    match id {
        PaletteId::Graphite => graphite(accent),
        PaletteId::Haze => with_legible_ink(haze(accent)),
        PaletteId::Glass => with_legible_ink(glass(accent)),
        PaletteId::Tint => tint(accent),
        PaletteId::Paper => paper(accent),
    }
}

/// Haze and Glass keep the accent's own ink unless it fails the contrast audit on their
/// lighter see-through grounds; then they take the first lighter tone that passes, or the
/// lightest offered (owner decision B, 2026-10-13). Fills, washes and badges are untouched.
fn with_legible_ink(mut palette: Palette) -> Palette {
    if contrast::accent_ink_passes(&palette) {
        return palette;
    }
    for ink in AccentColors::lifted_inks(Accent::new(palette.accent.fill)) {
        palette.accent.ink = ink;
        if contrast::accent_ink_passes(&palette) {
            break;
        }
    }
    palette
}

const fn tile(background: u32, foreground: u32) -> TileColors {
    TileColors {
        background: Color::hex(background),
        foreground: Color::hex(foreground),
    }
}

const fn white(alpha: f32) -> Color {
    Color::rgba(255, 255, 255, alpha)
}

const DARK_TILES: [TileColors; 8] = [
    tile(0x1E2A44, 0x7BA7FF),
    tile(0x3A2420, 0xFF8F7A),
    tile(0x3A3220, 0xF2C55C),
    tile(0x2A2C33, 0xD6D8DE),
    tile(0x2A2644, 0xA99BFF),
    tile(0x1F3836, 0x5ED3C8),
    tile(0x38331C, 0xF5D547),
    tile(0x3A2430, 0xF28BB0),
];

const LIGHT_TILES: [TileColors; 8] = [
    tile(0xDCE6FF, 0x2F5BD3),
    tile(0xFFE1DA, 0xB83A25),
    tile(0xFBEBC4, 0x8A5A00),
    tile(0xE4E5EA, 0x3E4048),
    tile(0xE6E0FF, 0x5A43CF),
    tile(0xD3F0EC, 0x0D7068),
    tile(0xF8EDBE, 0x7A6000),
    tile(0xFADCE8, 0xAE2E63),
];

const DARK_PLUGINS: PluginColors = PluginColors {
    forge: tile(0x1E2A44, 0x7BA7FF),
    notes: tile(0x2A2644, 0xB79AFF),
    fledge: tile(0x1F3A2E, 0x6FD3A8),
    neutral: tile(0x2A2C33, 0xC9CBD1),
};

const LIGHT_PLUGINS: PluginColors = PluginColors {
    forge: tile(0xDCE6FF, 0x2F5BD3),
    notes: tile(0xE6E0FF, 0x5A43CF),
    fledge: tile(0xD3F0E2, 0x11774F),
    neutral: tile(0xE4E5EA, 0x3E4048),
};

/// The canvas's Haze stand-in wallpaper: its base and four radial-gradient centres.
pub(crate) const HAZE_WALLPAPER: [Color; 5] = [
    Color::hex(0x1A1F35),
    Color::hex(0x3F52AE),
    Color::hex(0x6B3F94),
    Color::hex(0x1D6F6A),
    Color::hex(0x9C5637),
];

/// Stand-in wallpaper for a light theme's material: Windows draws light Acrylic and Mica with
/// a white luminosity layer, so what shows through is a pale version of the Haze stand-ins.
pub(crate) const LIGHT_WALLPAPER: [Color; 5] = [
    Color::hex(0xE4E7F0),
    Color::hex(0xC9D1F0),
    Color::hex(0xDCCDEA),
    Color::hex(0xC6E3E0),
    Color::hex(0xEED6CA),
];

/// The canvas's Glass stand-in wallpaper: four solid discs and three gradient stops.
const GLASS_WALLPAPER: [Color; 7] = [
    Color::hex(0x4A62D6),
    Color::hex(0x9446B0),
    Color::hex(0xC4623B),
    Color::hex(0x23948A),
    Color::hex(0x1B2550),
    Color::hex(0x2A1838),
    Color::hex(0x13252B),
];

pub(crate) fn graphite(accent: Accent) -> Palette {
    Palette {
        id: PaletteId::Graphite,
        ground: Ground::Solid(Color::hex(0x141519)),
        grain: 0.,
        edges: 0.,
        text: Color::hex(0xECEDF0),
        text_strong: Color::hex(0xFFFFFF),
        text_secondary: Color::hex(0xC9CBD1),
        text_secondary_bright: Color::hex(0xD5D7DD),
        text_muted: Color::hex(0x8B8E98),
        icon: Color::hex(0xB4B7BF),
        icon_dim: Color::hex(0xA3A6AE),
        disabled: Color::hex(0x50535C),
        line: Color::hex(0x24262C),
        field: white(0.06),
        row_selected: white(0.09),
        key: Color::hex(0x383B43),
        dash: Color::hex(0x43464F),
        dash_secondary: Color::hex(0x363941),
        badge_ring: Color::hex(0x141519),
        card: Color::hex(0x0D0E11),
        card_line: Color::hex(0x23252B),
        card_ring: Color::hex(0x0D0E11),
        surface: Color::hex(0x15161B),
        surface_line: Color::hex(0x24262C),
        pill: Color::hex(0x17181D),
        pill_line: Color::hex(0x2C2F36),
        chip_line: Color::hex(0x2C2F36),
        chip_selected: white(0.11),
        panel: Color::hex(0x1B1C21),
        panel_line: Color::hex(0x31343C),
        panel_side: Color::hex(0x17181D),
        panel_divider: Color::hex(0x282B32),
        input: Color::hex(0x101115),
        input_line: Color::hex(0x2C2F36),
        scrim: Color::rgba(6, 7, 9, 0.62),
        tiles: DARK_TILES,
        plugins: DARK_PLUGINS,
        accent: AccentColors::dark(accent),
    }
}

fn haze(accent: Accent) -> Palette {
    Palette {
        id: PaletteId::Haze,
        ground: Ground::Material {
            material: WindowMaterial::Mica,
            tint: Color::rgba(26, 27, 32, 0.74),
            wallpaper_standins: &HAZE_WALLPAPER,
        },
        text: Color::hex(0xEFF0F4),
        text_secondary: Color::hex(0xD3D5DC),
        text_secondary_bright: Color::hex(0xDCDEE4),
        text_muted: Color::hex(0xA3A6B0),
        icon: Color::hex(0xC0C3CB),
        icon_dim: Color::hex(0xAFB2BC),
        disabled: white(0.28),
        line: white(0.09),
        field: white(0.07),
        row_selected: white(0.11),
        key: white(0.2),
        dash: white(0.26),
        dash_secondary: white(0.2),
        badge_ring: Color::hex(0x262833),
        card: Color::rgba(10, 11, 14, 0.5),
        card_line: white(0.08),
        card_ring: Color::hex(0x16171C),
        surface: white(0.045),
        surface_line: white(0.08),
        pill: white(0.06),
        pill_line: white(0.12),
        chip_line: white(0.14),
        chip_selected: white(0.14),
        panel: Color::hex(0x202127),
        panel_line: white(0.12),
        panel_side: Color::hex(0x1B1C21),
        panel_divider: white(0.08),
        input: Color::rgba(0, 0, 0, 0.28),
        input_line: white(0.12),
        scrim: Color::rgba(6, 7, 10, 0.5),
        ..graphite(accent)
    }
}

fn glass(accent: Accent) -> Palette {
    Palette {
        id: PaletteId::Glass,
        ground: Ground::Material {
            material: WindowMaterial::Acrylic,
            tint: Color::rgba(16, 17, 24, 0.46),
            wallpaper_standins: &GLASS_WALLPAPER,
        },
        // The canvas's `inset 0 1px 0 rgba(255,255,255,0.24)` glass edge.
        edges: 0.24,
        text: Color::hex(0xF4F5F8),
        text_secondary: Color::hex(0xE0E2E8),
        text_secondary_bright: Color::hex(0xE6E8ED),
        text_muted: Color::hex(0xBCBFCA),
        icon: Color::hex(0xD0D3DA),
        icon_dim: Color::hex(0xC2C5CE),
        disabled: white(0.32),
        line: white(0.14),
        field: white(0.10),
        row_selected: white(0.16),
        key: white(0.28),
        dash: white(0.32),
        dash_secondary: white(0.26),
        badge_ring: Color::hex(0x2B2E44),
        card: white(0.05),
        card_line: white(0.16),
        card_ring: Color::hex(0x2B2E44),
        surface: white(0.08),
        surface_line: white(0.16),
        pill: white(0.10),
        pill_line: white(0.22),
        chip_line: white(0.18),
        chip_selected: white(0.18),
        panel: Color::rgba(28, 30, 42, 0.94),
        panel_line: white(0.16),
        panel_side: Color::rgba(0, 0, 0, 0.18),
        panel_divider: white(0.09),
        input: Color::rgba(0, 0, 0, 0.28),
        input_line: white(0.14),
        scrim: Color::rgba(6, 7, 12, 0.42),
        tiles: DARK_TILES.map(|tile| TileColors {
            background: tile.background.with_alpha_byte(0xC7),
            ..tile
        }),
        ..graphite(accent)
    }
}

fn tint(accent: Accent) -> Palette {
    let tone = |saturation: f64, lightness: f64| accent.tone(saturation, lightness);
    Palette {
        id: PaletteId::Tint,
        ground: Ground::Gradient {
            angle_degrees: 155.,
            stops: [
                (tone(34., 15.), 0.),
                (tone(24., 9.), 0.55),
                (tone(20., 6.), 1.),
            ],
        },
        // The canvas's grain is an SVG noise at 9 % opacity.
        grain: 0.09,
        text: tone(18., 95.),
        text_secondary: tone(12., 83.),
        text_secondary_bright: tone(12., 87.),
        text_muted: tone(10., 66.),
        icon: tone(10., 76.),
        icon_dim: tone(9., 70.),
        disabled: tone(10., 32.),
        line: white(0.08),
        field: white(0.07),
        row_selected: white(0.11),
        key: white(0.18),
        dash: white(0.26),
        dash_secondary: white(0.18),
        badge_ring: tone(34., 15.),
        card: tone(22., 6.),
        card_line: tone(18., 14.),
        card_ring: tone(22., 6.),
        surface: tone(20., 9.),
        surface_line: tone(16., 15.),
        pill: tone(20., 10.),
        pill_line: tone(16., 19.),
        chip_line: tone(14., 22.),
        chip_selected: white(0.12),
        panel: tone(18., 11.),
        panel_line: tone(14., 21.),
        panel_side: tone(18., 9.),
        panel_divider: tone(14., 16.),
        input: tone(20., 6.),
        input_line: tone(14., 21.),
        scrim: Color::rgba(6, 5, 5, 0.6),
        ..graphite(accent)
    }
}

pub(crate) fn paper(accent: Accent) -> Palette {
    let ink = |alpha: f32| Color::rgba(20, 22, 30, alpha);
    Palette {
        id: PaletteId::Paper,
        ground: Ground::Solid(Color::hex(0xE9EAEE)),
        grain: 0.,
        edges: 0.,
        text: Color::hex(0x1B1C21),
        text_strong: Color::hex(0x0E0F12),
        text_secondary: Color::hex(0x3A3C44),
        text_secondary_bright: Color::hex(0x2B2D34),
        text_muted: Color::hex(0x5E616A),
        icon: Color::hex(0x4A4D56),
        icon_dim: Color::hex(0x5E616A),
        disabled: Color::hex(0xB1B3BA),
        line: Color::hex(0xD5D7DD),
        field: ink(0.06),
        row_selected: Color::hex(0xFFFFFF),
        key: Color::hex(0xC3C5CC),
        dash: Color::hex(0xB5B8C0),
        dash_secondary: Color::hex(0xC3C5CC),
        badge_ring: Color::hex(0xE9EAEE),
        card: Color::hex(0xF7F7F9),
        card_line: Color::hex(0xDADCE2),
        card_ring: Color::hex(0xF7F7F9),
        surface: Color::hex(0xFFFFFF),
        surface_line: Color::hex(0xE0E2E7),
        pill: Color::hex(0xFFFFFF),
        pill_line: Color::hex(0xD7D9DF),
        chip_line: Color::hex(0xD5D7DD),
        chip_selected: ink(0.08),
        panel: Color::hex(0xFFFFFF),
        panel_line: Color::hex(0xD5D7DD),
        panel_side: Color::hex(0xF5F6F8),
        panel_divider: Color::hex(0xE4E5EA),
        input: Color::hex(0xFFFFFF),
        input_line: Color::hex(0xD0D2D9),
        scrim: Color::rgba(20, 22, 30, 0.26),
        tiles: LIGHT_TILES,
        plugins: LIGHT_PLUGINS,
        accent: AccentColors::light(accent),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEE_THROUGH: [PaletteId; 2] = [PaletteId::Haze, PaletteId::Glass];

    #[test]
    fn see_through_palettes_keep_passing_green_and_gold_accents() {
        for color in [0x6FD3A8, 0xE8C16A] {
            let accent = Accent::new(Color::hex(color));
            for id in SEE_THROUGH {
                assert_eq!(
                    palette(id, accent).accent,
                    AccentColors::dark(accent),
                    "{color:06x}"
                );
            }
        }
    }

    #[test]
    fn see_through_palettes_lift_only_the_failing_default_inks() {
        for id in SEE_THROUGH {
            let drawn = palette(id, Accent::default());
            assert_eq!(drawn.accent.ink, Color::hsl(15., 100., 70.));
            assert_eq!(
                drawn.accent,
                AccentColors {
                    ink: drawn.accent.ink,
                    ..AccentColors::dark(Accent::default())
                }
            );
            assert!(contrast::accent_ink_passes(&drawn));
        }
        // The blue preset failed only as a mark on Glass's card (2.94:1), so only Glass lifts it.
        let blue = Accent::new(Color::hex(0x6EA8FF));
        assert_eq!(
            palette(PaletteId::Haze, blue).accent,
            AccentColors::dark(blue)
        );
        let glass_blue = palette(PaletteId::Glass, blue).accent;
        assert!(glass_blue.ink.lightness_percent() > blue.fill().lightness_percent());
        assert_eq!(glass_blue.fill, blue.fill());
        // Graphite passes with the accent's own colour, so it is never lifted.
        let graphite = palette(PaletteId::Graphite, Accent::default());
        assert_eq!(graphite.accent, AccentColors::dark(Accent::default()));
    }

    #[test]
    fn reduce_transparency_turns_see_through_palettes_into_graphite() {
        assert_eq!(PaletteId::Haze.resolve(true), PaletteId::Graphite);
        assert_eq!(PaletteId::Glass.resolve(true), PaletteId::Graphite);
        for id in [PaletteId::Graphite, PaletteId::Tint, PaletteId::Paper] {
            assert_eq!(id.resolve(true), id);
        }
        for id in PaletteId::ALL {
            assert_eq!(id.resolve(false), id);
        }
    }

    #[test]
    fn every_palette_reports_its_own_id() {
        for id in PaletteId::ALL {
            assert_eq!(palette(id, Accent::default()).id, id);
        }
    }

    #[test]
    fn graphite_keeps_the_preview_dark_values() {
        let graphite = palette(PaletteId::Graphite, Accent::default());
        assert_eq!(graphite.ground, Ground::Solid(Color::hex(0x141519)));
        assert_eq!(graphite.text, Color::hex(0xECEDF0));
        assert_eq!(graphite.line, Color::hex(0x24262C));
        assert_eq!(graphite.panel, Color::hex(0x1B1C21));
    }

    #[test]
    fn tint_follows_the_accent_hue() {
        let tint = palette(PaletteId::Tint, Accent::default());
        assert_eq!(tint.text, Color::hsl(14., 18., 95.));
        assert_eq!(tint.card, Color::hsl(14., 22., 6.));
        let blue = palette(PaletteId::Tint, Accent::new(Color::hex(0x6EA8FF)));
        assert_ne!(blue.card, tint.card);
    }

    #[test]
    fn glass_tiles_are_translucent_versions_of_the_dark_tiles() {
        let glass = palette(PaletteId::Glass, Accent::default());
        for (glass_tile, dark_tile) in glass.tiles.iter().zip(DARK_TILES) {
            assert_eq!(glass_tile.background.alpha, 0xC7);
            assert_eq!(glass_tile.foreground, dark_tile.foreground);
        }
    }

    #[test]
    fn material_grounds_present_opaque_colours() {
        for id in [PaletteId::Haze, PaletteId::Glass] {
            let ground = palette(id, Accent::default()).ground;
            assert!(
                ground
                    .presented_colors()
                    .iter()
                    .all(|color| color.is_opaque())
            );
            assert!(ground.representative().is_opaque());
        }
    }
}
