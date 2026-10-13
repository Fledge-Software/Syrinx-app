//! Exercises catalogue mutations, query semantics and boundary failures with synthetic rows.

use std::num::NonZeroU64;

use syrinx_core::{SavedPageId, TabId};

use super::*;
use crate::{
    Usage,
    candidate::{FieldMasks, QueryBound, score_upper_bound},
};

fn target(id: u64) -> Target {
    Target::Tab(TabId::new(id).expect("synthetic nonzero ID"))
}

fn entry(id: u64, title: &str, address: &str) -> Entry {
    Entry {
        target: target(id),
        title: title.into(),
        address: address.into(),
        usage: Usage {
            visits: 0,
            last_used: None,
        },
        open: false,
    }
}

fn policy() -> SearchPolicy {
    SearchPolicy {
        limit: 50,
        max_typos: 0,
        blank: BlankQuery::All,
        boosts: BoostWeights {
            open: 0,
            frequency: 0,
            recency: 0,
            recency_window_secs: NonZeroU64::new(100).expect("nonzero window"),
        },
    }
}

fn search(catalogue: &mut Catalogue, text: &str) -> SearchResults {
    catalogue
        .query(text, Scope::Everything, 100, policy())
        .expect("synthetic query")
}

#[test]
fn catalogue_and_owned_results_can_move_to_a_worker() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Catalogue>();
    assert_send_sync::<SearchResults>();
}

fn assert_digit_query_results(catalogue: &mut Catalogue) {
    assert_groups_follow_entries(catalogue);
    let mut queries: Vec<String> = [
        "0", "111111", "001122", "é12", "\0\u{31}", "🧭９9", "project",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    queries.extend(["1".repeat(63), "1".repeat(64)]);
    for query in queries {
        for max_typos in [0, 1, 2] {
            let mut options = policy();
            options.max_typos = max_typos;
            for (scope, limit) in [
                (Scope::Everything, 0),
                (Scope::Everything, 3),
                (Scope::Tabs, 50),
                (Scope::Saved, 50),
                (Scope::History, 50),
                (Scope::Commands, 50),
            ] {
                options.limit = limit;
                assert_eq!(
                    catalogue.query(&query, scope, 100, options),
                    catalogue.query_exhaustive(&query, scope, 100, options),
                    "synthetic digit query {query:?}, budget {max_typos}, scope {scope:?}",
                );
            }
        }
    }
}

#[test]
fn digit_metadata_preserves_union_scopes_errors_and_mutation_atomicity() {
    let mut catalogue = Catalogue::new();
    for (index, (title, address)) in [
        ("11111", "111111"),
        ("001122", ""),
        ("é12", "1é2"),
        ("\0\u{31}", "🧭９9"),
        ("project", "https://digits.example.test/009"),
    ]
    .into_iter()
    .enumerate()
    {
        let id = index as u64 + 1;
        let mut row = entry(id, title, address);
        row.target = scoped_targets(id)[index % 4];
        catalogue.upsert(row).expect("synthetic digit row");
    }
    catalogue
        .upsert(entry(50, &"1".repeat(256), "012"))
        .expect("saturated digit field");
    assert_digit_query_results(&mut catalogue);

    // Replacing text and swapping the final row must keep both per-field counters aligned.
    catalogue
        .upsert(entry(1, "2222", "11"))
        .expect("replace first row");
    assert!(
        catalogue
            .remove(scoped_targets(2)[1])
            .expect("swap removal")
    );
    assert_digit_query_results(&mut catalogue);
    let mut changed = catalogue.get(target(50)).expect("saturated row").clone();
    changed.open = true;
    changed.usage.visits = u32::MAX;
    catalogue.upsert(changed).expect("metadata-only update");
    assert_digit_query_results(&mut catalogue);

    // Rejected and no-op mutations preserve every cached counter and result generation.
    catalogue.generation = u64::MAX;
    let before = catalogue.digit_counts.clone();
    assert_eq!(
        catalogue.upsert(entry(1, "9", "8")),
        Err(SearchError::GenerationExhausted)
    );
    assert_eq!(
        catalogue.remove(target(50)),
        Err(SearchError::GenerationExhausted)
    );
    let unchanged = catalogue.get(target(1)).expect("existing row").clone();
    assert_eq!(catalogue.upsert(unchanged), Ok(()));
    assert_eq!(catalogue.remove(target(999)), Ok(false));
    assert_eq!(catalogue.digit_counts, before);
    assert_digit_query_results(&mut catalogue);
}

#[test]
fn mutations_change_generation_only_when_metadata_changes() {
    let mut catalogue = Catalogue::new();
    assert!(catalogue.is_empty());
    assert_eq!(catalogue.generation(), 0);
    let original = entry(1, "Synthetic alpha", "https://example.test/alpha");
    catalogue.upsert(original.clone()).expect("insert");
    let original_groups = catalogue.field_groups.clone();
    catalogue.upsert(original.clone()).expect("unchanged row");
    assert_eq!(catalogue.field_groups, original_groups);
    assert_eq!(catalogue.generation(), 1);
    let mut updated = original;
    updated.title = "Synthetic beta".into();
    catalogue.upsert(updated.clone()).expect("update");
    assert_eq!(catalogue.get(target(1)), Some(&updated));
    assert_eq!(catalogue.generation(), 2);
    assert!(search(&mut catalogue, "beta").counts.total() == 1);
    let before_metadata = catalogue.field_groups.clone();
    let before_storage = catalogue.groups.storage_bytes();
    updated.open = true;
    updated.usage.visits = 7;
    updated.usage.last_used = Some(99);
    catalogue.upsert(updated).expect("metadata-only change");
    assert_eq!(catalogue.field_groups, before_metadata);
    assert_eq!(catalogue.groups.storage_bytes(), before_storage);
    assert_groups_follow_entries(&catalogue);
    assert!(!catalogue.remove(target(2)).expect("missing row"));
    assert_eq!(catalogue.generation(), 3);
    assert!(catalogue.remove(target(1)).expect("remove"));
    assert_eq!(catalogue.generation(), 4);
    assert!(catalogue.get(target(1)).is_none());
    assert!(search(&mut catalogue, "beta").hits.is_empty());
    assert_groups_follow_entries(&catalogue);
}

#[test]
fn swap_removal_keeps_other_targets_addressable() {
    let mut catalogue = Catalogue::new();
    for id in 1..=3 {
        catalogue
            .upsert(entry(id, "Synthetic row", ""))
            .expect("insert");
    }
    catalogue.remove(target(1)).expect("remove first row");
    catalogue
        .upsert(entry(3, "Updated third row", ""))
        .expect("update moved row");
    assert_eq!(catalogue.len(), 2);
    assert_eq!(
        catalogue.get(target(3)).expect("moved row").title,
        "Updated third row"
    );
    assert_eq!(search(&mut catalogue, "third").hits[0].target, target(3));
}

#[test]
fn scope_counts_include_every_match_before_filter_and_limit() {
    let targets = [
        target(1),
        Target::Saved(SavedPageId::new(1).expect("saved ID")),
        Target::History(NonZeroU64::new(1).expect("history ID")),
        Target::Command(NonZeroU64::new(1).expect("command ID")),
    ];
    let mut catalogue = Catalogue::new();
    for target in targets {
        let mut row = entry(1, "Synthetic marker", "https://example.test/marker");
        row.target = target;
        catalogue.upsert(row).expect("insert scoped row");
    }
    let mut limited = policy();
    limited.limit = 1;
    let result = catalogue
        .query("marker", Scope::Saved, 100, limited)
        .expect("query");
    assert_eq!(
        result.counts,
        ScopeCounts {
            tabs: 1,
            saved: 1,
            history: 1,
            commands: 1
        }
    );
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].target, targets[1]);
    assert_eq!(result.generation, catalogue.generation());
    limited.limit = 0;
    let result = catalogue
        .query("marker", Scope::Commands, 100, limited)
        .expect("zero limit");
    assert_eq!(result.counts.total(), 4);
    assert!(result.hits.is_empty());
}

