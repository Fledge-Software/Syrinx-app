//! Measures bounded eager scoring while retaining the ordinary candidate heap as a fallback.

use super::{CandidateRanking, Catalogue, QueryObserver, Ranked, retain_best, score_fields};
use crate::{
    Scope, ScopeCounts, SearchError, SearchPolicy,
    candidate::QueryBound,
    field_groups::{FieldGroups, GroupId, query_safe},
    score_cache::QueryScoreCache,
};

#[derive(Clone, Copy, Debug)]
pub(super) struct Budget {
    pub(super) new_groups: usize,
    pub(super) work: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            new_groups: 64,
            work: 1_000_000,
        }
    }
}

#[derive(Clone, Copy, Default, Debug, Eq, PartialEq)]
struct Work {
    new_groups: usize,
    cells: usize,
}

impl Work {
    fn permits(self, additional: Self, budget: Budget) -> bool {
        additional.new_groups <= budget.new_groups.saturating_sub(self.new_groups)
            && additional.cells <= budget.work.saturating_sub(self.cells)
    }

    fn include(&mut self, additional: Self) {
        self.new_groups += additional.new_groups;
        self.cells += additional.cells;
    }
}

fn missing_work(
    ids: [Option<GroupId>; 2],
    groups: &FieldGroups,
    scores: &QueryScoreCache,
    query_bytes: usize,
) -> Result<Option<Work>, SearchError> {
    let [Some(title), Some(address)] = ids else {
        return Ok(None);
    };
    let mut work = Work::default();
    for (field, id) in [title, address].into_iter().enumerate() {
        if (field == 1 && id == title) || scores.group_is_scored(id)? {
            continue;
        }
        let text = groups.get(id).ok_or(SearchError::InconsistentMatch)?;
        let Some(cells) = (query_bytes + 1).checked_mul(text.len() + 1) else {
            return Ok(None);
        };
        let Some(total) = work.cells.checked_add(cells) else {
            return Ok(None);
        };
        work.new_groups += 1;
        work.cells = total;
    }
    Ok(Some(work))
}

impl Catalogue {
    pub(super) fn rank_hybrid(
        &mut self,
        text: &str,
        scope: Scope,
        now: i64,
        policy: SearchPolicy,
        observer: &mut impl QueryObserver,
        budget: Budget,
    ) -> Result<ScopeCounts, SearchError> {
        if policy.max_typos > 1 {
            return self.rank_matches(text, scope, now, policy, true);
        }
        let mut membership =
            super::membership::GroupedMembership::new(text, policy.max_typos, &self.groups);
        let bound = QueryBound::new(text);
        let mut ranking = CandidateRanking {
            counts: ScopeCounts::default(),
            candidates: Vec::new(),
            selected: std::collections::BinaryHeap::new(),
            scores: QueryScoreCache::with_groups(text, self.groups.slots()),
            direct_count: 0,
        };
        let mut work = Work::default();
        let eligible = !text.is_empty() && query_safe(text) && policy.max_typos <= 1;

        // Every matching target is counted; only visible candidates consume the eager budget.
        for (index, (entry, masks)) in self.entries.iter().zip(&self.field_masks).enumerate() {
            let ids = self.field_groups[index];
            if !membership.entry_matches_counted(entry, masks, ids, &self.digit_counts[index])? {
                continue;
            }
            ranking.counts.include(entry.target);
            if policy.limit == 0 || !scope.includes(entry.target) {
                continue;
            }
            let additional = if eligible {
                missing_work(ids, &self.groups, &ranking.scores, text.len())?
            } else {
                None
            };
            if let Some(additional) =
                additional.filter(|additional| work.permits(*additional, budget))
            {
                let base = score_fields(entry, ids, &mut self.matcher, &mut ranking.scores)?;
                work.include(additional);
                retain_best(
                    &mut ranking.selected,
                    Ranked::new(index, entry, base, now, policy.boosts),
                    policy.limit,
                );
                ranking.direct_count += 1;
                observer.candidate_scored();
                continue;
            }
            ranking.candidates.push(std::cmp::Reverse(Ranked::new(
                index,
                entry,
                masks.score_upper_bound(entry, &bound),
                now,
                policy.boosts,
            )));
        }
        observer.hybrid_work(
            ranking.direct_count,
            ranking.candidates.len(),
            work.new_groups,
            work.cells,
        );
        self.rank_from_candidates(ranking, now, policy, observer)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        hint::black_box,
        num::NonZeroU64,
        time::{Duration, Instant},
    };

    use super::*;
    use crate::catalogue::{QueryStrategy, Unobserved};
    use crate::membership::indexed::diagnostic;
    use crate::{BlankQuery, BoostWeights, Entry, SearchResults, Target, Usage};

    const NOW: i64 = 1_800_000_000;
    const ZERO: Budget = Budget {
        new_groups: 0,
        work: 0,
    };
    const TINY: Budget = Budget {
        new_groups: 2,
        work: 20_000,
    };
    const GENEROUS: Budget = Budget {
        new_groups: 2_048,
        work: 128 * 1_024 * 1_024,
    };

