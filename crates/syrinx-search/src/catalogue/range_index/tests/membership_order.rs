//! Compares cached-first membership in complete queries while preserving the ordinary ranking path.

use std::time::Duration;

use super::*;
use crate::catalogue::{CandidateRanking, GroupedMembership, QueryObserver, Ranked};
use crate::{ScopeCounts, SearchResults, candidate::QueryBound, score_cache::QueryScoreCache};

impl Catalogue {
    pub(in crate::catalogue) fn rank_membership_order(
        &mut self,
        text: &str,
        scope: Scope,
        now: i64,
        options: SearchPolicy,
        observer: &mut impl QueryObserver,
        cached_first: bool,
    ) -> Result<ScopeCounts, SearchError> {
        if options.max_typos > 1 {
            return self.rank_matches(text, scope, now, options, true);
        }
        let mut membership = GroupedMembership::new(text, options.max_typos, &self.groups);
        let bound = QueryBound::new(text);
        let mut counts = ScopeCounts::default();
        let mut candidates = Vec::new();
        // Only the membership call changes; counts, bounds and boosts retain their ordinary order.
        for (position, (entry, masks)) in self.entries.iter().zip(&self.field_masks).enumerate() {
            let ids = self.field_groups[position];
            let digits = &self.digit_counts[position];
            let matched = if cached_first {
                membership.entry_matches_counted_cached_first(entry, masks, ids, digits)?
            } else {
                membership.entry_matches_counted(entry, masks, ids, digits)?
            };
            if !matched {
                continue;
            }
            counts.include(entry.target);
            if options.limit > 0 && scope.includes(entry.target) {
                candidates.push(Reverse(Ranked::new(
                    position,
                    entry,
                    masks.score_upper_bound(entry, &bound),
                    now,
                    options.boosts,
                )));
            }
        }
        drop(membership);
        self.rank_from_candidates(
            CandidateRanking {
                counts,
                candidates,
                selected: std::collections::BinaryHeap::new(),
                scores: QueryScoreCache::with_groups(text, self.groups.slots()),
                direct_count: 0,
            },
            now,
            options,
            observer,
        )
    }
}

fn compare(catalogue: &mut Catalogue, query: &str, scope: Scope, options: SearchPolicy) {
    let expected = catalogue
        .query_exhaustive(query, scope, NOW, options)
        .expect("reference");
    assert_eq!(
        catalogue
            .query(query, scope, NOW, options)
            .expect("production"),
        expected
    );
    for cached_first in [false, true] {
        assert_eq!(
            catalogue
                .query_with_strategy(
                    query,
                    scope,
                    NOW,
                    options,
                    QueryStrategy::MembershipOrder(cached_first),
                    &mut Unobserved
                )
                .expect("experiment"),
            expected
        );
    }
}

#[test]
fn membership_order_preserves_complete_queries_scopes_and_mutations() {
    let mut catalogue = fixture();
    for scope in [
        Scope::Everything,
        Scope::Tabs,
        Scope::Saved,
        Scope::History,
        Scope::Commands,
    ] {
        for limit in [0, 1, 50] {
            for max_typos in [0, 1, 2] {
                for query in [
                    "", "project", "PROJECT", "café", "東京", "a\0b", "01", "missing",
                ] {
                    compare(
                        &mut catalogue,
                        query,
                        scope,
                        SearchPolicy {
                            limit,
                            max_typos,
                            ..policy()
                        },
                    );
                }
            }
        }
    }
    for boosts in [
        BoostWeights {
            open: 0,
            frequency: 0,
            recency: 0,
            ..policy().boosts
        },
        BoostWeights {
            open: u16::MAX,
            frequency: u16::MAX,
            recency: u16::MAX,
            recency_window_secs: NonZeroU64::new(u64::MAX).expect("wide window"),
        },
    ] {
        compare(
            &mut catalogue,
            "project",
            Scope::Everything,
            SearchPolicy {
                limit: 50,
                boosts,
                ..policy()
            },
        );
    }
    let mut changed = catalogue.entries[0].clone();
    let target = changed.target;
    changed.title = "project a123B".into();
    changed.address.clear();
    catalogue.upsert(changed.clone()).expect("replace");
    compare(&mut catalogue, "project", Scope::Everything, policy());
    assert!(catalogue.remove(target).expect("remove"));
    changed.title = "project a987B".into();
    catalogue.upsert(changed).expect("reused group slots");
    compare(&mut catalogue, "project", Scope::Everything, policy());
}

fn sample(
    catalogue: &mut Catalogue,
    query: &str,
    options: SearchPolicy,
    cached_first: bool,
) -> (SearchResults, Duration) {
    let started = Instant::now();
    let result = catalogue.query_with_strategy(
        query,
        Scope::Everything,
        NOW,
        options,
        QueryStrategy::MembershipOrder(cached_first),
        &mut Unobserved,
    );
    let elapsed = started.elapsed();
    (result.expect("complete synthetic query"), elapsed)
}

