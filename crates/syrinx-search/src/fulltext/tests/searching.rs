//! Checks ranking inputs, folding and the bounded, highlighted snippet.

use super::super::*;
use super::fixture::{DAY, Fixture, NOW, document, notes, page_document, upsert};

fn request(text: &str, prefix_last: bool) -> SearchRequest {
    SearchRequest {
        text: text.to_owned(),
        prefix_last,
        namespaces: NamespaceFilter::All,
        limit: 10,
    }
}

fn index_with(fixture: &Fixture, documents: Vec<FullTextDocument>) -> FullText {
    let mut index = fixture.open();
    let changes: Vec<_> = documents.into_iter().map(upsert).collect();
    index
        .stage(&changes, NOW)
        .expect("stage synthetic documents");
    index.commit().expect("commit synthetic documents");
    index
}

#[test]
fn a_prefix_match_has_a_highlighted_snippet() {
    let fixture = Fixture::new();
    let index = index_with(&fixture, vec![page_document("a", 1, NOW, "lanternfish")]);
    let hits = index
        .search(&request("lantern", true))
        .expect("search by prefix");
    assert_eq!(hits.len(), 1);
    let hit = &hits[0];
    assert!(!hit.snippet.is_empty(), "prefix matches lost their snippet");
    let highlighted: Vec<&str> = hit
        .highlights
        .iter()
        .map(|range| &hit.snippet[range.clone()])
        .collect();
    assert_eq!(highlighted, ["lanternfish"]);
}

#[test]
fn whole_words_do_not_match_as_prefixes() {
    let fixture = Fixture::new();
    let index = index_with(&fixture, vec![page_document("a", 1, NOW, "lanternfish")]);
    assert!(
        index
            .search(&request("lantern", false))
            .expect("search")
            .is_empty()
    );
}

#[test]
fn every_word_must_match_in_the_title_or_the_body() {
    let fixture = Fixture::new();
    let mut titled = page_document("titled", 1, NOW, "quietword");
    titled.title = "Driftwood almanac".to_owned();
    let index = index_with(
        &fixture,
        vec![titled, page_document("plain", 1, NOW, "quietword")],
    );
    assert_eq!(fixture.ids(&index, "driftwood quietword"), ["titled"]);
    assert!(fixture.ids(&index, "driftwood missingword").is_empty());
}

#[test]
fn a_title_only_match_shows_the_start_of_the_body() {
    let fixture = Fixture::new();
    let mut document = page_document("a", 1, NOW, "bodyword");
    document.title = "Marigold".to_owned();
    document.body = "é".repeat(500);
    let index = index_with(&fixture, vec![document]);
    let hits = index
        .search(&request("marigold", false))
        .expect("search title");
    assert_eq!(hits.len(), 1);
    // 160 bytes of two-byte characters: the cut never splits one.
    assert_eq!(hits[0].snippet, "é".repeat(search::SNIPPET_BYTES / 2));
    assert!(hits[0].highlights.is_empty());
}

#[test]
fn case_and_accents_are_folded() {
    let fixture = Fixture::new();
    let index = index_with(&fixture, vec![page_document("a", 1, NOW, "Crème Brûlée")]);
    assert_eq!(fixture.ids(&index, "creme BRULEE"), ["a"]);
}

#[test]
fn snippets_stay_bounded_and_limits_hold() {
    let fixture = Fixture::new();
    let documents = (0..5)
        .map(|number| {
            let mut document = page_document(&format!("d{number}"), 1, NOW, "boundsigil");
            document.body = format!(
                "{} boundsigil {}",
                "filler ".repeat(200),
                "filler ".repeat(200)
            );
            document
        })
        .collect();
    let index = index_with(&fixture, documents);
    let mut limited = request("boundsigil", false);
    limited.limit = 3;
    let hits = index.search(&limited).expect("search with limit");
    assert_eq!(hits.len(), 3);
    for hit in hits {
        assert!(hit.snippet.len() <= search::SNIPPET_BYTES);
        assert!(hit.snippet.contains("boundsigil"));
    }
}