#[test]
fn matching_both_fields_counts_one_target_and_uses_best_score() {
    let mut catalogue = Catalogue::new();
    catalogue
        .upsert(entry(1, "marker", "https://example.test/marker"))
        .expect("insert");
    let result = search(&mut catalogue, "marker");
    assert_eq!(result.counts.total(), 1);
    assert_eq!(result.hits[0].title_matches, vec![0..6]);
    assert!(!result.hits[0].address_matches.is_empty());
    let mut matcher = Matcher::new("marker", &matcher_config(0));
    let expected = matcher.match_one("marker", 0).expect("title match").score;
    assert_eq!(result.hits[0].score, u32::from(expected));
}

#[test]
fn fields_do_not_create_a_cross_field_match_or_duplicate_identity() {
    let mut catalogue = Catalogue::new();
    catalogue.upsert(entry(1, "abc", "def")).expect("insert");
    assert_eq!(search(&mut catalogue, "ad").counts.total(), 0);
    catalogue
        .upsert(entry(2, "abc", "def"))
        .expect("separate target");
    assert_eq!(search(&mut catalogue, "abc").counts.total(), 2);
}

#[test]
fn blank_behavior_is_explicit_and_whitespace_stays_literal() {
    let mut catalogue = Catalogue::new();
    catalogue.upsert(entry(1, "nospace", "")).expect("insert");
    catalogue
        .upsert(entry(2, "with space", ""))
        .expect("insert");
    assert_eq!(search(&mut catalogue, "").counts.total(), 2);
    assert_eq!(search(&mut catalogue, " ").counts.total(), 1);
    let mut no_blank = policy();
    no_blank.blank = BlankQuery::None;
    assert_eq!(
        catalogue
            .query("", Scope::Everything, 0, no_blank)
            .expect("blank")
            .counts
            .total(),
        0
    );
    assert!(
        search(&mut catalogue, "")
            .hits
            .iter()
            .all(|hit| hit.title_matches.is_empty())
    );
}

#[test]
fn literal_operators_are_not_interpreted_as_query_syntax() {
    let mut catalogue = Catalogue::new();
    catalogue.upsert(entry(1, "!save", "")).expect("insert");
    catalogue.upsert(entry(2, "save", "")).expect("insert");
    let result = search(&mut catalogue, "!save");
    assert_eq!(result.counts.total(), 1);
    assert_eq!(result.hits[0].target, target(1));
}

#[test]
fn ties_follow_target_order_after_removal_and_reinsertion() {
    let mut catalogue = Catalogue::new();
    for id in [3, 1, 2] {
        catalogue.upsert(entry(id, "same", "")).expect("insert");
    }
    catalogue.remove(target(1)).expect("remove");
    catalogue.upsert(entry(1, "same", "")).expect("reinsert");
    let targets: Vec<_> = search(&mut catalogue, "same")
        .hits
        .iter()
        .map(|hit| hit.target)
        .collect();
    assert_eq!(targets, vec![target(1), target(2), target(3)]);
}

#[test]
fn limited_selection_matches_the_prefix_of_complete_ranking() {
    let mut catalogue = Catalogue::new();
    for id in (1..=100).rev() {
        let mut row = entry(id, "Synthetic marker", "https://example.test/marker");
        row.usage.visits = id as u32;
        row.open = id % 3 == 0;
        catalogue.upsert(row).expect("insert");
    }
    let mut ranked = policy();
    ranked.limit = usize::MAX;
    ranked.boosts.open = 40;
    ranked.boosts.frequency = 320;
    let all = catalogue
        .query("marker", Scope::Everything, 0, ranked)
        .expect("all");
    ranked.limit = 7;
    let limited = catalogue
        .query("marker", Scope::Everything, 0, ranked)
        .expect("limited");
    assert_eq!(limited.hits, all.hits[..7]);
    assert_eq!(limited.counts, all.counts);
    assert!(
        all.hits
            .windows(2)
            .all(|pair| pair[0].score >= pair[1].score)
    );
}

#[test]
fn boosts_cannot_introduce_a_nonmatching_row() {
    let mut catalogue = Catalogue::new();
    catalogue.upsert(entry(1, "marker", "")).expect("insert");
    let mut other = entry(2, "unrelated", "");
    other.open = true;
    other.usage = Usage {
        visits: u32::MAX,
        last_used: Some(100),
    };
    catalogue.upsert(other).expect("insert");
    let mut boosted = policy();
    boosted.boosts.open = u16::MAX;
    boosted.boosts.frequency = u16::MAX;
    boosted.boosts.recency = u16::MAX;
    let result = catalogue
        .query("marker", Scope::Everything, 100, boosted)
        .expect("query");
    assert_eq!(result.counts.total(), 1);
    assert_eq!(result.hits[0].target, target(1));
}

#[test]
fn boosts_are_bounded_monotonic_and_handle_timestamp_extremes() {
    let mut row = entry(1, "synthetic", "");
    let weights = BoostWeights {
        open: u16::MAX,
        frequency: u16::MAX,
        recency: u16::MAX,
        recency_window_secs: NonZeroU64::new(u64::MAX).expect("nonzero window"),
    };
    let mut previous = 0;
    for visits in [0, 1, 2, 3, 100, u32::MAX] {
        row.usage.visits = visits;
        let score = boost(&row, 0, weights);
        assert!(score >= previous);
        assert!(score <= u32::from(weights.frequency));
        previous = score;
    }
    row.open = true;
    row.usage.last_used = Some(i64::MAX);
    assert_eq!(boost(&row, i64::MIN, weights), 3 * u32::from(u16::MAX));
    row.usage.last_used = Some(i64::MIN);
    assert_eq!(boost(&row, i64::MAX, weights), 2 * u32::from(u16::MAX));
    let short_window = policy().boosts;
    let short_window = BoostWeights {
        recency: 100,
        ..short_window
    };
    assert_eq!(recency_boost(50, 100, short_window), 50);
    assert_eq!(recency_boost(0, 100, short_window), 0);
    assert_eq!(recency_boost(200, 100, short_window), 100);
    assert_eq!(recency_boost(100, 100, short_window), 100);
    assert_eq!(recency_boost(1, 100, short_window), 1);
    assert_eq!(recency_boost(-1, 100, short_window), 0);

    // Compare the fast path with the original arithmetic across both signed extremes.
    let timestamps = [i64::MIN, -1, 0, 1, i64::MAX];
    let pairs: Vec<_> = timestamps
        .into_iter()
        .flat_map(|last_used| timestamps.map(|now| (last_used, now)))
        .collect();
    for recency in [0, 1, 2, 100, u16::MAX] {
        for window in [1, 100, i64::MAX as u64, i64::MAX as u64 + 1, u64::MAX] {
            let weights = BoostWeights {
                recency,
                recency_window_secs: NonZeroU64::new(window).expect("nonzero synthetic window"),
                ..weights
            };
            assert_recency_matches_original(weights, &pairs);
        }
    }

    // Adjacent products straddle the checked-multiplication boundary without changing the quotient.
    for recency in [2, 100, u16::MAX] {
        let threshold = u64::MAX / u64::from(recency);
        for remaining in [threshold - 1, threshold, threshold + 1] {
            let weights = BoostWeights {
                recency,
                recency_window_secs: NonZeroU64::new(remaining + 1)
                    .expect("nonzero synthetic window"),
                ..weights
            };
            assert_recency_matches_original(weights, &[(-1, 0)]);
        }
    }
}

