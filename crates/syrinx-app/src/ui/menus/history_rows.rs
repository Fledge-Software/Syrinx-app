//! Builds Back and Forward's menu of the tab's recent pages, which hangs under its button and
//! jumps straight to a row (#126; the owner chose this design on 2026-10-28).

use syrinx_core::HistoryDirection;

use super::*;
use crate::session::HistoryListRow;

/// Back and Forward's menu is wider than the others, so a title and its host share a row.
pub(super) const HISTORY_WIDTH: f32 = 320.;

impl BrowserView {
    /// Opens `direction`'s menu for `tab` with its top-left corner at `anchor`, the button's
    /// bottom-left. With no rows it says the pages are not listed and offers full history.
    pub(in crate::ui) fn show_history_menu(
        &mut self,
        (tab, direction): (TabId, HistoryDirection),
        rows: &[HistoryListRow],
        anchor: Point<Pixels>,
        select_first: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let kind = MenuKind::History(direction);
        let available =
            f32::from(window.viewport_size().height) - f32::from(anchor.y) - WINDOW_MARGIN;
        let rows = fit_rows(kind, history_menu_rows(tab, direction, rows), available);
        let opening = MenuOpening {
            anchor: MenuAnchor {
                position: anchor,
                corner: Corner::TopLeft,
            },
            returns: MenuReturn::Page,
            select_first,
            asked: Vec::new(),
        };
        self.show_menu_rows(kind, rows, opening, window, cx);
    }
}

/// The menu's rows: the pages nearest first, then "Show full history". A list reset after a
/// wrong landing has no pages, so a note says so instead.
fn history_menu_rows(
    tab: TabId,
    direction: HistoryDirection,
    pages: &[HistoryListRow],
) -> Vec<MenuRow> {
    let mut rows: Vec<MenuRow> = pages
        .iter()
        .map(|page| page_row(tab, direction, page))
        .collect();
    if rows.is_empty() {
        rows.push(MenuRow {
            item: MenuItem::HistoryUnlisted,
            label: unlisted_note(direction).to_owned(),
            shortcut: None,
            controls: &[],
        });
    }
    rows.push(fixed_row(MenuItem::Separator));
    rows.push(MenuRow {
        item: MenuItem::History,
        label: "Show full history".to_owned(),
        shortcut: MenuItem::History.shortcut(),
        controls: &[],
    });
    rows
}

/// One page: its label on the left and its host, dimmed, where other menus show a shortcut.
fn page_row(tab: TabId, direction: HistoryDirection, page: &HistoryListRow) -> MenuRow {
    let (label, host) = row_face(&page.address, &page.title);
    MenuRow {
        item: MenuItem::HistoryPage {
            tab,
            direction,
            steps: page.steps,
        },
        label,
        shortcut: host,
        controls: &[],
    }
}

/// What the note says when a direction has pages the list could not keep.
fn unlisted_note(direction: HistoryDirection) -> &'static str {
    match direction {
        HistoryDirection::Back => "Earlier pages aren't listed",
        HistoryDirection::Forward => "Later pages aren't listed",
    }
}

/// A page's label and host. The label is its title or, without one, its path, or its host
/// when the path is bare; the host drops a leading "www." and is left out when it is already
/// the label. An address that is not http(s) is shown whole, with no host. WebView2 names an
/// untitled page by its address without the scheme, which counts as no title.
fn row_face(address: &str, title: &str) -> (String, Option<String>) {
    let readable = syrinx_core::readable_address(address);
    let bare = readable
        .split_once("://")
        .filter(|(scheme, _)| matches!(*scheme, "http" | "https"))
        .map(|(_, rest)| rest);
    let title = Some(title.trim())
        .filter(|title| Some(*title) != bare && *title != readable)
        .unwrap_or_default();
    let parts = bare.map(|rest| rest.split_at(rest.find('/').unwrap_or(rest.len())));
    let Some((host, path)) = parts else {
        let label = if title.is_empty() {
            readable
        } else {
            title.to_owned()
        };
        return (label, None);
    };
    let host = host.strip_prefix("www.").unwrap_or(host).to_owned();
    if !title.is_empty() {
        return (title.to_owned(), Some(host));
    }
    if path.is_empty() || path == "/" {
        return (host, None);
    }
    (path.to_owned(), Some(host))
}

/// What a screen reader hears for a page row, such as "Getting started, docs.example.test,
/// 3 pages back".
pub(super) fn spoken_page(row: &MenuRow) -> String {
    let MenuItem::HistoryPage {
        direction, steps, ..
    } = row.item
    else {
        return row.label.clone();
    };
    let pages = if steps == 1 { "page" } else { "pages" };
    let way = match direction {
        HistoryDirection::Back => "back",
        HistoryDirection::Forward => "forward",
    };
    let distance = format!("{steps} {pages} {way}");
    match &row.shortcut {
        Some(host) => format!("{}, {host}, {distance}", row.label),
        None => format!("{}, {distance}", row.label),
    }
}