#[test]
fn queries_without_words_or_room_return_nothing() {
    let fixture = Fixture::new();
    let index = index_with(&fixture, vec![page_document("a", 1, NOW, "anyword")]);
    assert!(
        index
            .search(&request("  !? ", false))
            .expect("blank")
            .is_empty()
    );
    let mut none = request("anyword", false);
    none.limit = 0;
    assert!(index.search(&none).expect("zero limit").is_empty());
    // An empty prefix matches nothing rather than every document.
    assert!(
        index
            .search(&request("", true))
            .expect("empty prefix")
            .is_empty()
    );
}

/// The page ids `search_pages` returns for `text` at `now`, accepting every page.
fn page_ids(index: &FullText, text: &str, now: i64) -> Vec<String> {
    index
        .search_pages(text, now, 10, |_, _| true)
        .expect("search page text")
        .into_iter()
        .map(|hit| hit.id)
        .collect()
}

#[test]
fn a_page_search_matches_the_last_word_as_typed_and_highlights_it() {
    let fixture = Fixture::new();
    let index = index_with(&fixture, vec![page_document("a", 1, NOW, "lanternfish")]);
    let hits = index
        .search_pages("paragraph lantern", NOW, 10, |_, _| true)
        .expect("search page text");
    assert_eq!(hits.len(), 1);
    let highlighted: Vec<&str> = hits[0]
        .highlights
        .iter()
        .map(|range| &hits[0].snippet[range.clone()])
        .collect();
    assert!(highlighted.contains(&"lanternfish"), "{highlighted:?}");
}

#[test]
fn a_page_search_reads_page_text_only_and_skips_title_only_matches() {
    let fixture = Fixture::new();
    let mut titled = page_document("titled", 1, NOW, "bodyword");
    titled.title = "Marigold".to_owned();
    let index = index_with(
        &fixture,
        vec![
            titled,
            page_document("body", 1, NOW, "marigold"),
            document(notes(), "note", 1, NOW, "marigold"),
        ],
    );
    // History already lists a page by its title; only a match in its text earns a row.
    assert_eq!(page_ids(&index, "marigold", NOW), ["body"]);
    // A title word still narrows a body match.
    assert_eq!(page_ids(&index, "marigold bodyword", NOW), ["titled"]);
}

#[test]
fn a_page_search_never_returns_text_past_seven_days_even_before_expiry_runs() {
    let fixture = Fixture::new();
    let index = index_with(
        &fixture,
        vec![
            page_document("fresh", 1, NOW, "agesigil"),
            page_document("ageing", 1, NOW - 6 * DAY, "agesigil"),
        ],
    );
    let mut found = page_ids(&index, "agesigil", NOW);
    found.sort();
    assert_eq!(found, ["ageing", "fresh"]);
    // Seven days after its visit the text is past D40's limit, as `expire` judges it.
    let expires = NOW - 6 * DAY + PAGE_RETENTION_SECONDS;
    assert_eq!(page_ids(&index, "agesigil", expires - 1).len(), 2);
    assert_eq!(page_ids(&index, "agesigil", expires), ["fresh"]);
    assert_eq!(fixture.ids(&index, "agesigil").len(), 2, "still stored");
}

#[test]
fn a_page_search_skips_refused_pages_and_still_fills_its_limit() {
    let fixture = Fixture::new();
    let documents = (0..8)
        .map(|number| page_document(&format!("p{number}"), 1, NOW - number, "fillsigil"))
        .collect();
    let index = index_with(&fixture, documents);
    let refused = ["p0", "p1", "p2", "p3", "p4"];
    let hits = index
        .search_pages("fillsigil", NOW, 3, |id, visited_at| {
            assert!(visited_at <= NOW, "the stored date is the visit");
            !refused.contains(&id)
        })
        .expect("search page text");
    assert_eq!(hits.len(), 3);
    assert!(hits.iter().all(|hit| !refused.contains(&hit.id.as_str())));
}
