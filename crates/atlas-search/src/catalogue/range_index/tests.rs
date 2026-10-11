//! Checks exact range ranking, bounded lifecycle recovery and synthetic workload costs.

use std::{collections::BTreeSet, mem::size_of, num::NonZeroU64, time::Instant};

use super::*;
use crate::catalogue::{Catalogue, QueryStrategy, Unobserved};
use crate::membership::indexed::diagnostic;

mod bound_reuse;
mod bounds;
mod in_place;
mod mutation_costs;
mod projection;
mod restore;
use crate::{BlankQuery, BoostWeights, Scope, SearchError, SearchPolicy, Usage};

const NOW: i64 = 1_800_000_000;

fn policy() -> SearchPolicy {
    SearchPolicy {
        limit: 20,
        max_typos: 0,
        blank: BlankQuery::All,
        boosts: BoostWeights {
            open: 120,
            frequency: 30,
            recency: 30,
            recency_window_secs: NonZeroU64::new(604_800).expect("synthetic window"),
        },
    }
}

fn entry(id: u64, title: &str, address: &str) -> Entry {
    let identity = NonZeroU64::new(id).expect("synthetic ID");
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
            visits: 7,
            last_used: Some(NOW - id as i64),
        },
        open: false,
    }
}

fn enable(catalogue: &mut Catalogue, limits: Limits) {
    catalogue.ranges = Some(RangeIndex::with_limits(
        &catalogue.entries,
        &catalogue.field_groups,
        limits,
    ));
    check(catalogue);
}

fn fixture() -> Catalogue {
    let mut catalogue = Catalogue::new();
    for id in 1..=24 {
        let title = match id % 3 {
            0 => format!("project café a{id:02}B"),
            1 => format!("project 東京{id:02}🧭"),
            _ => format!("project a\0b{id:02}"),
        };
        catalogue
            .upsert(entry(id, &title, &format!("https://ranges.test/{id:02}")))
            .expect("synthetic row");
    }
    let mut fallback = entry(25, &format!("project{}", "x".repeat(1_025)), "");
    fallback.open = true;
    catalogue.upsert(fallback).expect("synthetic fallback row");
    enable(&mut catalogue, Limits::default());
    catalogue
}

fn check(catalogue: &Catalogue) {
    let index = catalogue.ranges.as_ref().expect("index enabled");
    let grouped = catalogue
        .field_groups
        .iter()
        .filter(|ids| pair_key(**ids).is_some())
        .count();
    assert_eq!(index.grouped, grouped);
    assert_eq!(index.fallback, catalogue.len() - grouped);
    assert!(index.previous.is_none());
    let Some(state) = &index.state else {
        return;
    };
    let mut targets = BTreeSet::new();
    let mut buckets = 0;
    for (key, pair) in &state.pairs {
        let mut counts = [0; 4];
        assert!(!pair.buckets.is_empty());
        for (class, bucket) in &pair.buckets {
            buckets += 1;
            assert!(!bucket.members.is_empty());
            assert!(
                bucket
                    .members
                    .windows(2)
                    .all(|members| members[0].key() < members[1].key())
            );
            assert!(bucket.members.capacity() <= 2 * bucket.members.len().max(1));
            assert!(bucket.minimum.capacity() <= 2 * bucket.minimum.len());
            let leaves = bucket.minimum.len() / 2;
            for (position, member) in bucket.members.iter().enumerate() {
                let row = catalogue.positions[&member.target];
                let entry = &catalogue.entries[row];
                assert!(targets.insert(member.target));
                assert!(Class::new(entry) == *class);
                assert!(Member::new(entry) == *member);
                assert_eq!(pair_key(catalogue.field_groups[row]), Some(*key));
                assert_eq!(bucket.minimum[leaves + position], position as u32);
                counts[class.kind] += 1;
            }
            for node in 1..leaves {
                assert_eq!(
                    bucket.minimum[node],
                    bucket.minimum_of(bucket.minimum[2 * node], bucket.minimum[2 * node + 1])
                );
            }
            let minimum = bucket
                .members
                .iter()
                .map(|member| member.target)
                .min()
                .expect("nonempty bucket");
            assert_eq!(bucket.members[bucket.minimum[1] as usize].target, minimum);
        }
        assert_eq!(counts, pair.counts);
    }
    assert_eq!(buckets, state.buckets);
    assert_eq!(targets.len(), index.grouped);
    assert!(
        state
            .fallback
            .windows(2)
            .all(|records| records[0].position < records[1].position)
    );
    for record in &state.fallback {
        assert!(targets.insert(record.target));
        assert_eq!(catalogue.positions[&record.target], record.position);
        assert_eq!(catalogue.entries[record.position].target, record.target);
        assert!(pair_key(catalogue.field_groups[record.position]).is_none());
    }
    assert_eq!(state.fallback.len(), index.fallback);
    assert_eq!(targets.len(), catalogue.len());
}

