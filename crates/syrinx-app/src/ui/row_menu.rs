//! The sidebar row's actions (#95, #58): what its right-click menu lists, and the one place
//! that runs them for the menu, the row's keys and its buttons.

use std::collections::{BTreeMap, HashSet};

use gpui::{ClipboardItem, Context, Pixels, Point, Window};
use syrinx_core::{
    BrowserAction, CommandAction, CommandRegistry, PageOrigin, SavedFolderId, SavedPageId,
    Shortcut, TabId, TabModelSnapshot,
};
use syrinx_platform::browser::BrowserCommand;

use super::{
    BrowserView,
    folder_editor::{FolderEditor, REMOVE_ROW_KEY, RENAME_ROW_KEY},
    menus::{Corner, MenuAnchor, MenuOpening, MenuReturn},
    mute::change_mute,
    sidebar_tree::{TreeId, live_tab},
};
use crate::session::SessionCommand;

/// The setting that says whether Close other tabs and Close tabs below ask first.
pub(super) const ASK_BULK_CLOSE_KEY: &str = "tabs.ask_before_bulk_close";

/// One thing a sidebar row can do. Each carries the identity it acts on, so a late choice
/// acts on the row it was offered for, never on whatever is selected by then.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TabAction {
    Reload(TabId),
    Duplicate(TabId),
    /// Mutes (`true`) or unmutes the tab's page; the badge follows what the page reports.
    SetMuted(TabId, bool),
    CopyLink(LinkSource),
    Save(TabId),
    /// Closes an ordinary tab; its row goes.
    Close(TabId),
    /// Closes a saved page's live tab; the saved row stays.
    Unload(TabId),
    /// Close other tabs or Close tabs below, which ask first unless the owner said not to.
    Bulk(BulkClose),
    ReopenClosed,
    RemoveSaved(SavedPageId),
    RenameFolder(SavedFolderId),
    RemoveFolder(SavedFolderId),
}

/// Where Copy link reads its address when it runs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum LinkSource {
    Tab(TabId),
    Saved(SavedPageId),
}

/// A bulk close and the tab it keeps.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct BulkClose {
    pub(super) kind: BulkKind,
    pub(super) tab: TabId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BulkKind {
    Others,
    Below,
}

impl TabAction {
    /// The item's name in the menu, which is also its accessible name.
    pub(super) fn label(self) -> &'static str {
        match self {
            TabAction::Reload(_) => "Reload tab",
            TabAction::Duplicate(_) => "Duplicate tab",
            TabAction::SetMuted(_, true) => "Mute tab",
            TabAction::SetMuted(_, false) => "Unmute tab",
            TabAction::CopyLink(_) => "Copy link",
            TabAction::Save(_) => "Save page",
            TabAction::Close(_) => "Close tab",
            TabAction::Unload(_) => "Unload page",
            TabAction::Bulk(BulkClose {
                kind: BulkKind::Others,
                ..
            }) => "Close other tabs",
            TabAction::Bulk(BulkClose {
                kind: BulkKind::Below,
                ..
            }) => "Close tabs below",
            TabAction::ReopenClosed => "Reopen closed tab",
            TabAction::RemoveSaved(_) => "Remove saved page",
            TabAction::RenameFolder(_) => "Rename folder",
            TabAction::RemoveFolder(_) => "Remove folder",
        }
    }
}

/// What the row menu needs to know about one row, read from the model by `row_subject`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RowSubject {
    OpenTab {
        tab: TabId,
        page: PageFacts,
        /// How many tabs Close other tabs and Close tabs below would close.
        others: usize,
        below: usize,
    },
    LiveSaved {
        saved: SavedPageId,
        tab: TabId,
        page: PageFacts,
    },
    UnopenedSaved(SavedPageId),
    Folder(SavedFolderId),
}

/// An open tab's facts that decide its items and their key labels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PageFacts {
    /// A web page, not Home or Settings, which have nothing to reload, mute or copy.
    pub(super) web: bool,
    pub(super) active: bool,
    /// The owner's mute choice, which the item offers to undo.
    pub(super) muted: bool,
}

