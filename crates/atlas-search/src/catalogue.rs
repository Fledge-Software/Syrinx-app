//! Maintains a derived catalogue and ranks matches without sorting every row.

use std::{
    cmp::{Ordering, Reverse},
    collections::{BinaryHeap, HashMap},
    fmt,
};

use frizbee::{Config, Match, MatchIndices, Matcher, Scoring, SortStrategy};

use crate::{
    BlankQuery, BoostWeights, Entry, Scope, ScopeCounts, SearchError, SearchHit, SearchPolicy,
    SearchResults, Target, TextField,
    candidate::{AsciiPresence, FieldMasks, QueryBound},
    digit_counts::DigitCounts,
    field_groups::{FieldGroups, GroupId},
    highlights,
    score_cache::QueryScoreCache,
};

mod membership;
use membership::GroupedMembership;
mod highlight_cache;
use highlight_cache::HighlightCache;

#[cfg(test)]
mod hybrid;
#[cfg(test)]
mod occupancy;
#[cfg(test)]
mod range_index;

#[derive(Clone, Copy)]
enum QueryStrategy {
    Bounded,
    Filtered,
    #[cfg(test)]
    Exhaustive,
    #[cfg(test)]
    Hybrid(hybrid::Budget),
    #[cfg(test)]
    Ranges,
    #[cfg(test)]
    BoundReuse(bool),
}

impl QueryStrategy {
    fn cache_highlights(self) -> bool {
        #[cfg(test)]
        if matches!(self, Self::Exhaustive) {
            return false;
        }
        true
    }
}

pub struct Catalogue {
    entries: Vec<Entry>,
    field_masks: Vec<FieldMasks>,
    digit_counts: Vec<[DigitCounts; 2]>,
    groups: FieldGroups,
    field_groups: Vec<[Option<GroupId>; 2]>,
    positions: HashMap<Target, usize>,
    generation: u64,
    matcher: Matcher,
    ranked: Vec<Ranked>,
    #[cfg(test)]
    ranges: Option<range_index::RangeIndex>,
}

#[derive(Clone, Copy)]
enum QueryStage {
    Setup,
    MembershipAndCandidates,
    HeapAndExactScoring,
    FinalistOrdering,
    HighlightsAndOwnedResults,
    #[cfg(test)]
    RangeMembershipAndRanking,
}

trait QueryObserver {
    fn stage_finished(&mut self, _stage: QueryStage) {}
    fn candidates(&mut self, _count: usize) {}
    fn candidate_scored(&mut self) {}
    fn field_scores(&mut self, _computed: usize, _reused: usize) {}
    fn highlight_traces(&mut self, _computed: usize, _reused: usize) {}
    #[cfg(test)]
    fn hybrid_work(&mut self, _direct: usize, _pending: usize, _new_groups: usize, _work: usize) {}
    #[cfg(test)]
    fn bound_work(&mut self, _computed: usize, _reused: usize) {}
}

struct CandidateRanking {
    counts: ScopeCounts,
    candidates: Vec<Reverse<Ranked>>,
    selected: BinaryHeap<Ranked>,
    scores: QueryScoreCache,
    direct_count: usize,
}

struct Unobserved;

impl QueryObserver for Unobserved {}

impl fmt::Debug for Catalogue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Catalogue")
            .field("len", &self.len())
            .field("generation", &self.generation)
            .finish()
    }
}

impl Default for Catalogue {
    fn default() -> Self {
        Self::new()
    }
}

