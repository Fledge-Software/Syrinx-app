//! Verifies guarded member updates against the unchanged detach-and-reinsert path.

use super::*;

fn catalogue(timestamps: &[Option<i64>], fallback: bool) -> Catalogue {
    let mut catalogue = Catalogue::new();
    for (position, timestamp) in timestamps.iter().enumerate() {
        let title = if fallback {
            format!("project{}", "x".repeat(1_025))
        } else {
            "project row-000 café".into()
        };
        let mut row = entry(
            (position as u64 + 1) * 4,
            &title,
            "https://in-place.test/000",
        );
        row.usage.last_used = *timestamp;
        catalogue.upsert(row).expect("synthetic in-place row");
    }
    enable(&mut catalogue, Limits::default());
    catalogue
}

fn counters(catalogue: &Catalogue) -> InPlaceCounters {
    catalogue
        .ranges
        .as_ref()
        .expect("index")
        .in_place_counters()
}

fn verify_pair(legacy: &mut Catalogue, indexed: &mut Catalogue) {
    check(legacy);
    check(indexed);
    assert_eq!(legacy.generation(), indexed.generation());
    for query in ["project", "café", "000", "absent"] {
        for limit in [0, 1, 20] {
            let options = SearchPolicy { limit, ..policy() };
            let expected = legacy
                .query_exhaustive(query, Scope::Everything, NOW, options)
                .expect("valid synthetic exhaustive query");
            let actual = indexed
                .query_with_strategy(
                    query,
                    Scope::Everything,
                    NOW,
                    options,
                    QueryStrategy::Ranges,
                    &mut Unobserved,
                )
                .expect("valid synthetic range query");
            assert_eq!(actual, expected);
        }
    }
}

#[test]
fn accepted_updates_change_live_recency_scores_without_rebuilding_target_minima() {
    let timestamps = [Some(NOW - 10), Some(NOW - 20), None];
    let mut indexed = catalogue(&timestamps, false);
    let mut legacy = catalogue(&timestamps, false);
    legacy
        .ranges
        .as_mut()
        .expect("index")
        .set_in_place_enabled(false);
    let options = SearchPolicy {
        boosts: BoostWeights {
            open: 0,
            frequency: 0,
            recency: 100,
            recency_window_secs: NonZeroU64::new(100).expect("positive test window"),
        },
        ..policy()
    };
    for (position, timestamp, increment) in [(0, NOW - 1, 9), (1, NOW - 11, 9), (2, NOW - 40, 60)] {
        let mut changed = indexed.entries[position].clone();
        let before = indexed
            .query_with_strategy(
                "project",
                Scope::Everything,
                NOW,
                options,
                QueryStrategy::Ranges,
                &mut Unobserved,
            )
            .expect("before recency update");
        let old_score = before
            .hits
            .iter()
            .find(|hit| hit.target == changed.target)
            .expect("existing hit")
            .score;
        changed.usage.last_used = Some(timestamp);
        indexed
            .upsert(changed.clone())
            .expect("in-place recency update");
        legacy
            .upsert(changed.clone())
            .expect("legacy recency update");
        let expected = legacy
            .query_exhaustive("project", Scope::Everything, NOW, options)
            .expect("recency oracle");
        let actual = indexed
            .query_with_strategy(
                "project",
                Scope::Everything,
                NOW,
                options,
                QueryStrategy::Ranges,
                &mut Unobserved,
            )
            .expect("updated recency query");
        assert_eq!(actual, expected);
        let new_score = actual
            .hits
            .iter()
            .find(|hit| hit.target == changed.target)
            .expect("updated hit")
            .score;
        assert_eq!(new_score, old_score + increment);
        check(&indexed);
    }
    assert_eq!(counters(&indexed).grouped, 3);
}

