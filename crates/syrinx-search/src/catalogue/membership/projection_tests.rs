//! Checks that ASCII projection rejects only fields the original matcher cannot accept.

use frizbee::{Config, Matcher};

use super::{
    tests::{entry, intern},
    *,
};

fn words(alphabet: &[char], depth: usize) -> Vec<String> {
    let mut all = vec![String::new()];
    let mut frontier = all.clone();
    for _ in 0..depth {
        let next: Vec<_> = frontier
            .iter()
            .flat_map(|prefix| {
                alphabet
                    .iter()
                    .map(move |character| format!("{prefix}{character}"))
            })
            .collect();
        all.extend(next.iter().cloned());
        frontier = next;
    }
    all
}

fn check_query(text: &str, budget: u16, fields: &[String]) {
    let rows: Vec<_> = fields.iter().map(|field| entry(field, field)).collect();
    let mut groups = FieldGroups::new();
    let ids: Vec<_> = rows.iter().map(|row| intern(&mut groups, row)).collect();
    let masks: Vec<_> = rows.iter().map(FieldMasks::new).collect();
    let mut matcher = Matcher::new(text, &Config::default().max_typos(Some(budget)));
    let mut projected = GroupedMembership::new(text, budget, &groups);
    let mut original = GroupedMembership::with_ascii_projection(text, budget, &groups, false);
    let mut uncached = GroupedMembership::new(text, budget, &groups);
    assert_eq!(projected.ascii_projection.is_some(), !text.is_ascii());
    assert!(original.ascii_projection.is_none());
    for (index, row) in rows.iter().enumerate() {
        let expected = matcher.match_one(row.title.as_str(), 0).is_some();
        if expected {
            assert!(projected.may_match_field(&masks[index], TextField::Title, None));
        }
        assert_eq!(
            projected.entry_matches(row, &masks[index], ids[index]),
            Ok(expected),
            "projected query {text:?}, field {:?}, budget {budget}",
            row.title,
        );
        assert_eq!(
            original.entry_matches(row, &masks[index], ids[index]),
            Ok(expected)
        );
        assert_eq!(
            uncached.entry_matches(row, &masks[index], [None; 2]),
            Ok(expected)
        );
    }
}

#[test]
fn projection_preserves_public_membership_for_exhaustive_short_ascii_fields() {
    let mut fields = words(&['a', 'B', '_', '\0'], 3);
    fields.extend(["éa", "Éa", "aéB", "café", "東京", "a0B", "a9B"].map(str::to_owned));
    for ascii in words(&['a', 'B', '\0'], 2) {
        let mut queries = vec![ascii.clone()];
        for scalar in ['é', 'É', 'ſ', 'K', 'İ', '\u{301}', '😀'] {
            for position in 0..=ascii.len() {
                let mut query = ascii.clone();
                query.insert(position, scalar);
                queries.push(query);
            }
        }
        queries.push(format!("é{ascii}é"));
        for query in queries {
            for budget in [0, 1] {
                check_query(query.as_str(), budget, &fields);
            }
        }
    }
}

#[test]
fn projected_presence_is_conservative_for_repeats_case_and_literal_nul() {
    for (query, field, budget, may_match, expected) in [
        ("éaaaa", "a", 1, true, false),
        ("éaa", "", 1, false, false),
        ("é", "", 1, true, true),
        ("é", "ordinary", 0, false, false),
        ("éé", "ordinary", 1, false, false),
        ("Éa", "A", 1, true, false),
        ("Éa", "a", 1, true, true),
        ("café", "CAF", 1, true, true),
        ("cafe\u{301}", "CAFE", 1, true, true),
        ("café", "synthetic project", 1, false, false),
        ("café", "café", 0, true, true),
        ("ſ", "S", 0, false, false),
        ("K", "K", 0, false, false),
        ("İ", "I", 0, false, false),
        ("\0é", "\0", 1, true, true),
        ("\0é", "", 1, false, false),
        ("éab", "a", 1, false, false),
        ("éab", "ba", 1, true, false),
        ("éab", "a_b", 1, true, true),
        ("é11", "1", 1, true, false),
        ("é11", "11", 1, true, true),
    ] {
        let row = entry(field, field);
        let masks = FieldMasks::new(&row);
        let groups = FieldGroups::new();
        let mut projected = GroupedMembership::new(query, budget, &groups);
        assert_eq!(
            projected.may_match_field(&masks, TextField::Title, None),
            may_match,
            "{query:?}/{field:?}"
        );
        let counts = [DigitCounts::new(field); 2];
        assert_eq!(
            projected.entry_matches_counted(&row, &masks, [None; 2], &counts),
            Ok(expected)
        );
        check_query(query, budget, &[field.to_owned()]);
    }
}

