//! The small form that adds or changes a Quick access tile or a Favorite.

use atlas_core::{
    BuiltInPlugin, Destination, DestinationEdit, DestinationSurface, DestinationTarget,
};
use atlas_ui::Palette;
use atlas_ui::metrics::{radius, text};
use gpui::{
    AppContext, Context, Div, Entity, FontWeight, InteractiveElement, ParentElement, Stateful,
    StatefulInteractiveElement, Styled, Subscription, Window, div, px,
};
use gpui_component::input::{Input, InputEvent, InputState};

use super::BrowserView;
use crate::navigation;
use crate::theme::rgba;

/// What the form's tile will point at.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TargetChoice {
    Website,
    Plugin(BuiltInPlugin),
}

/// An open add/change form for one surface.
pub(super) struct TileEditor {
    pub(super) surface: DestinationSurface,
    /// `None` adds a tile; `Some` replaces the tile at that position.
    index: Option<usize>,
    label: Entity<InputState>,
    address: Entity<InputState>,
    target: TargetChoice,
    error: Option<String>,
    _subscriptions: [Subscription; 2],
}

impl BrowserView {
    /// Opens the form, filled from the tile being changed if there is one.
    pub(super) fn start_tile_editor(
        &mut self,
        surface: DestinationSurface,
        index: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let existing = index.and_then(|index| self.home.destinations.list(surface).get(index));
        let (label_text, address_text, target) = match existing {
            Some(tile) => match &tile.target {
                DestinationTarget::Web(address) => {
                    (tile.label.clone(), address.clone(), TargetChoice::Website)
                }
                DestinationTarget::Plugin(plugin) => (
                    tile.label.clone(),
                    String::new(),
                    TargetChoice::Plugin(*plugin),
                ),
            },
            None => (String::new(), String::new(), TargetChoice::Website),
        };
        let label = cx.new(|cx| InputState::new(window, cx).placeholder("Name"));
        let address = cx.new(|cx| InputState::new(window, cx).placeholder("Website address"));
        label.update(cx, |input, cx| {
            input.set_value(label_text, window, cx);
            input.focus(window, cx);
        });
        address.update(cx, |input, cx| input.set_value(address_text, window, cx));
        let submit_on_enter = |this: &mut Self,
                               _: &Entity<InputState>,
                               event: &InputEvent,
                               window: &mut Window,
                               cx: &mut Context<Self>| {
            if let InputEvent::PressEnter { .. } = event {
                this.submit_tile_editor(window, cx);
            }
        };
        let subscriptions = [
            cx.subscribe_in(&label, window, submit_on_enter),
            cx.subscribe_in(&address, window, submit_on_enter),
        ];
        self.home.editor = Some(TileEditor {
            surface,
            index,
            label,
            address,
            target,
            error: None,
            _subscriptions: subscriptions,
        });
        cx.notify();
    }

    fn submit_tile_editor(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = &mut self.home.editor else {
            return;
        };
        let label = editor.label.read(cx).value().to_string();
        let address = editor.address.read(cx).value().to_string();
        let tile = match tile_from_form(&label, &address, editor.target) {
            Ok(tile) => tile,
            Err(message) => {
                editor.error = Some(message);
                cx.notify();
                return;
            }
        };
        let surface = editor.surface;
        let edit = match editor.index {
            Some(index) => DestinationEdit::Replace { index, tile },
            None => DestinationEdit::Add(tile),
        };
        self.home.editor = None;
        self.edit_tiles(surface, edit);
        cx.notify();
    }

