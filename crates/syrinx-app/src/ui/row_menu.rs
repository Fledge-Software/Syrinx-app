//! The sidebar row's actions (#95, #58): what its right-click menu lists, and the one place
//! that runs them for the menu, the row's keys and its buttons.

use std::collections::{BTreeMap, HashSet};

use gpui::{ClipboardItem, Context, Pixels, Point, Window};
use syrinx_core::{
    BrowserAction, BulkScope, CommandAction, CommandRegistry, PageOrigin, SavedFolderId,
    SavedPageId, Shortcut, TabId, TabModelSnapshot,
};
use syrinx_platform::browser::BrowserCommand;

use super::{
    BrowserView,
    folder_editor::{FolderEditor, REMOVE_ROW_KEY, RENAME_ROW_KEY},
    menus::{Corner, MenuAnchor, MenuOpening, MenuReturn},
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
    pub(super) kind: BulkScope,
    pub(super) tab: TabId,
}

impl BulkClose {
    /// The tabs it would close now, read by the same rule the tab model closes them by.
    pub(super) fn targets(self, model: &TabModelSnapshot) -> Vec<TabId> {
        model.bulk_close_targets(self.kind, self.tab)
    }
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
                kind: BulkScope::Others,
                ..
            }) => "Close other tabs",
            TabAction::Bulk(BulkClose {
                kind: BulkScope::Below,
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

/// Where a row's menu is shown: `wide` is false in the favicon-only sidebar, and `keyed` is
/// true when Shift F10 or the Menu key opened it on the focused row, so the row's own keys
/// (Ctrl W, Delete, F2) act on it once the menu closes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct RowPlace {
    pub(super) wide: bool,
    pub(super) keyed: bool,
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
/// tab, Ctrl W on the active tab or the focused row, and Delete and F2 on the focused row
/// only. In the favicon-only sidebar, renaming and removals that need the wide sidebar's
/// card are left out.
pub(super) fn row_actions(subject: RowSubject, place: RowPlace, keys: &RowKeys) -> Vec<RowEntry> {
    let row_key = |label: &Option<String>| label.clone().filter(|_| place.keyed);
    let groups = match subject {
        RowSubject::OpenTab {
            tab,
            page,
            others,
            below,
        } => open_tab_groups(tab, page, (others, below), place, keys),
        RowSubject::LiveSaved { saved, tab, page } => vec![
            vec![
                (TabAction::Unload(tab), close_key(page, place, keys)),
                (TabAction::RemoveSaved(saved), row_key(&keys.remove)),
            ],
            page_items(tab, page, keys),
            vec![(TabAction::ReopenClosed, keys.reopen.clone())],
        ],
        RowSubject::UnopenedSaved(saved) => vec![
            vec![(TabAction::CopyLink(LinkSource::Saved(saved)), None)],
            // Removing an unopened page asks first, on the wide sidebar's card.
            when(
                place.wide,
                (TabAction::RemoveSaved(saved), row_key(&keys.remove)),
            ),
        ],
        RowSubject::Folder(folder) if place.wide => vec![vec![
            (TabAction::RenameFolder(folder), row_key(&keys.rename)),
            (TabAction::RemoveFolder(folder), row_key(&keys.remove)),
        ]],
        RowSubject::Folder(_) => Vec::new(),
    };
    join_groups(groups)
}

/// Ctrl W closes the active tab, or the focused row when a key opened the menu on it.
fn close_key(page: PageFacts, place: RowPlace, keys: &RowKeys) -> Option<String> {
    keys.close.clone().filter(|_| place.keyed || page.active)
}

/// An open tab's groups: its page, saving, closing, and reopening.
fn open_tab_groups(
    tab: TabId,
    page: PageFacts,
    (others, below): (usize, usize),
    place: RowPlace,
    keys: &RowKeys,
) -> Vec<Vec<Item>> {
    // Delete removes the focused row only, so it joins Ctrl W only on a keyed menu.
    let delete = keys.remove.as_deref().filter(|_| place.keyed);
    let close_keys = joined(&[close_key(page, place, keys).as_deref(), delete]);
    let save_keys = keys.save.clone().filter(|_| page.active);
    let mut closing = vec![(TabAction::Close(tab), close_keys)];
    // A bulk close that would close nothing is left out, as Firefox greys it out.
    closing.extend(when(others > 0, bulk(BulkScope::Others, tab)));
    closing.extend(when(below > 0, bulk(BulkScope::Below, tab)));
    vec![
        page_items(tab, page, keys),
        when(page.web, (TabAction::Save(tab), save_keys)),
        closing,
        vec![(TabAction::ReopenClosed, keys.reopen.clone())],
    ]
}

/// Reload, Mute, Duplicate and Copy link, in Firefox's order, for a web page only.
fn page_items(tab: TabId, page: PageFacts, keys: &RowKeys) -> Vec<Item> {
    if !page.web {
        return Vec::new();
    }
    let reload_keys = keys.reload.clone().filter(|_| page.active);
    vec![
        (TabAction::Reload(tab), reload_keys),
        (TabAction::SetMuted(tab, !page.muted), None),
        (TabAction::Duplicate(tab), None),
        (TabAction::CopyLink(LinkSource::Tab(tab)), None),
    ]
}

fn bulk(kind: BulkScope, tab: TabId) -> Item {
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
            others: model.bulk_close_targets(BulkScope::Others, tab).len(),
            below: model.bulk_close_targets(BulkScope::Below, tab).len(),
        }),
    }
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

