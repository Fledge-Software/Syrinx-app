//! The sidebar's folder and saved-row controls: naming, removal with its confirmations,
//! moves, and where focus goes after a row is used (P1-21).

use atlas_core::{
    ActivationTarget, FolderContents, SavedEntryId, SavedFolderId, SavedPageId,
    saved_folder_contents,
};
use gpui::{
    App, AppContext, Context, Entity, FocusHandle, IntoElement, KeyBinding, ParentElement, Render,
    Styled, Subscription, Window, actions, div, px,
};
use gpui_component::input::{InputEvent, InputState};

use super::{
    BrowserView,
    saved_moves::{DropTarget, Step, drop_move, keyboard_move},
};
use crate::{
    session::{FolderCommand, SessionCommand},
    theme,
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
/// The key context of the naming form and the removal cards.
pub(super) const CARD_CONTEXT: &str = "AtlasFolderCard";

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
    RemovePage(PageCard),
}

pub(super) struct NameForm {
    pub(super) target: NameTarget,
    pub(super) input: Entity<InputState>,
    pub(super) error: Option<&'static str>,
    _subscription: Subscription,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NameTarget {
    New,
    Rename(SavedFolderId),
}

pub(super) struct RemoveCard {
    pub(super) id: SavedFolderId,
    pub(super) step: RemoveStep,
    /// What the card shows; a confirmed delete carries it so the worker can refuse a change.
    pub(super) shown: FolderContents,
    pub(super) focus: FocusHandle,
}

/// The removal card's two steps.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RemoveStep {
    /// Offers the default removal, which keeps every page.
    Choose,
    /// Asks to confirm deleting the contents; only the focused Delete button confirms.
    ConfirmDelete,
}

/// Confirms removing a saved page that is not open, because nothing else would keep it.
pub(super) struct PageCard {
    pub(super) id: SavedPageId,
    pub(super) focus: FocusHandle,
}

/// What a removal card does when its folder changes underneath it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CardFollow {
    Keep,
    /// The default removal loses nothing, so it just shows the new counts.
    Refresh(FolderContents),
    /// A delete confirmation for contents the owner did not see is withdrawn.
    Close,
}

/// Where keyboard focus goes after a saved page or tab row is activated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RowFocus {
    /// The peek closes and hands focus to the page itself.
    DismissPeek,
    /// The row lets go of focus and the shown page takes it, so row keys stop applying.
    Page,
}

/// What a row being dragged carries, and the small label that follows the pointer.
#[derive(Clone)]
pub(super) struct DraggedRow {
    pub(super) entry: SavedEntryId,
    pub(super) title: String,
}

impl Render for DraggedRow {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        use atlas_ui::metrics::{radius, space, text};
        div()
            .px(px(space::M))
            .py(px(space::XS))
            .rounded(px(radius::CONTROL))
            .bg(theme::preview_row_selected())
            .text_color(theme::text())
            .text_size(px(text::BODY))
            .child(self.title.clone())
    }
}

impl BrowserView {
    /// The one handler for using a saved page or tab row (click, Enter, Space): activate it
    /// through `activate()`, then give focus to the page so later keys go there, not the row.
    pub(super) fn activate_row(
        &mut self,
        target: ActivationTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.activate(target);
        match row_focus_after_activation(self.is_peek_window(window)) {
            RowFocus::DismissPeek => {
                self.queue_peek_action(super::peek::PeekAction::Dismiss, window, cx);
            }
            RowFocus::Page => {
                window.blur(cx);
                self.focus_page(window);
            }
        }
        cx.notify();
    }

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

    pub(super) fn submit_folder_name(&mut self, cx: &mut Context<Self>) {
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
        let shown = saved_folder_contents(&self.model.saved_layout, id);
        if shown.is_empty() {
            self.folder_editor = None;
            self.command(SessionCommand::Folder(FolderCommand::Remove { id }));
            cx.notify();
            return;
        }
        let step = if delete_contents {
            RemoveStep::ConfirmDelete
        } else {
            RemoveStep::Choose
        };
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        self.folder_editor = Some(FolderEditor::Remove(RemoveCard {
            id,
            step,
            shown,
            focus,
        }));
        cx.notify();
    }