impl Catalogue {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            field_masks: Vec::new(),
            digit_counts: Vec::new(),
            groups: FieldGroups::new(),
            field_groups: Vec::new(),
            positions: HashMap::new(),
            generation: 0,
            matcher: Matcher::new("", &matcher_config(0)),
            ranked: Vec::new(),
            #[cfg(test)]
            ranges: None,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Changes only the addressed row. Identical input leaves the generation unchanged.
    pub fn upsert(&mut self, entry: Entry) -> Result<(), SearchError> {
        validate_entry(&entry)?;
        let position = self.positions.get(&entry.target).copied();
        if position.is_some_and(|index| self.entries[index] == entry) {
            return Ok(());
        }
        if position.is_none() && self.len() >= u32::MAX as usize {
            return Err(SearchError::TooManyEntries);
        }
        let generation = next_generation(self.generation)?;
        let masks = FieldMasks::new(&entry);
        let digits = [
            DigitCounts::new(&entry.title),
            DigitCounts::new(&entry.address),
        ];

        // All fallible validation precedes mutation of the derived catalogue.
        #[cfg(test)]
        let range_updated = if let (Some(ranges), Some(index)) = (&mut self.ranges, position) {
            let updated = ranges.try_update_in_place(
                &self.entries[index],
                &entry,
                self.field_groups[index],
                index,
            );
            if !updated {
                ranges.before_remove(&self.entries[index], self.field_groups[index], index);
            }
            updated
        } else {
            false
        };
        let groups = self.updated_field_groups(position, &entry);
        #[cfg(test)]
        let index = position.unwrap_or(self.entries.len());
        if let Some(index) = position {
            self.entries[index] = entry;
            self.field_masks[index] = masks;
            self.digit_counts[index] = digits;
            self.field_groups[index] = groups;
        } else {
            self.positions.insert(entry.target, self.entries.len());
            self.entries.push(entry);
            self.field_masks.push(masks);
            self.digit_counts.push(digits);
            self.field_groups.push(groups);
        }
        self.generation = generation;
        #[cfg(test)]
        if !range_updated && let Some(ranges) = &mut self.ranges {
            ranges.after_change(
                Some((index, &self.entries[index], self.field_groups[index])),
                &self.entries,
                &self.field_groups,
            );
        }
        Ok(())
    }

    pub fn remove(&mut self, target: Target) -> Result<bool, SearchError> {
        let Some(index) = self.positions.get(&target).copied() else {
            return Ok(false);
        };
        let generation = next_generation(self.generation)?;
        #[cfg(test)]
        if let Some(ranges) = &mut self.ranges {
            ranges.before_remove(&self.entries[index], self.field_groups[index], index);
        }
        self.positions.remove(&target);
        self.entries.swap_remove(index);
        self.field_masks.swap_remove(index);
        self.digit_counts.swap_remove(index);
        for id in self.field_groups.swap_remove(index).into_iter().flatten() {
            assert!(self.groups.release(id), "removed field owns its group");
        }
        if let Some(moved) = self.entries.get(index) {
            self.positions.insert(moved.target, index);
            #[cfg(test)]
            if let Some(ranges) = &mut self.ranges {
                ranges.relocated(moved.target, self.entries.len(), index);
            }
        }
        self.generation = generation;
        #[cfg(test)]
        if let Some(ranges) = &mut self.ranges {
            ranges.after_change(None, &self.entries, &self.field_groups);
        }
        Ok(true)
    }

    fn updated_field_groups(
        &mut self,
        position: Option<usize>,
        entry: &Entry,
    ) -> [Option<GroupId>; 2] {
        let Some(index) = position else {
            return [
                self.groups.intern(&entry.title),
                self.groups.intern(&entry.address),
            ];
        };
        let previous = &self.entries[index];
        let mut ids = self.field_groups[index];
        // Unchanged fields retain their leases, including fields that previously used fallback.
        for (field, old, new) in [
            (0, previous.title.as_str(), entry.title.as_str()),
            (1, previous.address.as_str(), entry.address.as_str()),
        ] {
            if old == new {
                continue;
            }
            if let Some(id) = ids[field] {
                assert!(self.groups.release(id), "replaced field owns its group");
            }
            ids[field] = self.groups.intern(new);
        }
        ids
    }

    pub fn get(&self, target: Target) -> Option<&Entry> {
        self.positions
            .get(&target)
            .and_then(|&index| self.entries.get(index))
    }

    /// Matches title and address independently and ranks each target by its best field.
    /// Counts cover all matching targets before the selected scope and display limit.
    pub fn query(
        &mut self,
        text: &str,
        scope: Scope,
        now: i64,
        policy: SearchPolicy,
    ) -> Result<SearchResults, SearchError> {
        let strategy = if policy.max_typos <= 1 {
            QueryStrategy::Bounded
        } else {
            QueryStrategy::Filtered
        };
        self.query_with_strategy(text, scope, now, policy, strategy, &mut Unobserved)
    }

