//! A theme the owner builds in Customize: a few choices from which every palette colour is
//! worked out. Graphite and Paper stay the presets (`docs/theme-editor.md`).

use crate::accent::Accent;
use crate::color::Color;
use crate::contrast::{self, ContrastCheck};
use crate::palette::{
    GLASS_WALLPAPER, Ground, HAZE_WALLPAPER, LIGHT_WALLPAPER, Palette, PaletteId, WindowMaterial,
    palette, template_drops,
};

/// The longest theme name, in characters.
pub const MAX_NAME_CHARS: usize = 40;

/// The longest name in UTF-8 bytes: a settings value holds 256 bytes and the other fields of
/// a stored theme take at most 135, so a name of 40 emoji still fits.
pub const MAX_NAME_BYTES: usize = 120;

/// The least tint a selected row keeps under it, the slider's old floor: with no tint at all,
/// text on the row's light fill over a bright window falls below 4.5:1.
const ROW_TINT_FLOOR: f32 = 0.1;

/// The least tint dark Acrylic keeps (owner decision 2026-10-24): 32/255 is the least byte at
/// which white text on Graphite's #141519 reaches 4.5:1 over #124's teal disc (#23948A).
const DARK_ACRYLIC_TINT_FLOOR: f32 = 32. / 255.;

/// The least tint under dark Acrylic's white-film surfaces (selected row, field, pill, surface,
/// card): below it white text on the selected row falls under 4.5:1 over the teal disc.
const DARK_ACRYLIC_SURFACE_FLOOR: f32 = 0.343;

/// Each stepped choice's range and step, in percent or degrees.
pub const TRANSPARENCY: Range = Range::new(0, 90, 5);
pub const EDGES: Range = Range::new(0, 40, 4);
pub const GRAIN: Range = Range::new(0, 15, 1);
pub const ANGLE: Range = Range::new(0, 355, 5);

/// The first line of every stored theme; a later layout gets a new version.
const VERSION: &str = "v1";

/// A stepped whole-number choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    pub min: u16,
    pub max: u16,
    pub step: u16,
}

impl Range {
    const fn new(min: u16, max: u16, step: u16) -> Self {
        Self { min, max, step }
    }

    /// `value` moved by `steps` steps, kept in range.
    pub fn stepped(self, value: u16, steps: i32) -> u16 {
        let moved = i32::from(value) + steps * i32::from(self.step);
        moved.clamp(i32::from(self.min), i32::from(self.max)) as u16
    }

    /// The angle range wraps around the circle instead of stopping.
    pub fn wrapped(self, value: u16, steps: i32) -> u16 {
        let full = i32::from(self.max) + i32::from(self.step);
        (i32::from(value) + steps * i32::from(self.step)).rem_euclid(full) as u16
    }

    fn holds(self, value: u16) -> bool {
        (self.min..=self.max).contains(&value) && (value - self.min).is_multiple_of(self.step)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Dark,
    Light,
}

/// What Windows draws behind a see-through theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Material {
    None,
    Mica,
    MicaAlt,
    Acrylic,
}

impl Material {
    pub const ALL: [Material; 4] = [
        Material::None,
        Material::Mica,
        Material::MicaAlt,
        Material::Acrylic,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Material::None => "Off",
            Material::Mica => "Mica",
            Material::MicaAlt => "Mica Alt",
            Material::Acrylic => "Acrylic",
        }
    }

    const fn id(self) -> &'static str {
        match self {
            Material::None => "none",
            Material::Mica => "mica",
            Material::MicaAlt => "mica-alt",
            Material::Acrylic => "acrylic",
        }
    }

    const fn window_material(self) -> Option<WindowMaterial> {
        match self {
            Material::None => None,
            Material::Mica => Some(WindowMaterial::Mica),
            Material::MicaAlt => Some(WindowMaterial::MicaAlt),
            Material::Acrylic => Some(WindowMaterial::Acrylic),
        }
    }
}

/// How the background is filled where no material shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fill {
    Solid,
    /// From the background colour to `second`, along `angle` degrees as CSS measures it.
    Gradient {
        second: Color,
        angle: u16,
    },
}

/// One saved theme.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeSpec {
    pub name: String,
    pub mode: Mode,
    pub background: Color,
    pub fill: Fill,
    pub material: Material,
    /// Percent of the material that shows through the background colour.
    pub transparency: u16,
    /// Whether Home's card follows Transparency too, instead of staying as the template draws it.
    pub see_through_home: bool,
    /// Percent opacity of the glass edge on surfaces.
    pub edges: u16,
    /// Percent opacity of the grain.
    pub grain: u16,
}

/// Why a stored theme could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unreadable;

impl ThemeSpec {
    /// A new theme: Graphite's look under the given name.
    pub fn new(name: &str) -> Self {
        Self {
            name: clean_name(name),
            mode: Mode::Dark,
            background: Color::hex(0x141519),
            fill: Fill::Solid,
            material: Material::None,
            transparency: 50,
            see_through_home: false,
            edges: 0,
            grain: 0,
        }
    }

    /// The Glass preset (owner decision 2026-10-21): the canvas's Glass under Acrylic, its
    /// `rgba(16,17,24,0.46)` tint as 50 % Transparency (a 0.44 tint, the nearest step under
    /// the 1 - t/90 mapping; owner decision 2026-10-24) and its 0.24 edge.
    pub fn glass() -> Self {
        Self {
            background: Color::hex(0x101118),
            material: Material::Acrylic,
            transparency: 50,
            edges: 24,
            ..Self::new(PaletteId::Glass.name())
        }
    }

