//! Counts shared pairs once and expands only ranges that can improve the selected results.

use std::{
    cmp::{Ordering, Reverse},
    collections::BinaryHeap,
    time::Instant,
};

use super::super::{
    Catalogue, GroupedMembership, QueryObserver, QueryStage, RankKey, Ranked, boost, boost_values,
    retain_best, score_fields,
};
use super::{Bucket, Class, PhaseTimings, QueryReport, RangeIndex, Route, State};
use crate::{
    BoostWeights, Scope, ScopeCounts, SearchError, SearchPolicy, candidate::QueryBound,
    field_groups::query_safe, score_cache::QueryScoreCache,
};

#[derive(Clone, Copy)]
struct Range<'a> {
    bucket: &'a Bucket,
    class: &'a Class,
    pair: usize,
    node: usize,
    start: usize,
    width: usize,
    exact: bool,
}

enum Source<'a> {
    Range(Range<'a>),
    Fallback(usize),
}

struct Pending<'a> {
    bound: RankKey,
    source: Source<'a>,
}

pub(super) struct BoundRequest<'a> {
    pub(super) bucket: &'a Bucket,
    pub(super) class: &'a Class,
    pub(super) node: usize,
    pub(super) start: usize,
    pub(super) base: u16,
}

pub(super) fn bound_priority(
    catalogue: &Catalogue,
    request: BoundRequest<'_>,
    now: i64,
    weights: BoostWeights,
    metadata: bool,
) -> Result<RankKey, SearchError> {
    let minimum = request.bucket.minimum[request.node] as usize;
    let target = request
        .bucket
        .members
        .get(minimum)
        .ok_or(SearchError::InconsistentMatch)?
        .target;
    let first = &request.bucket.members[request.start];
    if metadata {
        let score = u32::from(request.base)
            + boost_values(
                request.class.open,
                request.class.frequency,
                first.last_used,
                now,
                weights,
            );
        return Ok((Reverse(score), target));
    }
    // Keep the original two position lookups as the paired experimental reference.
    let minimum_target = catalogue
        .positions
        .get_key_value(&target)
        .expect("minimum target has a catalogue row")
        .0;
    let first_entry = &catalogue.entries[catalogue.positions[&first.target]];
    let score = u32::from(request.base) + boost(first_entry, now, weights);
    Ok((Reverse(score), *minimum_target))
}

impl PartialEq for Pending<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.bound == other.bound
    }
}
impl Eq for Pending<'_> {}
impl Ord for Pending<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        other.bound.cmp(&self.bound)
    }
}
impl PartialOrd for Pending<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

struct PairScore {
    representative: usize,
    exact: Option<u16>,
}

struct Search<'a> {
    counts: ScopeCounts,
    pending: BinaryHeap<Pending<'a>>,
    selected: BinaryHeap<Ranked>,
    pairs: Vec<PairScore>,
    scores: QueryScoreCache,
    candidates: usize,
    examined: usize,
    frontier: usize,
    report: QueryReport,
}

impl<'a> Search<'a> {
    fn new(text: &str, slots: usize, frontier: usize, report: QueryReport) -> Self {
        Self {
            counts: ScopeCounts::default(),
            pending: BinaryHeap::new(),
            selected: BinaryHeap::new(),
            pairs: Vec::new(),
            scores: QueryScoreCache::with_groups(text, slots),
            candidates: 0,
            examined: 0,
            frontier,
            report: QueryReport {
                attempted: true,
                ..report
            },
        }
    }

    fn push(&mut self, pending: Pending<'a>) -> bool {
        if self.pending.len() == self.frontier {
            return false;
        }
        self.pending.push(pending);
        self.report.frontier_peak = self.report.frontier_peak.max(self.pending.len());
        true
    }

    fn finish_report(&self, route: Route) -> QueryReport {
        QueryReport {
            route,
            work_recorded: true,
            matching_rows: self.counts.total(),
            candidate_rows: self.candidates,
            selected_rows: self.selected.len(),
            ..self.report
        }
    }
}