    #[cfg(test)]
    fn query_exhaustive(
        &mut self,
        text: &str,
        scope: Scope,
        now: i64,
        policy: SearchPolicy,
    ) -> Result<SearchResults, SearchError> {
        self.query_with_strategy(
            text,
            scope,
            now,
            policy,
            QueryStrategy::Exhaustive,
            &mut Unobserved,
        )
    }

    fn query_with_strategy<Observer: QueryObserver>(
        &mut self,
        text: &str,
        scope: Scope,
        now: i64,
        policy: SearchPolicy,
        strategy: QueryStrategy,
        observer: &mut Observer,
    ) -> Result<SearchResults, SearchError> {
        #[cfg(test)]
        if matches!(strategy, QueryStrategy::Ranges)
            && let Some(ranges) = &mut self.ranges
        {
            ranges.prepare_query(text.is_empty());
        }
        if text.len() > Scoring::default().max_needle_len() {
            return Err(SearchError::QueryTooLong);
        }
        self.ranked.clear();
        if !text.is_empty() {
            self.matcher.set_config(matcher_config(policy.max_typos));
            self.matcher.set_pattern(text);
        }
        observer.stage_finished(QueryStage::Setup);
        let counts = if text.is_empty() {
            self.rank_blank(scope, now, policy)
        } else {
            match strategy {
                QueryStrategy::Bounded => self.rank_bounded(text, scope, now, policy, observer)?,
                QueryStrategy::Filtered => self.rank_matches(text, scope, now, policy, true)?,
                #[cfg(test)]
                QueryStrategy::Exhaustive => self.rank_matches(text, scope, now, policy, false)?,
                #[cfg(test)]
                QueryStrategy::Hybrid(budget) => {
                    self.rank_hybrid(text, scope, now, policy, observer, budget)?
                }
                #[cfg(test)]
                QueryStrategy::Ranges => self.rank_ranges(text, scope, now, policy, observer)?,
                #[cfg(test)]
                QueryStrategy::BoundReuse(reuse) => {
                    self.rank_reused_bounds(text, scope, now, policy, observer, reuse)?
                }
            }
        };
        observer.stage_finished(QueryStage::HeapAndExactScoring);

        // Partition in linear time and sort only the requested finalists.
        let limit = policy.limit.min(self.ranked.len());
        if limit < self.ranked.len() {
            self.ranked.select_nth_unstable_by(limit, Ranked::compare);
            self.ranked.truncate(limit);
        }
        self.ranked.sort_unstable_by(Ranked::compare);
        observer.stage_finished(QueryStage::FinalistOrdering);
        let hits = self.build_hits(text, strategy.cache_highlights(), observer)?;
        let result = SearchResults {
            generation: self.generation,
            counts,
            hits,
        };
        observer.stage_finished(QueryStage::HighlightsAndOwnedResults);
        Ok(result)
    }

    fn rank_blank(&mut self, scope: Scope, now: i64, policy: SearchPolicy) -> ScopeCounts {
        let mut counts = ScopeCounts::default();
        if policy.blank == BlankQuery::None {
            return counts;
        }
        for (index, entry) in self.entries.iter().enumerate() {
            counts.include(entry.target);
            if scope.includes(entry.target) {
                self.ranked
                    .push(Ranked::new(index, entry, 0, now, policy.boosts));
            }
        }
        counts
    }

    fn rank_matches(
        &mut self,
        text: &str,
        scope: Scope,
        now: i64,
        policy: SearchPolicy,
        use_masks: bool,
    ) -> Result<ScopeCounts, SearchError> {
        // Reject only impossible fields, then restore original indices before merging matches.
        let query = AsciiPresence::new(text);
        let mut batch = FieldBatch::new(self.entries.len());
        batch.select(
            &self.entries,
            &self.field_masks,
            TextField::Title,
            &query,
            policy.max_typos,
            use_masks,
        );
        let titles = batch.matches(&mut self.matcher)?;
        batch.select(
            &self.entries,
            &self.field_masks,
            TextField::Address,
            &query,
            policy.max_typos,
            use_masks,
        );
        let addresses = batch.matches(&mut self.matcher)?;
        let mut matches = MergedMatches::new(&titles, &addresses);
        let mut counts = ScopeCounts::default();
        while let Some((index, score)) = matches.next() {
            let entry = self
                .entries
                .get(index)
                .ok_or(SearchError::InvalidMatchIndex)?;
            counts.include(entry.target);
            if scope.includes(entry.target) {
                self.ranked
                    .push(Ranked::new(index, entry, score, now, policy.boosts));
            }
        }
        Ok(counts)
    }

