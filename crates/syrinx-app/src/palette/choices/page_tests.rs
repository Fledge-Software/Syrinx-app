//! Checks that pages found by their kept text join the rows without moving them (#54).

use syrinx_core::TabId;
use syrinx_search::ScopeCounts;

use super::*;

fn tab(id: u64) -> Target {
    Target::Tab(TabId::new(id).expect("synthetic tab"))
}

fn rows(targets: &[Target]) -> SearchResults {
    SearchResults {
        generation: 1,
        counts: ScopeCounts::default(),
        hits: targets
            .iter()
            .map(|target| SearchHit {
                target: *target,
                score: 1,
                title: "Synthetic row".to_owned(),
                address: "https://rows.example.test/".to_owned(),
                title_matches: Vec::new(),
                address_matches: Vec::new(),
            })
            .collect(),
    }
}

fn page(address: &str) -> FoundPage {
    FoundPage {
        address: address.to_owned(),
        title: "Synthetic found page".to_owned(),
        passage: "words around recallword in a synthetic page".to_owned(),
        matches: vec![std::ops::Range { start: 13, end: 23 }],
    }
}

const FIRST: &str = "https://first.example.test/";
const SECOND: &str = "https://second.example.test/";

/// A History-view query with one row, answered, and two pages under it.
fn answered_with_pages(choices: &mut PaletteChoices) -> u64 {
    let request = choices.query_scoped("recallword", false);
    assert!(choices.receive(request, Ok(rows(&[tab(1)]))));
    assert!(choices.receive_pages(request, vec![page(FIRST), page(SECOND)]));
    request
}

#[test]
fn pages_follow_the_rows_and_enter_on_one_opens_its_address() {
    let mut choices = PaletteChoices::default();
    answered_with_pages(&mut choices);
    assert_eq!(choices.row_count(), 3);
    assert_eq!(choices.selected, Some(0), "the first row stays chosen");
    choices.next();
    assert_eq!(
        choices.selected_page().map(|page| page.address.as_str()),
        Some(FIRST)
    );
    assert!(matches!(
        choices.submission("recallword".to_owned(), false),
        Some(PaletteEvent::OpenPage(address)) if address == FIRST
    ));
    choices.next();
    choices.next();
    assert_eq!(choices.selected, Some(2), "Down stops at the last page");
}

#[test]
fn pages_for_another_request_or_before_the_rows_are_ignored() {
    let mut choices = PaletteChoices::default();
    let old = choices.query_scoped("recall", false);
    let current = choices.query_scoped("recallword", false);
    assert!(
        !choices.receive_pages(current, vec![page(FIRST)]),
        "the rows come first"
    );
    assert!(choices.receive(current, Ok(rows(&[tab(1)]))));
    assert!(!choices.receive_pages(old, vec![page(FIRST)]));
    assert!(choices.pages.is_empty());
    // A new query drops the pages the last one found.
    assert!(choices.receive_pages(current, vec![page(FIRST)]));
    choices.query_scoped("other", false);
    assert!(choices.pages.is_empty());
}

#[test]
fn a_refresh_keeps_a_chosen_page_and_chooses_nothing_else_meanwhile() {
    let mut choices = PaletteChoices::default();
    answered_with_pages(&mut choices);
    choices.next();
    choices.next();
    let request = choices.refresh();
    assert!(choices.receive(request, Ok(rows(&[tab(1)]))));
    assert_eq!(choices.selected, None, "the page has not come back yet");
    // Enter waits for the page rather than choosing another row or nothing.
    assert!(choices.submission("recallword".to_owned(), false).is_none());
    assert!(choices.take_deferred().is_none());
    // The pages may come back in another order; the same page is chosen again, and opens.
    assert!(choices.receive_pages(request, vec![page(SECOND), page(FIRST)]));
    assert_eq!(
        choices.selected_page().map(|page| page.address.as_str()),
        Some(SECOND)
    );
    assert!(matches!(
        choices.take_deferred(),
        Some(PaletteEvent::OpenPage(address)) if address == SECOND
    ));
}

#[test]
fn a_chosen_page_the_refresh_no_longer_finds_gives_way_to_the_typed_row() {
    let mut choices = PaletteChoices::default();
    let request = choices.query_scoped("recallword", true);
    assert!(choices.receive(request, Ok(rows(&[tab(1)]))));
    assert!(choices.receive_pages(request, vec![page(FIRST)]));
    choices.next();
    choices.next();
    assert!(choices.selected_page().is_some());
    let request = choices.refresh();
    assert!(choices.receive(request, Ok(rows(&[tab(1)]))));
    assert!(choices.receive_pages(request, Vec::new()));
    assert_eq!(choices.selected, Some(0), "the typed row");

    // A failed reply sends no pages, so nothing waits for one.
    choices.next();
    choices.next();
    let request = choices.refresh();
    let failure = SessionError::Search(syrinx_search::SearchError::QueryTooLong);
    assert!(choices.receive(request, Err(failure)));
    assert!(!choices.must_wait_for_rows(false));
}

#[test]
fn a_click_opens_only_a_page_still_listed_for_the_current_request() {
    let mut choices = PaletteChoices::default();
    let request = answered_with_pages(&mut choices);
    assert!(matches!(
        choices.page_activation(request, SECOND),
        Some(PaletteEvent::OpenPage(address)) if address == SECOND
    ));
    assert!(choices.page_activation(request + 1, SECOND).is_none());
    assert!(
        choices
            .page_activation(request, "https://gone.example.test/")
            .is_none()
    );
    choices.forget_pages();
    assert!(choices.page_activation(request, SECOND).is_none());
    assert_eq!(choices.row_count(), 1);
}
