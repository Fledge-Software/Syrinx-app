//! Defines search inputs and results without exposing their text through diagnostics.

use std::{
    fmt,
    num::{NonZeroU32, NonZeroU64},
    ops::Range,
};

use syrinx_core::{SavedPageId, TabId};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Target {
    Tab(TabId),
    Saved(SavedPageId),
    History(NonZeroU64),
    Command(NonZeroU64),
    /// A plugin search feed's row (#141); the application maps both keys back to the plugin
    /// id, feed name and entry id it interned them for.
    Plugin {
        feed: FeedKey,
        entry: EntryKey,
    },
}

// Every catalogue row carries a target, so a plugin row must not make each one larger.
const _: () = assert!(std::mem::size_of::<Target>() <= 16);

/// One plugin search feed, interned by the application: it stands for a plugin id and the
/// name of one feed that plugin declares.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FeedKey(NonZeroU32);

impl FeedKey {
    pub const fn new(key: NonZeroU32) -> Self {
        Self(key)
    }

    pub const fn get(self) -> NonZeroU32 {
        self.0
    }
}

/// One entry of a plugin search feed, interned by the application within its feed.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EntryKey(NonZeroU32);

impl EntryKey {
    pub const fn new(key: NonZeroU32) -> Self {
        Self(key)
    }

    pub const fn get(self) -> NonZeroU32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Scope {
    Everything,
    Tabs,
    Saved,
    History,
    Commands,
}

impl Scope {
    /// A plugin row is found under `Everything` only; its own chip is the owner's call (#265).
    pub(crate) fn includes(self, target: Target) -> bool {
        matches!(self, Self::Everything)
            || matches!(
                (self, target),
                (Self::Tabs, Target::Tab(_))
                    | (Self::Saved, Target::Saved(_))
                    | (Self::History, Target::History(_))
                    | (Self::Commands, Target::Command(_))
            )
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct Usage {
    pub visits: u32,
    /// Unix seconds supplied by the owning model or history feed.
    pub last_used: Option<i64>,
}

impl fmt::Debug for Usage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Usage").finish_non_exhaustive()
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct Entry {
    pub target: Target,
    pub title: String,
    pub address: String,
    pub usage: Usage,
    pub open: bool,
}

impl fmt::Debug for Entry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Entry").finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlankQuery {
    All,
    None,
}

/// Weights are independent, bounded additions to a field's fuzzy score.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoostWeights {
    pub open: u16,
    pub frequency: u16,
    pub recency: u16,
    pub recency_window_secs: NonZeroU64,
}

/// UI defaults are intentionally absent; each caller supplies its policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SearchPolicy {
    pub limit: usize,
    pub max_typos: u16,
    /// Applies to an empty string only; whitespace and operators stay literal.
    pub blank: BlankQuery,
    pub boosts: BoostWeights,
}

/// Records partition workers the operating system refused to start.
/// Their rows were searched on the caller, so results stay complete; only parallelism was lost.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WorkerFallbacks {
    pub refused: u64,
    pub last_error: Option<std::io::ErrorKind>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ScopeCounts {
    pub tabs: usize,
    pub saved: usize,
    pub history: usize,
    pub commands: usize,
    /// Plugin search feed rows (#141), which only `Scope::Everything` returns.
    pub plugins: usize,
}

impl ScopeCounts {
    pub fn total(self) -> usize {
        self.tabs
            .saturating_add(self.saved)
            .saturating_add(self.history)
            .saturating_add(self.commands)
            .saturating_add(self.plugins)
    }

    pub(crate) fn include(&mut self, target: Target) {
        match target {
            Target::Tab(_) => self.tabs += 1,
            Target::Saved(_) => self.saved += 1,
            Target::History(_) => self.history += 1,
            Target::Command(_) => self.commands += 1,
            Target::Plugin { .. } => self.plugins += 1,
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct SearchHit {
    pub target: Target,
    pub score: u32,
    pub title: String,
    pub address: String,
    /// Ascending, disjoint UTF-8 byte ranges into the original title.
    pub title_matches: Vec<Range<usize>>,
    /// Ascending, disjoint UTF-8 byte ranges into the original address.
    pub address_matches: Vec<Range<usize>>,
}

impl fmt::Debug for SearchHit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("SearchHit").finish_non_exhaustive()
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct SearchResults {
    pub generation: u64,
    pub counts: ScopeCounts,
    pub hits: Vec<SearchHit>,
}

impl fmt::Debug for SearchResults {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SearchResults")
            .field("generation", &self.generation)
            .field("counts", &self.counts)
            .field("hit_count", &self.hits.len())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextField {
    Title,
    Address,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchError {
    FieldTooLong(TextField),
    QueryTooLong,
    TooManyEntries,
    GenerationExhausted,
    InvalidMatchIndex,
    InvalidMatchRange,
    InconsistentMatch,
}

impl fmt::Display for SearchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::FieldTooLong(_) => "catalogue field exceeds the matcher index limit",
            Self::QueryTooLong => "query exceeds the matcher scoring limit",
            Self::TooManyEntries => "catalogue exceeds the matcher index limit",
            Self::GenerationExhausted => "catalogue generation exhausted",
            Self::InvalidMatchIndex => "matcher returned an invalid catalogue index",
            Self::InvalidMatchRange => "matcher returned an invalid text index",
            Self::InconsistentMatch => "matcher scoring and highlighting disagree",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for SearchError {}
