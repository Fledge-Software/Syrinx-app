//! Prototypes short-field membership by advancing through cached character positions.

use std::num::NonZeroU8;

use super::{deletion_budget, unicode_counterpart};

const MAX_FIELD_UNITS: usize = 64;
const UNREACHABLE: u8 = 65;

pub(crate) struct FieldIndex {
    ascii: u128,
    masks: Box<[u64]>,
    non_ascii: Box<[char]>,
    length_plus_one: NonZeroU8,
}

impl FieldIndex {
    pub(crate) fn new(field: &str) -> Option<Self> {
        let length = field.chars().count();
        if length > MAX_FIELD_UNITS {
            return None;
        }
        let length_plus_one = NonZeroU8::new(length as u8 + 1)?;
        let mut positions: Vec<(char, u64)> = Vec::new();
        for (position, character) in field.chars().enumerate() {
            let bit = 1u64 << position;
            if let Some((_, mask)) = positions.iter_mut().find(|(key, _)| *key == character) {
                *mask |= bit;
            } else {
                positions.push((character, bit));
            }
        }
        positions.sort_unstable_by_key(|(character, _)| *character);
        let ascii = positions
            .iter()
            .filter(|(character, _)| character.is_ascii())
            .fold(0, |presence, (character, _)| {
                presence | (1u128 << *character as u32)
            });
        let non_ascii = positions
            .iter()
            .filter_map(|(character, _)| (!character.is_ascii()).then_some(*character))
            .collect();
        let masks = positions.into_iter().map(|(_, mask)| mask).collect();
        Some(Self {
            ascii,
            masks,
            non_ascii,
            length_plus_one,
        })
    }

    fn positions(&self, character: char) -> u64 {
        let index = if character.is_ascii() {
            let bit = 1u128 << character as u32;
            if self.ascii & bit == 0 {
                return 0;
            }
            (self.ascii & (bit - 1)).count_ones() as usize
        } else {
            let Ok(index) = self.non_ascii.binary_search(&character) else {
                return 0;
            };
            self.ascii.count_ones() as usize + index
        };
        self.masks[index]
    }

    pub(crate) fn storage_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + std::mem::size_of_val(self.masks.as_ref())
            + std::mem::size_of_val(self.non_ascii.as_ref())
    }
}

pub(crate) struct IndexedQuery {
    pairs: Vec<(char, char)>,
}

impl IndexedQuery {
    pub(crate) fn new(query: &str) -> Self {
        let case_sensitive = query.chars().any(char::is_uppercase);
        Self {
            pairs: query
                .chars()
                .map(|character| (character, unicode_counterpart(character, case_sensitive)))
                .collect(),
        }
    }

    pub(crate) fn matches(&self, field: &FieldIndex, max_typos: u16) -> Option<bool> {
        let allow_deletion = deletion_budget(max_typos)?;
        let length = usize::from(field.length_plus_one.get() - 1);
        if self.pairs.len().saturating_sub(usize::from(allow_deletion)) > length {
            return Some(false);
        }
        let mut exact = 0;
        let mut deleted = UNREACHABLE;
        for &(original, counterpart) in &self.pairs {
            let mut positions = field.positions(original);
            if original != counterpart {
                positions |= field.positions(counterpart);
            }
            // The earliest endpoint dominates later endpoints with the same deletion count.
            let next_exact = successor(positions, exact);
            if allow_deletion {
                deleted = successor(positions, deleted).min(exact);
            }
            exact = next_exact;
            if exact == UNREACHABLE && deleted == UNREACHABLE {
                return Some(false);
            }
        }
        Some(exact != UNREACHABLE || deleted != UNREACHABLE)
    }
}

fn successor(positions: u64, start: u8) -> u8 {
    if start >= MAX_FIELD_UNITS as u8 {
        return UNREACHABLE;
    }
    let remaining = positions >> start;
    if remaining == 0 {
        UNREACHABLE
    } else {
        start + remaining.trailing_zeros() as u8 + 1
    }
}

#[cfg(test)]
pub(crate) mod diagnostic {
    use std::{
        hint::black_box,
        num::NonZeroU64,
        time::{Duration, Instant},
    };

    use syrinx_core::{SavedPageId, TabId};

    use super::*;
    use crate::{
        Entry, Target, TextField, Usage,
        candidate::{AsciiPresence, FieldMasks},
        membership::Membership,
    };

