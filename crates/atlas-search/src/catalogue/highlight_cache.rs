//! Reuses finalized trace indices while validating highlights against each original field.

use frizbee::{MatchIndices, Matcher};

use super::{Entry, FieldRanges, SearchError, highlights, matcher_config};
use crate::field_groups::{GroupId, query_safe};

const MAX_FIELD_BYTES: usize = 1_024;
const MAX_INDEX_BYTES: usize = 256 * 1_024;

// None rejects a field; Some(empty) accepts it without any recoverable positions.
type RawTrace = Option<Box<[u32]>>;

enum TraceSource {
    Cached(usize),
    Computed(RawTrace),
}

pub(super) struct HighlightCache<'query> {
    query: &'query str,
    fallback: Option<Matcher>,
    traces: Option<Vec<Option<RawTrace>>>,
    index_bytes: usize,
    computed: usize,
    reused: usize,
}

impl<'query> HighlightCache<'query> {
    /// The caller retains each field's live ID and one matcher configuration throughout the query.
    pub(super) fn new(query: &'query str, slots: usize) -> Self {
        Self {
            query,
            fallback: None,
            traces: (!query.is_empty() && query_safe(query)).then(|| vec![None; slots]),
            index_bytes: 0,
            computed: 0,
            reused: 0,
        }
    }

    pub(super) fn match_ranges(
        &mut self,
        matcher: &mut Matcher,
        entry: &Entry,
        groups: [Option<GroupId>; 2],
    ) -> Result<FieldRanges, SearchError> {
        let title = self.trace(matcher, entry.title.as_str(), groups[0])?;
        let address = self.trace(matcher, entry.address.as_str(), groups[1])?;
        let title = self.resolve(&title)?;
        let address = self.resolve(&address)?;
        if title.is_none() && address.is_none() {
            return Err(SearchError::InconsistentMatch);
        }

        // Cached indices still pass through the original text's UTF-8 and range checks.
        Ok((
            highlights::ranges(entry.title.as_str(), title.unwrap_or(&[]))?,
            highlights::ranges(entry.address.as_str(), address.unwrap_or(&[]))?,
        ))
    }

    /// Counts actual matcher calls, including empty-trace retries, and reused finalized traces.
    pub(super) fn stats(&self) -> (usize, usize) {
        (self.computed, self.reused)
    }

    fn trace(
        &mut self,
        matcher: &mut Matcher,
        original: &str,
        id: Option<GroupId>,
    ) -> Result<TraceSource, SearchError> {
        if original.len() > MAX_FIELD_BYTES {
            return Ok(TraceSource::Computed(self.compute(matcher, original)));
        }
        let Some((traces, id)) = self.traces.as_ref().zip(id) else {
            return Ok(TraceSource::Computed(self.compute(matcher, original)));
        };
        let slot = id.get() as usize - 1;
        if traces
            .get(slot)
            .ok_or(SearchError::InconsistentMatch)?
            .is_some()
        {
            self.reused += 1;
            return Ok(TraceSource::Cached(slot));
        }

        // Score and trace an original representative, never the pool's canonical text.
        let trace = self.compute(matcher, original);
        let bytes = trace
            .as_ref()
            .map_or(0, |indices| indices.len().saturating_mul(size_of::<u32>()));
        if bytes > MAX_INDEX_BYTES - self.index_bytes {
            return Ok(TraceSource::Computed(trace));
        }
        let cached = self
            .traces
            .as_mut()
            .and_then(|traces| traces.get_mut(slot))
            .ok_or(SearchError::InconsistentMatch)?;
        *cached = Some(trace);
        self.index_bytes += bytes;
        Ok(TraceSource::Cached(slot))
    }

    fn compute(&mut self, matcher: &mut Matcher, original: &str) -> RawTrace {
        self.computed += 1;
        let found = matcher.match_one_indices(original, 0);
        if found.as_ref().is_some_and(|found| found.indices.is_empty()) {
            self.computed += 1;
        }
        finalized_indices(found, original, self.query, &mut self.fallback)
            .map(Vec::into_boxed_slice)
    }

    fn resolve<'trace>(
        &'trace self,
        source: &'trace TraceSource,
    ) -> Result<Option<&'trace [u32]>, SearchError> {
        match source {
            TraceSource::Computed(trace) => Ok(trace.as_deref()),
            TraceSource::Cached(slot) => self
                .traces
                .as_ref()
                .and_then(|traces| traces.get(*slot))
                .and_then(Option::as_ref)
                .map(Option::as_deref)
                .ok_or(SearchError::InconsistentMatch),
        }
    }
}

