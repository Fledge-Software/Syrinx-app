//! Checks partition counts, local cutoffs and mutation against the serial and exhaustive paths.

use std::num::NonZeroU64;

use super::*;
use crate::{BlankQuery, Usage};

fn fixture(rows: usize) -> Catalogue {
    let mut catalogue = Catalogue::new();
    for index in 0..rows {
        let id = NonZeroU64::new(index as u64 + 1).expect("synthetic ID");
        let target = match index % 4 {
            0 => Target::History(id),
            1 => Target::Command(id),
            2 => Target::Tab(atlas_core::TabId::new(id.get()).expect("synthetic tab")),
            _ => Target::Saved(atlas_core::SavedPageId::new(id.get()).expect("synthetic saved")),
        };
        catalogue
            .upsert(Entry {
                target,
                title: match index % 5 {
                    0 => "project café 東京 🧭\0".to_owned(),
                    1 => "project cafe\u{301} row-000008".to_owned(),
                    2 => format!("{}project", "x".repeat(1_050)),
                    _ => format!("PROJECT row-{index:06}"),
                },
                address: format!("https://partition.test/{index}/project"),
                usage: Usage {
                    visits: index as u32,
                    last_used: Some(index as i64),
                },
                open: index.is_multiple_of(3),
            })
            .expect("insert synthetic row");
    }
    catalogue
}

fn compare(catalogue: &mut Catalogue, query: &str, scope: Scope, now: i64, policy: SearchPolicy) {
    let serial = catalogue
        .query_with_strategy(
            query,
            scope,
            now,
            policy,
            QueryStrategy::Bounded,
            &mut Unobserved,
        )
        .expect("serial exact result");
    let parallel = catalogue
        .query_with_strategy(
            query,
            scope,
            now,
            policy,
            QueryStrategy::Partitioned,
            &mut Unobserved,
        )
        .expect("partitioned exact result");
    assert_eq!(parallel, serial);
    assert_eq!(
        parallel,
        catalogue
            .query_exhaustive(query, scope, now, policy)
            .expect("exhaustive result")
    );
}

#[test]
fn counts_ties_and_limits_match_across_uneven_partitions() {
    let mut catalogue = fixture(19);
    let mut policy = SearchPolicy {
        limit: 1,
        max_typos: 0,
        blank: BlankQuery::All,
        boosts: BoostWeights {
            open: 0,
            frequency: 0,
            recency: 0,
            recency_window_secs: NonZeroU64::new(1).expect("nonzero window"),
        },
    };
    for scope in [
        Scope::Everything,
        Scope::Tabs,
        Scope::Saved,
        Scope::History,
        Scope::Commands,
    ] {
        for limit in [0, 1, 4, 19, usize::MAX] {
            policy.limit = limit;
            compare(&mut catalogue, "project", scope, 0, policy);
        }
    }
}

#[test]
fn unicode_digits_long_fields_and_mutations_preserve_original_results() {
    let mut catalogue = fixture(17);
    let mut policy = SearchPolicy {
        limit: 7,
        max_typos: 1,
        blank: BlankQuery::All,
        boosts: BoostWeights {
            open: u16::MAX,
            frequency: u16::MAX,
            recency: u16::MAX,
            recency_window_secs: NonZeroU64::new(u64::MAX).expect("nonzero window"),
        },
    };
    for now in [i64::MIN, i64::MAX] {
        for query in [
            "",
            "p",
            "porject",
            "café",
            "cafe\u{301}",
            "東京",
            "🧭",
            "\0",
            "000008",
            "qzxvjk",
        ] {
            compare(&mut catalogue, query, Scope::Everything, now, policy);
        }
    }
    // Remove from the first partition, relocate the final row, then recycle field-group slots.
    let target = catalogue.entries[0].target;
    assert!(catalogue.remove(target).expect("synthetic removal"));
    let mut changed = catalogue.entries[0].clone();
    changed.title = "new project café".to_owned();
    changed.address = "https://changed.test/000008".to_owned();
    changed.usage.visits = u32::MAX;
    changed.open = !changed.open;
    catalogue
        .upsert(changed.clone())
        .expect("synthetic replacement");
    changed.target = target;
    catalogue.upsert(changed).expect("synthetic reinsertion");
    for typos in [0, 1] {
        policy.max_typos = typos;
        for query in ["project", "000008", "café"] {
            compare(&mut catalogue, query, Scope::Everything, 9, policy);
        }
    }
}

#[test]
fn empty_and_single_row_partitions_are_exact() {
    let policy = SearchPolicy {
        limit: 50,
        max_typos: 0,
        blank: BlankQuery::All,
        boosts: BoostWeights {
            open: 0,
            frequency: 0,
            recency: 0,
            recency_window_secs: NonZeroU64::new(1).expect("nonzero window"),
        },
    };
    for rows in 0..=4 {
        compare(&mut fixture(rows), "project", Scope::Everything, 0, policy);
    }
}

#[test]
fn partition_failure_returns_no_partial_counts_or_hits() {
    let mut catalogue = fixture(19);
    let policy = SearchPolicy {
        limit: 7,
        max_typos: 0,
        blank: BlankQuery::All,
        boosts: BoostWeights {
            open: 0,
            frequency: 0,
            recency: 0,
            recency_window_secs: NonZeroU64::new(1).expect("nonzero window"),
        },
    };
    let original = catalogue.field_groups[11];
    catalogue.field_groups[11] = [GroupId::new(u32::MAX); 2];
    assert_eq!(
        catalogue.query_with_strategy(
            "project",
            Scope::Everything,
            0,
            policy,
            QueryStrategy::Partitioned,
            &mut Unobserved,
        ),
        Err(SearchError::InconsistentMatch),
    );
    catalogue.field_groups[11] = original;
    compare(&mut catalogue, "project", Scope::Everything, 0, policy);
}

#[test]
fn public_large_catalogue_keeps_unsupported_budgets_and_blank_policy() {
    let mut catalogue = fixture(MIN_ROWS);
    let mut policy = SearchPolicy {
        limit: 9,
        max_typos: 2,
        blank: BlankQuery::None,
        boosts: BoostWeights {
            open: 0,
            frequency: 0,
            recency: 0,
            recency_window_secs: NonZeroU64::new(1).expect("nonzero window"),
        },
    };
    for query in ["prjct", ""] {
        assert_eq!(
            catalogue
                .query(query, Scope::Saved, 1, policy)
                .expect("public fallback"),
            catalogue
                .query_exhaustive(query, Scope::Saved, 1, policy)
                .expect("exhaustive fallback")
        );
    }
    policy.max_typos = 1;
    assert_eq!(
        catalogue
            .query("", Scope::Everything, 1, policy)
            .expect("blank policy"),
        catalogue
            .query_exhaustive("", Scope::Everything, 1, policy)
            .expect("exhaustive blank")
    );
}