/// The labels of the keys that act on a focused row, read from the same bindings the keys use.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct RowKeys {
    pub(super) close: Option<String>,
    pub(super) reload: Option<String>,
    pub(super) save: Option<String>,
    pub(super) reopen: Option<String>,
    pub(super) remove: Option<String>,
    pub(super) rename: Option<String>,
}

impl RowKeys {
    /// Ctrl W, Ctrl R, Ctrl D and Ctrl Shift T from the command registry; Delete and F2 from
    /// the row's own bindings.
    pub(super) fn from_bindings(registry: &CommandRegistry) -> Self {
        Self {
            close: browser_key_label(registry, BrowserAction::CloseTab),
            reload: browser_key_label(registry, BrowserAction::Reload),
            save: browser_key_label(registry, BrowserAction::SavePage),
            reopen: browser_key_label(registry, BrowserAction::ReopenTab),
            remove: key_label(REMOVE_ROW_KEY),
            rename: key_label(RENAME_ROW_KEY),
        }
    }
}

/// The registry's shortcut for one browser action as the menus show it, such as `Ctrl W`.
pub(super) fn browser_key_label(
    registry: &CommandRegistry,
    wanted: BrowserAction,
) -> Option<String> {
    registry
        .iter()
        .find_map(|(_, command)| match command.action {
            CommandAction::Browser(action) if action == wanted => command.shortcut.as_ref(),
            _ => None,
        })
        .map(Shortcut::label)
}

fn key_label(keystroke: &str) -> Option<String> {
    Shortcut::new(keystroke)
        .ok()
        .map(|shortcut| shortcut.label())
}

/// One menu line: an action with its key labels, or a separator between groups.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum RowEntry {
    Action {
        action: TabAction,
        keys: Option<String>,
    },
    Separator,
}

/// One item before grouping: the action and the keys that do the same on this row.
type Item = (TabAction, Option<String>);

/// The row's menu in Firefox's order (owner, 2026-10-27), groups split by separators. A key
/// is shown only where pressing it would act on this row: Ctrl R and Ctrl D act on the active
/// tab, so they label only the active row. `wide` is false in the favicon-only sidebar, where
/// renaming and removals that need the wide sidebar's card are left out.
pub(super) fn row_actions(subject: RowSubject, wide: bool, keys: &RowKeys) -> Vec<RowEntry> {
    let groups = match subject {
        RowSubject::OpenTab {
            tab,
            page,
            others,
            below,
        } => open_tab_groups(tab, page, (others, below), keys),
        RowSubject::LiveSaved { saved, tab, page } => vec![
            vec![
                (TabAction::Unload(tab), keys.close.clone()),
                (TabAction::RemoveSaved(saved), keys.remove.clone()),
            ],
            page_items(tab, page, keys),
            vec![(TabAction::ReopenClosed, keys.reopen.clone())],
        ],
        RowSubject::UnopenedSaved(saved) => vec![
            vec![(TabAction::CopyLink(LinkSource::Saved(saved)), None)],
            // Removing an unopened page asks first, on the wide sidebar's card.
            when(wide, (TabAction::RemoveSaved(saved), keys.remove.clone())),
        ],
        RowSubject::Folder(folder) if wide => vec![vec![
            (TabAction::RenameFolder(folder), keys.rename.clone()),
            (TabAction::RemoveFolder(folder), keys.remove.clone()),
        ]],
        RowSubject::Folder(_) => Vec::new(),
    };
    join_groups(groups)
}

/// An open tab's groups: its page, saving, closing, and reopening.
fn open_tab_groups(
    tab: TabId,
    page: PageFacts,
    (others, below): (usize, usize),
    keys: &RowKeys,
) -> Vec<Vec<Item>> {
    let close_keys = joined(&[keys.close.as_deref(), keys.remove.as_deref()]);
    let save_keys = keys.save.clone().filter(|_| page.active);
    let mut closing = vec![(TabAction::Close(tab), close_keys)];
    // A bulk close that would close nothing is left out, as Firefox greys it out.
    closing.extend(when(others > 0, bulk(BulkKind::Others, tab)));
    closing.extend(when(below > 0, bulk(BulkKind::Below, tab)));
    vec![
        page_items(tab, page, keys),
        when(page.web, (TabAction::Save(tab), save_keys)),
        closing,
        vec![(TabAction::ReopenClosed, keys.reopen.clone())],
    ]
}