fn compare(catalogue: &mut Catalogue, query: &str, scope: Scope, now: i64, options: SearchPolicy) {
    let ordinary = catalogue.query(query, scope, now, options);
    let exhaustive = catalogue.query_exhaustive(query, scope, now, options);
    assert_eq!(ordinary, exhaustive, "ordinary oracle {query:?}");
    let ranges = catalogue.query_with_strategy(
        query,
        scope,
        now,
        options,
        QueryStrategy::Ranges,
        &mut Unobserved,
    );
    assert_eq!(ranges, exhaustive, "range oracle {query:?}");
    check(catalogue);
}

#[test]
fn ranges_preserve_scopes_limits_unicode_digits_and_errors() {
    let mut catalogue = fixture();
    for limit in [0, 1, 7, 100] {
        for scope in [
            Scope::Everything,
            Scope::Tabs,
            Scope::Saved,
            Scope::History,
            Scope::Commands,
        ] {
            for typos in [0, 1, 2] {
                for query in [
                    "project", "café", "東京", "🧭", "aB", "a\0b", "\0", "01", "", "missing",
                ] {
                    compare(
                        &mut catalogue,
                        query,
                        scope,
                        NOW,
                        SearchPolicy {
                            limit,
                            max_typos: typos,
                            ..policy()
                        },
                    );
                }
            }
        }
    }
}

#[test]
fn timestamp_bounds_keep_target_ties_across_every_recency_plateau() {
    let mut catalogue = Catalogue::new();
    for id in 1..=12 {
        let mut row = entry(id * 4, "project 001", "project 999");
        row.usage.last_used = match id % 6 {
            0 => None,
            1 => Some(i64::MIN),
            2 => Some(NOW - 1),
            3 => Some(NOW),
            4 => Some(NOW + 1),
            _ => Some(i64::MAX),
        };
        catalogue.upsert(row).expect("timestamp row");
    }
    enable(&mut catalogue, Limits::default());
    for now in [i64::MIN, NOW, i64::MAX] {
        for weight in [0, 1, u16::MAX] {
            for window in [1, 604_800, u64::MAX] {
                let mut options = policy();
                options.limit = 4;
                options.boosts = BoostWeights {
                    open: weight,
                    frequency: weight,
                    recency: weight,
                    recency_window_secs: NonZeroU64::new(window).expect("positive window"),
                };
                compare(&mut catalogue, "project", Scope::Everything, now, options);
            }
        }
    }
}

#[test]
fn mutations_representatives_reused_ids_and_failed_generation_remain_atomic() {
    let mut catalogue = fixture();
    let target = catalogue.entries[0].target;
    assert!(catalogue.remove(target).expect("representative removal"));
    check(&catalogue);
    catalogue
        .upsert(entry(1, "replacement project", "https://replacement.test/"))
        .expect("new pair");
    let mut changed = catalogue.entries[0].clone();
    changed.open = true;
    changed.usage.visits = u32::MAX;
    changed.usage.last_used = None;
    catalogue.upsert(changed.clone()).expect("bucket move");
    let generation = catalogue.generation();
    catalogue.upsert(changed.clone()).expect("no-op");
    assert_eq!(catalogue.generation(), generation);
    for query in ["project", "replacement", "café", "99"] {
        compare(&mut catalogue, query, Scope::Everything, NOW, policy());
    }
    catalogue.generation = u64::MAX;
    changed.title = "would change project".into();
    assert_eq!(
        catalogue.upsert(changed),
        Err(SearchError::GenerationExhausted)
    );
    assert_eq!(
        catalogue.remove(target),
        Err(SearchError::GenerationExhausted)
    );
    check(&catalogue);
    compare(&mut catalogue, "project", Scope::Everything, NOW, policy());
}

