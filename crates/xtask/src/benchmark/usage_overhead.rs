//! Checks AN-01's overhead budget: one usage update costs under 100 ns at the 99th percentile.

use std::hint::black_box;
use std::io::{self, Write};
use std::time::{Duration, Instant};

use serde_json::json;
use syrinx_core::usage::{Feature, Timing, UsageRecorder};

/// Updates per timed batch; a single update is shorter than the clock's own cost.
const BATCH: u32 = 16;
const WARMUP_BATCHES: usize = 10_000;
const BATCHES: usize = 200_000;
/// The proposal's budget for one update on the calling thread.
const BUDGET_NS: f64 = 100.0;

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
    let passed = cases.iter().all(|(_, p99)| *p99 < BUDGET_NS);
    let report = json!({
        "benchmark": "usage-recorder",
        "method": format!("mean of {BATCH} consecutive updates per sample, {BATCHES} samples"),
        "budget_ns_p99": BUDGET_NS,
        "p99_ns": cases.iter().map(|(name, p99)| json!({ "case": name, "p99_ns": p99 })).collect::<Vec<_>>(),
        "passed": passed,
    });
    let mut output = io::stdout().lock();
    serde_json::to_writer_pretty(&mut output, &report)
        .map_err(|error| format!("Could not write the usage benchmark report: {error}"))?;
    writeln!(output)
        .map_err(|error| format!("Could not finish the usage benchmark report: {error}"))?;
    Ok(passed)
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
