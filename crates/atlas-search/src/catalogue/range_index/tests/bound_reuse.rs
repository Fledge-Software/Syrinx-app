//! Compares per-query group-bound reuse with unchanged candidate bounds and complete results.

use std::time::Duration;

use super::*;
use crate::candidate::{FieldMasks, GroupBoundCache, QueryBound};
use crate::catalogue::{CandidateRanking, QueryObserver, Ranked};
use crate::{ScopeCounts, SearchResults, score_cache::QueryScoreCache};

struct Candidates {
    counts: ScopeCounts,
    rows: Vec<Reverse<Ranked>>,
    work: Work,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Work {
    computed: usize,
    reused: usize,
}

impl QueryObserver for Work {
    fn bound_work(&mut self, computed: usize, reused: usize) {
        self.computed = computed;
        self.reused = reused;
    }
}

impl Catalogue {
    pub(in crate::catalogue) fn rank_reused_bounds(
        &mut self,
        text: &str,
        scope: Scope,
        now: i64,
        options: SearchPolicy,
        observer: &mut impl QueryObserver,
        reuse: bool,
    ) -> Result<ScopeCounts, SearchError> {
        if options.max_typos > 1 {
            return self.rank_matches(text, scope, now, options, true);
        }
        let candidates = self.reused_bound_candidates(text, scope, now, options, reuse)?;
        observer.bound_work(candidates.work.computed, candidates.work.reused);
        self.rank_from_candidates(
            CandidateRanking {
                counts: candidates.counts,
                candidates: candidates.rows,
                selected: std::collections::BinaryHeap::new(),
                scores: QueryScoreCache::with_groups(text, self.groups.slots()),
                direct_count: 0,
            },
            now,
            options,
            observer,
        )
    }