impl Catalogue {
    pub(in super::super) fn rank_ranges(
        &mut self,
        text: &str,
        scope: Scope,
        now: i64,
        policy: SearchPolicy,
        observer: &mut impl QueryObserver,
    ) -> Result<ScopeCounts, SearchError> {
        if policy.max_typos > 1 {
            if let Some(index) = &mut self.ranges {
                index.begin_query(Route::UnsupportedBudget);
            }
            return self.rank_matches(text, scope, now, policy, true);
        }
        if !query_safe(text) {
            if let Some(index) = &mut self.ranges {
                index.begin_query(Route::DigitQuery);
            }
            return self.rank_bounded(text, scope, now, policy, observer);
        }
        let Some(mut index) = self.ranges.take() else {
            return self.rank_bounded(text, scope, now, policy, observer);
        };
        index.begin_query(Route::Unavailable);
        // Restore the index on every Result path, including matcher errors and frontier restart.
        let result = self.try_ranges(text, scope, now, policy, &mut index, observer);
        if result.is_err() {
            index.last_query.route = Route::Error;
        }
        self.ranges = Some(index);
        match result? {
            Some(counts) => Ok(counts),
            None => self.rank_bounded(text, scope, now, policy, observer),
        }
    }

    fn try_ranges(
        &mut self,
        text: &str,
        scope: Scope,
        now: i64,
        policy: SearchPolicy,
        index: &mut RangeIndex,
        observer: &mut impl QueryObserver,
    ) -> Result<Option<ScopeCounts>, SearchError> {
        let Some(state) = &index.state else {
            return Ok(None);
        };
        index.last_query.attempted = true;
        let mut search = Search::new(
            text,
            self.groups.slots(),
            index.limits.frontier,
            index.last_query,
        );
        let candidates_started = index.phase_timing_enabled.then(Instant::now);
        if !self.range_candidates(state, text, scope, now, policy, &mut search)? {
            index.last_query = search.finish_report(Route::FrontierAdmission);
            return Ok(None);
        }
        let expansion_started = candidates_started.map(|_| Instant::now());
        while let Some(pending) = search.pending.pop() {
            search.report.frontier_pops += 1;
            if search.selected.len() == policy.limit
                && search
                    .selected
                    .peek()
                    .is_some_and(|worst| pending.bound >= worst.key())
            {
                break;
            }
            if !self.expand_range(pending, now, policy, &mut search)? {
                index.last_query = search.finish_report(Route::FrontierExpansion);
                return Ok(None);
            }
        }
        index.last_query = search.finish_report(Route::Ranges);
        // Publish completed phase timings only after a successful range attempt.
        index.last_phases = candidates_started
            .zip(expansion_started)
            .map(|(start, middle)| PhaseTimings {
                candidates: middle.duration_since(start),
                expansion: middle.elapsed(),
            });
        observer.candidates(search.candidates);
        observer.stage_finished(QueryStage::RangeMembershipAndRanking);
        for _ in 0..search.examined {
            observer.candidate_scored();
        }
        let (computed, reused) = search.scores.stats();
        observer.field_scores(computed, reused);
        self.ranked.extend(search.selected);
        Ok(Some(search.counts))
    }

