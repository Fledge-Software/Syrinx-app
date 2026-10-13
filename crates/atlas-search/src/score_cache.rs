//! Reuses equivalent field scores within one query while retaining each original field for display.

use std::collections::HashMap;

use frizbee::Matcher;

use crate::{
    SearchError,
    field_groups::{GroupId, query_safe},
};

const MAX_FIELD_BYTES: usize = 1_024;
const MAX_ENTRIES: usize = 512;
// Key payload is bounded separately from the map's at-most-512 entry overhead.
const MAX_KEY_BYTES: usize = 256 * 1_024;

pub(crate) struct QueryScoreCache {
    digits: Option<[u8; 10]>,
    scores: HashMap<Vec<u8>, Option<u16>>,
    group_scores: Option<Vec<Option<Option<u16>>>>,
    key_bytes: usize,
    computed: usize,
    reused: usize,
}

impl QueryScoreCache {
    /// The caller keeps the matcher's query and configuration unchanged for this cache's lifetime.
    pub(crate) fn new(query: &str) -> Self {
        Self {
            digits: digit_classes(query),
            scores: HashMap::new(),
            group_scores: None,
            key_bytes: 0,
            computed: 0,
            reused: 0,
        }
    }

    /// Group IDs and the matcher's configuration stay unchanged until this query finishes.
    pub(crate) fn with_groups(query: &str, slots: usize) -> Self {
        let mut cache = Self::new(query);
        if !query.is_empty() && query_safe(query) {
            cache.group_scores = Some(vec![None; slots]);
        }
        cache
    }

    /// The caller supplies this original field's live group ID, or none for an ungrouped field.
    pub(crate) fn score_grouped(
        &mut self,
        matcher: &mut Matcher,
        original: &str,
        id: Option<GroupId>,
    ) -> Result<Option<u16>, SearchError> {
        let Some((scores, id)) = self.group_scores.as_ref().zip(id) else {
            return Ok(self.score(matcher, original));
        };
        let slot = id.get() as usize - 1;
        let cached = scores.get(slot).ok_or(SearchError::InconsistentMatch)?;
        if original.len() > MAX_FIELD_BYTES {
            return Err(SearchError::InconsistentMatch);
        }
        if let Some(score) = cached {
            self.reused += 1;
            return Ok(*score);
        }

        // Digit-free queries cannot distinguish a group's digits through Unicode case mapping.
        let score = self.compute(matcher, original);
        let cached = self
            .group_scores
            .as_mut()
            .and_then(|scores| scores.get_mut(slot))
            .ok_or(SearchError::InconsistentMatch)?;
        *cached = Some(score);
        Ok(score)
    }

    pub(crate) fn score(&mut self, matcher: &mut Matcher, original: &str) -> Option<u16> {
        let Some(digits) = self.digits else {
            return self.compute(matcher, original);
        };
        if original.len() > MAX_FIELD_BYTES || !original.is_ascii() {
            return self.compute(matcher, original);
        }

        // Preserve length and all matching bytes; only impossible-to-match digits share a class.
        let mut buffer = [0; MAX_FIELD_BYTES];
        let signature = &mut buffer[..original.len()];
        for (destination, byte) in signature.iter_mut().zip(original.bytes()) {
            *destination = if byte.is_ascii_digit() {
                digits[usize::from(byte - b'0')]
            } else {
                byte
            };
        }
        if let Some(score) = self.scores.get(&*signature) {
            self.reused += 1;
            return *score;
        }

        // Score the original representative, including rejection and accepted zero scores.
        let score = self.compute(matcher, original);
        if self.scores.len() < MAX_ENTRIES && signature.len() <= MAX_KEY_BYTES - self.key_bytes {
            self.key_bytes += signature.len();
            self.scores.insert(signature.to_vec(), score);
        }
        score
    }

    /// Returns actual matcher calls and equivalent-field reuses for this query only.
    pub(crate) fn stats(&self) -> (usize, usize) {
        (self.computed, self.reused)
    }