/// What a confirmed bulk close does once its tabs are read again.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Recount {
    /// The tabs the question named are still exactly the ones that would close.
    Close,
    /// Tabs opened, closed or moved meanwhile; ask again about these.
    AskAgain(Vec<TabId>),
    Nothing,
}

/// Closes only the tabs the question named: any other set, even of the same size, asks again.
pub(super) fn recount(asked: &[TabId], now: Vec<TabId>) -> Recount {
    if now.is_empty() {
        Recount::Nothing
    } else if now == asked {
        Recount::Close
    } else {
        Recount::AskAgain(now)
    }
}

/// The status line when a bulk close finds nothing left to close.
const NOTHING_TO_CLOSE: &str = "No other tabs would close.";

/// The confirmation's question, such as "Close 3 other tabs?".
pub(super) fn bulk_question(kind: BulkScope, count: usize) -> String {
    let tabs = tabs_phrase(count);
    match kind {
        BulkScope::Others => format!("Close {count} other {tabs}?"),
        BulkScope::Below => format!("Close {count} {tabs} below?"),
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

/// The menu an action came from: where it hung, so a confirmation can open in its place,
/// and what it was opened on.
#[derive(Clone)]
pub(super) struct MenuOrigin {
    pub(super) anchor: MenuAnchor,
    pub(super) returns: MenuReturn,
    /// The tab active when the menu opened; the ⋯ menu's Close tab closes this one.
    pub(super) active: Option<TabId>,
    /// The tabs a bulk-close question named; empty for every other menu.
    pub(super) asked: Vec<TabId>,
}

impl BrowserView {
    /// The row's menu entries now, or none when the row is gone.
    pub(super) fn row_entries(&self, row: TreeId, place: RowPlace) -> Vec<RowEntry> {
        let Some(subject) = row_subject(&self.model, row, &self.muted, &self.popup_pages) else {
            return Vec::new();
        };
        row_actions(subject, place, &RowKeys::from_bindings(&self.commands))
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

    /// Copies the row's address. Home and Settings rows never offer it.
    fn copy_link(&mut self, source: LinkSource, cx: &mut Context<Self>) {
        self.message = if copy_row_link(&self.model, source, cx) {
            "Link copied.".to_owned()
        } else {
            "That row has no link to copy.".to_owned()
        };
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
        let targets = bulk.targets(&self.model);
        match bulk_plan(self.ask_before_bulk_close, targets.len()) {
            BulkPlan::Nothing => {
                self.message = NOTHING_TO_CLOSE.to_owned();
                false
            }
            BulkPlan::Close => {
                self.close_in_bulk(bulk);
                false
            }
            BulkPlan::Ask => {
                let Some(menu) = menu else {
                    // Only a menu offers a bulk close; say so rather than do nothing.
                    self.message = "Close several tabs from a tab's menu.".to_owned();
                    return false;
                };
                self.show_bulk_confirmation(bulk, targets, menu, window, cx);
                true
            }
        }
    }

    /// Closes the tabs the confirmation named (`origin.asked`); with `remember`, it never asks
    /// again. Tabs opened, closed or moved while it was asking make it ask again about the
    /// tabs that would close now.
    pub(super) fn confirm_bulk_close(
        &mut self,
        bulk: BulkClose,
        remember: bool,
        origin: MenuOrigin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ActionFocus {
        match recount(&origin.asked, bulk.targets(&self.model)) {
            Recount::Nothing => {
                self.message = NOTHING_TO_CLOSE.to_owned();
                ActionFocus::Unplaced
            }
            Recount::AskAgain(targets) => {
                self.show_bulk_confirmation(bulk, targets, origin, window, cx);
                ActionFocus::Placed
            }
            Recount::Close => {
                if remember {
                    self.set_ask_before_bulk_close(false);
                }
                self.close_in_bulk(bulk);
                ActionFocus::Unplaced
            }
        }
    }

    fn close_in_bulk(&mut self, bulk: BulkClose) {
        self.command(match bulk.kind {
            BulkScope::Others => SessionCommand::CloseOthers(bulk.tab),
            BulkScope::Below => SessionCommand::CloseBelow(bulk.tab),
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
            asked: Vec::new(),
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
            asked: Vec::new(),
        };
        self.show_row_menu(row, opening, window, cx);
    }
}

/// The row's own address as Copy link writes it, or `None` for a row without one.
fn row_link(model: &TabModelSnapshot, source: LinkSource) -> Option<ClipboardItem> {
    let address = match source {
        LinkSource::Tab(tab) => model
            .tabs
            .iter()
            .find(|candidate| candidate.id == tab)
            .map(|candidate| &candidate.metadata.address),
        LinkSource::Saved(saved) => model
            .saved_pages
            .iter()
            .find(|page| page.id == saved)
            .map(|page| &page.metadata.address),
    }?;
    (!address.is_empty()).then(|| ClipboardItem::new_string(address.clone()))
}

/// Copy link's one clipboard write, of `row_link` only; returns whether there was one.
fn copy_row_link(
    model: &TabModelSnapshot,
    source: LinkSource,
    cx: &mut Context<BrowserView>,
) -> bool {
    let Some(link) = row_link(model, source) else {
        return false;
    };
    cx.write_to_clipboard(link);
    true
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

    const WIDE: RowPlace = RowPlace {
        wide: true,
        keyed: false,
    };
    const WIDE_KEYED: RowPlace = RowPlace {
        wide: true,
        keyed: true,
    };
    const COMPACT: RowPlace = RowPlace {
        wide: false,
        keyed: false,
    };

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
        let entries = row_actions(subject(TreeId::Tab(tab_id(200)), &[]), WIDE_KEYED, &keys());
        assert_eq!(
            shown(&entries),
            [
                ("Reload tab", Some("Ctrl R")),
                ("Mute tab", None),
                ("Duplicate tab", None),
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
        let entries = row_actions(subject(TreeId::Tab(tab_id(400)), &[]), WIDE_KEYED, &keys());
        let shown = shown(&entries);
        assert!(shown.contains(&("Reload tab", None)), "{shown:?}");
        assert!(shown.contains(&("Save page", None)), "{shown:?}");
        assert!(shown.contains(&("Close tab", Some("Ctrl W, Delete"))));
        assert!(shown.contains(&("Close other tabs", None)));
        assert!(!shown.iter().any(|(label, _)| *label == "Close tabs below"));
    }

    #[test]
    fn close_keys_show_only_where_they_would_close_this_row() {
        let close = |row: u64, place| {
            let entries = row_actions(subject(TreeId::Tab(tab_id(row)), &[]), place, &keys());
            shown(&entries)
                .into_iter()
                .find(|(label, _)| *label == "Close tab")
                .and_then(|(_, keys)| keys.map(str::to_owned))
        };
        // A click on an inactive row leaves the keys elsewhere, so Ctrl W would close the
        // active tab and Delete would reach the page.
        assert_eq!(close(400, WIDE), None);
        assert_eq!(
            close(200, WIDE).as_deref(),
            Some("Ctrl W"),
            "the active tab"
        );
        assert_eq!(close(400, WIDE_KEYED).as_deref(), Some("Ctrl W, Delete"));
        let saved = row_actions(subject(TreeId::Page(page_id(1)), &[]), WIDE, &keys());
        assert!(shown(&saved).contains(&("Unload page", None)));
        assert!(shown(&saved).contains(&("Remove saved page", None)));
        let folder = row_actions(subject(TreeId::Folder(folder_id(10)), &[]), WIDE, &keys());
        assert_eq!(
            shown(&folder),
            [("Rename folder", None), ("Remove folder", None)]
        );
    }

    #[test]
    fn a_muted_tab_offers_unmute() {
        let entries = row_actions(subject(TreeId::Tab(tab_id(400)), &[400]), WIDE, &keys());
        assert!(entries.contains(&RowEntry::Action {
            action: TabAction::SetMuted(tab_id(400), false),
            keys: None,
        }));
        assert!(!shown(&entries).contains(&("Mute tab", None)));
    }

    #[test]
    fn home_offers_only_closing_and_reopening() {
        let entries = row_actions(subject(TreeId::Tab(tab_id(300)), &[]), WIDE_KEYED, &keys());
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
            let place = RowPlace { wide, keyed: true };
            let entries = row_actions(subject(TreeId::Page(page_id(1)), &[]), place, &keys());
            assert_eq!(
                shown(&entries),
                [
                    ("Unload page", Some("Ctrl W")),
                    ("Remove saved page", Some("Delete")),
                    ("—", None),
                    ("Reload tab", None),
                    ("Mute tab", None),
                    ("Duplicate tab", None),
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
        let wide = row_actions(subject(row, &[]), WIDE_KEYED, &keys());
        assert_eq!(
            shown(&wide),
            [
                ("Copy link", None),
                ("—", None),
                ("Remove saved page", Some("Delete")),
            ]
        );
        let compact = row_actions(subject(row, &[]), COMPACT, &keys());
        assert_eq!(shown(&compact), [("Copy link", None)]);
    }

    #[test]
    fn a_folder_is_renamed_and_removed_only_from_the_wide_sidebar() {
        let row = TreeId::Folder(folder_id(10));
        let wide = row_actions(subject(row, &[]), WIDE_KEYED, &keys());
        assert_eq!(
            shown(&wide),
            [
                ("Rename folder", Some("F2")),
                ("Remove folder", Some("Delete"))
            ]
        );
        assert!(row_actions(subject(row, &[]), COMPACT, &keys()).is_empty());
    }

    #[test]
    fn compact_open_tabs_keep_every_item() {
        let row = TreeId::Tab(tab_id(200));
        assert_eq!(
            row_actions(subject(row, &[]), COMPACT, &keys()),
            row_actions(subject(row, &[]), WIDE, &keys())
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
    fn a_rows_bulk_counts_spare_saved_pages_as_the_tab_model_does() {
        let (tabs, first, middle) = bulk_model();
        let snapshot = tabs.snapshot();
        let none = HashSet::new();
        let counts = |tab| match row_subject(&snapshot, TreeId::Tab(tab), &none, &none) {
            Some(RowSubject::OpenTab { others, below, .. }) => (others, below),
            other => panic!("an open tab: {other:?}"),
        };
        assert_eq!(counts(first), (2, 2));
        assert_eq!(counts(middle), (2, 1));
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
    fn a_confirmation_closes_only_the_tabs_it_named() {
        let [a, b, c, d] = [1, 2, 3, 4].map(tab_id);
        assert_eq!(recount(&[a, b, c], vec![a, b, c]), Recount::Close);
        assert_eq!(
            recount(&[a, b, c], vec![a, b, c, d]),
            Recount::AskAgain(vec![a, b, c, d])
        );
        assert_eq!(recount(&[a, b, c], vec![a]), Recount::AskAgain(vec![a]));
        assert_eq!(recount(&[a, b, c], Vec::new()), Recount::Nothing);
        // One tab closed and another opened: the same count, but not the tabs it named.
        assert_eq!(
            recount(&[a, b, c], vec![a, b, d]),
            Recount::AskAgain(vec![a, b, d])
        );
    }

    #[test]
    fn copy_link_writes_exactly_the_rows_own_address() {
        let text = |source| row_link(&model(), source).and_then(|item| item.text());
        assert_eq!(
            text(LinkSource::Tab(tab_id(200))).as_deref(),
            Some("https://two.example.test/")
        );
        assert_eq!(
            text(LinkSource::Saved(page_id(2))).as_deref(),
            Some("https://saved.example.test/")
        );
        assert_eq!(text(LinkSource::Tab(tab_id(999))), None, "a row gone");
        let mut blank = model();
        blank.tabs[1].metadata.address.clear();
        assert!(row_link(&blank, LinkSource::Tab(tab_id(200))).is_none());
    }

    #[test]
    fn the_confirmation_names_how_many_tabs_close() {
        assert_eq!(bulk_question(BulkScope::Others, 3), "Close 3 other tabs?");
        assert_eq!(bulk_question(BulkScope::Below, 1), "Close 1 tab below?");
        assert_eq!(bulk_confirm_label(1), "Close 1 tab");
        assert_eq!(bulk_confirm_label(4), "Close 4 tabs");
    }
}
