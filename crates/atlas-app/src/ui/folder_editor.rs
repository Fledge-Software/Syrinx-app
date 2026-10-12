//! The sidebar's folder controls: naming, removal with its confirmation, and moves (P1-21).

use atlas_core::{FolderRemoval, SavedEntryId, SavedFolderId};
use atlas_ui::metrics::{radius, text};
use gpui::{
    App, AppContext, Context, Div, Entity, FocusHandle, FontWeight, InteractiveElement,
    IntoElement, KeyBinding, ParentElement, Render, Stateful, StatefulInteractiveElement, Styled,
    Subscription, Window, actions, div, px,
};
use gpui_component::input::{Input, InputEvent, InputState};

use super::{
    BrowserView,
    saved_moves::{DropTarget, Step, contents, drop_move, keyboard_move},
    tile_editor::form_button,
};
use crate::{
    session::{FolderCommand, SessionCommand},
    theme::{self, rgba},
};

actions!(
    saved_rows,
    [
        OpenRow,
        RenameRow,
        RemoveRow,
        DeleteFolderContents,
        MoveRowUp,
        MoveRowDown,
        MoveRowOut,
        MoveRowIn,
        PressButton,
        ConfirmFolderCard,
        CancelFolderCard
    ]
);

/// The key context of a focused sidebar row.
pub(super) const ROW_CONTEXT: &str = "AtlasSavedRow";
/// The key context of a focusable sidebar button.
pub(super) const BUTTON_CONTEXT: &str = "AtlasSidebarButton";
/// The key context of the naming form and the removal card.
const CARD_CONTEXT: &str = "AtlasFolderCard";

/// Row keys win over the window's own (Alt Left is Back elsewhere) only while a row has focus.
pub(super) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", OpenRow, Some(ROW_CONTEXT)),
        KeyBinding::new("space", OpenRow, Some(ROW_CONTEXT)),
        KeyBinding::new("f2", RenameRow, Some(ROW_CONTEXT)),
        KeyBinding::new("delete", RemoveRow, Some(ROW_CONTEXT)),
        KeyBinding::new("shift-delete", DeleteFolderContents, Some(ROW_CONTEXT)),
        KeyBinding::new("alt-up", MoveRowUp, Some(ROW_CONTEXT)),
        KeyBinding::new("alt-down", MoveRowDown, Some(ROW_CONTEXT)),
        KeyBinding::new("alt-left", MoveRowOut, Some(ROW_CONTEXT)),
        KeyBinding::new("alt-right", MoveRowIn, Some(ROW_CONTEXT)),
        KeyBinding::new("enter", PressButton, Some(BUTTON_CONTEXT)),
        KeyBinding::new("space", PressButton, Some(BUTTON_CONTEXT)),
        KeyBinding::new("enter", ConfirmFolderCard, Some(CARD_CONTEXT)),
        KeyBinding::new("escape", CancelFolderCard, Some(CARD_CONTEXT)),
        KeyBinding::new("shift-delete", DeleteFolderContents, Some(CARD_CONTEXT)),
    ]);
}

/// The open naming form or removal card; at most one shows, under the saved section's header.
pub(super) enum FolderEditor {
    Name(NameForm),
    Remove(RemoveCard),
}

pub(super) struct NameForm {
    target: NameTarget,
    input: Entity<InputState>,
    error: Option<&'static str>,
    _subscription: Subscription,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NameTarget {
    New,
    Rename(SavedFolderId),
}

pub(super) struct RemoveCard {
    id: SavedFolderId,
    /// False offers the default removal; true is the explicit delete's confirmation step.
    deleting: bool,
    focus: FocusHandle,
}

/// What a row being dragged carries, and the small label that follows the pointer.
#[derive(Clone)]
pub(super) struct DraggedRow {
    pub(super) entry: SavedEntryId,
    pub(super) title: String,
}

impl Render for DraggedRow {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(theme::preview_row_selected())
            .text_color(theme::text())
            .text_sm()
            .child(self.title.clone())
    }
}

