//! Caches conservative field metadata for membership rejection and score bounds.

use frizbee::Scoring;

use crate::{Entry, TextField};

const MAX_DP_FIELD_BYTES: usize = 1_024;

#[derive(Clone, Copy)]
pub(crate) struct AsciiPresence(Option<u128>);

impl AsciiPresence {
    pub(crate) fn new(text: &str) -> Self {
        let mut characters = 0u128;
        for byte in text.bytes() {
            if !byte.is_ascii() {
                return Self(None);
            }
            characters |= ascii_bit(byte);
        }
        Self(Some(characters))
    }

    /// Use only against ASCII fields after accounting for forced non-ASCII deletions.
    pub(crate) fn ascii_projection(text: &str) -> Self {
        let mut characters = 0u128;
        for byte in text.bytes().filter(u8::is_ascii) {
            characters |= ascii_bit(byte);
        }
        Self(Some(characters))
    }

    fn may_match(&self, query: &Self, max_typos: u16) -> bool {
        let (Some(query), Some(field)) = (query.0, self.0) else {
            return true;
        };

        // Every absent distinct byte requires at least one needle deletion in frizbee.
        // Folding even smart-case queries admits extra candidates but cannot lose a match.
        (query & !field).count_ones() <= u32::from(max_typos)
    }
}

fn ascii_bit(byte: u8) -> u128 {
    1u128 << byte.to_ascii_lowercase()
}

#[derive(Clone, Copy)]
pub(crate) struct FieldMasks {
    title: AsciiPresence,
    address: AsciiPresence,
    title_boundaries: u32,
    address_boundaries: u32,
}

impl FieldMasks {
    pub(crate) fn new(entry: &Entry) -> Self {
        Self {
            title: AsciiPresence::new(&entry.title),
            address: AsciiPresence::new(&entry.address),
            title_boundaries: boundary_count(&entry.title),
            address_boundaries: boundary_count(&entry.address),
        }
    }

    pub(crate) fn may_match(
        &self,
        field: TextField,
        query: &AsciiPresence,
        max_typos: u16,
    ) -> bool {
        match field {
            TextField::Title => self.title.may_match(query, max_typos),
            TextField::Address => self.address.may_match(query, max_typos),
        }
    }

    pub(crate) fn is_ascii(&self, field: TextField) -> bool {
        match field {
            TextField::Title => self.title.0.is_some(),
            TextField::Address => self.address.0.is_some(),
        }
    }

    pub(crate) fn score_upper_bound(&self, entry: &Entry, query: &QueryBound<'_>) -> u16 {
        query
            .field_score(&entry.title, self.title_boundaries)
            .max(query.field_score(&entry.address, self.address_boundaries))
    }
}

pub(crate) struct QueryBound<'a> {
    text: &'a str,
    unicode_units: Option<usize>,
    scoring: Scoring,
    coarse: u16,
}

impl<'a> QueryBound<'a> {
    pub(crate) fn new(text: &'a str) -> Self {
        Self {
            text,
            unicode_units: (!text.is_ascii()).then(|| text.chars().count()),
            scoring: Scoring::default(),
            coarse: score_upper_bound(text.len()),
        }
    }

    fn field_score(&self, field: &str, boundaries: u32) -> u16 {
        // Longer original fields might take the pinned scorer's byte-based greedy fallback.
        let units = match self.unicode_units {
            Some(scalars) if field.len() <= MAX_DP_FIELD_BYTES => scalars,
            _ => self.text.len(),
        };
        let units = units.min(usize::from(u16::MAX)) as u32;
        let scoring = &self.scoring;
        let matched = u32::from(scoring.match_score) + u32::from(scoring.matching_case_bonus);
        let boundary = u32::from(scoring.delimiter_bonus.max(scoring.capitalization_bonus));
        // The wrapper's exact bonus also requires the trimmed window to cover the whole field.
        let exact = if self.text == field {
            u32::from(scoring.exact_match_bonus)
        } else {
            0
        };

        // Every real boundary contributes at most once; padded NUL lanes create no boundaries.
        let upper = units
            .saturating_mul(matched)
            .saturating_add(units.min(boundaries).saturating_mul(boundary))
            .saturating_add(u32::from(scoring.prefix_bonus))
            .saturating_add(exact);
        upper.min(u32::from(self.coarse)) as u16
    }
}

fn boundary_count(text: &str) -> u32 {
    text.as_bytes()
        .windows(2)
        .filter(|pair| {
            (is_delimiter(pair[0]) && !is_delimiter(pair[1]))
                || (pair[0].is_ascii_lowercase() && pair[1].is_ascii_uppercase())
        })
        .count()
        .min(u32::MAX as usize) as u32
}

#[cfg(test)]
pub(crate) struct GroupBoundCache<'query> {
    query: QueryBound<'query>,
    fields: Option<Vec<Option<u16>>>,
    computed: usize,
    reused: usize,
}