    #[cfg(test)]
    pub(crate) fn group_is_scored(&self, id: GroupId) -> Result<bool, SearchError> {
        self.group_scores
            .as_ref()
            .and_then(|scores| scores.get((id.get() - 1) as usize))
            .map(Option::is_some)
            .ok_or(SearchError::InconsistentMatch)
    }

    fn compute(&mut self, matcher: &mut Matcher, original: &str) -> Option<u16> {
        self.computed += 1;
        matcher.match_one(original, 0).map(|found| found.score)
    }
}

fn digit_classes(query: &str) -> Option<[u8; 10]> {
    if query.is_empty() || !query.is_ascii() {
        return None;
    }
    let representative = (b'0'..=b'9').find(|digit| !query.as_bytes().contains(digit));
    Some(std::array::from_fn(|index| {
        let digit = b'0' + index as u8;
        if query.as_bytes().contains(&digit) {
            digit
        } else {
            representative.unwrap_or(digit)
        }
    }))
}

#[cfg(test)]
mod tests {
    use frizbee::{Config, SortStrategy};

    use super::*;
    use crate::field_groups::FieldGroups;

    fn matcher(query: &str, max_typos: u16) -> Matcher {
        Matcher::new(
            query,
            &Config::default()
                .max_typos(Some(max_typos))
                .sort(SortStrategy::IndexAsc),
        )
    }

    fn compare(query: &str, max_typos: u16, fields: &[String]) {
        let mut cache = QueryScoreCache::new(query);
        let mut cached_matcher = matcher(query, max_typos);
        let mut reference = matcher(query, max_typos);
        // Reversing the second pass changes which original representatives are encountered next.
        for field in fields.iter().chain(fields.iter().rev()) {
            let expected = reference
                .match_one(field.as_str(), 0)
                .map(|found| found.score);
            assert_eq!(
                cache.score(&mut cached_matcher, field),
                expected,
                "synthetic query {query:?}, field {field:?}, typos {max_typos}"
            );
        }
        let (computed, reused) = cache.stats();
        assert_eq!(computed + reused, fields.len() * 2);
        assert!(cache.scores.len() <= MAX_ENTRIES);
        assert!(cache.key_bytes <= MAX_KEY_BYTES);
    }

    fn words(alphabet: &[char], max_len: usize) -> Vec<String> {
        let mut words = vec![String::new()];
        let mut previous = vec![String::new()];
        for _ in 0..max_len {
            let next: Vec<_> = previous
                .iter()
                .flat_map(|prefix| {
                    alphabet
                        .iter()
                        .map(move |letter| format!("{prefix}{letter}"))
                })
                .collect();
            words.extend(next.iter().cloned());
            previous = next;
        }
        words
    }

    fn compare_grouped(query: &str, max_typos: u16, fields: &[String]) {
        let mut groups = FieldGroups::new();
        let ids: Vec<_> = fields.iter().map(|field| groups.intern(field)).collect();
        // Either member of an equivalent pair must be a valid original representative.
        for reverse in [false, true] {
            let mut cache = QueryScoreCache::with_groups(query, groups.slots());
            let mut cached_matcher = matcher(query, max_typos);
            let mut reference = matcher(query, max_typos);
            for offset in 0..fields.len() * 2 {
                let index = offset % fields.len();
                let index = if reverse {
                    fields.len() - index - 1
                } else {
                    index
                };
                let field = &fields[index];
                let expected = reference
                    .match_one(field.as_str(), 0)
                    .map(|found| found.score);
                assert_eq!(
                    cache.score_grouped(&mut cached_matcher, field, ids[index]),
                    Ok(expected),
                    "synthetic query {query:?}, field {field:?}, typos {max_typos}"
                );
            }
            let (computed, reused) = cache.stats();
            assert_eq!(computed + reused, fields.len() * 2);
            if !query.is_empty() && query_safe(query) && ids.iter().all(Option::is_some) {
                assert_eq!(computed, groups.len());
            }
        }
    }

