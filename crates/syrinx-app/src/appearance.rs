//! Turns the Customize choices, including the owner's named themes, into stored settings keys
//! and back (P1-29, `docs/theme-editor.md`).

use std::collections::BTreeMap;

use syrinx_ui::theme::{Material, ThemeSpec};
use syrinx_ui::{ACCENT_PRESETS, Accent, FORMER_DEFAULT_ACCENT, Palette, PaletteId, palette};

/// The settings keys this slice owns. Other slices add their own keys to the same table.
pub const THEME_KEY: &str = "appearance.theme";
pub const ACCENT_KEY: &str = "appearance.accent";
pub const REDUCE_TRANSPARENCY_KEY: &str = "appearance.reduce_transparency";

/// Before the editor, one of five palettes; read only to carry an old choice over.
pub const PALETTE_KEY: &str = "appearance.palette";

/// A saved theme's key is this prefix and its slot number, 1 to `MAX_THEMES`.
const CUSTOM_KEY_PREFIX: &str = "appearance.custom.";

/// How many themes can be saved; each takes one settings row.
pub const MAX_THEMES: usize = 12;

/// Which theme the window draws.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThemeChoice {
    Graphite,
    Paper,
    /// The canvas's see-through Glass (owner decision 2026-10-21).
    Glass,
    /// A saved theme, by its slot (0-based).
    Custom(usize),
}

/// The Customize panel's choices.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Appearance {
    pub theme: ThemeChoice,
    pub accent: Accent,
    pub reduce_transparency: bool,
}

impl Default for Appearance {
    /// Graphite with the default accent (the logo's orange), opaque.
    fn default() -> Self {
        Self {
            theme: ThemeChoice::Graphite,
            accent: Accent::default(),
            reduce_transparency: false,
        }
    }
}

/// The presets Customize lists above the saved themes, in order: the choice, its name and
/// the line under it.
pub const PRESETS: [(ThemeChoice, &str, &str); 3] = [
    (
        ThemeChoice::Graphite,
        "Graphite",
        "Solid dark. The default.",
    ),
    (ThemeChoice::Paper, "Paper", "Light."),
    (
        ThemeChoice::Glass,
        "Glass",
        "See-through, with light edges.",
    ),
];

/// The owner's saved themes, by slot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SavedThemes(Vec<Option<ThemeSpec>>);

impl Default for SavedThemes {
    fn default() -> Self {
        Self(vec![None; MAX_THEMES])
    }
}

impl SavedThemes {
    pub fn get(&self, slot: usize) -> Option<&ThemeSpec> {
        self.0.get(slot).and_then(Option::as_ref)
    }

    /// Every saved theme with its slot, in slot order.
    pub fn iter(&self) -> impl Iterator<Item = (usize, &ThemeSpec)> {
        self.0
            .iter()
            .enumerate()
            .filter_map(|(slot, theme)| theme.as_ref().map(|theme| (slot, theme)))
    }

    pub fn first_free(&self) -> Option<usize> {
        self.0.iter().position(Option::is_none)
    }

    /// Store `theme` in `slot`, returning the settings row to save. Names are cut to a byte
    /// budget, so the value always fits the settings table's 256 bytes.
    pub fn set(&mut self, slot: usize, theme: ThemeSpec) -> (String, String) {
        let value = theme.to_setting();
        debug_assert!(value.len() <= syrinx_store::MAX_SETTING_VALUE_BYTES);
        self.0[slot] = Some(theme);
        (custom_key(slot), value)
    }

    /// Free `slot`, returning the row to save; the table has no delete, so it is emptied.
    pub fn remove(&mut self, slot: usize) -> (String, String) {
        self.0[slot] = None;
        (custom_key(slot), String::new())
    }

    /// A name for a new theme that no saved theme uses yet.
    pub fn unused_name(&self, base: &str) -> String {
        let taken = |name: &str| self.iter().any(|(_, theme)| theme.name == name);
        if !taken(base) {
            return base.to_owned();
        }
        (2..)
            .map(|number| format!("{base} {number}"))
            .find(|name| !taken(name))
            .unwrap_or_else(|| base.to_owned())
    }
}

