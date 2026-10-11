//! Reuses membership within one query while keeping each catalogue row independently counted.

use crate::{
    Entry, SearchError, TextField,
    candidate::{AsciiPresence, FieldMasks},
    digit_counts::{DigitCounts, DigitQuery},
    field_groups::{FieldGroups, GroupId, query_safe},
    membership::Membership,
};

pub(super) struct GroupedMembership<'a> {
    membership: Membership,
    mask: AsciiPresence,
    max_typos: u16,
    groups: &'a FieldGroups,
    memo: Option<Vec<Option<bool>>>,
    digits: Option<DigitQuery>,
    ascii_projection: Option<AsciiProjection>,
}

struct AsciiProjection {
    mask: AsciiPresence,
    remaining_typos: Option<u16>,
}

impl<'a> GroupedMembership<'a> {
    pub(super) fn new(text: &str, max_typos: u16, groups: &'a FieldGroups) -> Self {
        Self::prepare(text, max_typos, groups, true)
    }

    #[cfg(test)]
    pub(super) fn with_ascii_projection(
        text: &str,
        max_typos: u16,
        groups: &'a FieldGroups,
        enabled: bool,
    ) -> Self {
        Self::prepare(text, max_typos, groups, enabled)
    }

    fn prepare(text: &str, max_typos: u16, groups: &'a FieldGroups, projection: bool) -> Self {
        let membership = Membership::new(text);
        let ascii_projection =
            (projection && max_typos <= 1 && !text.is_ascii()).then(|| AsciiProjection {
                mask: AsciiPresence::ascii_projection(text),
                remaining_typos: membership.ascii_field_budget(max_typos),
            });
        Self {
            membership,
            mask: AsciiPresence::new(text),
            max_typos,
            groups,
            memo: (max_typos <= 1 && query_safe(text)).then(|| vec![None; groups.slots()]),
            digits: DigitQuery::new(text, max_typos),
            ascii_projection,
        }
    }

    pub(super) fn entry_matches_counted(
        &mut self,
        entry: &Entry,
        masks: &FieldMasks,
        ids: [Option<GroupId>; 2],
        counts: &[DigitCounts; 2],
    ) -> Result<bool, SearchError> {
        let Some(digits) = &self.digits else {
            return self.entry_matches(entry, masks, ids);
        };
        // Each field must independently supply enough occurrences of every queried digit.
        let possible = counts.map(|counts| digits.may_match(&counts));
        self.entry_matches_selected(entry, masks, ids, possible)
    }

    pub(super) fn entry_matches(
        &mut self,
        entry: &Entry,
        masks: &FieldMasks,
        ids: [Option<GroupId>; 2],
    ) -> Result<bool, SearchError> {
        self.entry_matches_selected(entry, masks, ids, [true; 2])
    }