impl BrowserView {
    /// Opens the naming form for a new top-level folder.
    pub(super) fn start_new_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_name_form(NameTarget::New, String::new(), window, cx);
    }

    /// Opens the naming form filled with the folder's current name.
    pub(super) fn start_rename_folder(
        &mut self,
        id: SavedFolderId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(folder) = self
            .model
            .saved_folders
            .iter()
            .find(|folder| folder.id == id)
        else {
            return;
        };
        let title = folder.title.clone();
        self.open_name_form(NameTarget::Rename(id), title, window, cx);
    }

    fn open_name_form(
        &mut self,
        target: NameTarget,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Folder name"));
        input.update(cx, |input, cx| {
            input.set_value(value, window, cx);
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        let subscription = cx.subscribe_in(&input, window, |this, _, event, _, cx| {
            if let InputEvent::PressEnter { .. } = event {
                this.submit_folder_name(cx);
            }
        });
        self.folder_editor = Some(FolderEditor::Name(NameForm {
            target,
            input,
            error: None,
            _subscription: subscription,
        }));
        cx.notify();
    }

    fn submit_folder_name(&mut self, cx: &mut Context<Self>) {
        let Some(FolderEditor::Name(form)) = &mut self.folder_editor else {
            return;
        };
        let title = match folder_name(form.input.read(cx).value().as_ref()) {
            Ok(title) => title,
            Err(message) => {
                form.error = Some(message);
                cx.notify();
                return;
            }
        };
        let command = match form.target {
            NameTarget::New => FolderCommand::Create {
                title,
                parent: None,
            },
            NameTarget::Rename(id) => FolderCommand::Rename { id, title },
        };
        self.folder_editor = None;
        self.command(SessionCommand::Folder(command));
        cx.notify();
    }

    /// Removes an empty folder at once; otherwise asks how, starting at the delete
    /// confirmation when `delete_contents` is the owner's explicit choice.
    pub(super) fn request_folder_removal(
        &mut self,
        id: SavedFolderId,
        delete_contents: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if contents(&self.model.saved_layout, id) == (0, 0) {
            self.remove_folder(id, FolderRemoval::MoveContentsUp, cx);
            return;
        }
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        self.folder_editor = Some(FolderEditor::Remove(RemoveCard {
            id,
            deleting: delete_contents,
            focus,
        }));
        cx.notify();
    }

    fn remove_folder(&mut self, id: SavedFolderId, removal: FolderRemoval, cx: &mut Context<Self>) {
        self.folder_editor = None;
        self.command(SessionCommand::Folder(FolderCommand::Remove {
            id,
            removal,
        }));
        cx.notify();
    }

    /// Enter on the card: the default removal, or the delete once it has been chosen.
    fn confirm_folder_card(&mut self, cx: &mut Context<Self>) {
        match &self.folder_editor {
            Some(FolderEditor::Name(_)) => self.submit_folder_name(cx),
            Some(FolderEditor::Remove(card)) => {
                let removal = if card.deleting {
                    FolderRemoval::RemoveContents
                } else {
                    FolderRemoval::MoveContentsUp
                };
                self.remove_folder(card.id, removal, cx);
            }
            None => {}
        }
    }

    /// Moves the card from the default removal to the delete confirmation.
    fn choose_delete_contents(&mut self, cx: &mut Context<Self>) {
        if let Some(FolderEditor::Remove(card)) = &mut self.folder_editor {
            card.deleting = true;
            cx.notify();
        }
    }

    fn cancel_folder_card(&mut self, cx: &mut Context<Self>) {
        self.folder_editor = None;
        cx.notify();
    }

    /// Closes a form or card whose folder another change removed.
    pub(super) fn follow_folder_editor(&mut self) {
        let target = match &self.folder_editor {
            Some(FolderEditor::Name(NameForm {
                target: NameTarget::Rename(id),
                ..
            }))
            | Some(FolderEditor::Remove(RemoveCard { id, .. })) => *id,
            _ => return,
        };
        if !self
            .model
            .saved_folders
            .iter()
            .any(|folder| folder.id == target)
        {
            self.folder_editor = None;
        }
    }

    /// Applies a keyboard move to the focused row; a row moved into a folder stays in view.
    pub(super) fn move_saved_row(&mut self, entry: SavedEntryId, step: Step) {
        let Some(planned) = keyboard_move(&self.model.saved_layout, entry, step) else {
            return;
        };
        self.send_move(planned.entry, planned.parent, planned.index);
    }

    /// Applies a drop onto a row or the header; drops onto itself or its contents are ignored.
    pub(super) fn drop_saved_row(&mut self, dragged: SavedEntryId, target: DropTarget) {
        let Some(planned) = drop_move(&self.model.saved_layout, dragged, target) else {
            return;
        };
        self.send_move(planned.entry, planned.parent, planned.index);
    }

    fn send_move(&mut self, entry: SavedEntryId, parent: Option<SavedFolderId>, index: usize) {
        if let Some(folder) = parent {
            self.expand_folder(folder);
        }
        self.command(SessionCommand::Folder(FolderCommand::Move {
            entry,
            parent,
            index,
        }));
    }

    /// The open form or card, drawn under the saved section's header in the wide sidebar.
    pub(super) fn folder_editor_view(&self, cx: &mut Context<Self>) -> Option<Div> {
        let editor = self.folder_editor.as_ref()?;
        let card = card_frame().on_action(cx.listener(|this, _: &CancelFolderCard, _, cx| {
            this.cancel_folder_card(cx);
        }));
        match editor {
            FolderEditor::Name(form) => Some(self.name_form(card, form, cx)),
            FolderEditor::Remove(removal) => self.removal_card(card, removal, cx),
        }
    }

    fn name_form(&self, card: Div, form: &NameForm, cx: &mut Context<Self>) -> Div {
        let palette = theme::current();
        let heading = match form.target {
            NameTarget::New => "New folder",
            NameTarget::Rename(_) => "Rename folder",
        };
        card.child(card_heading(heading))
            .child(Input::new(&form.input))
            .children(form.error.map(|error| {
                div()
                    .text_size(px(text::SMALL))
                    .text_color(rgba(palette.text_secondary))
                    .child(error)
            }))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        keyboard_button(form_button("folder-save", "Save", &palette, true))
                            .on_action(cx.listener(|this, _: &PressButton, _, cx| {
                                this.submit_folder_name(cx);
                            }))
                            .on_click(cx.listener(|this, _, _, cx| this.submit_folder_name(cx))),
                    )
                    .child(cancel_button(&palette, cx)),
            )
    }

    /// The removal card, or nothing once its folder is gone (the next state closes it).
    fn removal_card(&self, card: Div, removal: &RemoveCard, cx: &mut Context<Self>) -> Option<Div> {
        let palette = theme::current();
        let title = &self
            .model
            .saved_folders
            .iter()
            .find(|folder| folder.id == removal.id)?
            .title;
        let (pages, folders) = contents(&self.model.saved_layout, removal.id);
        let held = contents_phrase(pages, folders);
        let card = card
            .track_focus(&removal.focus)
            .on_action(cx.listener(|this, _: &ConfirmFolderCard, _, cx| {
                this.confirm_folder_card(cx);
            }))
            .on_action(cx.listener(|this, _: &DeleteFolderContents, _, cx| {
                this.choose_delete_contents(cx);
            }));
        let note = |line: String| {
            div()
                .text_size(px(text::SMALL))
                .text_color(rgba(palette.text_secondary))
                .child(line)
        };
        if removal.deleting {
            return Some(
                card.child(card_heading(format!(
                    "Delete “{title}” and everything in it?"
                )))
                .child(note(format!(
                    "{held} will be deleted. Open pages stay open as ordinary tabs."
                )))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            keyboard_button(form_button("folder-delete", "Delete", &palette, true))
                                .on_action(cx.listener(|this, _: &PressButton, _, cx| {
                                    this.confirm_folder_card(cx);
                                }))
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.confirm_folder_card(cx)),
                                ),
                        )
                        .child(cancel_button(&palette, cx)),
                ),
            );
        }
        Some(
            card.child(card_heading(format!("Remove “{title}”?")))
                .child(note(format!("Its {held} will move up one level.")))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .child(
                            keyboard_button(form_button("folder-remove", "Remove", &palette, true))
                                .on_action(cx.listener(|this, _: &PressButton, _, cx| {
                                    this.confirm_folder_card(cx);
                                }))
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.confirm_folder_card(cx)),
                                ),
                        )
                        .child(
                            keyboard_button(form_button(
                                "folder-delete-contents",
                                "Delete contents…",
                                &palette,
                                false,
                            ))
                            .on_action(cx.listener(|this, _: &PressButton, _, cx| {
                                this.choose_delete_contents(cx)
                            }))
                            .on_click(
                                cx.listener(|this, _, _, cx| this.choose_delete_contents(cx)),
                            ),
                        )
                        .child(cancel_button(&palette, cx)),
                ),
        )
    }
}