    /// The custom theme that stands in for a retired Haze, Glass or Tint choice; Tint followed
    /// the accent, so its stand-in is built from `accent`.
    pub fn from_retired_palette(id: PaletteId, accent: Accent) -> Option<Self> {
        let base = Self::new(id.name());
        match id {
            PaletteId::Haze => Some(Self {
                background: Color::hex(0x1A1B20),
                material: Material::Mica,
                transparency: 25,
                ..base
            }),
            PaletteId::Glass => Some(Self::glass()),
            PaletteId::Tint => Some(Self {
                background: accent.tone(34., 15.),
                fill: Fill::Gradient {
                    second: accent.tone(20., 6.),
                    angle: 155,
                },
                grain: 9,
                ..base
            }),
            PaletteId::Graphite | PaletteId::Paper => None,
        }
    }

    /// Whether a material shows: one is chosen, some transparency is asked for, and Reduce
    /// transparency is off.
    pub fn shows_material(&self, reduce_transparency: bool) -> bool {
        self.material != Material::None && self.transparency > 0 && !reduce_transparency
    }

    /// The preset whose surfaces and inks this theme starts from: Paper for light themes,
    /// Graphite for opaque dark ones, the canvas's Glass under Acrylic, and Haze under Mica.
    pub fn template(&self, reduce_transparency: bool) -> PaletteId {
        match (self.mode, self.shows_material(reduce_transparency)) {
            (Mode::Light, _) => PaletteId::Paper,
            (Mode::Dark, false) => PaletteId::Graphite,
            (Mode::Dark, true) if self.material == Material::Acrylic => PaletteId::Glass,
            (Mode::Dark, true) => PaletteId::Haze,
        }
    }

    /// Every colour role for this saved theme with `accent` applied.
    pub fn palette(&self, accent: Accent, reduce_transparency: bool) -> Palette {
        self.drawn(accent, reduce_transparency, Origin::Saved)
    }

    /// Every colour role for this theme drawn as a preset: the canvas's own colours, keeping
    /// the canvas's recorded contrast exceptions. Only the Glass preset is drawn this way.
    pub fn preset_palette(&self, accent: Accent, reduce_transparency: bool) -> Palette {
        self.drawn(accent, reduce_transparency, Origin::Preset)
    }

    fn drawn(&self, accent: Accent, reduce_transparency: bool, origin: Origin) -> Palette {
        let see_through = self.shows_material(reduce_transparency);
        let template = self.template(reduce_transparency);
        let exempt = exceptions_from(template, origin);
        let mut drawn = palette(template, accent);
        let canvas_colours = origin == Origin::Preset && template == PaletteId::Glass;
        if !canvas_colours && Some(self.background) != solid_colour(&drawn.ground) {
            recolour(&mut drawn, self.background, self.mode);
        }
        drawn.ground = self.ground(see_through);
        if see_through {
            self.thin_for_the_material(&mut drawn);
        }
        drawn.badge_ring = self.background;
        keep_text_readable(&mut drawn, accent, self.mode, exempt);
        drawn.grain = f32::from(self.grain) / 100.;
        drawn.edges = f32::from(self.edges) / 100.;
        drawn.shadows =
            template_drops(template).with_glass_edges(drawn.edges, self.mode == Mode::Light);
        drawn
    }

    /// The pairs this theme still fails after text was made readable, so the editor can say
    /// which: accent marks and tiles, or text that cannot reach the minimum at all.
    pub fn contrast_problems(&self, accent: Accent) -> Vec<ContrastCheck> {
        let exempt = exceptions_from(self.template(false), Origin::Saved);
        failures_beyond(
            &self.palette(accent, false),
            &inherited_failures(exempt, accent),
        )
    }

    /// How opaque Syrinx's own tint is over the material: the slider's opacity, except that
    /// dark Acrylic never drops below its floor (owner decisions 2026-10-22 and 2026-10-24).
    pub fn tint_opacity(&self) -> f32 {
        let slider = self.slider_opacity();
        if self.is_dark_acrylic() {
            return slider.max(DARK_ACRYLIC_TINT_FLOOR);
        }
        slider
    }

    /// The slider's own opacity: fully at 0 %, not at all at its top.
    fn slider_opacity(&self) -> f32 {
        let shown = f32::from(self.transparency.min(TRANSPARENCY.max));
        1. - shown / f32::from(TRANSPARENCY.max)
    }

    fn is_dark_acrylic(&self) -> bool {
        self.mode == Mode::Dark && self.material == Material::Acrylic
    }

    /// Home's card follows the slider when asked, and the light-film surfaces keep a little
    /// tint under them however clear the window is.
    fn thin_for_the_material(&self, drawn: &mut Palette) {
        let tint = self.tint_opacity();
        if self.see_through_home {
            drawn.card = with_alpha(drawn.card, alpha_of(drawn.card) * self.slider_opacity());
        }
        if !self.is_dark_acrylic() {
            drawn.row_selected = self.backed(drawn.row_selected, ROW_TINT_FLOOR, tint);
            return;
        }
        // Acrylic shows the bright desktop, so every white film needs a darker footing.
        let floor = DARK_ACRYLIC_SURFACE_FLOOR;
        for surface in [
            &mut drawn.row_selected,
            &mut drawn.field,
            &mut drawn.pill,
            &mut drawn.surface,
        ] {
            *surface = self.backed(*surface, floor, tint);
        }
        if !self.see_through_home {
            drawn.card = self.backed(drawn.card, floor, tint);
        }
    }

