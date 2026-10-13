//! The sidebar's folder and saved-row controls: naming, removal with its confirmations,
//! moves, and where focus goes after a row is used (P1-21).

use gpui::{
    App, AppContext, Context, Entity, FocusHandle, IntoElement, KeyBinding, ParentElement, Render,
    Styled, Subscription, Window, actions, div, px,
};
use gpui_component::input::{InputEvent, InputState};
use syrinx_core::{
    ActivationTarget, FolderContents, SavedEntryId, SavedFolderId, SavedPageId, TabId,
    TabModelSnapshot, saved_folder_contents,
};

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
        RenameRow,
        RemoveRow,
        DeleteFolderContents,
        MoveRowUp,
        MoveRowDown,
        MoveRowOut,
        MoveRowIn,
        ConfirmFolderCard,
        CancelFolderCard
    ]
);

/// The key context of a focused sidebar row.
pub(super) const ROW_CONTEXT: &str = "SyrinxSavedRow";
/// The key context of the naming form and the removal cards.
pub(super) const CARD_CONTEXT: &str = "SyrinxFolderCard";

/// Row keys win over the window's own (Alt Left is Back elsewhere) only while a row has focus.
/// Enter and Space on a focused row or button are GPUI keyboard clicks (P1-36's `control`),
/// so they reach the same `on_click` handler as the mouse.
pub(super) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("f2", RenameRow, Some(ROW_CONTEXT)),
        KeyBinding::new("delete", RemoveRow, Some(ROW_CONTEXT)),
        KeyBinding::new("shift-delete", DeleteFolderContents, Some(ROW_CONTEXT)),
        KeyBinding::new("alt-up", MoveRowUp, Some(ROW_CONTEXT)),
        KeyBinding::new("alt-down", MoveRowDown, Some(ROW_CONTEXT)),
        KeyBinding::new("alt-left", MoveRowOut, Some(ROW_CONTEXT)),
        KeyBinding::new("alt-right", MoveRowIn, Some(ROW_CONTEXT)),
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

/// What a removal card does when its folder or page changes underneath it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum CardFollow {
    Keep,
    /// The default removal loses nothing, so it just shows the new contents.
    Refresh(FolderContents),
    /// The card closes, and the status line says why.
    Close(Withdrawn),
}

/// Why an open removal card closed without doing anything.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Withdrawn {
    FolderGone,
    /// A delete confirmation for contents the owner did not see.
    FolderChanged,
    PageGone,
    /// An opened page no longer needs the confirmation: removing it keeps its tab.
    PageOpened,
}