    pub(crate) const ROWS: usize = 100_000;
    const PASSES: usize = 5;
    const LONG_FRAGMENT: &str = "reproducible synthetic catalogue exploration includes detailed release notes and project reference material for keyboard navigation";
    const TOPICS: [&str; 8] = [
        "amber", "birch", "cobalt", "delta", "ember", "forest", "granite", "harbor",
    ];
    const PURPOSES: [&str; 8] = [
        "project guide",
        "release notes",
        "planning reference",
        "workshop manual",
        "example collection",
        "design journal",
        "archive index",
        "navigation handbook",
    ];
    const UNICODE: [&str; 5] = ["café", "cafe\u{301}", "東京", "🧭", "naïve résumé"];

    struct Corpus {
        entries: Vec<Entry>,
        masks: Vec<FieldMasks>,
        indices: Vec<[Option<FieldIndex>; 2]>,
    }

    pub(crate) struct Query {
        existing: Membership,
        indexed: IndexedQuery,
        pub(crate) mask: AsciiPresence,
        pub(crate) budget: u16,
    }

    impl Query {
        pub(crate) fn new(text: &str) -> Self {
            Self {
                existing: Membership::new(text),
                indexed: IndexedQuery::new(text),
                mask: AsciiPresence::new(text),
                budget: u16::from(text.chars().count() >= 4),
            }
        }
    }

    fn target(index: usize) -> Target {
        let value = NonZeroU64::new(index as u64 + 1).expect("synthetic nonzero identity");
        match index {
            0..256 => Target::Tab(TabId::new(value.get()).expect("synthetic tab identity")),
            256..20_256 => {
                Target::Saved(SavedPageId::new(value.get()).expect("synthetic saved identity"))
            }
            20_256..99_936 => Target::History(value),
            _ => Target::Command(value),
        }
    }

    fn entry(index: usize) -> Entry {
        // Mirror the unchanged xtask workload's field shapes and insertion permutation.
        let topic = TOPICS[index % TOPICS.len()];
        let purpose = PURPOSES[(index / TOPICS.len()) % PURPOSES.len()];
        let mut title = format!("{topic} {purpose} row-{index:06}");
        if index % 10 < 2 {
            title.push(' ');
            title.push_str(UNICODE[(index / 10) % UNICODE.len()]);
        }
        if index % 10 == 2 {
            title.push(' ');
            title.push_str(LONG_FRAGMENT);
            title.push(' ');
            title.push_str(LONG_FRAGMENT);
        }
        if index.is_multiple_of(101) {
            title.make_ascii_uppercase();
        }
        let target = target(index);
        let address = if matches!(target, Target::Command(_)) {
            String::new()
        } else {
            format!(
                "https://catalogue-{:03}.test/{topic}/{}/row-{index:06}",
                index % 127,
                if index.is_multiple_of(97) {
                    "urlmarker"
                } else {
                    "reference"
                },
            )
        };
        Entry {
            target,
            title,
            address,
            usage: Usage {
                visits: ((index * 73) % 1_001) as u32,
                last_used: (!index.is_multiple_of(17))
                    .then_some(1_800_000_000 - ((index % 9) as i64 * 86_400)),
            },
            open: matches!(target, Target::Tab(_))
                || (matches!(target, Target::Saved(_)) && index.is_multiple_of(13)),
        }
    }

    pub(crate) fn entries() -> Vec<Entry> {
        (0..ROWS)
            .map(|position| entry((position * 7_919) % ROWS))
            .collect()
    }

    fn corpus() -> Corpus {
        let entries = entries();
        let masks = entries.iter().map(FieldMasks::new).collect();
        let started = Instant::now();
        let indices: Vec<_> = entries
            .iter()
            .map(|entry| {
                [
                    FieldIndex::new(&entry.title),
                    FieldIndex::new(&entry.address),
                ]
            })
            .collect();
        let build = started.elapsed();
        let indexed_fields = indices
            .iter()
            .flatten()
            .filter(|index| index.is_some())
            .count();
        let payload: usize = indices
            .iter()
            .flatten()
            .filter_map(Option::as_ref)
            .map(|index| index.storage_bytes() - std::mem::size_of::<FieldIndex>())
            .sum();
        let slots = std::mem::size_of_val(indices.as_slice());
        println!(
            "index rows={ROWS} indexed_fields={indexed_fields} fallback_fields={} slot_bytes={slots} payload_bytes={payload} retained_bytes={} build_ms={:.3}; excludes allocator overhead and query buffers",
            ROWS * 2 - indexed_fields,
            slots + payload,
            build.as_secs_f64() * 1_000.,
        );
        Corpus {
            entries,
            masks,
            indices,
        }
    }

