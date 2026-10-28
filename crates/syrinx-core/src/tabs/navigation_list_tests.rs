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
    assert!(list.start_jump(HistoryDirection::Back, 4));
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
    assert!(list.start_jump(HistoryDirection::Back, 2));
    let observed = list.observe(&back_or_forward("https://elsewhere.test/", true, true));
    assert_eq!(observed, HistoryObserved::Reset);
    assert_eq!(list.current_address(), Some("https://elsewhere.test/"));
    assert!(addresses(&list, HistoryDirection::Back).is_empty());
    assert!(addresses(&list, HistoryDirection::Forward).is_empty());
}

#[test]
fn a_jump_past_the_end_of_the_list_is_refused() {
    let mut list = five_pages();
    assert!(!list.start_jump(HistoryDirection::Forward, 1));
    assert!(!list.start_jump(HistoryDirection::Back, 5));
    assert!(!list.start_jump(HistoryDirection::Back, 0));
}

#[test]
fn a_same_page_change_replaces_the_current_row_instead_of_adding_one() {
    // pushState and replaceState raise the same events, so both replace (owner, 2026-10-28).
    let mut list = five_pages();
    list.observe(&report(
        HistoryChange::SamePage { navigating: false },
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
    assert!(list.start_jump(HistoryDirection::Back, 2));
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
    for page in 1..(HISTORY_ENTRY_LIMIT + 10) {
        list.observe(&typed(&format!("https://page{page}.test/"), true));
    }
    assert_eq!(
        list.rows(HistoryDirection::Back, usize::MAX).len(),
        HISTORY_ENTRY_LIMIT - 1
    );
    let last = format!("https://page{}.test/", HISTORY_ENTRY_LIMIT + 9);
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
    lists.remove(one);
    assert!(lists.get(one).is_none());
    assert!(lists.get(two).is_some());
}

#[test]
fn a_refused_jump_clears_an_earlier_target() {
    let mut list = five_pages();
    assert!(list.start_jump(HistoryDirection::Back, 3));
    assert!(!list.start_jump(HistoryDirection::Forward, 1));
    // An unrequested step to d.test is matched by address, not failed against b.test.
    let observed = list.observe(&back_or_forward("https://d.test/", true, true));
    assert_eq!(observed, HistoryObserved::Followed);
}

#[test]
fn a_jump_that_lands_inside_the_same_document_on_another_page_resets() {
    // Review B2: the engine kept a pushState entry the list replaced, so one GoForward
    // lands on it, inside the same document, instead of on the chosen row.
    let mut list = five_pages();
    list.start_jump(HistoryDirection::Back, 2);
    list.observe(&back_or_forward("https://c.test/", true, true));
    assert!(list.start_jump(HistoryDirection::Forward, 1));
    let observed = list.observe(&report(
        HistoryChange::SamePage { navigating: false },
        "https://c.test/pushed",
        true,
        true,
    ));
    assert_eq!(observed, HistoryObserved::Reset);
    assert_eq!(list.current_address(), Some("https://c.test/pushed"));
}

#[test]
fn a_jump_that_lands_inside_the_same_document_on_its_row_follows() {
    let mut list = five_pages();
    list.start_jump(HistoryDirection::Back, 1);
    list.observe(&back_or_forward("https://d.test/", true, true));
    assert!(list.start_jump(HistoryDirection::Forward, 1));
    let observed = list.observe(&report(
        HistoryChange::SamePage { navigating: false },
        "https://e.test/",
        true,
        false,
    ));
    assert_eq!(observed, HistoryObserved::Followed);
    assert_eq!(
        addresses(&list, HistoryDirection::Back)[0],
        "https://d.test/"
    );
}

#[test]
fn a_reload_ends_a_pending_jump() {
    let mut list = five_pages();
    assert!(list.start_jump(HistoryDirection::Back, 3));
    list.observe(&report(
        HistoryChange::Unchanged,
        "https://e.test/",
        true,
        false,
    ));
    // A later unrequested step is matched by address, not failed against the old target.
    let observed = list.observe(&back_or_forward("https://d.test/", true, true));
    assert_eq!(observed, HistoryObserved::Followed);
}

mod in_the_tab_model {
    use super::*;
    use crate::tabs::{Activation, TabKind, TabMetadata, TabModel};

    fn model_with_tab() -> (TabModel, TabId) {
        let mut model = TabModel::new();
        let id = model
            .open(TabKind::Web, TabMetadata::default(), Activation::Foreground)
            .unwrap();
        (model, id)
    }

    #[test]
    fn each_open_tab_keeps_its_own_list() {
        let (mut model, id) = model_with_tab();
        model
            .observe_history(id, &typed("https://a.test/", false))
            .unwrap();
        model
            .observe_history(id, &typed("https://b.test/", true))
            .unwrap();
        let rows = model.history_rows(id, HistoryDirection::Back, HISTORY_ROW_LIMIT);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].address, "https://a.test/");
        assert_eq!(
            model.history_reach(id),
            HistoryReach {
                back: true,
                forward: false
            }
        );
    }

    #[test]
    fn a_report_for_a_tab_that_is_not_open_is_refused() {
        let (mut model, _) = model_with_tab();
        let stranger = TabId::new(99).unwrap();
        assert_eq!(
            model.observe_history(stranger, &typed("https://a.test/", false)),
            Err(TabError::UnknownTab(stranger))
        );
    }

    #[test]
    fn closing_a_tab_drops_its_list() {
        let (mut model, id) = model_with_tab();
        model
            .observe_history(id, &typed("https://a.test/", false))
            .unwrap();
        model
            .observe_history(id, &typed("https://b.test/", true))
            .unwrap();
        model.close(id).unwrap();
        assert_eq!(model.history_reach(id), HistoryReach::default());
        assert!(!model.start_history_jump(id, HistoryDirection::Back, 1));
    }

    #[test]
    fn a_new_title_names_the_current_row() {
        let (mut model, id) = model_with_tab();
        model
            .observe_history(id, &typed("https://a.test/", false))
            .unwrap();
        let metadata = TabMetadata {
            title: "Synthetic A".into(),
            ..TabMetadata::default()
        };
        model.update_metadata(id, metadata).unwrap();
        model
            .observe_history(id, &typed("https://b.test/", true))
            .unwrap();
        let rows = model.history_rows(id, HistoryDirection::Back, HISTORY_ROW_LIMIT);
        assert_eq!(rows[0].title, "Synthetic A");
    }

    #[test]
    fn a_typed_navigation_does_not_erase_the_title_of_the_page_it_leaves() {
        // Found in #126's window check: typing an address resets the tab's metadata to a blank
        // title before the new page commits, which named the page being left "".
        let (mut model, id) = model_with_tab();
        model
            .observe_history(id, &typed("https://a.test/", false))
            .unwrap();
        let titled = TabMetadata {
            title: "Synthetic A".into(),
            ..TabMetadata::default()
        };
        model.update_metadata(id, titled).unwrap();
        let typing = TabMetadata {
            address: "https://b.test/".into(),
            ..TabMetadata::default()
        };
        model.update_metadata(id, typing).unwrap();
        model
            .observe_history(id, &typed("https://b.test/", true))
            .unwrap();
        let rows = model.history_rows(id, HistoryDirection::Back, HISTORY_ROW_LIMIT);
        assert_eq!(rows[0].title, "Synthetic A");
    }

    #[test]
    fn a_jump_is_planned_on_the_tab_model() {
        let (mut model, id) = model_with_tab();
        model
            .observe_history(id, &typed("https://a.test/", false))
            .unwrap();
        model
            .observe_history(id, &typed("https://b.test/", true))
            .unwrap();
        assert!(model.start_history_jump(id, HistoryDirection::Back, 1));
        assert!(!model.start_history_jump(id, HistoryDirection::Forward, 1));
    }
}