#[cfg(test)]
impl<'query> GroupBoundCache<'query> {
    pub(crate) fn new(text: &'query str, slots: usize, enabled: bool) -> Self {
        Self {
            query: QueryBound::new(text),
            fields: (enabled && !text.is_empty() && crate::field_groups::query_safe(text))
                .then(|| vec![None; slots]),
            computed: 0,
            reused: 0,
        }
    }

    pub(crate) fn score_upper_bound(
        &mut self,
        entry: &Entry,
        masks: &FieldMasks,
        ids: [Option<crate::field_groups::GroupId>; 2],
    ) -> Result<u16, crate::SearchError> {
        let Some(fields) = &mut self.fields else {
            self.computed += 2;
            return Ok(masks.score_upper_bound(entry, &self.query));
        };
        let mut maximum = 0;
        for (text, boundaries, id) in [
            (entry.title.as_str(), masks.title_boundaries, ids[0]),
            (entry.address.as_str(), masks.address_boundaries, ids[1]),
        ] {
            let cached = match id {
                Some(id) => {
                    if text.len() > MAX_DP_FIELD_BYTES {
                        return Err(crate::SearchError::InconsistentMatch);
                    }
                    Some(
                        fields
                            .get_mut(id.get() as usize - 1)
                            .ok_or(crate::SearchError::InconsistentMatch)?,
                    )
                }
                None => None,
            };
            let bound = if let Some(Some(bound)) = cached.as_deref() {
                self.reused += 1;
                *bound
            } else {
                // Groups preserve length and boundary classes. A digit-free exact match is shared too.
                let bound = self.query.field_score(text, boundaries);
                self.computed += 1;
                if let Some(cached) = cached {
                    *cached = Some(bound);
                }
                bound
            };
            maximum = maximum.max(bound);
        }
        Ok(maximum)
    }

    pub(crate) fn stats(&self) -> (usize, usize) {
        (self.computed, self.reused)
    }
}

fn is_delimiter(byte: u8) -> bool {
    byte.is_ascii() && !byte.is_ascii_alphanumeric()
}

pub(crate) fn score_upper_bound(query_bytes: usize) -> u16 {
    let scoring = Scoring::default();
    // Each query unit can earn one match, case bonus and delimiter OR capitalization bonus.
    // Bytes also bound the pinned matcher's long-Unicode greedy fallback.
    let per_unit = u32::from(scoring.match_score)
        + u32::from(scoring.matching_case_bonus)
        + u32::from(scoring.delimiter_bonus.max(scoring.capitalization_bonus));
    let one_time = u32::from(scoring.prefix_bonus) + u32::from(scoring.exact_match_bonus);
    let bounded_units = query_bytes.min(usize::from(u16::MAX)) as u32;
    bounded_units
        .saturating_mul(per_unit)
        .saturating_add(one_time)
        .min(u32::from(u16::MAX)) as u16
}

#[cfg(test)]
mod tests {
    use super::AsciiPresence;

    fn may_match(query: &str, field: &str, max_typos: u16) -> bool {
        AsciiPresence::new(field).may_match(&AsciiPresence::new(query), max_typos)
    }

    #[test]
    fn distinct_missing_characters_supply_only_a_lower_bound() {
        assert!(!may_match("abc", "a", 1));
        assert!(may_match("abc", "a", 2));
        assert!(may_match("aaaa", "xyz", 1));
        assert!(may_match("abc", "cba", 0));
    }

    #[test]
    fn ascii_case_folding_never_excludes_smart_case_candidates() {
        assert!(may_match("AbC", "aBc", 0));
        assert!(!may_match("ABC", "ab", 0));
        assert!(may_match("ABC", "ab", 1));
    }

    #[test]
    fn every_ascii_byte_including_controls_has_a_presence_bit() {
        for byte in 0u8..=127 {
            let text = char::from(byte).to_string();
            assert!(may_match(&text, &text, 0));
            assert!(!may_match(&text, "", 0));
            assert!(may_match(&text, "", 1));
        }
        assert!(!may_match("\0\u{7f}", "\0", 0));
        assert!(may_match("\0\u{7f}", "\0", 1));
    }

    #[test]
    fn unicode_in_either_string_bypasses_the_filter() {
        assert!(may_match("missing", "café", 0));
        assert!(may_match("é", "ascii", 0));
        assert!(may_match("東京", "🧭", 0));
    }

    #[test]
    fn empty_strings_respect_the_query_deletion_budget() {
        assert!(may_match("", "", 0));
        assert!(may_match("", "anything", 0));
        assert!(!may_match("abc", "", 2));
        assert!(may_match("abc", "", 3));
    }

    #[test]
    fn large_typo_budgets_do_not_wrap_or_reject_absent_bytes() {
        let query: String = (0u8..=127).map(char::from).collect();
        assert!(may_match(&query, "", 128));
        assert!(may_match(&query, "", u16::MAX));
    }
}