fn assert_recency_matches_original(weights: BoostWeights, timestamps: &[(i64, i64)]) {
    for &(last_used, now) in timestamps {
        let age = (i128::from(now) - i128::from(last_used)).max(0) as u128;
        let window = u128::from(weights.recency_window_secs.get());
        let expected = (u128::from(weights.recency) * window.saturating_sub(age) / window) as u32;
        assert_eq!(
            recency_boost(last_used, now, weights),
            expected,
            "last_used {last_used}, now {now}, weight {}, window {window}",
            weights.recency
        );
    }
}

#[test]
fn unicode_highlights_preserve_original_utf8_and_scalar_boundaries() {
    let mut catalogue = Catalogue::new();
    catalogue
        .upsert(entry(1, "é😀x", "https://example.test/é😀x"))
        .expect("insert");
    let result = search(&mut catalogue, "éx");
    assert_eq!(result.hits[0].title, "é😀x");
    assert_eq!(result.hits[0].title_matches, vec![0..2, 6..7]);
    for hit in result.hits {
        for range in hit.title_matches {
            assert!(hit.title.get(range).is_some());
        }
        for range in hit.address_matches {
            assert!(hit.address.get(range).is_some());
        }
    }
    catalogue
        .upsert(entry(2, "研究書庫😀", ""))
        .expect("Japanese row");
    assert_eq!(
        search(&mut catalogue, "書庫").hits[0].title_matches,
        vec![6..12]
    );
}

#[test]
fn long_fields_and_queries_preserve_membership_and_original_highlight_offsets() {
    let mut catalogue = Catalogue::new();
    // The separated endpoints keep the scoring window beyond the greedy threshold.
    let separated = format!("a{}b", "_".repeat(1_100));
    catalogue
        .upsert(entry(1, &separated, "https://example.test/long"))
        .expect("long field");
    let result = search(&mut catalogue, "ab");
    assert_eq!(result.counts.total(), 1);
    assert_eq!(result.hits[0].title, separated);
    assert_eq!(result.hits[0].title_matches, vec![0..1, 1_101..1_102]);

    // Accepted long queries cross the same boundary without truncating the input.
    for length in [1_023, 1_024, 1_025] {
        let literal = "q".repeat(length);
        catalogue
            .upsert(entry(2, &literal, ""))
            .expect("long literal");
        let result = search(&mut catalogue, &literal);
        assert_eq!(result.counts.total(), 1);
        assert_eq!(result.hits[0].title, literal);
        assert_eq!(result.hits[0].title_matches, vec![0..length]);
    }

    let distant_unicode = format!("{}é😀x", "_".repeat(1_100));
    catalogue
        .upsert(entry(3, &distant_unicode, ""))
        .expect("Unicode suffix");
    let result = search(&mut catalogue, "éx");
    assert_eq!(result.counts.total(), 1);
    assert_eq!(result.hits[0].title, distant_unicode);
    assert_eq!(
        result.hits[0].title_matches,
        vec![1_100..1_102, 1_106..1_107]
    );
    assert!(
        result.hits[0]
            .title_matches
            .iter()
            .all(|range| { result.hits[0].title.get(range.clone()).is_some() })
    );
}

#[test]
fn accepted_near_match_recovers_real_highlights_without_changing_score_or_membership() {
    let fragment = "reproducible synthetic catalogue exploration includes detailed release notes and project reference material for keyboard navigation";
    let title = format!("cobalt example collection row-000162 {fragment} {fragment}");
    let query = format!("{}xyz", &fragment[..93]);
    let address = "https://catalogue-035.test/cobalt/reference/row-000162";
    let mut catalogue = Catalogue::new();
    catalogue
        .upsert(entry(163, &title, address))
        .expect("synthetic benchmark row");
    let mut one_typo = policy();
    one_typo.max_typos = 1;

    // The pinned engine accepts this row but its capped best trace is empty.
    let mut matcher = Matcher::new(query.as_str(), &matcher_config(1));
    let original = matcher
        .match_one_indices(title.as_str(), 0)
        .expect("accepted field");
    assert!(original.score > 0);
    assert!(original.indices.is_empty());
    assert!(matcher.match_one_indices(address, 0).is_none());

    let result = catalogue
        .query(&query, Scope::Everything, 0, one_typo)
        .expect("query");
    assert_eq!(result.counts.total(), 1);
    assert_eq!(result.hits[0].target, target(163));
    assert_eq!(result.hits[0].score, u32::from(original.score));
    assert_eq!(result.hits[0].title, title);
    assert_eq!(result.hits[0].address, address);
    assert!(!result.hits[0].title_matches.is_empty());
    assert!(result.hits[0].address_matches.is_empty());

    // The fallback publishes only character positions returned by the engine.
    matcher.set_config(matcher_config(0).max_typos(None));
    let trace = matcher
        .match_one_indices(title.as_str(), 0)
        .expect("uncapped trace");
    assert_eq!(
        result.hits[0].title_matches,
        highlights::ranges(&title, &trace.indices).expect("safe ranges")
    );
}

#[test]
fn zero_score_greedy_match_is_not_removed_when_no_trace_can_be_recovered() {
    let title = format!("a{}c", "_".repeat(1_100));
    let mut catalogue = Catalogue::new();
    catalogue
        .upsert(entry(1, &title, ""))
        .expect("synthetic long row");
    let mut one_typo = policy();
    one_typo.max_typos = 1;
    let result = catalogue
        .query("abc", Scope::Everything, 0, one_typo)
        .expect("greedy fallback");
    assert_eq!(result.counts.total(), 1);
    assert_eq!(result.hits[0].score, 0);
    assert_eq!(result.hits[0].title, title);
    assert!(result.hits[0].title_matches.is_empty());
    assert!(result.hits[0].address_matches.is_empty());
}

#[test]
fn smart_case_and_absence_of_normalization_are_visible_in_results() {
    let mut catalogue = Catalogue::new();
    catalogue.upsert(entry(1, "Project", "")).expect("insert");
    catalogue.upsert(entry(2, "project", "")).expect("insert");
    assert_eq!(search(&mut catalogue, "project").counts.total(), 2);
    assert_eq!(search(&mut catalogue, "Project").counts.total(), 1);
    catalogue
        .upsert(entry(3, "e\u{301}", ""))
        .expect("decomposed text");
    assert_eq!(search(&mut catalogue, "é").counts.total(), 0);
    assert_eq!(search(&mut catalogue, "e\u{301}").hits[0].target, target(3));
}

#[test]
fn typo_policy_is_explicit_and_extreme_values_do_not_overflow() {
    let mut catalogue = Catalogue::new();
    catalogue.upsert(entry(1, "marker", "")).expect("insert");
    assert_eq!(search(&mut catalogue, "marxer").counts.total(), 0);
    let mut fuzzy = policy();
    fuzzy.max_typos = 1;
    assert_eq!(
        catalogue
            .query("marxer", Scope::Everything, 0, fuzzy)
            .expect("one typo")
            .counts
            .total(),
        1
    );
    fuzzy.max_typos = u16::MAX;
    assert_eq!(
        catalogue
            .query("marker", Scope::Everything, 0, fuzzy)
            .expect("large typo budget")
            .counts
            .total(),
        1
    );
}

