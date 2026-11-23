//! Notices a feed going round: a plugin that keeps saying "more" while sending cursors it has
//! already sent would otherwise be pulled for as long as Syrinx stays open (#141).

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use super::worker::IDLE_BOUND;
use super::{FeedError, FeedName};

/// How long a stalled feed's announcements are dropped. A plugin that announces its feed
/// again at once, even from inside its own answer, would otherwise restart the loop every
/// few wake-ups; this reuses the idle work's own pace instead of a new tuning number.
const STALL_COOL_DOWN: Duration = IDLE_BOUND;

/// The cursors the pull in flight and its feed's previous budget-spent announcement asked
/// after, and when each feed last stalled this session.
#[derive(Default)]
pub(super) struct CursorHistory {
    /// Every cursor the pull in flight's announcement asked after.
    asked: Vec<String>,
    /// What the feed's previous announcement asked after, when it spent its budget.
    earlier: Vec<String>,
    /// The cursors of each feed waiting after spending its budget, for its next `earlier`.
    carried: BTreeMap<FeedName, Vec<String>>,
    /// When each feed last stalled. Its first stall is shown to the owner and later ones only
    /// logged; for `STALL_COOL_DOWN` after any of them, its announcements are dropped.
    stalled: BTreeMap<FeedName, Instant>,
    /// Whether a reset has already cleared this announcement's cursors.
    reset_cleared: bool,
}

impl CursorHistory {
    /// Starts the history of a new pull of `name` after `after`, with what the feed's previous
    /// announcement carried.
    pub(super) fn start(&mut self, name: &FeedName, after: Option<&str>) {
        self.earlier = self.carried.remove(name).unwrap_or_default();
        self.asked = after.map(str::to_owned).into_iter().collect();
        self.reset_cleared = false;
    }

    /// Whether this pull or the feed's previous announcement already asked after `cursor`.
    pub(super) fn seen(&self, cursor: &str) -> bool {
        let held = |cursors: &[String]| cursors.iter().any(|asked| asked == cursor);
        held(&self.asked) || held(&self.earlier)
    }

    /// Records that the pull asks after `cursor` next.
    pub(super) fn ask(&mut self, cursor: &str) {
        self.asked.push(cursor.to_owned());
    }

    /// The announcement spent its budget at `cursor`: keeps what it asked after for the feed's
    /// next announcement.
    pub(super) fn carry(&mut self, name: &FeedName, cursor: &str) {
        self.ask(cursor);
        self.carried
            .insert(name.clone(), std::mem::take(&mut self.asked));
    }

    /// Forgets the pull's cursors when its batch begins the feed again: always for a pull from
    /// the start, and for the announcement's first reset only. A plugin that starts its feed
    /// again, as a Notes vault switch (#36) would, may resend its offsets; one that resets
    /// every batch still stalls within the announcement.
    pub(super) fn restart(&mut self, from_start: bool, reset: bool) {
        let first_reset = !from_start && reset && !self.reset_cleared;
        if !from_start && !first_reset {
            return;
        }
        self.reset_cleared |= first_reset;
        self.asked.clear();
        self.earlier.clear();
    }

    /// Whether `name` stalled less than `STALL_COOL_DOWN` before `now`, so its announcement is
    /// dropped.
    pub(super) fn cooling(&self, name: &FeedName, now: Instant) -> bool {
        self.stalled
            .get(name)
            .is_some_and(|stalled| now.saturating_duration_since(*stalled) < STALL_COOL_DOWN)
    }

    /// Whether any waiting feed carries cursors into its next announcement.
    #[cfg(test)]
    pub(super) fn carries_any(&self) -> bool {
        !self.carried.is_empty()
    }

    /// Forgets what a removed plugin's feeds carried, that they stalled, and their cool-down.
    pub(super) fn forget_plugin(&mut self, plugin: &str) {
        self.carried.retain(|name, _| name.plugin != plugin);
        self.stalled.retain(|name, _| name.plugin != plugin);
    }

    /// Stops a feed that went round at `now` and starts its cool-down: the owner is shown its
    /// first stall this session, and the rest are only logged, by plugin id, so a looping
    /// plugin cannot flood the window.
    pub(super) fn report_stall(&mut self, name: FeedName, now: Instant) -> Result<(), FeedError> {
        let plugin = name.plugin.clone();
        if self.stalled.insert(name, now).is_none() {
            return Err(FeedError::Stalled { plugin });
        }
        eprintln!("Syrinx: plugin {plugin}: a search feed went round again and was stopped");
        Ok(())
    }
}