    fn entry_matches_selected(
        &mut self,
        entry: &Entry,
        masks: &FieldMasks,
        ids: [Option<GroupId>; 2],
        possible: [bool; 2],
    ) -> Result<bool, SearchError> {
        for (field, text, id, possible) in [
            (TextField::Title, entry.title.as_str(), ids[0], possible[0]),
            (
                TextField::Address,
                entry.address.as_str(),
                ids[1],
                possible[1],
            ),
        ] {
            if !possible || !self.may_match_field(masks, field, id) {
                continue;
            }
            if self.field_matches(text, masks.is_ascii(field), id)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn may_match_field(&self, masks: &FieldMasks, field: TextField, id: Option<GroupId>) -> bool {
        // Avoid repeating projection work for fields that can reuse group membership.
        if let Some(projection) = &self.ascii_projection
            && (id.is_none() || self.memo.is_none())
            && masks.is_ascii(field)
        {
            // Missing ASCII symbols and forced non-ASCII deletions concern disjoint units.
            return projection
                .remaining_typos
                .is_some_and(|remaining| masks.may_match(field, &projection.mask, remaining));
        }
        masks.may_match(field, &self.mask, self.max_typos)
    }

    fn field_matches(
        &mut self,
        text: &str,
        ascii: bool,
        id: Option<GroupId>,
    ) -> Result<bool, SearchError> {
        if let (Some(memo), Some(id)) = (&mut self.memo, id) {
            let slot = (id.get() - 1) as usize;
            let cached = memo.get_mut(slot).ok_or(SearchError::InconsistentMatch)?;
            if let Some(matched) = cached {
                return Ok(*matched);
            }
            let canonical = self.groups.get(id).ok_or(SearchError::InconsistentMatch)?;
            // Canonicalization preserves ASCII status and non-digit query membership.
            let matched = matches(&self.membership, canonical, ascii, self.max_typos)?;
            *cached = Some(matched);
            return Ok(matched);
        }
        matches(&self.membership, text, ascii, self.max_typos)
    }
}

#[cfg(test)]
mod projection_tests;

fn matches(
    membership: &Membership,
    text: &str,
    ascii: bool,
    max_typos: u16,
) -> Result<bool, SearchError> {
    if ascii {
        membership.matches_ascii(text, max_typos)
    } else {
        membership.matches(text, max_typos)
    }
    .ok_or(SearchError::InconsistentMatch)
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use crate::{Target, Usage};

    use super::*;

    pub(super) fn entry(title: &str, address: &str) -> Entry {
        Entry {
            target: Target::History(NonZeroU64::new(1).expect("synthetic history ID")),
            title: title.into(),
            address: address.into(),
            usage: Usage {
                visits: 0,
                last_used: None,
            },
            open: false,
        }
    }

    pub(super) fn intern(groups: &mut FieldGroups, entry: &Entry) -> [Option<GroupId>; 2] {
        [groups.intern(&entry.title), groups.intern(&entry.address)]
    }

    #[test]
    fn matching_and_rejected_groups_are_reused_only_within_the_current_query() {
        let first = entry("a123x", "");
        let second = entry("a987x", "");
        let first_masks = FieldMasks::new(&first);
        let second_masks = FieldMasks::new(&second);
        let mut groups = FieldGroups::new();
        let first_ids = intern(&mut groups, &first);
        let second_ids = intern(&mut groups, &second);
        assert_eq!(first_ids, second_ids);
        for (text, expected) in [("ax", true), ("xa", false)] {
            let mut query = GroupedMembership::new(text, 0, &groups);
            assert!(
                query
                    .memo
                    .as_ref()
                    .expect("enabled memo")
                    .iter()
                    .all(Option::is_none)
            );
            assert_eq!(
                query.entry_matches(&first, &first_masks, first_ids),
                Ok(expected)
            );
            assert_eq!(
                query.entry_matches(&second, &second_masks, second_ids),
                Ok(expected)
            );
            let memo = query.memo.as_ref().expect("enabled memo");
            assert_eq!(memo.iter().filter(|cached| cached.is_some()).count(), 1);
            assert_eq!(
                memo[(first_ids[0].expect("title group").get() - 1) as usize],
                Some(expected)
            );
        }
    }

    #[test]
    fn digit_queries_use_original_fields_and_never_allocate_group_results() {
        let first = entry("a123x", "");
        let second = entry("a987x", "");
        let mut groups = FieldGroups::new();
        let first_ids = intern(&mut groups, &first);
        let second_ids = intern(&mut groups, &second);
        let mut query = GroupedMembership::new("123", 0, &groups);
        assert!(query.memo.is_none());
        assert_eq!(
            query.entry_matches(&first, &FieldMasks::new(&first), first_ids),
            Ok(true)
        );
        assert_eq!(
            query.entry_matches(&second, &FieldMasks::new(&second), second_ids),
            Ok(false)
        );
        assert!(GroupedMembership::new("ax", 2, &groups).memo.is_none());
    }

    #[test]
    fn uncached_fields_and_title_address_union_keep_unicode_and_smart_case_semantics() {
        let rows = [
            entry("É123a", "https://synthetic.example.test/"),
            entry("é456A", "https://synthetic.example.test/"),
            entry("unrelated", "é987A"),
            entry("a\0B123", ""),
            entry("東京123🧭", ""),
        ];
        let mut groups = FieldGroups::new();
        let ids: Vec<_> = rows.iter().map(|row| intern(&mut groups, row)).collect();
        for text in ["éa", "Éa", "a\0B", "東京", "🧭", "unrelated", ""] {
            for budget in [0, 1] {
                let mut grouped = GroupedMembership::new(text, budget, &groups);
                let mut uncached = GroupedMembership::new(text, budget, &groups);
                for (row, ids) in rows.iter().zip(&ids) {
                    let masks = FieldMasks::new(row);
                    assert_eq!(
                        grouped.entry_matches(row, &masks, *ids),
                        uncached.entry_matches(row, &masks, [None; 2]),
                        "synthetic query {text:?}, budget {budget}",
                    );
                }
            }
        }
    }

    #[test]
    fn missing_or_out_of_bounds_group_ids_report_an_inconsistent_match() {
        let row = entry("synthetic", "");
        let masks = FieldMasks::new(&row);
        let mut groups = FieldGroups::new();
        let id = groups.intern(&row.title).expect("synthetic title group");
        assert!(groups.release(id));
        for invalid in [id, GroupId::new(u32::MAX).expect("nonzero invalid ID")] {
            let mut query = GroupedMembership::new("synthetic", 0, &groups);
            assert_eq!(
                query.entry_matches(&row, &masks, [Some(invalid), None]),
                Err(SearchError::InconsistentMatch)
            );
        }
        let empty = FieldGroups::new();
        let mut query = GroupedMembership::new("synthetic", 0, &empty);
        assert_eq!(
            query.entry_matches(&row, &masks, [Some(id), None]),
            Err(SearchError::InconsistentMatch)
        );
    }
}