    #[test]
    fn grouped_scores_match_original_ascii_unicode_case_and_padding_paths() {
        let mut fields = words(&['0', '9', 'a', 'A', 'é', '_', '\0'], 3);
        fields.extend(
            [
                "é0z", "é9z", "É0z", "É9z", "ſ0z", "ſ9z", "İ0z", "İ9z", "K0z", "K9z", "ß0z", "ß9z",
                "ẞ0z", "ẞ9z", "𐐀0z", "𐐀9z", "𐐨0z", "𐐨9z",
            ]
            .map(str::to_owned),
        );
        for query in [
            "", "az", "Az", "é", "É", "ſ", "s", "İ", "i", "K", "k", "ß", "ẞ", "𐐀", "𐐨", "\0",
            "a\0z", "0", "é0z",
        ] {
            for max_typos in [0, 1] {
                compare_grouped(query, max_typos, &fields);
            }
        }
    }

    #[test]
    fn grouped_rejections_and_zero_scores_are_reused() {
        let mut groups = FieldGroups::new();
        let id = groups.intern("123").expect("synthetic group");
        assert_eq!(groups.intern("987"), Some(id));
        for (query, expected) in [("ab", None), ("a", Some(0)), ("é", Some(0))] {
            let mut cache = QueryScoreCache::with_groups(query, groups.slots());
            let mut matcher = matcher(query, 1);
            assert_eq!(
                cache.score_grouped(&mut matcher, "123", Some(id)),
                Ok(expected)
            );
            assert_eq!(
                cache.score_grouped(&mut matcher, "987", Some(id)),
                Ok(expected)
            );
            assert_eq!(cache.stats(), (1, 1));
        }
    }

    #[test]
    fn grouped_boundaries_and_long_queries_preserve_original_scores() {
        let fields: Vec<_> = [15, 16, 17, 31, 32, 33, 63, 64, 65, 1_023, 1_024, 1_025]
            .into_iter()
            .flat_map(|length| {
                (*b"09").map(move |digit| {
                    format!("é{}z", char::from(digit).to_string().repeat(length - 3))
                })
            })
            .collect();
        for query in ["éz", "Éz", "\0", "é\0z", "é0z"] {
            for max_typos in [0, 1] {
                compare_grouped(query, max_typos, &fields);
            }
        }
        for length in [63, 64, 65, 95, 96] {
            let query = "é".repeat(length);
            let fields = [format!("0{query}z"), format!("9{query}z")];
            compare_grouped(&query, 0, &fields);
            compare_grouped(&query, 1, &fields);
        }
    }

    #[test]
    fn grouped_scores_reset_for_new_queries_and_reused_slots() {
        let mut groups = FieldGroups::new();
        let id = groups.intern("a0z").expect("synthetic group");
        let mut first = QueryScoreCache::with_groups("az", groups.slots());
        assert!(
            first
                .score_grouped(&mut matcher("az", 0), "a0z", Some(id))
                .expect("score")
                .is_some()
        );
        let mut next = QueryScoreCache::with_groups("xx", groups.slots());
        assert_eq!(
            next.score_grouped(&mut matcher("xx", 0), "a0z", Some(id)),
            Ok(None)
        );
        assert_eq!(next.stats(), (1, 0));

        assert!(groups.release(id));
        assert_eq!(groups.intern("x9x"), Some(id));
        let mut replaced = QueryScoreCache::with_groups("az", groups.slots());
        assert_eq!(
            replaced.score_grouped(&mut matcher("az", 0), "x9x", Some(id)),
            Ok(None)
        );
        assert_eq!(replaced.stats(), (1, 0));
        let mut digits = QueryScoreCache::with_groups("x0x", groups.slots());
        let mut digit_matcher = matcher("x0x", 0);
        assert!(
            digits
                .score_grouped(&mut digit_matcher, "x0x", Some(id))
                .expect("score")
                .is_some()
        );
        assert_eq!(
            digits.score_grouped(&mut digit_matcher, "x9x", Some(id)),
            Ok(None)
        );
        assert_eq!(digits.stats(), (2, 0));
    }

