//! Measures retained-field coverage and hypothetical range-index storage on synthetic catalogues.

use std::{collections::BTreeMap, mem::size_of, num::NonZeroU64, time::Instant};

use super::Catalogue;
use crate::{
    BlankQuery, BoostWeights, Entry, Scope, SearchPolicy, Target, field_groups::GroupId,
    membership::indexed::diagnostic,
};

const NOW: i64 = 1_800_000_000;
type Pair = [GroupId; 2];
type Bucket = (Pair, usize, bool, u32);

#[derive(Debug, Eq, PartialEq)]
struct Occupancy {
    rows: usize,
    grouped: usize,
    fallback: usize,
    empty_fields: usize,
    long_fields: usize,
    missing_short_fields: usize,
    pairs: BTreeMap<Pair, [usize; 4]>,
    buckets: BTreeMap<Bucket, usize>,
}

fn kind(target: Target) -> usize {
    match target {
        Target::Tab(_) => 0,
        Target::Saved(_) => 1,
        Target::History(_) => 2,
        Target::Command(_) => 3,
    }
}

fn canonical(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_ascii_digit() {
                '0'
            } else {
                character
            }
        })
        .collect()
}

fn inspect(catalogue: &Catalogue) -> Occupancy {
    assert_eq!(catalogue.entries.len(), catalogue.field_groups.len());
    assert_eq!(catalogue.entries.len(), catalogue.field_masks.len());
    let mut result = Occupancy {
        rows: catalogue.len(),
        grouped: 0,
        fallback: 0,
        empty_fields: 0,
        long_fields: 0,
        missing_short_fields: 0,
        pairs: BTreeMap::new(),
        buckets: BTreeMap::new(),
    };
    let mut referenced = BTreeMap::new();
    let mut all_kinds = [0; 4];
    let mut fallback_kinds = [0; 4];
    for (entry, ids) in catalogue.entries.iter().zip(&catalogue.field_groups) {
        all_kinds[kind(entry.target)] += 1;
        for (field, id) in [&entry.title, &entry.address].into_iter().zip(ids) {
            result.empty_fields += usize::from(field.is_empty());
            result.long_fields += usize::from(field.len() > 1_024);
            let Some(id) = id else {
                result.missing_short_fields += usize::from(field.len() <= 1_024);
                continue;
            };
            let retained = catalogue.groups.get(*id).expect("live field group");
            assert_eq!(retained, canonical(field));
            assert!(retained.len() <= 1_024);
            referenced.insert(*id, retained.len());
        }
        let [Some(title), Some(address)] = *ids else {
            result.fallback += 1;
            fallback_kinds[kind(entry.target)] += 1;
            continue;
        };
        result.grouped += 1;
        let pair = [title, address];
        result.pairs.entry(pair).or_default()[kind(entry.target)] += 1;
        let frequency_class = u32::BITS - entry.usage.visits.leading_zeros();
        *result
            .buckets
            .entry((pair, kind(entry.target), entry.open, frequency_class))
            .or_default() += 1;
    }
    assert_eq!(referenced.len(), catalogue.groups.len());
    assert_eq!(
        referenced.values().sum::<usize>(),
        catalogue.groups.storage_bytes()
    );
    assert_eq!(result.grouped + result.fallback, result.rows);
    assert_eq!(result.buckets.values().sum::<usize>(), result.grouped);
    assert_eq!(
        result.pairs.values().flatten().sum::<usize>(),
        result.grouped
    );
    for row_kind in 0..4 {
        let pair_count: usize = result.pairs.values().map(|counts| counts[row_kind]).sum();
        let bucket_count: usize = result
            .buckets
            .iter()
            .filter(|((_, bucket_kind, _, _), _)| *bucket_kind == row_kind)
            .map(|(_, count)| count)
            .sum();
        assert_eq!(pair_count, bucket_count);
        assert_eq!(pair_count + fallback_kinds[row_kind], all_kinds[row_kind]);
    }
    result
}

