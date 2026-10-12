//! Splits exact counting and bounded ranking over disjoint borrowed catalogue rows.

use std::{ops::Range, thread};

use super::*;
use crate::score_cache::SharedScores;

const PARTITIONS: usize = 4;
pub(super) const MIN_ROWS: usize = 16_384;

#[derive(Clone, Copy)]
pub(super) struct Rows<'a> {
    entries: &'a [Entry],
    masks: &'a [FieldMasks],
    digits: &'a [[DigitCounts; 2]],
    fields: &'a [[Option<GroupId>; 2]],
    groups: &'a FieldGroups,
}

impl<'a> Rows<'a> {
    pub(super) fn new(catalogue: &'a Catalogue) -> Self {
        Self::from_parts(
            &catalogue.entries,
            &catalogue.field_masks,
            &catalogue.digit_counts,
            &catalogue.field_groups,
            &catalogue.groups,
        )
    }

    pub(super) fn from_parts(
        entries: &'a [Entry],
        masks: &'a [FieldMasks],
        digits: &'a [[DigitCounts; 2]],
        fields: &'a [[Option<GroupId>; 2]],
        groups: &'a FieldGroups,
    ) -> Self {
        Self {
            entries,
            masks,
            digits,
            fields,
            groups,
        }
    }

    pub(super) fn bound_candidates(
        self,
        range: Range<usize>,
        text: &str,
        scope: Scope,
        now: i64,
        policy: SearchPolicy,
    ) -> Result<(ScopeCounts, Vec<Reverse<Ranked>>), SearchError> {
        let mut membership = GroupedMembership::new(text, policy.max_typos, self.groups);
        let bound = QueryBound::new(text);
        let mut counts = ScopeCounts::default();
        let mut candidates = Vec::new();

        // Count the title/address union even outside the displayed scope and result limit.
        for index in range {
            let entry = &self.entries[index];
            let masks = &self.masks[index];
            if !membership.entry_matches_counted(
                entry,
                masks,
                self.fields[index],
                &self.digits[index],
            )? {
                continue;
            }
            counts.include(entry.target);
            if policy.limit > 0 && scope.includes(entry.target) {
                candidates.push(Reverse(Ranked::new(
                    index,
                    entry,
                    masks.score_upper_bound(entry, &bound),
                    now,
                    policy.boosts,
                )));
            }
        }
        Ok((counts, candidates))
    }

    pub(super) fn score_candidate(
        self,
        index: usize,
        matcher: &mut Matcher,
        now: i64,
        weights: BoostWeights,
        scores: &mut QueryScoreCache,
    ) -> Result<Ranked, SearchError> {
        let entry = self
            .entries
            .get(index)
            .ok_or(SearchError::InvalidMatchIndex)?;
        let groups = self
            .fields
            .get(index)
            .ok_or(SearchError::InvalidMatchIndex)?;
        let base = score_fields(entry, *groups, matcher, scores)?;
        Ok(Ranked::new(index, entry, base, now, weights))
    }

    pub(super) fn select<Observer: QueryObserver>(
        self,
        mut ranking: CandidateRanking,
        matcher: &mut Matcher,
        now: i64,
        policy: SearchPolicy,
        observer: &mut Observer,
    ) -> Result<(ScopeCounts, BinaryHeap<Ranked>), SearchError> {
        observer.candidates(ranking.candidates.len() + ranking.direct_count);
        observer.stage_finished(QueryStage::MembershipAndCandidates);
        let mut pending = BinaryHeap::from(ranking.candidates);

        // A remaining bound that loses to the cutoff cannot change this partition's top N.
        while let Some(Reverse(candidate)) = pending.pop() {
            if ranking.selected.len() == policy.limit
                && ranking
                    .selected
                    .peek()
                    .is_some_and(|worst| Ranked::compare(&candidate, worst) != Ordering::Less)
            {
                break;
            }
            let ranked = self.score_candidate(
                candidate.index,
                matcher,
                now,
                policy.boosts,
                &mut ranking.scores,
            )?;
            observer.candidate_scored();
            retain_best(&mut ranking.selected, ranked, policy.limit);
        }
        let (computed, reused) = ranking.scores.stats();
        observer.field_scores(computed, reused);
        Ok((ranking.counts, ranking.selected))
    }

    fn search(
        self,
        range: Range<usize>,
        text: &str,
        scope: Scope,
        now: i64,
        policy: SearchPolicy,
        shared: Option<SharedScores>,
    ) -> Result<(ScopeCounts, BinaryHeap<Ranked>), SearchError> {
        let (counts, candidates) = self.bound_candidates(range, text, scope, now, policy)?;
        let ranking = CandidateRanking {
            counts,
            candidates,
            selected: BinaryHeap::new(),
            scores: QueryScoreCache::with_shared_groups(text, self.groups.slots(), shared),
            direct_count: 0,
        };
        let mut matcher = Matcher::new(text, &matcher_config(policy.max_typos));
        self.select(ranking, &mut matcher, now, policy, &mut Unobserved)
    }

    pub(super) fn search_partitions(
        self,
        text: &str,
        scope: Scope,
        now: i64,
        policy: SearchPolicy,
    ) -> Result<(ScopeCounts, Vec<Ranked>), SearchError> {
        if self.entries.is_empty() {
            return Ok((ScopeCounts::default(), Vec::new()));
        }
        thread::scope(|workers| {
            let shared = QueryScoreCache::shared_groups(text, self.groups.slots());
            let width = self.entries.len().div_ceil(PARTITIONS);
            let mut pending = Vec::new();
            let mut completed = Vec::new();
            for start in (width..self.entries.len()).step_by(width) {
                let range = start..(start + width).min(self.entries.len());
                let worker_range = range.clone();
                let worker_scores = shared.clone();
                let worker = thread::Builder::new().spawn_scoped(workers, move || {
                    self.search(worker_range, text, scope, now, policy, worker_scores)
                });
                match worker {
                    Ok(worker) => pending.push(worker),
                    // Resource exhaustion degrades to serial work without losing any rows.
                    Err(_) => {
                        completed.push(self.search(range, text, scope, now, policy, shared.clone()))
                    }
                }
            }
            completed.push(self.search(
                0..width.min(self.entries.len()),
                text,
                scope,
                now,
                policy,
                shared,
            ));
            for worker in pending {
                completed.push(match worker.join() {
                    Ok(result) => result,
                    Err(panic) => std::panic::resume_unwind(panic),
                });
            }

            // Disjoint counts add exactly; every global finalist is a local finalist somewhere.
            let mut counts = ScopeCounts::default();
            let mut finalists = Vec::new();
            for result in completed {
                let (partition_counts, ranked) = result?;
                counts.tabs += partition_counts.tabs;
                counts.saved += partition_counts.saved;
                counts.history += partition_counts.history;
                counts.commands += partition_counts.commands;
                finalists.extend(ranked);
            }
            Ok((counts, finalists))
        })
    }
}

#[cfg(test)]
mod tests;