#[test]
fn resource_failure_restarts_queries_and_recovers_after_mutations() {
    let mut catalogue = fixture();
    for limits in [
        Limits {
            members: 1,
            ..Limits::default()
        },
        Limits {
            fallback: 0,
            ..Limits::default()
        },
        Limits {
            pairs: 1,
            ..Limits::default()
        },
        Limits {
            buckets: 1,
            ..Limits::default()
        },
        Limits {
            frontier: 0,
            ..Limits::default()
        },
        Limits {
            frontier: 2,
            ..Limits::default()
        },
    ] {
        enable(&mut catalogue, limits);
        compare(&mut catalogue, "project", Scope::Everything, NOW, policy());
    }
    let limits = Limits {
        members: 24,
        fallback: 0,
        ..Limits::default()
    };
    enable(&mut catalogue, limits);
    assert!(catalogue.ranges.as_ref().expect("index").state.is_none());
    let target = entry(25, "", "").target;
    assert!(catalogue.remove(target).expect("remove fallback overflow"));
    assert!(catalogue.ranges.as_ref().expect("index").state.is_some());
    catalogue
        .upsert(entry(26, "project unique", ""))
        .expect("member overflow");
    assert!(catalogue.ranges.as_ref().expect("index").state.is_none());
    assert!(
        catalogue
            .remove(entry(26, "", "").target)
            .expect("remove member overflow")
    );
    assert!(catalogue.ranges.as_ref().expect("index").state.is_some());
    compare(&mut catalogue, "project", Scope::Everything, NOW, policy());
}

#[test]
fn pair_cap_retries_ignore_timestamp_only_edits_and_recover() {
    let mut catalogue = Catalogue::new();
    for id in 1..=64 {
        catalogue
            .upsert(entry(
                id,
                if id == 64 {
                    "project other"
                } else {
                    "project same"
                },
                "",
            ))
            .expect("pair fixture");
    }
    enable(
        &mut catalogue,
        Limits {
            pairs: 1,
            ..Limits::default()
        },
    );
    for _ in 0..8 {
        let mut row = catalogue.entries[0].clone();
        row.usage.last_used = Some(row.usage.last_used.expect("timestamp") - 1);
        catalogue.upsert(row).expect("timestamp edit");
    }
    assert_eq!(catalogue.ranges.as_ref().expect("index").retry_mutations, 0);
    catalogue
        .remove(entry(64, "", "").target)
        .expect("remove extra pair");
    for id in [63, 62] {
        catalogue
            .remove(entry(id, "", "").target)
            .expect("retry structural mutation");
    }
    assert!(catalogue.ranges.as_ref().expect("index").state.is_some());
    compare(&mut catalogue, "project", Scope::Everything, NOW, policy());
}

#[test]
fn released_pair_slots_and_retained_ungrouped_fields_follow_the_live_catalogue() {
    let mut catalogue = Catalogue::new();
    for id in 1..=1_025 {
        let suffix = alphabetic(id as usize);
        catalogue
            .upsert(entry(
                id,
                &format!("project {suffix}"),
                &format!("https://unique.test/{suffix}"),
            ))
            .expect("pool exhaustion fixture");
    }
    enable(&mut catalogue, Limits::default());
    let mut fallback = catalogue.entries[1_024].clone();
    let fallback_target = fallback.target;
    assert_eq!(catalogue.field_groups[1_024], [None, None]);
    fallback.open = true;
    catalogue
        .upsert(fallback)
        .expect("metadata-only fallback update");
    assert_eq!(
        catalogue.field_groups[catalogue.positions[&fallback_target]],
        [None, None]
    );
    let removed = catalogue.entries[0].target;
    let released = catalogue.field_groups[0];
    catalogue.remove(removed).expect("release sole pair");
    catalogue
        .upsert(entry(
            2_000,
            "project replacement",
            "https://replacement.test/",
        ))
        .expect("reuse vacant slots");
    let replacement = catalogue.positions[&entry(2_000, "", "").target];
    assert!(
        catalogue.field_groups[replacement]
            .iter()
            .any(|id| id.is_some() && released.contains(id))
    );
    assert_eq!(
        catalogue.field_groups[catalogue.positions[&fallback_target]],
        [None, None]
    );
    for query in ["project", "replacement", "unique", "aaa"] {
        compare(&mut catalogue, query, Scope::Everything, NOW, policy());
    }
}