/// Makes a sidebar button a tab stop that Enter or Space presses (`PressButton`).
pub(super) fn keyboard_button(button: Stateful<Div>) -> Stateful<Div> {
    button
        .key_context(BUTTON_CONTEXT)
        .tab_index(0)
        .focus(|style| style.border_1().border_color(theme::accent()))
}

/// The trimmed name to save, or what to tell the owner instead.
pub(super) fn folder_name(text: &str) -> Result<String, &'static str> {
    let name = text.trim();
    if name.is_empty() {
        return Err("Give the folder a name.");
    }
    Ok(name.to_owned())
}

/// "2 saved pages and 1 folder", for the removal card.
pub(super) fn contents_phrase(pages: usize, folders: usize) -> String {
    let count = |number: usize, one: &str, many: &str| {
        format!("{number} {}", if number == 1 { one } else { many })
    };
    let pages = count(pages, "saved page", "saved pages");
    match folders {
        0 => pages,
        folders => format!("{pages} and {}", count(folders, "folder", "folders")),
    }
}

/// The tile editor's card shape, in the sidebar.
fn card_frame() -> Div {
    let palette = theme::current();
    div()
        .key_context(CARD_CONTEXT)
        .p(px(10.))
        .rounded(px(radius::TILE))
        .bg(rgba(palette.surface))
        .border_1()
        .border_color(rgba(palette.surface_line))
        .flex()
        .flex_col()
        .gap(px(8.))
}

fn card_heading(heading: impl Into<String>) -> Div {
    div()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text())
        .child(heading.into())
}

fn cancel_button(palette: &atlas_ui::Palette, cx: &mut Context<BrowserView>) -> Stateful<Div> {
    keyboard_button(form_button("folder-cancel", "Cancel", palette, false))
        .on_action(cx.listener(|this, _: &PressButton, _, cx| this.cancel_folder_card(cx)))
        .on_click(cx.listener(|this, _, _, cx| this.cancel_folder_card(cx)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_names_are_trimmed_and_blank_ones_refused() {
        assert_eq!(
            folder_name("  Synthetic work "),
            Ok("Synthetic work".to_owned())
        );
        for blank in ["", "   ", "\t", "\u{3000}"] {
            assert_eq!(
                folder_name(blank),
                Err("Give the folder a name."),
                "{blank:?}"
            );
        }
    }

    #[test]
    fn the_removal_card_counts_what_moves_or_goes() {
        assert_eq!(contents_phrase(1, 0), "1 saved page");
        assert_eq!(contents_phrase(0, 1), "0 saved pages and 1 folder");
        assert_eq!(contents_phrase(3, 2), "3 saved pages and 2 folders");
    }
}
