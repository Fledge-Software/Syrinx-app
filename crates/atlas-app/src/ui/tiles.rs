//! Draws Quick access and Favorites tiles the same way and decides what opening one does.

use atlas_core::{BuiltInPlugin, Destination, DestinationTarget};
use atlas_ui::{Palette, TileColors};
use gpui::{Div, ParentElement, Styled, div, px};

use crate::session::SessionCommand;
use crate::theme::rgba;

/// What a click on a tile asks for.
pub(super) enum TileAction {
    Session(SessionCommand),
    /// Plugin pages wait for plan 2's plugin lifecycle; the tile says so instead of opening.
    Unavailable(BuiltInPlugin),
}

/// Opens a web tile in the Home tab when Home is showing, otherwise in a new tab.
pub(super) fn open_action(tile: &Destination, home_showing: bool) -> TileAction {
    match &tile.target {
        DestinationTarget::Plugin(plugin) => TileAction::Unavailable(*plugin),
        DestinationTarget::Web(address) if home_showing => {
            TileAction::Session(SessionCommand::NavigateActive {
                address: address.clone(),
            })
        }
        DestinationTarget::Web(address) => TileAction::Session(SessionCommand::NewTab {
            address: address.clone(),
        }),
    }
}

/// The footer notice for a plugin tile that cannot open yet.
pub(super) fn unavailable_message(plugin: BuiltInPlugin) -> String {
    format!(
        "{} is not available yet. Plugins arrive with the plugin runtime.",
        plugin.name()
    )
}

/// The tile's monogram: the label's first letter or digit, uppercased.
pub(super) fn monogram(label: &str) -> String {
    label
        .chars()
        .find(|character| character.is_alphanumeric())
        .map_or_else(|| "·".to_owned(), |first| first.to_uppercase().collect())
}

/// A site keeps one tile colour everywhere, so the colour follows its host, not its position.
pub(super) fn tile_colors(palette: &Palette, target: &DestinationTarget) -> TileColors {
    match target {
        DestinationTarget::Plugin(BuiltInPlugin::Forge) => palette.plugins.forge,
        DestinationTarget::Plugin(BuiltInPlugin::Notes) => palette.plugins.notes,
        DestinationTarget::Plugin(BuiltInPlugin::Fledge) => palette.plugins.fledge,
        DestinationTarget::Web(address) => {
            palette.tiles[site_hash(host(address)) as usize % palette.tiles.len()]
        }
    }
}

/// The square monogram face of a tile, at `size` with corner `radius`.
pub(super) fn tile_face(
    palette: &Palette,
    tile: &Destination,
    size: f32,
    radius: f32,
    letter_size: f32,
) -> Div {
    let colors = tile_colors(palette, &tile.target);
    let mut face = div()
        .relative()
        .size(px(size))
        .flex_none()
        .rounded(px(radius))
        .bg(rgba(colors.background))
        .text_color(rgba(colors.foreground))
        .text_size(px(letter_size))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .flex()
        .items_center()
        .justify_center()
        .child(monogram(&tile.label));
    // Plugin tiles are dimmed until their plugin exists, like a closed plugin row.
    if matches!(tile.target, DestinationTarget::Plugin(_)) {
        face = face.opacity(0.55);
    }
    face
}

/// The dashed "add" square used by both surfaces.
pub(super) fn add_face(palette: &Palette, size: f32, radius: f32) -> Div {
    div()
        .size(px(size))
        .flex_none()
        .rounded(px(radius))
        .border(px(1.5))
        .border_dashed()
        .border_color(rgba(palette.dash_secondary))
        .text_color(rgba(palette.text_muted))
        .text_size(px(20.))
        .flex()
        .items_center()
        .justify_center()
        .child("+")
}

fn host(address: &str) -> &str {
    // Addresses are normalised `http(s)://host/...`; anything else hashes whole.
    let rest = address.split_once("://").map_or(address, |(_, rest)| rest);
    rest.split(['/', '?', '#']).next().unwrap_or(rest)
}

fn site_hash(host: &str) -> u32 {
    // FNV-1a keeps the colour stable across runs and builds.
    host.bytes().fold(0x811c_9dc5, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

#[cfg(test)]
mod tests {
    use atlas_ui::{Accent, PaletteId, palette};

    use super::*;

    fn web(label: &str, address: &str) -> Destination {
        Destination::new(label, DestinationTarget::Web(address.to_owned())).expect("synthetic tile")
    }

    #[test]
    fn a_site_keeps_its_colour_on_every_path() {
        let palette = palette(PaletteId::Graphite, Accent::default());
        let first = web("One", "https://same.example.test/mail");
        let second = web("Two", "https://same.example.test/calendar?view=week");
        assert_eq!(
            tile_colors(&palette, &first.target),
            tile_colors(&palette, &second.target)
        );
        assert_eq!(
            tile_colors(&palette, &DestinationTarget::Plugin(BuiltInPlugin::Notes)),
            palette.plugins.notes
        );
    }

    #[test]
    fn monograms_use_the_first_letter_or_digit() {
        assert_eq!(monogram("synthetic"), "S");
        assert_eq!(monogram("  ☆ 9 lives"), "9");
        assert_eq!(monogram("éclair"), "É");
        assert_eq!(monogram("☆"), "·");
    }

    #[test]
    fn web_tiles_reuse_home_and_plugin_tiles_stay_unavailable() {
        let tile = web("Synthetic", "https://open.example.test/");
        assert!(matches!(
            open_action(&tile, true),
            TileAction::Session(SessionCommand::NavigateActive { address })
                if address == "https://open.example.test/"
        ));
        assert!(matches!(
            open_action(&tile, false),
            TileAction::Session(SessionCommand::NewTab { address })
                if address == "https://open.example.test/"
        ));
        let plugin = Destination::new(
            "Synthetic forge",
            DestinationTarget::Plugin(BuiltInPlugin::Forge),
        )
        .expect("plugin tile");
        assert!(matches!(
            open_action(&plugin, true),
            TileAction::Unavailable(BuiltInPlugin::Forge)
        ));
    }
}
