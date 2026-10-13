//! Audits each palette's foreground/background pairs against WCAG 2.x contrast minimums.

use crate::accent::{ACCENT_PRESETS, Accent};
use crate::color::{Color, contrast_ratio};
use crate::palette::{Palette, PaletteId, palette};

/// WCAG 1.4.3: normal-size text needs 4.5:1. `design/README.md` holds every palette to it.
pub const TEXT_MINIMUM: f64 = 4.5;

/// WCAG 1.4.11: icons and selection marks need 3:1 against what is next to them.
pub const NON_TEXT_MINIMUM: f64 = 3.0;

/// One measured pair. Translucent layers are measured over every colour the ground presents
/// and the worst ratio is kept.
#[derive(Clone, Debug, PartialEq)]
pub struct ContrastCheck {
    pub palette: PaletteId,
    pub accent: Color,
    pub foreground: &'static str,
    pub background: &'static str,
    pub ratio: f64,
    pub minimum: f64,
}

impl ContrastCheck {
    pub fn passes(&self) -> bool {
        self.ratio >= self.minimum
    }

    /// A stable label for reports and the recorded-failure list:
    /// `palette/accent/foreground/background`, the accent as six lowercase hex digits.
    pub fn label(&self) -> String {
        format!(
            "{}/{:06x}/{}/{}",
            self.palette.id(),
            self.accent.to_rgba_u32() >> 8,
            self.foreground,
            self.background
        )
    }
}

/// Measures every audited pair in one palette.
pub fn audit(palette: &Palette) -> Vec<ContrastCheck> {
    let backgrounds = text_backgrounds(palette);
    let mut checks = Vec::new();

    // Text roles over every surface text is drawn on.
    for (foreground_name, foreground) in text_roles(palette) {
        for (background_name, background) in &backgrounds {
            checks.push(measure(
                palette,
                foreground_name,
                foreground,
                background_name,
                background,
                TEXT_MINIMUM,
            ));
        }
    }

    // Icons beside text on the window ground and in popups.
    for (icon_name, icon) in [("icon", palette.icon), ("icon_dim", palette.icon_dim)] {
        for (background_name, background) in
            backgrounds.iter().filter(|(name, _)| is_icon_surface(name))
        {
            checks.push(measure(
                palette,
                icon_name,
                icon,
                background_name,
                background,
                NON_TEXT_MINIMUM,
            ));
        }
    }

    checks.extend(tile_checks(palette));
    checks.extend(accent_checks(palette, &backgrounds));
    checks
}

/// Audits all five palettes with each Customize accent preset.
pub fn audit_all() -> Vec<ContrastCheck> {
    let mut checks = Vec::new();
    for accent in ACCENT_PRESETS {
        for id in PaletteId::ALL {
            checks.extend(audit(&palette(id, Accent::new(accent))));
        }
    }
    checks
}