#[test]
fn expansion_overflow_restarts_without_duplicate_observer_stages() {
    use crate::catalogue::{QueryObserver, QueryStage};

    #[derive(Default)]
    struct Stages {
        ordinary: bool,
        ranges: bool,
    }
    impl QueryObserver for Stages {
        fn stage_finished(&mut self, stage: QueryStage) {
            let seen = match stage {
                QueryStage::MembershipAndCandidates => &mut self.ordinary,
                QueryStage::RangeMembershipAndRanking => &mut self.ranges,
                _ => return,
            };
            assert!(!*seen, "each membership stage is reported once");
            *seen = true;
        }
    }
    let mut catalogue = Catalogue::new();
    for id in 1..=8 {
        catalogue
            .upsert(entry(id * 4, "project same", ""))
            .expect("one bucket");
    }
    let mut fallback = entry(5, &format!("project{}", "x".repeat(1_025)), "");
    fallback.open = true;
    catalogue.upsert(fallback).expect("strong fallback");
    enable(
        &mut catalogue,
        Limits {
            frontier: 2,
            ..Limits::default()
        },
    );
    let mut options = policy();
    options.limit = 3;
    options.boosts.open = u16::MAX;
    let expected = catalogue.query_exhaustive("project", Scope::Everything, NOW, options);
    let mut stages = Stages::default();
    let actual = catalogue.query_with_strategy(
        "project",
        Scope::Everything,
        NOW,
        options,
        QueryStrategy::Ranges,
        &mut stages,
    );
    assert_eq!(actual, expected);
    assert!(stages.ordinary);
    assert!(!stages.ranges);
    let report = catalogue.ranges.as_ref().expect("index").last_query;
    assert_eq!(report.route, Route::FrontierExpansion);
    assert!(report.attempted);
    assert!(report.frontier_pops > 0);
    assert!(report.selected_rows > 0);
    check(&catalogue);
}

#[test]
fn fallback_positions_follow_every_swap_and_replacement_direction() {
    let mut catalogue = Catalogue::new();
    let long = format!("project{}", "x".repeat(1_025));
    for (position, id) in [40, 4, 28, 12, 24, 8].into_iter().enumerate() {
        let title = if matches!(position, 1 | 2 | 5) {
            long.as_str()
        } else {
            "project"
        };
        catalogue
            .upsert(entry(id, title, ""))
            .expect("permuted target fixture");
    }
    enable(&mut catalogue, Limits::default());
    for id in [40, 4, 28] {
        assert!(
            catalogue
                .remove(entry(id, "", "").target)
                .expect("swap removal")
        );
        compare(&mut catalogue, "project", Scope::Everything, NOW, policy());
    }
    catalogue
        .upsert(entry(100, &long, ""))
        .expect("append fallback");
    let mut row = catalogue.entries[1].clone();
    row.title = long.clone();
    catalogue.upsert(row).expect("grouped becomes fallback");
    compare(&mut catalogue, "project", Scope::Everything, NOW, policy());
    let mut row = catalogue.entries[0].clone();
    row.title = "project".into();
    catalogue.upsert(row).expect("fallback becomes grouped");
    let mut row = catalogue.entries[3].clone();
    row.open = true;
    catalogue
        .upsert(row.clone())
        .expect("fallback metadata replacement");
    let before = catalogue
        .ranges
        .as_ref()
        .expect("index")
        .state
        .as_ref()
        .expect("state")
        .fallback
        .clone();
    let generation = catalogue.generation();
    catalogue.upsert(row.clone()).expect("fallback no-op");
    assert_eq!(catalogue.generation(), generation);
    assert_eq!(
        catalogue
            .ranges
            .as_ref()
            .expect("index")
            .state
            .as_ref()
            .expect("state")
            .fallback,
        before
    );
    catalogue.remove(row.target).expect("last row removal");
    for scope in [
        Scope::Everything,
        Scope::Tabs,
        Scope::Saved,
        Scope::History,
        Scope::Commands,
    ] {
        compare(&mut catalogue, "project", scope, NOW, policy());
    }
    let before = catalogue
        .ranges
        .as_ref()
        .expect("index")
        .state
        .as_ref()
        .expect("state")
        .fallback
        .clone();
    let mut row = catalogue.entries[1].clone();
    catalogue.generation = u64::MAX;
    assert_eq!(
        catalogue.remove(row.target),
        Err(SearchError::GenerationExhausted)
    );
    row.open = !row.open;
    assert_eq!(catalogue.upsert(row), Err(SearchError::GenerationExhausted));
    assert_eq!(
        catalogue
            .ranges
            .as_ref()
            .expect("index")
            .state
            .as_ref()
            .expect("state")
            .fallback,
        before
    );
    check(&catalogue);
}

