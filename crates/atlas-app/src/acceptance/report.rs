//! Summarizes P1-33's measurements against their budgets and writes them as JSON.

use std::{fmt::Write as _, time::Duration};

use super::BURST_CADENCE;

/// D48: a complete tier-1 search within 5 ms at the 95th percentile.
const D48_BUDGET: Duration = Duration::from_millis(5);
/// Used, and reported as assumed, when Windows does not report the display's refresh rate.
const ASSUMED_INTERVAL: Duration = Duration::from_micros(16_667);
/// Owner decision 2026-10-12: keystroke to presented rows within two refresh intervals.
const COMPOSED_FRAMES: u32 = 2;
const OPEN_BUDGET: &str = "p95 within one refresh interval (owner decision 2026-10-12)";
const COMPOSED_BUDGET: &str = "p95 within two refresh intervals (owner decision 2026-10-12)";

/// One palette opening.
#[derive(Clone, Copy, Debug)]
pub struct OpenSample {
    /// From the open action to the first presented frame.
    pub presented: Duration,
    /// From the open action to the kept popup being reset and its show queued (DW-90).
    pub requested: Duration,
    /// From the open action to the show task starting to move and show the popup.
    pub started: Duration,
    /// From the open action to the popup's first render.
    pub rendered: Duration,
}

/// One keystroke in the palette; `None` means that stage never happened.
#[derive(Clone, Copy, Debug, Default)]
pub struct KeySample {
    /// The posted key to the frame showing the changed text.
    pub echo: Option<Duration>,
    /// The posted key to the frame showing that text's ranked rows: the composed latency.
    pub rows: Option<Duration>,
    /// Query queued to the worker, until its reply reached the palette.
    pub round_trip: Option<Duration>,
    /// The worker's catalogue time: ranking, scope counts and highlights (D48's quantity).
    pub worker: Option<Duration>,
    pub superseded: bool,
}

impl KeySample {
    fn timed_out(&self) -> bool {
        self.rows.is_none()
    }
}

/// One word typed or erased at a fixed cadence without waiting for each key's rows.
#[derive(Clone, Copy, Debug, Default)]
pub struct BurstSample {
    /// The last key posted, to the frame showing the final text's rows.
    pub final_rows: Option<Duration>,
    /// Palette requests started during the burst, and replies the palette accepted.
    pub queries: u64,
    pub replies: u64,
}

/// The keystrokes measured at one synthetic catalogue size.
#[derive(Clone, Debug)]
pub struct SizeResult {
    pub synthetic_rows: usize,
    pub catalogue_rows: usize,
    pub seed: Duration,
    pub samples: Vec<KeySample>,
    /// Back-to-back typing; reported, with no budget of its own.
    pub bursts: Vec<BurstSample>,
}

/// A pass/fail functional check with what was observed.
#[derive(Clone, Debug)]
pub struct Check {
    pub name: &'static str,
    pub passed: bool,
    pub detail: String,
}

/// Everything one acceptance run measured, in the order it ran.
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub refresh: Option<Duration>,
    /// Why Windows did not report the refresh interval, when it did not.
    pub refresh_problem: Option<String>,
    pub opens: Vec<OpenSample>,
    pub sizes: Vec<SizeResult>,
    pub checks: Vec<Check>,
    /// Set when the run could not continue; what ran before still counts.
    pub aborted: Option<String>,
}

/// Nearest-rank percentiles, in milliseconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stats {
    pub count: usize,
    pub p50: f64,
    pub p95: f64,
    pub max: f64,
}

impl Stats {
    pub fn of(values: impl IntoIterator<Item = Duration>) -> Option<Self> {
        let mut values: Vec<Duration> = values.into_iter().collect();
        if values.is_empty() {
            return None;
        }
        values.sort_unstable();
        Some(Self {
            count: values.len(),
            p50: milliseconds(nearest_rank(&values, 50)),
            p95: milliseconds(nearest_rank(&values, 95)),
            max: milliseconds(values[values.len() - 1]),
        })
    }

    fn within(&self, budget: Duration) -> bool {
        self.p95 <= milliseconds(budget)
    }
}

/// The smallest sample with at least `percent` percent of samples at or below it.
fn nearest_rank(sorted: &[Duration], percent: usize) -> Duration {
    let rank = (sorted.len() * percent).div_ceil(100).max(1);
    sorted[rank - 1]
}

