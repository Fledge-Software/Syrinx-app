//! Maintains a bounded experimental range index without retaining query answers.

use std::{cmp::Reverse, collections::BTreeMap, time::Duration};

use crate::{Entry, Target, field_groups::GroupId};

mod ranking;
mod tests;

type PairKey = [GroupId; 2];
type FieldIds = [Option<GroupId>; 2];

#[derive(Clone, Copy)]
struct Limits {
    members: usize,
    fallback: usize,
    pairs: usize,
    buckets: usize,
    frontier: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            members: 131_072,
            fallback: 16_384,
            pairs: 2_048,
            buckets: 16_384,
            frontier: 32_768,
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
struct Class {
    kind: usize,
    open: bool,
    frequency: u32,
}

impl Class {
    fn new(entry: &Entry) -> Self {
        Self {
            kind: match entry.target {
                Target::Tab(_) => 0,
                Target::Saved(_) => 1,
                Target::History(_) => 2,
                Target::Command(_) => 3,
            },
            open: entry.open,
            frequency: u32::BITS - entry.usage.visits.leading_zeros(),
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct Member {
    target: Target,
    last_used: Option<i64>,
}

impl Member {
    fn new(entry: &Entry) -> Self {
        Self {
            target: entry.target,
            last_used: entry.usage.last_used,
        }
    }

    fn key(self) -> (Reverse<Option<i64>>, Target) {
        (Reverse(self.last_used), self.target)
    }
}

#[derive(Default)]
struct Bucket {
    members: Vec<Member>,
    minimum: Vec<u32>,
}

impl Bucket {
    fn finalize(&mut self) {
        self.members.sort_unstable_by_key(|member| member.key());
        self.trim_members();
        self.rebuild_tree();
    }

    fn rebuild_tree(&mut self) {
        let leaves = self.members.len().next_power_of_two();
        self.minimum.clear();
        self.minimum.resize(2 * leaves, u32::MAX);
        for index in 0..self.members.len() {
            self.minimum[leaves + index] = index as u32;
        }
        for node in (1..leaves).rev() {
            self.minimum[node] =
                self.minimum_of(self.minimum[2 * node], self.minimum[2 * node + 1]);
        }
        if self.minimum.capacity() > 2 * self.minimum.len() {
            self.minimum.shrink_to_fit();
        }
    }

    fn minimum_of(&self, left: u32, right: u32) -> u32 {
        match (
            self.members.get(left as usize),
            self.members.get(right as usize),
        ) {
            (Some(left_member), Some(right_member)) => {
                if left_member.target <= right_member.target {
                    left
                } else {
                    right
                }
            }
            (Some(_), None) => left,
            (None, Some(_)) => right,
            (None, None) => u32::MAX,
        }
    }

    fn insert(&mut self, entry: &Entry) {
        let member = Member::new(entry);
        let position = self
            .members
            .binary_search_by_key(&member.key(), |member| member.key())
            .expect_err("new range member is absent");
        self.members.insert(position, member);
        self.trim_members();
        self.rebuild_tree();
    }

    fn remove(&mut self, entry: &Entry) {
        let position = self
            .members
            .binary_search_by_key(&Member::new(entry).key(), |member| member.key())
            .expect("old range member is present");
        self.members.remove(position);
        self.trim_members();
        if !self.members.is_empty() {
            self.rebuild_tree();
        }
    }

    fn update_in_place(&mut self, old: &Entry, new: &Entry) -> bool {
        let position = self
            .members
            .binary_search_by_key(&Member::new(old).key(), |member| member.key())
            .expect("old range member is present");
        let member = Member::new(new);
        if position > 0 && self.members[position - 1].key() >= member.key() {
            return false;
        }
        if self
            .members
            .get(position + 1)
            .is_some_and(|next| member.key() >= next.key())
        {
            return false;
        }
        // Every target stays at its existing position, preserving the minimum-target tree.
        self.members[position] = member;
        true
    }

    fn trim_members(&mut self) {
        if self.members.capacity() > 2 * self.members.len() {
            self.members.shrink_to_fit();
        }
    }

    fn representative(&self) -> Target {
        self.members[0].target
    }
}

#[derive(Default)]
struct Pair {
    counts: [usize; 4],
    buckets: BTreeMap<Class, Bucket>,
}

impl Pair {
    fn representative(&self) -> Target {
        self.buckets
            .first_key_value()
            .expect("live pair has a bucket")
            .1
            .representative()
    }
}

#[derive(Default)]
struct State {
    pairs: BTreeMap<PairKey, Pair>,
    fallback: Vec<Fallback>,
    buckets: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Fallback {
    target: Target,
    position: usize,
}

impl State {
    fn insert(
        &mut self,
        entry: &Entry,
        ids: FieldIds,
        row: usize,
        limits: Limits,
        bulk: bool,
    ) -> bool {
        let Some(key) = pair_key(ids) else {
            if self.fallback.len() >= limits.fallback {
                return false;
            }
            let record = Fallback {
                target: entry.target,
                position: row,
            };
            if bulk {
                self.fallback.push(record);
            } else {
                let position = self
                    .fallback
                    .binary_search_by_key(&row, |record| record.position)
                    .expect_err("fallback position absent");
                self.fallback.insert(position, record);
            }
            return true;
        };
        let class = Class::new(entry);
        let new_pair = !self.pairs.contains_key(&key);
        let new_bucket = self
            .pairs
            .get(&key)
            .is_none_or(|pair| !pair.buckets.contains_key(&class));
        if (new_pair && self.pairs.len() >= limits.pairs)
            || (new_bucket && self.buckets >= limits.buckets)
        {
            return false;
        }
        let pair = self.pairs.entry(key).or_default();
        let bucket = pair.buckets.entry(class).or_default();
        if bulk {
            bucket.members.push(Member::new(entry));
        } else {
            bucket.insert(entry);
        }
        pair.counts[class.kind] += 1;
        self.buckets += usize::from(new_bucket);
        true
    }

    fn remove(&mut self, entry: &Entry, ids: FieldIds, row: usize) {
        let Some(key) = pair_key(ids) else {
            let position = self
                .fallback
                .binary_search_by_key(&row, |record| record.position)
                .expect("fallback position present");
            let removed = self.fallback.remove(position);
            assert_eq!(removed.target, entry.target);
            return;
        };
        let class = Class::new(entry);
        let pair = self.pairs.get_mut(&key).expect("old pair present");
        let bucket = pair.buckets.get_mut(&class).expect("old bucket present");
        bucket.remove(entry);
        pair.counts[class.kind] -= 1;
        if bucket.members.is_empty() {
            pair.buckets.remove(&class);
            self.buckets -= 1;
        }
        if pair.buckets.is_empty() {
            self.pairs.remove(&key);
        }
    }

    fn build(entries: &[Entry], ids: &[FieldIds], limits: Limits) -> Option<Self> {
        let mut state = Self::default();
        for (position, (entry, ids)) in entries.iter().zip(ids).enumerate() {
            if !state.insert(entry, *ids, position, limits, true) {
                return None;
            }
        }
        for pair in state.pairs.values_mut() {
            for bucket in pair.buckets.values_mut() {
                bucket.finalize();
            }
        }
        Some(state)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Route {
    #[default]
    NotAttempted,
    BlankQuery,
    Unavailable,
    DigitQuery,
    UnsupportedBudget,
    Ranges,
    FrontierAdmission,
    FrontierExpansion,
    Error,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct QueryReport {
    route: Route,
    available: bool,
    attempted: bool,
    work_recorded: bool,
    indexed_rows: usize,
    fallback_rows: usize,
    pairs_examined: usize,
    fallback_examined: usize,
    matching_rows: usize,
    candidate_rows: usize,
    frontier_peak: usize,
    frontier_pops: usize,
    pairs_scored: usize,
    range_leaves_ranked: usize,
    fallback_rows_scored: usize,
    selected_rows: usize,
    metadata_bounds_enabled: bool,
    bound_constructions: usize,
    bound_position_lookups: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct InPlaceCounters {
    attempted: usize,
    grouped: usize,
    fallback: usize,
    declined: usize,
}

#[derive(Clone, Copy, Debug)]
struct PhaseTimings {
    candidates: Duration,
    expansion: Duration,
}

pub(super) struct RangeIndex {
    state: Option<State>,
    limits: Limits,
    grouped: usize,
    fallback: usize,
    was_within: bool,
    retry_mutations: usize,
    previous: Option<(Option<PairKey>, Class)>,
    last_query: QueryReport,
    in_place_enabled: bool,
    in_place_counters: InPlaceCounters,
    metadata_bounds_enabled: bool,
    phase_timing_enabled: bool,
    last_phases: Option<PhaseTimings>,
}

impl RangeIndex {
    pub(super) fn build(entries: &[Entry], ids: &[FieldIds]) -> Self {
        Self::with_limits(entries, ids, Limits::default())
    }

    fn with_limits(entries: &[Entry], ids: &[FieldIds], limits: Limits) -> Self {
        assert_eq!(entries.len(), ids.len());
        let grouped = ids.iter().filter(|ids| pair_key(**ids).is_some()).count();
        let fallback = entries.len() - grouped;
        let within = grouped <= limits.members && fallback <= limits.fallback;
        Self {
            state: if within {
                State::build(entries, ids, limits)
            } else {
                None
            },
            limits,
            grouped,
            fallback,
            was_within: within,
            retry_mutations: 0,
            previous: None,
            last_query: QueryReport::default(),
            in_place_enabled: true,
            in_place_counters: InPlaceCounters::default(),
            metadata_bounds_enabled: true,
            phase_timing_enabled: false,
            last_phases: None,
        }
    }

    fn set_in_place_enabled(&mut self, enabled: bool) {
        self.in_place_enabled = enabled;
    }

    fn in_place_counters(&self) -> InPlaceCounters {
        self.in_place_counters
    }

    fn take_in_place_counters(&mut self) -> InPlaceCounters {
        std::mem::take(&mut self.in_place_counters)
    }

    fn set_metadata_bounds_enabled(&mut self, enabled: bool) {
        self.metadata_bounds_enabled = enabled;
    }

    fn set_phase_timing_enabled(&mut self, enabled: bool) {
        self.phase_timing_enabled = enabled;
        self.last_phases = None;
    }

    pub(super) fn try_update_in_place(
        &mut self,
        old: &Entry,
        new: &Entry,
        ids: FieldIds,
        position: usize,
    ) -> bool {
        assert!(
            self.previous.is_none(),
            "in-place update precedes detachment"
        );
        self.in_place_counters.attempted += 1;
        let updated = self.update_in_place(old, new, ids, position);
        if !updated {
            self.in_place_counters.declined += 1;
        } else if pair_key(ids).is_some() {
            self.in_place_counters.grouped += 1;
        } else {
            self.in_place_counters.fallback += 1;
        }
        updated
    }

    fn update_in_place(
        &mut self,
        old: &Entry,
        new: &Entry,
        ids: FieldIds,
        position: usize,
    ) -> bool {
        if !self.in_place_enabled
            || old.target != new.target
            || old.title != new.title
            || old.address != new.address
            || Class::new(old) != Class::new(new)
        {
            return false;
        }
        let Some(state) = &mut self.state else {
            return false;
        };
        let Some(key) = pair_key(ids) else {
            let slot = state
                .fallback
                .binary_search_by_key(&position, |record| record.position)
                .expect("existing fallback position");
            assert_eq!(state.fallback[slot].target, old.target);
            return true;
        };
        state
            .pairs
            .get_mut(&key)
            .expect("old pair present")
            .buckets
            .get_mut(&Class::new(old))
            .expect("old bucket present")
            .update_in_place(old, new)
    }

    pub(super) fn before_remove(&mut self, entry: &Entry, ids: FieldIds, position: usize) {
        assert!(self.previous.is_none(), "one mutation is pending");
        self.previous = Some((pair_key(ids), Class::new(entry)));
        if pair_key(ids).is_some() {
            self.grouped -= 1;
        } else {
            self.fallback -= 1;
        }
        if let Some(state) = &mut self.state {
            state.remove(entry, ids, position);
        }
    }

    pub(super) fn relocated(&mut self, target: Target, from: usize, to: usize) {
        let Some(state) = &mut self.state else {
            return;
        };
        let Ok(slot) = state
            .fallback
            .binary_search_by_key(&from, |record| record.position)
        else {
            return;
        };
        let mut record = state.fallback.remove(slot);
        assert_eq!(record.target, target);
        record.position = to;
        let destination = state
            .fallback
            .binary_search_by_key(&to, |record| record.position)
            .expect_err("removed row leaves its position vacant");
        state.fallback.insert(destination, record);
    }

    fn begin_query(&mut self, route: Route) {
        self.last_phases = None;
        self.last_query = QueryReport {
            route,
            available: self.state.is_some(),
            indexed_rows: self.grouped,
            fallback_rows: self.fallback,
            metadata_bounds_enabled: self.metadata_bounds_enabled,
            ..QueryReport::default()
        };
    }

    pub(super) fn prepare_query(&mut self, blank: bool) {
        self.begin_query(if blank {
            Route::BlankQuery
        } else {
            Route::NotAttempted
        });
    }

    pub(super) fn after_change(
        &mut self,
        new: Option<(usize, &Entry, FieldIds)>,
        entries: &[Entry],
        ids: &[FieldIds],
    ) {
        let previous = self.previous.take();
        let current = new.map(|(_, entry, ids)| (pair_key(ids), Class::new(entry)));
        let structural = previous != current;
        if let Some((_, _, ids)) = new {
            if pair_key(ids).is_some() {
                self.grouped += 1;
            } else {
                self.fallback += 1;
            }
        }
        assert_eq!(self.grouped + self.fallback, entries.len());
        let within = self.grouped <= self.limits.members && self.fallback <= self.limits.fallback;
        let was_available = self.state.is_some();
        if !within {
            self.state = None;
        } else if let (Some(state), Some((position, entry, ids))) = (&mut self.state, new)
            && !state.insert(entry, ids, position, self.limits, false)
        {
            self.state = None;
        }
        // Rebuild only from complete post-mutation rows, never during a query.
        if self.state.is_none() && !was_available && within && structural {
            self.retry_mutations += 1;
            if !self.was_within || self.retry_mutations >= (entries.len() / 16).max(1) {
                self.state = State::build(entries, ids, self.limits);
                self.retry_mutations = 0;
            }
        } else if self.state.is_some() || was_available {
            self.retry_mutations = 0;
        }
        self.was_within = within;
    }
}

fn pair_key(ids: FieldIds) -> Option<PairKey> {
    let [Some(title), Some(address)] = ids else {
        return None;
    };
    Some([title, address])
}