#[test]
fn query_limit_refuses_without_mutating_catalogue_or_truncating_text() {
    let mut catalogue = Catalogue::new();
    let row = entry(1, "Synthetic marker", "https://example.test");
    catalogue.upsert(row.clone()).expect("insert");
    let query = "x".repeat(Scoring::default().max_needle_len() + 1);
    assert_eq!(
        catalogue.query(&query, Scope::Everything, 0, policy()),
        Err(SearchError::QueryTooLong)
    );
    assert_eq!(catalogue.generation(), 1);
    assert_eq!(catalogue.get(target(1)), Some(&row));
    assert_eq!(search(&mut catalogue, "marker").counts.total(), 1);
}

#[test]
fn exhausted_generation_rejects_mutations_atomically_but_allows_no_ops() {
    let mut catalogue = Catalogue::new();
    let original = entry(1, "original", "https://example.test");
    catalogue.upsert(original.clone()).expect("insert");
    let original_ids = catalogue.field_groups.clone();
    let original_storage = catalogue.groups.storage_bytes();
    let original_slots = catalogue.groups.slots();
    catalogue.generation = u64::MAX;
    assert_eq!(
        catalogue.upsert(entry(1, "changed", "")),
        Err(SearchError::GenerationExhausted)
    );
    assert_eq!(
        catalogue.upsert(entry(2, "new", "")),
        Err(SearchError::GenerationExhausted)
    );
    assert_eq!(
        catalogue.remove(target(1)),
        Err(SearchError::GenerationExhausted)
    );
    assert_eq!(catalogue.get(target(1)), Some(&original));
    assert_eq!(catalogue.len(), 1);
    assert_eq!(catalogue.generation(), u64::MAX);
    assert_eq!(catalogue.upsert(original), Ok(()));
    assert_eq!(catalogue.remove(target(2)), Ok(false));
    assert_eq!(catalogue.field_groups, original_ids);
    assert_eq!(catalogue.groups.storage_bytes(), original_storage);
    assert_eq!(catalogue.groups.slots(), original_slots);
    assert_groups_follow_entries(&catalogue);

    // A test-only generation reset permits cleanup to expose leaked leases after refused writes.
    catalogue.generation = 0;
    assert!(
        catalogue
            .remove(target(1))
            .expect("remove preserved synthetic row")
    );
    assert_groups_follow_entries(&catalogue);
}

#[test]
fn matcher_limits_are_checked_without_allocating_huge_strings() {
    assert_eq!(
        validate_field_len(u32::MAX as usize, TextField::Title),
        Ok(())
    );
    assert_eq!(
        validate_field_len(u32::MAX as usize + 1, TextField::Address),
        Err(SearchError::FieldTooLong(TextField::Address))
    );
}

#[test]
fn zero_score_matches_remain_eligible() {
    let titles = [Match {
        score: 0,
        index: 0,
        exact: false,
    }];
    assert_eq!(MergedMatches::new(&titles, &[]).next(), Some((0, 0)));
}

#[test]
fn diagnostics_redact_text_and_usage_metadata() {
    let mut catalogue = Catalogue::new();
    let mut row = entry(
        1,
        "private-synthetic-marker",
        "https://redaction.invalid/secret",
    );
    row.usage = Usage {
        visits: 123_456,
        last_used: Some(987_654_321),
    };
    let entry_debug = format!("{row:?} {:?}", row.usage);
    catalogue.upsert(row).expect("insert");
    let result = search(&mut catalogue, "private-synthetic-marker");
    let debug = format!("{entry_debug} {catalogue:?} {result:?} {:?}", result.hits);
    for excluded in [
        "private-synthetic-marker",
        "redaction.invalid",
        "secret",
        "123456",
        "987654321",
    ] {
        assert!(!debug.contains(excluded));
    }
}

fn compare_to_exhaustive(
    catalogue: &mut Catalogue,
    query: &str,
    scope: Scope,
    now: i64,
    options: SearchPolicy,
) -> SearchResults {
    let optimized = catalogue.query(query, scope, now, options);
    // Score every field with the pinned matcher, bypassing masks, membership and score pruning.
    let exhaustive = catalogue.query_exhaustive(query, scope, now, options);
    if optimized.is_err() || exhaustive.is_err() {
        panic!(
            "synthetic query {query:?}, scope {scope:?}, policy {options:?}; filtered={optimized:?}; exhaustive={exhaustive:?}; field diagnostics: {}",
            synthetic_trace_failures(catalogue, query, options.max_typos)
        );
    }
    let optimized = optimized.expect("filtered synthetic query must succeed");
    let exhaustive = exhaustive.expect("exhaustive synthetic query must succeed");
    assert_eq!(
        optimized, exhaustive,
        "synthetic query {query:?}, scope {scope:?}, policy {options:?}"
    );
    optimized
}

fn synthetic_trace_failures(catalogue: &Catalogue, query: &str, max_typos: u16) -> String {
    // Only generated rows reach this oracle; never print the matcher's content-bearing Debug.
    let mut matcher = Matcher::new(query, &matcher_config(max_typos));
    let mut fallback = None;
    let mut failures = Vec::new();
    for row in &catalogue.entries {
        for (field, text) in [
            ("title", row.title.as_str()),
            ("address", row.address.as_str()),
        ] {
            let found = matcher.match_one_indices(text, 0);
            if let Err(error) = field_ranges(found.clone(), text, query, &mut fallback) {
                let fallback_trace = fallback
                    .as_mut()
                    .and_then(|matcher| matcher.match_one_indices(text, 0));
                failures.push(format!("target={:?}, field={field}, bytes={}, error={error:?}, original={found:?}, fallback={fallback_trace:?}", row.target, text.len()));
            }
        }
    }
    failures.join("; ")
}

fn varied_filter_catalogue() -> Catalogue {
    let fields = [
        ("needle", "zzzz"),
        ("zzzz", "needle"),
        ("needle", "needle"),
        ("aaaa", "bbbb"),
        ("", ""),
        ("AB ab Aa", "https://ascii.example.test/A"),
        ("!a |b ^a$", "a b"),
        ("\0\u{7f}a", "\u{7f}"),
        ("café needle", "ascii"),
        ("ascii", "cafe\u{301} needle"),
        ("東京😀", "https://unicode.example.test/é"),
        ("Straße", "SS"),
        ("aaaaaa", "z"),
        ("ba", "ab"),
    ];
    let mut catalogue = Catalogue::new();
    for (index, (title, address)) in fields.into_iter().enumerate() {
        let number = index as u64 + 1;
        let mut row = entry(number, title, address);
        row.target = match index % 4 {
            0 => target(number),
            1 => Target::Saved(SavedPageId::new(number).expect("synthetic saved ID")),
            2 => Target::History(NonZeroU64::new(number).expect("synthetic history ID")),
            _ => Target::Command(NonZeroU64::new(number).expect("synthetic command ID")),
        };
        row.open = index % 2 == 0;
        row.usage.visits = [0, 1, 31, u32::MAX][index % 4];
        row.usage.last_used = [None, Some(i64::MIN), Some(100), Some(i64::MAX)][index % 4];
        catalogue.upsert(row).expect("insert varied synthetic row");
    }
    catalogue
}