    fn reused_bound_candidates(
        &self,
        text: &str,
        scope: Scope,
        now: i64,
        options: SearchPolicy,
        reuse: bool,
    ) -> Result<Candidates, SearchError> {
        let mut membership = crate::catalogue::membership::GroupedMembership::new(
            text,
            options.max_typos,
            &self.groups,
        );
        let mut bounds = GroupBoundCache::new(text, self.groups.slots(), reuse);
        let mut counts = ScopeCounts::default();
        let mut rows = Vec::new();
        // Keep this test-only scan comparable with the unchanged production candidate reference.
        for (position, (entry, masks)) in self.entries.iter().zip(&self.field_masks).enumerate() {
            let ids = self.field_groups[position];
            if !membership.entry_matches_counted(entry, masks, ids, &self.digit_counts[position])? {
                continue;
            }
            counts.include(entry.target);
            if options.limit > 0 && scope.includes(entry.target) {
                rows.push(Reverse(Ranked::new(
                    position,
                    entry,
                    bounds.score_upper_bound(entry, masks, ids)?,
                    now,
                    options.boosts,
                )));
            }
        }
        let (computed, reused) = bounds.stats();
        Ok(Candidates {
            counts,
            rows,
            work: Work { computed, reused },
        })
    }
}

fn check_candidates(
    catalogue: &Catalogue,
    text: &str,
    scope: Scope,
    now: i64,
    options: SearchPolicy,
) {
    if text.is_empty() || options.max_typos > 1 {
        return;
    }
    let (counts, rows) = catalogue
        .bound_candidates(text, scope, now, options)
        .expect("original bounds");
    for reuse in [false, true] {
        let actual = catalogue
            .reused_bound_candidates(text, scope, now, options, reuse)
            .expect("experimental bounds");
        assert_eq!(actual.counts, counts);
        assert!(
            actual.rows == rows,
            "every original candidate index, target and bound agrees"
        );
        assert_eq!(actual.work.computed + actual.work.reused, 2 * rows.len());
    }
}

fn check_results(
    catalogue: &mut Catalogue,
    text: &str,
    scope: Scope,
    now: i64,
    options: SearchPolicy,
) {
    check_candidates(catalogue, text, scope, now, options);
    let expected = catalogue
        .query_exhaustive(text, scope, now, options)
        .expect("valid synthetic reference");
    assert_eq!(
        catalogue
            .query(text, scope, now, options)
            .expect("ordinary result"),
        expected
    );
    for reuse in [false, true] {
        let result = catalogue
            .query_with_strategy(
                text,
                scope,
                now,
                options,
                QueryStrategy::BoundReuse(reuse),
                &mut Work::default(),
            )
            .expect("experimental result");
        assert_eq!(result, expected);
    }
}

#[test]
fn reused_bounds_preserve_exact_field_values_and_digit_fallback() {
    let mut groups = crate::field_groups::FieldGroups::new();
    let fields = [
        "a0B".to_owned(),
        "a9B".into(),
        "é0_東京".into(),
        "é9_東京".into(),
        "a\u{0}0b".into(),
        "a\u{0}9b".into(),
        "exact".into(),
        String::new(),
        "a".repeat(1_024),
        "a".repeat(1_025),
    ];
    let rows: Vec<_> = fields
        .iter()
        .enumerate()
        .map(|(index, text)| entry(index as u64 + 1, text, text))
        .collect();
    let ids: Vec<_> = rows
        .iter()
        .map(|row| [groups.intern(&row.title), groups.intern(&row.address)])
        .collect();
    for query in [
        "aB", "é東京", "a\0b", "exact", "EXACT", "0", "a9B", "", "🧭",
    ] {
        let reference = QueryBound::new(query);
        let mut cached = GroupBoundCache::new(query, groups.slots(), true);
        for index in (0..rows.len()).chain((0..rows.len()).rev()) {
            let row = &rows[index];
            let masks = FieldMasks::new(row);
            assert_eq!(
                cached.score_upper_bound(row, &masks, ids[index]),
                Ok(masks.score_upper_bound(row, &reference))
            );
        }
        let (_, reused) = cached.stats();
        if query.is_empty() || !crate::field_groups::query_safe(query) {
            assert_eq!(reused, 0);
        } else {
            assert!(reused > 0);
        }
    }
}

#[test]
fn bound_cache_is_query_local_and_checks_supplied_group_slots() {
    let mut groups = crate::field_groups::FieldGroups::new();
    let old = entry(1, "a", "");
    let id = groups.intern(&old.title).expect("old group");
    let mut cache = GroupBoundCache::new("a", groups.slots(), true);
    let old_bound = cache
        .score_upper_bound(&old, &FieldMasks::new(&old), [Some(id), None])
        .expect("old bound");
    drop(cache);
    assert!(groups.release(id));
    let new = entry(1, "_a0", "");
    let replacement = groups.intern(&new.title).expect("replacement group");
    assert_eq!(replacement, id, "fixture must reuse the released slot");
    let mut cache = GroupBoundCache::new("a", groups.slots(), true);
    let masks = FieldMasks::new(&new);
    let expected = masks.score_upper_bound(&new, &QueryBound::new("a"));
    assert_ne!(expected, old_bound, "replacement must change the bound");
    assert_eq!(
        cache.score_upper_bound(&new, &masks, [Some(replacement), None]),
        Ok(expected)
    );
    let invalid = crate::field_groups::GroupId::new(u32::MAX);
    assert_eq!(
        cache.score_upper_bound(&new, &masks, [invalid, None]),
        Err(SearchError::InconsistentMatch)
    );
    let long = entry(1, &"a".repeat(1_025), "");
    let masks = FieldMasks::new(&long);
    assert_eq!(
        cache.score_upper_bound(&long, &masks, [Some(id), None]),
        Err(SearchError::InconsistentMatch)
    );
    assert_eq!(
        cache.score_upper_bound(&long, &masks, [None; 2]),
        Ok(masks.score_upper_bound(&long, &QueryBound::new("a")))
    );
}

#[test]
fn bound_reuse_preserves_scopes_limits_extreme_boosts_and_mutations() {
    let mut catalogue = fixture();
    catalogue
        .upsert(entry(100, &"a".repeat(1_025), "a\0b"))
        .expect("long fallback");
    for scope in [
        Scope::Everything,
        Scope::Tabs,
        Scope::Saved,
        Scope::History,
        Scope::Commands,
    ] {
        for limit in [0, 1, 50] {
            for budget in [0, 1, 2] {
                for query in ["", "project", "café", "東京", "a\0b", "01", "missing"] {
                    check_results(
                        &mut catalogue,
                        query,
                        scope,
                        NOW,
                        SearchPolicy {
                            limit,
                            max_typos: budget,
                            ..policy()
                        },
                    );
                }
            }
        }
    }
    for now in [i64::MIN, NOW, i64::MAX] {
        check_results(
            &mut catalogue,
            "project",
            Scope::Everything,
            now,
            SearchPolicy {
                boosts: BoostWeights {
                    open: u16::MAX,
                    frequency: u16::MAX,
                    recency: u16::MAX,
                    recency_window_secs: NonZeroU64::new(u64::MAX).expect("window"),
                },
                ..policy()
            },
        );
    }
    let mut changed = catalogue.entries[0].clone();
    let target = changed.target;
    changed.title = "project a123B".into();
    changed.address.clear();
    catalogue.upsert(changed.clone()).expect("replacement");
    check_results(&mut catalogue, "project", Scope::Everything, NOW, policy());
    assert!(catalogue.remove(target).expect("remove"));
    changed.title = "project a987B".into();
    catalogue.upsert(changed).expect("fresh leases");
    check_results(&mut catalogue, "project", Scope::Everything, NOW, policy());
}

fn sample(
    catalogue: &mut Catalogue,
    query: &str,
    options: SearchPolicy,
    reuse: bool,
) -> (SearchResults, Duration, Work) {
    let mut work = Work::default();
    let started = Instant::now();
    let result = catalogue.query_with_strategy(
        query,
        Scope::Everything,
        NOW,
        options,
        QueryStrategy::BoundReuse(reuse),
        &mut work,
    );
    let elapsed = started.elapsed();
    (result.expect("synthetic complete query"), elapsed, work)
}

#[test]
fn bound_reuse_workload_diagnostic_preserves_complete_results() {
    println!(
        "Test-only conservative bound reuse, not D48 acceptance. Both modes share the instrumented candidate scan; original mode disables memoization, rather than timing production bound_candidates directly. Unchanged existing/mixed100k fixtures and fourteen queries; limit20, two warmups and five retained alternating same-generation pairs. Query preparation, fresh bounded memo and owned result construction are timed; returned SearchResults destruction is outside timing. Revisions, every production candidate bound comparison and complete exhaustive/production result equality are outside timing. Counters report two field requests per visible matching row; no cross-query answers or pair cache. Production path remains unchanged."
    );
    for mixed in [false, true] {
        let mut catalogue = workload_catalogue(mixed);
        let target =
            Target::Command(NonZeroU64::new(diagnostic::ROWS as u64).expect("last target"));
        let original = catalogue.get(target).expect("revision row").clone();
        let mut revision = 0;
        for (label, query) in diagnostic::cases() {
            let options = SearchPolicy {
                max_typos: u16::from(query.chars().count() >= 4),
                ..policy()
            };
            let mut times: [Vec<(Duration, Work)>; 2] = std::array::from_fn(|_| Vec::new());
            for pass in 0..7 {
                revision += 1;
                let mut changed = original.clone();
                changed.title.push_str(&format!(" revision-{revision}"));
                catalogue.upsert(changed).expect("workload revision");
                let generation = catalogue.generation();
                check_candidates(&catalogue, &query, Scope::Everything, NOW, options);
                let expected = catalogue
                    .query_exhaustive(&query, Scope::Everything, NOW, options)
                    .expect("reference");
                assert_eq!(
                    catalogue
                        .query(&query, Scope::Everything, NOW, options)
                        .expect("ordinary"),
                    expected
                );
                for reuse in if pass % 2 == 0 {
                    [false, true]
                } else {
                    [true, false]
                } {
                    let (actual, elapsed, work) = sample(&mut catalogue, &query, options, reuse);
                    assert_eq!(actual, expected);
                    assert_eq!(catalogue.generation(), generation);
                    if pass >= 2 {
                        times[usize::from(reuse)].push((elapsed, work));
                    }
                }
            }
            for (mode, mut samples) in ["original", "reused"].into_iter().zip(times) {
                samples.sort_unstable_by_key(|sample| sample.0);
                assert_eq!(samples.len(), 5);
                println!(
                    "bound_reuse mixed={mixed} case={label} mode={mode} min_ms={:.3} median_ms={:.3} max_ms={:.3} median_work={:?}",
                    samples[0].0.as_secs_f64() * 1000.,
                    samples[2].0.as_secs_f64() * 1000.,
                    samples[4].0.as_secs_f64() * 1000.,
                    samples[2].1
                );
            }
        }
    }
}