/// Reload, Duplicate, Mute and Copy link, for a web page only.
fn page_items(tab: TabId, page: PageFacts, keys: &RowKeys) -> Vec<Item> {
    if !page.web {
        return Vec::new();
    }
    let reload_keys = keys.reload.clone().filter(|_| page.active);
    vec![
        (TabAction::Reload(tab), reload_keys),
        (TabAction::Duplicate(tab), None),
        (TabAction::SetMuted(tab, !page.muted), None),
        (TabAction::CopyLink(LinkSource::Tab(tab)), None),
    ]
}

fn bulk(kind: BulkKind, tab: TabId) -> Item {
    (TabAction::Bulk(BulkClose { kind, tab }), None)
}

/// A group of one item when `shown`, else an empty group.
fn when(shown: bool, item: Item) -> Vec<Item> {
    if shown { vec![item] } else { Vec::new() }
}

/// Labels joined as the menus show several keys, such as `Ctrl W, Delete`.
fn joined(labels: &[Option<&str>]) -> Option<String> {
    let present: Vec<&str> = labels.iter().flatten().copied().collect();
    (!present.is_empty()).then(|| present.join(", "))
}

/// The non-empty groups with one separator between each pair.
fn join_groups(groups: Vec<Vec<Item>>) -> Vec<RowEntry> {
    let mut entries = Vec::new();
    for group in groups.into_iter().filter(|group| !group.is_empty()) {
        if !entries.is_empty() {
            entries.push(RowEntry::Separator);
        }
        entries.extend(
            group
                .into_iter()
                .map(|(action, keys)| RowEntry::Action { action, keys }),
        );
    }
    entries
}

/// The row as the menu sees it, or `None` when the row is gone from the model.
pub(super) fn row_subject(
    model: &TabModelSnapshot,
    row: TreeId,
    muted: &HashSet<TabId>,
    popups: &HashSet<TabId>,
) -> Option<RowSubject> {
    let facts = |tab: TabId| {
        let shown = model.tabs.iter().find(|candidate| candidate.id == tab)?;
        let origin = PageOrigin::from_adopted_popup(popups.contains(&tab));
        Some(PageFacts {
            web: !shown.metadata.is_syrinx_page(origin),
            active: model.active_id == Some(tab),
            muted: muted.contains(&tab),
        })
    };
    match row {
        TreeId::Folder(folder) => model
            .saved_folders
            .iter()
            .any(|candidate| candidate.id == folder)
            .then_some(RowSubject::Folder(folder)),
        TreeId::Page(saved) => {
            model.saved_pages.iter().find(|page| page.id == saved)?;
            Some(match live_tab(model, saved) {
                Some(tab) => RowSubject::LiveSaved {
                    saved,
                    tab,
                    page: facts(tab)?,
                },
                None => RowSubject::UnopenedSaved(saved),
            })
        }
        TreeId::Tab(tab) => Some(RowSubject::OpenTab {
            tab,
            page: facts(tab)?,
            others: bulk_close_count(model, BulkKind::Others, tab),
            below: bulk_close_count(model, BulkKind::Below, tab),
        }),
    }
}

/// How many tabs a bulk close would close: the ordinary tabs other than `tab` (Others), or
/// those after it (Below). Open saved pages are spared, as `TabModel::close_others` and
/// `close_below` spare them.
pub(super) fn bulk_close_count(model: &TabModelSnapshot, kind: BulkKind, tab: TabId) -> usize {
    let Some(position) = model.tabs.iter().position(|candidate| candidate.id == tab) else {
        return 0;
    };
    let start = match kind {
        BulkKind::Others => 0,
        BulkKind::Below => position + 1,
    };
    model.tabs[start..]
        .iter()
        .filter(|candidate| candidate.saved_page_id.is_none() && candidate.id != tab)
        .count()
}

