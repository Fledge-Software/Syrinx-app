//! Checks retained metadata against entry-derived range priorities and unchanged workload results.

use std::cmp::Reverse;

use super::*;
use crate::catalogue::range_index::ranking::{BoundRequest, bound_priority};

mod stages;

// Keep the original boost arithmetic independent of the extracted production helper.
fn old_score(entry: &Entry, base: u16, now: i64, weights: BoostWeights) -> u32 {
    let open = u32::from(entry.open) * u32::from(weights.open);
    let frequency =
        u32::from(weights.frequency) * (u32::BITS - entry.usage.visits.leading_zeros()) / 32;
    let recency = match entry.usage.last_used {
        None => 0,
        Some(last_used) if now <= last_used => u32::from(weights.recency),
        Some(last_used) => {
            let window = weights.recency_window_secs.get();
            let remaining = window.saturating_sub(now.abs_diff(last_used));
            (u128::from(weights.recency) * u128::from(remaining) / u128::from(window)) as u32
        }
    };
    u32::from(base) + open + frequency + recency
}

fn weight_cases() -> Vec<BoostWeights> {
    let mut cases = Vec::new();
    for bits in 0..8 {
        for window in [1, 604_800, u64::MAX] {
            cases.push(BoostWeights {
                open: if bits & 1 == 0 { 0 } else { u16::MAX },
                frequency: if bits & 2 == 0 { 0 } else { u16::MAX },
                recency: if bits & 4 == 0 { 0 } else { u16::MAX },
                recency_window_secs: NonZeroU64::new(window).expect("positive window"),
            });
        }
    }
    cases.push(policy().boosts);
    cases
}

fn boundary_catalogue() -> Catalogue {
    let timestamps = [
        None,
        Some(i64::MIN),
        Some(NOW - 1),
        Some(NOW),
        Some(NOW + 1),
        Some(i64::MAX),
        Some(NOW),
        None,
    ];
    let mut catalogue = Catalogue::new();
    for visits in [0, 1, 7, 8, u32::MAX] {
        for open in [false, true] {
            add_classes(&mut catalogue, visits, open, &timestamps);
        }
    }
    enable(&mut catalogue, Limits::default());
    catalogue
}

fn add_classes(catalogue: &mut Catalogue, visits: u32, open: bool, timestamps: &[Option<i64>]) {
    for kind in 0..4 {
        for timestamp in timestamps {
            let mut row = entry(
                (catalogue.len() as u64 + 1) * 4 + kind,
                "project row-001",
                "https://bounds.example.test/001",
            );
            row.open = open;
            row.usage.visits = visits;
            row.usage.last_used = *timestamp;
            catalogue.upsert(row).expect("synthetic boundary row");
        }
    }
}

fn audit_node(
    catalogue: &Catalogue,
    bucket: &Bucket,
    class: &Class,
    node: usize,
    start: usize,
    width: usize,
) -> bool {
    let members = &bucket.members[start..(start + width).min(bucket.members.len())];
    let minimum = members
        .iter()
        .map(|member| member.target)
        .min()
        .expect("populated node");
    let first = catalogue.get(members[0].target).expect("first member");
    assert_eq!(
        bucket.members[bucket.minimum[node] as usize].target,
        minimum
    );
    let request = BoundRequest {
        bucket,
        class,
        node,
        start,
        base: 0,
    };
    for weights in weight_cases() {
        audit_weights(catalogue, &request, members, first, minimum, weights);
    }
    minimum != first.target
}

fn audit_weights(
    catalogue: &Catalogue,
    request: &BoundRequest<'_>,
    members: &[Member],
    first: &Entry,
    minimum: Target,
    weights: BoostWeights,
) {
    for now in [i64::MIN, 0, NOW, i64::MAX] {
        for base in [0, 1, u16::MAX] {
            audit_priority(
                catalogue,
                request,
                members,
                first,
                minimum,
                weights,
                (now, base),
            );
        }
    }
}