#[test]
fn ascii_filter_matches_exhaustive_scoring_counts_ranking_and_highlights() {
    let mut catalogue = varied_filter_catalogue();
    let queries = [
        "",
        "a",
        "A",
        "ab",
        "AB",
        "aaaa",
        "az",
        "!a",
        "a b",
        "needle",
        "NEEDLE",
        "é",
        "e\u{301}",
        "東京",
        "😀",
        "SS",
        "\0",
        "\u{7f}",
        "notpresent",
    ];
    let scopes = [
        Scope::Everything,
        Scope::Tabs,
        Scope::Saved,
        Scope::History,
        Scope::Commands,
    ];
    for query in queries {
        // Include complete deletion budgets: even an empty field must reach the pinned matcher.
        let mut variants = [policy(); 4];
        variants[0].limit = 0;
        variants[1].max_typos = 1;
        variants[1].limit = 1;
        variants[1].boosts = BoostWeights {
            open: u16::MAX,
            frequency: u16::MAX,
            recency: u16::MAX,
            recency_window_secs: NonZeroU64::new(u64::MAX).expect("nonzero synthetic window"),
        };
        variants[2].max_typos =
            u16::try_from(query.chars().count()).expect("short synthetic query");
        variants[2].limit = usize::MAX;
        variants[3].max_typos = u16::MAX;
        variants[3].blank = BlankQuery::None;
        for (index, variant) in variants.into_iter().enumerate() {
            let now = [0, i64::MIN, 100, i64::MAX][index];
            for scope in scopes {
                compare_to_exhaustive(&mut catalogue, query, scope, now, variant);
            }
        }
    }
}

#[test]
fn ascii_filter_covers_every_ascii_bit_without_changing_matches() {
    let mut catalogue = varied_filter_catalogue();
    for byte in 0..=127_u8 {
        let query = char::from(byte).to_string();
        for typos in [0, 1] {
            let mut options = policy();
            options.max_typos = typos;
            compare_to_exhaustive(&mut catalogue, &query, Scope::Everything, 100, options);
        }
    }
}

#[test]
fn independently_filtered_fields_remap_to_original_rows_before_union() {
    let mut catalogue = Catalogue::new();
    for (id, title, address) in [
        (9, "needle", "zzzz"),
        (3, "zzzz", "needle"),
        (7, "needle", "needle"),
        (1, "zzzz", "zzzz"),
    ] {
        catalogue
            .upsert(entry(id, title, address))
            .expect("insert synthetic mapping row");
    }
    compare_to_exhaustive(&mut catalogue, "needle", Scope::Everything, 0, policy());
    let result = search(&mut catalogue, "needle");
    assert_eq!(result.counts.total(), 3);
    assert_eq!(
        result.hits.iter().map(|hit| hit.target).collect::<Vec<_>>(),
        vec![target(3), target(7), target(9)]
    );
    let address_only = result
        .hits
        .iter()
        .find(|hit| hit.target == target(3))
        .expect("address-only row");
    assert!(address_only.title_matches.is_empty());
    assert!(!address_only.address_matches.is_empty());
    let title_only = result
        .hits
        .iter()
        .find(|hit| hit.target == target(9))
        .expect("title-only row");
    assert!(!title_only.title_matches.is_empty());
    assert!(title_only.address_matches.is_empty());
}

fn compare_mutated_catalogue(catalogue: &mut Catalogue) {
    assert_groups_follow_entries(catalogue);
    for query in [
        "alpha", "ALPHA", "beta", "gamma", "zeta", "é", "xyz", "_aB_aB", "",
    ] {
        for typos in [0, 1, 4] {
            let mut options = policy();
            options.max_typos = typos;
            options.limit = 2;
            compare_to_exhaustive(catalogue, query, Scope::Everything, 100, options);
        }
    }
}

#[test]
fn filter_metadata_tracks_updates_swap_removal_and_reinsertions_atomically() {
    let mut catalogue = Catalogue::new();
    for (id, title, address) in [(1, "alpha", ""), (2, "beta", "é"), (3, "gamma", "xyz")] {
        catalogue
            .upsert(entry(id, title, address))
            .expect("insert synthetic mutable row");
    }
    compare_mutated_catalogue(&mut catalogue);
    catalogue
        .upsert(entry(2, "é", "ALPHA"))
        .expect("change each field's ASCII status");
    compare_mutated_catalogue(&mut catalogue);
    assert!(catalogue.remove(target(1)).expect("swap-remove first row"));
    compare_mutated_catalogue(&mut catalogue);
    catalogue
        .upsert(entry(3, "zeta", ""))
        .expect("update moved last row");
    compare_mutated_catalogue(&mut catalogue);
    catalogue
        .upsert(entry(1, "alpha", "gamma"))
        .expect("reinsert removed target");
    compare_mutated_catalogue(&mut catalogue);
    assert!(catalogue.remove(target(2)).expect("swap-remove middle row"));
    compare_mutated_catalogue(&mut catalogue);
    assert!(catalogue.remove(target(1)).expect("remove last row"));
    compare_mutated_catalogue(&mut catalogue);
    catalogue
        .upsert(entry(2, "beta", "é"))
        .expect("restore synthetic row");
    catalogue
        .upsert(entry(1, "alpha", "gamma"))
        .expect("restore another synthetic row");
    compare_mutated_catalogue(&mut catalogue);
    catalogue
        .upsert(entry(3, "_aB_aB", "alpha_beta"))
        .expect("replace cached delimiter and capitalization counts");
    compare_mutated_catalogue(&mut catalogue);

    // Failed mutation must preserve cached character presence as well as visible entry metadata.
    catalogue.generation = u64::MAX;
    let before = catalogue
        .get(target(3))
        .expect("synthetic row exists")
        .clone();
    assert_eq!(
        catalogue.upsert(entry(3, "qqqq", "mmmm")),
        Err(SearchError::GenerationExhausted)
    );
    assert_eq!(
        catalogue.remove(target(3)),
        Err(SearchError::GenerationExhausted)
    );
    assert_eq!(
        catalogue.upsert(entry(4, "new", "row")),
        Err(SearchError::GenerationExhausted)
    );
    assert_eq!(catalogue.get(target(3)), Some(&before));
    assert_eq!(catalogue.upsert(before), Ok(()));
    assert_eq!(catalogue.remove(target(99)), Ok(false));
    compare_mutated_catalogue(&mut catalogue);
    assert_eq!(catalogue.generation(), u64::MAX);

    let too_long = "a".repeat(Scoring::default().max_needle_len() + 1);
    assert_eq!(
        catalogue.query(&too_long, Scope::Everything, 0, policy()),
        Err(SearchError::QueryTooLong)
    );
    compare_mutated_catalogue(&mut catalogue);
}

fn scoped_targets(id: u64) -> [Target; 4] {
    [
        target(id),
        Target::Saved(SavedPageId::new(id).expect("synthetic saved ID")),
        Target::History(NonZeroU64::new(id).expect("synthetic history ID")),
        Target::Command(NonZeroU64::new(id).expect("synthetic command ID")),
    ]
}

fn boosted_pruning_catalogue() -> Catalogue {
    let mut catalogue = Catalogue::new();
    // Reverse insertion order makes the strongest equal-score targets arrive last.
    for id in (1..=80).rev() {
        let (title, address) = match id % 3 {
            0 => ("marker", ""),
            1 => ("", "marker"),
            _ => ("marker", "marker"),
        };
        for scoped_target in scoped_targets(id).into_iter().rev() {
            let mut row = entry(id, title, address);
            row.target = scoped_target;
            row.open = id == 1 || id % 3 == 0;
            row.usage = Usage {
                visits: if id == 1 || id % 5 == 0 {
                    u32::MAX
                } else {
                    id as u32
                },
                last_used: Some(if id == 1 { 100 } else { id as i64 }),
            };
            catalogue.upsert(row).expect("insert boosted synthetic row");
        }
    }
    // A maximum boost must never admit a row rejected by both independent fields.
    for scoped_target in scoped_targets(999) {
        let mut row = entry(999, "zzzz", "");
        row.target = scoped_target;
        row.open = true;
        row.usage = Usage {
            visits: u32::MAX,
            last_used: Some(i64::MAX),
        };
        catalogue.upsert(row).expect("insert boosted nonmatch");
    }
    catalogue
}