    #[derive(Default, Debug)]
    struct Observed {
        direct: usize,
        pending: usize,
        new_groups: usize,
        work: usize,
        candidates: usize,
        scored: usize,
        computed: usize,
        reused: usize,
    }

    impl QueryObserver for Observed {
        fn hybrid_work(&mut self, direct: usize, pending: usize, new_groups: usize, work: usize) {
            self.direct = direct;
            self.pending = pending;
            self.new_groups = new_groups;
            self.work = work;
        }

        fn candidates(&mut self, count: usize) {
            self.candidates = count;
        }
        fn candidate_scored(&mut self) {
            self.scored += 1;
        }
        fn field_scores(&mut self, computed: usize, reused: usize) {
            self.computed = computed;
            self.reused = reused;
        }
    }

    fn policy() -> SearchPolicy {
        SearchPolicy {
            limit: 50,
            max_typos: 0,
            blank: BlankQuery::All,
            boosts: BoostWeights {
                open: 120,
                frequency: 30,
                recency: 30,
                recency_window_secs: NonZeroU64::new(604_800).expect("nonzero synthetic window"),
            },
        }
    }

    fn entry(id: u64, title: &str, address: &str) -> Entry {
        let identity = NonZeroU64::new(id).expect("nonzero synthetic identity");
        let target = match id % 4 {
            0 => Target::Tab(atlas_core::TabId::new(id).expect("synthetic tab")),
            1 => Target::Saved(atlas_core::SavedPageId::new(id).expect("synthetic saved page")),
            2 => Target::History(identity),
            _ => Target::Command(identity),
        };
        Entry {
            target,
            title: title.into(),
            address: address.into(),
            usage: Usage {
                visits: (id % 7) as u32,
                last_used: Some(NOW - (id % 3) as i64),
            },
            open: id.is_multiple_of(5),
        }
    }

    fn fixture() -> Catalogue {
        let mut catalogue = Catalogue::new();
        for id in 1..=24 {
            let title = match id % 3 {
                0 => format!("marker café a{id:02}B"),
                1 => format!("marker 東京{id:02}🧭"),
                _ => format!("marker a\0b{id:02}"),
            };
            catalogue
                .upsert(entry(
                    id,
                    &title,
                    &format!("https://hybrid.example.test/marker{id:02}"),
                ))
                .expect("insert synthetic row");
        }
        catalogue
    }

    fn compare(
        catalogue: &mut Catalogue,
        query: &str,
        scope: Scope,
        options: SearchPolicy,
        budget: Budget,
    ) -> Observed {
        let bounded = catalogue.query(query, scope, NOW, options);
        let exhaustive = catalogue.query_exhaustive(query, scope, NOW, options);
        assert_eq!(bounded, exhaustive, "existing oracle query {query:?}");
        let mut observer = Observed::default();
        let hybrid = catalogue.query_with_strategy(
            query,
            scope,
            NOW,
            options,
            QueryStrategy::Hybrid(budget),
            &mut observer,
        );
        assert_eq!(
            hybrid, exhaustive,
            "hybrid oracle query {query:?}, budget {budget:?}"
        );
        assert!(observer.new_groups <= budget.new_groups);
        assert!(observer.work <= budget.work);
        if !query.is_empty() && options.max_typos <= 1 {
            assert_eq!(observer.candidates, observer.direct + observer.pending);
            assert_eq!(observer.computed + observer.reused, observer.scored * 2);
        }
        observer
    }

