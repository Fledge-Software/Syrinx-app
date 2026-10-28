//! Builds Back and Forward's menu of the tab's recent pages, which hangs under its button and
//! jumps straight to a row (#126; the owner chose this design on 2026-10-28).

use syrinx_core::HistoryDirection;

use super::*;
use crate::session::HistoryListRow;
use crate::ui::back_forward::way;

/// Back and Forward's menu is wider than the others, so a title and its host share a row.
pub(super) const HISTORY_WIDTH: f32 = 320.;

impl BrowserView {
    /// Opens `direction`'s menu for `tab` with its top-left corner at `anchor`, the button's
    /// bottom-left. With no rows it says the pages are not listed and offers full history.
    pub(in crate::ui) fn show_history_menu(
        &mut self,
        (tab, direction): (TabId, HistoryDirection),
        pages: &[HistoryListRow],
        anchor: Point<Pixels>,
        select_first: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let kind = MenuKind::History(direction);
        let available =
            f32::from(window.viewport_size().height) - f32::from(anchor.y) - WINDOW_MARGIN;
        let rows = history_menu_rows(tab, direction, pages, available);
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

/// The menu's rows: as many pages as fit in `available` logical pixels, nearest first, then
/// the footer. A list reset after a wrong landing has no pages, so a note says so instead.
fn history_menu_rows(
    tab: TabId,
    direction: HistoryDirection,
    pages: &[HistoryListRow],
    available: f32,
) -> Vec<MenuRow> {
    let footer = footer_rows();
    let fitting = pages_that_fit(MenuKind::History(direction), &footer, available);
    let mut rows: Vec<MenuRow> = pages
        .iter()
        .take(fitting)
        .map(|page| page_row(tab, direction, page))
        .collect();
    if rows.is_empty() {
        rows.push(MenuRow {
            item: MenuItem::HistoryUnlisted,
            label: way(direction).unlisted.to_owned(),
            shortcut: None,
            controls: &[],
            icon: None,
        });
    }
    rows.extend(footer);
    rows
}

/// The separator and "Show full history", which every history menu ends with.
fn footer_rows() -> [MenuRow; 2] {
    [
        fixed_row(MenuItem::Separator),
        MenuRow {
            item: MenuItem::History,
            label: "Show full history".to_owned(),
            shortcut: MenuItem::History.shortcut(),
            controls: &[],
            icon: None,
        },
    ]
}

/// How many pages fit above `footer` in `available` logical pixels; the nearest always does,
/// so a very short window still offers one page. The list itself holds at most 15.
fn pages_that_fit(kind: MenuKind, footer: &[MenuRow], available: f32) -> usize {
    // The footer's height includes the card's padding and border, which no page adds.
    let footer_height = f32::from(kind_size(kind, footer).height);
    let pages = ((available - footer_height) / ITEM_HEIGHT).floor();
    if pages < 1. {
        return 1;
    }
    // Saturates at `usize::MAX` for a huge window, which `take` then ignores.
    pages as usize
}

/// One page: its icon and label on the left and its host, dimmed, where other menus show a
/// shortcut.
fn page_row(tab: TabId, direction: HistoryDirection, page: &HistoryListRow) -> MenuRow {
    let face = row_face(&page.address, &page.title);
    MenuRow {
        item: MenuItem::HistoryPage {
            tab,
            direction,
            steps: page.steps,
        },
        label: face.label,
        shortcut: face.host,
        controls: &[],
        icon: Some(face.icon),
    }
}

/// What a page row shows: its label, its host, and its placeholder icon's letter.
#[derive(Debug, Eq, PartialEq)]
struct RowFace {
    label: String,
    host: Option<String>,
    icon: String,
}

/// A page's face. The label is its title or, without one, its path, or its host when the
/// path is bare; the host drops a leading "www." and is left out when it is already the label.
/// An address that is not http(s) is shown whole, with no host. WebView2 names an untitled
/// page by its address without the scheme, which counts as no title. The icon is lettered as
/// the sidebar letters the page: by its title, or else by its host.
fn row_face(address: &str, title: &str) -> RowFace {
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
        return face(label, None);
    };
    let host = host.strip_prefix("www.").unwrap_or(host).to_owned();
    if !title.is_empty() {
        return face(title.to_owned(), Some(host));
    }
    if path.is_empty() || path == "/" {
        return face(host, None);
    }
    let icon = super::super::tiles::monogram(&host);
    RowFace {
        label: path.to_owned(),
        host: Some(host),
        icon,
    }
}

/// A face whose icon is lettered by its label.
fn face(label: String, host: Option<String>) -> RowFace {
    RowFace {
        icon: super::super::tiles::monogram(&label),
        label,
        host,
    }
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
    let distance = format!("{steps} {pages} {}", way(direction).distance);
    match &row.shortcut {
        Some(host) => format!("{}, {host}, {distance}", row.label),
        None => format!("{}, {distance}", row.label),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Room for every page the list can hold.
    const TALL: f32 = 1000.;

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

    fn shown(face: RowFace) -> (String, Option<String>) {
        (face.label, face.host)
    }

    #[test]
    fn a_row_shows_its_title_and_its_host_without_www() {
        let face = row_face("https://www.docs.example.test/guide", "Getting started");
        assert_eq!(face.icon, "G");
        assert_eq!(
            shown(face),
            (
                "Getting started".to_owned(),
                Some("docs.example.test".to_owned())
            )
        );
    }

    #[test]
    fn a_row_without_a_title_shows_its_path_or_else_its_host_once() {
        assert_eq!(
            shown(row_face("https://example.test/watch?v=1", "  ")),
            ("/watch?v=1".to_owned(), Some("example.test".to_owned()))
        );
        assert_eq!(
            shown(row_face("http://127.0.0.1:8080/", "")),
            ("127.0.0.1:8080".to_owned(), None)
        );
        assert_eq!(
            shown(row_face("about:blank", "")),
            ("about:blank".to_owned(), None)
        );
        // WebView2's own name for an untitled page is its address without the scheme.
        assert_eq!(
            shown(row_face(
                "http://127.0.0.1:8765/b.html",
                "127.0.0.1:8765/b.html"
            )),
            ("/b.html".to_owned(), Some("127.0.0.1:8765".to_owned()))
        );
    }

    #[test]
    fn a_rows_icon_follows_its_title_or_else_its_host_never_its_path() {
        // Review of #213: the icon was read back from the label, so "/b.html" gave "B".
        assert_eq!(row_face("https://example.test/b.html", "").icon, "E");
        assert_eq!(
            row_face("https://example.test/b.html", "/slash title").icon,
            "S"
        );
    }

    #[test]
    fn pages_come_nearest_first_then_full_history() {
        let pages = [
            page(1, "https://b.test/", "B"),
            page(2, "https://a.test/", "A"),
        ];
        let rows = history_menu_rows(tab(), HistoryDirection::Back, &pages, TALL);
        let labels: Vec<&str> = rows.iter().map(|row| row.label.as_str()).collect();
        assert_eq!(labels, ["B", "A", "", "Show full history"]);
        assert_eq!(rows[3].shortcut.as_deref(), Some("Ctrl H"));
        assert_eq!(rows[3].icon, None);
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
        // The owner kept the Forward-side note too (2026-10-28).
        let rows = history_menu_rows(tab(), HistoryDirection::Forward, &[], TALL);
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
        let rows = history_menu_rows(tab(), HistoryDirection::Back, &pages, TALL);
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
        let rows = |available| history_menu_rows(tab(), HistoryDirection::Back, &pages, available);
        assert_eq!(rows(TALL).len(), 17);
        // Five pages, the separator and full history: 6 × 30 + 9 + 10 = 199.
        let fitted = rows(199.);
        assert_eq!(fitted.len(), 7);
        assert!(matches!(
            fitted[4].item,
            MenuItem::HistoryPage { steps: 5, .. }
        ));
        assert_eq!(fitted[6].label, "Show full history");
        // However short the window, the nearest page stays.
        assert_eq!(rows(10.).len(), 3);
    }

    #[test]
    fn the_menu_is_named_for_its_direction_and_wider_than_the_others() {
        let back = MenuKind::History(HistoryDirection::Back);
        assert_eq!(menu_name(back, &[]), "Back history");
        assert_eq!(
            menu_name(MenuKind::History(HistoryDirection::Forward), &[]),
            "Forward history"
        );
        assert!(back.width() > MenuKind::Page.width());
    }
}
