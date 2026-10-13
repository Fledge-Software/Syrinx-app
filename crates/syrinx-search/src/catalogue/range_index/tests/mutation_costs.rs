//! Measures synthetic large-bucket updates before considering production index activation.

use super::*;

struct Catalogues {
    ordinary: Catalogue,
    legacy: Catalogue,
    optimized: Catalogue,
}

impl Catalogues {
    fn new(rows: usize) -> Self {
        let ordinary = catalogue(rows);
        let mut legacy = catalogue(rows);
        let mut optimized = catalogue(rows);
        for (label, catalogue) in [("legacy", &mut legacy), ("optimized", &mut optimized)] {
            let started = Instant::now();
            catalogue.ranges = Some(RangeIndex::build(
                &catalogue.entries,
                &catalogue.field_groups,
            ));
            let elapsed = started.elapsed();
            println!("large_bucket build_mode={label}");
            report_layout(catalogue, rows, elapsed);
        }
        legacy
            .ranges
            .as_mut()
            .expect("legacy index")
            .set_in_place_enabled(false);
        Self {
            ordinary,
            legacy,
            optimized,
        }
    }

    fn verify(&mut self) {
        verify(&mut self.ordinary, &mut self.legacy);
        verify(&mut self.ordinary, &mut self.optimized);
    }

    fn counters(&mut self) -> [InPlaceCounters; 2] {
        [
            self.legacy
                .ranges
                .as_mut()
                .expect("legacy index")
                .take_in_place_counters(),
            self.optimized
                .ranges
                .as_mut()
                .expect("optimized index")
                .take_in_place_counters(),
        ]
    }

    fn update(&mut self, variant: &Entry, reverse: bool) -> [std::time::Duration; 3] {
        // Clone all three entries before starting any timer.
        let ordinary = variant.clone();
        let legacy = variant.clone();
        let optimized = variant.clone();
        if reverse {
            let optimized = timed_update(&mut self.optimized, optimized);
            let legacy = timed_update(&mut self.legacy, legacy);
            let ordinary = timed_update(&mut self.ordinary, ordinary);
            return [ordinary, legacy, optimized];
        }
        [
            timed_update(&mut self.ordinary, ordinary),
            timed_update(&mut self.legacy, legacy),
            timed_update(&mut self.optimized, optimized),
        ]
    }

    fn restore(&mut self, original: Entry) {
        self.ordinary
            .upsert(original.clone())
            .expect("restore ordinary row");
        self.legacy
            .upsert(original.clone())
            .expect("restore legacy row");
        self.optimized
            .upsert(original)
            .expect("restore optimized row");
        self.verify();
    }
}

fn catalogue(rows: usize) -> Catalogue {
    let mut catalogue = Catalogue::new();
    for number in 1..=rows {
        catalogue
            .upsert(entry(
                (number * 4) as u64,
                &format!("project row-{number:06}"),
                &format!("https://large-bucket.example.test/{number:06}"),
            ))
            .expect("synthetic bucket row");
    }
    catalogue
}

fn timed_update(catalogue: &mut Catalogue, changed: Entry) -> std::time::Duration {
    let started = Instant::now();
    catalogue.upsert(changed).expect("synthetic bucket update");
    started.elapsed()
}

fn verify_update(catalogue: &Catalogue, changed: &Entry, generation: u64) {
    assert_eq!(catalogue.generation(), generation);
    assert!(catalogue.get(changed.target) == Some(changed));
}

fn verify(ordinary: &mut Catalogue, indexed: &mut Catalogue) {
    check(indexed);
    assert_eq!(ordinary.generation(), indexed.generation());
    for query in ["project", "999"] {
        let expected = ordinary
            .query_exhaustive(query, Scope::Everything, NOW, policy())
            .expect("synthetic exhaustive query");
        let actual = indexed
            .query_with_strategy(
                query,
                Scope::Everything,
                NOW,
                policy(),
                QueryStrategy::Ranges,
                &mut Unobserved,
            )
            .expect("synthetic range query");
        assert_eq!(actual, expected, "large-bucket complete result");
        if query == "project" {
            assert_eq!(actual.counts.total(), indexed.len());
        }
    }
}