fn audit_priority(
    catalogue: &Catalogue,
    request: &BoundRequest<'_>,
    members: &[Member],
    first: &Entry,
    minimum: Target,
    weights: BoostWeights,
    scenario: (i64, u16),
) {
    let (now, base) = scenario;
    let expected = (Reverse(old_score(first, base, now, weights)), minimum);
    for metadata in [false, true] {
        let actual = bound_priority(
            catalogue,
            BoundRequest {
                bucket: request.bucket,
                class: request.class,
                node: request.node,
                start: request.start,
                base,
            },
            now,
            weights,
            metadata,
        )
        .expect("populated bound");
        assert_eq!(
            actual, expected,
            "node={}, metadata={metadata}, now={now}, base={base}",
            request.node
        );
    }
    for member in members {
        let row = catalogue.get(member.target).expect("actual member");
        let actual = (Reverse(old_score(row, base, now, weights)), row.target);
        assert_eq!(
            crate::catalogue::Ranked::new(0, row, base, now, weights).key(),
            actual
        );
        assert!(
            expected <= actual,
            "range bound must be optimistic for every member"
        );
    }
}

fn audit_all_nodes(catalogue: &Catalogue) -> usize {
    check(catalogue);
    let state = catalogue
        .ranges
        .as_ref()
        .expect("index")
        .state
        .as_ref()
        .expect("available");
    let mut independent = 0;
    for pair in state.pairs.values() {
        for (class, bucket) in &pair.buckets {
            independent += audit_bucket(catalogue, bucket, class);
        }
    }
    independent
}

fn audit_bucket(catalogue: &Catalogue, bucket: &Bucket, class: &Class) -> usize {
    let leaves = bucket.minimum.len() / 2;
    let mut independent = 0;
    for node in 1..bucket.minimum.len() {
        let level = 1usize << node.ilog2();
        let width = leaves / level;
        let start = (node - level) * width;
        if start >= bucket.members.len() {
            continue;
        }
        independent += usize::from(audit_node(catalogue, bucket, class, node, start, width));
    }
    independent
}

#[test]
fn every_populated_node_matches_literal_entry_bounds_at_numeric_boundaries() {
    let catalogue = boundary_catalogue();
    assert!(
        audit_all_nodes(&catalogue) > 0,
        "minimum target and latest timestamp must be independent"
    );
}

fn mutation_catalogue() -> Catalogue {
    let mut catalogue = Catalogue::new();
    for (position, timestamp) in [Some(NOW + 30), Some(NOW + 20), Some(NOW + 10), None]
        .into_iter()
        .enumerate()
    {
        let mut row = entry(
            (position as u64 + 1) * 4,
            "project row-001",
            "https://bounds.example.test/001",
        );
        row.usage.last_used = timestamp;
        catalogue.upsert(row).expect("mutation row");
    }
    enable(&mut catalogue, Limits::default());
    catalogue
}

fn verify_mutation(
    legacy: &mut Catalogue,
    optimized: &mut Catalogue,
    changed: Entry,
    in_place: bool,
) {
    legacy
        .ranges
        .as_mut()
        .expect("index")
        .take_in_place_counters();
    optimized
        .ranges
        .as_mut()
        .expect("index")
        .take_in_place_counters();
    let generation = legacy.generation() + 1;
    legacy.upsert(changed.clone()).expect("legacy mutation");
    optimized.upsert(changed).expect("optimized mutation");
    assert_eq!(legacy.generation(), generation);
    assert_eq!(optimized.generation(), generation);
    let counters = optimized
        .ranges
        .as_ref()
        .expect("index")
        .in_place_counters();
    assert_eq!(counters.attempted, 1);
    assert_eq!(counters.grouped, usize::from(in_place));
    assert_eq!(counters.declined, usize::from(!in_place));
    assert_eq!(
        legacy
            .ranges
            .as_ref()
            .expect("index")
            .in_place_counters()
            .declined,
        1
    );
    audit_all_nodes(legacy);
    audit_all_nodes(optimized);
    for query in ["project", "001"] {
        compare_modes(legacy, query);
        compare_modes(optimized, query);
        assert_eq!(
            legacy
                .query(query, Scope::Everything, NOW, policy())
                .expect("valid legacy query"),
            optimized
                .query(query, Scope::Everything, NOW, policy())
                .expect("valid optimized query")
        );
    }
}