#[test]
fn removing_fallback_relocates_a_different_fallback_into_its_slot() {
    let mut catalogue = Catalogue::new();
    let long = format!("project{}", "x".repeat(1_025));
    for id in [40, 4, 28] {
        catalogue
            .upsert(entry(id, &long, ""))
            .expect("fallback row");
    }
    enable(&mut catalogue, Limits::default());
    let generation = catalogue.generation();
    assert!(
        catalogue
            .remove(entry(40, "", "").target)
            .expect("first fallback removal")
    );
    assert_eq!(catalogue.generation(), generation + 1);
    let state = catalogue
        .ranges
        .as_ref()
        .expect("index")
        .state
        .as_ref()
        .expect("available index");
    assert_eq!(
        state.fallback,
        vec![
            Fallback {
                target: entry(28, "", "").target,
                position: 0
            },
            Fallback {
                target: entry(4, "", "").target,
                position: 1
            },
        ]
    );
    compare(&mut catalogue, "project", Scope::Everything, NOW, policy());
    let result = catalogue
        .query("project", Scope::Everything, NOW, policy())
        .expect("remaining fallback rows");
    assert_eq!(result.counts.total(), 2);
}

#[test]
fn unavailable_index_rebuilds_fallback_positions_after_swap_removal() {
    let mut catalogue = Catalogue::new();
    let long = format!("project{}", "x".repeat(1_025));
    for (id, title) in [(4, "project"), (8, long.as_str()), (12, long.as_str())] {
        catalogue
            .upsert(entry(id, title, ""))
            .expect("recovery fixture");
    }
    enable(
        &mut catalogue,
        Limits {
            fallback: 1,
            ..Limits::default()
        },
    );
    compare(&mut catalogue, "project", Scope::Everything, NOW, policy());
    let report = catalogue.ranges.as_ref().expect("index").last_query;
    assert_eq!(report.route, Route::Unavailable);
    assert!(!report.available);
    assert!(!report.attempted);
    catalogue
        .remove(entry(4, "", "").target)
        .expect("move fallback while unavailable");
    catalogue
        .remove(entry(8, "", "").target)
        .expect("recover under fallback cap");
    let index = catalogue.ranges.as_ref().expect("index");
    assert!(index.state.is_some());
    assert_eq!(
        index.state.as_ref().expect("recovered").fallback[0].position,
        0
    );
    compare(&mut catalogue, "project", Scope::Everything, NOW, policy());
    assert_eq!(
        catalogue.ranges.as_ref().expect("index").last_query.route,
        Route::Ranges
    );
}