    fn rank_bounded<Observer: QueryObserver>(
        &mut self,
        text: &str,
        scope: Scope,
        now: i64,
        policy: SearchPolicy,
        observer: &mut Observer,
    ) -> Result<ScopeCounts, SearchError> {
        let (counts, candidates) = self.bound_candidates(text, scope, now, policy)?;
        let ranking = CandidateRanking {
            counts,
            candidates,
            selected: BinaryHeap::new(),
            scores: QueryScoreCache::with_groups(text, self.groups.slots()),
            direct_count: 0,
        };
        self.rank_from_candidates(ranking, now, policy, observer)
    }

    fn rank_from_candidates<Observer: QueryObserver>(
        &mut self,
        mut ranking: CandidateRanking,
        now: i64,
        policy: SearchPolicy,
        observer: &mut Observer,
    ) -> Result<ScopeCounts, SearchError> {
        observer.candidates(ranking.candidates.len() + ranking.direct_count);
        observer.stage_finished(QueryStage::MembershipAndCandidates);
        let mut pending = BinaryHeap::from(ranking.candidates);

        // Heap construction is linear; stop only when every remaining bound loses to the cutoff.
        while let Some(Reverse(candidate)) = pending.pop() {
            if ranking.selected.len() == policy.limit
                && ranking
                    .selected
                    .peek()
                    .is_some_and(|worst| Ranked::compare(&candidate, worst) != Ordering::Less)
            {
                break;
            }
            let ranked =
                self.score_candidate(candidate.index, now, policy.boosts, &mut ranking.scores)?;
            observer.candidate_scored();
            retain_best(&mut ranking.selected, ranked, policy.limit);
        }
        let (computed, reused) = ranking.scores.stats();
        observer.field_scores(computed, reused);
        self.ranked.extend(ranking.selected);
        Ok(ranking.counts)
    }

