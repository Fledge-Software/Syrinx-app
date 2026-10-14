//! Counts usage in memory with one relaxed atomic increment per call, only while switched on.

use std::{
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
    time::Duration,
};

use super::schema::{
    COMMAND_BASE, ErrorCategory, Feature, OTHER_COMMAND, SLOT_COUNT, Timing, command_index,
};
use crate::BrowserAction;

/// A copy of every counter, in the schema's slot order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Counts([u32; SLOT_COUNT]);

impl Default for Counts {
    fn default() -> Self {
        Self([0; SLOT_COUNT])
    }
}

impl Counts {
    pub fn feature(&self, feature: Feature) -> u32 {
        self.0[feature.slot()]
    }

    pub fn command(&self, action: BrowserAction) -> u32 {
        self.0[COMMAND_BASE + command_index(action)]
    }

    pub fn other_command(&self) -> u32 {
        self.0[COMMAND_BASE + OTHER_COMMAND]
    }

    /// The histogram's buckets, in the order of its scale's bucket names.
    pub fn timing(&self, timing: Timing) -> &[u32] {
        let first = timing.first_slot();
        &self.0[first..first + timing.scale().bucket_names().len()]
    }

    pub fn error(&self, category: ErrorCategory) -> u32 {
        self.0[category.slot()]
    }

    /// Adds `other` slot by slot, stopping at the maximum instead of wrapping.
    pub fn add(&mut self, other: &Self) {
        for (total, more) in self.0.iter_mut().zip(other.0) {
            *total = total.saturating_add(more);
        }
    }

    pub(crate) fn slots(&self) -> &[u32; SLOT_COUNT] {
        &self.0
    }

    pub(crate) fn from_slots(slots: [u32; SLOT_COUNT]) -> Self {
        Self(slots)
    }
}

/// The in-memory counters every part of Syrinx records into, shared through an `Arc`.
///
/// Recording takes only schema enums and durations, so no call site can pass text:
///
/// ```compile_fail
/// let recorder = syrinx_core::usage::UsageRecorder::new();
/// recorder.count("https://private.example.test/");
/// ```
///
/// ```compile_fail
/// let recorder = syrinx_core::usage::UsageRecorder::new();
/// recorder.error(String::from("C:\\synthetic\\path"));
/// ```
pub struct UsageRecorder {
    enabled: AtomicBool,
    /// Set by any increment since the last `take`, so an unchanged recorder arms no write.
    changed: AtomicBool,
    slots: [AtomicU32; SLOT_COUNT],
}

impl Default for UsageRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl UsageRecorder {
    /// A recorder that counts nothing until the session worker switches it on.
    pub const fn new() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            changed: AtomicBool::new(false),
            slots: [const { AtomicU32::new(0) }; SLOT_COUNT],
        }
    }

    pub fn count(&self, feature: Feature) {
        self.increment(feature.slot());
    }

    pub fn command(&self, action: BrowserAction) {
        self.increment(COMMAND_BASE + command_index(action));
    }

    /// A command that is not built in; its name is never recorded.
    pub fn other_command(&self) {
        self.increment(COMMAND_BASE + OTHER_COMMAND);
    }

    /// Adds one to the bucket `elapsed` falls in; the measurement itself is not kept.
    pub fn time(&self, timing: Timing, elapsed: Duration) {
        self.increment(timing.first_slot() + timing.scale().bucket(elapsed));
    }

    pub fn error(&self, category: ErrorCategory) {
        self.increment(category.slot());
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Whether anything was counted since the last `take`.
    pub fn has_changes(&self) -> bool {
        self.changed.load(Ordering::Relaxed)
    }

    /// Starts counting from zero.
    pub fn switch_on(&self) {
        self.clear();
        self.enabled.store(true, Ordering::Relaxed);
    }

    /// Stops counting and forgets everything not yet taken.
    pub fn switch_off(&self) {
        self.enabled.store(false, Ordering::Relaxed);
        self.clear();
    }

    /// Moves the counts out, leaving zeros; `None` when nothing changed.
    pub fn take(&self) -> Option<Counts> {
        if !self.changed.swap(false, Ordering::Relaxed) {
            return None;
        }
        let mut counts = [0; SLOT_COUNT];
        for (count, slot) in counts.iter_mut().zip(&self.slots) {
            *count = slot.swap(0, Ordering::Relaxed);
        }
        Some(Counts(counts))
    }

    /// Sets one counter directly, so saturation can be tested without four billion calls.
    #[cfg(test)]
    pub(super) fn preset(&self, feature: Feature, count: u32) {
        self.slots[feature.slot()].store(count, Ordering::Relaxed);
    }

    fn clear(&self) {
        for slot in &self.slots {
            slot.store(0, Ordering::Relaxed);
        }
        self.changed.store(false, Ordering::Relaxed);
    }

    /// When off, returns at once and writes nothing; a full counter stays full.
    fn increment(&self, slot: usize) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }
        let added = self.slots[slot]
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
                count.checked_add(1)
            })
            .is_ok();
        if added {
            self.changed.store(true, Ordering::Relaxed);
        }
    }
}