#[test]
fn route_reports_reset_before_blank_validation_and_error_paths() {
    let mut catalogue = fixture();
    compare(&mut catalogue, "project", Scope::Everything, NOW, policy());
    compare(&mut catalogue, "", Scope::Everything, NOW, policy());
    let report = catalogue.ranges.as_ref().expect("index").last_query;
    assert_eq!(report.route, Route::BlankQuery);
    assert!(!report.attempted);
    assert!(!report.work_recorded);
    assert_eq!(report.candidate_rows, 0);
    let long = "a".repeat(frizbee::Scoring::default().max_needle_len() + 1);
    let result = catalogue.query_with_strategy(
        &long,
        Scope::Everything,
        NOW,
        policy(),
        QueryStrategy::Ranges,
        &mut Unobserved,
    );
    assert_eq!(result, Err(SearchError::QueryTooLong));
    let report = catalogue.ranges.as_ref().expect("index").last_query;
    assert_eq!(report.route, Route::NotAttempted);
    assert!(!report.attempted);
    assert!(!report.work_recorded);
    let representative = catalogue
        .ranges
        .as_ref()
        .expect("index")
        .state
        .as_ref()
        .expect("state")
        .pairs
        .first_key_value()
        .expect("pair")
        .1
        .representative();
    let position = catalogue.positions[&representative];
    let original = catalogue.field_groups[position];
    catalogue.field_groups[position][0] = GroupId::new(u32::MAX);
    let result = catalogue.query_with_strategy(
        "project",
        Scope::Everything,
        NOW,
        policy(),
        QueryStrategy::Ranges,
        &mut Unobserved,
    );
    assert_eq!(result, Err(SearchError::InconsistentMatch));
    let report = catalogue.ranges.as_ref().expect("index").last_query;
    assert_eq!(report.route, Route::Error);
    assert!(report.attempted);
    assert!(!report.work_recorded);
    catalogue.field_groups[position] = original;
    compare(&mut catalogue, "project", Scope::Everything, NOW, policy());
}

fn alphabetic(mut number: usize) -> String {
    let mut value = String::new();
    for _ in 0..6 {
        value.push(char::from(b'a' + (number % 26) as u8));
        number /= 26;
    }
    assert_eq!(number, 0);
    value
}

fn storage(catalogue: &Catalogue) {
    let index = catalogue.ranges.as_ref().expect("diagnostic index");
    let state = index.state.as_ref().expect("diagnostic fits limits");
    let mut member_payload = 0;
    let mut member_capacity = 0;
    let mut tree_payload = 0;
    let mut tree_capacity = 0;
    for pair in state.pairs.values() {
        for bucket in pair.buckets.values() {
            member_payload += bucket.members.len() * size_of::<Member>();
            member_capacity += bucket.members.capacity() * size_of::<Member>();
            tree_payload += bucket.minimum.len() * size_of::<u32>();
            tree_capacity += bucket.minimum.capacity() * size_of::<u32>();
        }
    }
    println!(
        "range_storage grouped={} fallback={} pairs={} buckets={} member_payload={member_payload} member_capacity={member_capacity} tree_payload={tree_payload} tree_capacity={tree_capacity} fallback_capacity={}; excludes BTreeMap nodes, structs, allocator overhead and transient query allocations",
        index.grouped,
        index.fallback,
        state.pairs.len(),
        state.buckets,
        state.fallback.capacity() * size_of::<Fallback>()
    );
}

fn report_route(catalogue: &mut Catalogue, query: &str, options: SearchPolicy) {
    let ordinary = catalogue.query(query, Scope::Everything, NOW, options);
    let ranges = catalogue.query_with_strategy(
        query,
        Scope::Everything,
        NOW,
        options,
        QueryStrategy::Ranges,
        &mut Unobserved,
    );
    assert_eq!(ranges, ordinary);
    let report = catalogue
        .ranges
        .as_ref()
        .expect("diagnostic index")
        .last_query;
    assert!(report.available);
    assert_eq!(report.indexed_rows + report.fallback_rows, catalogue.len());
    assert!(report.frontier_peak <= catalogue.ranges.as_ref().expect("index").limits.frontier);
    if crate::field_groups::query_safe(query) {
        assert_eq!(report.route, Route::Ranges);
        assert!(report.attempted);
        assert!(report.work_recorded);
        assert_eq!(report.fallback_examined, report.fallback_rows);
        assert_eq!(
            report.matching_rows,
            ranges.expect("diagnostic result").counts.total()
        );
    } else {
        assert_eq!(report.route, Route::DigitQuery);
        assert!(!report.attempted);
    }
    println!(
        "  untimed_range_ranking_route={:?} available={} attempted={} work_recorded={} indexed_rows={} fallback_rows={} pairs_examined={} fallback_examined={} matching_rows={} candidate_rows={} frontier_peak={} frontier_pops={} pairs_scored={} range_leaves_ranked={} fallback_rows_scored={} selected_rows={}",
        report.route,
        report.available,
        report.attempted,
        report.work_recorded,
        report.indexed_rows,
        report.fallback_rows,
        report.pairs_examined,
        report.fallback_examined,
        report.matching_rows,
        report.candidate_rows,
        report.frontier_peak,
        report.frontier_pops,
        report.pairs_scored,
        report.range_leaves_ranked,
        report.fallback_rows_scored,
        report.selected_rows
    );
}

