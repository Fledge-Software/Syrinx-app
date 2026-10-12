//! Shows recent visits in the page card and opens one in a new tab (P1-20, Ctrl H).

use atlas_store::HistoryVisit;
use gpui::{
    Context, Div, InteractiveElement, ParentElement, StatefulInteractiveElement, Styled, div, px,
};

use super::{BrowserView, button};
use crate::{
    session::{self, SessionCommand, SessionError},
    theme,
};

/// How many visits the list reads; older history stays searchable through the palette.
const RECENT_LIMIT: usize = 200;

/// Whether the list is shown, and the newest read whose reply it will accept.
#[derive(Default)]
pub(super) struct HistoryList {
    open: bool,
    sequence: u64,
    pending: Option<u64>,
    visits: Vec<HistoryVisit>,
    error: Option<String>,
}

impl HistoryList {
    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    /// Shows the list and returns the read that fills it; a newer read supersedes older replies.
    pub(super) fn open(&mut self) -> SessionCommand {
        self.open = true;
        self.sequence = self.sequence.wrapping_add(1);
        self.pending = Some(self.sequence);
        SessionCommand::RecentHistory {
            request_id: self.sequence,
            limit: RECENT_LIMIT,
        }
    }

    pub(super) fn close(&mut self) {
        self.open = false;
        self.pending = None;
        self.visits.clear();
        self.error = None;
    }

