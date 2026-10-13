//! Checks cached-first membership against the existing path and the public matcher.

use frizbee::{Config, Matcher};

use super::{
    tests::{entry, intern},
    *,
};

fn check_row(
    original: &mut GroupedMembership<'_>,
    cached_first: &mut GroupedMembership<'_>,
    row: &Entry,
    ids: [Option<GroupId>; 2],
    expected: Result<bool, SearchError>,
) {
    let masks = FieldMasks::new(row);
    let counts = [DigitCounts::new(&row.title), DigitCounts::new(&row.address)];
    assert_eq!(
        original.entry_matches_counted(row, &masks, ids, &counts),
        expected,
    );
    assert_eq!(
        cached_first.entry_matches_counted_cached_first(row, &masks, ids, &counts),
        expected,
    );
    assert_eq!(original.memo, cached_first.memo);
}

#[test]
fn cached_true_and_false_preserve_title_address_union() {
    let rows = [entry("a0b", ""), entry("b0a", ""), entry("a9b", "b9a")];
    let mut groups = FieldGroups::new();
    let ids: Vec<_> = rows.iter().map(|row| intern(&mut groups, row)).collect();
    assert_eq!(ids[0][0], ids[2][0]);
    assert_eq!(ids[1][0], ids[2][1]);
    let mut original = GroupedMembership::new("ba", 0, &groups);
    let mut cached_first = GroupedMembership::new("ba", 0, &groups);
    for index in [0, 1, 2, 0, 1, 2] {
        check_row(
            &mut original,
            &mut cached_first,
            &rows[index],
            ids[index],
            Ok(index != 0),
        );
    }
    let memo = cached_first.memo.as_ref().expect("enabled memo");
    assert_eq!(
        memo[(ids[0][0].expect("rejected group").get() - 1) as usize],
        Some(false)
    );
    assert_eq!(
        memo[(ids[1][0].expect("matching group").get() - 1) as usize],
        Some(true)
    );
}

#[test]
fn cold_and_warm_grouped_and_uncached_fields_match_public_membership() {
    let rows = [
        entry("a123B", ""),
        entry("a987B", ""),
        entry("É123a", "é456A"),
        entry("É987a", "é654A"),
        entry("a\0B123", "東京123🧭"),
        entry("a\0B987", "東京987🧭"),
        entry("", "cafe\u{301}"),
        entry("café", "CAF"),
        entry("ſKİ", "SKI"),
        entry("1", "11"),
        entry("", ""),
        entry(&"a".repeat(1_025), ""),
    ];
    let mut groups = FieldGroups::new();
    let ids: Vec<_> = rows.iter().map(|row| intern(&mut groups, row)).collect();
    assert!(ids[11][0].is_none());
    for text in [
        "", "ab", "aB", "Ba", "éa", "Éa", "a\0B", "東京", "🧭", "café", "ſ", "K", "İ", "123", "11",
        "é11",
    ] {
        for budget in [0, 1] {
            for grouped in [false, true] {
                let mut original = GroupedMembership::new(text, budget, &groups);
                let mut cached_first = GroupedMembership::new(text, budget, &groups);
                let mut matcher = Matcher::new(text, &Config::default().max_typos(Some(budget)));
                for index in (0..rows.len()).chain((0..rows.len()).rev()) {
                    let row = &rows[index];
                    let expected = matcher.match_one(row.title.as_str(), 0).is_some()
                        || matcher.match_one(row.address.as_str(), 0).is_some();
                    check_row(
                        &mut original,
                        &mut cached_first,
                        row,
                        if grouped { ids[index] } else { [None; 2] },
                        Ok(expected),
                    );
                }
            }
        }
    }
}