fn milliseconds(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.
}

impl Report {
    pub fn check(&mut self, name: &'static str, passed: bool, detail: impl Into<String>) {
        self.checks.push(Check {
            name,
            passed,
            detail: detail.into(),
        });
    }

    /// The display interval the one-frame budget is judged at.
    fn interval(&self) -> Duration {
        self.refresh.unwrap_or(ASSUMED_INTERVAL)
    }

    fn open_stats(&self) -> Option<Stats> {
        Stats::of(self.opens.iter().map(|open| open.presented))
    }

    /// Opening passes when its 95th percentile is within one refresh interval.
    fn open_passed(&self) -> bool {
        self.open_stats()
            .is_some_and(|stats| stats.within(self.interval()))
    }

    /// D48 is judged on the worker's catalogue time; every key must also show its rows.
    fn d48_passed(size: &SizeResult) -> bool {
        let timed_out = size.samples.iter().any(KeySample::timed_out);
        let worker = Stats::of(size.samples.iter().filter_map(|key| key.worker));
        !timed_out && worker.is_some_and(|stats| stats.within(D48_BUDGET))
    }

    fn composed_budget(&self) -> Duration {
        self.interval() * COMPOSED_FRAMES
    }

    /// Key to presented rows, judged at p95; a key without rows fails it.
    fn composed_passed(&self, size: &SizeResult) -> bool {
        let timed_out = size.samples.iter().any(KeySample::timed_out);
        let composed = Stats::of(size.samples.iter().filter_map(|key| key.rows));
        !timed_out && composed.is_some_and(|stats| stats.within(self.composed_budget()))
    }

    /// Every stated budget and functional check must pass; bursts are reported only.
    pub fn passed(&self) -> bool {
        self.aborted.is_none()
            && self.open_passed()
            && !self.sizes.is_empty()
            && self
                .sizes
                .iter()
                .all(|size| Self::d48_passed(size) && self.composed_passed(size))
            && self.checks.iter().all(|check| check.passed)
    }

    pub fn to_json(&self, generated_unix: u64) -> String {
        let mut json = String::new();
        json.push_str("{\n");
        push_field(&mut json, 1, "slice", &quoted("P1-33"));
        push_field(&mut json, 1, "generated_unix", &generated_unix.to_string());
        push_field(&mut json, 1, "passed", &self.passed().to_string());
        let aborted = self.aborted.as_deref().map_or("null".to_owned(), quoted);
        push_field(&mut json, 1, "aborted", &aborted);
        push_field(&mut json, 1, "display", &self.display_json());
        push_field(&mut json, 1, "open", &self.open_json());
        let sizes: Vec<String> = self.sizes.iter().map(|size| self.size_json(size)).collect();
        push_field(&mut json, 1, "ranking", &array(&sizes, 2));
        let checks: Vec<String> = self.checks.iter().map(check_json).collect();
        push_last(&mut json, 1, "checks", &array(&checks, 2));
        json.push('}');
        json.push('\n');
        json
    }

    fn display_json(&self) -> String {
        let measured = self.refresh.is_some();
        let problem = self
            .refresh_problem
            .as_deref()
            .map_or("null".to_owned(), quoted);
        object(&[
            ("interval_ms", number(milliseconds(self.interval()))),
            ("refresh_hz", number(1. / self.interval().as_secs_f64())),
            ("measured", measured.to_string()),
            ("problem", problem),
        ])
    }

    fn open_json(&self) -> String {
        let budget = self.interval();
        let frames: Vec<String> = self
            .opens
            .iter()
            .map(|open| frames(open.presented, budget).to_string())
            .collect();
        object(&[
            ("budget", quoted(OPEN_BUDGET)),
            ("budget_ms", number(milliseconds(budget))),
            ("passed", self.open_passed().to_string()),
            ("presented", stats_json(self.open_stats())),
            (
                "show_requested",
                stats_json(Stats::of(self.opens.iter().map(|open| open.requested))),
            ),
            (
                "show_started",
                stats_json(Stats::of(self.opens.iter().map(|open| open.started))),
            ),
            (
                "first_render",
                stats_json(Stats::of(self.opens.iter().map(|open| open.rendered))),
            ),
            ("frames", format!("[{}]", frames.join(", "))),
            (
                "presented_ms",
                durations_json(self.opens.iter().map(|open| open.presented)),
            ),
        ])
    }