#[test]
fn the_page_changing_its_own_address_during_a_back_keeps_the_jump() {
    // Review of round 2: a replaceState while a slow Back loads is not the Back's landing.
    let mut list = five_pages();
    assert!(list.start_jump(HistoryDirection::Back, 1));
    let observed = list.observe(&report(
        HistoryChange::SamePage { navigating: true },
        "https://e.test/?tracking-removed",
        true,
        false,
    ));
    assert_eq!(observed, HistoryObserved::Followed);
    let landed = list.observe(&back_or_forward("https://d.test/", true, true));
    assert_eq!(landed, HistoryObserved::Followed);
    assert_eq!(
        addresses(&list, HistoryDirection::Forward),
        ["https://e.test/?tracking-removed"]
    );
}

#[test]
fn an_abandoned_jump_is_dropped_without_renaming_the_current_row() {
    // Review of #212, nit: a failed Back is reported with the next change, so its report
    // carries the address of the page typed after it, which is not the current row's.
    let mut list = five_pages();
    assert!(list.start_jump(HistoryDirection::Back, 2));
    let dropped = list.observe(&report(
        HistoryChange::JumpAbandoned,
        "https://f.test/",
        true,
        false,
    ));
    assert_eq!(dropped, HistoryObserved::Followed);
    list.observe(&typed("https://f.test/", true));
    assert_eq!(
        addresses(&list, HistoryDirection::Back),
        [
            "https://e.test/",
            "https://d.test/",
            "https://c.test/",
            "https://b.test/",
            "https://a.test/"
        ]
    );
    // The dropped jump no longer judges a later, unrequested step.
    let stepped = list.observe(&back_or_forward("https://e.test/", true, true));
    assert_eq!(stepped, HistoryObserved::Followed);
}

#[test]
fn a_reset_list_still_knows_the_engine_can_go_back() {
    // After a reset, Back still works though nothing is listed, so the menu can say so.
    let mut list = five_pages();
    assert!(list.start_jump(HistoryDirection::Back, 1));
    let observed = list.observe(&back_or_forward("https://elsewhere.test/", true, true));
    assert_eq!(observed, HistoryObserved::Reset);
    assert_eq!(list.reach(), HistoryReach::default());
    assert_eq!(
        list.engine_reach(),
        HistoryReach {
            back: true,
            forward: true
        }
    );
}

#[test]
fn an_abandoned_jump_does_not_change_the_engine_reach() {
    let mut list = five_pages();
    list.observe(&report(
        HistoryChange::JumpAbandoned,
        "https://f.test/",
        false,
        true,
    ));
    assert_eq!(
        list.engine_reach(),
        HistoryReach {
            back: true,
            forward: false
        }
    );
}