    fn field_matches<const INDEXED: bool>(
        corpus: &Corpus,
        query: &Query,
        row: usize,
        field: TextField,
    ) -> bool {
        let masks = &corpus.masks[row];
        if !masks.may_match(field, &query.mask, query.budget) {
            return false;
        }
        let (text, slot) = match field {
            TextField::Title => (corpus.entries[row].title.as_str(), 0),
            TextField::Address => (corpus.entries[row].address.as_str(), 1),
        };
        if INDEXED && let Some(index) = &corpus.indices[row][slot] {
            return query
                .indexed
                .matches(index, query.budget)
                .expect("supported diagnostic budget");
        }
        existing_membership(text, masks.is_ascii(field), query)
    }

    pub(crate) fn existing_membership(text: &str, ascii: bool, query: &Query) -> bool {
        if ascii {
            query.existing.matches_ascii(text, query.budget)
        } else {
            query.existing.matches(text, query.budget)
        }
        .expect("supported diagnostic budget")
    }

    fn scan<const INDEXED: bool>(corpus: &Corpus, query: &Query) -> Vec<bool> {
        (0..corpus.entries.len())
            .map(|row| {
                field_matches::<INDEXED>(corpus, query, row, TextField::Title)
                    || field_matches::<INDEXED>(corpus, query, row, TextField::Address)
            })
            .collect()
    }

    fn sample<const INDEXED: bool>(corpus: &Corpus, query: &Query, expected: &[bool]) -> Duration {
        let started = Instant::now();
        let actual = scan::<INDEXED>(black_box(corpus), black_box(query));
        let elapsed = started.elapsed();
        // Full row-by-row union comparison is outside the measured membership loop.
        assert_eq!(actual, expected);
        elapsed
    }

    fn compare_case(corpus: &Corpus, label: &str, text: &str) {
        let query = Query::new(text);
        let expected = scan::<false>(corpus, &query);
        assert_eq!(scan::<true>(corpus, &query), expected);
        for _ in 0..2 {
            sample::<false>(corpus, &query, &expected);
            sample::<true>(corpus, &query, &expected);
        }
        let mut existing = Vec::new();
        let mut indexed = Vec::new();
        for pass in 0..PASSES {
            // Alternate order so either implementation can be measured first.
            if pass % 2 == 0 {
                existing.push(sample::<false>(corpus, &query, &expected));
                indexed.push(sample::<true>(corpus, &query, &expected));
            } else {
                indexed.push(sample::<true>(corpus, &query, &expected));
                existing.push(sample::<false>(corpus, &query, &expected));
            }
        }
        existing.sort_unstable();
        indexed.sort_unstable();
        println!(
            "{label} matches={} existing_median_ms={:.3} indexed_median_ms={:.3} existing_worst_ms={:.3} indexed_worst_ms={:.3}",
            expected.iter().filter(|matched| **matched).count(),
            existing[PASSES / 2].as_secs_f64() * 1_000.,
            indexed[PASSES / 2].as_secs_f64() * 1_000.,
            existing[PASSES - 1].as_secs_f64() * 1_000.,
            indexed[PASSES - 1].as_secs_f64() * 1_000.,
        );
    }

    pub(crate) fn cases() -> Vec<(&'static str, String)> {
        let mut cases: Vec<_> = [
            ("broad", "a"),
            ("typed-two", "pr"),
            ("typed-three", "pro"),
            ("word", "project"),
            ("uppercase", "PROJECT"),
            ("address-only", "urlmarker"),
            ("rare-identity", "row-099983"),
            ("composed", "café"),
            ("decomposed", "cafe\u{301}"),
            ("cjk", "東京"),
            ("emoji", "🧭"),
            ("no-match", "qzxvjk-no-result"),
            ("long-match", &LONG_FRAGMENT[..96]),
        ]
        .into_iter()
        .map(|(label, query)| (label, query.to_owned()))
        .collect();
        cases.push(("long-near-match", format!("{}xyz", &LONG_FRAGMENT[..93])));
        cases
    }

    #[test]
    fn positional_index_workload_diagnostic_preserves_all_membership() {
        println!(
            "Synthetic membership-only diagnostic, not D48 acceptance. No scoring, scope counts, ranking or highlights are timed; query preparation is outside timing. Two warmups and five alternating paired samples."
        );
        let corpus = corpus();
        for (label, query) in cases() {
            compare_case(&corpus, label, &query);
        }
    }
}

#[cfg(test)]
mod tests {
    use frizbee::{Config, Matcher, SortStrategy};

    use super::*;
    use crate::membership::Membership;

    fn words(alphabet: &[char], maximum: usize) -> Vec<String> {
        let mut all = vec![String::new()];
        let mut frontier = vec![String::new()];
        for _ in 0..maximum {
            frontier = frontier
                .iter()
                .flat_map(|prefix| {
                    alphabet
                        .iter()
                        .map(move |character| format!("{prefix}{character}"))
                })
                .collect();
            all.extend(frontier.iter().cloned());
        }
        all
    }