    fn range_candidates<'a>(
        &self,
        state: &'a State,
        text: &str,
        scope: Scope,
        now: i64,
        policy: SearchPolicy,
        search: &mut Search<'a>,
    ) -> Result<bool, SearchError> {
        let mut membership = GroupedMembership::new(text, policy.max_typos, &self.groups);
        let bound = QueryBound::new(text);
        for pair in state.pairs.values() {
            search.report.pairs_examined += 1;
            let representative = self.positions[&pair.representative()];
            let entry = &self.entries[representative];
            if !membership.entry_matches_counted(
                entry,
                &self.field_masks[representative],
                self.field_groups[representative],
                &self.digit_counts[representative],
            )? {
                continue;
            }
            search.counts.tabs += pair.counts[0];
            search.counts.saved += pair.counts[1];
            search.counts.history += pair.counts[2];
            search.counts.commands += pair.counts[3];
            let pair_slot = search.pairs.len();
            search.pairs.push(PairScore {
                representative,
                exact: None,
            });
            if policy.limit == 0 {
                continue;
            }
            // Digit substitution preserves the field-bound inputs for every member of this pair.
            let upper = self.field_masks[representative].score_upper_bound(entry, &bound);
            for (class, bucket) in &pair.buckets {
                if !scope.includes(bucket.representative()) {
                    continue;
                }
                search.candidates += bucket.members.len();
                let range = Range {
                    bucket,
                    class,
                    pair: pair_slot,
                    node: 1,
                    start: 0,
                    width: bucket.minimum.len() / 2,
                    exact: false,
                };
                let pending = self.range_bound(range, upper, now, policy, search)?;
                if !search.push(pending) {
                    return Ok(false);
                }
            }
        }
        for record in &state.fallback {
            search.report.fallback_examined += 1;
            let position = record.position;
            let entry = &self.entries[position];
            if !membership.entry_matches_counted(
                entry,
                &self.field_masks[position],
                self.field_groups[position],
                &self.digit_counts[position],
            )? {
                continue;
            }
            search.counts.include(record.target);
            if policy.limit == 0 || !scope.includes(record.target) {
                continue;
            }
            search.candidates += 1;
            let upper = self.field_masks[position].score_upper_bound(entry, &bound);
            if !search.push(Pending {
                bound: Ranked::new(position, entry, upper, now, policy.boosts).key(),
                source: Source::Fallback(position),
            }) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn range_bound<'a>(
        &self,
        range: Range<'a>,
        base: u16,
        now: i64,
        policy: SearchPolicy,
        search: &mut Search<'a>,
    ) -> Result<Pending<'a>, SearchError> {
        search.report.bound_constructions += 1;
        let metadata = search.report.metadata_bounds_enabled;
        search.report.bound_position_lookups += if metadata { 0 } else { 2 };
        let request = BoundRequest {
            bucket: range.bucket,
            class: range.class,
            node: range.node,
            start: range.start,
            base,
        };
        let bound = bound_priority(self, request, now, policy.boosts, metadata)?;
        Ok(Pending {
            bound,
            source: Source::Range(range),
        })
    }

    fn expand_range<'a>(
        &mut self,
        pending: Pending<'a>,
        now: i64,
        policy: SearchPolicy,
        search: &mut Search<'a>,
    ) -> Result<bool, SearchError> {
        let range = match pending.source {
            Source::Fallback(index) => {
                let ranked = self.score_candidate(index, now, policy.boosts, &mut search.scores)?;
                search.examined += 1;
                search.report.fallback_rows_scored += 1;
                retain_best(&mut search.selected, ranked, policy.limit);
                return Ok(true);
            }
            Source::Range(range) => range,
        };
        let pair = &mut search.pairs[range.pair];
        let base = match pair.exact {
            Some(score) => score,
            None => {
                search.report.pairs_scored += 1;
                let score = score_fields(
                    &self.entries[pair.representative],
                    self.field_groups[pair.representative],
                    &mut self.matcher,
                    &mut search.scores,
                )?;
                pair.exact = Some(score);
                score
            }
        };
        if !range.exact {
            let pending = self.range_bound(
                Range {
                    exact: true,
                    ..range
                },
                base,
                now,
                policy,
                search,
            )?;
            return Ok(search.push(pending));
        }
        if range.width == 1 {
            let target = range.bucket.members[range.start].target;
            let index = self.positions[&target];
            let ranked = Ranked::new(index, &self.entries[index], base, now, policy.boosts);
            search.examined += 1;
            search.report.range_leaves_ranked += 1;
            retain_best(&mut search.selected, ranked, policy.limit);
            return Ok(true);
        }
        let width = range.width / 2;
        for offset in 0..2 {
            let start = range.start + offset * width;
            if start >= range.bucket.members.len() {
                continue;
            }
            let child = Range {
                node: range.node * 2 + offset,
                start,
                width,
                ..range
            };
            let pending = self.range_bound(child, base, now, policy, search)?;
            if !search.push(pending) {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