fn timestamp_case(
    timestamps: &[Option<i64>],
    position: usize,
    timestamp: Option<i64>,
    accepted: bool,
) {
    let mut legacy = catalogue(timestamps, false);
    legacy
        .ranges
        .as_mut()
        .expect("index")
        .set_in_place_enabled(false);
    let mut indexed = catalogue(timestamps, false);
    let ids = indexed.field_groups.clone();
    let state = indexed
        .ranges
        .as_ref()
        .expect("index")
        .state
        .as_ref()
        .expect("state");
    let pair = state.pairs.first_key_value().expect("pair").1;
    let bucket = pair.buckets.first_key_value().expect("bucket").1;
    let targets: Vec<_> = bucket.members.iter().map(|member| member.target).collect();
    let minimum = bucket.minimum.clone();
    let pair_counts = pair.counts;
    let mut changed = indexed.entries[position].clone();
    changed.usage.last_used = timestamp;
    assert!(changed != indexed.entries[position]);
    legacy
        .upsert(changed.clone())
        .expect("legacy timestamp update");
    indexed.upsert(changed).expect("guarded timestamp update");
    assert_eq!(
        counters(&indexed),
        InPlaceCounters {
            attempted: 1,
            grouped: usize::from(accepted),
            fallback: 0,
            declined: usize::from(!accepted)
        }
    );
    assert_eq!(
        counters(&legacy),
        InPlaceCounters {
            attempted: 1,
            declined: 1,
            ..InPlaceCounters::default()
        }
    );
    assert_eq!(indexed.field_groups, ids);
    if accepted {
        let state = indexed
            .ranges
            .as_ref()
            .expect("index")
            .state
            .as_ref()
            .expect("state");
        let pair = state.pairs.first_key_value().expect("pair").1;
        let bucket = pair.buckets.first_key_value().expect("bucket").1;
        assert_eq!(bucket.minimum, minimum);
        assert_eq!(pair.counts, pair_counts);
        assert_eq!(
            bucket
                .members
                .iter()
                .map(|member| member.target)
                .collect::<Vec<_>>(),
            targets
        );
    }
    verify_pair(&mut legacy, &mut indexed);
}

#[test]
fn first_middle_last_and_tied_timestamp_neighbors_keep_full_key_order() {
    for (position, timestamp, accepted) in [
        (0, 301, true),
        (1, 250, true),
        (2, 50, true),
        (0, 200, true),
        (1, 300, true),
        (1, 100, true),
        (2, 200, true),
        (0, 199, false),
        (1, 301, false),
        (1, 99, false),
        (2, 201, false),
    ] {
        timestamp_case(
            &[Some(300), Some(200), Some(100)],
            position,
            Some(timestamp),
            accepted,
        );
    }
    for position in [0, 2] {
        timestamp_case(
            &[Some(100), Some(200), Some(300)],
            position,
            Some(200),
            false,
        );
    }
}

#[test]
fn absent_and_extreme_timestamps_obey_neighbors_and_singleton_bounds() {
    let timestamps = [Some(i64::MAX), Some(0), None];
    for (position, timestamp, accepted) in [
        (0, Some(i64::MAX - 1), true),
        (1, Some(i64::MIN), true),
        (1, None, true),
        (2, Some(i64::MIN), true),
        (0, None, false),
        (2, Some(i64::MAX), false),
    ] {
        timestamp_case(&timestamps, position, timestamp, accepted);
    }
    for (old, new) in [
        (None, Some(i64::MIN)),
        (Some(i64::MIN), Some(i64::MAX)),
        (Some(i64::MAX), None),
    ] {
        timestamp_case(&[old], 0, new, true);
    }
}

#[test]
fn frequency_classes_open_state_and_fallback_use_the_guarded_routes() {
    for fallback in [false, true] {
        for (visits, open, accepted) in [(4, false, true), (8, false, false), (7, true, false)] {
            let mut indexed = catalogue(&[Some(200)], fallback);
            let mut legacy = catalogue(&[Some(200)], fallback);
            legacy
                .ranges
                .as_mut()
                .expect("index")
                .set_in_place_enabled(false);
            let mut changed = indexed.entries[0].clone();
            changed.usage.visits = visits;
            changed.open = open;
            indexed
                .upsert(changed.clone())
                .expect("guarded class update");
            legacy.upsert(changed).expect("legacy class update");
            let observed = counters(&indexed);
            assert_eq!(observed.attempted, 1);
            assert_eq!(observed.declined, usize::from(!accepted));
            assert_eq!(observed.grouped, usize::from(accepted && !fallback));
            assert_eq!(observed.fallback, usize::from(accepted && fallback));
            verify_pair(&mut legacy, &mut indexed);
        }
    }
    let mut indexed = catalogue(&[None], true);
    let before = indexed
        .ranges
        .as_ref()
        .expect("index")
        .state
        .as_ref()
        .expect("state")
        .fallback
        .clone();
    let mut changed = indexed.entries[0].clone();
    changed.usage.last_used = Some(i64::MAX);
    indexed.upsert(changed).expect("fallback timestamp update");
    assert_eq!(counters(&indexed).fallback, 1);
    assert_eq!(
        indexed
            .ranges
            .as_ref()
            .expect("index")
            .state
            .as_ref()
            .expect("state")
            .fallback,
        before
    );
    compare(&mut indexed, "project", Scope::Everything, NOW, policy());
}