/// Which stored values could not be read and fell back to their defaults.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Unreadable(pub Vec<String>);

/// Everything read from the settings table.
#[derive(Clone, Debug, PartialEq)]
pub struct Loaded {
    pub appearance: Appearance,
    pub themes: SavedThemes,
    pub unreadable: Unreadable,
    /// Rows to save once: an old palette choice carried over into the new keys.
    pub carried_over: Vec<(String, String)>,
}

/// Reads the theme, accent, transparency and saved themes; a missing key is the default, an
/// unreadable one is reported and skipped.
pub fn load(settings: &BTreeMap<String, String>) -> Loaded {
    let mut unreadable = Unreadable::default();
    let themes = read_themes(settings, &mut unreadable);
    let mut loaded = Loaded {
        appearance: Appearance::default(),
        themes,
        unreadable,
        carried_over: Vec::new(),
    };
    // The accent first: a carried-over Tint is built in the chosen accent's colour.
    if let Some(value) = settings.get(ACCENT_KEY) {
        match parse_accent(value) {
            Some(accent) => loaded.appearance.accent = migrate_former_default(accent),
            None => loaded.unreadable.0.push(ACCENT_KEY.to_owned()),
        }
    }
    read_theme_choice(settings, &mut loaded);
    if let Some(value) = settings.get(REDUCE_TRANSPARENCY_KEY) {
        match value.as_str() {
            "true" => loaded.appearance.reduce_transparency = true,
            "false" => loaded.appearance.reduce_transparency = false,
            _ => loaded.unreadable.0.push(REDUCE_TRANSPARENCY_KEY.to_owned()),
        }
    }
    loaded
}

fn read_themes(settings: &BTreeMap<String, String>, unreadable: &mut Unreadable) -> SavedThemes {
    let mut themes = SavedThemes::default();
    for slot in 0..MAX_THEMES {
        let key = custom_key(slot);
        let Some(value) = settings.get(&key).filter(|value| !value.is_empty()) else {
            continue;
        };
        match ThemeSpec::parse(value) {
            Ok(theme) => themes.0[slot] = Some(theme),
            Err(_) => unreadable.0.push(key),
        }
    }
    themes
}

/// The chosen theme, or the old palette carried over when the theme key was never written.
fn read_theme_choice(settings: &BTreeMap<String, String>, loaded: &mut Loaded) {
    if let Some(value) = settings.get(THEME_KEY) {
        match parse_choice(value).filter(|choice| exists(*choice, &loaded.themes)) {
            Some(choice) => loaded.appearance.theme = choice,
            None => loaded.unreadable.0.push(THEME_KEY.to_owned()),
        }
        return;
    }
    let Some(old) = settings.get(PALETTE_KEY) else {
        return;
    };
    let retired = PaletteId::ALL.into_iter().find(|id| id.id() == old);
    match retired {
        Some(PaletteId::Graphite) | None => {}
        Some(PaletteId::Paper) => loaded.appearance.theme = ThemeChoice::Paper,
        // Glass is a preset again, so an old Glass choice needs no saved theme.
        Some(PaletteId::Glass) => {
            loaded.appearance.theme = ThemeChoice::Glass;
            loaded.carried_over.push(theme_row(ThemeChoice::Glass));
        }
        Some(id) => carry_over_retired(id, loaded),
    }
}

/// A chosen Haze, Glass or Tint becomes a saved theme of the same name, chosen.
fn carry_over_retired(id: PaletteId, loaded: &mut Loaded) {
    let (Some(theme), Some(slot)) = (
        ThemeSpec::from_retired_palette(id, loaded.appearance.accent),
        loaded.themes.first_free(),
    ) else {
        return;
    };
    loaded.carried_over.push(loaded.themes.set(slot, theme));
    loaded.appearance.theme = ThemeChoice::Custom(slot);
    loaded.carried_over.push(theme_row(loaded.appearance.theme));
}