/// What choosing a bulk close does now.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BulkPlan {
    /// Nothing would close; the status line says so.
    Nothing,
    Ask,
    Close,
}

/// A bulk close asks every time unless the owner chose "Don't ask again" (owner, 2026-10-27).
pub(super) fn bulk_plan(ask: bool, count: usize) -> BulkPlan {
    match (count, ask) {
        (0, _) => BulkPlan::Nothing,
        (_, true) => BulkPlan::Ask,
        (_, false) => BulkPlan::Close,
    }
}

/// The stored choice; anything but an explicit "false" keeps asking.
pub(super) fn asks_before_bulk_close(settings: &BTreeMap<String, String>) -> bool {
    settings
        .get(ASK_BULK_CLOSE_KEY)
        .is_none_or(|value| value != "false")
}

pub(super) fn bulk_close_setting(ask: bool) -> (String, String) {
    (ASK_BULK_CLOSE_KEY.to_owned(), ask.to_string())
}

/// The confirmation's question, such as "Close 3 other tabs?".
pub(super) fn bulk_question(kind: BulkKind, count: usize) -> String {
    let tabs = tabs_phrase(count);
    match kind {
        BulkKind::Others => format!("Close {count} other {tabs}?"),
        BulkKind::Below => format!("Close {count} {tabs} below?"),
    }
}

/// The confirmation's Close button, such as "Close 3 tabs".
pub(super) fn bulk_confirm_label(count: usize) -> String {
    format!("Close {count} {}", tabs_phrase(count))
}

fn tabs_phrase(count: usize) -> &'static str {
    if count == 1 { "tab" } else { "tabs" }
}

/// Where a chosen action leaves the keys.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ActionFocus {
    /// The action placed them itself (a form, a card or a confirmation).
    Placed,
    /// The caller returns them where they were.
    Unplaced,
}

/// The menu a row action came from, so a confirmation can open in its place.
#[derive(Clone)]
pub(super) struct MenuOrigin {
    pub(super) anchor: MenuAnchor,
    pub(super) returns: MenuReturn,
}

impl BrowserView {
    /// The row's menu entries now, or none when the row is gone.
    pub(super) fn row_entries(&self, row: TreeId, wide: bool) -> Vec<RowEntry> {
        let Some(subject) = row_subject(&self.model, row, &self.muted, &self.popup_pages) else {
            return Vec::new();
        };
        row_actions(subject, wide, &RowKeys::from_bindings(&self.commands))
    }