fn workload_catalogue(mixed: bool) -> Catalogue {
    let mut catalogue = Catalogue::new();
    for (position, mut row) in diagnostic::entries().into_iter().enumerate() {
        if mixed {
            let suffix = if position % 10 == 0 {
                alphabetic(position)
            } else {
                format!("{position:06}")
            };
            row.title = format!("synthetic project {suffix}");
            row.address = format!("https://ranges.example.test/{suffix}");
        }
        catalogue.upsert(row).expect("diagnostic row");
    }
    catalogue
}

#[test]
fn range_workload_diagnostic_preserves_complete_results() {
    println!(
        "Experimental complete-query diagnostic, not D48 acceptance: unchanged 100k corpus and separate ten-percent alphabetic-unique stress corpus. Index bulk construction and incremental updates reported separately; no answer caching. Every revision is followed by exhaustive equality outside timing. Two warmups and five alternating paired samples per case."
    );
    for mixed in [false, true] {
        let mut catalogue = workload_catalogue(mixed);
        let started = Instant::now();
        catalogue.ranges = Some(RangeIndex::build(
            &catalogue.entries,
            &catalogue.field_groups,
        ));
        println!(
            "mixed={mixed} construction_ms={:.3}",
            started.elapsed().as_secs_f64() * 1_000.
        );
        check(&catalogue);
        storage(&catalogue);
        let target = Target::Command(
            NonZeroU64::new(diagnostic::ROWS as u64).expect("last benchmark target"),
        );
        let original = catalogue
            .get(target)
            .expect("benchmark revision row")
            .clone();
        let mut revision = 0;
        for (label, query) in diagnostic::cases() {
            let options = SearchPolicy {
                max_typos: u16::from(query.chars().count() >= 4),
                ..policy()
            };
            let mut ordinary = Vec::new();
            let mut indexed = Vec::new();
            let mut updates = Vec::new();
            for pass in 0..7 {
                revision += 1;
                let mut changed = original.clone();
                changed.title.push_str(&format!(" revision-{revision}"));
                let started = Instant::now();
                catalogue.upsert(changed).expect("incremental revision");
                let update = started.elapsed();
                let expected = catalogue
                    .query_exhaustive(&query, Scope::Everything, NOW, options)
                    .expect("synthetic exhaustive result");
                let strategies = if pass % 2 == 0 {
                    [QueryStrategy::Bounded, QueryStrategy::Ranges]
                } else {
                    [QueryStrategy::Ranges, QueryStrategy::Bounded]
                };
                for strategy in strategies {
                    let started = Instant::now();
                    let actual = catalogue.query_with_strategy(
                        &query,
                        Scope::Everything,
                        NOW,
                        options,
                        strategy,
                        &mut Unobserved,
                    );
                    let elapsed = started.elapsed();
                    assert_eq!(actual.expect("synthetic query"), expected);
                    if pass >= 2 {
                        if matches!(strategy, QueryStrategy::Ranges) {
                            indexed.push(elapsed);
                        } else {
                            ordinary.push(elapsed);
                        }
                    }
                }
                if pass >= 2 {
                    updates.push(update);
                }
            }
            ordinary.sort_unstable();
            indexed.sort_unstable();
            updates.sort_unstable();
            println!(
                "mixed={mixed} {label} bounded_median_ms={:.3} range_median_ms={:.3} bounded_worst_ms={:.3} range_worst_ms={:.3} update_median_us={:.3} update_worst_us={:.3}",
                ordinary[2].as_secs_f64() * 1_000.,
                indexed[2].as_secs_f64() * 1_000.,
                ordinary[4].as_secs_f64() * 1_000.,
                indexed[4].as_secs_f64() * 1_000.,
                updates[2].as_secs_f64() * 1_000_000.,
                updates[4].as_secs_f64() * 1_000_000.
            );
            report_route(&mut catalogue, &query, options);
        }
        check(&catalogue);
    }
}