    #[test]
    fn indexed_endpoints_match_existing_membership_exhaustively() {
        let fields = words(&['a', 'B', '\0', 'é', 'É'], 4);
        let indices: Vec<_> = fields
            .iter()
            .map(|field| FieldIndex::new(field).expect("short synthetic field"))
            .collect();
        for query in words(&['a', 'B', '\0', 'é', 'É'], 3) {
            let indexed = IndexedQuery::new(&query);
            let existing = Membership::new(&query);
            for (field, index) in fields.iter().zip(&indices) {
                for budget in [0, 1] {
                    let expected = if field.is_ascii() {
                        existing.matches_ascii(field, budget)
                    } else {
                        existing.matches(field, budget)
                    };
                    assert_eq!(
                        indexed.matches(index, budget),
                        expected,
                        "synthetic query {query:?}, field {field:?}, budget {budget}"
                    );
                }
            }
        }
    }

    fn compare_public(query: &str, fields: &[String]) {
        let indexed = IndexedQuery::new(query);
        let borrowed: Vec<_> = fields.iter().map(String::as_str).collect();
        for budget in [0, 1] {
            let config = Config::default()
                .max_typos(Some(budget))
                .sort(SortStrategy::IndexAsc);
            let found = Matcher::new(query, &config).match_list(&borrowed);
            let mut expected = vec![false; fields.len()];
            for matched in found {
                expected[matched.index as usize] = true;
            }
            for (field, expected) in fields.iter().zip(expected) {
                let index = FieldIndex::new(field).expect("short synthetic field");
                assert_eq!(
                    indexed.matches(&index, budget),
                    Some(expected),
                    "synthetic query {query:?}, field {field:?}, budget {budget}"
                );
            }
        }
    }

    #[test]
    fn case_pairs_unicode_width_and_nul_match_the_pinned_public_matcher() {
        let fields: Vec<_> = [
            "",
            "a",
            "aa",
            "aaa",
            "aBa",
            "aba",
            "café",
            "CAFÉ",
            "cafe\u{301}",
            "Éa",
            "éA",
            "İi",
            "ıI",
            "ßẞ",
            "ssSS",
            "KkK",
            "ﬀff",
            "σΣς",
            "東京🧭",
            "a\0B",
            "\0\0",
            "ab",
            "ac",
            "bc",
            "abc",
            "éa🧭B\0",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        for query in [
            "",
            "a",
            "aa",
            "aaa",
            "aba",
            "aB",
            "café",
            "CAFÉ",
            "cafe\u{301}",
            "Éa",
            "éa",
            "İ",
            "ı",
            "ß",
            "ẞ",
            "K",
            "ﬀ",
            "σ",
            "Σ",
            "ς",
            "東京",
            "🧭",
            "\0",
            "a\0",
            "\0\0",
            "abc",
            "abxc",
            "xabc",
            "abcx",
        ] {
            compare_public(query, &fields);
        }
    }

    #[test]
    fn sixty_four_positions_never_shift_by_word_width_or_reuse_a_character() {
        for unit in ['a', '\0', 'é', '🧭'] {
            let fields: Vec<_> = [0, 1, 62, 63, 64]
                .into_iter()
                .map(|length| unit.to_string().repeat(length))
                .collect();
            for length in [0, 1, 2, 62, 63, 64, 65, 66] {
                compare_public(&unit.to_string().repeat(length), &fields);
            }
            assert!(FieldIndex::new(&unit.to_string().repeat(65)).is_none());
        }
        assert_eq!(successor(1u64 << 63, 63), 64);
        assert_eq!(successor(u64::MAX, 64), UNREACHABLE);
        assert_eq!(successor(u64::MAX, UNREACHABLE), UNREACHABLE);
    }

    #[test]
    fn index_layout_counts_retained_payload_and_preserves_fallback_budgets() {
        let index = FieldIndex::new("aaBBéé\0🧭").expect("short synthetic field");
        assert_eq!(index.ascii.count_ones(), 3);
        assert_eq!(index.non_ascii.as_ref(), &['é', '🧭']);
        assert_eq!(index.masks.len(), 5);
        assert_eq!(
            index.storage_bytes(),
            std::mem::size_of::<FieldIndex>() + 48
        );
        assert_eq!(
            std::mem::size_of::<Option<FieldIndex>>(),
            std::mem::size_of::<FieldIndex>()
        );
        for budget in [2, u16::MAX] {
            assert_eq!(IndexedQuery::new("missing").matches(&index, budget), None);
        }
        let empty = FieldIndex::new("").expect("empty synthetic field");
        assert_eq!(empty.storage_bytes(), std::mem::size_of::<FieldIndex>());
    }
}