    /// Runs one row action for the menu, the row's keys or its buttons. `menu` is the menu it
    /// was chosen from, where a bulk close asks; from anywhere else a bulk close does nothing.
    pub(super) fn run_tab_action(
        &mut self,
        action: TabAction,
        menu: Option<MenuOrigin>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ActionFocus {
        let placed = match action {
            TabAction::Reload(tab) => {
                self.reload_tab(tab);
                false
            }
            TabAction::Duplicate(tab) => self.command_unplaced(SessionCommand::Duplicate(tab)),
            TabAction::SetMuted(tab, mute) => {
                self.set_tab_muted(tab, mute);
                false
            }
            TabAction::CopyLink(source) => {
                self.copy_link(source, cx);
                false
            }
            TabAction::Save(tab) => self.command_unplaced(SessionCommand::SaveTab(tab)),
            TabAction::Close(tab) | TabAction::Unload(tab) => {
                self.command_unplaced(SessionCommand::CloseTab(tab))
            }
            TabAction::Bulk(bulk) => self.request_bulk_close(bulk, menu, window, cx),
            TabAction::ReopenClosed => self.command_unplaced(SessionCommand::ReopenClosed),
            TabAction::RemoveSaved(saved) => {
                self.request_page_removal(saved, window, cx);
                // An unopened page asks on its card, which takes the keys.
                matches!(self.folder_editor, Some(FolderEditor::RemovePage(_)))
            }
            TabAction::RenameFolder(folder) => {
                self.start_rename_folder(folder, window, cx);
                true
            }
            TabAction::RemoveFolder(folder) => {
                self.request_folder_removal(folder, false, window, cx);
                // A folder with contents asks how on its card, which takes the keys.
                matches!(self.folder_editor, Some(FolderEditor::Remove(_)))
            }
        };
        cx.notify();
        if placed {
            ActionFocus::Placed
        } else {
            ActionFocus::Unplaced
        }
    }

    /// Sends a session command for an action that leaves the keys to its caller.
    fn command_unplaced(&mut self, command: SessionCommand) -> bool {
        self.command(command);
        false
    }

    /// Reloads a tab's live page; a tab without one has nothing to reload.
    fn reload_tab(&mut self, tab: TabId) {
        if self.loaded.contains(&tab) {
            self.page_command(BrowserCommand::Reload { id: tab.get() });
        }
    }

    /// Records the owner's mute choice and asks the tab's live page to follow it. The badge
    /// waits for the page's own report (`audio_reported`).
    pub(super) fn set_tab_muted(&mut self, tab: TabId, mute: bool) {
        let Some(command) = change_mute(&mut self.muted, &self.loaded, tab, mute) else {
            return;
        };
        if self.page_command(command) {
            self.mute_requests.insert(tab);
        }
    }

    /// Copies the row's address. Home and Settings rows never offer it.
    fn copy_link(&mut self, source: LinkSource, cx: &mut Context<Self>) {
        let address = match source {
            LinkSource::Tab(tab) => self
                .model
                .tabs
                .iter()
                .find(|candidate| candidate.id == tab)
                .map(|candidate| candidate.metadata.address.clone()),
            LinkSource::Saved(saved) => self
                .model
                .saved_pages
                .iter()
                .find(|page| page.id == saved)
                .map(|page| page.metadata.address.clone()),
        };
        let Some(address) = address.filter(|address| !address.is_empty()) else {
            self.message = "That row has no link to copy.".to_owned();
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(address));
        self.message = "Link copied.".to_owned();
    }

    /// Asks before a bulk close, in the menu's place, unless the owner said not to ask;
    /// returns whether the confirmation took the keys.
    fn request_bulk_close(
        &mut self,
        bulk: BulkClose,
        menu: Option<MenuOrigin>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let count = bulk_close_count(&self.model, bulk.kind, bulk.tab);
        match bulk_plan(self.ask_before_bulk_close, count) {
            BulkPlan::Nothing => {
                self.message = "No other tabs would close.".to_owned();
                false
            }
            BulkPlan::Close => {
                self.close_in_bulk(bulk);
                false
            }
            BulkPlan::Ask => {
                let Some(menu) = menu else {
                    return false;
                };
                self.show_bulk_confirmation(bulk, count, menu, window, cx);
                true
            }
        }
    }

    /// Closes the tabs the confirmation named; with `remember`, it never asks again.
    pub(super) fn confirm_bulk_close(&mut self, bulk: BulkClose, remember: bool) {
        if remember {
            self.set_ask_before_bulk_close(false);
        }
        self.close_in_bulk(bulk);
    }

    fn close_in_bulk(&mut self, bulk: BulkClose) {
        self.command(match bulk.kind {
            BulkKind::Others => SessionCommand::CloseOthers(bulk.tab),
            BulkKind::Below => SessionCommand::CloseBelow(bulk.tab),
        });
    }

    /// Saves whether bulk closes ask first; Settings turns it back on.
    pub(super) fn set_ask_before_bulk_close(&mut self, ask: bool) {
        self.ask_before_bulk_close = ask;
        self.command(SessionCommand::SaveSettings(vec![bulk_close_setting(ask)]));
    }

    /// A right-click on a row opens its menu at the pointer; Escape gives the keys back to
    /// wherever the click found them.
    pub(super) fn open_row_menu_at(
        &mut self,
        row: TreeId,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let opening = MenuOpening {
            anchor: MenuAnchor {
                position,
                corner: Corner::TopLeft,
            },
            returns: MenuReturn::Was(self.chrome_return_focus(window, cx)),
            select_first: false,
        };
        self.show_row_menu(row, opening, window, cx);
    }

    /// Shift F10 or the Menu key on a focused row opens its menu at the row's bottom-left
    /// corner, first item selected; Escape gives the keys back to the row.
    pub(super) fn open_row_menu_by_key(
        &mut self,
        row: TreeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(position) = self.sidebar_trees.main_row_bottom_left(row) else {
            return; // The peek's rows, or a row not laid out yet, open no menu.
        };
        let opening = MenuOpening {
            anchor: MenuAnchor {
                position,
                corner: Corner::TopLeft,
            },
            returns: MenuReturn::Row(row),
            select_first: true,
        };
        self.show_row_menu(row, opening, window, cx);
    }
}

#[cfg(test)]
mod tests {
    use syrinx_core::{
        SavedEntryId, SavedFolderSnapshot, SavedPageSnapshot, SavedPlacement, TabKind, TabMetadata,
        TabModel, TabSnapshot,
    };

