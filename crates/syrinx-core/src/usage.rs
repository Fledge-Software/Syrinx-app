//! Local usage counting (D39, AN-01): a fixed schema of counters, kept only while switched on.
//!
//! The approved design is `docs/proposals/an-01-02-local-analytics.md`; what was built is in
//! `docs/an-01-local-analytics.md`.

mod codec;
mod period;
mod recorder;
mod schema;

#[cfg(test)]
mod tests;

pub use codec::{ABOUT, MAX_FILE_BYTES, UsageFileError, report};
pub use period::{PERIOD_DAYS, Period, UsageDate, UsageFile};
pub use recorder::{Counts, UsageRecorder};
pub use schema::{COMMAND_KEYS, ErrorCategory, Feature, SCHEMA_VERSION, SLOT_COUNT, Scale, Timing};