    /// Accepts only the reply to the newest read while the list is open.
    pub(super) fn receive(
        &mut self,
        request_id: u64,
        result: Result<Vec<HistoryVisit>, SessionError>,
    ) -> bool {
        if !self.open || self.pending != Some(request_id) {
            return false;
        }
        self.pending = None;
        match result {
            Ok(visits) => {
                self.visits = visits;
                self.error = None;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
        true
    }

    fn status(&self) -> Option<String> {
        if let Some(error) = &self.error {
            return Some(error.clone());
        }
        if self.pending.is_some() {
            return Some("Loading history…".to_owned());
        }
        if self.visits.is_empty() {
            return Some("No history yet. Pages you visit will appear here.".to_owned());
        }
        None
    }
}

/// The visible name of a visit: its title, or its address before a title is known.
fn label(visit: &HistoryVisit) -> &str {
    if visit.title.is_empty() {
        &visit.address
    } else {
        &visit.title
    }
}

/// A short relative time; future times from a changed clock read as "just now".
fn age(visited_at: i64, now: i64) -> String {
    let seconds = now.saturating_sub(visited_at).max(0);
    match seconds {
        0..60 => "just now".to_owned(),
        60..3_600 => format!("{} min ago", seconds / 60),
        3_600..86_400 => format!("{} h ago", seconds / 3_600),
        _ => format!("{} d ago", seconds / 86_400),
    }
}

/// The page card's contents while the list is open.
pub(super) fn render(list: &HistoryList, cx: &mut Context<BrowserView>) -> Div {
    let header = div()
        .flex()
        .items_center()
        .gap_2()
        .child(div().flex_1().text_xl().child("History"))
        .child(
            button("history-clear-data", "Clear browsing data…").on_click(cx.listener(
                |this, _, _, cx| {
                    this.open_clear_data();
                    cx.notify();
                },
            )),
        )
        .child(
            button("history-close", "Close · Ctrl H").on_click(cx.listener(|this, _, _, cx| {
                this.toggle_history();
                cx.notify();
            })),
        );
    let mut rows = div().flex().flex_col().gap_1();
    if let Some(status) = list.status() {
        rows = rows.child(div().px_2().text_color(theme::icon_dim()).child(status));
    }
    // Without a clock the rows simply omit their age.
    let now = session::utc_seconds().ok();
    for visit in &list.visits {
        rows = rows.child(row(visit, now, cx));
    }
    div()
        .size_full()
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .bg(theme::ground())
        .child(header)
        .child(
            div()
                .id("history-scroll")
                .flex_1()
                .overflow_y_scroll()
                .child(rows),
        )
}

fn row(
    visit: &HistoryVisit,
    now: Option<i64>,
    cx: &mut Context<BrowserView>,
) -> impl gpui::IntoElement {
    let entry = visit.entry_id;
    let when = now.map_or_else(String::new, |now| age(visit.visited_at, now));
    let name = label(visit).to_owned();
    crate::controls::control(
        div().id(("history-visit", visit.id.get())),
        gpui::Role::Link,
        name,
    )
    .aria_description(format!("{} {when}", visit.address))
    .h(px(44.))
    .px_2()
    .rounded_md()
    .flex()
    .items_center()
    .gap_3()
    .cursor_pointer()
    .hover(|style| style.bg(theme::preview_row_selected()))
    .on_click(cx.listener(move |this, _, _, cx| {
        this.toggle_history();
        this.command(SessionCommand::OpenHistory(entry));
        cx.notify();
    }))
    .child(
        div()
            .flex_1()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(div().overflow_hidden().child(label(visit).to_owned()))
            .child(
                div()
                    .overflow_hidden()
                    .text_xs()
                    .text_color(theme::icon_dim())
                    .child(visit.address.clone()),
            ),
    )
    .child(div().text_xs().text_color(theme::icon_dim()).child(when))
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use super::*;

    fn visit(id: u64, title: &str) -> HistoryVisit {
        let id = NonZeroU64::new(id).expect("synthetic visit identity");
        HistoryVisit {
            id,
            entry_id: id,
            visited_at: 1_000,
            address: format!("https://visit-{id}.example.test/"),
            title: title.to_owned(),
        }
    }

    #[test]
    fn only_the_newest_open_read_fills_the_list() {
        let mut list = HistoryList::default();
        let SessionCommand::RecentHistory {
            request_id: first,
            limit,
        } = list.open()
        else {
            panic!("opening reads recent history");
        };
        assert_eq!(limit, RECENT_LIMIT);
        let SessionCommand::RecentHistory {
            request_id: second, ..
        } = list.open()
        else {
            panic!("refreshing reads recent history");
        };
        assert!(!list.receive(first, Ok(vec![visit(1, "Stale")])));
        assert!(list.visits.is_empty());
        assert!(list.receive(second, Ok(vec![visit(2, "Fresh")])));
        assert_eq!(list.visits, [visit(2, "Fresh")]);
        assert!(!list.receive(second, Ok(Vec::new())));

        // A reply after closing is ignored and the closed list holds nothing.
        let SessionCommand::RecentHistory {
            request_id: third, ..
        } = list.open()
        else {
            panic!("reopening reads recent history");
        };
        list.close();
        assert!(!list.receive(third, Ok(vec![visit(3, "Late")])));
        assert!(list.visits.is_empty());
        assert!(!list.is_open());
    }

    #[test]
    fn status_reports_loading_empty_and_failure() {
        let mut list = HistoryList::default();
        let SessionCommand::RecentHistory { request_id, .. } = list.open() else {
            panic!("opening reads recent history");
        };
        assert_eq!(list.status().as_deref(), Some("Loading history…"));
        assert!(list.receive(request_id, Ok(Vec::new())));
        assert!(
            list.status()
                .is_some_and(|status| status.starts_with("No history yet"))
        );
        let SessionCommand::RecentHistory { request_id, .. } = list.open() else {
            panic!("refreshing reads recent history");
        };
        assert!(list.receive(request_id, Err(SessionError::HistoryUnavailable)));
        assert_eq!(
            list.status(),
            Some(SessionError::HistoryUnavailable.to_string())
        );
    }

    #[test]
    fn labels_fall_back_to_the_address_and_ages_are_coarse() {
        assert_eq!(label(&visit(1, "Synthetic title")), "Synthetic title");
        assert_eq!(label(&visit(2, "")), "https://visit-2.example.test/");
        assert_eq!(age(1_000, 1_030), "just now");
        assert_eq!(age(1_000, 900), "just now");
        assert_eq!(age(0, 125), "2 min ago");
        assert_eq!(age(0, 7_200), "2 h ago");
        assert_eq!(age(0, 259_200), "3 d ago");
    }
}