    use super::*;

    fn tab_id(id: u64) -> TabId {
        TabId::new(id).expect("synthetic tab id")
    }

    fn page_id(id: u64) -> SavedPageId {
        SavedPageId::new(id).expect("synthetic saved page id")
    }

    fn folder_id(id: u64) -> SavedFolderId {
        SavedFolderId::new(id).expect("synthetic folder id")
    }

    fn metadata(address: &str) -> TabMetadata {
        TabMetadata {
            title: "Synthetic".to_owned(),
            address: address.to_owned(),
            favicon: None,
            badge: None,
        }
    }

    fn tab(id: u64, saved: Option<u64>, address: &str) -> TabSnapshot {
        TabSnapshot {
            id: tab_id(id),
            kind: TabKind::Web,
            metadata: metadata(address),
            saved_page_id: saved.map(page_id),
        }
    }

    /// Saved page 1 live in tab 100, saved page 2 unopened, folder 10; ordinary tabs 200
    /// (active), 300 (Home) and 400, in that order.
    fn model() -> TabModelSnapshot {
        TabModelSnapshot {
            tabs: vec![
                tab(100, Some(1), "https://one.example.test/"),
                tab(200, None, "https://two.example.test/"),
                tab(300, None, "about:blank"),
                tab(400, None, "https://four.example.test/"),
            ],
            saved_pages: vec![
                SavedPageSnapshot {
                    id: page_id(1),
                    metadata: metadata("https://one.example.test/"),
                },
                SavedPageSnapshot {
                    id: page_id(2),
                    metadata: metadata("https://saved.example.test/"),
                },
            ],
            saved_folders: vec![SavedFolderSnapshot {
                id: folder_id(10),
                title: "Folder".to_owned(),
            }],
            saved_layout: vec![
                SavedPlacement {
                    entry: SavedEntryId::Page(page_id(1)),
                    parent: None,
                },
                SavedPlacement {
                    entry: SavedEntryId::Folder(folder_id(10)),
                    parent: None,
                },
                SavedPlacement {
                    entry: SavedEntryId::Page(page_id(2)),
                    parent: Some(folder_id(10)),
                },
            ],
            active_id: Some(tab_id(200)),
            ..TabModelSnapshot::default()
        }
    }

    fn keys() -> RowKeys {
        RowKeys::from_bindings(&CommandRegistry::browser())
    }

    fn subject(row: TreeId, muted: &[u64]) -> RowSubject {
        let muted = muted.iter().map(|id| tab_id(*id)).collect();
        row_subject(&model(), row, &muted, &HashSet::new()).expect("the row is in the model")
    }

    /// The menu as label and key pairs, with "—" for each separator.
    fn shown(entries: &[RowEntry]) -> Vec<(&'static str, Option<&str>)> {
        entries
            .iter()
            .map(|entry| match entry {
                RowEntry::Action { action, keys } => (action.label(), keys.as_deref()),
                RowEntry::Separator => ("—", None),
            })
            .collect()
    }

    #[test]
    fn the_keys_come_from_the_real_bindings() {
        let keys = keys();
        assert_eq!(keys.close.as_deref(), Some("Ctrl W"));
        assert_eq!(keys.reload.as_deref(), Some("Ctrl R"));
        assert_eq!(keys.save.as_deref(), Some("Ctrl D"));
        assert_eq!(keys.reopen.as_deref(), Some("Ctrl Shift T"));
        // #58 named Shift Delete for removal; the row's binding is Delete.
        assert_eq!(keys.remove.as_deref(), Some("Delete"));
        assert_eq!(keys.rename.as_deref(), Some("F2"));
    }