#[test]
fn retained_bounds_follow_in_place_reordering_text_and_class_mutations() {
    let mut legacy = mutation_catalogue();
    let mut optimized = mutation_catalogue();
    legacy
        .ranges
        .as_mut()
        .expect("index")
        .set_in_place_enabled(false);
    let mut changed = optimized.entries[1].clone();
    changed.usage.last_used = Some(NOW + 25);
    verify_mutation(&mut legacy, &mut optimized, changed.clone(), true);
    changed.usage.last_used = Some(i64::MAX);
    verify_mutation(&mut legacy, &mut optimized, changed.clone(), false);
    changed.title = "project row-999".into();
    verify_mutation(&mut legacy, &mut optimized, changed.clone(), false);
    changed.open = true;
    changed.usage.visits = u32::MAX;
    verify_mutation(&mut legacy, &mut optimized, changed, false);
}

fn query_mode(
    catalogue: &mut Catalogue,
    query: &str,
    options: SearchPolicy,
    metadata: bool,
) -> (crate::SearchResults, QueryReport, std::time::Duration) {
    catalogue
        .ranges
        .as_mut()
        .expect("index")
        .set_metadata_bounds_enabled(metadata);
    let generation = catalogue.generation();
    let started = Instant::now();
    let result = catalogue.query_with_strategy(
        query,
        Scope::Everything,
        NOW,
        options,
        QueryStrategy::Ranges,
        &mut Unobserved,
    );
    let elapsed = started.elapsed();
    assert_eq!(catalogue.generation(), generation);
    let report = catalogue.ranges.as_ref().expect("index").last_query;
    (result.expect("synthetic bounds query"), report, elapsed)
}

fn compare_reports(mut legacy: QueryReport, mut metadata: QueryReport) {
    assert!(!legacy.metadata_bounds_enabled);
    assert!(metadata.metadata_bounds_enabled);
    assert_eq!(
        legacy.bound_position_lookups,
        2 * legacy.bound_constructions
    );
    assert_eq!(metadata.bound_position_lookups, 0);
    legacy.metadata_bounds_enabled = true;
    legacy.bound_position_lookups = 0;
    metadata.bound_position_lookups = 0;
    assert_eq!(
        legacy, metadata,
        "both bounds must perform identical search work"
    );
}

fn compare_modes(catalogue: &mut Catalogue, query: &str) -> QueryReport {
    let expected = catalogue
        .query_exhaustive(query, Scope::Everything, NOW, policy())
        .expect("exhaustive bounds query");
    let (legacy, legacy_report, _) = query_mode(catalogue, query, policy(), false);
    let (metadata, metadata_report, _) = query_mode(catalogue, query, policy(), true);
    assert_eq!(legacy, expected);
    assert_eq!(metadata, expected);
    compare_reports(legacy_report, metadata_report);
    check(catalogue);
    metadata_report
}

#[test]
fn bounds_toggle_preserves_unavailable_cap_recovery_and_digit_fallback() {
    let mut catalogue = mutation_catalogue();
    enable(
        &mut catalogue,
        Limits {
            members: 3,
            ..Limits::default()
        },
    );
    assert_eq!(
        compare_modes(&mut catalogue, "project").route,
        Route::Unavailable
    );
    let target = catalogue.entries[0].target;
    assert!(catalogue.remove(target).expect("cap recovery"));
    assert_eq!(
        compare_modes(&mut catalogue, "project").route,
        Route::Ranges
    );
    audit_all_nodes(&catalogue);
    let report = compare_modes(&mut catalogue, "001");
    assert_eq!(report.route, Route::DigitQuery);
    assert_eq!(report.bound_constructions, 0);
}