fn report(label: &str, catalogue: &Catalogue, occupancy: &Occupancy) {
    let maximum = occupancy.buckets.values().copied().max().unwrap_or(0);
    let tree_slots: usize = occupancy
        .buckets
        .values()
        .map(|count| 2 * count.next_power_of_two())
        .sum();
    let member_bytes = occupancy.grouped * size_of::<(Target, Option<i64>)>();
    let tree_bytes = tree_slots * size_of::<u32>();
    let fallback_bytes = occupancy.fallback * size_of::<Target>();
    // These are explicit hypothetical packed records, not the diagnostic maps' allocations.
    let pair_bytes = occupancy.pairs.len() * (2 * size_of::<u32>() + 4 * size_of::<usize>());
    let bucket_bytes =
        occupancy.buckets.len() * (2 * size_of::<u32>() + 3 + 2 * size_of::<usize>());
    println!(
        "{label}: rows={} grouped={} fallback={} groups={} group_key_bytes={} pairs={} buckets={} max_bucket={maximum} empty_fields={} long_fields={} missing_short_fields={}",
        occupancy.rows,
        occupancy.grouped,
        occupancy.fallback,
        catalogue.groups.len(),
        catalogue.groups.storage_bytes(),
        occupancy.pairs.len(),
        occupancy.buckets.len(),
        occupancy.empty_fields,
        occupancy.long_fields,
        occupancy.missing_short_fields
    );
    println!(
        "  hypothetical_bytes members={member_bytes} padded_u32_tree={tree_bytes} packed_pair_records={pair_bytes} packed_bucket_records={bucket_bytes} stable_fallback_targets={fallback_bytes}; excludes lookup maps, representative handles, capacities, alignment in packed records, allocator overhead and query frontier"
    );
    for cap in [64, 256, 1_024, 4_096] {
        let admitted: usize = occupancy
            .buckets
            .values()
            .filter(|count| **count <= cap)
            .sum();
        println!(
            "  bucket_length_cap={cap} individually_admissible_rows={admitted} all_buckets_fit={}",
            maximum <= cap
        );
    }
    for (members, pairs, buckets, fallback) in [
        (10_000, 256, 1_024, 0),
        (100_000, 1_024, 4_096, 10_000),
        (131_072, 4_096, 16_384, 100_000),
    ] {
        let grouped_fit = occupancy.grouped <= members
            && occupancy.pairs.len() <= pairs
            && occupancy.buckets.len() <= buckets;
        println!(
            "  illustrative_caps members={members} pairs={pairs} buckets={buckets} fallback={fallback}: all_rows_eligible={} partial_storage_eligible={}",
            grouped_fit && occupancy.fallback == 0,
            grouped_fit && occupancy.fallback <= fallback
        );
    }
}

fn policy() -> SearchPolicy {
    SearchPolicy {
        limit: 20,
        max_typos: 0,
        blank: BlankQuery::All,
        boosts: BoostWeights {
            open: 30,
            frequency: 20,
            recency: 10,
            recency_window_secs: NonZeroU64::new(7 * 86_400).expect("positive window"),
        },
    }
}

fn assert_oracle(catalogue: &mut Catalogue, query: &str) {
    let actual = catalogue
        .query(query, Scope::Everything, NOW, policy())
        .expect("ordinary synthetic query");
    let expected = catalogue
        .query_exhaustive(query, Scope::Everything, NOW, policy())
        .expect("exhaustive synthetic query");
    assert_eq!(actual, expected);
}

fn alphabetic(mut index: usize) -> String {
    let mut bytes = [b'a'; 6];
    for byte in &mut bytes {
        *byte += (index % 26) as u8;
        index /= 26;
    }
    assert_eq!(index, 0);
    String::from_utf8(bytes.to_vec()).expect("synthetic ASCII identifier")
}

fn fields(entry: &mut Entry, index: usize, unique: bool) {
    let identifier = if unique {
        alphabetic(index)
    } else {
        format!("{index:06}")
    };
    entry.title = format!("synthetic project {identifier}");
    entry.address = format!("https://occupancy.example.test/{identifier}");
}

fn catalogue(entries: impl IntoIterator<Item = Entry>) -> Catalogue {
    let mut catalogue = Catalogue::new();
    for entry in entries {
        catalogue.upsert(entry).expect("synthetic occupancy row");
    }
    catalogue
}

fn inspect_unchanged(catalogue: &mut Catalogue) -> Occupancy {
    let generation = catalogue.generation();
    let groups = catalogue.field_groups.clone();
    let before = catalogue
        .query("project", Scope::Everything, NOW, policy())
        .expect("query before inspection");
    let occupancy = inspect(catalogue);
    let after = catalogue
        .query("project", Scope::Everything, NOW, policy())
        .expect("query after inspection");
    assert_eq!(before, after);
    assert_eq!(generation, catalogue.generation());
    assert_eq!(groups, catalogue.field_groups);
    assert_oracle(catalogue, "project");
    occupancy
}

