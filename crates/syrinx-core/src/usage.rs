//! Local usage counting (D39, AN-01/02): a fixed schema of counters, kept only while switched on.
//!
//! The approved design is `docs/proposals/an-01-02-local-analytics.md`; what was built is in
//! `docs/an-01-local-analytics.md` and `docs/an-02-report-export.md`.

mod codec;
mod period;
mod recorder;
mod report;
mod schema;

#[cfg(test)]
mod tests;

pub use codec::{ABOUT, MAX_FILE_BYTES, UsageFileError, read_report, report};
pub use period::{PERIOD_DAYS, Period, UsageDate, UsageFile};
pub use recorder::{Counts, UsageRecorder};
pub use report::{ReportGroup, ReportPeriod, ReportRow, ReportTable, ReportValue};
pub use schema::{COMMAND_KEYS, ErrorCategory, Feature, SCHEMA_VERSION, SLOT_COUNT, Scale, Timing};
