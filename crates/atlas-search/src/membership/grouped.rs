//! Measures query-local membership reuse across bounded canonical field groups.

use std::{
    hint::black_box,
    time::{Duration, Instant},
};

use crate::{
    Entry, TextField,
    candidate::FieldMasks,
    field_groups::{FieldGroups, GroupId, query_safe},
};

use super::indexed::diagnostic::{self, Query, existing_membership};

const PASSES: usize = 5;

pub(super) struct Corpus {
    pub(super) entries: Vec<Entry>,
    pub(super) masks: Vec<FieldMasks>,
    pub(super) groups: FieldGroups,
    pub(super) fields: Vec<[Option<GroupId>; 2]>,
}

fn slot(id: GroupId) -> usize {
    id.get() as usize - 1
}

pub(super) fn corpus() -> Corpus {
    let entries = diagnostic::entries();
    let masks = entries.iter().map(FieldMasks::new).collect();
    let started = Instant::now();
    let mut groups = FieldGroups::new();
    let fields: Vec<_> = entries
        .iter()
        .map(|entry| [groups.intern(&entry.title), groups.intern(&entry.address)])
        .collect();
    let build = started.elapsed();
    let uncached = fields.iter().flatten().filter(|id| id.is_none()).count();
    let references = std::mem::size_of_val(fields.as_slice());
    println!(
        "groups rows={} groups={} slots={} uncached_fields={uncached} key_payload_bytes={} row_reference_bytes={references} key_payload_plus_row_references_bytes={} build_ms={:.3}; excludes map, group slots, Arc headers, free list, allocator overhead, source strings and query-local buffers",
        entries.len(),
        groups.len(),
        groups.slots(),
        groups.storage_bytes(),
        groups.storage_bytes() + references,
        build.as_secs_f64() * 1_000.,
    );
    Corpus {
        entries,
        masks,
        groups,
        fields,
    }
}

fn field_matches<const GROUPED: bool>(
    corpus: &Corpus,
    query: &Query,
    row: usize,
    field: TextField,
    memo: &mut [Option<bool>],
) -> bool {
    let masks = &corpus.masks[row];
    if !masks.may_match(field, &query.mask, query.budget) {
        return false;
    }
    let (text, index) = match field {
        TextField::Title => (corpus.entries[row].title.as_str(), 0),
        TextField::Address => (corpus.entries[row].address.as_str(), 1),
    };
    if GROUPED
        && !memo.is_empty()
        && let Some(id) = corpus.fields[row][index]
    {
        let slot = slot(id);
        if let Some(matched) = memo[slot] {
            return matched;
        }
        let canonical = corpus
            .groups
            .get(id)
            .expect("retained synthetic field group");
        // Digit canonicalization leaves each group's ASCII status unchanged.
        let matched = existing_membership(canonical, masks.is_ascii(field), query);
        memo[slot] = Some(matched);
        return matched;
    }
    existing_membership(text, masks.is_ascii(field), query)
}

pub(super) fn scan<const GROUPED: bool>(
    corpus: &Corpus,
    query: &Query,
    safe: bool,
) -> (Vec<bool>, Vec<Option<bool>>) {
    // Every scan starts with an empty memo; nothing persists between repeated queries.
    let mut memo = if GROUPED && safe {
        vec![None; corpus.groups.slots()]
    } else {
        Vec::new()
    };
    let matched = (0..corpus.entries.len())
        .map(|row| {
            field_matches::<GROUPED>(corpus, query, row, TextField::Title, &mut memo)
                || field_matches::<GROUPED>(corpus, query, row, TextField::Address, &mut memo)
        })
        .collect();
    (matched, memo)
}

fn sample<const GROUPED: bool>(
    corpus: &Corpus,
    query: &Query,
    safe: bool,
    expected: &[bool],
) -> Duration {
    let started = Instant::now();
    let (actual, memo) = scan::<GROUPED>(black_box(corpus), black_box(query), safe);
    let elapsed = started.elapsed();
    assert_eq!(actual, expected);
    // Reading and dropping the memo after timing keeps its allocation in the measured query.
    drop(black_box(memo));
    elapsed
}

fn compare_case(corpus: &Corpus, label: &str, text: &str) {
    let query = Query::new(text);
    let safe = query_safe(text);
    let (expected, _) = scan::<false>(corpus, &query, safe);
    let (candidate, memo) = scan::<true>(corpus, &query, safe);
    assert_eq!(candidate, expected);
    if !safe {
        assert!(memo.is_empty(), "ASCII-digit queries must bypass groups");
    }
    let evaluated = memo.iter().filter(|matched| matched.is_some()).count();
    for _ in 0..2 {
        sample::<false>(corpus, &query, safe, &expected);
        sample::<true>(corpus, &query, safe, &expected);
    }
    let mut existing = Vec::new();
    let mut grouped = Vec::new();
    for pass in 0..PASSES {
        if pass % 2 == 0 {
            existing.push(sample::<false>(corpus, &query, safe, &expected));
            grouped.push(sample::<true>(corpus, &query, safe, &expected));
        } else {
            grouped.push(sample::<true>(corpus, &query, safe, &expected));
            existing.push(sample::<false>(corpus, &query, safe, &expected));
        }
    }
    existing.sort_unstable();
    grouped.sort_unstable();
    println!(
        "{label} matches={} groups_evaluated={evaluated} existing_median_ms={:.3} grouped_median_ms={:.3} existing_worst_ms={:.3} grouped_worst_ms={:.3}",
        expected.iter().filter(|matched| **matched).count(),
        existing[PASSES / 2].as_secs_f64() * 1_000.,
        grouped[PASSES / 2].as_secs_f64() * 1_000.,
        existing[PASSES - 1].as_secs_f64() * 1_000.,
        grouped[PASSES - 1].as_secs_f64() * 1_000.,
    );
}

#[test]
fn grouped_workload_diagnostic_preserves_every_row_membership() {
    println!(
        "Synthetic membership-only diagnostic, not D48 acceptance. Query-local memo allocation is timed; query preparation, ranking, counts, scoring and highlights are not. Two warmups and five alternating paired samples; full row equality checked outside timing."
    );
    let corpus = corpus();
    assert_eq!(corpus.entries.len(), diagnostic::ROWS);
    for (label, text) in diagnostic::cases() {
        compare_case(&corpus, label, &text);
    }
}
