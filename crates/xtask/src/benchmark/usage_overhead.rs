//! Checks AN-01's overhead budget: one usage update costs under 100 ns at the 99th percentile.

use std::fs;
use std::hint::black_box;
use std::io::{self, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::json;
use syrinx_core::usage::{Feature, PERIOD_DAYS, Timing, UsageDate, UsageFile, UsageRecorder};

/// Updates per timed batch; a single update is shorter than the clock's own cost.
const BATCH: u32 = 16;
const WARMUP_BATCHES: usize = 10_000;
const BATCHES: usize = 200_000;
/// The proposal's budget for one update on the calling thread.
const BUDGET_NS: f64 = 100.0;
/// The proposal's budget for loading the usage file at start-up, at the 95th percentile.
const LOAD_BUDGET_NS: f64 = 1_000_000.0;
const LOADS: usize = 2_000;

/// Prints the measurement and returns whether every case met the budget.
pub fn run() -> Result<bool, String> {
    let recorder = UsageRecorder::new();
    recorder.switch_on();
    let counter = measure(|| recorder.count(black_box(Feature::TabCreated)));
    let timing = measure(|| {
        recorder.time(
            black_box(Timing::SearchTier1),
            black_box(Duration::from_micros(900)),
        );
    });
    recorder.switch_off();
    let off = measure(|| recorder.count(black_box(Feature::TabCreated)));

    let cases = [
        ("counter", counter),
        ("timing", timing),
        ("switched_off", off),
    ];
    let load_p95_ns = measure_file_load()?;
    let passed = cases.iter().all(|(_, p99)| *p99 < BUDGET_NS) && load_p95_ns < LOAD_BUDGET_NS;
    let report = json!({
        "benchmark": "usage-recorder",
        "method": format!(
            "p99 over samples that are each the mean of {BATCH} consecutive updates, \
             {BATCHES} samples; a single update is shorter than the clock's own cost"
        ),
        "budget_ns_p99": BUDGET_NS,
        "p99_ns": cases.iter().map(|(name, p99)| json!({ "case": name, "p99_ns": p99 })).collect::<Vec<_>>(),
        "file_load": {
            "method": format!(
                "read, decode and apply one start to the largest version 1 file, {LOADS} runs"
            ),
            "budget_ns_p95": LOAD_BUDGET_NS,
            "p95_ns": load_p95_ns,
        },
        "passed": passed,
    });
    let mut output = io::stdout().lock();
    serde_json::to_writer_pretty(&mut output, &report)
        .map_err(|error| format!("Could not write the usage benchmark report: {error}"))?;
    writeln!(output)
        .map_err(|error| format!("Could not finish the usage benchmark report: {error}"))?;
    Ok(passed)
}

/// The 95th percentile, in nanoseconds, of loading the largest possible usage file at start-up.
fn measure_file_load() -> Result<f64, String> {
    let path = std::env::temp_dir().join(format!("syrinx-usage-bench-{}.json", std::process::id()));
    fs::write(&path, largest_file()?)
        .map_err(|error| format!("Could not write the synthetic usage file: {error}"))?;
    let measured = time_loads(&path);
    let removed = fs::remove_file(&path)
        .map_err(|error| format!("Could not remove the synthetic usage file: {error}"));
    let mut samples = measured?;
    removed?;
    samples.sort_by(f64::total_cmp);
    Ok(samples[LOADS * 95 / 100])
}

/// Both periods with every day used and every counter at its maximum: the most text a
/// version 1 file can hold.
fn largest_file() -> Result<String, String> {
    let start = UsageDate::from_ymd(2026, 1, 1).ok_or("Invalid synthetic date.")?;
    let mut file = UsageFile::begin(start);
    // Day 30 closes the first period; every other day is counted in its period.
    for day in 1..2 * PERIOD_DAYS {
        file.start_day(start.plus_days(day));
    }
    Ok(file.encode().replace(": 0", ": 4294967295"))
}

fn time_loads(path: &Path) -> Result<Vec<f64>, String> {
    let today = UsageDate::from_ymd(2026, 2, 5).ok_or("Invalid synthetic date.")?;
    (0..LOADS)
        .map(|_| {
            let started = Instant::now();
            let bytes = fs::read(path)
                .map_err(|error| format!("Could not read the synthetic usage file: {error}"))?;
            let mut file = UsageFile::decode(black_box(&bytes))
                .map_err(|error| format!("The synthetic usage file was refused: {error}"))?;
            file.start_day(today);
            black_box(&file);
            Ok(started.elapsed().as_secs_f64() * 1e9)
        })
        .collect()
}

/// The 99th percentile, in nanoseconds, of one update's mean cost within each batch.
fn measure(update: impl Fn()) -> f64 {
    for _ in 0..WARMUP_BATCHES {
        for _ in 0..BATCH {
            update();
        }
    }
    let mut samples: Vec<f64> = (0..BATCHES)
        .map(|_| {
            let started = Instant::now();
            for _ in 0..BATCH {
                update();
            }
            started.elapsed().as_secs_f64() * 1e9 / f64::from(BATCH)
        })
        .collect();
    samples.sort_by(f64::total_cmp);
    samples[BATCHES * 99 / 100]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_largest_file_is_one_the_decoder_accepts() {
        let text = largest_file().expect("synthetic file");
        let file = UsageFile::decode(text.as_bytes()).expect("a file the encoder could write");
        let previous = file.previous.expect("a closed period");
        assert_eq!(previous.days_used, PERIOD_DAYS as u32);
        assert_eq!(file.current.days_used, PERIOD_DAYS as u32);
    }
}