    fn size_json(&self, size: &SizeResult) -> String {
        let samples = &size.samples;
        let composed = Stats::of(samples.iter().filter_map(|key| key.rows));
        let timeouts = samples.iter().filter(|key| key.timed_out()).count();
        let superseded = samples.iter().filter(|key| key.superseded).count();
        object(&[
            ("synthetic_rows", size.synthetic_rows.to_string()),
            ("catalogue_rows", size.catalogue_rows.to_string()),
            ("seed_ms", number(milliseconds(size.seed))),
            ("keystrokes", samples.len().to_string()),
            ("timeouts", timeouts.to_string()),
            ("superseded", superseded.to_string()),
            ("d48_budget_ms", number(milliseconds(D48_BUDGET))),
            ("d48_passed", Self::d48_passed(size).to_string()),
            (
                "worker",
                stats_json(Stats::of(samples.iter().filter_map(|key| key.worker))),
            ),
            (
                "round_trip",
                stats_json(Stats::of(samples.iter().filter_map(|key| key.round_trip))),
            ),
            (
                "echo",
                stats_json(Stats::of(samples.iter().filter_map(|key| key.echo))),
            ),
            ("composed", stats_json(composed)),
            ("composed_budget", quoted(COMPOSED_BUDGET)),
            (
                "composed_budget_ms",
                number(milliseconds(self.composed_budget())),
            ),
            ("composed_passed", self.composed_passed(size).to_string()),
            ("burst", burst_json(&size.bursts)),
            (
                "composed_ms",
                durations_json(samples.iter().filter_map(|key| key.rows)),
            ),
            (
                "worker_ms",
                durations_json(samples.iter().filter_map(|key| key.worker)),
            ),
        ])
    }
}

/// How many refresh intervals a latency spans; anything up to one interval is one frame.
fn frames(latency: Duration, interval: Duration) -> u32 {
    let ratio = latency.as_secs_f64() / interval.as_secs_f64();
    (ratio.ceil() as u32).max(1)
}

/// Back-to-back typing: final-key latency and how many requests a newer one replaced.
fn burst_json(bursts: &[BurstSample]) -> String {
    let queries: u64 = bursts.iter().map(|burst| burst.queries).sum();
    let replies: u64 = bursts.iter().map(|burst| burst.replies).sum();
    let missing = bursts
        .iter()
        .filter(|burst| burst.final_rows.is_none())
        .count();
    object(&[
        ("cadence_ms", number(milliseconds(BURST_CADENCE))),
        ("words", bursts.len().to_string()),
        ("missing_final_rows", missing.to_string()),
        (
            "final_rows",
            stats_json(Stats::of(
                bursts.iter().filter_map(|burst| burst.final_rows),
            )),
        ),
        ("queries", queries.to_string()),
        ("superseded", queries.saturating_sub(replies).to_string()),
    ])
}

fn check_json(check: &Check) -> String {
    object(&[
        ("name", quoted(check.name)),
        ("passed", check.passed.to_string()),
        ("detail", quoted(&check.detail)),
    ])
}

fn stats_json(stats: Option<Stats>) -> String {
    let Some(stats) = stats else {
        return "null".to_owned();
    };
    object(&[
        ("count", stats.count.to_string()),
        ("p50_ms", number(stats.p50)),
        ("p95_ms", number(stats.p95)),
        ("max_ms", number(stats.max)),
    ])
}

fn durations_json(values: impl Iterator<Item = Duration>) -> String {
    let values: Vec<String> = values.map(|value| number(milliseconds(value))).collect();
    format!("[{}]", values.join(", "))
}

/// A compact one-line object; nested values are already JSON.
fn object(fields: &[(&str, String)]) -> String {
    let fields: Vec<String> = fields
        .iter()
        .map(|(name, value)| format!("{}: {value}", quoted(name)))
        .collect();
    format!("{{{}}}", fields.join(", "))
}

fn array(items: &[String], depth: usize) -> String {
    if items.is_empty() {
        return "[]".to_owned();
    }
    let indent = "  ".repeat(depth);
    let closing = "  ".repeat(depth - 1);
    format!(
        "[\n{indent}{}\n{closing}]",
        items.join(&format!(",\n{indent}"))
    )
}