    /// Removes an open saved page at once (D43 keeps its tab); asks first when it is not open.
    pub(super) fn request_page_removal(
        &mut self,
        id: SavedPageId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let live = self
            .model
            .tabs
            .iter()
            .any(|tab| tab.saved_page_id == Some(id));
        if !page_removal_needs_confirmation(live) {
            self.command(SessionCommand::Unsave(id));
            cx.notify();
            return;
        }
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        self.folder_editor = Some(FolderEditor::RemovePage(PageCard { id, focus }));
        cx.notify();
    }

    /// Removes the folder and keeps its contents: the card's default (Remove, or Enter).
    pub(super) fn remove_keeping_contents(&mut self, cx: &mut Context<Self>) {
        let Some(FolderEditor::Remove(card)) = &self.folder_editor else {
            return;
        };
        let id = card.id;
        self.folder_editor = None;
        self.command(SessionCommand::Folder(FolderCommand::Remove { id }));
        cx.notify();
    }

    /// Deletes the folder with exactly the contents the card showed (the Delete button).
    pub(super) fn delete_with_contents(&mut self, cx: &mut Context<Self>) {
        let Some(FolderEditor::Remove(card)) = &self.folder_editor else {
            return;
        };
        if card.step != RemoveStep::ConfirmDelete {
            return;
        }
        let command = FolderCommand::DeleteWithContents {
            id: card.id,
            expected: card.shown,
        };
        self.folder_editor = None;
        self.command(SessionCommand::Folder(command));
        cx.notify();
    }

    /// Removes the unopened saved page the card asked about (its Delete button).
    pub(super) fn delete_saved_page(&mut self, cx: &mut Context<Self>) {
        let Some(FolderEditor::RemovePage(card)) = &self.folder_editor else {
            return;
        };
        let id = card.id;
        self.folder_editor = None;
        self.command(SessionCommand::Unsave(id));
        cx.notify();
    }

    /// Enter on a card: only the non-destructive default; a delete needs its focused button.
    pub(super) fn confirm_folder_card(&mut self, cx: &mut Context<Self>) {
        match &self.folder_editor {
            Some(FolderEditor::Name(_)) => self.submit_folder_name(cx),
            Some(FolderEditor::Remove(card)) if enter_removes(card.step) => {
                self.remove_keeping_contents(cx);
            }
            _ => {}
        }
    }

    /// Moves the card from the default removal to the delete confirmation.
    pub(super) fn choose_delete_contents(&mut self, cx: &mut Context<Self>) {
        if let Some(FolderEditor::Remove(card)) = &mut self.folder_editor {
            card.step = RemoveStep::ConfirmDelete;
            cx.notify();
        }
    }

    pub(super) fn cancel_folder_card(&mut self, cx: &mut Context<Self>) {
        self.folder_editor = None;
        cx.notify();
    }

    /// Keeps an open form or card true to the latest state: it closes when its folder or
    /// page is gone or a delete confirmation no longer matches, and otherwise refreshes.
    pub(super) fn follow_folder_editor(&mut self) {
        const CHANGED: &str = "That changed before it was removed, so nothing was removed.";
        let folders = &self.model.saved_folders;
        let (close, message) = match &mut self.folder_editor {
            Some(FolderEditor::Name(NameForm {
                target: NameTarget::Rename(id),
                ..
            })) => (!folders.iter().any(|folder| folder.id == *id), None),
            Some(FolderEditor::Remove(card)) => {
                let now = folders
                    .iter()
                    .any(|folder| folder.id == card.id)
                    .then(|| saved_folder_contents(&self.model.saved_layout, card.id));
                match follow_removal(card.step, card.shown, now) {
                    CardFollow::Keep => (false, None),
                    CardFollow::Refresh(contents) => {
                        card.shown = contents;
                        (false, None)
                    }
                    CardFollow::Close => (true, Some(CHANGED)),
                }
            }
            Some(FolderEditor::RemovePage(card)) => {
                let id = card.id;
                let saved = self.model.saved_pages.iter().any(|page| page.id == id);
                let live = self
                    .model
                    .tabs
                    .iter()
                    .any(|tab| tab.saved_page_id == Some(id));
                (!saved || live, Some(CHANGED))
            }
            _ => (false, None),
        };
        if !close {
            return;
        }
        self.folder_editor = None;
        if let Some(message) = message {
            self.message = message.to_owned();
        }
    }