fn measure_family(rows: usize, label: &str, catalogues: &mut Catalogues, variants: [Entry; 2]) {
    let original = catalogues
        .ordinary
        .get(variants[0].target)
        .expect("existing row")
        .clone();
    let mut times: [Vec<std::time::Duration>; 3] = std::array::from_fn(|_| Vec::new());
    let mut retained = [InPlaceCounters::default(); 2];
    catalogues.counters();
    for pass in 0..7 {
        for variant in &variants {
            let changed = catalogues.ordinary.get(variant.target) != Some(variant);
            assert_eq!(changed, label != "unchanged");
            let generation = catalogues.ordinary.generation() + u64::from(changed);
            let elapsed = catalogues.update(variant, pass % 2 != 0);
            for catalogue in [
                &catalogues.ordinary,
                &catalogues.legacy,
                &catalogues.optimized,
            ] {
                verify_update(catalogue, variant, generation);
            }
            let routes = catalogues.counters();
            assert_routes(label, changed, routes);
            // Check both distinct states once, including the state later undone by restoration.
            if pass == 0 {
                catalogues.verify();
            }
            if pass >= 2 {
                for (times, elapsed) in times.iter_mut().zip(elapsed) {
                    times.push(elapsed);
                }
                for (total, routes) in retained.iter_mut().zip(routes) {
                    total.attempted += routes.attempted;
                    total.grouped += routes.grouped;
                    total.fallback += routes.fallback;
                    total.declined += routes.declined;
                }
            }
        }
    }
    catalogues.verify();
    for (mode, mut times) in ["ordinary", "legacy", "optimized"].into_iter().zip(times) {
        times.sort_unstable();
        assert_eq!(times.len(), 10);
        println!(
            "large_bucket rows={rows} operation={label} mode={mode} samples=10 median_us={:.3} worst_us={:.3}",
            (times[4] + times[5]).as_secs_f64() * 500_000.,
            times[9].as_secs_f64() * 1_000_000.
        );
    }
    println!(
        "large_bucket rows={rows} operation={label} retained_legacy={:?} retained_optimized={:?}",
        retained[0], retained[1]
    );
    catalogues.restore(original);
}

fn assert_routes(label: &str, changed: bool, routes: [InPlaceCounters; 2]) {
    let attempted = usize::from(changed);
    let legacy = InPlaceCounters {
        attempted,
        declined: attempted,
        ..InPlaceCounters::default()
    };
    let optimized = if label == "first-timestamp" {
        InPlaceCounters {
            attempted,
            grouped: attempted,
            ..InPlaceCounters::default()
        }
    } else {
        legacy
    };
    assert_eq!(routes, [legacy, optimized], "route for {label}");
}

fn report_layout(indexed: &Catalogue, rows: usize, construction: std::time::Duration) {
    let state = indexed
        .ranges
        .as_ref()
        .expect("index")
        .state
        .as_ref()
        .expect("within cap");
    assert_eq!(state.pairs.len(), 1);
    assert_eq!(state.buckets, 1);
    let bucket = state
        .pairs
        .values()
        .next()
        .expect("pair")
        .buckets
        .values()
        .next()
        .expect("bucket");
    assert_eq!(bucket.members.len(), rows);
    assert_eq!(bucket.minimum.len(), 2 * rows.next_power_of_two());
    println!(
        "large_bucket rows={rows} build_ms={:.3} member_size_bytes={} member_payload_bytes={} member_capacity_bytes={} tree_entries={} tree_payload_bytes={} tree_capacity_bytes={}; excludes maps, strings, allocator overhead and transient allocations",
        construction.as_secs_f64() * 1_000.,
        size_of::<Member>(),
        bucket.members.len() * size_of::<Member>(),
        bucket.members.capacity() * size_of::<Member>(),
        bucket.minimum.len(),
        bucket.minimum.len() * size_of::<u32>(),
        bucket.minimum.capacity() * size_of::<u32>()
    );
}

