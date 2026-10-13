//! Prototypes shared non-digit transitions while matching each row's original numeric bytes.

use std::{
    ops::Range,
    time::{Duration, Instant},
};

use crate::{
    SearchError,
    field_groups::{FieldGroups, GroupId},
};

use super::ascii_counterpart;

mod digits;
use digits::DigitTransitions;

const MAX_FIELD_BYTES: usize = 1_024;
const IDENTITY_TABLE: u32 = u32::MAX;

#[derive(Clone, Copy)]
struct Limits {
    static_steps: usize,
    payload_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            static_steps: 1_000_000,
            payload_bytes: 512 * 1_024,
        }
    }
}

#[derive(Clone, Copy, Default, Debug, Eq, PartialEq)]
struct Statistics {
    compiled_groups: usize,
    length_rejected_groups: usize,
    fallback_groups: usize,
    static_steps: usize,
    payload_bytes: usize,
    table_bytes: usize,
    cache_slot_bytes: usize,
    digit_table_bytes: usize,
    digit_table_steps: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DigitMode {
    Scalar,
    Packed,
}

#[derive(Clone, Copy, Default)]
struct State {
    exact: u8,
    deleted: u8,
}

struct Needle {
    pairs: Vec<(u8, u8)>,
    deletion: bool,
    digits_only: bool,
}

impl Needle {
    fn new(text: &str, budget: u16) -> Option<Self> {
        if !text.is_ascii()
            || !(1..=63).contains(&text.len())
            || budget > 1
            || !text.bytes().any(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        let sensitive = text.bytes().any(|byte| byte.is_ascii_uppercase());
        Some(Self {
            pairs: text
                .bytes()
                .map(|byte| (byte, ascii_counterpart(byte, sensitive)))
                .collect(),
            deletion: budget == 1,
            digits_only: text.bytes().all(|byte| byte.is_ascii_digit()),
        })
    }

    fn length(&self) -> u8 {
        self.pairs.len() as u8
    }

    fn initial(&self) -> State {
        State {
            exact: 0,
            deleted: u8::from(self.deletion),
        }
    }

    fn advance(&self, prefix: u8, byte: u8) -> u8 {
        if prefix == self.length() {
            return prefix;
        }
        let (original, counterpart) = self.pairs[usize::from(prefix)];
        prefix + u8::from(byte == original || byte == counterpart)
    }

    fn step(&self, state: State, byte: u8) -> State {
        let exact = self.advance(state.exact, byte);
        let deleted = if self.deletion {
            self.advance(state.deleted, byte)
                .max((exact + 1).min(self.length()))
        } else {
            0
        };
        State { exact, deleted }
    }

    fn fold(&self, mut state: State, bytes: &[u8]) -> State {
        for &byte in bytes {
            state = self.step(state, byte);
        }
        state
    }

    fn accepted(&self, state: State) -> bool {
        state.exact == self.length() || (self.deletion && state.deleted == self.length())
    }
}

struct Layout {
    prefix_end: usize,
    spans: Vec<(Range<usize>, Range<usize>)>,
}

impl Layout {
    fn new(bytes: &[u8]) -> Self {
        let prefix_end = bytes
            .iter()
            .position(u8::is_ascii_digit)
            .unwrap_or(bytes.len());
        let mut spans = Vec::new();
        let mut position = prefix_end;
        while position < bytes.len() {
            let start = position;
            while position < bytes.len() && bytes[position].is_ascii_digit() {
                position += 1;
            }
            let digits = start..position;
            let start = position;
            while position < bytes.len() && !bytes[position].is_ascii_digit() {
                position += 1;
            }
            spans.push((digits, start..position));
        }
        Self { prefix_end, spans }
    }

