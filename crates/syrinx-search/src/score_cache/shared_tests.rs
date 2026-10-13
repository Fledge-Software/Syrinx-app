//! Verifies query-local score sharing without caching across group mutations or numeric queries.

use std::{num::NonZeroU32, thread};

use frizbee::{Config, SortStrategy};

use super::*;
use crate::field_groups::FieldGroups;

#[test]
fn concurrent_partitions_compute_each_equivalent_group_once() {
    let fields = [
        "café123東京\0ax",
        "café456東京\0ax",
        "café789東京\0ax",
        "café000東京\0ax",
    ];
    let mut groups = FieldGroups::new();
    let ids = fields.map(|field| groups.intern(field).expect("synthetic group"));
    assert!(ids.iter().all(|id| *id == ids[0]));
    for query in ["ax", "café", "東京", "\0", "zzzz", "z", "1"] {
        for max_typos in [0, 1] {
            check_shared_query(query, max_typos, fields, ids, groups.slots());
        }
    }
}

struct ScoredField {
    field: &'static str,
    score: Option<u16>,
    computed: usize,
    reused: usize,
}

fn check_shared_query(
    query: &str,
    max_typos: u16,
    fields: [&'static str; 4],
    ids: [crate::field_groups::GroupId; 4],
    slots: usize,
) {
    let config = Config::default()
        .max_typos(Some(max_typos))
        .sort(SortStrategy::IndexAsc);
    let results = score_workers(query, &config, fields, ids, slots);
    let mut reference = Matcher::new(query, &config);
    let mut computed = 0;
    let mut reused = 0;
    for result in results {
        assert_eq!(
            result.score,
            reference
                .match_one(result.field, 0)
                .map(|found| found.score)
        );
        computed += result.computed;
        reused += result.reused;
    }
    assert_eq!(computed, if query_safe(query) { 1 } else { 4 });
    assert_eq!(computed + reused, fields.len());
}

fn score_workers(
    query: &str,
    config: &Config,
    fields: [&'static str; 4],
    ids: [crate::field_groups::GroupId; 4],
    slots: usize,
) -> Vec<ScoredField> {
    let shared = QueryScoreCache::shared_groups(query, slots);
    thread::scope(|scope| {
        let mut pending = Vec::new();
        for (field, id) in fields.into_iter().zip(ids) {
            let shared = shared.clone();
            pending.push(scope.spawn(move || {
                let mut scores = QueryScoreCache::with_shared_groups(query, slots, shared);
                let mut matcher = Matcher::new(query, config);
                let score = scores
                    .score_grouped(&mut matcher, field, Some(id))
                    .expect("shared original score");
                let (computed, reused) = scores.stats();
                ScoredField {
                    field,
                    score,
                    computed,
                    reused,
                }
            }));
        }
        pending
            .into_iter()
            .map(|worker| worker.join().expect("synthetic worker"))
            .collect()
    })
}

#[test]
fn shared_hits_preserve_field_guards_and_new_queries_start_empty() {
    let mut groups = FieldGroups::new();
    let id = groups.intern("ax123").expect("synthetic group");
    let config = Config::default().max_typos(Some(0));
    let mut matcher = Matcher::new("ax", &config);
    let shared =
        QueryScoreCache::shared_groups("ax", groups.slots()).expect("eligible shared groups");
    let mut scores =
        QueryScoreCache::with_shared_groups("ax", groups.slots(), Some(shared.clone()));
    assert!(
        scores
            .score_grouped(&mut matcher, "ax123", Some(id))
            .expect("first score")
            .is_some()
    );
    assert!(shared[0].get().is_some());
    assert_eq!(
        scores.score_grouped(&mut matcher, &"x".repeat(MAX_FIELD_BYTES + 1), Some(id)),
        Err(SearchError::InconsistentMatch)
    );
    assert_eq!(
        scores.score_grouped(&mut matcher, "ax123", NonZeroU32::new(u32::MAX)),
        Err(SearchError::InconsistentMatch)
    );

    // The next query receives new cells even when a mutation recycles the same group slot.
    assert!(groups.release(id));
    assert_eq!(groups.intern("zz456"), Some(id));
    let fresh = QueryScoreCache::shared_groups("ax", groups.slots()).expect("fresh query cache");
    assert!(fresh.iter().all(|slot| slot.get().is_none()));
    let mut scores = QueryScoreCache::with_shared_groups("ax", groups.slots(), Some(fresh));
    assert_eq!(
        scores.score_grouped(&mut matcher, "zz456", Some(id)),
        Ok(None)
    );
    assert!(QueryScoreCache::shared_groups("1", groups.slots()).is_none());
    assert!(QueryScoreCache::shared_groups("", groups.slots()).is_none());
}