fn revision_shapes(catalogue: &mut Catalogue) {
    let target = Target::Command(NonZeroU64::new(diagnostic::ROWS as u64).expect("synthetic ID"));
    let original = catalogue.get(target).expect("last benchmark row").clone();
    for revision in [1, 2, 9, 10, 99, 100, 999, 1_000] {
        let generation = catalogue.generation();
        let position = catalogue.positions[&target];
        let previous = catalogue.field_groups[position][0]
            .and_then(|id| catalogue.groups.get(id))
            .map(str::to_owned);
        let mut changed = original.clone();
        changed.title.push_str(&format!(" revision-{revision}"));
        let started = Instant::now();
        catalogue.upsert(changed).expect("benchmark-shaped update");
        let update = started.elapsed();
        assert_eq!(catalogue.generation(), generation + 1);
        let current = catalogue.field_groups[position][0].and_then(|id| catalogue.groups.get(id));
        let key_changed = previous.as_deref() != current;
        let occupancy = inspect(catalogue);
        println!(
            "  revision={revision} update_us={:.3} retained_group_text_changed={key_changed} generation={} grouped={} pairs={} buckets={}",
            update.as_secs_f64() * 1_000_000.,
            catalogue.generation(),
            occupancy.grouped,
            occupancy.pairs.len(),
            occupancy.buckets.len()
        );
        assert_oracle(catalogue, "revision");
    }
}

#[test]
fn occupancy_diagnostic_preserves_complete_results() {
    println!(
        "Synthetic occupancy only: no range index, query timing claim or D48 acceptance. Existing 100k corpus is unchanged; other labelled corpora are separate deterministic stress fixtures. Logical storage estimates are not allocated-memory measurements. Cap examples are sensitivity probes, not a production recommendation. Empty fields are valid grouped strings; absent IDs mean ordinary fallback."
    );
    for (label, unique_stride, sparse) in [
        ("existing-100k", None, false),
        ("repeated-100k", Some(0), false),
        ("mixed-unique-one-percent", Some(100), false),
        ("mixed-unique-ten-percent", Some(10), false),
        ("unique-alphabetic-100k", Some(1), false),
        ("repeated-empty-and-long", Some(0), true),
    ] {
        let entries = diagnostic::entries()
            .into_iter()
            .enumerate()
            .map(|(index, mut entry)| {
                if let Some(stride) = unique_stride {
                    fields(&mut entry, index, stride != 0 && index % stride == 0);
                }
                if sparse && index % 7 == 0 {
                    entry.address.clear();
                }
                if sparse && index % 11 == 0 {
                    entry.title = format!("project {}", "x".repeat(1_025));
                }
                entry
            });
        let mut catalogue = catalogue(entries);
        let occupancy = inspect_unchanged(&mut catalogue);
        report(label, &catalogue, &occupancy);
        if unique_stride == Some(1) {
            assert_eq!(catalogue.groups.len(), 2_048);
            assert_eq!(occupancy.grouped, 1_024);
            assert_eq!(occupancy.fallback, diagnostic::ROWS - 1_024);
        }
        if unique_stride == Some(0) && !sparse {
            assert_eq!(occupancy.grouped, diagnostic::ROWS);
            assert_eq!(occupancy.pairs.len(), 1);
        }
        revision_shapes(&mut catalogue);
    }
}

#[test]
fn pool_boundary_and_empty_long_fields_are_counted_separately() {
    let entries = diagnostic::entries()
        .into_iter()
        .take(1_025)
        .enumerate()
        .map(|(index, mut entry)| {
            fields(&mut entry, index, true);
            entry
        });
    let mut catalogue = catalogue(entries);
    let full = inspect(&catalogue);
    assert_eq!((full.grouped, full.fallback), (1_024, 1));
    assert_eq!(full.missing_short_fields, 2);
    let target = catalogue.entries[0].target;
    assert!(catalogue.remove(target).expect("free two group slots"));
    let mut changed = catalogue.entries[0].clone();
    changed.title.clear();
    changed.address = "x".repeat(1_025);
    catalogue.upsert(changed).expect("empty and long fields");
    let after = inspect(&catalogue);
    assert_eq!((after.empty_fields, after.long_fields), (1, 1));
    assert_eq!(after.grouped + after.fallback, 1_024);
    assert_oracle(&mut catalogue, "project");
}