fn push_field(json: &mut String, depth: usize, name: &str, value: &str) {
    push_last(json, depth, name, value);
    json.insert(json.len() - 1, ',');
}

fn push_last(json: &mut String, depth: usize, name: &str, value: &str) {
    let indent = "  ".repeat(depth);
    // Writing to a String cannot fail.
    writeln!(json, "{indent}{}: {value}", quoted(name)).expect("write to string");
}

/// Milliseconds with microsecond precision; never NaN, since every input is a duration.
fn number(value: f64) -> String {
    format!("{value:.3}")
}

/// A JSON string with quotes, backslashes and control characters escaped.
fn quoted(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len() + 2);
    escaped.push('"');
    for character in text.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            control if control.is_control() => {
                write!(escaped, "\\u{:04x}", u32::from(control)).expect("write to string");
            }
            other => escaped.push(other),
        }
    }
    escaped.push('"');
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    #[test]
    fn percentiles_use_nearest_rank_and_report_the_maximum() {
        let stats = Stats::of((1..=100).map(ms)).expect("samples");
        assert_eq!(
            (stats.count, stats.p50, stats.p95, stats.max),
            (100, 50., 95., 100.)
        );
        let single = Stats::of([ms(7)]).expect("one sample");
        assert_eq!((single.p50, single.p95, single.max), (7., 7., 7.));
        assert!(Stats::of([]).is_none());
    }

    #[test]
    fn frames_count_whole_refresh_intervals() {
        let interval = Duration::from_micros(16_667);
        assert_eq!(frames(ms(3), interval), 1);
        assert_eq!(frames(interval, interval), 1);
        assert_eq!(frames(ms(17), interval), 2);
        assert_eq!(frames(ms(50), interval), 3);
    }

    #[test]
    fn the_owner_budgets_decide_the_run() {
        let fast_key = KeySample {
            echo: Some(ms(2)),
            rows: Some(ms(20)),
            round_trip: Some(ms(4)),
            worker: Some(ms(3)),
            superseded: false,
        };
        let mut report = Report {
            refresh: Some(Duration::from_micros(16_667)),
            opens: vec![OpenSample {
                presented: ms(10),
                requested: ms(1),
                started: ms(3),
                rendered: ms(4),
            }],
            sizes: vec![SizeResult {
                synthetic_rows: 1_000,
                catalogue_rows: 1_004,
                seed: ms(20),
                samples: vec![fast_key],
                bursts: vec![BurstSample {
                    final_rows: Some(ms(90)),
                    queries: 5,
                    replies: 2,
                }],
            }],
            ..Report::default()
        };
        report.check("synthetic", true, "ok");
        // A slow burst is reported but has no budget of its own.
        assert!(report.passed());
        assert!(report.to_json(1).contains("\"superseded\": 3"));

        report.sizes[0].samples[0].rows = Some(ms(40));
        assert!(!report.passed(), "40 ms misses two 16.7 ms frames at p95");
        assert!(report.to_json(1).contains("\"composed_passed\": false"));
        report.sizes[0].samples[0].rows = Some(ms(20));

        report.sizes[0].samples.push(KeySample {
            worker: Some(ms(3)),
            ..KeySample::default()
        });
        assert!(!report.passed(), "a keystroke without rows fails");
        report.sizes[0].samples.pop();
        report.opens.push(OpenSample {
            presented: ms(30),
            requested: ms(1),
            started: ms(3),
            rendered: ms(4),
        });
        assert!(!report.passed(), "a slow opening at p95 fails");
    }

    #[test]
    fn json_escapes_text_and_marks_an_assumed_refresh_rate() {
        assert_eq!(quoted("a\"b\\c\nd\u{1}"), "\"a\\\"b\\\\c\\nd\\u0001\"");
        let mut report = Report {
            aborted: Some("stopped \"early\"".to_owned()),
            ..Report::default()
        };
        report.check("quoted", false, "detail with \\ and \"");
        let json = report.to_json(5);
        assert!(json.contains("\"aborted\": \"stopped \\\"early\\\"\""));
        assert!(json.contains("\"measured\": false"));
        assert!(json.contains("\"passed\": false"));
        assert!(json.ends_with("}\n"));
    }
}