    fn cost(&self, needle: &Needle) -> (usize, usize) {
        let span_bytes = self.spans.len() * std::mem::size_of::<Span>();
        if needle.digits_only {
            return (0, span_bytes);
        }
        let table_width = needle.pairs.len() + 1;
        let table_count = self
            .spans
            .iter()
            .filter(|(_, fixed)| !fixed.is_empty())
            .count();
        let static_bytes: usize = self.spans.iter().map(|(_, fixed)| fixed.len()).sum();
        let work = self.prefix_end + static_bytes * table_width;
        let bytes = span_bytes + table_count * table_width * std::mem::size_of::<Transition>();
        (work, bytes)
    }
}

struct Span {
    digit_start: u16,
    digit_end: u16,
    table_start: u32,
}

struct Transition {
    zero: u8,
    one: u8,
}

struct CompiledGroup {
    length: u16,
    initial: State,
    spans: Box<[Span]>,
    tables: Box<[Transition]>,
}

impl CompiledGroup {
    fn compile(needle: &Needle, bytes: &[u8], layout: Layout) -> Self {
        // Non-digit bytes cannot advance a numeric-only query's already deletion-closed state.
        let initial = if needle.digits_only {
            needle.initial()
        } else {
            needle.fold(needle.initial(), &bytes[..layout.prefix_end])
        };
        let mut spans = Vec::with_capacity(layout.spans.len());
        let mut tables = Vec::new();
        for (digits, fixed) in layout.spans {
            let table_start = if needle.digits_only || fixed.is_empty() {
                IDENTITY_TABLE
            } else {
                let start = tables.len() as u32;
                for prefix in 0..=needle.length() {
                    let input = State {
                        exact: prefix,
                        deleted: (prefix + u8::from(needle.deletion)).min(needle.length()),
                    };
                    let output = needle.fold(input, &bytes[fixed.clone()]);
                    tables.push(Transition {
                        zero: output.exact,
                        one: output.deleted,
                    });
                }
                start
            };
            spans.push(Span {
                digit_start: digits.start as u16,
                digit_end: digits.end as u16,
                table_start,
            });
        }
        Self {
            length: bytes.len() as u16,
            initial,
            spans: spans.into_boxed_slice(),
            tables: tables.into_boxed_slice(),
        }
    }