fn exists(choice: ThemeChoice, themes: &SavedThemes) -> bool {
    match choice {
        ThemeChoice::Graphite | ThemeChoice::Paper | ThemeChoice::Glass => true,
        ThemeChoice::Custom(slot) => themes.get(slot).is_some(),
    }
}

impl Appearance {
    /// The theme, accent and transparency rows, so a saved choice never depends on a later
    /// default. Saved themes are written separately, one row each.
    pub fn to_settings(self) -> Vec<(String, String)> {
        vec![
            theme_row(self.theme),
            (ACCENT_KEY.to_owned(), accent_hex(self.accent)),
            (
                REDUCE_TRANSPARENCY_KEY.to_owned(),
                self.reduce_transparency.to_string(),
            ),
        ]
    }

    /// The custom theme chosen, if any.
    pub fn custom<'a>(&self, themes: &'a SavedThemes) -> Option<&'a ThemeSpec> {
        match self.theme {
            ThemeChoice::Custom(slot) => themes.get(slot),
            ThemeChoice::Graphite | ThemeChoice::Paper | ThemeChoice::Glass => None,
        }
    }

    /// The theme settings behind the choice: the Glass preset or a saved theme. Graphite and
    /// Paper are drawn from their palettes directly.
    fn spec(&self, themes: &SavedThemes) -> Option<ThemeSpec> {
        match self.theme {
            ThemeChoice::Glass => Some(ThemeSpec::glass()),
            _ => self.custom(themes).cloned(),
        }
    }

    /// Every colour the window draws with.
    pub fn drawn_palette(self, themes: &SavedThemes) -> Palette {
        if let Some(theme) = self.spec(themes) {
            return theme.palette(self.accent, self.reduce_transparency);
        }
        match self.theme {
            ThemeChoice::Paper => palette(PaletteId::Paper, self.accent),
            _ => palette(PaletteId::Graphite, self.accent),
        }
    }

    /// The material Windows should draw behind the window, `None` when it is opaque.
    pub fn material(self, themes: &SavedThemes) -> Option<Material> {
        self.spec(themes)
            .filter(|theme| theme.shows_material(self.reduce_transparency))
            .map(|theme| theme.material)
    }

    /// Whether the chosen accent is one of the Customize panel's presets.
    pub fn uses_preset_accent(self) -> bool {
        ACCENT_PRESETS.contains(&self.accent.fill())
    }
}

fn theme_row(choice: ThemeChoice) -> (String, String) {
    let value = match choice {
        ThemeChoice::Graphite => "graphite".to_owned(),
        ThemeChoice::Paper => "paper".to_owned(),
        ThemeChoice::Glass => "glass".to_owned(),
        ThemeChoice::Custom(slot) => format!("custom:{}", slot + 1),
    };
    (THEME_KEY.to_owned(), value)
}

fn parse_choice(text: &str) -> Option<ThemeChoice> {
    match text {
        "graphite" => Some(ThemeChoice::Graphite),
        "paper" => Some(ThemeChoice::Paper),
        "glass" => Some(ThemeChoice::Glass),
        _ => {
            let number: usize = text.strip_prefix("custom:")?.parse().ok()?;
            (1..=MAX_THEMES)
                .contains(&number)
                .then(|| ThemeChoice::Custom(number - 1))
        }
    }
}

fn custom_key(slot: usize) -> String {
    format!("{CUSTOM_KEY_PREFIX}{}", slot + 1)
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
    syrinx_ui::theme::parse_hex(text).map(Accent::new)
}

/// Writes an accent as uppercase `#RRGGBB`.
pub fn accent_hex(accent: Accent) -> String {
    syrinx_ui::theme::hex(accent.fill())
}

#[cfg(test)]
mod tests {
    use syrinx_ui::Color;

    use super::*;

