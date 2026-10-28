//! Tests the per-tab back and forward list against the event sequences the #126 spike recorded.

use super::*;

fn report(change: HistoryChange, address: &str, back: bool, forward: bool) -> HistoryReport {
    HistoryReport {
        change,
        address: address.to_owned(),
        can_go_back: back,
        can_go_forward: forward,
    }
}

fn typed(address: &str, back: bool) -> HistoryReport {
    report(
        HistoryChange::NewDocument {
            user_initiated: true,
        },
        address,
        back,
        false,
    )
}

fn back_or_forward(address: &str, back: bool, forward: bool) -> HistoryReport {
    report(HistoryChange::BackOrForward, address, back, forward)
}

fn addresses(list: &NavigationList, direction: HistoryDirection) -> Vec<String> {
    list.rows(direction, HISTORY_ROW_LIMIT)
        .into_iter()
        .map(|row| row.address.to_owned())
        .collect()
}

/// Five typed pages, a.test to e.test, with e.test current.
fn five_pages() -> NavigationList {
    let mut list = NavigationList::default();
    list.observe(&typed("https://a.test/", false));
    for (index, page) in ["b", "c", "d", "e"].into_iter().enumerate() {
        let followed = list.observe(&typed(&format!("https://{page}.test/"), true));
        assert_eq!(followed, HistoryObserved::Followed, "page {index}");
    }
    list
}

#[test]
fn the_first_report_starts_the_list_at_its_address() {
    let mut list = NavigationList::default();
    assert_eq!(list.current_address(), None);
    list.observe(&typed("https://a.test/", false));
    assert_eq!(list.current_address(), Some("https://a.test/"));
    assert!(addresses(&list, HistoryDirection::Back).is_empty());
}

#[test]
fn back_rows_list_earlier_pages_nearest_first_with_their_step_counts() {
    let list = five_pages();
    assert_eq!(
        addresses(&list, HistoryDirection::Back),
        [
            "https://d.test/",
            "https://c.test/",
            "https://b.test/",
            "https://a.test/"
        ]
    );
    let steps: Vec<usize> = list
        .rows(HistoryDirection::Back, HISTORY_ROW_LIMIT)
        .iter()
        .map(|row| row.steps)
        .collect();
    assert_eq!(steps, [1, 2, 3, 4]);
    assert!(addresses(&list, HistoryDirection::Forward).is_empty());
}

#[test]
fn the_engine_replacing_its_initial_blank_entry_resets_to_the_first_page() {
    // The spike: the first HistoryChanged shows about:blank, then the first page with no back.
    let mut list = NavigationList::default();
    list.observe(&report(
        HistoryChange::Unchanged,
        "about:blank",
        false,
        false,
    ));
    let observed = list.observe(&typed("https://a.test/", false));
    assert_eq!(observed, HistoryObserved::Reset);
    assert_eq!(list.current_address(), Some("https://a.test/"));
    assert!(addresses(&list, HistoryDirection::Back).is_empty());
}

#[test]
fn a_jump_back_lands_on_the_chosen_page_and_keeps_the_pages_between() {
    let mut list = five_pages();
    assert_eq!(list.start_jump(HistoryDirection::Back, 4), Some(4));
    let observed = list.observe(&back_or_forward("https://a.test/", false, true));
    assert_eq!(observed, HistoryObserved::Followed);
    assert_eq!(list.current_address(), Some("https://a.test/"));
    assert!(addresses(&list, HistoryDirection::Back).is_empty());
    assert_eq!(
        addresses(&list, HistoryDirection::Forward),
        [
            "https://b.test/",
            "https://c.test/",
            "https://d.test/",
            "https://e.test/"
        ]
    );
}

