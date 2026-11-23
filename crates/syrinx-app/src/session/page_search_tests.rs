//! Proves the palette finds kept page text only where the owner's privacy rules allow it (#54).

use syrinx_core::PageTextPolicy;

use super::*;

/// Sends a palette query and returns its catalogue reply, which always comes first.
fn query(
    handle: &SessionHandle,
    events: &mut UnboundedReceiver<SessionEvent>,
    request_id: u64,
    text: &str,
    scope: Scope,
) {
    send(
        handle,
        SessionCommand::Query {
            request_id,
            text: text.to_owned(),
            scope,
            policy: query_policy(),
        },
    );
    match next_event(events) {
        SessionEvent::Search {
            request_id: answered,
            result: Ok(_),
        } if answered == request_id => {}
        event => panic!("expected the synthetic catalogue reply, got {event:?}"),
    }
}

/// The pages a query in Everything found by their text, sent after its catalogue reply.
fn found_pages(
    handle: &SessionHandle,
    events: &mut UnboundedReceiver<SessionEvent>,
    text: &str,
) -> Vec<FoundPage> {
    query(handle, events, 1, text, Scope::Everything);
    match next_event(events) {
        SessionEvent::FoundPages {
            request_id: 1,
            pages,
        } => pages,
        event => panic!("expected synthetic pages, got {event:?}"),
    }
}

/// Checks that a query in Everything sends no pages at all. A query in Tabs, which never looks
/// pages up, follows it, so its reply must be the next event.
fn assert_no_pages_sent(
    handle: &SessionHandle,
    events: &mut UnboundedReceiver<SessionEvent>,
    text: &str,
) {
    query(handle, events, 1, text, Scope::Everything);
    query(handle, events, 2, text, Scope::Tabs);
}

fn addresses(pages: &[FoundPage]) -> Vec<&str> {
    pages.iter().map(|page| page.address.as_str()).collect()
}

fn store(handle: &SessionHandle, address: &str) {
    let body = format!("Opening words of a synthetic article, then {WORD} near its end.");
    send(
        handle,
        SessionCommand::StorePageText(captured(address, &body)),
    );
}

#[test]
fn the_palette_finds_a_kept_page_by_a_half_typed_body_word_after_its_rows() {
    let fixture = Fixture::new();
    let address = "https://recall.example.test/article";
    let (handle, mut events) = fixture.start();
    state(&mut events);
    send(&handle, capture(true));
    store(&handle, address);

    let pages = found_pages(&handle, &mut events, &WORD[..7]);
    assert_eq!(addresses(&pages), [address]);
    assert_eq!(pages[0].title, "Synthetic captured page");
    let matched: Vec<_> = pages[0]
        .matches
        .iter()
        .map(|range| &pages[0].passage[range.clone()])
        .collect();
    assert_eq!(matched, [WORD]);

    // History lists visited pages, so it looks them up too; the other views do not.
    query(&handle, &mut events, 3, WORD, Scope::History);
    match next_event(&mut events) {
        SessionEvent::FoundPages {
            request_id: 3,
            pages,
        } => assert_eq!(addresses(&pages), [address]),
        event => panic!("expected synthetic History pages, got {event:?}"),
    }
    for scope in [Scope::Saved, Scope::Commands] {
        query(&handle, &mut events, 4, WORD, scope);
    }
    query(&handle, &mut events, 5, "", Scope::Everything);
    query(&handle, &mut events, 5, &WORD[..2], Scope::Everything);
    // None of those looked pages up, so the next event is this reply.
    query(&handle, &mut events, 6, WORD, Scope::Tabs);
    stop(handle, &mut events);
    fixture.cleanup();
}

#[test]
fn switching_capture_off_hides_kept_text_at_once_and_it_does_not_come_back() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    state(&mut events);
    send(&handle, capture(true));
    store(&handle, "https://erased.example.test/");
    assert_eq!(found_pages(&handle, &mut events, WORD).len(), 1);

    send(&handle, capture(false));
    assert_no_pages_sent(&handle, &mut events, WORD);
    send(&handle, capture(true));
    let pages = found_pages(&handle, &mut events, WORD);
    assert!(pages.is_empty(), "the erased page stays erased");
    stop(handle, &mut events);
    fixture.cleanup();
}

#[test]
fn excluding_a_site_hides_its_pages_and_its_subdomains_but_not_lookalikes() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    state(&mut events);
    send(&handle, capture(true));
    for address in [
        "https://www.owner.test/",
        "https://notowner.test/",
        "https://other.example.test/",
    ] {
        store(&handle, address);
    }
    send(&handle, excluding(true, &["owner.test"]));
    let pages = found_pages(&handle, &mut events, WORD);
    let mut found = addresses(&pages);
    found.sort_unstable();
    assert_eq!(
        found,
        ["https://notowner.test/", "https://other.example.test/"]
    );
    stop(handle, &mut events);
    fixture.cleanup();
}