    #[test]
    fn missing_groups_fall_back_and_invalid_group_invariants_return_errors() {
        let mut groups = FieldGroups::new();
        for index in 0..2_048 {
            let field: String = [index / 676, index / 26 % 26, index % 26]
                .map(|letter| char::from(b'a' + letter as u8))
                .into_iter()
                .collect();
            assert!(groups.intern(&field).is_some());
        }
        assert_eq!(groups.intern("é0z"), None);
        let mut cache = QueryScoreCache::with_groups("éz", groups.slots());
        let mut cached_matcher = matcher("éz", 0);
        let mut reference = matcher("éz", 0);
        for field in ["é0z".to_owned(), format!("é{}z", "9".repeat(1_022))] {
            assert_eq!(groups.intern(&field), None);
            let expected = reference
                .match_one(field.as_str(), 0)
                .map(|found| found.score);
            assert_eq!(
                cache.score_grouped(&mut cached_matcher, &field, None),
                Ok(expected)
            );
        }
        assert_eq!(cache.stats(), (2, 0));

        let invalid = GroupId::new(2_049).expect("nonzero ID");
        assert_eq!(
            cache.score_grouped(&mut cached_matcher, "é0z", Some(invalid)),
            Err(SearchError::InconsistentMatch)
        );
        let valid = GroupId::new(1).expect("nonzero ID");
        assert_eq!(
            cache.score_grouped(&mut cached_matcher, &"0".repeat(1_025), Some(valid)),
            Err(SearchError::InconsistentMatch)
        );
        assert_eq!(cache.stats(), (2, 0));
    }

    #[test]
    fn equivalent_digits_reuse_the_original_score() {
        let mut cache = QueryScoreCache::new("az");
        let mut matcher = matcher("az", 0);
        let first = cache.score(&mut matcher, "a0123z");
        assert!(first.is_some());
        assert_eq!(cache.score(&mut matcher, "a9876z"), first);
        assert_eq!(cache.stats(), (1, 1));
        assert_eq!(cache.scores.len(), 1);
    }

    #[test]
    fn query_digits_are_not_merged_with_nonmatching_digits() {
        let mut cache = QueryScoreCache::new("a0z");
        let mut matcher = matcher("a0z", 0);
        assert!(cache.score(&mut matcher, "a01z").is_some());
        assert_eq!(cache.score(&mut matcher, "a91z"), None);
        assert_eq!(cache.stats(), (2, 0));
        assert_eq!(cache.scores.len(), 2);
    }

    #[test]
    fn score_and_membership_match_exhaustively_for_short_ascii_fields() {
        let alphabet = ['0', '1', 'a', 'A', '_', '\0'];
        let fields = words(&alphabet, 3);
        for query in words(&alphabet, 2)
            .into_iter()
            .filter(|query| !query.is_empty())
        {
            for max_typos in [0, 1] {
                compare(&query, max_typos, &fields);
            }
        }
    }