fn text_roles(palette: &Palette) -> [(&'static str, Color); 5] {
    [
        ("text", palette.text),
        ("text_strong", palette.text_strong),
        ("text_secondary", palette.text_secondary),
        ("text_secondary_bright", palette.text_secondary_bright),
        ("text_muted", palette.text_muted),
    ]
}

fn text_backgrounds(palette: &Palette) -> Vec<(&'static str, Vec<Color>)> {
    // Each surface is resolved to the opaque colours it can actually present.
    let grounds = palette.ground.presented_colors();
    let over_ground = |layer: Color| {
        grounds
            .iter()
            .map(|ground| layer.over(*ground))
            .collect::<Vec<_>>()
    };
    let panels = over_ground(palette.panel);
    vec![
        ("ground", grounds.clone()),
        ("row_selected", over_ground(palette.row_selected)),
        ("field", over_ground(palette.field)),
        ("card", over_ground(palette.card)),
        ("surface", over_ground(palette.surface.over(palette.card))),
        ("pill", over_ground(palette.pill.over(palette.card))),
        ("panel", panels.clone()),
        (
            "panel_side",
            panels
                .iter()
                .map(|panel| palette.panel_side.over(*panel))
                .collect(),
        ),
        (
            "input",
            panels
                .iter()
                .map(|panel| palette.input.over(*panel))
                .collect(),
        ),
    ]
}

fn is_icon_surface(name: &str) -> bool {
    matches!(name, "ground" | "card" | "panel")
}

fn tile_checks(palette: &Palette) -> Vec<ContrastCheck> {
    // Monogram letters are text on their tile, which may itself be translucent (Glass).
    let grounds = palette.ground.presented_colors();
    let plugins = [
        ("plugin_forge", palette.plugins.forge),
        ("plugin_notes", palette.plugins.notes),
        ("plugin_fledge", palette.plugins.fledge),
        ("plugin_neutral", palette.plugins.neutral),
    ];
    const TILE_NAMES: [&str; 8] = [
        "tile_0", "tile_1", "tile_2", "tile_3", "tile_4", "tile_5", "tile_6", "tile_7",
    ];
    TILE_NAMES
        .into_iter()
        .zip(palette.tiles)
        .chain(plugins)
        .map(|(name, tile)| {
            let background: Vec<Color> = grounds
                .iter()
                .map(|ground| tile.background.over(*ground))
                .collect();
            measure(
                palette,
                name,
                tile.foreground,
                "own_tile",
                &background,
                TEXT_MINIMUM,
            )
        })
        .collect()
}

/// Whether the accent ink meets 4.5:1 as text and 3:1 as a mark on this palette's surfaces.
pub fn accent_ink_passes(palette: &Palette) -> bool {
    let backgrounds = text_backgrounds(palette);
    accent_checks(palette, &backgrounds)
        .iter()
        .filter(|check| check.foreground != "accent_on_fill")
        .all(ContrastCheck::passes)
}

fn accent_checks(
    palette: &Palette,
    backgrounds: &[(&'static str, Vec<Color>)],
) -> Vec<ContrastCheck> {
    // Badge numbers are text on the accent; the accent ink marks selection on surfaces and
    // also draws matched characters as text in palette rows (canvas `Palette.dc.html`).
    let accent = palette.accent;
    let mut checks = vec![measure(
        palette,
        "accent_on_fill",
        accent.on_fill,
        "accent_fill",
        &[accent.fill],
        TEXT_MINIMUM,
    )];
    for (background_name, background) in
        backgrounds.iter().filter(|(name, _)| is_icon_surface(name))
    {
        checks.push(measure(
            palette,
            "accent_ink",
            accent.ink,
            background_name,
            background,
            NON_TEXT_MINIMUM,
        ));
    }
    for (background_name, background) in ink_text_backgrounds(palette, backgrounds) {
        checks.push(measure(
            palette,
            "accent_ink_text",
            accent.ink,
            background_name,
            &background,
            TEXT_MINIMUM,
        ));
    }
    checks
}

fn ink_text_backgrounds(
    palette: &Palette,
    backgrounds: &[(&'static str, Vec<Color>)],
) -> Vec<(&'static str, Vec<Color>)> {
    // Palette rows: plain panel, and the selected row's accent wash over the panel.
    let panels = backgrounds
        .iter()
        .find(|(name, _)| *name == "panel")
        .map(|(_, colors)| colors.clone())
        .unwrap_or_default();
    let selected = panels
        .iter()
        .map(|panel| palette.accent.soft.over(*panel))
        .collect();
    vec![("panel", panels), ("panel_row_selected", selected)]
}

fn measure(
    palette: &Palette,
    foreground_name: &'static str,
    foreground: Color,
    background_name: &'static str,
    backgrounds: &[Color],
    minimum: f64,
) -> ContrastCheck {
    // Translucent text is blended onto each background before measuring.
    let ratio = backgrounds
        .iter()
        .map(|background| contrast_ratio(foreground.over(*background), *background))
        .fold(f64::INFINITY, f64::min);
    ContrastCheck {
        palette: palette.id,
        accent: palette.accent.fill,
        foreground: foreground_name,
        background: background_name,
        ratio,
        minimum,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    /// Pairs below their WCAG minimum in the canvas values, recorded for the owner's review in
    /// `docs/p1-07-design-tokens.md` rather than silently retuned. Any change here is a design change.
    const RECORDED_FAILURES: &[&str] = &[
        "glass/6ea8ff/text_muted/card",
        "glass/6ea8ff/text_muted/field",
        "glass/6ea8ff/text_muted/ground",
        "glass/6ea8ff/text_muted/pill",
        "glass/6ea8ff/text_muted/row_selected",
        "glass/6ea8ff/text_muted/surface",
        "glass/6ea8ff/text_secondary/pill",
        "glass/6ea8ff/text_secondary/row_selected",
        "glass/6ea8ff/text_secondary_bright/row_selected",
        "glass/6fd3a8/text_muted/card",
        "glass/6fd3a8/text_muted/field",
        "glass/6fd3a8/text_muted/ground",
        "glass/6fd3a8/text_muted/pill",
        "glass/6fd3a8/text_muted/row_selected",
        "glass/6fd3a8/text_muted/surface",
        "glass/6fd3a8/text_secondary/pill",
        "glass/6fd3a8/text_secondary/row_selected",
        "glass/6fd3a8/text_secondary_bright/row_selected",
        "glass/e8c16a/text_muted/card",
        "glass/e8c16a/text_muted/field",
        "glass/e8c16a/text_muted/ground",
        "glass/e8c16a/text_muted/pill",
        "glass/e8c16a/text_muted/row_selected",
        "glass/e8c16a/text_muted/surface",
        "glass/e8c16a/text_secondary/pill",
        "glass/e8c16a/text_secondary/row_selected",
        "glass/e8c16a/text_secondary_bright/row_selected",
        "glass/ff612e/text_muted/card",
        "glass/ff612e/text_muted/field",
        "glass/ff612e/text_muted/ground",
        "glass/ff612e/text_muted/pill",
        "glass/ff612e/text_muted/row_selected",
        "glass/ff612e/text_muted/surface",
        "glass/ff612e/text_secondary/pill",
        "glass/ff612e/text_secondary/row_selected",
        "glass/ff612e/text_secondary_bright/row_selected",
        "graphite/6ea8ff/text_muted/row_selected",
        "graphite/6fd3a8/text_muted/row_selected",
        "graphite/e8c16a/text_muted/row_selected",
        "graphite/ff612e/text_muted/row_selected",
        "haze/6ea8ff/text_muted/field",
        "haze/6ea8ff/text_muted/row_selected",
        "haze/6fd3a8/text_muted/field",
        "haze/6fd3a8/text_muted/row_selected",
        "haze/e8c16a/text_muted/field",
        "haze/e8c16a/text_muted/row_selected",
        "haze/ff612e/text_muted/field",
        "haze/ff612e/text_muted/row_selected",
        "paper/6fd3a8/accent_ink_text/panel",
        "paper/6fd3a8/accent_ink_text/panel_row_selected",
        "paper/e8c16a/accent_ink_text/panel",
        "paper/e8c16a/accent_ink_text/panel_row_selected",
        "tint/6fd3a8/text_muted/row_selected",
        "tint/e8c16a/text_muted/row_selected",
    ];

    #[test]
    fn every_palette_is_audited_with_every_accent() {
        let checks = audit_all();
        for id in PaletteId::ALL {
            for accent in ACCENT_PRESETS {
                assert!(
                    checks
                        .iter()
                        .any(|check| check.palette == id && check.accent == accent)
                );
            }
        }
    }

    #[test]
    fn contrast_failures_match_the_recorded_owner_review_list() {
        let failures: BTreeSet<String> = audit_all()
            .iter()
            .filter(|check| !check.passes())
            .map(ContrastCheck::label)
            .collect();
        let recorded: BTreeSet<String> = RECORDED_FAILURES
            .iter()
            .map(|label| (*label).to_owned())
            .collect();
        let report: Vec<String> = audit_all()
            .iter()
            .filter(|check| !check.passes())
            .map(|check| format!("{}: {:.2} < {}", check.label(), check.ratio, check.minimum))
            .collect();
        assert_eq!(
            failures,
            recorded,
            "contrast report:\n{}",
            report.join("\n")
        );
    }

    #[test]
    fn translucent_glass_tiles_are_measured_over_every_wallpaper() {
        let glass = palette(PaletteId::Glass, Accent::default());
        let tile = glass.tiles[0];
        let worst = glass
            .ground
            .presented_colors()
            .iter()
            .map(|ground| {
                let background = tile.background.over(*ground);
                contrast_ratio(tile.foreground, background)
            })
            .fold(f64::INFINITY, f64::min);
        let check = audit(&glass)
            .into_iter()
            .find(|check| check.foreground == "tile_0")
            .expect("tile 0 is audited");
        assert_eq!(check.ratio, worst);
    }

    #[test]
    fn labels_tell_accents_apart() {
        let default = audit(&palette(PaletteId::Paper, Accent::default()));
        let green = audit(&palette(
            PaletteId::Paper,
            Accent::new(Color::hex(0x6FD3A8)),
        ));
        assert_eq!(default[0].label(), "paper/ff612e/text/ground");
        assert_ne!(default[0].label(), green[0].label());
    }

    #[test]
    fn default_accent_text_clears_the_text_minimum_in_every_palette() {
        // 2026-10-13: the logo orange must stay at 4.5:1 as text and under badge text.
        for id in PaletteId::ALL {
            let text_checks: Vec<_> = audit(&palette(id, Accent::default()))
                .into_iter()
                .filter(|check| check.foreground.starts_with("accent"))
                .filter(|check| check.minimum == TEXT_MINIMUM)
                .collect();
            assert_eq!(text_checks.len(), 3, "{}", id.id());
            for check in text_checks {
                assert!(check.passes(), "{}: {:.2}", check.label(), check.ratio);
            }
        }
    }

    #[test]
    fn graphite_body_text_clears_the_text_minimum() {
        let graphite = palette(PaletteId::Graphite, Accent::default());
        let ground = graphite.ground.representative();
        assert!(contrast_ratio(graphite.text, ground) >= TEXT_MINIMUM);
    }
}