#[test]
fn clearing_browsing_data_hides_the_pages_visited_in_its_range() {
    let fixture = Fixture::new();
    let now = utc_seconds().expect("synthetic clock");
    let (handle, mut events) = fixture.start();
    state(&mut events);
    send(&handle, capture(true));
    let body = format!("{WORD} synthetic words");
    send(
        &handle,
        SessionCommand::StorePageText(visited("https://cleared.example.test/", &body, now - 30)),
    );
    send(
        &handle,
        SessionCommand::StorePageText(visited("https://older.example.test/", &body, now - 7_200)),
    );
    clear_history(&handle, &mut events, now - 3_600, now + 1);
    let pages = found_pages(&handle, &mut events, WORD);
    assert_eq!(addresses(&pages), ["https://older.example.test/"]);
    stop(handle, &mut events);
    fixture.cleanup();
}

#[test]
fn rows_still_stored_are_hidden_when_the_rules_or_an_erase_forbid_them() {
    let fixture = Fixture::new();
    let now = utc_seconds().expect("synthetic clock");
    let (lease, mut page_text) = open_page_text(&fixture);
    // Rows an erase, a clear or the daily expiry has not removed yet: the search alone hides them.
    for (address, visited_at) in [
        ("https://fresh.example.test/", now - 60),
        ("https://www.owner.test/", now - 60),
        ("https://accounts.google.com/", now - 60),
        ("https://cleared.example.test/", now - 120),
        (
            "https://old.example.test/",
            now - PAGE_RETENTION_SECONDS - 10,
        ),
    ] {
        let page = visited(address, "guardsigil words", visited_at);
        page_text
            .index_mut()
            .expect("the index is open")
            .stage(
                &[Change::Upsert(FullTextDocument {
                    namespace: Namespace::page(),
                    id: page.address,
                    revision: 1,
                    captured_at: page.visited_at,
                    title: page.title,
                    body: page.text,
                })],
                visited_at,
            )
            .expect("stage a synthetic row");
    }
    page_text
        .index_mut()
        .expect("the index is open")
        .commit()
        .expect("commit synthetic rows");
    // A clear this session made covers one visit, as if its deletion had not reached the index.
    page_text.fence_clear(now - 150, now - 90, now);
    let listed = |page_text: &mut page_text::PageText, excluded: &[&str]| {
        let policy = PageTextPolicy::new(excluded).expect("synthetic exclusions");
        let pages = page_text.search("guardsigil", &policy, now);
        let mut found: Vec<_> = pages.into_iter().map(|page| page.address).collect();
        found.sort();
        found
    };
    assert_eq!(
        listed(&mut page_text, &[]),
        ["https://fresh.example.test/", "https://www.owner.test/"]
    );
    assert_eq!(
        listed(&mut page_text, &["owner.test"]),
        ["https://fresh.example.test/"]
    );
    assert_eq!(page_text.take_problem(), Ok(()));
    drop(page_text);
    drop(lease);
    fixture.cleanup();
}

#[test]
fn a_page_of_one_very_long_word_sends_a_bounded_passage() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    state(&mut events);
    send(&handle, capture(true));
    // Tantivy's snippet keeps one token whole, however long.
    let word = format!("longsigil{}", "x".repeat(50_000));
    send(
        &handle,
        SessionCommand::StorePageText(captured("https://long.example.test/", &word)),
    );
    let pages = found_pages(&handle, &mut events, "longsigil");
    assert_eq!(pages.len(), 1);
    let page = &pages[0];
    assert!(page.passage.len() <= 640, "{} bytes", page.passage.len());
    assert!(
        page.matches
            .iter()
            .all(|range| range.start < range.end && range.end <= page.passage.len())
    );
    stop(handle, &mut events);
    fixture.cleanup();
}

#[test]
fn a_found_page_never_shows_its_address_title_or_text_in_debug_output() {
    let page = FoundPage {
        address: "https://debug-sentinel.example.test/".to_owned(),
        title: "sentinel title".to_owned(),
        passage: "sentinel passage".to_owned(),
        matches: vec![std::ops::Range { start: 0, end: 8 }],
    };
    let event = SessionEvent::FoundPages {
        request_id: 1,
        pages: vec![page],
    };
    assert!(!format!("{event:?}").contains("sentinel"));
}