pub(super) fn finalized_indices(
    found: Option<MatchIndices>,
    original: &str,
    query: &str,
    fallback: &mut Option<Matcher>,
) -> Option<Vec<u32>> {
    let found = found?;
    if !found.indices.is_empty() {
        return Some(found.indices);
    }

    // A valid candidate can exhaust its typo budget before tracing a real match.
    let highlighter =
        fallback.get_or_insert_with(|| Matcher::new(query, &matcher_config(0).max_typos(None)));
    Some(match highlighter.match_one_indices(original, 0) {
        Some(trace) => trace.indices,
        None => Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use super::*;
    use crate::{Target, Usage, field_groups::FieldGroups};

    fn entry(title: &str, address: &str) -> Entry {
        Entry {
            target: Target::History(NonZeroU64::new(1).expect("synthetic ID")),
            title: title.to_owned(),
            address: address.to_owned(),
            usage: Usage {
                visits: 0,
                last_used: None,
            },
            open: false,
        }
    }

    // Keep the original retry procedure independent from the shared production finalizer.
    fn original_trace(
        matcher: &mut Matcher,
        fallback: &mut Option<Matcher>,
        query: &str,
        original: &str,
    ) -> Option<Vec<u32>> {
        let found = matcher.match_one_indices(original, 0)?;
        if !found.indices.is_empty() {
            return Some(found.indices);
        }
        let highlighter =
            fallback.get_or_insert_with(|| Matcher::new(query, &matcher_config(0).max_typos(None)));
        match highlighter.match_one_indices(original, 0) {
            Some(trace) => Some(trace.indices),
            None => Some(Vec::new()),
        }
    }

    fn compare_traces(query: &str, max_typos: u16, fields: &[String]) {
        let mut groups = FieldGroups::new();
        let ids: Vec<_> = fields.iter().map(|field| groups.intern(field)).collect();
        for reverse in [false, true] {
            let mut cache = HighlightCache::new(query, groups.slots());
            let mut matcher = Matcher::new(query, &matcher_config(max_typos));
            let mut reference = Matcher::new(query, &matcher_config(max_typos));
            let mut fallback = None;
            for offset in 0..fields.len() * 2 {
                let index = offset % fields.len();
                let index = if reverse {
                    fields.len() - index - 1
                } else {
                    index
                };
                let field = fields[index].as_str();
                let source = cache
                    .trace(&mut matcher, field, ids[index])
                    .expect("valid ID");
                let actual = cache.resolve(&source).expect("prepared trace");
                let expected = original_trace(&mut reference, &mut fallback, query, field);
                assert_eq!(
                    actual,
                    expected.as_deref(),
                    "synthetic query {query:?}, field {field:?}, budget {max_typos}"
                );
                assert_eq!(
                    highlights::ranges(field, actual.unwrap_or(&[])),
                    highlights::ranges(field, expected.as_deref().unwrap_or(&[]))
                );
            }
            assert!(cache.index_bytes <= MAX_INDEX_BYTES);
        }
    }

    #[test]
    fn grouped_raw_indices_ranges_and_errors_match_original_case_and_tie_paths() {
        let alphabet = ["0", "9", "a", "A", "_", "\0", "é"];
        let mut fields = vec![String::new()];
        for first in alphabet {
            fields.push(first.to_owned());
            fields.extend(alphabet.map(|last| format!("{first}{last}")));
        }
        fields.extend(
            [
                "a0a9z", "a9a0z", "é0é9z", "é9é0z", "a_0_z", "a_9_z", "İ0z", "İ9z", "K0z", "K9z",
                "ſ0z", "ſ9z", "ß0z", "ß9z", "ẞ0z", "ẞ9z", "𐐀0z", "𐐀9z", "𐐨0z", "𐐨9z", "😀0z",
                "😀9z",
            ]
            .map(str::to_owned),
        );
        for query in [
            "", "az", "Az", "aa", "é", "É", "İ", "i", "K", "k", "ſ", "s", "ß", "ẞ", "𐐀", "𐐨", "😀",
            "\0", "a\0z", "a0z",
        ] {
            compare_traces(query, 0, &fields);
            compare_traces(query, 1, &fields);
        }
    }

    #[test]
    fn original_length_boundaries_and_overlength_fallback_preserve_traces() {
        let fields: Vec<_> = [15, 16, 17, 31, 32, 33, 63, 64, 65, 1_023, 1_024, 1_025]
            .into_iter()
            .flat_map(|length| {
                ['0', '9'].map(move |digit| format!("é{}z", digit.to_string().repeat(length - 3)))
            })
            .collect();
        for query in ["éz", "Éz", "\0", "é\0z", "é0z"] {
            compare_traces(query, 0, &fields);
            compare_traces(query, 1, &fields);
        }
        for length in [63, 64, 65, 95, 96] {
            let query = "é".repeat(length);
            compare_traces(
                query.as_str(),
                1,
                &[format!("0{query}z"), format!("9{query}z")],
            );
        }
    }

    #[test]
    fn accepted_empty_rejected_and_recovered_traces_remain_distinct() {
        let fragment = "reproducible synthetic catalogue exploration includes detailed release notes and project reference material for keyboard navigation";
        let query = format!("{}xyz", &fragment[..93]);
        let fields = ["000162", "999837"]
            .map(|digits| format!("cobalt example collection row-{digits} {fragment} {fragment}"));
        let mut primary = Matcher::new(query.as_str(), &matcher_config(1));
        assert!(
            primary
                .match_one_indices(fields[0].as_str(), 0)
                .expect("accepted original")
                .indices
                .is_empty()
        );
        compare_traces(query.as_str(), 1, &fields);
        let mut groups = FieldGroups::new();
        let id = groups.intern(fields[0].as_str());
        assert_eq!(groups.intern(fields[1].as_str()), id);
        let mut cache = HighlightCache::new(query.as_str(), groups.slots());
        let first = cache
            .trace(&mut primary, fields[0].as_str(), id)
            .expect("valid group");
        assert!(
            !cache
                .resolve(&first)
                .expect("trace")
                .expect("accepted")
                .is_empty()
        );
        assert!(matches!(
            cache.trace(&mut primary, fields[1].as_str(), id),
            Ok(TraceSource::Cached(_))
        ));
        assert_eq!(cache.stats(), (2, 1));

        let mut groups = FieldGroups::new();
        let id = groups.intern("123");
        assert_eq!(groups.intern("987"), id);
        for (query, expected) in [("a", Some(&[][..])), ("ab", None)] {
            let mut cache = HighlightCache::new(query, groups.slots());
            let mut matcher = Matcher::new(query, &matcher_config(1));
            for field in ["123", "987"] {
                let source = cache.trace(&mut matcher, field, id).expect("valid group");
                assert_eq!(cache.resolve(&source).expect("trace"), expected);
            }
            assert_eq!(cache.stats(), (if expected.is_some() { 2 } else { 1 }, 1));
        }
        compare_traces("abc", 1, &[format!("a{}c", "_".repeat(1_100))]);
    }

    #[test]
    fn entry_ranges_preserve_rejection_order_original_text_checks_and_fresh_slots() {
        let mut groups = FieldGroups::new();
        let title = groups.intern("a0z");
        let address = groups.intern("q0q");
        let ids = [title, address];
        for query in ["az", "qq", "xx", "\0", "a0z"] {
            let mut cache = HighlightCache::new(query, groups.slots());
            let mut matcher = Matcher::new(query, &matcher_config(1));
            let mut reference = Matcher::new(query, &matcher_config(1));
            let mut fallback = None;
            for row in [entry("a0z", "q0q"), entry("a9z", "q9q")] {
                assert_eq!(
                    cache.match_ranges(&mut matcher, &row, ids),
                    super::super::match_ranges(&mut reference, &mut fallback, query, &row)
                );
            }
        }

        // A deleted NUL has no real position, including when its empty trace is reused.
        let mut cache = HighlightCache::new("\0", groups.slots());
        let mut matcher = Matcher::new("\0", &matcher_config(1));
        for row in [entry("a0z", "q0q"), entry("a9z", "q9q")] {
            assert_eq!(
                cache.match_ranges(&mut matcher, &row, ids),
                Ok((Vec::new(), Vec::new()))
            );
        }
        assert_eq!(cache.stats(), (4, 2));

        // Inject malformed cached traces so range validation does not depend on a matcher defect.
        for invalid_field in 0..2 {
            let mut cache = HighlightCache::new("az", groups.slots());
            let traces = cache.traces.as_mut().expect("digit-free cache");
            for (field, id) in ids.into_iter().enumerate() {
                let slot = id.expect("field ID").get() as usize - 1;
                let indices = if field == invalid_field {
                    vec![3]
                } else {
                    Vec::new()
                };
                traces[slot] = Some(Some(indices.into_boxed_slice()));
            }
            let mut matcher = Matcher::new("az", &matcher_config(1));
            for row in [entry("a0z", "q0q"), entry("a9z", "q9q")] {
                assert_eq!(
                    cache.match_ranges(&mut matcher, &row, ids),
                    Err(SearchError::InvalidMatchRange)
                );
            }
            assert_eq!(cache.stats(), (0, 4));
        }

        assert!(groups.release(title.expect("title ID")));
        let reused = groups.intern("x9x");
        assert_eq!(reused, title);
        let mut cache = HighlightCache::new("az", groups.slots());
        let mut matcher = Matcher::new("az", &matcher_config(0));
        assert_eq!(
            cache.match_ranges(&mut matcher, &entry("x9x", "q0q"), [reused, address]),
            Err(SearchError::InconsistentMatch)
        );
        assert_eq!(cache.stats(), (2, 0));
    }

    #[test]
    fn index_payload_cap_keeps_original_scoring_and_invalid_ids_return_errors() {
        let query = "a".repeat(64);
        let mut groups = FieldGroups::new();
        let fields: Vec<_> = (0..1_025)
            .map(|index| {
                let suffix: String = [index / 676, index / 26 % 26, index % 26]
                    .map(|letter| char::from(b'b' + letter as u8))
                    .into_iter()
                    .collect();
                format!("{query}_{suffix}")
            })
            .collect();
        let ids: Vec<_> = fields.iter().map(|field| groups.intern(field)).collect();
        let mut cache = HighlightCache::new(query.as_str(), groups.slots());
        let mut matcher = Matcher::new(query.as_str(), &matcher_config(0));
        let mut reference = Matcher::new(query.as_str(), &matcher_config(0));
        for _ in 0..2 {
            for (field, &id) in fields.iter().zip(&ids) {
                let source = cache
                    .trace(&mut matcher, field.as_str(), id)
                    .expect("valid group");
                let expected = reference
                    .match_one_indices(field.as_str(), 0)
                    .expect("matching prefix");
                assert_eq!(
                    cache.resolve(&source).expect("trace"),
                    Some(expected.indices.as_slice())
                );
            }
        }
        assert_eq!(cache.index_bytes, MAX_INDEX_BYTES);
        assert_eq!(cache.stats(), (1_026, 1_024));
        assert!(matches!(
            cache.trace(&mut matcher, "a", GroupId::new(2_049)),
            Err(SearchError::InconsistentMatch)
        ));

        let mut disabled = HighlightCache::new("a0", groups.slots());
        let mut matcher = Matcher::new("a0", &matcher_config(0));
        let first = disabled
            .trace(&mut matcher, "a0", ids[0])
            .expect("numeric fallback");
        let second = disabled
            .trace(&mut matcher, "a9", ids[0])
            .expect("numeric fallback");
        assert!(disabled.resolve(&first).expect("trace").is_some());
        assert!(disabled.resolve(&second).expect("trace").is_none());
        assert_eq!(disabled.stats(), (2, 0));
    }

    #[test]
    fn complete_queries_preserve_original_rows_through_mutations_and_group_reuse() {
        use super::super::Catalogue;
        use crate::{BlankQuery, BoostWeights, Scope, SearchPolicy};

        let mut catalogue = Catalogue::new();
        for index in 1..=8 {
            let mut row = entry(&format!("é{index}a{index}z"), &format!("q{index}q"));
            row.target = Target::History(NonZeroU64::new(index).expect("synthetic ID"));
            row.open = index % 2 == 0;
            row.usage.visits = index as u32;
            catalogue.upsert(row).expect("insert synthetic row");
        }
        let policy = SearchPolicy {
            limit: 5,
            max_typos: 1,
            blank: BlankQuery::All,
            boosts: BoostWeights {
                open: 20,
                frequency: 32,
                recency: 10,
                recency_window_secs: NonZeroU64::new(100).expect("synthetic window"),
            },
        };
        for generation in 0..3 {
            for query in ["éaz", "Éaz", "az", "qq", "xx", "é1az", "\0", ""] {
                let actual = catalogue.query(query, Scope::Everything, 100, policy);
                let expected = catalogue.query_exhaustive(query, Scope::Everything, 100, policy);
                assert_eq!(
                    actual, expected,
                    "synthetic query {query:?}, mutation {generation}"
                );
            }
            // Removing all leases makes the next insertion reuse group slots with different text.
            for index in 1..=8 {
                assert!(
                    catalogue
                        .remove(Target::History(
                            NonZeroU64::new(index).expect("synthetic ID")
                        ))
                        .expect("remove")
                );
            }
            for index in 1..=8 {
                let mut row = entry(&format!("q{index}q"), &format!("é{index}a{index}z"));
                row.target = Target::History(NonZeroU64::new(index).expect("synthetic ID"));
                row.usage.last_used = Some(100 - generation);
                catalogue.upsert(row).expect("reinsert synthetic row");
            }
        }
    }
}