#[test]
fn score_pruning_preserves_boosted_rank_target_ties_and_all_scope_counts() {
    let mut catalogue = boosted_pruning_catalogue();
    let mut options = policy();
    options.boosts.open = u16::MAX;
    options.boosts.frequency = u16::MAX;
    options.boosts.recency = u16::MAX;
    let expected_counts = ScopeCounts {
        tabs: 80,
        saved: 80,
        history: 80,
        commands: 80,
    };

    // The two-typo policy also checks the full-matcher fallback, with the same rank contract.
    for max_typos in [0, 1, 2] {
        options.max_typos = max_typos;
        for scope in [Scope::Everything, Scope::Saved, Scope::Commands] {
            for limit in [0, 1, 7, usize::MAX] {
                options.limit = limit;
                let result = compare_to_exhaustive(&mut catalogue, "marker", scope, 100, options);
                assert_eq!(result.counts, expected_counts);
                let selected_count = if scope == Scope::Everything { 320 } else { 80 };
                assert_eq!(result.hits.len(), limit.min(selected_count));
                if let Some(first) = result.hits.first() {
                    let expected = match scope {
                        Scope::Everything => scoped_targets(1)[0],
                        Scope::Saved => scoped_targets(1)[1],
                        Scope::Commands => scoped_targets(1)[3],
                        _ => unreachable!("only the three synthetic scopes are queried"),
                    };
                    assert_eq!(first.target, expected);
                }
                if scope == Scope::Everything && limit >= 4 {
                    let tied_targets: Vec<_> =
                        result.hits[..4].iter().map(|hit| hit.target).collect();
                    assert_eq!(tied_targets, scoped_targets(1));
                    assert!(
                        result.hits[..4]
                            .windows(2)
                            .all(|pair| pair[0].score == pair[1].score)
                    );
                }
            }
        }
    }
}

#[test]
fn membership_pruning_preserves_unicode_empty_fields_and_greedy_zero_scores() {
    let mut catalogue = Catalogue::new();
    let separated = format!("a{}b", "_".repeat(1_100));
    let zero_score = format!("a{}c", "_".repeat(1_100));
    for (id, title, address) in [
        (1, "café", ""),
        (2, "", "研究書庫😀"),
        (3, "", ""),
        (4, separated.as_str(), ""),
        (5, zero_score.as_str(), ""),
        (6, "Straße", "SS"),
        (7, "e\u{301}", "é"),
    ] {
        let mut row = entry(id, title, address);
        row.target = scoped_targets(id)[id as usize % 4];
        catalogue.upsert(row).expect("insert boundary row");
    }
    let cases = [
        ("café", 0, Scope::Everything, 1),
        ("cafx", 1, Scope::Everything, usize::MAX),
        ("書庫😀", 0, Scope::History, 1),
        ("e\u{301}", 0, Scope::Everything, 7),
        ("SS", 0, Scope::Saved, 0),
        ("ab", 0, Scope::Everything, 1),
        ("abc", 1, Scope::Everything, usize::MAX),
        ("abc", 2, Scope::Saved, 1),
        ("x", 1, Scope::Everything, usize::MAX),
        ("", 0, Scope::Everything, 1),
    ];
    for (query, max_typos, scope, limit) in cases {
        let mut options = policy();
        options.max_typos = max_typos;
        options.limit = limit;
        let result = compare_to_exhaustive(&mut catalogue, query, scope, 100, options);
        if query == "abc" && max_typos == 1 {
            let accepted = result
                .hits
                .iter()
                .find(|hit| hit.target == scoped_targets(5)[1])
                .expect("zero-score greedy candidate remains eligible");
            assert_eq!(accepted.score, 0);
            assert!(accepted.title_matches.is_empty());
            assert!(accepted.address_matches.is_empty());
        }
    }
}

#[test]
fn score_bound_covers_adjacent_delimiter_and_capitalization_bonuses() {
    for repeats in [2, 4, 16, 128, 1_170] {
        let literal = "_aB".repeat(repeats);
        let mut matcher = Matcher::new(literal.as_str(), &matcher_config(0));
        let actual = matcher
            .match_one(literal.as_str(), 0)
            .expect("exact synthetic alternating match")
            .score;
        if repeats == 2 {
            assert_eq!(actual, 132);
            assert!(usize::from(actual) > 18 * literal.len() + 22);
        }
        if repeats == 1_170 {
            assert_eq!(actual, u16::MAX);
        }
        assert!(actual <= score_upper_bound(literal.len()));
        assert_field_bound(&literal, &literal, 0);
    }
}

#[test]
fn saturated_greedy_scores_keep_exact_target_ties_at_the_pruning_bound() {
    let literal = "_aB".repeat(1_170);
    assert_eq!(literal.len(), 3_510);
    assert!(literal.len() <= Scoring::default().max_needle_len());
    let mut catalogue = Catalogue::new();
    for scoped_target in scoped_targets(1).into_iter().rev() {
        let mut row = entry(1, &literal, "");
        row.target = scoped_target;
        catalogue
            .upsert(row)
            .expect("insert saturated synthetic row");
    }
    for limit in [1, 2, usize::MAX] {
        let mut options = policy();
        options.limit = limit;
        let result = compare_to_exhaustive(&mut catalogue, &literal, Scope::Everything, 0, options);
        let expected: Vec<_> = scoped_targets(1).into_iter().take(limit).collect();
        assert_eq!(result.counts.total(), 4);
        assert_eq!(
            result.hits.iter().map(|hit| hit.target).collect::<Vec<_>>(),
            expected
        );
        for hit in result.hits {
            assert_eq!(hit.score, u32::from(u16::MAX));
            assert_eq!(
                hit.title_matches.as_slice(),
                std::slice::from_ref(&(0..literal.len()))
            );
        }
    }
}

fn assert_field_bound(query: &str, field: &str, max_typos: u16) {
    // Identical title/address fields let the public cached row bound test one field exactly.
    let row = entry(1, field, field);
    let cached = FieldMasks::new(&row);
    let upper = cached.score_upper_bound(&row, &QueryBound::new(query));
    let mut matcher = Matcher::new(query, &matcher_config(max_typos));
    let actual = matcher
        .match_one(field, 0)
        .expect("synthetic bound fixture must match");
    assert!(
        actual.score <= upper,
        "synthetic query bytes {}, field bytes {}, typos {max_typos}: score {} exceeds bound {upper}",
        query.len(),
        field.len(),
        actual.score
    );
    assert_eq!(actual.exact, query == field);
}

#[test]
fn field_bounds_preserve_original_exact_bonus_and_adjacent_boundaries() {
    let cases = [
        ("marker", "marker"),
        ("marker", "marker-suffix"),
        ("marker", "prefix-marker"),
        ("marker", "prefix-marker-suffix"),
        ("_aB_aB", "_aB_aB"),
        ("_aB_aB", "prefix_aB_aB_suffix"),
        ("ab", "aB"),
        ("aB", "aB"),
        ("aB", "prefix_aB_suffix"),
        ("éB", "prefix_éB_suffix"),
    ];
    let mut catalogue = Catalogue::new();
    for (index, (_, field)) in cases.iter().enumerate() {
        let mut row = entry(index as u64 + 1, field, "");
        row.open = index % 3 == 0;
        catalogue.upsert(row).expect("insert synthetic bound field");
    }
    for (query, field) in cases {
        for max_typos in [0, 1] {
            assert_field_bound(query, field, max_typos);
            let mut options = policy();
            options.limit = 1;
            options.max_typos = max_typos;
            options.boosts.open = 25;
            compare_to_exhaustive(&mut catalogue, query, Scope::Everything, 0, options);
        }
    }
}