fn measure_case(
    catalogue: &mut Catalogue,
    original: &Entry,
    revision: &mut usize,
    query: &str,
    options: SearchPolicy,
) -> ([Vec<std::time::Duration>; 2], QueryReport) {
    let mut times: [Vec<_>; 2] = std::array::from_fn(|_| Vec::new());
    let mut last_report = QueryReport::default();
    for pass in 0..7 {
        *revision += 1;
        let mut changed = original.clone();
        changed.title.push_str(&format!(" revision-{revision}"));
        catalogue.upsert(changed).expect("paired workload revision");
        let generation = catalogue.generation();
        let expected = catalogue
            .query_exhaustive(query, Scope::Everything, NOW, options)
            .expect("synthetic exhaustive result");
        let mut reports = [QueryReport::default(); 2];
        for metadata in if pass % 2 == 0 {
            [false, true]
        } else {
            [true, false]
        } {
            let (actual, report, elapsed) = query_mode(catalogue, query, options, metadata);
            assert_eq!(actual, expected, "same-generation complete bounds result");
            assert_eq!(catalogue.generation(), generation);
            reports[usize::from(metadata)] = report;
            if pass >= 2 {
                times[usize::from(metadata)].push(elapsed);
            }
        }
        compare_reports(reports[0], reports[1]);
        last_report = reports[1];
    }
    (times, last_report)
}

#[test]
fn metadata_bound_workload_diagnostic_preserves_complete_results() {
    println!(
        "Test-only complete-query bounds diagnostic, not D48 acceptance. Both modes share the new position-free key layout; legacy means entry-based bound construction, not the entire historical implementation. Reuses unchanged existing/mixed100k fixtures and fourteen cases. Same-generation legacy/metadata pairs alternate order; two warmups and five measured pairs. Query preparation remains timed; revisions, oracle comparisons and route checks are untimed. No answer caching or timing thresholds."
    );
    for mixed in [false, true] {
        let mut catalogue = workload_catalogue(mixed);
        enable(&mut catalogue, Limits::default());
        let target = Target::Command(
            NonZeroU64::new(diagnostic::ROWS as u64).expect("last synthetic target"),
        );
        let original = catalogue.get(target).expect("revision row").clone();
        let mut revision = 0;
        let mut total_bounds = 0;
        for (label, query) in diagnostic::cases() {
            let options = SearchPolicy {
                max_typos: u16::from(query.chars().count() >= 4),
                ..policy()
            };
            let (times, report) =
                measure_case(&mut catalogue, &original, &mut revision, &query, options);
            let expected_route = if crate::field_groups::query_safe(&query) {
                Route::Ranges
            } else {
                Route::DigitQuery
            };
            assert_eq!(report.route, expected_route);
            total_bounds += report.bound_constructions;
            for (mode, mut samples) in ["legacy", "metadata"].into_iter().zip(times) {
                samples.sort_unstable();
                assert_eq!(samples.len(), 5);
                println!(
                    "metadata_bounds mixed={mixed} case={label} mode={mode} median_ms={:.3} worst_ms={:.3}",
                    samples[2].as_secs_f64() * 1_000.,
                    samples[4].as_secs_f64() * 1_000.
                );
            }
            println!(
                "  untimed_metadata_bounds mixed={mixed} case={label} route={:?} bounds={} legacy_map_lookups={} metadata_map_lookups={} matching={} fallback_examined={} pairs={} pops={} selected={}",
                report.route,
                report.bound_constructions,
                2 * report.bound_constructions,
                report.bound_position_lookups,
                report.matching_rows,
                report.fallback_examined,
                report.pairs_examined,
                report.frontier_pops,
                report.selected_rows
            );
        }
        assert!(
            total_bounds > 0,
            "diagnostic must exercise both bound constructors"
        );
        check(&catalogue);
    }
}
