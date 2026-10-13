//! Ranks local catalogue metadata without owning tab state or performing I/O.

mod candidate;
mod catalogue;
mod digit_counts;
mod field_groups;
mod highlights;
mod membership;
mod score_cache;
mod types;

pub use catalogue::Catalogue;
pub use types::{
    BlankQuery, BoostWeights, Entry, Scope, ScopeCounts, SearchError, SearchHit, SearchPolicy,
    SearchResults, Target, TextField, Usage, WorkerFallbacks,
};