    #[test]
    fn boundary_case_prefix_exact_and_padding_paths_match_the_original() {
        let fields: Vec<_> = [
            "",
            "az",
            "Az",
            "AZ",
            "a0z",
            "a9z",
            "_a0z",
            "_a9z",
            "a_0_z",
            "a_9_z",
            "aB012z",
            "aB987z",
            "012az",
            "987az",
            "az012",
            "az987",
            "0aZ",
            "9aZ",
            "a\0z012",
            "a\0z987",
            "\0",
            "0123456789",
            "9876543210",
            "aé0z",
            "aé9z",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        for query in [
            "az",
            "Az",
            "aZ",
            "0",
            "a0z",
            "0123456789",
            "\0",
            "a\0z",
            "é",
        ] {
            for max_typos in [0, 1] {
                compare(query, max_typos, &fields);
            }
        }
    }

    #[test]
    fn rejected_fields_and_accepted_zero_scores_are_cached_distinctly() {
        let mut rejected = QueryScoreCache::new("ab");
        let mut rejected_matcher = matcher("ab", 1);
        assert_eq!(rejected.score(&mut rejected_matcher, "123"), None);
        assert_eq!(rejected.score(&mut rejected_matcher, "987"), None);
        assert_eq!(rejected.stats(), (1, 1));

        let mut zero = QueryScoreCache::new("a");
        let mut zero_matcher = matcher("a", 1);
        assert_eq!(zero.score(&mut zero_matcher, "123"), Some(0));
        assert_eq!(zero.score(&mut zero_matcher, "987"), Some(0));
        assert_eq!(zero.stats(), (1, 1));
    }

    #[test]
    fn long_fields_unicode_and_new_queries_do_not_reuse_other_state() {
        let mut cache = QueryScoreCache::new("az");
        let mut ascii_matcher = matcher("az", 1);
        let fields = ["aé0z".to_owned(), format!("a{}z", "0".repeat(1_023))];
        for field in &fields {
            assert!(cache.score(&mut ascii_matcher, field).is_some());
        }
        assert_eq!(cache.stats(), (2, 0));
        assert!(cache.scores.is_empty());

        let mut cache = QueryScoreCache::new("é");
        let mut unicode_matcher = matcher("é", 1);
        assert_eq!(cache.score(&mut unicode_matcher, "012"), Some(0));
        assert_eq!(cache.score(&mut unicode_matcher, "987"), Some(0));
        assert_eq!(cache.stats(), (2, 0));
        assert!(cache.scores.is_empty());

        let mut cache = QueryScoreCache::new("a");
        let mut first_matcher = matcher("a", 1);
        assert!(
            cache
                .score(&mut first_matcher, "a01")
                .is_some_and(|score| score > 0)
        );
        let mut cache = QueryScoreCache::new("z");
        assert_eq!(cache.stats(), (0, 0));
        assert!(cache.scores.is_empty());
        assert_eq!(cache.score(&mut matcher("z", 1), "a01"), Some(0));
    }

    #[test]
    fn field_length_and_simd_edges_match_without_changing_fallbacks() {
        let fields: Vec<_> = [15, 16, 17, 31, 32, 33, 63, 64, 65, 1_023, 1_024, 1_025]
            .into_iter()
            .flat_map(|length| {
                (*b"09").map(move |digit| {
                    format!("a{}z", char::from(digit).to_string().repeat(length - 2))
                })
            })
            .collect();
        for query in ["az", "a0z", "\0", "a\0z"] {
            for max_typos in [0, 1] {
                compare(query, max_typos, &fields);
            }
        }
    }

    #[test]
    fn long_near_matches_and_query_length_edges_preserve_original_scores() {
        let fragment = "reproducible synthetic catalogue exploration includes detailed release notes and project reference material for keyboard navigation";
        let fields = ["012345", "987654", "001199", "1234567"]
            .map(|digits| format!("synthetic row-{digits} {fragment} {fragment}"));
        let query = format!("{}xyz", &fragment[..93]);
        compare(&query, 1, &fields);
        let mut cache = QueryScoreCache::new(&query);
        let mut matcher = matcher(&query, 1);
        let first = cache.score(&mut matcher, &fields[0]);
        assert!(first.is_some());
        assert_eq!(cache.score(&mut matcher, &fields[1]), first);
        assert_eq!(cache.stats(), (1, 1));

        for length in [63, 64, 65, 95, 96] {
            let query = "ab".repeat(length).chars().take(length).collect::<String>();
            let fields = [
                format!("012{query}"),
                format!("987{query}"),
                format!("{query}012"),
                format!("{query}987"),
            ];
            compare(&query, 0, &fields);
            compare(&query, 1, &fields);
        }
    }

    #[test]
    fn cache_limits_fall_back_to_original_scoring() {
        for lengths in [1..=1_024, 600..=1_024] {
            let mut cache = QueryScoreCache::new("z");
            let mut matcher = matcher("z", 0);
            for length in lengths {
                assert_eq!(cache.score(&mut matcher, &"0".repeat(length)), None);
            }
            assert!(cache.scores.len() <= MAX_ENTRIES);
            assert!(cache.key_bytes <= MAX_KEY_BYTES);
            assert!(cache.computed > cache.scores.len());
            let expected = matcher.match_one("z9", 0).map(|found| found.score);
            assert!(expected.is_some());
            assert_eq!(cache.score(&mut matcher, "z9"), expected);
        }
    }
}
