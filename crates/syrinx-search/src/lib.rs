//! Syrinx's local search: the in-memory catalogue (tier 1) and the full-text index (tier 2).
//!
//! The catalogue ranks metadata without I/O; only `fulltext` reads and writes files.

mod candidate;
mod catalogue;
mod digit_counts;
mod field_groups;
pub mod fulltext;
mod highlights;
mod membership;
mod score_cache;
mod types;

pub use catalogue::Catalogue;
pub use types::{
    BlankQuery, BoostWeights, Entry, Scope, ScopeCounts, SearchError, SearchHit, SearchPolicy,
    SearchResults, Target, TextField, Usage, WorkerFallbacks,
};