    /// `layer` over enough of the background that, with the window's tint, at least `floor`
    /// of tint lies under it.
    fn backed(&self, layer: Color, floor: f32, tint: f32) -> Color {
        if tint >= floor {
            return layer;
        }
        // Under the layer, (1 - backing)(1 - tint) of the window shows: 1 - the floor.
        let backing = 1. - (1. - floor) / (1. - tint);
        layer.over(with_alpha(self.background, backing))
    }

    fn ground(&self, see_through: bool) -> Ground {
        if let (true, Some(material)) = (see_through, self.material.window_material()) {
            // Acrylic shows the desktop itself, so dark text is checked on Glass's bright discs;
            // Windows draws a light theme's material light, so its text is checked on pale ones.
            let wallpaper_standins: &'static [Color] = match (self.mode, material) {
                (Mode::Light, _) => &LIGHT_WALLPAPER,
                (Mode::Dark, WindowMaterial::Acrylic) => &GLASS_WALLPAPER,
                (Mode::Dark, WindowMaterial::Mica | WindowMaterial::MicaAlt) => &HAZE_WALLPAPER,
            };
            return Ground::Material {
                material,
                tint: with_alpha(self.background, self.tint_opacity()),
                wallpaper_standins,
            };
        }
        match self.fill {
            Fill::Solid => Ground::Solid(self.background),
            Fill::Gradient { second, angle } => Ground::Gradient {
                angle_degrees: f32::from(angle),
                stops: [
                    (self.background, 0.),
                    (mix(self.background, second, 0.5), 0.55),
                    (second, 1.),
                ],
            },
        }
    }

    /// One settings value: `v1;name=…;mode=dark;bg=#141519;…`. A name's `;` and `=` are
    /// dropped when it is cleaned, so the fields always split.
    pub fn to_setting(&self) -> String {
        let (style, second, angle) = match self.fill {
            Fill::Solid => ("solid", self.background, 155),
            Fill::Gradient { second, angle } => ("gradient", second, angle),
        };
        let mode = match self.mode {
            Mode::Dark => "dark",
            Mode::Light => "light",
        };
        // Written only when on, so a theme without it reads in builds that predate the field.
        let home = if self.see_through_home {
            ";home=see-through"
        } else {
            ""
        };
        format!(
            "{VERSION};name={};mode={mode};bg={};fill={style};bg2={};angle={angle};material={};transparency={};edges={};grain={}{home}",
            self.name,
            hex(self.background),
            hex(second),
            self.material.id(),
            self.transparency,
            self.edges,
            self.grain
        )
    }

    /// Reads a value `to_setting` wrote, refusing anything else.
    pub fn parse(text: &str) -> Result<Self, Unreadable> {
        let mut fields = text.split(';');
        if fields.next() != Some(VERSION) {
            return Err(Unreadable);
        }
        let mut theme = Self::new("");
        let mut style = "solid";
        let mut second = theme.background;
        let mut angle = 155;
        for field in fields {
            let (key, value) = field.split_once('=').ok_or(Unreadable)?;
            match key {
                "name" => theme.name = clean_name(value),
                "mode" => theme.mode = parse_mode(value)?,
                "bg" => theme.background = parse_hex(value).ok_or(Unreadable)?,
                "fill" => style = value,
                "bg2" => second = parse_hex(value).ok_or(Unreadable)?,
                "angle" => angle = parse_in(value, ANGLE)?,
                "material" => theme.material = parse_material(value)?,
                "transparency" => theme.transparency = parse_in(value, TRANSPARENCY)?,
                "edges" => theme.edges = parse_in(value, EDGES)?,
                "grain" => theme.grain = parse_in(value, GRAIN)?,
                "home" => theme.see_through_home = parse_home(value)?,
                _ => return Err(Unreadable),
            }
        }
        theme.fill = match style {
            "solid" => Fill::Solid,
            "gradient" => Fill::Gradient { second, angle },
            _ => return Err(Unreadable),
        };
        if theme.name.is_empty() {
            return Err(Unreadable);
        }
        Ok(theme)
    }
}

/// Trims a name, drops the characters the stored form uses, and keeps at most 40 characters
/// and `MAX_NAME_BYTES` bytes.
pub fn clean_name(name: &str) -> String {
    let mut cleaned = String::new();
    let kept = name
        .trim()
        .chars()
        .filter(|character| !matches!(character, ';' | '=') && !character.is_control())
        .take(MAX_NAME_CHARS);
    for character in kept {
        if cleaned.len() + character.len_utf8() > MAX_NAME_BYTES {
            break;
        }
        cleaned.push(character);
    }
    cleaned
}

/// Accepts exactly `#RRGGBB`.
pub fn parse_hex(text: &str) -> Option<Color> {
    let digits = text.trim().strip_prefix('#')?;
    if digits.len() != 6 || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(digits, 16).ok().map(Color::hex)
}

/// Writes a colour as uppercase `#RRGGBB`.
pub fn hex(color: Color) -> String {
    format!("#{:02X}{:02X}{:02X}", color.red, color.green, color.blue)
}