    fn bound_candidates(
        &self,
        text: &str,
        scope: Scope,
        now: i64,
        policy: SearchPolicy,
    ) -> Result<(ScopeCounts, Vec<Reverse<Ranked>>), SearchError> {
        let mut membership = GroupedMembership::new(text, policy.max_typos, &self.groups);
        let bound = QueryBound::new(text);
        let mut counts = ScopeCounts::default();
        let mut candidates = Vec::new();

        // Count the title/address union in every scope, including rows outside the display limit.
        for (index, (entry, masks)) in self.entries.iter().zip(&self.field_masks).enumerate() {
            if !membership.entry_matches_counted(
                entry,
                masks,
                self.field_groups[index],
                &self.digit_counts[index],
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

    fn score_candidate(
        &mut self,
        index: usize,
        now: i64,
        weights: BoostWeights,
        scores: &mut QueryScoreCache,
    ) -> Result<Ranked, SearchError> {
        let entry = self
            .entries
            .get(index)
            .ok_or(SearchError::InvalidMatchIndex)?;
        let groups = self
            .field_groups
            .get(index)
            .ok_or(SearchError::InvalidMatchIndex)?;
        let base = score_fields(entry, *groups, &mut self.matcher, scores)?;
        Ok(Ranked::new(index, entry, base, now, weights))
    }

    fn build_hits<Observer: QueryObserver>(
        &mut self,
        text: &str,
        cache_highlights: bool,
        observer: &mut Observer,
    ) -> Result<Vec<SearchHit>, SearchError> {
        let mut hits = Vec::with_capacity(self.ranked.len());
        let mut fallback_matcher = None;
        let mut highlights =
            cache_highlights.then(|| HighlightCache::new(text, self.groups.slots()));
        for ranked in &self.ranked {
            let entry = self
                .entries
                .get(ranked.index)
                .ok_or(SearchError::InvalidMatchIndex)?;
            let (title_matches, address_matches) = if text.is_empty() {
                (Vec::new(), Vec::new())
            } else if let Some(highlights) = &mut highlights {
                let groups = self
                    .field_groups
                    .get(ranked.index)
                    .ok_or(SearchError::InvalidMatchIndex)?;
                highlights.match_ranges(&mut self.matcher, entry, *groups)?
            } else {
                match_ranges(&mut self.matcher, &mut fallback_matcher, text, entry)?
            };
            hits.push(SearchHit {
                target: entry.target,
                score: ranked.score,
                title: entry.title.clone(),
                address: entry.address.clone(),
                title_matches,
                address_matches,
            });
        }
        if let Some(highlights) = highlights {
            let (computed, reused) = highlights.stats();
            observer.highlight_traces(computed, reused);
        }
        Ok(hits)
    }
}

fn score_fields(
    entry: &Entry,
    groups: [Option<GroupId>; 2],
    matcher: &mut Matcher,
    scores: &mut QueryScoreCache,
) -> Result<u16, SearchError> {
    let title = scores.score_grouped(matcher, entry.title.as_str(), groups[0])?;
    let address = scores.score_grouped(matcher, entry.address.as_str(), groups[1])?;
    match (title, address) {
        (Some(title), Some(address)) => Ok(title.max(address)),
        (Some(found), None) | (None, Some(found)) => Ok(found),
        (None, None) => Err(SearchError::InconsistentMatch),
    }
}

struct FieldBatch<'a> {
    fields: Vec<&'a str>,
    indices: Vec<u32>,
}

impl<'a> FieldBatch<'a> {
    fn new(capacity: usize) -> Self {
        Self {
            fields: Vec::with_capacity(capacity),
            indices: Vec::with_capacity(capacity),
        }
    }

    fn select(
        &mut self,
        entries: &'a [Entry],
        masks: &[FieldMasks],
        field: TextField,
        query: &AsciiPresence,
        max_typos: u16,
        use_masks: bool,
    ) {
        self.fields.clear();
        self.indices.clear();
        for (index, (entry, masks)) in entries.iter().zip(masks).enumerate() {
            if use_masks && !masks.may_match(field, query, max_typos) {
                continue;
            }
            let text = match field {
                TextField::Title => &entry.title,
                TextField::Address => &entry.address,
            };
            self.fields.push(text.as_str());
            self.indices.push(index as u32);
        }
    }

    fn matches(&self, matcher: &mut Matcher) -> Result<Vec<Match>, SearchError> {
        let mut matches = matcher.match_list(&self.fields);
        for found in &mut matches {
            found.index = *self
                .indices
                .get(found.index as usize)
                .ok_or(SearchError::InvalidMatchIndex)?;
        }
        Ok(matches)
    }
}

fn matcher_config(max_typos: u16) -> Config {
    Config::default()
        .max_typos(Some(max_typos))
        .sort(SortStrategy::IndexAsc)
}

fn retain_best(selected: &mut BinaryHeap<Ranked>, ranked: Ranked, limit: usize) {
    if selected.len() < limit {
        selected.push(ranked);
        return;
    }
    if let Some(mut worst) = selected.peek_mut()
        && Ranked::compare(&ranked, &worst) == Ordering::Less
    {
        *worst = ranked;
    }
}

fn validate_entry(entry: &Entry) -> Result<(), SearchError> {
    validate_field_len(entry.title.len(), TextField::Title)?;
    validate_field_len(entry.address.len(), TextField::Address)
}

fn validate_field_len(len: usize, field: TextField) -> Result<(), SearchError> {
    if len > u32::MAX as usize {
        return Err(SearchError::FieldTooLong(field));
    }
    Ok(())
}

fn next_generation(generation: u64) -> Result<u64, SearchError> {
    generation
        .checked_add(1)
        .ok_or(SearchError::GenerationExhausted)
}

type FieldRanges = (Vec<std::ops::Range<usize>>, Vec<std::ops::Range<usize>>);

fn match_ranges(
    matcher: &mut Matcher,
    fallback: &mut Option<Matcher>,
    query: &str,
    entry: &Entry,
) -> Result<FieldRanges, SearchError> {
    let title = matcher.match_one_indices(entry.title.as_str(), 0);
    let address = matcher.match_one_indices(entry.address.as_str(), 0);
    if title.is_none() && address.is_none() {
        return Err(SearchError::InconsistentMatch);
    }
    let title_ranges = field_ranges(title, &entry.title, query, fallback)?;
    let address_ranges = field_ranges(address, &entry.address, query, fallback)?;
    Ok((title_ranges, address_ranges))
}

fn field_ranges(
    found: Option<MatchIndices>,
    text: &str,
    query: &str,
    fallback: &mut Option<Matcher>,
) -> Result<Vec<std::ops::Range<usize>>, SearchError> {
    let Some(indices) = highlight_cache::finalized_indices(found, text, query, fallback) else {
        return Ok(Vec::new());
    };
    highlights::ranges(text, &indices)
}

type RankKey = (Reverse<u32>, Target);

#[derive(Eq, PartialEq)]
struct Ranked {
    index: usize,
    target: Target,
    score: u32,
}

impl Ord for Ranked {
    fn cmp(&self, other: &Self) -> Ordering {
        Self::compare(self, other).then_with(|| self.index.cmp(&other.index))
    }
}

impl PartialOrd for Ranked {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ranked {
    fn new(index: usize, entry: &Entry, base: u16, now: i64, weights: BoostWeights) -> Self {
        Self {
            index,
            target: entry.target,
            score: u32::from(base) + boost(entry, now, weights),
        }
    }

    fn compare(left: &Self, right: &Self) -> Ordering {
        left.key().cmp(&right.key())
    }

    fn key(&self) -> RankKey {
        (Reverse(self.score), self.target)
    }
}

fn boost(entry: &Entry, now: i64, weights: BoostWeights) -> u32 {
    boost_values(
        entry.open,
        32 - entry.usage.visits.leading_zeros(),
        entry.usage.last_used,
        now,
        weights,
    )
}

fn boost_values(
    open: bool,
    frequency_bits: u32,
    last_used: Option<i64>,
    now: i64,
    weights: BoostWeights,
) -> u32 {
    let open = if open { u32::from(weights.open) } else { 0 };
    // Bit length gives a logarithmic frequency in 0..=32, bounded by the weight.
    let frequency = u32::from(weights.frequency) * frequency_bits / 32;
    let recency = match last_used {
        Some(last_used) => recency_boost(last_used, now, weights),
        None => 0,
    };
    open + frequency + recency
}

fn recency_boost(last_used: i64, now: i64, weights: BoostWeights) -> u32 {
    if weights.recency == 0 {
        return 0;
    }
    if now <= last_used {
        return u32::from(weights.recency);
    }

    // The full positive difference between two i64 timestamps fits in u64.
    let age = now.abs_diff(last_used);
    let window = weights.recency_window_secs.get();
    let remaining = window.saturating_sub(age);
    if remaining == 0 {
        return 0;
    }

    // Ordinary windows fit the narrower product; extreme windows retain exact wide arithmetic.
    match u64::from(weights.recency).checked_mul(remaining) {
        Some(product) => (product / window) as u32,
        None => (u128::from(weights.recency) * u128::from(remaining) / u128::from(window)) as u32,
    }
}

struct MergedMatches<'a> {
    titles: &'a [Match],
    addresses: &'a [Match],
}

impl<'a> MergedMatches<'a> {
    fn new(titles: &'a [Match], addresses: &'a [Match]) -> Self {
        Self { titles, addresses }
    }

    fn next(&mut self) -> Option<(usize, u16)> {
        let result = match (self.titles.first(), self.addresses.first()) {
            (Some(title), Some(address)) if title.index == address.index => {
                let result = (title.index as usize, title.score.max(address.score));
                self.titles = &self.titles[1..];
                self.addresses = &self.addresses[1..];
                result
            }
            (Some(title), Some(address)) if title.index < address.index => self.take_title(),
            (Some(_), Some(_)) | (None, Some(_)) => self.take_address(),
            (Some(_), None) => self.take_title(),
            (None, None) => return None,
        };
        Some(result)
    }

    fn take_title(&mut self) -> (usize, u16) {
        let found = self.titles[0];
        self.titles = &self.titles[1..];
        (found.index as usize, found.score)
    }

    fn take_address(&mut self) -> (usize, u16) {
        let found = self.addresses[0];
        self.addresses = &self.addresses[1..];
        (found.index as usize, found.score)
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod profiling;