    #[test]
    fn the_active_open_tab_lists_every_group_with_its_keys() {
        let entries = row_actions(subject(TreeId::Tab(tab_id(200)), &[]), true, &keys());
        assert_eq!(
            shown(&entries),
            [
                ("Reload tab", Some("Ctrl R")),
                ("Duplicate tab", None),
                ("Mute tab", None),
                ("Copy link", None),
                ("—", None),
                ("Save page", Some("Ctrl D")),
                ("—", None),
                ("Close tab", Some("Ctrl W, Delete")),
                ("Close other tabs", None),
                ("Close tabs below", None),
                ("—", None),
                ("Reopen closed tab", Some("Ctrl Shift T")),
            ]
        );
    }

    #[test]
    fn an_inactive_tab_shows_no_ctrl_r_or_ctrl_d_and_the_last_has_no_close_below() {
        let entries = row_actions(subject(TreeId::Tab(tab_id(400)), &[]), true, &keys());
        let shown = shown(&entries);
        assert!(shown.contains(&("Reload tab", None)), "{shown:?}");
        assert!(shown.contains(&("Save page", None)), "{shown:?}");
        assert!(shown.contains(&("Close tab", Some("Ctrl W, Delete"))));
        assert!(shown.contains(&("Close other tabs", None)));
        assert!(!shown.iter().any(|(label, _)| *label == "Close tabs below"));
    }

    #[test]
    fn a_muted_tab_offers_unmute() {
        let entries = row_actions(subject(TreeId::Tab(tab_id(400)), &[400]), true, &keys());
        assert!(entries.contains(&RowEntry::Action {
            action: TabAction::SetMuted(tab_id(400), false),
            keys: None,
        }));
        assert!(!shown(&entries).contains(&("Mute tab", None)));
    }

    #[test]
    fn home_offers_only_closing_and_reopening() {
        let entries = row_actions(subject(TreeId::Tab(tab_id(300)), &[]), true, &keys());
        assert_eq!(
            shown(&entries),
            [
                ("Close tab", Some("Ctrl W, Delete")),
                ("Close other tabs", None),
                ("Close tabs below", None),
                ("—", None),
                ("Reopen closed tab", Some("Ctrl Shift T")),
            ]
        );
    }

    #[test]
    fn a_live_saved_page_unloads_and_removes_but_never_closes_others() {
        for wide in [true, false] {
            let entries = row_actions(subject(TreeId::Page(page_id(1)), &[]), wide, &keys());
            assert_eq!(
                shown(&entries),
                [
                    ("Unload page", Some("Ctrl W")),
                    ("Remove saved page", Some("Delete")),
                    ("—", None),
                    ("Reload tab", None),
                    ("Duplicate tab", None),
                    ("Mute tab", None),
                    ("Copy link", None),
                    ("—", None),
                    ("Reopen closed tab", Some("Ctrl Shift T")),
                ],
                "wide {wide}"
            );
        }
    }

    #[test]
    fn an_unopened_saved_page_is_removed_only_from_the_wide_sidebar() {
        let row = TreeId::Page(page_id(2));
        let wide = row_actions(subject(row, &[]), true, &keys());
        assert_eq!(
            shown(&wide),
            [
                ("Copy link", None),
                ("—", None),
                ("Remove saved page", Some("Delete")),
            ]
        );
        let compact = row_actions(subject(row, &[]), false, &keys());
        assert_eq!(shown(&compact), [("Copy link", None)]);
    }

    #[test]
    fn a_folder_is_renamed_and_removed_only_from_the_wide_sidebar() {
        let row = TreeId::Folder(folder_id(10));
        let wide = row_actions(subject(row, &[]), true, &keys());
        assert_eq!(
            shown(&wide),
            [
                ("Rename folder", Some("F2")),
                ("Remove folder", Some("Delete"))
            ]
        );
        assert!(row_actions(subject(row, &[]), false, &keys()).is_empty());
    }

