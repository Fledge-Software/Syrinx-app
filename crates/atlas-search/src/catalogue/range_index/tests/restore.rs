//! Measures a synthetic restore feed followed by one bounded range-index build.

use super::*;
use std::time::Duration;

struct LoadTimes {
    feed: Duration,
    build: Duration,
}

fn restore_rows(rows: Vec<Entry>, limits: Limits) -> (Catalogue, LoadTimes) {
    // Prepared owned rows stand in for decoded storage; cloning and I/O are not timed.
    let mut catalogue = Catalogue::new();
    let started = Instant::now();
    for row in rows {
        catalogue.upsert(row).expect("valid synthetic restore row");
    }
    let feed = started.elapsed();
    assert!(catalogue.ranges.is_none(), "loading uses ordinary upserts");
    let generation = catalogue.generation();

    // Construct once from complete rows and settled field leases, before any query.
    let started = Instant::now();
    catalogue.ranges = Some(RangeIndex::with_limits(
        &catalogue.entries,
        &catalogue.field_groups,
        limits,
    ));
    let build = started.elapsed();
    assert_eq!(catalogue.generation(), generation);
    check(&catalogue);
    (catalogue, LoadTimes { feed, build })
}

fn report_load(label: &str, catalogue: &Catalogue, times: LoadTimes) {
    let index = catalogue.ranges.as_ref().expect("one built index");
    println!(
        "restore label={label} rows={} generation={} feed_ms={:.3} build_ms={:.3} feed_plus_build_ms={:.3} available={} grouped={} fallback={}",
        catalogue.len(),
        catalogue.generation(),
        times.feed.as_secs_f64() * 1_000.,
        times.build.as_secs_f64() * 1_000.,
        (times.feed + times.build).as_secs_f64() * 1_000.,
        index.state.is_some(),
        index.grouped,
        index.fallback,
    );
    println!(
        "  catalogue_vector_capacity_bytes entries={} masks={} digit_counts={} field_ids={}; excludes their owned strings, position/group maps, matcher storage, allocator overhead and transient allocations",
        catalogue.entries.capacity() * size_of::<Entry>(),
        catalogue.field_masks.capacity() * size_of::<crate::candidate::FieldMasks>(),
        catalogue.digit_counts.capacity() * size_of::<[crate::digit_counts::DigitCounts; 2]>(),
        catalogue.field_groups.capacity() * size_of::<FieldIds>(),
    );
    if index.state.is_some() {
        storage(catalogue);
    }
}

fn verify_restore(reference: &mut Catalogue, restored: &mut Catalogue, queries: &[&str]) {
    assert!(reference.ranges.is_none());
    assert_eq!(restored.entries, reference.entries);
    assert_eq!(restored.generation(), reference.generation());
    let generation = restored.generation();
    for query in queries {
        let options = SearchPolicy {
            max_typos: u16::from(query.chars().count() >= 4),
            ..policy()
        };
        let expected = reference
            .query_exhaustive(query, Scope::Everything, NOW, options)
            .expect("valid synthetic exhaustive restore result");
        let actual = restored
            .query_with_strategy(
                query,
                Scope::Everything,
                NOW,
                options,
                QueryStrategy::Ranges,
                &mut Unobserved,
            )
            .expect("valid synthetic restored result");
        assert_eq!(actual, expected, "complete restore result for {query:?}");
        assert_eq!(restored.generation(), generation);
        let index = restored.ranges.as_ref().expect("restored index");
        let route = if !crate::field_groups::query_safe(query) {
            Route::DigitQuery
        } else if index.state.is_some() {
            Route::Ranges
        } else {
            Route::Unavailable
        };
        assert_eq!(index.last_query.route, route);
    }
    check(restored);
}

fn update_both(reference: &mut Catalogue, restored: &mut Catalogue, row: Entry) -> Duration {
    let original = row.clone();
    reference
        .upsert(original)
        .expect("ordinary synthetic update");
    let started = Instant::now();
    restored.upsert(row).expect("indexed synthetic update");
    started.elapsed()
}

fn remove_both(reference: &mut Catalogue, restored: &mut Catalogue, target: Target) -> Duration {
    assert!(
        reference
            .remove(target)
            .expect("ordinary synthetic removal")
    );
    let started = Instant::now();
    let removed = restored.remove(target).expect("indexed synthetic removal");
    let elapsed = started.elapsed();
    assert!(removed);
    elapsed
}