    /// The form itself, drawn under the surface it edits.
    pub(super) fn tile_editor(
        &self,
        editor: &TileEditor,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut choices = div().flex().flex_wrap().gap(px(6.));
        let options = [TargetChoice::Website]
            .into_iter()
            .chain(BuiltInPlugin::ALL.map(TargetChoice::Plugin));
        for (position, choice) in options.enumerate() {
            let chip = target_chip(position, choice, editor.target == choice, palette);
            choices = choices.child(chip.on_click(cx.listener(move |this, _, _, cx| {
                if let Some(editor) = &mut this.home.editor {
                    editor.target = choice;
                    editor.error = None;
                }
                cx.notify();
            })));
        }
        let title = if editor.index.is_some() {
            "Change tile"
        } else {
            "Add tile"
        };
        let mut form = div()
            .w_full()
            .max_w(px(420.))
            .p(px(12.))
            .rounded(px(radius::TILE))
            .bg(rgba(palette.surface))
            .border_1()
            .border_color(rgba(palette.surface_line))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgba(palette.text))
                    .child(title),
            )
            .child(Input::new(&editor.label))
            .child(choices);
        if editor.target == TargetChoice::Website {
            form = form.child(Input::new(&editor.address));
        } else {
            form = form.child(
                div()
                    .text_size(px(text::SMALL))
                    .text_color(rgba(palette.text_muted))
                    .child("Plugin tiles stay dimmed until the plugin runtime exists."),
            );
        }
        if let Some(error) = &editor.error {
            form = form.child(
                div()
                    .text_size(px(text::SMALL))
                    .text_color(rgba(palette.text_secondary))
                    .child(error.clone()),
            );
        }
        form.child(
            div()
                .flex()
                .gap(px(8.))
                .child(form_button("tile-save", "Save", palette, true).on_click(
                    cx.listener(|this, _, window, cx| this.submit_tile_editor(window, cx)),
                ))
                .child(
                    form_button("tile-cancel", "Cancel", palette, false).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.home.editor = None;
                            cx.notify();
                        },
                    )),
                ),
        )
    }
}

/// Builds the tile the form describes, or explains what to fix.
pub(super) fn tile_from_form(
    label: &str,
    address: &str,
    choice: TargetChoice,
) -> Result<Destination, String> {
    let target = match choice {
        TargetChoice::Plugin(plugin) => DestinationTarget::Plugin(plugin),
        TargetChoice::Website => DestinationTarget::Web(web_address(address)?),
    };
    if label.trim().is_empty() {
        return Err("Give the tile a name.".to_owned());
    }
    Destination::new(label, target).map_err(|error| format!("Cannot save this tile: {error}."))
}

fn web_address(text: &str) -> Result<String, String> {
    // Tiles use navigation's address rule, but never fall back to a web search.
    match navigation::recognized_address(text) {
        Ok(Some(address)) if address.starts_with("http") => Ok(address),
        Ok(_) => Err("Enter a website address, such as example.com.".to_owned()),
        Err(message) => Err(message.to_owned()),
    }
}

fn target_chip(
    position: usize,
    choice: TargetChoice,
    selected: bool,
    palette: &Palette,
) -> Stateful<Div> {
    let label = match choice {
        TargetChoice::Website => "Website",
        TargetChoice::Plugin(plugin) => plugin.name(),
    };
    let (background, outline) = if selected {
        (palette.chip_selected, palette.accent.soft_line)
    } else {
        (palette.field, palette.chip_line)
    };
    div()
        .id(("tile-target", position))
        .h(px(26.))
        .px(px(10.))
        .rounded(px(13.))
        .border_1()
        .border_color(rgba(outline))
        .bg(rgba(background))
        .text_size(px(text::SMALL))
        .text_color(rgba(palette.text_secondary))
        .flex()
        .items_center()
        .cursor_pointer()
        .child(label)
}

fn form_button(
    id: &'static str,
    label: &'static str,
    palette: &Palette,
    primary: bool,
) -> Stateful<Div> {
    let (background, foreground) = if primary {
        (palette.accent.fill, palette.accent.on_fill)
    } else {
        (palette.field, palette.text_secondary)
    };
    div()
        .id(id)
        .h(px(28.))
        .px(px(12.))
        .rounded(px(radius::CONTROL))
        .bg(rgba(background))
        .text_color(rgba(foreground))
        .text_size(px(text::ADDRESS))
        .flex()
        .items_center()
        .cursor_pointer()
        .child(label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn website_tiles_use_navigation_addresses_without_search_fallback() {
        let tile = tile_from_form("Synthetic", "example.test/path", TargetChoice::Website)
            .expect("bare host becomes an address");
        assert!(matches!(
            &tile.target,
            DestinationTarget::Web(address) if address == "https://example.test/path"
        ));
        for address in ["two words", "about:blank", "javascript:void(0)", ""] {
            assert!(
                tile_from_form("Synthetic", address, TargetChoice::Website).is_err(),
                "{address}"
            );
        }
    }

    #[test]
    fn plugin_tiles_ignore_the_address_and_names_are_required() {
        let tile = tile_from_form(
            "Synthetic notes",
            "ignored",
            TargetChoice::Plugin(BuiltInPlugin::Notes),
        )
        .expect("plugin tile");
        assert!(matches!(
            tile.target,
            DestinationTarget::Plugin(BuiltInPlugin::Notes)
        ));
        assert_eq!(
            tile_from_form("  ", "example.test", TargetChoice::Website).map(|tile| tile.label),
            Err("Give the tile a name.".to_owned())
        );
    }
}