    #[test]
    fn compact_open_tabs_keep_every_item() {
        let row = TreeId::Tab(tab_id(200));
        assert_eq!(
            row_actions(subject(row, &[]), false, &keys()),
            row_actions(subject(row, &[]), true, &keys())
        );
    }

    #[test]
    fn a_row_gone_from_the_model_has_no_subject() {
        let none = HashSet::new();
        for row in [
            TreeId::Tab(tab_id(999)),
            TreeId::Page(page_id(99)),
            TreeId::Folder(folder_id(99)),
        ] {
            assert_eq!(row_subject(&model(), row, &none, &none), None, "{row:?}");
        }
    }

    #[test]
    fn an_adopted_popups_blank_page_is_a_web_page_not_home() {
        let popups = HashSet::from([tab_id(300)]);
        let subject = row_subject(&model(), TreeId::Tab(tab_id(300)), &HashSet::new(), &popups);
        let Some(RowSubject::OpenTab { page, .. }) = subject else {
            panic!("an open tab: {subject:?}");
        };
        assert!(page.web);
    }

    /// A live saved page, then ordinary tabs a, b and c; returns the model and a, b.
    fn bulk_model() -> (TabModel, TabId, TabId) {
        let mut tabs = TabModel::new();
        let mut open = |address: &str| {
            tabs.open(
                TabKind::Web,
                metadata(address),
                syrinx_core::Activation::Foreground,
            )
            .expect("synthetic tab opens")
        };
        let saved = open("https://saved.example.test/");
        let first = open("https://a.example.test/");
        let middle = open("https://b.example.test/");
        open("https://c.example.test/");
        tabs.save_page(saved).expect("synthetic page saves");
        (tabs, first, middle)
    }

    #[test]
    fn bulk_counts_spare_saved_pages_and_match_the_tab_model() {
        let (mut tabs, first, middle) = bulk_model();
        let snapshot = tabs.snapshot();
        assert_eq!(bulk_close_count(&snapshot, BulkKind::Others, middle), 2);
        assert_eq!(bulk_close_count(&snapshot, BulkKind::Below, first), 2);
        assert_eq!(bulk_close_count(&snapshot, BulkKind::Below, middle), 1);
        assert_eq!(
            bulk_close_count(&snapshot, BulkKind::Others, tab_id(999)),
            0
        );
        // The model closes exactly the tabs counted.
        assert_eq!(tabs.close_below(first).expect("close below").len(), 2);
        let (mut tabs, _, middle) = bulk_model();
        assert_eq!(tabs.close_others(middle).expect("close others").len(), 2);
    }

    #[test]
    fn a_bulk_close_asks_unless_told_not_to_and_never_for_nothing() {
        assert_eq!(bulk_plan(true, 3), BulkPlan::Ask);
        assert_eq!(bulk_plan(false, 3), BulkPlan::Close);
        assert_eq!(bulk_plan(true, 0), BulkPlan::Nothing);
        assert_eq!(bulk_plan(false, 0), BulkPlan::Nothing);
    }

    #[test]
    fn dont_ask_again_is_stored_and_read_back() {
        let stored = |ask| BTreeMap::from([bulk_close_setting(ask)]);
        assert!(!asks_before_bulk_close(&stored(false)));
        assert!(asks_before_bulk_close(&stored(true)));
        assert!(asks_before_bulk_close(&BTreeMap::new()), "asks by default");
        let unreadable = BTreeMap::from([(ASK_BULK_CLOSE_KEY.to_owned(), "maybe".to_owned())]);
        assert!(asks_before_bulk_close(&unreadable));
    }

    #[test]
    fn the_confirmation_names_how_many_tabs_close() {
        assert_eq!(bulk_question(BulkKind::Others, 3), "Close 3 other tabs?");
        assert_eq!(bulk_question(BulkKind::Below, 1), "Close 1 tab below?");
        assert_eq!(bulk_confirm_label(1), "Close 1 tab");
        assert_eq!(bulk_confirm_label(4), "Close 4 tabs");
    }
}
