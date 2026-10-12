//! Draws the folder naming form and the removal cards in the wide sidebar (P1-21).

use atlas_ui::{
    Palette,
    metrics::{radius, space, text},
};
use gpui::{
    Context, Div, FontWeight, InteractiveElement, ParentElement, Stateful,
    StatefulInteractiveElement, Styled, div, px,
};
use gpui_component::input::Input;

use super::{
    BrowserView,
    folder_editor::{
        BUTTON_CONTEXT, CARD_CONTEXT, CancelFolderCard, ConfirmFolderCard, DeleteFolderContents,
        FolderEditor, NameForm, NameTarget, PageCard, PressButton, RemoveCard, RemoveStep,
        contents_phrase,
    },
    tile_editor::form_button,
};
use crate::theme::{self, rgba};

impl BrowserView {
    /// The open form or card, drawn under the saved section's header in the wide sidebar.
    pub(super) fn folder_editor_view(&self, cx: &mut Context<Self>) -> Option<Div> {
        let editor = self.folder_editor.as_ref()?;
        let card = card_frame().on_action(cx.listener(|this, _: &CancelFolderCard, _, cx| {
            this.cancel_folder_card(cx);
        }));
        match editor {
            FolderEditor::Name(form) => Some(name_form(card, form, cx)),
            FolderEditor::Remove(removal) => self.removal_card(card, removal, cx),
            FolderEditor::RemovePage(page) => self.page_card(card, page, cx),
        }
    }

    /// The folder removal card, or nothing once its folder is gone (the next state closes it).
    fn removal_card(&self, card: Div, removal: &RemoveCard, cx: &mut Context<Self>) -> Option<Div> {
        let palette = theme::current();
        let title = &self
            .model
            .saved_folders
            .iter()
            .find(|folder| folder.id == removal.id)?
            .title;
        let held = contents_phrase(removal.shown);
        let card = card
            .track_focus(&removal.focus)
            .on_action(cx.listener(|this, _: &ConfirmFolderCard, _, cx| {
                this.confirm_folder_card(cx);
            }))
            .on_action(cx.listener(|this, _: &DeleteFolderContents, _, cx| {
                this.choose_delete_contents(cx);
            }));
        if removal.step == RemoveStep::ConfirmDelete {
            let delete = keyboard_button(form_button("folder-delete", "Delete", &palette, true))
                .on_action(cx.listener(|this, _: &PressButton, _, cx| {
                    this.delete_with_contents(cx);
                }))
                .on_click(cx.listener(|this, _, _, cx| this.delete_with_contents(cx)));
            return Some(
                card.child(card_heading(format!(
                    "Delete “{title}” and everything in it?"
                )))
                .child(note(
                    &palette,
                    format!(
                        "{held} will be deleted. Open pages stay open as ordinary tabs. \
                         Tab to Delete to confirm."
                    ),
                ))
                .child(
                    button_row()
                        .child(delete)
                        .child(cancel_button(&palette, cx)),
                ),
            );
        }
        let remove = keyboard_button(form_button("folder-remove", "Remove", &palette, true))
            .on_action(cx.listener(|this, _: &PressButton, _, cx| {
                this.remove_keeping_contents(cx);
            }))
            .on_click(cx.listener(|this, _, _, cx| this.remove_keeping_contents(cx)));
        let delete_contents = keyboard_button(form_button(
            "folder-delete-contents",
            "Delete contents…",
            &palette,
            false,
        ))
        .on_action(cx.listener(|this, _: &PressButton, _, cx| this.choose_delete_contents(cx)))
        .on_click(cx.listener(|this, _, _, cx| this.choose_delete_contents(cx)));
        Some(
            card.child(card_heading(format!("Remove “{title}”?")))
                .child(note(
                    &palette,
                    format!("Its {held} will move up one level."),
                ))
                .child(
                    button_row()
                        .flex_wrap()
                        .child(remove)
                        .child(delete_contents)
                        .child(cancel_button(&palette, cx)),
                ),
        )
    }

    /// Confirms removing an unopened saved page, or nothing once it is gone or opened.
    fn page_card(&self, card: Div, page: &PageCard, cx: &mut Context<Self>) -> Option<Div> {
        let palette = theme::current();
        let saved = self
            .model
            .saved_pages
            .iter()
            .find(|saved| saved.id == page.id)?;
        let title = if saved.metadata.title.is_empty() {
            &saved.metadata.address
        } else {
            &saved.metadata.title
        };
        let delete = keyboard_button(form_button("saved-delete", "Delete", &palette, true))
            .on_action(cx.listener(|this, _: &PressButton, _, cx| this.delete_saved_page(cx)))
            .on_click(cx.listener(|this, _, _, cx| this.delete_saved_page(cx)));
        Some(
            card.track_focus(&page.focus)
                .child(card_heading(format!("Delete saved page “{title}”?")))
                .child(note(
                    &palette,
                    "It is not open, so nothing else keeps it. Tab to Delete to confirm."
                        .to_owned(),
                ))
                .child(
                    button_row()
                        .child(delete)
                        .child(cancel_button(&palette, cx)),
                ),
        )
    }
}

fn name_form(card: Div, form: &NameForm, cx: &mut Context<BrowserView>) -> Div {
    let palette = theme::current();
    let heading = match form.target {
        NameTarget::New => "New folder",
        NameTarget::Rename(_) => "Rename folder",
    };
    let save = keyboard_button(form_button("folder-save", "Save", &palette, true))
        .on_action(cx.listener(|this, _: &PressButton, _, cx| this.submit_folder_name(cx)))
        .on_click(cx.listener(|this, _, _, cx| this.submit_folder_name(cx)));
    card.child(card_heading(heading))
        .child(Input::new(&form.input))
        .children(form.error.map(|error| note(&palette, error.to_owned())))
        .child(button_row().child(save).child(cancel_button(&palette, cx)))
}

/// Makes a sidebar button a tab stop that Enter or Space presses (`PressButton`).
pub(super) fn keyboard_button(button: Stateful<Div>) -> Stateful<Div> {
    button
        .key_context(BUTTON_CONTEXT)
        .tab_index(0)
        .focus(|style| style.border_1().border_color(theme::accent()))
}

/// The tile editor's card shape, in the sidebar.
fn card_frame() -> Div {
    let palette = theme::current();
    div()
        .key_context(CARD_CONTEXT)
        .p(px(space::L))
        .rounded(px(radius::TILE))
        .bg(rgba(palette.surface))
        .border_1()
        .border_color(rgba(palette.surface_line))
        .flex()
        .flex_col()
        .gap(px(space::M))
}

fn button_row() -> Div {
    div().flex().gap(px(space::M))
}

fn card_heading(heading: impl Into<String>) -> Div {
    div()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text())
        .child(heading.into())
}

fn note(palette: &Palette, line: String) -> Div {
    div()
        .text_size(px(text::SMALL))
        .text_color(rgba(palette.text_secondary))
        .child(line)
}

fn cancel_button(palette: &Palette, cx: &mut Context<BrowserView>) -> Stateful<Div> {
    keyboard_button(form_button("folder-cancel", "Cancel", palette, false))
        .on_action(cx.listener(|this, _: &PressButton, _, cx| this.cancel_folder_card(cx)))
        .on_click(cx.listener(|this, _, _, cx| this.cancel_folder_card(cx)))
}