impl Withdrawn {
    pub(super) fn message(self) -> &'static str {
        match self {
            Self::FolderGone => "That folder was removed elsewhere, so its card closed.",
            Self::FolderChanged => {
                "That folder's contents changed, so nothing was deleted. Choose again."
            }
            Self::PageGone => "That saved page was removed elsewhere, so its card closed.",
            Self::PageOpened => {
                "That page is open now, so removing it would keep its tab. Remove it again if you \
                 still want to."
            }
        }
    }
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
        use syrinx_ui::metrics::{radius, space, text};
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
    /// The one handler for using a saved page or tab row (a mouse or keyboard click): activate
    /// it through `activate()`, then give focus to the page so later keys go there, not the row.
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
                self.release_row_focus(window, cx);
                // A page that is not shown yet is focused by `sync_pages` when it appears;
                // focusing now would reach the page that was showing before.
                if target_is_shown(&self.model, target, self.shown) {
                    self.focus_page(window, cx);
                }
            }
        }
        cx.notify();
    }

    /// Takes keyboard focus off a sidebar row, back to the window's root (as `focus_page`
    /// does), so the row's own keys stop applying.
    pub(super) fn release_row_focus(&self, window: &mut Window, cx: &mut App) {
        if Window::window_handle(window) == self.main_window {
            window.focus(&self.focus, cx);
        } else {
            window.blur(cx);
        }
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
            expected: card.shown.clone(),
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

    /// Enter on the removal card itself performs only the non-destructive default. Enter on
    /// a focused button inside it is passed on, so that button's own click runs; a delete
    /// therefore always needs its Delete button.
    pub(super) fn confirm_folder_card(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(FolderEditor::Remove(card)) = &self.folder_editor else {
            cx.propagate();
            return;
        };
        if !card.focus.is_focused(window) || !enter_removes(card.step) {
            cx.propagate();
            return;
        }
        self.remove_keeping_contents(cx);
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
        let folders = &self.model.saved_folders;
        let (close, withdrawn) = match &mut self.folder_editor {
            Some(FolderEditor::Name(NameForm {
                target: NameTarget::Rename(id),
                ..
            })) => (!folders.iter().any(|folder| folder.id == *id), None),
            Some(FolderEditor::Remove(card)) => {
                let now = folders
                    .iter()
                    .any(|folder| folder.id == card.id)
                    .then(|| saved_folder_contents(&self.model.saved_layout, card.id));
                match follow_removal(card.step, &card.shown, now) {
                    CardFollow::Keep => (false, None),
                    CardFollow::Refresh(contents) => {
                        card.shown = contents;
                        (false, None)
                    }
                    CardFollow::Close(reason) => (true, Some(reason)),
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
                let reason = follow_page_removal(saved, live);
                (reason.is_some(), reason)
            }
            _ => (false, None),
        };
        if !close {
            return;
        }
        self.folder_editor = None;
        if let Some(reason) = withdrawn {
            self.message = reason.message().to_owned();
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

/// Whether the activated destination is already the shown page, so focusing now reaches it.
pub(super) fn target_is_shown(
    model: &TabModelSnapshot,
    target: ActivationTarget,
    shown: Option<TabId>,
) -> bool {
    let live = match target {
        ActivationTarget::Tab(id) => Some(id),
        ActivationTarget::Saved(saved) => model
            .tabs
            .iter()
            .find(|tab| tab.saved_page_id == Some(saved))
            .map(|tab| tab.id),
    };
    live.is_some() && live == shown
}

/// How a folder removal card follows a change: `now` is the folder's contents, or `None`
/// if the folder is gone. The rows themselves are compared, so a swap is noticed.
pub(super) fn follow_removal(
    step: RemoveStep,
    shown: &FolderContents,
    now: Option<FolderContents>,
) -> CardFollow {
    match now {
        None => CardFollow::Close(Withdrawn::FolderGone),
        Some(now) if now == *shown => CardFollow::Keep,
        Some(now) => match step {
            RemoveStep::Choose => CardFollow::Refresh(now),
            RemoveStep::ConfirmDelete => CardFollow::Close(Withdrawn::FolderChanged),
        },
    }
}

/// Why an unopened-page card closes, if it does: the page is gone, or it is open now.
pub(super) fn follow_page_removal(saved: bool, live: bool) -> Option<Withdrawn> {
    if !saved {
        return Some(Withdrawn::PageGone);
    }
    live.then_some(Withdrawn::PageOpened)
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
pub(super) fn contents_phrase(contents: &FolderContents) -> String {
    let count = |number: usize, one: &str, many: &str| {
        format!("{number} {}", if number == 1 { one } else { many })
    };
    let parts: Vec<String> = [
        (contents.pages(), "saved page", "saved pages"),
        (contents.folders(), "folder", "folders"),
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

    /// Synthetic contents with `pages` pages (IDs from 1) and `folders` folders (IDs from 100).
    fn held(pages: u64, folders: u64) -> FolderContents {
        let pages = (1..=pages).map(|id| SavedEntryId::Page(page_id(id)));
        let folders = (100..100 + folders)
            .map(|id| SavedEntryId::Folder(SavedFolderId::new(id).expect("synthetic folder id")));
        pages.chain(folders).collect()
    }

    fn page_id(id: u64) -> SavedPageId {
        SavedPageId::new(id).expect("synthetic page id")
    }

    fn tab_id(id: u64) -> TabId {
        TabId::new(id).expect("synthetic tab id")
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
        assert_eq!(contents_phrase(&held(1, 0)), "1 saved page");
        assert_eq!(contents_phrase(&held(0, 1)), "1 folder");
        assert_eq!(contents_phrase(&held(3, 2)), "3 saved pages and 2 folders");
        assert_eq!(contents_phrase(&held(0, 0)), "nothing");
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
            follow_removal(RemoveStep::ConfirmDelete, &shown, Some(shown.clone())),
            CardFollow::Keep
        );
        assert_eq!(
            follow_removal(RemoveStep::ConfirmDelete, &shown, Some(held(3, 0))),
            CardFollow::Close(Withdrawn::FolderChanged)
        );
        assert_eq!(
            follow_removal(RemoveStep::Choose, &shown, Some(held(3, 0))),
            CardFollow::Refresh(held(3, 0))
        );
    }

    #[test]
    fn a_swap_with_equal_counts_still_withdraws_the_delete() {
        let shown = held(2, 0);
        // Page 2 left and page 3 arrived: still two pages, but not the ones shown.
        let swapped: FolderContents = [1, 3]
            .into_iter()
            .map(|id| SavedEntryId::Page(page_id(id)))
            .collect();
        assert_eq!(
            (swapped.pages(), swapped.folders()),
            (shown.pages(), shown.folders())
        );
        assert_eq!(
            follow_removal(RemoveStep::ConfirmDelete, &shown, Some(swapped)),
            CardFollow::Close(Withdrawn::FolderChanged)
        );
    }

    #[test]
    fn a_folder_removed_elsewhere_is_reported_as_removed_not_changed() {
        for step in [RemoveStep::Choose, RemoveStep::ConfirmDelete] {
            assert_eq!(
                follow_removal(step, &held(1, 0), None),
                CardFollow::Close(Withdrawn::FolderGone)
            );
        }
        assert_eq!(
            Withdrawn::FolderGone.message(),
            "That folder was removed elsewhere, so its card closed."
        );
        assert!(!Withdrawn::FolderGone.message().contains("changed"));
    }

    #[test]
    fn an_unopened_page_card_closes_with_the_right_reason() {
        assert_eq!(follow_page_removal(true, false), None);
        assert_eq!(follow_page_removal(false, false), Some(Withdrawn::PageGone));
        assert_eq!(follow_page_removal(true, true), Some(Withdrawn::PageOpened));
    }

    #[test]
    fn focus_waits_until_the_activated_page_is_the_shown_one() {
        let mut model = syrinx_core::TabModel::new();
        let open = |model: &mut syrinx_core::TabModel, number: u64| {
            model
                .open(
                    syrinx_core::TabKind::Web,
                    syrinx_core::TabMetadata {
                        title: format!("Synthetic focus {number}"),
                        address: format!("https://focus-{number}.example.test/"),
                        ..syrinx_core::TabMetadata::default()
                    },
                    syrinx_core::Activation::Foreground,
                )
                .expect("synthetic tab opens")
        };
        let first = open(&mut model, 1);
        let saved = model.save_page(first).expect("synthetic page saves");
        let second = open(&mut model, 2);
        let snapshot = model.snapshot();

        // The old page is still shown, so focusing now would reach it, not the target.
        assert!(!target_is_shown(
            &snapshot,
            ActivationTarget::Tab(second),
            Some(first)
        ));
        assert!(!target_is_shown(
            &snapshot,
            ActivationTarget::Tab(second),
            None
        ));
        assert!(target_is_shown(
            &snapshot,
            ActivationTarget::Tab(second),
            Some(second)
        ));
        // A saved page is shown through its live tab; an unopened one never is yet.
        assert!(target_is_shown(
            &snapshot,
            ActivationTarget::Saved(saved),
            Some(first)
        ));
        assert!(!target_is_shown(
            &snapshot,
            ActivationTarget::Saved(page_id(99)),
            Some(first)
        ));
        assert!(!target_is_shown(
            &snapshot,
            ActivationTarget::Tab(tab_id(99)),
            Some(first)
        ));
    }
}
