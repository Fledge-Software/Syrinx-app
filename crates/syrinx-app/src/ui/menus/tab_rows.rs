//! Builds a sidebar row's menu and the bulk-close confirmation that takes its place, and
//! runs their items (#95).

use crate::ui::row_menu::{
    ActionFocus, BulkClose, MenuOrigin, RowEntry, RowPlace, bulk_confirm_label, bulk_question,
};
use super::*;

impl BrowserView {
    /// Opens a sidebar row's menu, or the ⋯ menu when the row offers nothing here (a folder
    /// in the favicon-only sidebar). A row gone from the model opens nothing.
    pub(in crate::ui) fn show_row_menu(
        &mut self,
        row: TreeId,
        opening: MenuOpening,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let place = RowPlace {
            wide: self.sidebar_sizing.labels_visible(),
            keyed: opening.returns.keyed(),
        };
        let rows = row_menu_rows(self.row_entries(row, place));
        if rows.is_empty() {
            let rows = self.menu_rows(MenuKind::Sidebar);
            self.show_menu_rows(MenuKind::Sidebar, rows, opening, window, cx);
            return;
        }
        self.show_menu_rows(MenuKind::Row, rows, opening, window, cx);
    }

    /// Asks before closing `targets`, in the place of the menu the bulk close was chosen
    /// from; Close closes them only if they are still exactly the tabs that would close.
    pub(in crate::ui) fn show_bulk_confirmation(
        &mut self,
        bulk: BulkClose,
        targets: Vec<TabId>,
        origin: MenuOrigin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rows = confirm_rows(bulk, targets.len());
        // A question reached by keys starts on its first choice, as the row menu did.
        let opening = MenuOpening {
            anchor: origin.anchor,
            select_first: origin.returns.keyed(),
            returns: origin.returns,
            asked: targets,
        };
        self.show_menu_rows(MenuKind::Confirm, rows, opening, window, cx);
    }

    /// Runs a row menu's or confirmation's item; the keys go back where the menu found them
    /// unless the item placed them.
    pub(super) fn run_row_item(
        &mut self,
        item: MenuItem,
        origin: MenuOrigin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let returns = origin.returns.clone();
        let focus = match item {
            MenuItem::Tab(action) => self.run_tab_action(action, Some(origin), window, cx),
            MenuItem::ConfirmBulk { bulk, remember } => {
                self.confirm_bulk_close(bulk, remember, origin, window, cx)
            }
            _ => ActionFocus::Unplaced,
        };
        if focus == ActionFocus::Unplaced {
            self.return_menu_focus(returns, window, cx);
        }
    }
}

/// A row's menu entries as menu rows.
fn row_menu_rows(entries: Vec<RowEntry>) -> Vec<MenuRow> {
    entries
        .into_iter()
        .map(|entry| match entry {
            RowEntry::Action { action, keys } => MenuRow {
                item: MenuItem::Tab(action),
                label: action.label().to_owned(),
                shortcut: keys,
                controls: &[],
            },
            RowEntry::Separator => fixed_row(MenuItem::Separator),
        })
        .collect()
}

/// The confirmation: its question, then Close, Close and don't ask again, and Cancel.
fn confirm_rows(bulk: BulkClose, count: usize) -> Vec<MenuRow> {
    let labelled = |item: MenuItem, label: String| MenuRow {
        item,
        label,
        shortcut: None,
        controls: &[],
    };
    vec![
        labelled(MenuItem::ConfirmPrompt, bulk_question(bulk.kind, count)),
        fixed_row(MenuItem::Separator),
        labelled(
            MenuItem::ConfirmBulk {
                bulk,
                remember: false,
            },
            bulk_confirm_label(count),
        ),
        fixed_row(MenuItem::ConfirmBulk {
            bulk,
            remember: true,
        }),
        fixed_row(MenuItem::Cancel),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A row menu's shape: an item, a separator, two items, a separator and an item.
    fn grouped_rows() -> Vec<MenuRow> {
        let tab = syrinx_core::TabId::new(5).expect("synthetic tab id");
        row_menu_rows(vec![
            RowEntry::Action {
                action: TabAction::Reload(tab),
                keys: None,
            },
            RowEntry::Separator,
            RowEntry::Action {
                action: TabAction::Close(tab),
                keys: Some("Ctrl W, Delete".to_owned()),
            },
            RowEntry::Action {
                action: TabAction::Duplicate(tab),
                keys: None,
            },
            RowEntry::Separator,
            RowEntry::Action {
                action: TabAction::ReopenClosed,
                keys: None,
            },
        ])
    }

    #[test]
    fn arrows_skip_separators_in_both_directions_and_wrap() {
        let rows = grouped_rows();
        let step = |step, from| step_selection(step, from, &rows);
        assert_eq!(step(MenuStep::Next, None), Some(0));
        assert_eq!(step(MenuStep::Next, Some(0)), Some(2));
        assert_eq!(step(MenuStep::Previous, Some(2)), Some(0));
        assert_eq!(step(MenuStep::Next, Some(3)), Some(5));
        assert_eq!(step(MenuStep::Next, Some(5)), Some(0));
        assert_eq!(step(MenuStep::Previous, Some(0)), Some(5));
        assert_eq!(step(MenuStep::Last, Some(2)), Some(5));
        assert_eq!(first_selectable(&rows), Some(0));
        assert_eq!(rows[2].shortcut.as_deref(), Some("Ctrl W, Delete"));
    }

    #[test]
    fn the_confirmation_names_the_count_and_its_question_cannot_be_chosen() {
        let bulk = BulkClose {
            kind: syrinx_core::BulkScope::Others,
            tab: syrinx_core::TabId::new(5).expect("synthetic tab id"),
        };
        let rows = confirm_rows(bulk, 3);
        let labels: Vec<&str> = rows.iter().map(|row| row.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Close 3 other tabs?",
                "",
                "Close 3 tabs",
                "Close and don't ask again",
                "Cancel"
            ]
        );
        assert!(!rows[0].item.is_selectable());
        // Arrows start on Close, never on the question.
        assert_eq!(step_selection(MenuStep::Next, None, &rows), Some(2));
        assert_eq!(step_selection(MenuStep::First, None, &rows), Some(2));
        assert!(matches!(
            key_action("enter", Some(rows[3].item)),
            Some(MenuAction::Choose(MenuItem::ConfirmBulk {
                remember: true,
                ..
            }))
        ));
        assert!(key_action("enter", Some(rows[0].item)).is_none());
        assert_eq!(menu_name(MenuKind::Confirm, &rows), "Close 3 other tabs?");
        assert_eq!(menu_name(MenuKind::Row, &[]), "Tab menu");
    }
}
