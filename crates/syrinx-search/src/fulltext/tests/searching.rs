//! Checks ranking inputs, folding and the bounded, highlighted snippet.

use super::super::*;
use super::fixture::{Fixture, NOW, page_document, upsert};

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