    /// Applies a keyboard move to the focused row; returns whether one was planned, so an
    /// unused Alt arrow can go on to the window's own binding (Alt Left is Back).
    pub(super) fn move_saved_row(&mut self, entry: SavedEntryId, step: Step) -> bool {
        let Some(planned) = keyboard_move(&self.model.saved_layout, entry, step) else {
            return false;
        };
        self.send_move(planned.entry, planned.parent, planned.index);
        true
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
}

/// Activation from the peek lets the peek return focus; from the docked sidebar the page takes it.
pub(super) fn row_focus_after_activation(from_peek: bool) -> RowFocus {
    if from_peek {
        RowFocus::DismissPeek
    } else {
        RowFocus::Page
    }
}

/// An open saved page survives removal as an ordinary tab (D43); an unopened one would be lost.
pub(super) fn page_removal_needs_confirmation(live: bool) -> bool {
    !live
}

/// Enter performs only the default removal, which keeps every page.
pub(super) fn enter_removes(step: RemoveStep) -> bool {
    step == RemoveStep::Choose
}

/// How a removal card follows a change: `now` is the folder's contents, or `None` if it is gone.
pub(super) fn follow_removal(
    step: RemoveStep,
    shown: FolderContents,
    now: Option<FolderContents>,
) -> CardFollow {
    match now {
        None => CardFollow::Close,
        Some(now) if now == shown => CardFollow::Keep,
        Some(now) => match step {
            RemoveStep::Choose => CardFollow::Refresh(now),
            RemoveStep::ConfirmDelete => CardFollow::Close,
        },
    }
}

/// The trimmed name to save, or what to tell the owner instead.
pub(super) fn folder_name(text: &str) -> Result<String, &'static str> {
    let name = text.trim();
    if name.is_empty() {
        return Err("Give the folder a name.");
    }
    Ok(name.to_owned())
}

/// "3 saved pages and 2 folders", naming only the kinds the folder holds.
pub(super) fn contents_phrase(contents: FolderContents) -> String {
    let count = |number: usize, one: &str, many: &str| {
        format!("{number} {}", if number == 1 { one } else { many })
    };
    let parts: Vec<String> = [
        (contents.pages, "saved page", "saved pages"),
        (contents.folders, "folder", "folders"),
    ]
    .into_iter()
    .filter(|(number, _, _)| *number > 0)
    .map(|(number, one, many)| count(number, one, many))
    .collect();
    if parts.is_empty() {
        return "nothing".to_owned();
    }
    parts.join(" and ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(pages: usize, folders: usize) -> FolderContents {
        FolderContents { pages, folders }
    }

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
    fn the_removal_card_names_only_what_the_folder_holds() {
        assert_eq!(contents_phrase(held(1, 0)), "1 saved page");
        assert_eq!(contents_phrase(held(0, 1)), "1 folder");
        assert_eq!(contents_phrase(held(3, 2)), "3 saved pages and 2 folders");
        assert_eq!(contents_phrase(held(0, 0)), "nothing");
    }

    #[test]
    fn a_docked_row_hands_focus_to_the_page_and_the_peek_dismisses_itself() {
        assert_eq!(row_focus_after_activation(false), RowFocus::Page);
        assert_eq!(row_focus_after_activation(true), RowFocus::DismissPeek);
    }

    #[test]
    fn only_an_unopened_saved_page_asks_before_removal() {
        assert!(page_removal_needs_confirmation(false));
        assert!(!page_removal_needs_confirmation(true));
    }

    #[test]
    fn enter_never_confirms_a_delete() {
        assert!(enter_removes(RemoveStep::Choose));
        assert!(!enter_removes(RemoveStep::ConfirmDelete));
    }

    #[test]
    fn a_delete_confirmation_closes_when_the_folder_changes() {
        let shown = held(2, 0);
        assert_eq!(
            follow_removal(RemoveStep::ConfirmDelete, shown, Some(shown)),
            CardFollow::Keep
        );
        assert_eq!(
            follow_removal(RemoveStep::ConfirmDelete, shown, Some(held(3, 0))),
            CardFollow::Close
        );
        assert_eq!(
            follow_removal(RemoveStep::Choose, shown, Some(held(3, 0))),
            CardFollow::Refresh(held(3, 0))
        );
        assert_eq!(
            follow_removal(RemoveStep::Choose, shown, None),
            CardFollow::Close
        );
    }
}