#[test]
fn field_bounds_cover_unicode_scoring_on_both_sides_of_the_greedy_cutoff() {
    let mut catalogue = Catalogue::new();
    let fields: Vec<_> = [1_023, 1_024, 1_025]
        .into_iter()
        .map(|bytes| format!("é{}😀", "q".repeat(bytes - 6)))
        .collect();
    for (index, field) in fields.iter().enumerate() {
        assert_eq!(field.len(), 1_023 + index);
        catalogue
            .upsert(entry(index as u64 + 1, field, ""))
            .expect("insert Unicode cutoff field");
    }
    // A long original field can still be trimmed to a small Unicode scoring window.
    let trimmed = format!("{}_é😀_{}", "x".repeat(1_100), "z".repeat(1_100));
    catalogue
        .upsert(entry(4, "", &trimmed))
        .expect("insert distant Unicode address match");
    assert_field_bound("é😀", &trimmed, 0);

    for field in &fields {
        for query in [field.as_str(), "é😀"] {
            assert_field_bound(query, field, 0);
            for limit in [1, usize::MAX] {
                let mut options = policy();
                options.limit = limit;
                compare_to_exhaustive(&mut catalogue, query, Scope::Everything, 0, options);
            }
        }
    }
}

fn equivalent_digit_catalogue() -> Catalogue {
    let mut catalogue = Catalogue::new();
    let targets = [scoped_targets(1), scoped_targets(2)].concat();
    let suffixes = ["012", "123", "234", "345", "456", "567", "678", "789"];
    // Reverse insertion ensures equal-score ordering comes from Target, not the representative.
    for (target, suffix) in targets.into_iter().zip(suffixes).rev() {
        let mut row = entry(
            1,
            &format!("marker{suffix}"),
            &format!("https://catalogue.example.test/marker{suffix}"),
        );
        row.target = target;
        row.open = target == scoped_targets(1)[0];
        if target == scoped_targets(1)[1] {
            row.usage.visits = u32::MAX;
        }
        if target == scoped_targets(1)[2] {
            row.usage.last_used = Some(100);
        }
        catalogue.upsert(row).expect("insert equivalent-digit row");
    }
    catalogue
}

fn equivalent_digit_policy() -> SearchPolicy {
    let mut options = policy();
    options.limit = usize::MAX;
    options.boosts.open = 50;
    options.boosts.frequency = 32;
    options.boosts.recency = 20;
    options
}

#[test]
fn equivalent_field_scores_keep_distinct_targets_counts_boosts_ties_and_highlights() {
    let mut catalogue = equivalent_digit_catalogue();
    let mut options = equivalent_digit_policy();
    let result = compare_to_exhaustive(&mut catalogue, "marker", Scope::Everything, 100, options);
    let first = scoped_targets(1);
    let second = scoped_targets(2);
    assert_eq!(
        result.hits.iter().map(|hit| hit.target).collect::<Vec<_>>(),
        [
            first[0], first[1], first[2], second[0], second[1], second[2], first[3], second[3]
        ]
    );
    let base = result.hits[3].score;
    assert_eq!(result.hits[0].score, base + 50);
    assert_eq!(result.hits[1].score, base + 32);
    assert_eq!(result.hits[2].score, base + 20);

    // The original row text and both original highlight sets survive equivalent-score reuse.
    for hit in &result.hits {
        let row = catalogue.get(hit.target).expect("original synthetic row");
        assert_eq!(hit.title, row.title);
        assert_eq!(hit.address, row.address);
        assert_eq!(hit.title_matches, vec![0..6]);
        let address_start = row
            .address
            .find("marker")
            .expect("synthetic address marker");
        assert_eq!(hit.address_matches, vec![address_start..address_start + 6]);
    }
    let expected_counts = ScopeCounts {
        tabs: 2,
        saved: 2,
        history: 2,
        commands: 2,
    };
    for scope in [
        Scope::Everything,
        Scope::Tabs,
        Scope::Saved,
        Scope::History,
        Scope::Commands,
    ] {
        for limit in [0, 1, usize::MAX] {
            options.limit = limit;
            let result = compare_to_exhaustive(&mut catalogue, "marker", scope, 100, options);
            assert_eq!(result.counts, expected_counts);
            assert_eq!(
                result.hits.len(),
                limit.min(if scope == Scope::Everything { 8 } else { 2 })
            );
        }
    }
}

#[test]
fn equivalent_scores_do_not_survive_changed_queries_metadata_or_replaced_rows() {
    let mut catalogue = equivalent_digit_catalogue();
    let mut options = equivalent_digit_policy();
    let before = compare_to_exhaustive(&mut catalogue, "marker", Scope::Everything, 100, options);
    assert_eq!(before.counts.total(), 8);
    let digits = compare_to_exhaustive(&mut catalogue, "012", Scope::Everything, 100, options);
    assert_eq!(digits.counts.total(), 1);
    assert_eq!(digits.hits[0].target, target(1));
    assert_eq!(digits.hits[0].title_matches, vec![6..9]);

    // Replacing a previous representative changes membership without retaining its old base score.
    catalogue
        .upsert(entry(
            1,
            "other012",
            "https://catalogue.example.test/other012",
        ))
        .expect("replace equivalent-digit row");
    let changed = compare_to_exhaustive(&mut catalogue, "marker", Scope::Everything, 100, options);
    assert_eq!(changed.counts.total(), 7);
    assert_eq!(changed.generation, before.generation + 1);
    assert!(!changed.hits.iter().any(|hit| hit.target == target(1)));
    let digits = compare_to_exhaustive(&mut catalogue, "012", Scope::Everything, 100, options);
    assert_eq!(digits.counts.total(), 1);
    assert_eq!(digits.hits[0].title, "other012");
    assert_eq!(digits.hits[0].title_matches, vec![5..8]);

    // Removal/reinsertion, changed boosts/time, and changed typo policy all get fresh query state.
    let saved = scoped_targets(2)[1];
    assert!(catalogue.remove(saved).expect("remove synthetic saved row"));
    let mut replacement = entry(2, "marker987", "https://catalogue.example.test/marker987");
    replacement.target = saved;
    replacement.open = true;
    catalogue
        .upsert(replacement)
        .expect("reinsert synthetic saved row");
    options.boosts.open = 75;
    options.max_typos = 1;
    compare_to_exhaustive(&mut catalogue, "markez", Scope::Everything, 200, options);
    options.max_typos = 0;
    let result = compare_to_exhaustive(&mut catalogue, "marker", Scope::Everything, 200, options);
    assert_eq!(result.counts.total(), 7);
    assert_eq!(result.hits[0].target, saved);
    assert_eq!(result.hits[0].title, "marker987");
    assert_eq!(result.generation, before.generation + 3);

    // Reusing a released slot must not reuse the previous query's membership result.
    let targets: Vec<_> = catalogue.entries.iter().map(|row| row.target).collect();
    for removed in targets.into_iter().filter(|removed| *removed != target(1)) {
        assert!(
            catalogue
                .remove(removed)
                .expect("remove other synthetic rows")
        );
    }
    catalogue
        .upsert(entry(1, "alpha1", "unrelated"))
        .expect("prepare unique group");
    let alpha = compare_to_exhaustive(&mut catalogue, "alpha", Scope::Everything, 0, policy());
    assert_eq!(alpha.counts.total(), 1);
    let old_group = catalogue.field_groups[0][0].expect("unique title group");
    catalogue
        .upsert(entry(1, "zeta2", "unrelated"))
        .expect("reuse title slot");
    assert_eq!(catalogue.field_groups[0][0], Some(old_group));
    let alpha = compare_to_exhaustive(&mut catalogue, "alpha", Scope::Everything, 0, policy());
    assert_eq!(alpha.counts.total(), 0);
    let zeta = compare_to_exhaustive(&mut catalogue, "zeta", Scope::Everything, 0, policy());
    assert_eq!(zeta.counts.total(), 1);
    assert_groups_follow_entries(&catalogue);
}