#[test]
fn a_jump_that_lands_somewhere_else_resets_the_list_to_the_landed_page() {
    let mut list = five_pages();
    assert_eq!(list.start_jump(HistoryDirection::Back, 2), Some(2));
    let observed = list.observe(&back_or_forward("https://elsewhere.test/", true, true));
    assert_eq!(observed, HistoryObserved::Reset);
    assert_eq!(list.current_address(), Some("https://elsewhere.test/"));
    assert!(addresses(&list, HistoryDirection::Back).is_empty());
    assert!(addresses(&list, HistoryDirection::Forward).is_empty());
}

#[test]
fn a_jump_past_the_end_of_the_list_is_refused() {
    let mut list = five_pages();
    assert_eq!(list.start_jump(HistoryDirection::Forward, 1), None);
    assert_eq!(list.start_jump(HistoryDirection::Back, 5), None);
    assert_eq!(list.start_jump(HistoryDirection::Back, 0), None);
}

#[test]
fn a_same_page_change_replaces_the_current_row_instead_of_adding_one() {
    // pushState and replaceState raise the same events, so both replace (owner, 2026-10-28).
    let mut list = five_pages();
    list.observe(&report(
        HistoryChange::SamePage,
        "https://e.test/pushed",
        true,
        false,
    ));
    assert_eq!(list.current_address(), Some("https://e.test/pushed"));
    assert_eq!(
        addresses(&list, HistoryDirection::Back)[0],
        "https://d.test/"
    );
    assert_eq!(
        list.rows(HistoryDirection::Back, HISTORY_ROW_LIMIT).len(),
        4
    );
}

#[test]
fn a_page_left_without_a_user_gesture_is_hidden_and_not_counted_as_a_step() {
    // The spike: Chromium's Back skips a page that moved on by itself.
    let mut list = five_pages();
    list.observe(&typed("https://redirect.test/", true));
    list.observe(&report(
        HistoryChange::NewDocument {
            user_initiated: false,
        },
        "https://landing.test/",
        true,
        false,
    ));
    assert_eq!(
        addresses(&list, HistoryDirection::Back)[..2],
        ["https://e.test/", "https://d.test/"]
    );
    assert_eq!(list.start_jump(HistoryDirection::Back, 2), Some(2));
    assert_eq!(
        list.observe(&back_or_forward("https://d.test/", true, true)),
        HistoryObserved::Followed
    );
    assert_eq!(
        addresses(&list, HistoryDirection::Forward),
        ["https://e.test/", "https://landing.test/"]
    );
}

#[test]
fn a_new_page_drops_the_pages_ahead() {
    let mut list = five_pages();
    list.start_jump(HistoryDirection::Back, 2);
    list.observe(&back_or_forward("https://c.test/", true, true));
    list.observe(&typed("https://f.test/", true));
    assert!(addresses(&list, HistoryDirection::Forward).is_empty());
    assert_eq!(
        addresses(&list, HistoryDirection::Back)[0],
        "https://c.test/"
    );
}

#[test]
fn an_unrequested_back_step_finds_the_nearest_matching_page() {
    // A page's own history.back(), or the mouse's back button.
    let mut list = five_pages();
    let observed = list.observe(&back_or_forward("https://d.test/", true, true));
    assert_eq!(observed, HistoryObserved::Followed);
    assert_eq!(list.current_address(), Some("https://d.test/"));
    assert_eq!(
        addresses(&list, HistoryDirection::Forward),
        ["https://e.test/"]
    );
}

#[test]
fn an_unrequested_step_to_an_unknown_page_resets_the_list() {
    let mut list = five_pages();
    let observed = list.observe(&back_or_forward("https://unknown.test/", true, true));
    assert_eq!(observed, HistoryObserved::Reset);
    assert_eq!(list.current_address(), Some("https://unknown.test/"));
}

#[test]
fn the_engine_disagreeing_about_going_back_resets_the_list() {
    let mut list = five_pages();
    let observed = list.observe(&report(
        HistoryChange::Unchanged,
        "https://e.test/",
        false,
        false,
    ));
    assert_eq!(observed, HistoryObserved::Reset);
    assert!(addresses(&list, HistoryDirection::Back).is_empty());
}

