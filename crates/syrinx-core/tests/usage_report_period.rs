//! A usage report must never claim its period ended before it began.
use syrinx_core::usage::{ReportPeriod, UsageDate, UsageFile, read_report};

fn date(year: i32, month: u32, day: u32) -> UsageDate {
    UsageDate::from_ymd(year, month, day).expect("synthetic date")
}

#[test]
fn a_clock_set_back_cannot_end_the_current_period_before_its_start() {
    // The period began on the 14th, then the clock was set back to the 13th.
    let file = UsageFile::begin(date(2026, 10, 14));
    let text = file
        .report_text(ReportPeriod::Current, date(2026, 10, 13))
        .expect("current period always reports");
    let table = read_report(&text).expect("own report reads back");
    let start = table.envelope_value("period_start").expect("start");
    let end = table.envelope_value("period_end").expect("end");
    assert!(end >= start, "period_end {end} precedes period_start {start}");
}