fn assert_groups_follow_entries(catalogue: &Catalogue) {
    assert_eq!(catalogue.digit_counts.len(), catalogue.entries.len());
    for (entry, counts) in catalogue.entries.iter().zip(&catalogue.digit_counts) {
        assert_eq!(
            *counts,
            [
                DigitCounts::new(&entry.title),
                DigitCounts::new(&entry.address)
            ]
        );
    }
    assert_eq!(catalogue.entries.len(), catalogue.field_groups.len());
    let mut retained = std::collections::HashSet::new();
    let mut bytes = 0;
    for (row, fields) in catalogue.entries.iter().zip(&catalogue.field_groups) {
        for (original, id) in [row.title.as_str(), row.address.as_str()]
            .into_iter()
            .zip(fields)
        {
            let Some(id) = id else { continue };
            let expected: String = original
                .chars()
                .map(|character| {
                    if character.is_ascii_digit() {
                        '0'
                    } else {
                        character
                    }
                })
                .collect();
            assert_eq!(catalogue.groups.get(*id), Some(expected.as_str()));
            if retained.insert(*id) {
                bytes += expected.len();
            }
        }
    }
    assert_eq!(catalogue.groups.len(), retained.len());
    assert_eq!(catalogue.groups.storage_bytes(), bytes);
    assert!(catalogue.groups.slots() <= 2_048);
}

#[test]
fn shared_field_groups_retain_each_title_and_address_lease_until_removed() {
    let mut catalogue = Catalogue::new();
    catalogue
        .upsert(entry(1, "a1z", "a2z"))
        .expect("insert shared fields");
    catalogue
        .upsert(entry(2, "a3z", "other"))
        .expect("insert shared title");
    let shared = catalogue.field_groups[0][0].expect("shared group");
    assert_eq!(catalogue.field_groups[0], [Some(shared), Some(shared)]);
    assert_eq!(catalogue.field_groups[1][0], Some(shared));
    assert_eq!(catalogue.groups.len(), 2);
    compare_to_exhaustive(&mut catalogue, "az", Scope::Everything, 0, policy());

    catalogue
        .upsert(entry(1, "new", "a2z"))
        .expect("change title only");
    assert_eq!(catalogue.field_groups[0][1], Some(shared));
    assert_eq!(catalogue.field_groups[1][0], Some(shared));
    assert_eq!(catalogue.groups.len(), 3);
    assert_groups_follow_entries(&catalogue);
    let other_title = catalogue.field_groups[1][0];
    catalogue
        .upsert(entry(2, "a3z", "different"))
        .expect("change address only");
    assert_eq!(catalogue.field_groups[1][0], other_title);
    assert_groups_follow_entries(&catalogue);

    // Swap removal moves the remaining field IDs along with their row, without releasing them.
    let remaining = catalogue.field_groups[1];
    assert!(
        catalogue
            .remove(target(1))
            .expect("remove first shared row")
    );
    assert_eq!(catalogue.field_groups, vec![remaining]);
    assert_eq!(catalogue.groups.get(shared), Some("a0z"));
    assert_eq!(catalogue.groups.len(), 2);
    let result = compare_to_exhaustive(&mut catalogue, "az", Scope::Everything, 0, policy());
    assert_eq!(result.counts.total(), 1);
    assert_eq!(result.hits[0].target, target(2));
    assert!(
        catalogue
            .remove(target(2))
            .expect("release last shared row")
    );
    assert_eq!(catalogue.groups.get(shared), None);
    assert_groups_follow_entries(&catalogue);
}

fn capacity_fields(number: u64) -> (String, String) {
    let code: String = [number % 26, number / 26 % 26, number / (26 * 26) % 26]
        .into_iter()
        .map(|letter| char::from(b'a' + letter as u8))
        .collect();
    (
        format!("title-{code}"),
        format!("https://capacity.example.test/{code}"),
    )
}

#[test]
fn full_group_pool_and_overlength_fields_keep_fallback_rows_searchable() {
    let mut catalogue = Catalogue::new();
    for id in 1..=1_024 {
        let (title, address) = capacity_fields(id);
        catalogue
            .upsert(entry(id, &title, &address))
            .expect("fill synthetic groups");
    }
    assert_eq!(catalogue.groups.len(), 2_048);
    let existing = catalogue.field_groups[0];
    let (title, address) = capacity_fields(1);
    catalogue
        .upsert(entry(1_025, &title, "needle999"))
        .expect("insert capped fallback");
    assert_eq!(catalogue.field_groups[1_024], [existing[0], None]);
    let long = format!("needle{}", "q".repeat(1_019));
    assert_eq!(long.len(), 1_025);
    catalogue
        .upsert(entry(1_026, &long, &address))
        .expect("insert overlength fallback");
    assert_eq!(catalogue.field_groups[1_025], [None, existing[1]]);
    let result = compare_to_exhaustive(&mut catalogue, "needle", Scope::Everything, 0, policy());
    assert_eq!(result.counts.total(), 2);
    assert_groups_follow_entries(&catalogue);

    // Metadata-only changes preserve fallback IDs even after unrelated group capacity is freed.
    let released = catalogue.field_groups[1];
    assert!(catalogue.remove(target(2)).expect("free two unique groups"));
    let fallback_position = catalogue.positions[&target(1_025)];
    let before_metadata = catalogue.field_groups[fallback_position];
    let mut metadata = catalogue.get(target(1_025)).expect("fallback row").clone();
    metadata.open = true;
    catalogue
        .upsert(metadata)
        .expect("update fallback metadata");
    assert_eq!(catalogue.field_groups[fallback_position], before_metadata);
    assert_eq!(catalogue.groups.len(), 2_046);

    catalogue
        .upsert(entry(1_025, &title, "needle888"))
        .expect("intern changed fallback");
    let restored = catalogue.field_groups[fallback_position][1].expect("reclaimed group slot");
    assert!(released.contains(&Some(restored)));
    catalogue
        .upsert(entry(1_026, "needle777", &address))
        .expect("shorten long field");
    let shortened_position = catalogue.positions[&target(1_026)];
    assert_eq!(
        catalogue.field_groups[shortened_position][0],
        Some(restored)
    );
    assert_eq!(catalogue.groups.len(), 2_047);
    let result = compare_to_exhaustive(&mut catalogue, "needle", Scope::Everything, 0, policy());
    assert_eq!(result.counts.total(), 2);
    assert_groups_follow_entries(&catalogue);

    let targets: Vec<_> = catalogue.entries.iter().map(|row| row.target).collect();
    for target in targets {
        assert!(catalogue.remove(target).expect("release bounded fixture"));
    }
    assert_groups_follow_entries(&catalogue);
}