#[test]
fn membership_order_workload_diagnostic_preserves_complete_results() {
    println!(
        "Test-only cached-first membership, not D48 acceptance. Existing/mixed100k fixtures, fourteen queries, limit50, two warmups and five retained alternating same-generation pairs. Both modes share the same test adapter; original calls production membership. Full query setup, counts, ranking, boosts, highlights, owned results and query-local cleanup are timed; returned result destruction and complete production/exhaustive equality are outside timing. Every retained pair is printed in nanoseconds; no counter instrumentation or timing threshold."
    );
    for mixed in [false, true] {
        let mut catalogue = workload_catalogue(mixed);
        let target =
            Target::Command(NonZeroU64::new(diagnostic::ROWS as u64).expect("last target"));
        let original = catalogue.get(target).expect("revision row").clone();
        let mut revision = 0;
        for (label, query) in diagnostic::cases() {
            let options = SearchPolicy {
                limit: 50,
                max_typos: u16::from(query.chars().count() >= 4),
                ..policy()
            };
            let mut times: [Vec<Duration>; 2] = std::array::from_fn(|_| Vec::new());
            for pass in 0..7 {
                revision += 1;
                let mut changed = original.clone();
                changed.title.push_str(&format!(" revision-{revision}"));
                catalogue.upsert(changed).expect("revision");
                let generation = catalogue.generation();
                let expected = catalogue
                    .query_exhaustive(&query, Scope::Everything, NOW, options)
                    .expect("reference");
                assert_eq!(
                    catalogue
                        .query(&query, Scope::Everything, NOW, options)
                        .expect("production"),
                    expected
                );
                let order = if pass % 2 == 0 {
                    [false, true]
                } else {
                    [true, false]
                };
                let mut pair = [Duration::ZERO; 2];
                for cached_first in order {
                    let (actual, elapsed) = sample(&mut catalogue, &query, options, cached_first);
                    assert_eq!(actual, expected);
                    assert_eq!(catalogue.generation(), generation);
                    pair[usize::from(cached_first)] = elapsed;
                }
                if pass >= 2 {
                    for mode in 0..2 {
                        times[mode].push(pair[mode]);
                    }
                    println!(
                        "membership_order_pair mixed={mixed} case={label} pass={} first={} original_ns={} cached_first_ns={}",
                        pass - 2,
                        if order[0] { "cached-first" } else { "original" },
                        pair[0].as_nanos(),
                        pair[1].as_nanos()
                    );
                }
            }
            for (mode, mut samples) in ["original", "cached-first"].into_iter().zip(times) {
                samples.sort_unstable();
                assert_eq!(samples.len(), 5);
                println!(
                    "membership_order mixed={mixed} case={label} mode={mode} min_ms={:.3} median_ms={:.3} max_ms={:.3}",
                    samples[0].as_secs_f64() * 1000.,
                    samples[2].as_secs_f64() * 1000.,
                    samples[4].as_secs_f64() * 1000.
                );
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum MembershipMode {
    Original,
    CachedFirst,
    UnicodeSelected,
}

impl MembershipMode {
    fn cached_first(self, query: &str) -> bool {
        matches!(self, Self::CachedFirst)
            || (matches!(self, Self::UnicodeSelected) && !query.is_ascii())
    }

    fn index(self) -> usize {
        match self {
            Self::Original => 0,
            Self::CachedFirst => 1,
            Self::UnicodeSelected => 2,
        }
    }
}

const MEMBERSHIP_MODES: [MembershipMode; 3] = [
    MembershipMode::Original,
    MembershipMode::CachedFirst,
    MembershipMode::UnicodeSelected,
];

fn sample_unicode_selected(
    catalogue: &mut Catalogue,
    query: &str,
    scope: Scope,
    options: SearchPolicy,
    mode: MembershipMode,
) -> (SearchResults, Duration) {
    let started = Instant::now();
    // Include the query-wide selection in the complete query measurement.
    let strategy = QueryStrategy::MembershipOrder(mode.cached_first(query));
    let result =
        catalogue.query_with_strategy(query, scope, NOW, options, strategy, &mut Unobserved);
    let elapsed = started.elapsed();
    (result.expect("complete synthetic query"), elapsed)
}

fn compare_unicode_selected(
    catalogue: &mut Catalogue,
    query: &str,
    scope: Scope,
    options: SearchPolicy,
) {
    let expected = catalogue
        .query_exhaustive(query, scope, NOW, options)
        .expect("reference");
    assert_eq!(
        catalogue
            .query(query, scope, NOW, options)
            .expect("production"),
        expected
    );
    for mode in MEMBERSHIP_MODES {
        assert_eq!(
            sample_unicode_selected(catalogue, query, scope, options, mode).0,
            expected
        );
    }
}

#[test]
fn unicode_selected_preserves_complete_queries_scopes_and_mutations() {
    for query in ["", "project", "PROJECT", "a\0b", "01"] {
        assert!(!MembershipMode::UnicodeSelected.cached_first(query));
    }
    for query in ["café", "東京", "é01", "a\0é"] {
        assert!(MembershipMode::UnicodeSelected.cached_first(query));
    }
    let mut catalogue = fixture();
    for scope in [
        Scope::Everything,
        Scope::Tabs,
        Scope::Saved,
        Scope::History,
        Scope::Commands,
    ] {
        for limit in [0, 1, 50] {
            for max_typos in [0, 1, 2] {
                for query in [
                    "", "project", "PROJECT", "café", "東京", "a\0b", "01", "é01", "a\0é",
                    "missing",
                ] {
                    compare_unicode_selected(
                        &mut catalogue,
                        query,
                        scope,
                        SearchPolicy {
                            limit,
                            max_typos,
                            ..policy()
                        },
                    );
                }
            }
        }
    }
    for boosts in [
        BoostWeights {
            open: 0,
            frequency: 0,
            recency: 0,
            ..policy().boosts
        },
        BoostWeights {
            open: u16::MAX,
            frequency: u16::MAX,
            recency: u16::MAX,
            recency_window_secs: NonZeroU64::new(u64::MAX).expect("wide window"),
        },
    ] {
        for query in ["project", "café"] {
            compare_unicode_selected(
                &mut catalogue,
                query,
                Scope::Everything,
                SearchPolicy {
                    limit: 50,
                    boosts,
                    ..policy()
                },
            );
        }
    }
    let mut changed = catalogue.entries[0].clone();
    let target = changed.target;
    changed.address.clear();
    for title in ["project café a123B", "project café a987B"] {
        changed.title = title.into();
        catalogue
            .upsert(changed.clone())
            .expect("replace or reuse group slots");
        for query in ["project", "café", "123", "é987"] {
            compare_unicode_selected(&mut catalogue, query, Scope::Everything, policy());
        }
        assert!(catalogue.remove(target).expect("remove"));
        compare_unicode_selected(&mut catalogue, "café", Scope::Everything, policy());
    }
}

#[test]
fn unicode_selected_workload_diagnostic_preserves_complete_results() {
    println!(
        "Test-only Unicode-selected cached-first membership, not D48 acceptance. Existing/mixed100k fixtures, fourteen queries, limit50, two warmup triplets and six retained same-generation triplets covering all six mode permutations. Original, unconditional cached-first and Unicode-selected share the same test adapter. Query-wide Unicode selection, setup, counts, ranking, boosts, highlights, owned results and query-local cleanup are timed; returned result destruction and complete production/exhaustive equality are outside timing. Every retained triplet is printed in nanoseconds; six-sample median averages the central two. No counter instrumentation or timing threshold."
    );
    let orders = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    for mixed in [false, true] {
        let mut catalogue = workload_catalogue(mixed);
        let target =
            Target::Command(NonZeroU64::new(diagnostic::ROWS as u64).expect("last target"));
        let original = catalogue.get(target).expect("revision row").clone();
        let mut revision = 0;
        for (label, query) in diagnostic::cases() {
            let options = SearchPolicy {
                limit: 50,
                max_typos: u16::from(query.chars().count() >= 4),
                ..policy()
            };
            let mut times: [Vec<Duration>; 3] = std::array::from_fn(|_| Vec::new());
            // Warmups precede one complete, exactly balanced permutation cycle.
            for pass in 0..8 {
                revision += 1;
                let mut changed = original.clone();
                changed.title.push_str(&format!(" revision-{revision}"));
                catalogue.upsert(changed).expect("revision");
                let generation = catalogue.generation();
                let expected = catalogue
                    .query_exhaustive(&query, Scope::Everything, NOW, options)
                    .expect("reference");
                assert_eq!(
                    catalogue
                        .query(&query, Scope::Everything, NOW, options)
                        .expect("production"),
                    expected
                );
                let order = orders[if pass < 2 { pass } else { pass - 2 }];
                let mut triplet = [Duration::ZERO; 3];
                for index in order {
                    let mode = MEMBERSHIP_MODES[index];
                    let (actual, elapsed) = sample_unicode_selected(
                        &mut catalogue,
                        &query,
                        Scope::Everything,
                        options,
                        mode,
                    );
                    assert_eq!(actual, expected);
                    assert_eq!(catalogue.generation(), generation);
                    triplet[mode.index()] = elapsed;
                }
                if pass >= 2 {
                    for mode in 0..3 {
                        times[mode].push(triplet[mode]);
                    }
                    println!(
                        "unicode_selected_triplet mixed={mixed} case={label} pass={} generation={generation:?} order={:?} original_ns={} cached_first_ns={} unicode_selected_ns={}",
                        pass - 2,
                        order.map(|index| MEMBERSHIP_MODES[index]),
                        triplet[0].as_nanos(),
                        triplet[1].as_nanos(),
                        triplet[2].as_nanos()
                    );
                }
            }
            for (mode, mut samples) in MEMBERSHIP_MODES.into_iter().zip(times) {
                samples.sort_unstable();
                assert_eq!(samples.len(), 6);
                let median_ms = (samples[2].as_secs_f64() + samples[3].as_secs_f64()) * 500.;
                println!(
                    "unicode_selected mixed={mixed} case={label} mode={mode:?} min_ms={:.3} median_ms={median_ms:.3} max_ms={:.3}",
                    samples[0].as_secs_f64() * 1000.,
                    samples[5].as_secs_f64() * 1000.
                );
            }
        }
    }
}