#[test]
fn invalid_ids_preserve_mask_rejection_and_checked_errors() {
    let mut groups = FieldGroups::new();
    let released = groups.intern("a").expect("synthetic group");
    assert!(groups.release(released));
    for invalid in [
        released,
        GroupId::new(u32::MAX).expect("invalid nonzero ID"),
    ] {
        for (text, row, expected) in [
            ("z", entry("a", ""), Ok(false)),
            ("a", entry("a", ""), Err(SearchError::InconsistentMatch)),
            ("é", entry("a", ""), Err(SearchError::InconsistentMatch)),
        ] {
            let mut original = GroupedMembership::new(text, 0, &groups);
            let mut cached_first = GroupedMembership::new(text, 0, &groups);
            check_row(
                &mut original,
                &mut cached_first,
                &row,
                [Some(invalid), None],
                expected,
            );
        }
    }
}

#[test]
fn cached_title_results_preserve_address_error_order() {
    let rejected = entry("ab", "");
    let accepted = entry("ba", "");
    let mut groups = FieldGroups::new();
    let rejected_ids = intern(&mut groups, &rejected);
    let accepted_ids = intern(&mut groups, &accepted);
    let invalid = Some(GroupId::new(u32::MAX).expect("invalid nonzero ID"));
    let mut original = GroupedMembership::new("ba", 0, &groups);
    let mut cached_first = GroupedMembership::new("ba", 0, &groups);
    check_row(
        &mut original,
        &mut cached_first,
        &rejected,
        rejected_ids,
        Ok(false),
    );
    check_row(
        &mut original,
        &mut cached_first,
        &accepted,
        accepted_ids,
        Ok(true),
    );
    for (row, ids, expected) in [
        (
            entry("ab", "ba"),
            [rejected_ids[0], invalid],
            Err(SearchError::InconsistentMatch),
        ),
        (entry("ab", "z"), [rejected_ids[0], invalid], Ok(false)),
        (entry("ba", "ba"), [accepted_ids[0], invalid], Ok(true)),
    ] {
        check_row(&mut original, &mut cached_first, &row, ids, expected);
    }
}

#[test]
fn disabled_memo_keeps_original_digits_and_budget_fallbacks() {
    let rows = [
        entry("a123x", ""),
        entry("a987x", ""),
        entry("1", ""),
        entry("11", ""),
    ];
    let mut groups = FieldGroups::new();
    let ids: Vec<_> = rows.iter().map(|row| intern(&mut groups, row)).collect();
    let long_query = "1".repeat(64);
    for (text, budget) in [
        ("123", 0),
        ("11", 1),
        (long_query.as_str(), 0),
        ("ax", 2),
        ("é", u16::MAX),
    ] {
        let mut original = GroupedMembership::new(text, budget, &groups);
        let mut cached_first = GroupedMembership::new(text, budget, &groups);
        assert!(original.memo.is_none());
        assert!(cached_first.memo.is_none());
        for (row, ids) in rows.iter().zip(&ids) {
            let masks = FieldMasks::new(row);
            let counts = [DigitCounts::new(&row.title), DigitCounts::new(&row.address)];
            let expected = original.entry_matches_counted(row, &masks, *ids, &counts);
            check_row(&mut original, &mut cached_first, row, *ids, expected);
        }
    }
}

#[test]
fn reused_group_id_starts_with_a_fresh_query_memo() {
    let mut groups = FieldGroups::new();
    let old_id = groups.intern("a").expect("old synthetic group");
    {
        let mut original = GroupedMembership::new("a", 0, &groups);
        let mut cached_first = GroupedMembership::new("a", 0, &groups);
        check_row(
            &mut original,
            &mut cached_first,
            &entry("a", ""),
            [Some(old_id), None],
            Ok(true),
        );
    }
    assert!(groups.release(old_id));
    let new_id = groups.intern("b").expect("replacement synthetic group");
    assert_eq!(old_id, new_id);
    let mut original = GroupedMembership::new("a", 0, &groups);
    let mut cached_first = GroupedMembership::new("a", 0, &groups);
    assert!(
        cached_first
            .memo
            .as_ref()
            .expect("enabled memo")
            .iter()
            .all(Option::is_none)
    );
    check_row(
        &mut original,
        &mut cached_first,
        &entry("b", ""),
        [Some(new_id), None],
        Ok(false),
    );
}