fn parse_mode(text: &str) -> Result<Mode, Unreadable> {
    match text {
        "dark" => Ok(Mode::Dark),
        "light" => Ok(Mode::Light),
        _ => Err(Unreadable),
    }
}

fn parse_home(text: &str) -> Result<bool, Unreadable> {
    match text {
        "see-through" => Ok(true),
        "solid" => Ok(false),
        _ => Err(Unreadable),
    }
}

fn parse_material(text: &str) -> Result<Material, Unreadable> {
    Material::ALL
        .into_iter()
        .find(|material| material.id() == text)
        .ok_or(Unreadable)
}

fn parse_in(text: &str, range: Range) -> Result<u16, Unreadable> {
    text.parse::<u16>()
        .ok()
        .filter(|value| range.holds(*value))
        .ok_or(Unreadable)
}

fn solid_colour(ground: &Ground) -> Option<Color> {
    match ground {
        Ground::Solid(color) => Some(*color),
        Ground::Gradient { .. } | Ground::Material { .. } => None,
    }
}

/// Give the template's opaque neutral colours the background's hue, keeping each colour's
/// lightness so text keeps the template's contrast. Accent, tiles and plugins keep theirs.
fn recolour(drawn: &mut Palette, background: Color, mode: Mode) {
    let (hue, saturation) = background.hue_and_saturation();
    let shade = |color: Color| {
        if !color.is_opaque() {
            return color;
        }
        let lightness = color.lightness_percent();
        // Text stays nearly neutral; surfaces take more of the colour.
        let cap = match (mode, lightness > 50.) {
            (Mode::Dark, true) | (Mode::Light, false) => 12.,
            (Mode::Dark, false) | (Mode::Light, true) => 30.,
        };
        Color::hsl(hue, saturation.min(cap), lightness)
    };
    for role in [
        &mut drawn.text,
        &mut drawn.text_strong,
        &mut drawn.text_secondary,
        &mut drawn.text_secondary_bright,
        &mut drawn.text_muted,
        &mut drawn.icon,
        &mut drawn.icon_dim,
        &mut drawn.disabled,
        &mut drawn.line,
        &mut drawn.key,
        &mut drawn.dash,
        &mut drawn.dash_secondary,
        &mut drawn.card,
        &mut drawn.card_line,
        &mut drawn.card_ring,
        &mut drawn.surface,
        &mut drawn.surface_line,
        &mut drawn.pill,
        &mut drawn.pill_line,
        &mut drawn.chip_line,
        &mut drawn.panel,
        &mut drawn.panel_line,
        &mut drawn.panel_side,
        &mut drawn.panel_divider,
        &mut drawn.input,
        &mut drawn.input_line,
    ] {
        *role = shade(*role);
    }
}

/// Whether `role` is a text or icon role that is lightened or darkened to stay readable.
pub fn is_readable_role(role: &str) -> bool {
    READABLE_ROLES.contains(&role)
}

/// Text and icon roles, and how far each may move to stay readable.
const READABLE_ROLES: [&str; 7] = [
    "text",
    "text_strong",
    "text_secondary",
    "text_secondary_bright",
    "text_muted",
    "icon",
    "icon_dim",
];

/// Whether a theme is drawn as a preset or as one the owner saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Origin {
    Preset,
    Saved,
}

/// The preset whose failing contrast pairs a theme may keep, if any. The canvas's Glass has
/// nine recorded text exceptions (muted text at about 3:1 on the pill and selected row among
/// them); only the Glass preset keeps them. A saved theme built on Glass keeps none, so all
/// its text is lifted to 4.5:1 (#93 review). Haze's two would still excuse muted text on the
/// selected row (3.97:1), which the review asked to lift too.
fn exceptions_from(template: PaletteId, origin: Origin) -> Option<PaletteId> {
    match (template, origin) {
        (PaletteId::Glass, Origin::Saved) => None,
        (template, _) => Some(template),
    }
}

/// Lighten text on a dark theme, or darken it on a light one, two points of lightness at a
/// time, until no role fails beyond the pairs `exempt` already fails.
fn keep_text_readable(drawn: &mut Palette, accent: Accent, mode: Mode, exempt: Option<PaletteId>) {
    let step = match mode {
        Mode::Dark => 2.,
        Mode::Light => -2.,
    };
    let inherited = inherited_failures(exempt, accent);
    for _ in 0..50 {
        let mut failing: Vec<&str> = failures_beyond(drawn, &inherited)
            .iter()
            .map(|check| check.foreground)
            .filter(|role| is_readable_role(role))
            .collect();
        if failing.is_empty() {
            return;
        }
        // A role failing on several surfaces still moves one step per pass.
        failing.sort_unstable();
        failing.dedup();
        for role in failing {
            if let Some(color) = text_role(drawn, role) {
                let (hue, saturation) = color.hue_and_saturation();
                let lightness = (color.lightness_percent() + step).clamp(0., 100.);
                *color = Color::hsl(hue, saturation, lightness);
            }
        }
    }
}

fn text_role<'a>(drawn: &'a mut Palette, role: &str) -> Option<&'a mut Color> {
    match role {
        "text" => Some(&mut drawn.text),
        "text_strong" => Some(&mut drawn.text_strong),
        "text_secondary" => Some(&mut drawn.text_secondary),
        "text_secondary_bright" => Some(&mut drawn.text_secondary_bright),
        "text_muted" => Some(&mut drawn.text_muted),
        "icon" => Some(&mut drawn.icon),
        "icon_dim" => Some(&mut drawn.icon_dim),
        _ => None,
    }
}

