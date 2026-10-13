//! Turns the Customize choices into stored settings keys and back (P1-29).

use std::collections::BTreeMap;

use syrinx_ui::{
    ACCENT_PRESETS, Accent, Color, FORMER_DEFAULT_ACCENT, Palette, PaletteId, palette,
};

/// The settings keys this slice owns. Other slices add their own keys to the same table.
pub const PALETTE_KEY: &str = "appearance.palette";
pub const ACCENT_KEY: &str = "appearance.accent";
pub const REDUCE_TRANSPARENCY_KEY: &str = "appearance.reduce_transparency";

/// The Customize panel's choices.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Appearance {
    pub palette: PaletteId,
    pub accent: Accent,
    pub reduce_transparency: bool,
}

impl Default for Appearance {
    /// Graphite with the default accent (the logo's orange), transparency allowed.
    fn default() -> Self {
        Self {
            palette: PaletteId::Graphite,
            accent: Accent::default(),
            reduce_transparency: false,
        }
    }
}

/// Which stored values could not be read and fell back to their defaults.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Unreadable(pub Vec<&'static str>);

impl Appearance {
    /// Reads the three keys; a missing key is the default, an unreadable one is reported.
    pub fn from_settings(settings: &BTreeMap<String, String>) -> (Self, Unreadable) {
        let mut appearance = Self::default();
        let mut unreadable = Unreadable::default();
        if let Some(value) = settings.get(PALETTE_KEY) {
            match palette_from_id(value) {
                Some(id) => appearance.palette = id,
                None => unreadable.0.push(PALETTE_KEY),
            }
        }
        if let Some(value) = settings.get(ACCENT_KEY) {
            match parse_accent(value) {
                Some(accent) => appearance.accent = migrate_former_default(accent),
                None => unreadable.0.push(ACCENT_KEY),
            }
        }
        if let Some(value) = settings.get(REDUCE_TRANSPARENCY_KEY) {
            match value.as_str() {
                "true" => appearance.reduce_transparency = true,
                "false" => appearance.reduce_transparency = false,
                _ => unreadable.0.push(REDUCE_TRANSPARENCY_KEY),
            }
        }
        (appearance, unreadable)
    }

    /// All three keys, so a saved choice never depends on a later default.
    pub fn to_settings(self) -> Vec<(String, String)> {
        vec![
            (PALETTE_KEY.to_owned(), self.palette.id().to_owned()),
            (ACCENT_KEY.to_owned(), accent_hex(self.accent)),
            (
                REDUCE_TRANSPARENCY_KEY.to_owned(),
                self.reduce_transparency.to_string(),
            ),
        ]
    }

    /// The palette actually drawn: Reduce transparency turns Haze and Glass into Graphite.
    pub fn drawn_palette(self) -> Palette {
        palette(self.palette.resolve(self.reduce_transparency), self.accent)
    }

    /// Whether the chosen accent is one of the Customize panel's presets.
    pub fn uses_preset_accent(self) -> bool {
        ACCENT_PRESETS.contains(&self.accent.fill())
    }
}

/// The canvas id back to a palette.
pub fn palette_from_id(id: &str) -> Option<PaletteId> {
    PaletteId::ALL
        .into_iter()
        .find(|palette| palette.id() == id)
}

/// A stored `#FF8A65`, the default and first swatch until 2026-10-13, now means the default:
/// the owner decided a saved former default follows the new one. Other colours stay as chosen.
fn migrate_former_default(accent: Accent) -> Accent {
    if accent.fill() == FORMER_DEFAULT_ACCENT {
        return Accent::default();
    }
    accent
}

/// Accepts exactly `#RRGGBB`, the form the Customize panel shows.
pub fn parse_accent(text: &str) -> Option<Accent> {
    let digits = text.trim().strip_prefix('#')?;
    if digits.len() != 6 || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let rgb = u32::from_str_radix(digits, 16).ok()?;
    Some(Accent::new(Color::hex(rgb)))
}

/// Writes an accent as uppercase `#RRGGBB`.
pub fn accent_hex(accent: Accent) -> String {
    let color = accent.fill();
    format!("#{:02X}{:02X}{:02X}", color.red, color.green, color.blue)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn every_palette_and_accent_roundtrips_through_settings() {
        for palette in PaletteId::ALL {
            for accent in ACCENT_PRESETS.into_iter().chain([Color::hex(0x12AB9C)]) {
                for reduce_transparency in [false, true] {
                    let appearance = Appearance {
                        palette,
                        accent: Accent::new(accent),
                        reduce_transparency,
                    };
                    let settings: BTreeMap<_, _> = appearance.to_settings().into_iter().collect();
                    assert_eq!(
                        Appearance::from_settings(&settings),
                        (appearance, Unreadable::default())
                    );
                }
            }
        }
    }

    #[test]
    fn missing_keys_use_defaults_and_unreadable_keys_are_reported() {
        assert_eq!(
            Appearance::from_settings(&BTreeMap::new()),
            (Appearance::default(), Unreadable::default())
        );
        let (appearance, unreadable) = Appearance::from_settings(&stored(&[
            (PALETTE_KEY, "neon"),
            (ACCENT_KEY, "#12345"),
            (REDUCE_TRANSPARENCY_KEY, "yes"),
        ]));
        assert_eq!(appearance, Appearance::default());
        assert_eq!(
            unreadable.0,
            [PALETTE_KEY, ACCENT_KEY, REDUCE_TRANSPARENCY_KEY]
        );
    }

    #[test]
    fn a_saved_former_default_accent_loads_as_the_new_default() {
        // #FF8A65 was the default and a preset until 2026-10-13; the owner decided it follows.
        for spelling in ["#FF8A65", "#ff8a65"] {
            let (appearance, unreadable) =
                Appearance::from_settings(&stored(&[(ACCENT_KEY, spelling)]));
            assert_eq!(unreadable, Unreadable::default());
            assert_eq!(appearance.accent, Accent::default());
            assert_eq!(appearance.accent.fill(), Color::hex(0xFF612E));
            assert!(appearance.uses_preset_accent());
        }
    }

    #[test]
    fn a_saved_custom_accent_is_unchanged_by_the_default_migration() {
        for (stored_text, color) in [("#FF8A66", 0xFF8A66), ("#12AB9C", 0x12AB9C)] {
            let (appearance, _) = Appearance::from_settings(&stored(&[(ACCENT_KEY, stored_text)]));
            assert_eq!(appearance.accent.fill(), Color::hex(color));
            assert!(!appearance.uses_preset_accent());
        }
    }

    #[test]
    fn reduce_transparency_draws_see_through_palettes_as_graphite() {
        for (chosen, drawn) in [
            (PaletteId::Haze, PaletteId::Graphite),
            (PaletteId::Glass, PaletteId::Graphite),
            (PaletteId::Paper, PaletteId::Paper),
        ] {
            let appearance = Appearance {
                palette: chosen,
                reduce_transparency: true,
                ..Appearance::default()
            };
            assert_eq!(appearance.drawn_palette().id, drawn);
        }
    }

    #[test]
    fn accent_text_is_strict_hex() {
        assert_eq!(
            parse_accent("#6ea8ff").map(accent_hex),
            Some("#6EA8FF".to_owned())
        );
        for text in ["6EA8FF", "#6EA8F", "#6EA8FFF", "#GGGGGG", "#+1234F"] {
            assert_eq!(parse_accent(text), None, "{text}");
        }
    }
}