/// Drops the pages furthest away until the menu fits `available` logical pixels of height.
/// The nearest page, the separator and "Show full history" always stay.
fn fit_rows(kind: MenuKind, mut rows: Vec<MenuRow>, available: f32) -> Vec<MenuRow> {
    // The pages come first; the separator and full-history row are the last two.
    while rows.len() > 3 && f32::from(kind_size(kind, &rows).height) > available {
        rows.remove(rows.len() - 3);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab() -> TabId {
        TabId::new(4).expect("synthetic tab id")
    }

    fn page(steps: usize, address: &str, title: &str) -> HistoryListRow {
        HistoryListRow {
            steps,
            address: address.to_owned(),
            title: title.to_owned(),
        }
    }

    #[test]
    fn a_row_shows_its_title_and_its_host_without_www() {
        assert_eq!(
            row_face("https://www.docs.example.test/guide", "Getting started"),
            (
                "Getting started".to_owned(),
                Some("docs.example.test".to_owned())
            )
        );
    }

    #[test]
    fn a_row_without_a_title_shows_its_path_or_else_its_host_once() {
        assert_eq!(
            row_face("https://example.test/watch?v=1", "  "),
            ("/watch?v=1".to_owned(), Some("example.test".to_owned()))
        );
        assert_eq!(
            row_face("http://127.0.0.1:8080/", ""),
            ("127.0.0.1:8080".to_owned(), None)
        );
        assert_eq!(
            row_face("about:blank", ""),
            ("about:blank".to_owned(), None)
        );
        // WebView2's own name for an untitled page is its address without the scheme.
        assert_eq!(
            row_face("http://127.0.0.1:8765/b.html", "127.0.0.1:8765/b.html"),
            ("/b.html".to_owned(), Some("127.0.0.1:8765".to_owned()))
        );
    }

    #[test]
    fn pages_come_nearest_first_then_full_history() {
        let pages = [
            page(1, "https://b.test/", "B"),
            page(2, "https://a.test/", "A"),
        ];
        let rows = history_menu_rows(tab(), HistoryDirection::Back, &pages);
        let labels: Vec<&str> = rows.iter().map(|row| row.label.as_str()).collect();
        assert_eq!(labels, ["B", "A", "", "Show full history"]);
        assert_eq!(rows[3].shortcut.as_deref(), Some("Ctrl H"));
        assert_eq!(
            rows[1].item,
            MenuItem::HistoryPage {
                tab: tab(),
                direction: HistoryDirection::Back,
                steps: 2
            }
        );
        assert_eq!(first_selectable(&rows), Some(0));
        assert!(matches!(
            key_action("enter", Some(rows[1].item)),
            Some(MenuAction::Choose(MenuItem::HistoryPage { steps: 2, .. }))
        ));
    }

    #[test]
    fn a_reset_list_says_its_pages_are_not_listed_and_offers_full_history() {
        let rows = history_menu_rows(tab(), HistoryDirection::Forward, &[]);
        let labels: Vec<&str> = rows.iter().map(|row| row.label.as_str()).collect();
        assert_eq!(
            labels,
            ["Later pages aren't listed", "", "Show full history"]
        );
        assert!(!rows[0].item.is_selectable());
        // Arrows and a key-opened menu start on Show full history, never on the note.
        assert_eq!(first_selectable(&rows), Some(2));
        assert_eq!(step_selection(MenuStep::Next, None, &rows), Some(2));
    }

    #[test]
    fn a_row_is_heard_with_its_host_and_its_distance() {
        let pages = [
            page(1, "https://docs.example.test/a", "Intro"),
            page(3, "https://docs.example.test/", ""),
        ];
        let rows = history_menu_rows(tab(), HistoryDirection::Back, &pages);
        assert_eq!(
            spoken_page(&rows[0]),
            "Intro, docs.example.test, 1 page back"
        );
        assert_eq!(spoken_page(&rows[1]), "docs.example.test, 3 pages back");
        assert_eq!(spoken_page(&rows[3]), "Show full history");
    }

    #[test]
    fn a_short_window_drops_the_furthest_pages_but_keeps_full_history() {
        let pages: Vec<HistoryListRow> = (1..=15)
            .map(|steps| page(steps, &format!("https://p{steps}.test/"), "Page"))
            .collect();
        let kind = MenuKind::History(HistoryDirection::Back);
        let rows = history_menu_rows(tab(), HistoryDirection::Back, &pages);
        assert_eq!(fit_rows(kind, rows.clone(), 1000.).len(), 17);
        // Five pages, the separator and full history: 6 × 30 + 9 + 10 = 199.
        let fitted = fit_rows(kind, rows.clone(), 199.);
        assert_eq!(fitted.len(), 7);
        assert!(matches!(
            fitted[4].item,
            MenuItem::HistoryPage { steps: 5, .. }
        ));
        assert_eq!(fitted[6].label, "Show full history");
        // However short the window, the nearest page stays.
        assert_eq!(fit_rows(kind, rows, 10.).len(), 3);
    }

    #[test]
    fn the_menu_is_named_for_its_direction_and_wider_than_the_others() {
        let back = MenuKind::History(HistoryDirection::Back);
        assert_eq!(menu_name(back, &[]), "Back history");
        assert_eq!(
            menu_name(MenuKind::History(HistoryDirection::Forward), &[]),
            "Forward history"
        );
        assert_eq!(back.width(), HISTORY_WIDTH);
    }
}