/// The (foreground, background) pairs a preset already fails with `accent`: recorded design
/// exceptions (`https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-07-design-tokens.md`).
/// No preset means no exceptions.
fn inherited_failures(id: Option<PaletteId>, accent: Accent) -> Vec<(&'static str, &'static str)> {
    let Some(id) = id else {
        return Vec::new();
    };
    contrast::audit(&palette(id, accent))
        .into_iter()
        .filter(|check| !check.passes())
        .map(|check| (check.foreground, check.background))
        .collect()
}

fn failures_beyond(drawn: &Palette, inherited: &[(&str, &str)]) -> Vec<ContrastCheck> {
    contrast::audit(drawn)
        .into_iter()
        .filter(|check| !check.passes())
        .filter(|check| !inherited.contains(&(check.foreground, check.background)))
        .collect()
}

fn alpha_of(color: Color) -> f32 {
    f32::from(color.alpha) / 255.
}

fn with_alpha(color: Color, opacity: f32) -> Color {
    Color::rgba(color.red, color.green, color.blue, opacity)
}

fn mix(from: Color, to: Color, share: f32) -> Color {
    let channel =
        |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * share).round() as u8;
    Color::rgba(
        channel(from.red, to.red),
        channel(from.green, to.green),
        channel(from.blue, to.blue),
        1.,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_kind() -> Vec<ThemeSpec> {
        let mut themes = vec![ThemeSpec::new("Plain")];
        for id in [PaletteId::Haze, PaletteId::Glass, PaletteId::Tint] {
            themes.push(
                ThemeSpec::from_retired_palette(id, Accent::default()).expect("retired palette"),
            );
        }
        themes.push(ThemeSpec {
            mode: Mode::Light,
            background: Color::hex(0xE7EEF8),
            material: Material::MicaAlt,
            transparency: 35,
            ..ThemeSpec::new("Light blue glass")
        });
        // Home thins Haze's dark card, which a dark theme under Mica starts from.
        themes.push(ThemeSpec {
            material: Material::Mica,
            transparency: 60,
            see_through_home: true,
            ..ThemeSpec::new("Clear Home")
        });
        themes
    }

    #[test]
    fn a_new_theme_draws_exactly_graphite() {
        let accent = Accent::default();
        let mut expected = palette(PaletteId::Graphite, accent);
        expected.badge_ring = Color::hex(0x141519);
        assert_eq!(ThemeSpec::new("Mine").palette(accent, false), expected);
    }

    #[test]
    fn each_kind_of_theme_starts_from_its_preset() {
        let glass = ThemeSpec::from_retired_palette(PaletteId::Glass, Accent::default());
        let glass = glass.expect("glass");
        assert_eq!(glass.template(false), PaletteId::Glass);
        assert_eq!(glass.template(true), PaletteId::Graphite);
        let mica = ThemeSpec {
            material: Material::Mica,
            ..glass.clone()
        };
        assert_eq!(mica.template(false), PaletteId::Haze);
        let light = ThemeSpec {
            mode: Mode::Light,
            ..glass
        };
        assert_eq!(light.template(false), PaletteId::Paper);
        assert_eq!(ThemeSpec::new("Plain").template(false), PaletteId::Graphite);
    }

    #[test]
    fn themes_take_their_presets_drops_with_their_own_glass_edges() {
        let accent = Accent::default();
        for theme in every_kind() {
            let template = theme.template(false);
            let strength = f32::from(theme.edges) / 100.;
            let expected = crate::palette::template_drops(template)
                .with_glass_edges(strength, theme.mode == Mode::Light);
            assert_eq!(
                theme.palette(accent, false).shadows,
                expected,
                "{}",
                theme.name
            );
        }
    }

    /// The lowest ratio `role` reaches on `surface` in `drawn`.
    fn worst_ratio(drawn: &Palette, role: &str, surface: &str) -> f64 {
        contrast::audit(drawn)
            .into_iter()
            .filter(|check| check.foreground == role && check.background == surface)
            .map(|check| check.ratio)
            .fold(f64::INFINITY, f64::min)
    }

    #[test]
    fn a_saved_glass_theme_lifts_muted_text_but_the_preset_keeps_the_canvas() {
        let accent = Accent::default();
        // Saved themes under Acrylic: the old carried-over Glass and a less clear one.
        for transparency in [40, 55] {
            let theme = ThemeSpec {
                transparency,
                ..ThemeSpec::glass()
            };
            let drawn = theme.palette(accent, false);
            for surface in ["pill", "row_selected"] {
                let ratio = worst_ratio(&drawn, "text_muted", surface);
                assert!(
                    ratio >= contrast::TEXT_MINIMUM,
                    "{transparency} {surface}: {ratio}"
                );
            }
        }
        // A background too pale for its tint lets white text fall under 4.5:1 over the bright
        // stand-in wallpaper, so the editor reports it instead of hiding it behind the preset's
        // exceptions.
        let pale = ThemeSpec {
            background: Color::hex(0x8A8E9A),
            transparency: 90,
            ..ThemeSpec::glass()
        };
        assert!(
            !readable_failures(&pale, accent).is_empty(),
            "a pale dark Acrylic theme is reported"
        );
        // The preset draws the canvas's own inks, exceptions included.
        let preset = ThemeSpec::glass().preset_palette(accent, false);
        let canvas = palette(PaletteId::Glass, accent);
        assert_eq!(preset.text_muted, canvas.text_muted);
        assert_eq!(preset.text, canvas.text);
        assert!(worst_ratio(&preset, "text_muted", "pill") < contrast::TEXT_MINIMUM);
    }

    #[test]
    fn the_glass_preset_holds_the_canvas_settings() {
        let glass = ThemeSpec::glass();
        assert_eq!(glass.name, "Glass");
        assert_eq!(glass.mode, Mode::Dark);
        assert_eq!(glass.background, Color::hex(0x101118));
        assert_eq!(glass.fill, Fill::Solid);
        assert_eq!(glass.material, Material::Acrylic);
        assert_eq!((glass.transparency, glass.edges, glass.grain), (50, 24, 0));
        assert_eq!(
            ThemeSpec::from_retired_palette(PaletteId::Glass, Accent::default()),
            Some(glass)
        );
    }

    #[test]
    fn the_glass_theme_draws_the_canvas_glass() {
        // The canvas's `glass` tokens (design/canvas/project/Main.dc.html, lines 271-293).
        let accent = Accent::default();
        let theme = ThemeSpec::from_retired_palette(PaletteId::Glass, accent).expect("glass");
        let drawn = theme.palette(accent, false);
        let canvas = palette(PaletteId::Glass, accent);
        assert_eq!(drawn.id, PaletteId::Glass);
        assert_eq!(drawn.shadows, canvas.shadows);
        for (role, ours, theirs) in [
            ("field", drawn.field, canvas.field),
            ("row_selected", drawn.row_selected, canvas.row_selected),
            ("card", drawn.card, canvas.card),
            ("card_line", drawn.card_line, canvas.card_line),
            ("surface", drawn.surface, canvas.surface),
            ("surface_line", drawn.surface_line, canvas.surface_line),
            ("pill", drawn.pill, canvas.pill),
            ("pill_line", drawn.pill_line, canvas.pill_line),
            ("panel", drawn.panel, canvas.panel),
        ] {
            assert_eq!(ours, theirs, "{role}");
        }
        // `materialBg: rgba(16,17,24,0.46)`; Transparency moves in steps of 5, so 50 % is
        // the nearest setting, a 1 - 50/90 = 0.44 tint.
        let Ground::Material { material, tint, .. } = drawn.ground else {
            panic!("Glass shows a material");
        };
        assert_eq!(material, WindowMaterial::Acrylic);
        assert_eq!(tint, Color::rgba(16, 17, 24, 1. - 50. / 90.));
    }

    #[test]
    fn the_window_tint_thins_as_transparency_rises() {
        let accent = Accent::default();
        let glass = ThemeSpec::from_retired_palette(PaletteId::Glass, accent).expect("glass");
        let mut previous = u8::MAX;
        for transparency in (TRANSPARENCY.step..=TRANSPARENCY.max).step_by(5) {
            let theme = ThemeSpec {
                transparency,
                ..glass.clone()
            };
            let Ground::Material { tint, .. } = theme.palette(accent, false).ground else {
                panic!("{transparency}: no material");
            };
            // Dark Acrylic keeps its floor at the slider's clear end.
            let opacity = (1. - f32::from(transparency) / 90.).max(DARK_ACRYLIC_TINT_FLOOR);
            assert_eq!(tint, Color::rgba(16, 17, 24, opacity), "{transparency}");
            assert!(tint.alpha <= previous, "{transparency}");
            previous = tint.alpha;
        }
    }

    #[test]
    fn hazes_home_card_thins_only_with_the_home_option_and_glass_frost_stays() {
        let accent = Accent::default();
        let haze = ThemeSpec::from_retired_palette(PaletteId::Haze, accent).expect("haze");
        let card_at = |theme: &ThemeSpec, transparency, see_through_home| {
            ThemeSpec {
                transparency,
                see_through_home,
                ..theme.clone()
            }
            .palette(accent, false)
            .card
        };
        // Haze keeps Home solid, so its card is the canvas's `rgba(10,11,14,0.5)` at any step.
        assert!(!haze.see_through_home);
        let canvas = Color::rgba(10, 11, 14, 0.5).alpha;
        for transparency in [25, 60, 90] {
            assert_eq!(card_at(&haze, transparency, false).alpha, canvas);
        }
        // See-through Home thins the card as Transparency rises (owner decision 2026-10-24).
        assert!(card_at(&haze, 60, true).alpha < card_at(&haze, 25, true).alpha);
        assert!(card_at(&haze, 90, true).alpha < card_at(&haze, 60, true).alpha);
        let glass = ThemeSpec::from_retired_palette(PaletteId::Glass, accent).expect("glass");
        let frost = Color::rgba(255, 255, 255, 0.05);
        for transparency in [20, 50] {
            assert_eq!(card_at(&glass, transparency, false), frost);
        }
        // Clearer than the surface floor, the frost keeps a dark footing under it.
        let footed = glass.backed(frost, DARK_ACRYLIC_SURFACE_FLOOR, DARK_ACRYLIC_TINT_FLOOR);
        assert_eq!(card_at(&glass, 90, false), footed);
    }

    #[test]
    fn glass_edges_follow_their_setting() {
        let accent = Accent::default();
        let glass = ThemeSpec::from_retired_palette(PaletteId::Glass, accent).expect("glass");
        let field_highlight = |edges| {
            ThemeSpec {
                edges,
                ..glass.clone()
            }
            .palette(accent, false)
            .shadows
            .field
            .inset_layers()
            .next()
            .map(|layer| layer.color)
        };
        assert_eq!(field_highlight(0), None);
        assert_eq!(field_highlight(24), Some(Color::rgba(255, 255, 255, 0.24)));
        assert_eq!(field_highlight(40), Some(Color::rgba(255, 255, 255, 0.40)));
    }

    #[test]
    fn every_theme_round_trips_through_one_settings_value() {
        for theme in every_kind() {
            let stored = theme.to_setting();
            assert!(stored.len() <= 256, "{stored}");
            assert_eq!(ThemeSpec::parse(&stored), Ok(theme));
        }
    }

    #[test]
    fn damaged_or_foreign_values_are_refused() {
        let good = ThemeSpec::new("Ok").to_setting();
        for bad in [
            String::new(),
            "v2;name=x".to_owned(),
            good.replace("transparency=50", "transparency=51"),
            good.replace("mode=dark", "mode=dim"),
            good.replace("bg=#141519", "bg=#14151"),
            good.replace("name=Ok", "name="),
            format!("{good};extra=1"),
        ] {
            assert_eq!(ThemeSpec::parse(&bad), Err(Unreadable), "{bad}");
        }
    }

    #[test]
    fn the_longest_names_still_fit_one_settings_value() {
        for character in ["x", "é", "🦅"] {
            let theme = ThemeSpec {
                fill: Fill::Gradient {
                    second: Color::hex(0xFFFFFF),
                    angle: 355,
                },
                material: Material::MicaAlt,
                transparency: 90,
                see_through_home: true,
                edges: 40,
                grain: 15,
                ..ThemeSpec::new(&character.repeat(60))
            };
            let stored = theme.to_setting();
            assert!(stored.len() <= 256, "{} bytes", stored.len());
            assert_eq!(ThemeSpec::parse(&stored), Ok(theme));
        }
    }

    #[test]
    fn a_retired_tint_follows_the_chosen_accent() {
        let blue = Accent::new(Color::hex(0x6EA8FF));
        let tint = ThemeSpec::from_retired_palette(PaletteId::Tint, blue).expect("tint");
        assert_eq!(tint.background, blue.tone(34., 15.));
    }

    #[test]
    fn names_cannot_break_the_stored_form() {
        let name = format!("a;b=c\n{}", "x".repeat(60));
        let theme = ThemeSpec::new(&name);
        assert!(!theme.name.contains([';', '=', '\n']));
        assert_eq!(theme.name.chars().count(), MAX_NAME_CHARS);
        assert_eq!(ThemeSpec::parse(&theme.to_setting()), Ok(theme));
    }

    #[test]
    fn reduce_transparency_draws_the_background_opaque() {
        let glass =
            ThemeSpec::from_retired_palette(PaletteId::Glass, Accent::default()).expect("glass");
        assert!(matches!(
            glass.palette(Accent::default(), false).ground,
            Ground::Material { .. }
        ));
        assert_eq!(
            glass.palette(Accent::default(), true).ground,
            Ground::Solid(glass.background)
        );
    }

    #[test]
    fn retired_palettes_and_new_themes_keep_text_readable() {
        for theme in every_kind() {
            let text_failures: Vec<_> = theme
                .contrast_problems(Accent::default())
                .into_iter()
                .filter(|check| check.foreground.starts_with("text"))
                .map(|check| check.label())
                .collect();
            assert!(
                text_failures.is_empty(),
                "{}: {text_failures:?}",
                theme.name
            );
        }
    }

    #[test]
    fn a_bright_dark_mode_background_is_reported() {
        let theme = ThemeSpec {
            background: Color::hex(0xE0E0E0),
            ..ThemeSpec::new("Too bright")
        };
        assert!(!theme.contrast_problems(Accent::default()).is_empty());
    }

    fn see_through(material: Material, transparency: u16, see_through_home: bool) -> ThemeSpec {
        ThemeSpec {
            material,
            transparency,
            see_through_home,
            ..ThemeSpec::new("Frosted")
        }
    }

    fn acrylic(transparency: u16, see_through_home: bool) -> ThemeSpec {
        see_through(Material::Acrylic, transparency, see_through_home)
    }

    /// A dark theme under Mica starts from Haze, whose Home card is a dark tint.
    fn mica(transparency: u16, see_through_home: bool) -> ThemeSpec {
        see_through(Material::Mica, transparency, see_through_home)
    }

    fn tint_of(theme: &ThemeSpec) -> Color {
        let Ground::Material { tint, .. } = theme.palette(Accent::default(), false).ground else {
            panic!("{}: no material", theme.transparency);
        };
        tint
    }

    #[test]
    fn the_slider_thins_the_tint_to_nothing_at_its_top() {
        assert_eq!(mica(0, false).tint_opacity(), 1.);
        assert_eq!(mica(45, false).tint_opacity(), 0.5);
        assert_eq!(mica(90, false).tint_opacity(), 0.);
        // No floor under Mica: at 90 % only Windows' own material is drawn behind the chrome.
        assert_eq!(tint_of(&mica(90, false)).alpha, 0);
        let mut previous = u8::MAX;
        for transparency in (TRANSPARENCY.step..=TRANSPARENCY.max).step_by(5) {
            let tint = tint_of(&mica(transparency, false));
            assert!(tint.alpha < previous, "{transparency}");
            previous = tint.alpha;
        }
    }

    #[test]
    fn dark_acrylic_keeps_its_tint_floor_and_light_acrylic_does_not() {
        assert_eq!(acrylic(45, false).tint_opacity(), 0.5);
        for transparency in [80, 85, 90] {
            let opacity = acrylic(transparency, false).tint_opacity();
            assert_eq!(opacity, DARK_ACRYLIC_TINT_FLOOR, "{transparency}");
        }
        let light = ThemeSpec {
            mode: Mode::Light,
            background: Color::hex(0xE7EEF8),
            ..acrylic(90, false)
        };
        assert_eq!(light.tint_opacity(), 0.);
    }

    #[test]
    fn homes_card_follows_the_slider_only_when_asked() {
        let accent = Accent::default();
        let card = |theme: ThemeSpec| theme.palette(accent, false).card;
        // Off: the template's card, Haze's `rgba(10,11,14,0.5)`, whatever the slider says.
        let template = Color::rgba(10, 11, 14, 0.5).alpha;
        assert_eq!(card(mica(90, false)).alpha, template);
        // On: the card thins with the window's tint, and is clear at the top.
        assert_eq!(card(mica(45, true)).alpha, Color::rgba(0, 0, 0, 0.25).alpha);
        assert_eq!(card(mica(90, true)).alpha, 0);
        // With no material shown the option changes nothing.
        let opaque = ThemeSpec {
            material: Material::None,
            ..mica(90, true)
        };
        assert_eq!(card(opaque), palette(PaletteId::Graphite, accent).card);
        // Reduce transparency draws the card opaque again.
        assert_eq!(
            mica(90, true).palette(accent, true).card,
            palette(PaletteId::Graphite, accent).card
        );
    }

    #[test]
    fn the_backdrop_is_light_or_dark_as_the_theme_is() {
        let accent = Accent::default();
        let light = ThemeSpec {
            mode: Mode::Light,
            background: Color::hex(0xE7EEF8),
            ..acrylic(60, false)
        };
        assert!(light.palette(accent, false).is_light());
        assert!(!acrylic(60, false).palette(accent, false).is_light());
        assert!(palette(PaletteId::Paper, accent).is_light());
        assert!(!palette(PaletteId::Graphite, accent).is_light());
    }

    #[test]
    fn materials_are_offered_as_off_mica_mica_alt_and_acrylic() {
        let names: Vec<_> = Material::ALL.into_iter().map(Material::name).collect();
        assert_eq!(names, ["Off", "Mica", "Mica Alt", "Acrylic"]);
    }

    #[test]
    fn themes_saved_before_the_home_option_still_read() {
        // A value exactly as builds before 2026-10-22 wrote it.
        let stored = "v1;name=Old;mode=dark;bg=#101118;fill=solid;bg2=#101118;angle=155;\
                      material=acrylic;transparency=55;edges=24;grain=0";
        let theme = ThemeSpec::parse(stored).expect("an old theme reads");
        assert!(!theme.see_through_home);
        assert_eq!(theme.to_setting(), stored);
        let clear = ThemeSpec {
            see_through_home: true,
            ..theme
        };
        assert!(clear.to_setting().ends_with(";home=see-through"));
        assert_eq!(ThemeSpec::parse(&clear.to_setting()), Ok(clear));
        let bad = format!("{stored};home=foggy");
        assert_eq!(ThemeSpec::parse(&bad), Err(Unreadable));
    }

    #[test]
    fn text_stays_readable_at_every_transparency_in_both_modes() {
        let accent = Accent::default();
        let modes = [
            (Mode::Dark, Color::hex(0x141519)),
            (Mode::Light, Color::hex(0xE7EEF8)),
        ];
        let materials = [Material::Mica, Material::MicaAlt, Material::Acrylic];
        for (mode, background) in modes {
            for material in materials {
                for see_through_home in [false, true] {
                    for transparency in (0..=TRANSPARENCY.max).step_by(5) {
                        let theme = ThemeSpec {
                            mode,
                            background,
                            ..see_through(material, transparency, see_through_home)
                        };
                        let failing = readable_failures(&theme, accent);
                        assert!(
                            failing.is_empty(),
                            "{mode:?} {material:?} {transparency} home {see_through_home}: \
                             {failing:?}"
                        );
                    }
                }
            }
        }
    }

    fn readable_failures(theme: &ThemeSpec, accent: Accent) -> Vec<String> {
        theme
            .contrast_problems(accent)
            .into_iter()
            .filter(|check| is_readable_role(check.foreground))
            .map(|check| check.label())
            .collect()
    }

    #[test]
    fn steps_stay_in_range_and_the_angle_wraps() {
        assert_eq!(TRANSPARENCY.stepped(85, 1), 90);
        assert_eq!(TRANSPARENCY.stepped(90, 1), 90);
        assert_eq!(GRAIN.stepped(0, -1), 0);
        assert_eq!(ANGLE.wrapped(355, 1), 0);
        assert_eq!(ANGLE.wrapped(0, -1), 355);
    }
}