    fn matches(
        &self,
        needle: &Needle,
        bytes: &[u8],
        digits: Option<&DigitTransitions>,
    ) -> Result<bool, SearchError> {
        if bytes.len() != usize::from(self.length) {
            return Err(SearchError::InconsistentMatch);
        }
        let mut state = self.initial;
        if needle.accepted(state) {
            return Ok(true);
        }
        for span in &self.spans {
            // Only the original row supplies numeric bytes; canonical zeroes are never matched.
            let run = &bytes[usize::from(span.digit_start)..usize::from(span.digit_end)];
            let Some(next) = digit_run(needle, digits, state, run) else {
                return Ok(true);
            };
            state = next;
            if span.table_start != IDENTITY_TABLE {
                let start = span.table_start as usize;
                let exact = &self.tables[start + usize::from(state.exact)];
                // A deletion was spent before this span, or it is spent within this span.
                let deleted = if needle.deletion {
                    self.tables[start + usize::from(state.deleted)]
                        .zero
                        .max(exact.one)
                } else {
                    0
                };
                state = State {
                    exact: exact.zero,
                    deleted,
                };
                if needle.accepted(state) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}

fn digit_run(
    needle: &Needle,
    digits: Option<&DigitTransitions>,
    mut state: State,
    bytes: &[u8],
) -> Option<State> {
    if let Some(digits) = digits {
        return digits.fold(state, bytes);
    }
    for &byte in bytes {
        state = needle.step(state, byte);
        if needle.accepted(state) {
            return None;
        }
    }
    Some(state)
}

#[derive(Default)]
enum CachedGroup {
    #[default]
    Unseen,
    Rejected {
        length: u16,
    },
    Fallback,
    Compiled(CompiledGroup),
}

struct NumericMembership<'a> {
    needle: Option<Needle>,
    groups: &'a FieldGroups,
    cache: Vec<CachedGroup>,
    limits: Limits,
    statistics: Statistics,
    compilation_time: Option<Duration>,
    digits: Option<DigitTransitions>,
    digit_preparation_time: Duration,
}

impl<'a> NumericMembership<'a> {
    fn new(text: &str, budget: u16, groups: &'a FieldGroups, limits: Limits) -> Self {
        Self::with_mode(text, budget, groups, limits, DigitMode::Scalar, false)
    }

    fn with_mode(
        text: &str,
        budget: u16,
        groups: &'a FieldGroups,
        limits: Limits,
        mode: DigitMode,
        profile: bool,
    ) -> Self {
        let needle = Needle::new(text, budget);
        let started = (profile && mode == DigitMode::Packed).then(Instant::now);
        let digits = if mode == DigitMode::Packed {
            needle.as_ref().and_then(DigitTransitions::new)
        } else {
            None
        };
        let digit_preparation_time = started.map_or(Duration::ZERO, |started| started.elapsed());
        let cache: Vec<_> = if needle.is_some() {
            (0..groups.slots()).map(|_| CachedGroup::Unseen).collect()
        } else {
            Vec::new()
        };
        let statistics = Statistics {
            cache_slot_bytes: std::mem::size_of_val(cache.as_slice()),
            digit_table_bytes: digits.as_ref().map_or(0, DigitTransitions::bytes),
            digit_table_steps: digits.as_ref().map_or(0, DigitTransitions::steps),
            ..Statistics::default()
        };
        Self {
            needle,
            groups,
            cache,
            limits,
            statistics,
            compilation_time: None,
            digits,
            digit_preparation_time,
        }
    }

    fn matches(&mut self, text: &str, id: Option<GroupId>) -> Result<Option<bool>, SearchError> {
        let (Some(needle), Some(id)) = (&self.needle, id) else {
            return Ok(None);
        };
        let cached = self
            .cache
            .get_mut((id.get() - 1) as usize)
            .ok_or(SearchError::InconsistentMatch)?;
        if matches!(cached, CachedGroup::Unseen) {
            let canonical = self.groups.get(id).ok_or(SearchError::InconsistentMatch)?;
            let started = self.compilation_time.map(|_| Instant::now());
            *cached = compile_bounded(needle, canonical, self.limits, &mut self.statistics);
            if let (Some(total), Some(started)) = (&mut self.compilation_time, started) {
                *total += started.elapsed();
            }
        }
        match cached {
            CachedGroup::Compiled(group) => group
                .matches(needle, text.as_bytes(), self.digits.as_ref())
                .map(Some),
            CachedGroup::Rejected { length } if text.len() == usize::from(*length) => {
                Ok(Some(false))
            }
            CachedGroup::Rejected { .. } => Err(SearchError::InconsistentMatch),
            CachedGroup::Fallback => Ok(None),
            CachedGroup::Unseen => Err(SearchError::InconsistentMatch),
        }
    }
}

fn compile_bounded(
    needle: &Needle,
    canonical: &str,
    limits: Limits,
    statistics: &mut Statistics,
) -> CachedGroup {
    if canonical.len() > MAX_FIELD_BYTES {
        statistics.fallback_groups += 1;
        return CachedGroup::Fallback;
    }
    // Every retained ASCII query unit needs a distinct field byte, even in Unicode fields.
    if needle.pairs.len() - usize::from(needle.deletion) > canonical.len() {
        statistics.length_rejected_groups += 1;
        return CachedGroup::Rejected {
            length: canonical.len() as u16,
        };
    }
    let layout = Layout::new(canonical.as_bytes());
    let (work, bytes) = layout.cost(needle);
    if work > limits.static_steps.saturating_sub(statistics.static_steps)
        || bytes
            > limits
                .payload_bytes
                .saturating_sub(statistics.payload_bytes)
    {
        statistics.fallback_groups += 1;
        return CachedGroup::Fallback;
    }
    let compiled = CompiledGroup::compile(needle, canonical.as_bytes(), layout);
    statistics.compiled_groups += 1;
    statistics.static_steps += work;
    statistics.payload_bytes += bytes;
    statistics.table_bytes += std::mem::size_of_val(compiled.tables.as_ref());
    CachedGroup::Compiled(compiled)
}

#[cfg(test)]
mod tests {
    use frizbee::{Config, Matcher, SortStrategy};

    use super::*;
    use crate::membership::{Membership, ordered_match};

    fn words(alphabet: &[char], maximum: usize) -> Vec<String> {
        let mut result = vec![String::new()];
        let mut frontier = vec![String::new()];
        for _ in 0..maximum {
            frontier = frontier
                .iter()
                .flat_map(|prefix| {
                    alphabet
                        .iter()
                        .map(move |letter| format!("{prefix}{letter}"))
                })
                .collect();
            result.extend(frontier.iter().cloned());
        }
        result
    }

    fn compare(query: &str, fields: &[String], budget: u16, public: bool) {
        let mut groups = FieldGroups::new();
        let ids: Vec<_> = fields.iter().map(|field| groups.intern(field)).collect();
        let mut numeric = NumericMembership::new(query, budget, &groups, Limits::default());
        let mut packed = NumericMembership::with_mode(
            query,
            budget,
            &groups,
            Limits::default(),
            DigitMode::Packed,
            false,
        );
        let needle = Needle::new(query, budget).expect("supported synthetic query");
        let membership = Membership::new(query);
        let expected = if public {
            let config = Config::default()
                .max_typos(Some(budget))
                .sort(SortStrategy::IndexAsc);
            let borrowed: Vec<_> = fields.iter().map(String::as_str).collect();
            let mut expected = vec![false; fields.len()];
            for found in Matcher::new(query, &config).match_list(&borrowed) {
                expected[found.index as usize] = true;
            }
            Some(expected)
        } else {
            None
        };
        for (row, field) in fields.iter().enumerate() {
            let scalar = ordered_match(&needle.pairs, field.bytes(), budget == 1);
            assert_eq!(membership.matches(field, budget), Some(scalar));
            let actual = numeric.matches(field, ids[row]).expect("retained group");
            assert_eq!(
                actual,
                Some(scalar),
                "query={query:?} field={field:?} budget={budget}"
            );
            assert_eq!(
                packed
                    .matches(field, ids[row])
                    .expect("retained packed group"),
                actual
            );
            if let Some(expected) = &expected {
                assert_eq!(
                    scalar, expected[row],
                    "public query={query:?} field={field:?} budget={budget}"
                );
            }
        }
        assert!(numeric.statistics.compiled_groups > 0);
    }

    #[test]
    fn shared_spans_match_scalar_and_bitset_across_short_digit_queries() {
        let fields = words(&['a', 'B', '1', '2', '\0', 'é'], 4);
        for query in words(&['a', 'B', '0', '1', '\0'], 3) {
            if !query.bytes().any(|byte| byte.is_ascii_digit()) {
                continue;
            }
            for budget in [0, 1] {
                compare(&query, &fields, budget, false);
            }
        }
    }

    #[test]
    fn smartcase_nul_unicode_and_deletion_positions_match_public_matcher() {
        let fields: Vec<_> = [
            "",
            "1",
            "0",
            "11",
            "12",
            "a1b2c",
            "A1B2C",
            "1b2c",
            "a12c",
            "a1bc",
            "a1b2",
            "x1b2c",
            "a1x2c",
            "a1b2x",
            "é1a2東京",
            "é1\0a2",
            "1\0a2",
            "a1é2b",
            "a1b22c",
            "a9b8c",
            "a1b",
            "a2b",
            "a1\0b2",
            "a1🧭2b",
            "12ab34cd56",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        for query in [
            "1",
            "11",
            "12",
            "a1b2c",
            "A1B2C",
            "a1B2c",
            "1\0a2",
            "a12b",
            "12ab34cd56",
        ] {
            for budget in [0, 1] {
                compare(query, &fields, budget, true);
            }
        }
    }

    #[test]
    fn query_boundaries_and_terminal_deletion_match_public_matcher() {
        for length in [1, 2, 62, 63] {
            let query = format!("{}1", "a".repeat(length - 1));
            let fields = vec![
                query.clone(),
                "a".repeat(length - 1),
                format!("{}2", "a".repeat(length - 1)),
                format!("é{query}🧭"),
            ];
            for budget in [0, 1] {
                compare(&query, &fields, budget, true);
            }
        }
        for query in [String::new(), "alpha".into(), "é1".into(), "1".repeat(64)] {
            assert!(Needle::new(&query, 0).is_none());
        }
        assert!(Needle::new("a1", 2).is_none());
    }

    #[test]
    fn packed_query_boundary_and_scalar_fallback_preserve_public_results() {
        for query in ["a1b2c3d4e5f6g7h", "a1b2c3d4e5f6g7hi"] {
            let fields = vec![
                query.to_owned(),
                format!("é{query}🧭"),
                query.replace('3', ""),
                query.replace('1', "9"),
            ];
            for budget in [0, 1] {
                compare(query, &fields, budget, true);
                let mut groups = FieldGroups::new();
                let id = groups.intern(query);
                let mut packed = NumericMembership::with_mode(
                    query,
                    budget,
                    &groups,
                    Limits::default(),
                    DigitMode::Packed,
                    false,
                );
                assert_eq!(packed.matches(query, id).unwrap(), Some(true));
                let bytes = if query.len() > 15 {
                    0
                } else if budget == 0 {
                    160
                } else {
                    5_120
                };
                assert_eq!(packed.statistics.digit_table_bytes, bytes);
            }
        }
    }

    #[test]
    fn length_rejection_is_cached_before_compilation_and_validates_original_length() {
        let query = format!("{}1", "a".repeat(62));
        let fields: Vec<_> = [60, 61, 62]
            .into_iter()
            .map(|letters| format!("{}1", "a".repeat(letters)))
            .collect();
        for budget in [0, 1] {
            compare(&query, &fields, budget, true);
            let mut groups = FieldGroups::new();
            let id = groups.intern(&fields[0]).unwrap();
            let mut numeric = NumericMembership::new(&query, budget, &groups, Limits::default());
            assert_eq!(numeric.matches(&fields[0], Some(id)).unwrap(), Some(false));
            let statistics = numeric.statistics;
            assert_eq!(statistics.length_rejected_groups, 1);
            assert_eq!(statistics.compiled_groups, 0);
            assert_eq!(statistics.static_steps, 0);
            assert_eq!(statistics.payload_bytes, 0);
            assert_eq!(numeric.matches(&fields[0], Some(id)).unwrap(), Some(false));
            assert_eq!(numeric.statistics, statistics);
            assert!(numeric.matches(&fields[1], Some(id)).is_err());
            assert!(
                numeric
                    .matches(&fields[0], GroupId::new(id.get() + 1))
                    .is_err()
            );
        }

        // UTF-8 bytes give only a conservative bound; scalar count must not tighten this guard.
        let unicode = format!("{}1", "é".repeat(31));
        let mut groups = FieldGroups::new();
        let id = groups.intern(&unicode);
        let mut numeric = NumericMembership::new(&query, 1, &groups, Limits::default());
        assert_eq!(numeric.matches(&unicode, id).unwrap(), Some(false));
        assert_eq!(numeric.statistics.length_rejected_groups, 0);
        assert_eq!(numeric.statistics.compiled_groups, 1);
    }

    #[test]
    fn numeric_only_queries_skip_fixed_spans_with_public_matcher_parity() {
        let mut fields: Vec<_> = [
            "",
            "1",
            "2",
            "11",
            "12",
            "before1middle2after",
            "é1\0東京2🧭",
            "before1middle1after",
            "before2middle1after",
            "letters-only",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        fields.extend([
            "1".repeat(61),
            "1".repeat(62),
            "1".repeat(63),
            "é1\0".repeat(63),
        ]);
        for query in [
            "1".to_owned(),
            "12".to_owned(),
            "11".to_owned(),
            "1".repeat(63),
        ] {
            for budget in [0, 1] {
                compare(&query, &fields, budget, true);
                let mut groups = FieldGroups::new();
                let ids: Vec<_> = fields.iter().map(|field| groups.intern(field)).collect();
                let mut numeric =
                    NumericMembership::new(&query, budget, &groups, Limits::default());
                for (field, id) in fields.iter().zip(ids) {
                    assert_eq!(
                        numeric.matches(field, id).unwrap(),
                        Membership::new(&query).matches(field, budget)
                    );
                }
                assert!(numeric.statistics.compiled_groups > 0);
                assert_eq!(numeric.statistics.static_steps, 0);
                assert_eq!(numeric.statistics.table_bytes, 0);
            }
        }
    }

    #[test]
    fn original_digits_change_results_within_the_same_group() {
        let mut groups = FieldGroups::new();
        let first = groups.intern("before-12-after");
        let second = groups.intern("before-34-after");
        assert_eq!(first, second);
        let mut numeric = NumericMembership::new("12", 0, &groups, Limits::default());
        assert_eq!(
            numeric.matches("before-12-after", first).unwrap(),
            Some(true)
        );
        assert_eq!(
            numeric.matches("before-34-after", second).unwrap(),
            Some(false)
        );
        assert_eq!(numeric.statistics.compiled_groups, 1);
    }

    #[test]
    fn unsupported_queries_allocate_no_cache_and_preserve_fallback() {
        let mut groups = FieldGroups::new();
        let id = groups.intern("a1b");
        for (query, budget) in [("", 0), ("abc", 1), ("é1", 1), ("a1", 2)] {
            let mut numeric = NumericMembership::new(query, budget, &groups, Limits::default());
            assert!(numeric.cache.is_empty());
            assert_eq!(numeric.matches("a1b", id).unwrap(), None);
            assert_eq!(numeric.statistics, Statistics::default());
        }
    }

    #[test]
    fn compile_limits_fallback_before_work_and_do_not_retry() {
        let mut groups = FieldGroups::new();
        let id = groups.intern("prefix1middle2suffix");
        for limits in [
            Limits {
                static_steps: 0,
                payload_bytes: usize::MAX,
            },
            Limits {
                static_steps: usize::MAX,
                payload_bytes: 0,
            },
        ] {
            let mut numeric = NumericMembership::new("p12s", 1, &groups, limits);
            assert_eq!(numeric.matches("prefix1middle2suffix", id).unwrap(), None);
            let statistics = numeric.statistics;
            assert_eq!(statistics.compiled_groups, 0);
            assert_eq!(statistics.static_steps, 0);
            assert_eq!(statistics.payload_bytes, 0);
            assert_eq!(statistics.fallback_groups, 1);
            assert_eq!(numeric.matches("prefix1middle2suffix", id).unwrap(), None);
            assert_eq!(numeric.statistics, statistics);
        }
        let mut fresh = NumericMembership::new("p12s", 1, &groups, Limits::default());
        assert_eq!(fresh.statistics.compiled_groups, 0);
        assert_eq!(
            fresh.matches("prefix1middle2suffix", id).unwrap(),
            Some(true)
        );
        assert_eq!(fresh.statistics.compiled_groups, 1);
    }

    #[test]
    fn missing_groups_and_inconsistent_lengths_are_errors() {
        let mut groups = FieldGroups::new();
        let id = groups.intern("a1b").unwrap();
        let mut numeric = NumericMembership::new("a1", 0, &groups, Limits::default());
        assert!(numeric.matches("a1", Some(id)).is_err());
        assert!(numeric.matches("a1b", GroupId::new(id.get() + 1)).is_err());
        assert_eq!(numeric.matches("a1b", None).unwrap(), None);
        assert!(groups.release(id));
        let mut numeric = NumericMembership::new("a1", 0, &groups, Limits::default());
        assert!(numeric.matches("a1b", Some(id)).is_err());
    }

    #[test]
    fn maximum_group_length_compiles_and_longer_fields_fall_back() {
        let mut groups = FieldGroups::new();
        let field = format!("{}1", "a".repeat(1_023));
        let id = groups.intern(&field);
        assert!(id.is_some());
        let long = format!("{field}2");
        assert!(groups.intern(&long).is_none());
        let mut numeric = NumericMembership::new("a1", 1, &groups, Limits::default());
        assert_eq!(numeric.matches(&field, id).unwrap(), Some(true));
        assert_eq!(numeric.matches(&long, None).unwrap(), None);
    }
}

#[cfg(test)]
mod diagnostic {
    use std::{
        hint::black_box,
        time::{Duration, Instant},
    };

    use super::*;
    use crate::membership::{
        grouped::{self, Corpus},
        indexed::diagnostic::{self, Query, existing_membership},
    };
    use crate::{TextField, field_groups::query_safe};

    fn field_matches(
        corpus: &Corpus,
        query: &Query,
        row: usize,
        field: TextField,
        numeric: &mut NumericMembership<'_>,
    ) -> bool {
        let masks = &corpus.masks[row];
        if !masks.may_match(field, &query.mask, query.budget) {
            return false;
        }
        let (text, index) = match field {
            TextField::Title => (corpus.entries[row].title.as_str(), 0),
            TextField::Address => (corpus.entries[row].address.as_str(), 1),
        };
        numeric
            .matches(text, corpus.fields[row][index])
            .expect("retained synthetic group")
            .unwrap_or_else(|| existing_membership(text, masks.is_ascii(field), query))
    }

    fn scan<'a>(
        corpus: &'a Corpus,
        query: &Query,
        text: &str,
        mode: DigitMode,
    ) -> (Vec<bool>, NumericMembership<'a>) {
        let mut numeric = NumericMembership::with_mode(
            text,
            query.budget,
            &corpus.groups,
            Limits::default(),
            mode,
            false,
        );
        let rows = scan_rows(corpus, query, &mut numeric);
        (rows, numeric)
    }

    fn scan_rows(corpus: &Corpus, query: &Query, numeric: &mut NumericMembership<'_>) -> Vec<bool> {
        (0..corpus.entries.len())
            .map(|row| {
                field_matches(corpus, query, row, TextField::Title, numeric)
                    || field_matches(corpus, query, row, TextField::Address, numeric)
            })
            .collect()
    }

    fn sample(
        corpus: &Corpus,
        text: &str,
        expected: &[bool],
        mode: Option<DigitMode>,
    ) -> (Duration, Statistics) {
        let started = Instant::now();
        let query = Query::new(black_box(text));
        if mode.is_none() || query_safe(text) {
            let (rows, memo) =
                grouped::scan::<true>(black_box(corpus), black_box(&query), query_safe(text));
            let elapsed = started.elapsed();
            assert_eq!(rows, expected);
            drop(black_box(memo));
            return (elapsed, Statistics::default());
        }
        let (rows, numeric) = scan(
            black_box(corpus),
            black_box(&query),
            black_box(text),
            mode.expect("numeric mode"),
        );
        let elapsed = started.elapsed();
        assert_eq!(rows, expected);
        let statistics = numeric.statistics;
        drop(black_box(numeric));
        (elapsed, statistics)
    }

    fn compare(corpus: &Corpus, label: &str, text: &str) {
        let query = Query::new(text);
        let (expected, _) = grouped::scan::<false>(corpus, &query, query_safe(text));
        let modes = [None, Some(DigitMode::Scalar), Some(DigitMode::Packed)];
        for _ in 0..2 {
            for mode in modes {
                sample(corpus, text, &expected, mode);
            }
        }
        let mut timings = [Vec::new(), Vec::new(), Vec::new()];
        let mut statistics = [Statistics::default(); 3];
        for pass in 0..5 {
            let order = if pass % 2 == 0 { [0, 1, 2] } else { [2, 1, 0] };
            for index in order {
                let (elapsed, measured) = sample(corpus, text, &expected, modes[index]);
                timings[index].push(elapsed);
                statistics[index] = measured;
            }
        }
        for samples in &mut timings {
            samples.sort_unstable();
        }
        for measured in statistics {
            assert!(measured.static_steps <= Limits::default().static_steps);
            assert!(measured.payload_bytes <= Limits::default().payload_bytes);
            assert!(measured.digit_table_bytes <= 5_120);
            assert!(measured.digit_table_steps <= 2_560);
        }
        println!(
            "{label} matches={} grouped_median_ms={:.3} scalar_span_median_ms={:.3} packed_span_median_ms={:.3} grouped_worst_ms={:.3} scalar_span_worst_ms={:.3} packed_span_worst_ms={:.3} packed_statistics={:?}",
            expected.iter().filter(|matched| **matched).count(),
            timings[0][2].as_secs_f64() * 1_000.,
            timings[1][2].as_secs_f64() * 1_000.,
            timings[2][2].as_secs_f64() * 1_000.,
            timings[0][4].as_secs_f64() * 1_000.,
            timings[1][4].as_secs_f64() * 1_000.,
            timings[2][4].as_secs_f64() * 1_000.,
            statistics[2],
        );
        for mode in [DigitMode::Scalar, DigitMode::Packed] {
            attribute(corpus, label, text, &expected, mode);
        }
    }

    fn attribute(corpus: &Corpus, label: &str, text: &str, expected: &[bool], mode: DigitMode) {
        if query_safe(text) {
            return;
        }
        let mut samples = Vec::new();
        for _ in 0..3 {
            let started = Instant::now();
            let query = Query::new(black_box(text));
            let mut numeric = NumericMembership::with_mode(
                black_box(text),
                query.budget,
                &corpus.groups,
                Limits::default(),
                mode,
                true,
            );
            let preparation = started.elapsed();
            numeric.compilation_time = Some(Duration::ZERO);
            let rows = scan_rows(black_box(corpus), black_box(&query), &mut numeric);
            let total = started.elapsed();
            let compilation = numeric.compilation_time.expect("instrumented scan");
            let row_evaluation = total
                .checked_sub(preparation + compilation)
                .expect("stage timers lie within total");
            assert_eq!(rows, expected);
            let digit_preparation = numeric.digit_preparation_time;
            drop(black_box(numeric));
            samples.push((
                total,
                preparation,
                compilation,
                row_evaluation,
                digit_preparation,
            ));
        }
        samples.sort_unstable_by_key(|sample| sample.0);
        let (total, preparation, compilation, row_evaluation, digit_preparation) = samples[1];
        println!(
            "{label} {mode:?} attribution preparation_ms={:.3} digit_table_preparation_subset_ms={:.3} compilation_ms={:.3} row_evaluation_residual_ms={:.3} instrumented_total_ms={:.3}",
            preparation.as_secs_f64() * 1_000.,
            digit_preparation.as_secs_f64() * 1_000.,
            compilation.as_secs_f64() * 1_000.,
            row_evaluation.as_secs_f64() * 1_000.,
            total.as_secs_f64() * 1_000.
        );
    }

    #[test]
    fn numeric_workload_diagnostic_preserves_every_row_membership() {
        println!(
            "Synthetic membership-only diagnostic, not D48 acceptance. Compare existing grouped, revised scalar spans and packed digit spans; packed uses scalar above 15 query bytes. All query preparation (including the shared diagnostic query helper), digit table construction, lazy span compilation and cache allocation are timed with no reuse between scans; ranking, counts and highlights are excluded. Two warmups and five paired samples in alternating grouped/scalar/packed and packed/scalar/grouped order, row equality outside timing. Numeric cases additionally report the median-total of three separately instrumented scans per span mode: preparation includes all query/cache allocation, digit table time is a preparation subset, compilation includes layout and rejection, row evaluation is total minus preparation and compilation and includes timer overhead. Preparation and per-group timers are disabled in paired samples. Payload bytes count retained span/table arrays, table bytes are their fixed-span table subset, digit table bytes/steps are separate per-query costs, and cache slot bytes count enum storage; allocator overhead, transient layout allocation and query pairs are excluded from byte accounting. Static steps count compiled fixed byte-state steps, excluding layout parsing. Limits: 1,000,000 static steps, 512 KiB span payload, at most 5,120 digit table bytes/2,560 transitions and the bounded pool's 2,048 cache slots."
        );
        let corpus = grouped::corpus();
        assert_eq!(corpus.entries.len(), diagnostic::ROWS);
        for (label, text) in diagnostic::cases() {
            compare(&corpus, label, &text);
        }
        for text in [
            "0",
            "12",
            "1234",
            "099983",
            "catalogue-126",
            "ROW-099983",
            "r0w",
            "111111",
            "12ab34cd56",
        ] {
            compare(&corpus, text, text);
        }
        compare(&corpus, "numeric-length63", &format!("{}1", "a".repeat(62)));
    }
}