    fn stored(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    /// The settings table after `rows` were saved over `start`.
    fn saved(
        start: &BTreeMap<String, String>,
        rows: Vec<(String, String)>,
    ) -> BTreeMap<String, String> {
        let mut table = start.clone();
        table.extend(rows);
        table
    }

    #[test]
    fn choices_and_themes_roundtrip_through_settings() {
        let mut themes = SavedThemes::default();
        let mut rows = vec![themes.set(3, ThemeSpec::new("Night"))];
        for theme in [
            ThemeChoice::Graphite,
            ThemeChoice::Paper,
            ThemeChoice::Glass,
            ThemeChoice::Custom(3),
        ] {
            for accent in ACCENT_PRESETS.into_iter().chain([Color::hex(0x12AB9C)]) {
                let appearance = Appearance {
                    theme,
                    accent: Accent::new(accent),
                    reduce_transparency: true,
                };
                rows.extend(appearance.to_settings());
                let loaded = load(&saved(&BTreeMap::new(), rows.clone()));
                assert_eq!(loaded.appearance, appearance);
                assert_eq!(loaded.themes, themes);
                assert_eq!(loaded.unreadable, Unreadable::default());
            }
        }
    }

    #[test]
    fn missing_keys_use_defaults_and_unreadable_keys_are_reported() {
        let empty = load(&BTreeMap::new());
        assert_eq!(empty.appearance, Appearance::default());
        assert_eq!(empty.unreadable, Unreadable::default());
        let loaded = load(&stored(&[
            (THEME_KEY, "custom:4"),
            (ACCENT_KEY, "#12345"),
            (REDUCE_TRANSPARENCY_KEY, "yes"),
            ("appearance.custom.2", "v1;broken"),
        ]));
        assert_eq!(loaded.appearance, Appearance::default());
        assert_eq!(
            loaded.unreadable.0,
            [
                "appearance.custom.2",
                ACCENT_KEY,
                THEME_KEY,
                REDUCE_TRANSPARENCY_KEY
            ]
        );
    }

    #[test]
    fn a_retired_palette_becomes_a_saved_theme_of_its_name_once() {
        for (old, name) in [("haze", "Haze"), ("tint", "Tint")] {
            let start = stored(&[(PALETTE_KEY, old)]);
            let first = load(&start);
            assert_eq!(first.appearance.theme, ThemeChoice::Custom(0));
            assert_eq!(
                first.themes.get(0).map(|theme| theme.name.as_str()),
                Some(name)
            );
            // After the carried-over rows are saved, the next start reads the new keys only.
            let second = load(&saved(&start, first.carried_over.clone()));
            assert_eq!(second.appearance, first.appearance);
            assert_eq!(second.themes, first.themes);
            assert!(second.carried_over.is_empty());
        }
        assert_eq!(
            load(&stored(&[(PALETTE_KEY, "paper")])).appearance.theme,
            ThemeChoice::Paper
        );
        assert_eq!(
            load(&stored(&[(PALETTE_KEY, "graphite")])).appearance.theme,
            ThemeChoice::Graphite
        );
    }

    #[test]
    fn an_old_glass_choice_becomes_the_glass_preset_without_a_saved_theme() {
        let start = stored(&[(PALETTE_KEY, "glass")]);
        let first = load(&start);
        assert_eq!(first.appearance.theme, ThemeChoice::Glass);
        assert_eq!(first.themes, SavedThemes::default());
        assert_eq!(first.carried_over, [theme_row(ThemeChoice::Glass)]);
        let second = load(&saved(&start, first.carried_over.clone()));
        assert_eq!(second.appearance, first.appearance);
        assert!(second.carried_over.is_empty());
    }

    #[test]
    fn a_saved_theme_named_glass_stays_the_owners_own() {
        // The 2026-10-15 migration saved an old Glass choice as a theme. It is kept, and stays
        // chosen beside the preset, because the owner may have changed it since.
        let mut themes = SavedThemes::default();
        let edited = ThemeSpec {
            transparency: 80,
            ..ThemeSpec::glass()
        };
        let rows = vec![
            themes.set(0, edited.clone()),
            theme_row(ThemeChoice::Custom(0)),
            (PALETTE_KEY.to_owned(), "glass".to_owned()),
        ];
        let loaded = load(&saved(&BTreeMap::new(), rows));
        assert_eq!(loaded.appearance.theme, ThemeChoice::Custom(0));
        assert_eq!(loaded.themes.get(0), Some(&edited));
        assert!(loaded.carried_over.is_empty());
    }

    #[test]
    fn the_picker_lists_graphite_paper_and_glass() {
        let names: Vec<_> = PRESETS.iter().map(|(_, name, _)| *name).collect();
        assert_eq!(names, ["Graphite", "Paper", "Glass"]);
        let choices: Vec<_> = PRESETS.iter().map(|(choice, _, _)| *choice).collect();
        assert_eq!(
            choices,
            [
                ThemeChoice::Graphite,
                ThemeChoice::Paper,
                ThemeChoice::Glass
            ]
        );
    }

    #[test]
    fn choosing_the_glass_preset_draws_the_canvas_glass_with_no_editing() {
        let themes = SavedThemes::default();
        let glass = Appearance {
            theme: ThemeChoice::Glass,
            ..Appearance::default()
        };
        let drawn = glass.drawn_palette(&themes);
        assert_eq!(drawn.id, PaletteId::Glass);
        assert_eq!(drawn, ThemeSpec::glass().palette(Accent::default(), false));
        let canvas = palette(PaletteId::Glass, Accent::default());
        assert_eq!(drawn.shadows, canvas.shadows);
        assert_eq!(drawn.card, canvas.card);
        assert_eq!(drawn.pill, canvas.pill);
        assert_eq!(glass.material(&themes), Some(Material::Acrylic));
        let reduced = Appearance {
            reduce_transparency: true,
            ..glass
        };
        assert_eq!(reduced.material(&themes), None);
        assert_eq!(reduced.drawn_palette(&themes).id, PaletteId::Graphite);
    }

    #[test]
    fn a_removed_theme_frees_its_slot_and_its_name() {
        let mut themes = SavedThemes::default();
        let start = BTreeMap::new();
        let row = themes.set(0, ThemeSpec::new("My theme"));
        assert_eq!(themes.unused_name("My theme"), "My theme 2");
        let removed = themes.remove(0);
        let loaded = load(&saved(&start, vec![row, removed]));
        assert_eq!(loaded.themes, SavedThemes::default());
        assert_eq!(themes.first_free(), Some(0));
        assert_eq!(themes.unused_name("My theme"), "My theme");
    }

    #[test]
    fn a_saved_former_default_accent_loads_as_the_new_default() {
        // #FF8A65 was the default and a preset until 2026-10-13; the owner decided it follows.
        for spelling in ["#FF8A65", "#ff8a65"] {
            let loaded = load(&stored(&[(ACCENT_KEY, spelling)]));
            assert_eq!(loaded.unreadable, Unreadable::default());
            assert_eq!(loaded.appearance.accent, Accent::default());
            assert_eq!(loaded.appearance.accent.fill(), Color::hex(0xFF612E));
            assert!(loaded.appearance.uses_preset_accent());
        }
    }

    #[test]
    fn a_saved_custom_accent_is_unchanged_by_the_default_migration() {
        for (stored_text, color) in [("#FF8A66", 0xFF8A66), ("#12AB9C", 0x12AB9C)] {
            let loaded = load(&stored(&[(ACCENT_KEY, stored_text)]));
            assert_eq!(loaded.appearance.accent.fill(), Color::hex(color));
            assert!(!loaded.appearance.uses_preset_accent());
        }
    }

    #[test]
    fn reduce_transparency_turns_a_material_off() {
        let mut themes = SavedThemes::default();
        let glass =
            ThemeSpec::from_retired_palette(PaletteId::Glass, Accent::default()).expect("glass");
        themes.set(0, glass);
        let appearance = Appearance {
            theme: ThemeChoice::Custom(0),
            ..Appearance::default()
        };
        assert_eq!(appearance.material(&themes), Some(Material::Acrylic));
        let reduced = Appearance {
            reduce_transparency: true,
            ..appearance
        };
        assert_eq!(reduced.material(&themes), None);
        assert_eq!(Appearance::default().material(&themes), None);
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