    #[test]
    fn direct_pending_and_mixed_paths_preserve_all_result_fields() {
        let mut catalogue = fixture();
        for budget in [ZERO, TINY, GENEROUS] {
            let observer = compare(
                &mut catalogue,
                "marker",
                Scope::Everything,
                policy(),
                budget,
            );
            if budget.new_groups == 0 {
                assert_eq!(observer.direct, 0);
                assert_eq!(observer.pending, 24);
            } else if budget.new_groups == TINY.new_groups {
                assert!(observer.direct > 0);
                assert!(observer.pending > 0);
            } else {
                assert_eq!(observer.direct, 24);
                assert_eq!(observer.pending, 0);
            }
        }
        for budget in [ZERO, TINY, GENEROUS] {
            for scope in [
                Scope::Everything,
                Scope::Tabs,
                Scope::Saved,
                Scope::History,
                Scope::Commands,
            ] {
                for limit in [0, 1, usize::MAX] {
                    for query in [
                        "marker", "café", "東京", "🧭", "aB", "a\0b", "\0", "01", "", "missing",
                    ] {
                        for typos in [0, 1, 2] {
                            let options = SearchPolicy {
                                limit,
                                max_typos: typos,
                                ..policy()
                            };
                            compare(&mut catalogue, query, scope, options, budget);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn work_caps_shared_fields_fallback_and_mutations_remain_exact() {
        let mut catalogue = fixture();
        let work_only = Budget {
            new_groups: 2_048,
            work: 1,
        };
        let observer = compare(
            &mut catalogue,
            "marker",
            Scope::Everything,
            policy(),
            work_only,
        );
        assert_eq!(observer.direct, 0);
        assert_eq!(observer.new_groups, 0);
        catalogue
            .upsert(entry(25, "marker77", "marker88"))
            .expect("shared field pair");
        catalogue
            .upsert(entry(
                26,
                &format!("marker{}", "x".repeat(1_025)),
                "marker77",
            ))
            .expect("uncached long field");
        compare(
            &mut catalogue,
            "marker",
            Scope::Everything,
            policy(),
            GENEROUS,
        );
        let removed = catalogue.entries[0].target;
        assert!(catalogue.remove(removed).expect("swap removal"));
        catalogue
            .upsert(entry(2, "changed café999", "marker88"))
            .expect("replace text");
        let mut metadata = catalogue.entries[0].clone();
        metadata.open = !metadata.open;
        metadata.usage.visits = u32::MAX;
        metadata.usage.last_used = Some(i64::MAX);
        catalogue.upsert(metadata).expect("update metadata");
        for query in ["marker", "café", "changed", "999"] {
            for budget in [ZERO, TINY, GENEROUS] {
                let options = SearchPolicy {
                    max_typos: 1,
                    limit: 3,
                    ..policy()
                };
                compare(&mut catalogue, query, Scope::Everything, options, budget);
            }
        }
    }

    fn sample(
        catalogue: &mut Catalogue,
        query: &str,
        options: SearchPolicy,
        strategy: QueryStrategy,
        expected: &SearchResults,
    ) -> (Duration, Observed) {
        let mut observer = Observed::default();
        let started = Instant::now();
        let result = catalogue.query_with_strategy(
            black_box(query),
            Scope::Everything,
            NOW,
            options,
            strategy,
            &mut observer,
        );
        let elapsed = started.elapsed();
        assert_eq!(result.expect("synthetic diagnostic query"), *expected);
        (elapsed, observer)
    }

    #[test]
    fn hybrid_workload_diagnostic_preserves_complete_results() {
        println!(
            "Synthetic full-query diagnostic, not D48 acceptance. Existing 100k corpus and query cases; no prior-query result reuse. Query setup, scoring, ranking, highlights and owned results timed; complete bounded/exhaustive equality outside timing. Two warmups, five alternating paired samples. Eager work bounds newly scored field groups and raw byte matrix cells."
        );
        let mut catalogue = Catalogue::new();
        for entry in diagnostic::entries() {
            catalogue.upsert(entry).expect("synthetic workload entry");
        }
        assert_eq!(catalogue.len(), diagnostic::ROWS);
        for (label, query) in diagnostic::cases() {
            let options = SearchPolicy {
                max_typos: u16::from(query.chars().count() >= 4),
                ..policy()
            };
            let expected = catalogue
                .query_exhaustive(&query, Scope::Everything, NOW, options)
                .expect("exhaustive synthetic result");
            let bounded = catalogue
                .query_with_strategy(
                    &query,
                    Scope::Everything,
                    NOW,
                    options,
                    QueryStrategy::Bounded,
                    &mut Unobserved,
                )
                .expect("bounded synthetic result");
            assert_eq!(bounded, expected);
            let hybrid = QueryStrategy::Hybrid(Budget::default());
            for _ in 0..2 {
                sample(
                    &mut catalogue,
                    &query,
                    options,
                    QueryStrategy::Bounded,
                    &expected,
                );
                sample(&mut catalogue, &query, options, hybrid, &expected);
            }
            let mut baseline = Vec::new();
            let mut candidate = Vec::new();
            let mut statistics = Observed::default();
            for pass in 0..5 {
                if pass % 2 == 0 {
                    baseline.push(
                        sample(
                            &mut catalogue,
                            &query,
                            options,
                            QueryStrategy::Bounded,
                            &expected,
                        )
                        .0,
                    );
                }
                let (elapsed, observed) =
                    sample(&mut catalogue, &query, options, hybrid, &expected);
                candidate.push(elapsed);
                statistics = observed;
                if pass % 2 == 1 {
                    baseline.push(
                        sample(
                            &mut catalogue,
                            &query,
                            options,
                            QueryStrategy::Bounded,
                            &expected,
                        )
                        .0,
                    );
                }
            }
            baseline.sort_unstable();
            candidate.sort_unstable();
            println!(
                "{label} matched={} bounded_median_ms={:.3} hybrid_median_ms={:.3} bounded_worst_ms={:.3} hybrid_worst_ms={:.3} statistics={statistics:?}",
                expected.counts.total(),
                baseline[2].as_secs_f64() * 1_000.,
                candidate[2].as_secs_f64() * 1_000.,
                baseline[4].as_secs_f64() * 1_000.,
                candidate[4].as_secs_f64() * 1_000.
            );
        }
    }
}
