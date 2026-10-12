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

    /// A stable label for reports and the recorded-failure list: `palette/foreground/background`.
    pub fn label(&self) -> String {
        format!(
            "{}/{}/{}",
            self.palette.id(),
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
    // Monogram letters are text on their tile, which may itself be translucent.
    let ground = palette.ground.representative();
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
            let background = vec![tile.background.over(ground)];
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

fn accent_checks(
    palette: &Palette,
    backgrounds: &[(&'static str, Vec<Color>)],
) -> Vec<ContrastCheck> {
    // Badge numbers are text on the accent; the accent ink marks selection on surfaces.
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
    checks
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
        "glass/accent_ink/card",
        "glass/text_muted/card",
        "glass/text_muted/field",
        "glass/text_muted/ground",
        "glass/text_muted/pill",
        "glass/text_muted/row_selected",
        "glass/text_muted/surface",
        "glass/text_secondary/pill",
        "glass/text_secondary/row_selected",
        "glass/text_secondary_bright/row_selected",
        "graphite/text_muted/row_selected",
        "haze/text_muted/field",
        "haze/text_muted/row_selected",
        "tint/text_muted/row_selected",
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
            .map(|check| {
                format!(
                    "{} with accent {:08X}: {:.2} < {}",
                    check.label(),
                    check.accent.to_rgba_u32(),
                    check.ratio,
                    check.minimum
                )
            })
            .collect();
        assert_eq!(
            failures,
            recorded,
            "contrast report:\n{}",
            report.join("\n")
        );
    }

    #[test]
    fn graphite_body_text_clears_the_text_minimum() {
        let graphite = palette(PaletteId::Graphite, Accent::default());
        let ground = graphite.ground.representative();
        assert!(contrast_ratio(graphite.text, ground) >= TEXT_MINIMUM);
    }
}