#[test]
fn projection_keeps_scalar_counts_and_long_retained_ascii_boundaries() {
    for length in [0usize, 1, 62, 63, 64, 93] {
        let ascii = "a".repeat(length);
        let fields = [
            ascii.clone(),
            "a".repeat(length.saturating_sub(1)),
            format!("{ascii}A"),
        ];
        for scalar in ['é', '😀'] {
            let query = format!("{scalar}{ascii}");
            check_query(query.as_str(), 0, &fields);
            check_query(query.as_str(), 1, &fields);
        }
    }
    for length in [1_023, 1_024, 1_025] {
        let gap = "_".repeat(length - 3);
        let fields = [format!("a{gap}\0b"), format!("a{gap}_b")];
        check_query("éa\0b", 1, &fields);
    }
    let query = "é".repeat(64);
    check_query(
        query.as_str(),
        1,
        &[query.clone(), "é".repeat(63), "a".repeat(128)],
    );
}

#[test]
fn projection_preserves_field_union_group_reuse_and_exhausted_pool_fallback() {
    let mut groups = FieldGroups::new();
    let rows = [entry("a0b", ""), entry("a9b", ""), entry("missing", "a4b")];
    let ids: Vec<_> = rows.iter().map(|row| intern(&mut groups, row)).collect();
    assert_eq!(ids[0][0], ids[1][0]);
    let shared_title = ids[0][0].expect("shared title group");
    let rejected_title = ids[2][0].expect("rejected title group");
    let mut query = GroupedMembership::new("éab", 1, &groups);
    for (row, ids) in rows.iter().zip(ids) {
        assert_eq!(
            query.entry_matches(row, &FieldMasks::new(row), ids),
            Ok(true)
        );
    }
    assert_eq!(
        query
            .memo
            .as_ref()
            .expect("digit-free memo")
            .iter()
            .flatten()
            .count(),
        2
    );
    let memo = query.memo.as_ref().expect("digit-free memo");
    assert_eq!(memo[(shared_title.get() - 1) as usize], Some(true));
    assert_eq!(memo[(rejected_title.get() - 1) as usize], Some(false));

    let mut groups = FieldGroups::new();
    for number in 0..2_048 {
        let suffix: String = (0..11)
            .map(|bit| if number & (1 << bit) == 0 { 'a' } else { 'b' })
            .collect();
        assert!(groups.intern(&format!("synthetic-{suffix}")).is_some());
    }
    let row = entry("missing", "a0b");
    let ids = intern(&mut groups, &row);
    assert_eq!(ids, [None; 2]);
    let mut query = GroupedMembership::new("éab", 1, &groups);
    assert_eq!(
        query.entry_matches(&row, &FieldMasks::new(&row), ids),
        Ok(true)
    );
    assert!(
        query
            .memo
            .as_ref()
            .expect("memo")
            .iter()
            .all(Option::is_none)
    );
}

#[test]
fn projection_keeps_active_group_errors_and_prunes_only_uncached_fields() {
    let groups = FieldGroups::new();
    let invalid = Some(GroupId::new(u32::MAX).expect("invalid nonzero group"));
    let rejected = entry("no required symbol", "");
    let rejected_masks = FieldMasks::new(&rejected);
    let mut query = GroupedMembership::new("é\0", 1, &groups);
    assert!(query.may_match_field(&rejected_masks, TextField::Title, invalid));
    assert_eq!(
        query.entry_matches(&rejected, &rejected_masks, [invalid; 2]),
        Err(SearchError::InconsistentMatch)
    );
    let retained = entry("\0", "");
    assert_eq!(
        query.entry_matches(&retained, &FieldMasks::new(&retained), [invalid; 2]),
        Err(SearchError::InconsistentMatch)
    );
    assert!(!query.may_match_field(&rejected_masks, TextField::Title, None));
    assert_eq!(
        query.entry_matches(&rejected, &rejected_masks, [None; 2]),
        Ok(false)
    );

    // Digit queries have no group memo, so even a retained field ID uses projection.
    let mut digit_query = GroupedMembership::new("é0\0", 1, &groups);
    let digit_row = entry("0 required symbol", "");
    let digit_masks = FieldMasks::new(&digit_row);
    assert!(digit_query.memo.is_none());
    assert!(!digit_query.may_match_field(&digit_masks, TextField::Title, invalid));
    assert_eq!(
        digit_query.entry_matches(&digit_row, &digit_masks, [invalid; 2]),
        Ok(false)
    );
}

#[test]
fn projection_leaves_other_budgets_unchanged() {
    let groups = FieldGroups::new();
    let retained = entry("\0", "");
    for budget in [2, u16::MAX] {
        let mut projected = GroupedMembership::new("é\0", budget, &groups);
        let mut original = GroupedMembership::with_ascii_projection("é\0", budget, &groups, false);
        assert!(projected.ascii_projection.is_none());
        let masks = FieldMasks::new(&retained);
        assert_eq!(
            projected.entry_matches(&retained, &masks, [None; 2]),
            original.entry_matches(&retained, &masks, [None; 2])
        );
        assert_eq!(projected.membership.matches_ascii("\0", budget), None);
    }
}