#[test]
fn text_changes_never_use_recycled_or_equivalent_group_ids_as_proof() {
    let mut indexed = catalogue(&[Some(200)], false);
    let mut legacy = catalogue(&[Some(200)], false);
    legacy
        .ranges
        .as_mut()
        .expect("index")
        .set_in_place_enabled(false);
    let before = indexed.field_groups[0];
    for (title, address) in [
        ("project row-111 café", "https://in-place.test/000"),
        ("project row-111 café", "https://in-place.test/222"),
        ("project replaced field", "https://replacement.test/"),
    ] {
        let mut changed = indexed.entries[0].clone();
        changed.title = title.into();
        changed.address = address.into();
        let previous = counters(&indexed);
        indexed.upsert(changed.clone()).expect("text replacement");
        legacy.upsert(changed).expect("legacy text replacement");
        assert_eq!(
            indexed.field_groups[0], before,
            "sole field slots are reused"
        );
        assert_eq!(counters(&indexed).declined, previous.declined + 1);
        assert_eq!(counters(&indexed).grouped, 0);
        verify_pair(&mut legacy, &mut indexed);
    }
    let mut changed = indexed.entries[0].clone();
    changed.usage.last_used = None;
    indexed
        .upsert(changed.clone())
        .expect("new fields now eligible");
    legacy.upsert(changed).expect("legacy timestamp");
    assert_eq!(counters(&indexed).grouped, 1);
    verify_pair(&mut legacy, &mut indexed);
}

#[test]
fn disabled_unavailable_noop_and_failed_generation_do_not_take_the_fast_path() {
    for unavailable in [false, true] {
        let mut indexed = catalogue(&[Some(200)], false);
        if unavailable {
            enable(
                &mut indexed,
                Limits {
                    members: 0,
                    ..Limits::default()
                },
            );
        } else {
            indexed
                .ranges
                .as_mut()
                .expect("index")
                .set_in_place_enabled(false);
        }
        let mut changed = indexed.entries[0].clone();
        changed.usage.last_used = None;
        indexed
            .upsert(changed)
            .expect("ordinary lifecycle fallback");
        assert_eq!(
            counters(&indexed),
            InPlaceCounters {
                attempted: 1,
                declined: 1,
                ..InPlaceCounters::default()
            }
        );
        compare(&mut indexed, "project", Scope::Everything, NOW, policy());
    }
    let mut indexed = catalogue(&[Some(200)], false);
    let original = indexed.entries[0].clone();
    let generation = indexed.generation();
    indexed.upsert(original.clone()).expect("identical row");
    assert_eq!(indexed.generation(), generation);
    assert_eq!(counters(&indexed), InPlaceCounters::default());
    indexed.generation = u64::MAX;
    let mut changed = original.clone();
    changed.usage.last_used = None;
    assert_eq!(
        indexed.upsert(changed),
        Err(SearchError::GenerationExhausted)
    );
    assert_eq!(
        indexed.remove(original.target),
        Err(SearchError::GenerationExhausted)
    );
    assert_eq!(counters(&indexed), InPlaceCounters::default());
    assert!(indexed.entries[0] == original);
    check(&indexed);
}

#[test]
fn a_different_target_is_rejected_before_any_member_update() {
    let mut indexed = catalogue(&[Some(200)], false);
    let original = indexed.entries[0].clone();
    let mut changed = original.clone();
    changed.target = entry(8, "", "").target;
    let ids = indexed.field_groups[0];
    assert!(
        !indexed
            .ranges
            .as_mut()
            .expect("index")
            .try_update_in_place(&original, &changed, ids, 0)
    );
    assert_eq!(counters(&indexed).declined, 1);
    assert!(indexed.entries[0] == original);
    check(&indexed);
    let recorded = indexed
        .ranges
        .as_mut()
        .expect("index")
        .take_in_place_counters();
    assert_eq!(recorded.attempted, 1);
    assert_eq!(counters(&indexed), InPlaceCounters::default());
}