fn timed_remove(catalogue: &mut Catalogue, target: Target) -> std::time::Duration {
    let started = Instant::now();
    let removed = catalogue.remove(target);
    let elapsed = started.elapsed();
    assert!(removed.expect("cap recovery"));
    elapsed
}

fn measure_cap_recovery(catalogues: &mut Catalogues, rows: usize) {
    let extra = entry(
        ((rows + 1) * 4) as u64,
        "project row-999999",
        "https://large-bucket.example.test/999999",
    );
    let target = extra.target;
    let generation = catalogues.ordinary.generation() + 1;
    catalogues.counters();
    let invalidation = catalogues.update(&extra, false);
    for catalogue in [
        &catalogues.ordinary,
        &catalogues.legacy,
        &catalogues.optimized,
    ] {
        verify_update(catalogue, &extra, generation);
    }
    for catalogue in [&catalogues.legacy, &catalogues.optimized] {
        assert!(catalogue.ranges.as_ref().expect("index").state.is_none());
    }
    catalogues.verify();
    let optimized = timed_remove(&mut catalogues.optimized, target);
    let legacy = timed_remove(&mut catalogues.legacy, target);
    let ordinary = timed_remove(&mut catalogues.ordinary, target);
    for catalogue in [
        &catalogues.ordinary,
        &catalogues.legacy,
        &catalogues.optimized,
    ] {
        assert_eq!(catalogue.generation(), generation + 1);
        assert!(catalogue.get(target).is_none());
    }
    for catalogue in [&catalogues.legacy, &catalogues.optimized] {
        assert!(catalogue.ranges.as_ref().expect("index").state.is_some());
    }
    assert_eq!(catalogues.counters(), [InPlaceCounters::default(); 2]);
    catalogues.verify();
    for ((mode, invalidation), recovery) in ["ordinary", "legacy", "optimized"]
        .into_iter()
        .zip(invalidation)
        .zip([ordinary, legacy, optimized])
    {
        println!(
            "large_bucket cap_mode={mode} cap_invalidation_ms={:.3} cap_recovery_ms={:.3}",
            invalidation.as_secs_f64() * 1_000.,
            recovery.as_secs_f64() * 1_000.
        );
    }
}

#[test]
fn large_bucket_mutations_preserve_results_and_report_costs() {
    println!(
        "Test-only mutation diagnostic, not D48 acceptance. Three identical catalogues: ordinary, legacy index with in-place admission disabled, optimized index. One bucket; two warmup pairs then five alternating pairs, ten retained updates per family. Cloning, route assertions and complete-result verification are outside mutation timing; timing assertions are intentionally absent."
    );
    for rows in [1_024, 16_384, 65_536, 65_537, 100_000, 131_072] {
        let mut catalogues = Catalogues::new(rows);
        catalogues.verify();

        let first = catalogues.ordinary.entries[0].clone();
        let oldest = catalogues.ordinary.entries[rows - 1].clone();
        measure_family(
            rows,
            "unchanged",
            &mut catalogues,
            [first.clone(), first.clone()],
        );
        let mut newer = first.clone();
        newer.usage.last_used = Some(NOW + 1);
        let mut newest = newer.clone();
        newest.usage.last_used = Some(NOW + 2);
        measure_family(rows, "first-timestamp", &mut catalogues, [newer, newest]);
        let mut moved = oldest.clone();
        moved.usage.last_used = Some(NOW + 3);
        measure_family(rows, "oldest-to-first", &mut catalogues, [moved, oldest]);
        let mut digits = first.clone();
        digits.title = "project row-999999".into();
        measure_family(
            rows,
            "same-pair-text",
            &mut catalogues,
            [digits, first.clone()],
        );
        let mut open = first.clone();
        open.open = true;
        measure_family(rows, "open-class", &mut catalogues, [open, first.clone()]);
        let mut frequency = first.clone();
        frequency.usage.visits = 8;
        measure_family(rows, "frequency-class", &mut catalogues, [frequency, first]);

        if rows == Limits::default().members {
            measure_cap_recovery(&mut catalogues, rows);
        }
    }
}
