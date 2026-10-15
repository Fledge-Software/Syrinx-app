//! Gives each of Syrinx's text fields a node that screen readers hear as focused (DW-60, DW-70).

use gpui::{
    App, Div, Entity, FocusHandle, Focusable, InteractiveElement, ParentElement, Role,
    SharedString, Stateful, StatefulInteractiveElement, Window, div, prelude::FluentBuilder,
};
use gpui_component::input::{Input, InputState};

/// A text field's accessible identity: its container's element id, role and spoken name.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FieldName {
    pub(crate) id: &'static str,
    pub(crate) role: Role,
    pub(crate) label: &'static str,
}

/// The palette's query, an editable combo box because its rows sit inside it (DW-60).
pub(crate) const PALETTE_QUERY: FieldName = FieldName {
    id: "palette-query",
    role: Role::EditableComboBox,
    label: "Search or command",
};

pub(crate) const FIND_IN_PAGE: FieldName = plain("find-query", "Find in page");
pub(crate) const FOLDER_NAME: FieldName = plain("folder-name-field", "Folder name");
pub(crate) const TILE_NAME: FieldName = plain("tile-name-field", "Tile name");
pub(crate) const TILE_ADDRESS: FieldName = plain("tile-address-field", "Tile address");
pub(crate) const CUSTOM_ACCENT: FieldName = plain("custom-accent-field", "Custom accent colour");

const fn plain(id: &'static str, label: &'static str) -> FieldName {
    FieldName {
        id,
        role: Role::TextInput,
        label,
    }
}

/// A container that tracks `input`'s own focus handle, so it is the focused node while the
/// owner types. Keys, IME text and the caret still go to the field, which registers the
/// handle after its ancestor; the container adds no Tab stop.
pub(crate) fn container(
    field: FieldName,
    input: &Entity<InputState>,
    window: &Window,
    cx: &App,
) -> Stateful<Div> {
    let state = input.read(cx);
    let handle = state.focus_handle(cx);
    let value = spoken_value(window.is_a11y_active(), || state.value());
    let container = div()
        .id(field.id)
        .role(field.role)
        .aria_label(field.label)
        .when_some(value, |container, value| container.aria_value(value))
        // A second Tab stop for the field's handle would trap Shift Tab on the field.
        // `track_focus` keeps a clone, which reads the flag shared by every clone.
        .track_focus(&handle.clone().tab_stop(false));
    // Restore the shared flag before the field draws, or the field would leave Tab order.
    restore_tab_stop(handle);
    container
}

/// A field that stands alone: its container holding only the field itself.
pub(crate) fn labelled(
    field: FieldName,
    input: &Entity<InputState>,
    window: &Window,
    cx: &App,
) -> Stateful<Div> {
    container(field, input, window, cx).child(Input::new(input).aria_label(field.label))
}

/// The text a screen reader reads with the field, copied only while one is listening.
fn spoken_value(a11y_active: bool, value: impl FnOnce() -> SharedString) -> Option<SharedString> {
    a11y_active.then(value)
}

/// Makes the field's handle a Tab stop again for every clone made from now on.
fn restore_tab_stop(handle: FocusHandle) {
    drop(handle.tab_stop(true));
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    const ALL: [FieldName; 6] = [
        PALETTE_QUERY,
        FIND_IN_PAGE,
        FOLDER_NAME,
        TILE_NAME,
        TILE_ADDRESS,
        CUSTOM_ACCENT,
    ];

    #[test]
    fn every_field_has_its_own_id_and_a_spoken_name() {
        // Sibling nodes with one id panic GPUI's accessibility tree under debug assertions.
        let ids: HashSet<_> = ALL.iter().map(|field| field.id).collect();
        assert_eq!(ids.len(), ALL.len());
        assert!(ALL.iter().all(|field| !field.label.trim().is_empty()));
    }

    #[test]
    fn only_the_palette_query_is_a_combo_box() {
        assert_eq!(PALETTE_QUERY.id, "palette-query");
        assert_eq!(PALETTE_QUERY.role, Role::EditableComboBox);
        for field in &ALL[1..] {
            assert_eq!(field.role, Role::TextInput, "{}", field.id);
        }
    }

    #[test]
    fn the_value_is_copied_only_while_a_screen_reader_listens() {
        let read = || SharedString::from("#FF8A65");
        assert_eq!(spoken_value(true, read), Some("#FF8A65".into()));
        assert_eq!(
            spoken_value(false, || unreachable!("no copy without a client")),
            None
        );
    }
}