fn check_post_restore_mutations(reference: &mut Catalogue, restored: &mut Catalogue) {
    let target = Target::Command(NonZeroU64::new(diagnostic::ROWS as u64).expect("last target"));
    let original = reference.get(target).expect("revision row").clone();
    let generation = restored.generation();
    let unchanged = update_both(reference, restored, original.clone());
    assert_eq!(restored.generation(), generation);
    verify_restore(reference, restored, &["project", "row-099983"]);

    let mut changed = original.clone();
    changed.title.push_str(" revision-1");
    let replacement = update_both(reference, restored, changed);
    assert_eq!(restored.generation(), generation + 1);
    verify_restore(reference, restored, &["project", "revision", "row-099983"]);
    let removal = remove_both(reference, restored, target);
    assert_eq!(restored.generation(), generation + 2);
    verify_restore(reference, restored, &["project", "revision", "row-099983"]);
    let reinsert = update_both(reference, restored, original);
    assert_eq!(restored.generation(), generation + 3);
    verify_restore(reference, restored, &["project", "revision", "row-099983"]);
    println!(
        "  post_restore_update_us unchanged={:.3} replacement={:.3} removal={:.3} reinsert={:.3}; single observations, cloning and complete-result checks excluded",
        unchanged.as_secs_f64() * 1_000_000.,
        replacement.as_secs_f64() * 1_000_000.,
        removal.as_secs_f64() * 1_000_000.,
        reinsert.as_secs_f64() * 1_000_000.,
    );
}

#[test]
fn restore_feed_builds_once_then_preserves_complete_results_and_mutations() {
    println!(
        "Test-only restore lifecycle diagnostic, not D48 acceptance or storage I/O timing. Reuses unchanged existing/mixed 100k fixtures and fourteen query cases. Prepared owned rows enter ordinary upserts with the index absent, then one explicit bulk build precedes queries. Feed+build is the sum of separate intervals, not wall-clock restore latency. Fixture creation, empty Catalogue construction, cloning, assertions and exhaustive result checks are outside timings. Reports one observation per fixture, no thresholds. Existing policy returns 20 results; production activation/public load API is not introduced. No quadratic 100k incremental-on-empty trial."
    );
    for mixed in [false, true] {
        let mut reference = workload_catalogue(mixed);
        let rows = reference.entries.clone();
        let (mut restored, times) = restore_rows(rows, Limits::default());
        assert_eq!(restored.len(), diagnostic::ROWS);
        assert!(restored.ranges.as_ref().expect("index").state.is_some());
        report_load(
            if mixed { "mixed100k" } else { "existing100k" },
            &restored,
            times,
        );
        let cases = diagnostic::cases();
        let queries: Vec<_> = cases.iter().map(|(_, query)| query.as_str()).collect();
        verify_restore(&mut reference, &mut restored, &queries);
        check_post_restore_mutations(&mut reference, &mut restored);
    }
}

#[test]
fn restore_build_refusal_keeps_rows_and_recovers_after_cap_crossings() {
    // Reuse the existing 25-row fixture with explicit sensitivity caps, not production limits.
    let mut reference = fixture();
    reference.ranges = None;
    let rows = reference.entries.clone();
    let (mut restored, times) = restore_rows(
        rows,
        Limits {
            members: 24,
            fallback: 0,
            ..Limits::default()
        },
    );
    report_load("explicit-cap25", &restored, times);
    assert!(restored.ranges.as_ref().expect("index").state.is_none());
    verify_restore(&mut reference, &mut restored, &["project", "café", "01"]);
    let fallback = entry(25, "", "").target;
    let fallback_row = reference.get(fallback).expect("long fallback row").clone();
    let recover = remove_both(&mut reference, &mut restored, fallback);
    assert!(
        restored
            .ranges
            .as_ref()
            .expect("recovered index")
            .state
            .is_some()
    );
    verify_restore(&mut reference, &mut restored, &["project", "café", "01"]);

    let excess = entry(26, "project unique", "");
    let target = excess.target;
    let invalidate = update_both(&mut reference, &mut restored, excess);
    assert!(
        restored
            .ranges
            .as_ref()
            .expect("member overflow")
            .state
            .is_none()
    );
    verify_restore(&mut reference, &mut restored, &["project", "unique", "01"]);
    let recover_again = remove_both(&mut reference, &mut restored, target);
    assert!(
        restored
            .ranges
            .as_ref()
            .expect("recovered index")
            .state
            .is_some()
    );
    verify_restore(&mut reference, &mut restored, &["project", "unique", "01"]);
    update_both(&mut reference, &mut restored, fallback_row);
    assert!(
        restored
            .ranges
            .as_ref()
            .expect("fallback overflow")
            .state
            .is_none()
    );
    verify_restore(&mut reference, &mut restored, &["project", "café", "01"]);
    println!(
        "restore_cap single_update_us initial_fallback_recovery={:.3} member_invalidation={:.3} member_recovery={:.3}; includes ordinary catalogue maintenance, not isolated index cost",
        recover.as_secs_f64() * 1_000_000.,
        invalidate.as_secs_f64() * 1_000_000.,
        recover_again.as_secs_f64() * 1_000_000.,
    );
}
