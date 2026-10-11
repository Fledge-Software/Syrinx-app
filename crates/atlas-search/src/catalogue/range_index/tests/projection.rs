//! Measures query preparation and full-row membership with the ASCII projection gate toggled.

use std::time::Duration;

use super::*;
use crate::{catalogue::membership::GroupedMembership, membership::Membership};

fn scan(
    catalogue: &Catalogue,
    text: &str,
    max_typos: u16,
    projection: bool,
) -> (Vec<bool>, Duration) {
    let started = Instant::now();
    let mut membership =
        GroupedMembership::with_ascii_projection(text, max_typos, &catalogue.groups, projection);
    let mut outcomes = Vec::with_capacity(catalogue.len());
    for (position, entry) in catalogue.entries.iter().enumerate() {
        outcomes.push(
            membership
                .entry_matches_counted(
                    entry,
                    &catalogue.field_masks[position],
                    catalogue.field_groups[position],
                    &catalogue.digit_counts[position],
                )
                .expect("synthetic supported membership"),
        );
    }
    // Include query-local allocation cleanup in both modes.
    drop(membership);
    (outcomes, started.elapsed())
}

fn reference(catalogue: &Catalogue, text: &str, max_typos: u16) -> Vec<bool> {
    let membership = Membership::new(text);
    catalogue
        .entries
        .iter()
        .map(|entry| {
            membership
                .matches(&entry.title, max_typos)
                .expect("supported raw title membership")
                || membership
                    .matches(&entry.address, max_typos)
                    .expect("supported raw address membership")
        })
        .collect()
}

fn measure(
    catalogue: &mut Catalogue,
    original: &Entry,
    revision: &mut usize,
    query: &str,
    max_typos: u16,
) -> ([Vec<Duration>; 2], usize) {
    let mut samples: [Vec<_>; 2] = std::array::from_fn(|_| Vec::new());
    let mut matches = 0;
    for pass in 0..7 {
        *revision += 1;
        let mut changed = original.clone();
        changed.title.push_str(&format!(" revision-{revision}"));
        catalogue.upsert(changed).expect("paired workload revision");
        let generation = catalogue.generation();
        let expected = reference(catalogue, query, max_typos);
        matches = expected.iter().filter(|&&matched| matched).count();
        for projection in if pass % 2 == 0 {
            [false, true]
        } else {
            [true, false]
        } {
            let (actual, elapsed) = scan(catalogue, query, max_typos, projection);
            assert_eq!(actual, expected, "all original rows retain membership");
            assert_eq!(catalogue.generation(), generation);
            if pass >= 2 {
                samples[usize::from(projection)].push(elapsed);
            }
        }
    }
    (samples, matches)
}

#[test]
fn projection_membership_workload_diagnostic_preserves_all_rows() {
    println!(
        "Test-only full-row membership microdiagnostic, not D48 acceptance or range candidate traversal. Uses unchanged existing/mixed100k fixtures and fourteen cases, two warmups and five retained same-generation pairs in alternating old/new order. Timing includes query preparation, fresh group memo, outcome vector allocation, all row membership checks and query-local cleanup; old mode skips projection preparation. Revisions, raw unfiltered original-field oracle and every row comparison are outside timing. Ranking, boosts, highlights and owned search hits are absent. No retained query answers or timing thresholds."
    );
    for mixed in [false, true] {
        let mut catalogue = workload_catalogue(mixed);
        let ascii_fields = catalogue
            .entries
            .iter()
            .map(|entry| {
                usize::from(entry.title.is_ascii()) + usize::from(entry.address.is_ascii())
            })
            .sum::<usize>();
        let target = Target::Command(
            NonZeroU64::new(diagnostic::ROWS as u64).expect("last synthetic target"),
        );
        let original = catalogue.get(target).expect("revision row").clone();
        let mut revision = 0;
        for (label, query) in diagnostic::cases() {
            let max_typos = u16::from(query.chars().count() >= 4);
            let (samples, matches) =
                measure(&mut catalogue, &original, &mut revision, &query, max_typos);
            for (mode, mut times) in ["original", "projection"].into_iter().zip(samples) {
                times.sort_unstable();
                assert_eq!(times.len(), 5);
                println!(
                    "projection_membership mixed={mixed} case={label} mode={mode} rows={} ascii_fields={ascii_fields} matches_last_pair={matches} min_ms={:.3} median_ms={:.3} max_ms={:.3}",
                    catalogue.len(),
                    times[0].as_secs_f64() * 1_000.,
                    times[2].as_secs_f64() * 1_000.,
                    times[4].as_secs_f64() * 1_000.
                );
            }
        }
    }
}