#[test]
fn the_engine_disagreeing_about_going_forward_resets_the_list() {
    let mut list = five_pages();
    let observed = list.observe(&report(
        HistoryChange::Unchanged,
        "https://e.test/",
        true,
        true,
    ));
    assert_eq!(observed, HistoryObserved::Reset);
}

#[test]
fn a_reload_keeps_the_list() {
    let mut list = five_pages();
    let observed = list.observe(&report(
        HistoryChange::Unchanged,
        "https://e.test/",
        true,
        false,
    ));
    assert_eq!(observed, HistoryObserved::Followed);
    assert_eq!(
        list.rows(HistoryDirection::Back, HISTORY_ROW_LIMIT).len(),
        4
    );
}

#[test]
fn a_new_page_cancels_a_pending_jump() {
    let mut list = five_pages();
    list.start_jump(HistoryDirection::Back, 3);
    list.observe(&typed("https://f.test/", true));
    // A later unrequested step is matched by address, not against the stale target.
    let observed = list.observe(&back_or_forward("https://e.test/", true, true));
    assert_eq!(observed, HistoryObserved::Followed);
    assert_eq!(list.current_address(), Some("https://e.test/"));
}

#[test]
fn titles_follow_the_current_page() {
    let mut list = five_pages();
    list.retitle("Synthetic E");
    list.start_jump(HistoryDirection::Back, 1);
    list.observe(&back_or_forward("https://d.test/", true, true));
    let ahead = list.rows(HistoryDirection::Forward, HISTORY_ROW_LIMIT);
    assert_eq!(ahead[0].title, "Synthetic E");
    assert_eq!(ahead[0].address, "https://e.test/");
}

#[test]
fn rows_stop_at_the_limit() {
    let mut list = NavigationList::default();
    list.observe(&typed("https://page0.test/", false));
    for page in 1..30 {
        list.observe(&typed(&format!("https://page{page}.test/"), true));
    }
    assert_eq!(
        list.rows(HistoryDirection::Back, HISTORY_ROW_LIMIT).len(),
        HISTORY_ROW_LIMIT
    );
    assert_eq!(HISTORY_ROW_LIMIT, 15);
}

#[test]
fn the_list_keeps_at_most_the_engine_limit_of_entries() {
    let mut list = NavigationList::default();
    list.observe(&typed("https://page0.test/", false));
    for page in 1..(ENTRY_LIMIT + 10) {
        list.observe(&typed(&format!("https://page{page}.test/"), true));
    }
    assert_eq!(
        list.rows(HistoryDirection::Back, usize::MAX).len(),
        ENTRY_LIMIT - 1
    );
    let last = format!("https://page{}.test/", ENTRY_LIMIT + 9);
    assert_eq!(list.current_address(), Some(last.as_str()));
}

#[test]
fn debug_output_never_shows_addresses_or_titles() {
    let mut list = five_pages();
    list.retitle("Synthetic secret title");
    let debug = format!("{list:?}");
    assert!(!debug.contains("test"), "{debug}");
    assert!(!debug.contains("secret"), "{debug}");
    let report = typed("https://hidden.test/", true);
    assert!(!format!("{report:?}").contains("hidden"));
}

#[test]
fn lists_are_kept_per_tab_and_dropped_with_the_tab() {
    let mut lists = NavigationLists::default();
    let (one, two) = (TabId::new(1).unwrap(), TabId::new(2).unwrap());
    lists.observe(one, &typed("https://a.test/", false));
    lists.observe(two, &typed("https://b.test/", false));
    assert_eq!(
        lists.get(one).and_then(NavigationList::current_address),
        Some("https://a.test/")
    );
    lists.retain(|id| id == two);
    assert!(lists.get(one).is_none());
    assert!(lists.get(two).is_some());
}
