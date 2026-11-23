//! Measures the complete tier-one search path against a reproducible synthetic catalogue, and
//! AN-01's usage recorder against its overhead budget.

use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::hint::black_box;
use std::io::{self, Write};
use std::ops::Range;
use std::time::Instant;

use serde_json::{Value, json};
use syrinx_search::{
    BlankQuery, BoostWeights, Catalogue, Entry, EntryKey, FeedKey, Scope, SearchPolicy,
    SearchResults, Target, Usage,
};

mod usage_overhead;
mod workload;
use workload::*;

struct Sample {
    elapsed_ns: u64,
    matches: usize,
    hits: usize,
    checksum: u64,
}

#[derive(Debug, Eq, PartialEq)]
struct Distribution {
    median_ns: u64,
    p95_ns: u64,
    worst_ns: u64,
}

/// One set of cases measured over one corpus: every timed sample, and the corpus's build time.
struct Measured {
    samples: Vec<Vec<Sample>>,
    build_ns: u64,
}

pub fn run() -> Result<(), String> {
    // AN-01's usage recorder first: it needs no catalogue.
    let usage_passed = usage_overhead::run()?;
    let cases = cases();
    // D48's gate: its 100,000 rows alone, as before plugin feeds.
    let base = measure(&cases, 0)?;
    // The same cases with a same-size plugin feed (#141): reported, never failing the run,
    // until the owner decides what D48 covers.
    let with_plugin_feed = measure(&cases, PLUGIN_ROWS)?;
    let (report, passed) = report(&cases, &base, &with_plugin_feed)?;
    let mut output = io::stdout().lock();
    serde_json::to_writer_pretty(&mut output, &report)
        .map_err(|error| format!("Could not write the synthetic benchmark report: {error}"))?;
    writeln!(output)
        .map_err(|error| format!("Could not finish the synthetic benchmark report: {error}"))?;
    output
        .flush()
        .map_err(|error| format!("Could not flush the synthetic benchmark report: {error}"))?;
    if !passed {
        return Err("Search benchmark failed: at least one case has p95 >= 5 ms.".to_owned());
    }
    if !usage_passed {
        return Err("Usage benchmark failed: an update's p99 is 100 ns or more, or loading the file takes 1 ms or more at p95.".to_owned());
    }
    Ok(())
}

/// Builds the corpus with `plugin_rows` plugin rows, warms up, then times every case.
fn measure(cases: &[Case], plugin_rows: usize) -> Result<Measured, String> {
    // Construct all inputs before timing any complete query.
    let build_started = Instant::now();
    let mut catalogue = build_catalogue(plugin_rows)?;
    let build_ns = nanoseconds(build_started)?;
    let mut revision = 0;
    for pass in 0..WARMUPS {
        run_pass(&mut catalogue, cases, pass, &mut revision, plugin_rows)?;
    }

    // Retain every sample; printing and validation never enter the timed interval.
    let mut samples: Vec<Vec<Sample>> = (0..cases.len()).map(|_| Vec::new()).collect();
    for pass in 0..PASSES {
        let measured = run_pass(&mut catalogue, cases, pass, &mut revision, plugin_rows)?;
        for (case_index, sample) in measured {
            samples[case_index].push(sample);
        }
    }
    Ok(Measured { samples, build_ns })
}

fn run_pass(
    catalogue: &mut Catalogue,
    cases: &[Case],
    pass: usize,
    revision: &mut u64,
    plugin_rows: usize,
) -> Result<Vec<(usize, Sample)>, String> {
    let mut samples = Vec::with_capacity(cases.len());
    for offset in 0..cases.len() {
        let case_index = case_index(offset, pass, cases.len());
        refresh_generation(catalogue, revision)?;
        let case = &cases[case_index];
        let policy = policy(&case.text)?;

        // Include query work and complete owned results; exclude the checks below.
        let started = Instant::now();
        let result = black_box(catalogue.query(black_box(&case.text), case.scope, NOW, policy));
        let elapsed_ns = nanoseconds(started)?;
        let result = result.map_err(|error| invalid(case, &format!("query returned {error:?}")))?;
        validate_result(catalogue, case, &result, plugin_rows)?;
        samples.push((
            case_index,
            Sample {
                elapsed_ns,
                matches: result.counts.total(),
                hits: result.hits.len(),
                checksum: checksum(&result),
            },
        ));
    }
    Ok(samples)
}

fn validate_result(
    catalogue: &Catalogue,
    case: &Case,
    result: &SearchResults,
    plugin_rows: usize,
) -> Result<(), String> {
    let scoped_count = match case.scope {
        Scope::Everything => result.counts.total(),
        Scope::Tabs => result.counts.tabs,
        Scope::Saved => result.counts.saved,
        Scope::History => result.counts.history,
        Scope::Commands => result.counts.commands,
    };
    if result.generation != catalogue.generation() {
        return Err(invalid(
            case,
            "result generation differs from the catalogue",
        ));
    }
    if result.hits.len() != scoped_count.min(LIMIT) {
        return Err(invalid(
            case,
            &format!(
                "hit count {} differs from limited scope count {}",
                result.hits.len(),
                scoped_count.min(LIMIT)
            ),
        ));
    }
    if result.counts.tabs > TABS
        || result.counts.saved > SAVED
        || result.counts.history > HISTORY
        || result.counts.commands > COMMANDS
        || result.counts.plugins > plugin_rows
    {
        return Err(invalid(
            case,
            &format!("scope counts exceed corpus bounds: {:?}", result.counts),
        ));
    }
    if case.label == "no-match" && result.counts.total() != 0 {
        return Err(invalid(case, "no-match query unexpectedly matched a row"));
    }
    if !matches!(
        case.label,
        "no-match" | "transposed-characters" | "long-near-match"
    ) && result.hits.is_empty()
    {
        return Err(invalid(case, "positive query returned no hits"));
    }
    if case.text.is_empty()
        && (result.counts.tabs != TABS
            || result.counts.saved != SAVED
            || result.counts.history != HISTORY
            || result.counts.commands != COMMANDS
            || result.counts.plugins != plugin_rows
            || result.hits.len() != LIMIT)
    {
        return Err(invalid(
            case,
            "blank query lost rows or returned incomplete top results",
        ));
    }
    let mut identities = HashSet::new();
    let mut previous = None;
    for hit in &result.hits {
        let entry = catalogue.get(hit.target).ok_or_else(|| {
            invalid_hit(case, hit.target, "identity is absent from the catalogue")
        })?;
        if hit.title != entry.title {
            return Err(invalid_hit(
                case,
                hit.target,
                "title differs from the catalogue entry",
            ));
        }
        if hit.address != entry.address {
            return Err(invalid_hit(
                case,
                hit.target,
                "address differs from the catalogue entry",
            ));
        }
        let out_of_order = previous.is_some_and(|(score, target)| {
            hit.score > score || (hit.score == score && hit.target < target)
        });
        if !in_scope(hit.target, case.scope) {
            return Err(invalid_hit(
                case,
                hit.target,
                "target is outside the selected scope",
            ));
        }
        if !valid_ranges(&hit.title, &hit.title_matches) {
            return Err(invalid_hit(
                case,
                hit.target,
                &format!("invalid title highlight ranges {:?}", hit.title_matches),
            ));
        }
        if !valid_ranges(&hit.address, &hit.address_matches) {
            return Err(invalid_hit(
                case,
                hit.target,
                &format!("invalid address highlight ranges {:?}", hit.address_matches),
            ));
        }
        if !identities.insert(hit.target) {
            return Err(invalid_hit(case, hit.target, "duplicate result identity"));
        }
        if out_of_order {
            return Err(invalid_hit(
                case,
                hit.target,
                &format!(
                    "score/identity ordering failed: score {} after {:?}",
                    hit.score, previous
                ),
            ));
        }
        if !case.text.is_empty() && hit.title_matches.is_empty() && hit.address_matches.is_empty() {
            return Err(invalid_hit(
                case,
                hit.target,
                "nonblank query returned no highlighted field",
            ));
        }
        previous = Some((hit.score, hit.target));
    }
    Ok(())
}

fn invalid(case: &Case, reason: &str) -> String {
    format!("Synthetic case {}: {reason}.", case.label)
}

fn invalid_hit(case: &Case, target: Target, reason: &str) -> String {
    invalid(case, &format!("target {target:?}: {reason}"))
}

fn in_scope(target: Target, scope: Scope) -> bool {
    matches!(
        (target, scope),
        (_, Scope::Everything)
            | (Target::Tab(_), Scope::Tabs)
            | (Target::Saved(_), Scope::Saved)
            | (Target::History(_), Scope::History)
            | (Target::Command(_), Scope::Commands)
    )
}

fn valid_ranges(text: &str, ranges: &[Range<usize>]) -> bool {
    let mut previous_end = 0;
    for range in ranges {
        if range.start < previous_end
            || range.start >= range.end
            || range.end > text.len()
            || !text.is_char_boundary(range.start)
            || !text.is_char_boundary(range.end)
        {
            return false;
        }
        previous_end = range.end;
    }
    true
}

fn checksum(result: &SearchResults) -> u64 {
    // Consume all returned content, positions, identities, scores and uncapped counts.
    let mut hasher = DefaultHasher::new();
    result.generation.hash(&mut hasher);
    result.counts.tabs.hash(&mut hasher);
    result.counts.saved.hash(&mut hasher);
    result.counts.history.hash(&mut hasher);
    result.counts.commands.hash(&mut hasher);
    for hit in &result.hits {
        hit.target.hash(&mut hasher);
        hit.score.hash(&mut hasher);
        hit.title.hash(&mut hasher);
        hit.address.hash(&mut hasher);
        hit.title_matches.hash(&mut hasher);
        hit.address_matches.hash(&mut hasher);
    }
    black_box(hasher.finish())
}

fn distribution(samples: &[Sample]) -> Result<Distribution, String> {
    if samples.is_empty() {
        return Err("Cannot summarize an empty set of benchmark samples.".to_owned());
    }
    let mut times: Vec<_> = samples.iter().map(|sample| sample.elapsed_ns).collect();
    times.sort_unstable();
    Ok(Distribution {
        median_ns: times[(times.len() * 50).div_ceil(100) - 1],
        p95_ns: times[(times.len() * 95).div_ceil(100) - 1],
        worst_ns: times[times.len() - 1],
    })
}

/// The report: D48's gate from `base`, which alone decides whether the run passes, and the
/// same cases over `with_plugin_feed` as a separate set that never fails it.
fn report(
    cases: &[Case],
    base: &Measured,
    with_plugin_feed: &Measured,
) -> Result<(Value, bool), String> {
    let (reports, passed, worst_ns) = case_reports(cases, &base.samples)?;
    let (feed_reports, feed_within_budget, feed_worst_ns) =
        case_reports(cases, &with_plugin_feed.samples)?;
    Ok((
        json!({
            "protocol": "syrinx-tier-one-v1",
            "required_profile": "local",
            "architecture": std::env::consts::ARCH,
            "operating_system": std::env::consts::OS,
            "rows": ROWS,
            "kinds": {"tabs": TABS, "saved": SAVED, "history": HISTORY, "commands": COMMANDS},
            "unicode_rows": ROWS / 5,
            "long_title_rows": ROWS / 10,
            "uppercase_title_rows": ROWS.div_ceil(101),
            "corpus_build_ns": base.build_ns,
            "reference_time": NOW,
            "top_k": LIMIT,
            "max_typos": "one from four Unicode scalar values; otherwise zero",
            "blank_query": "all",
            "boosts": {"open": 120, "frequency": 30, "recency": 30, "window_seconds": 7 * 86_400},
            "warmup_passes": WARMUPS,
            "measured_passes": PASSES,
            "acceptance": "nearest-rank p95 < 5000000 ns for every case; worst samples retained",
            "passed": passed,
            "worst_ns": worst_ns,
            "cases": reports,
            "with_plugin_feed": {
                "plugin_feed_rows": PLUGIN_ROWS,
                "rows": ROWS + PLUGIN_ROWS,
                "corpus_build_ns": with_plugin_feed.build_ns,
                "gates_the_run": false,
                "acceptance": "reported only, until the owner decides whether D48 covers plugin feeds (#141)",
                "within_budget": feed_within_budget,
                "worst_ns": feed_worst_ns,
                "cases": feed_reports,
            },
        }),
        passed,
    ))
}

/// Each case's distribution and samples, whether every case's p95 is within the budget, and
/// the slowest sample of all.
fn case_reports(
    cases: &[Case],
    samples: &[Vec<Sample>],
) -> Result<(Vec<Value>, bool, u64), String> {
    if cases.len() != samples.len() || samples.iter().any(|samples| samples.len() != PASSES) {
        return Err("Benchmark report is missing a case or measured sample.".to_owned());
    }
    let mut passed = true;
    let mut worst_ns = 0;
    let mut reports = Vec::with_capacity(cases.len());
    for (case, samples) in cases.iter().zip(samples) {
        let summary = distribution(samples)?;
        let case_passed = summary.p95_ns < BUDGET_NS;
        passed &= case_passed;
        worst_ns = worst_ns.max(summary.worst_ns);
        reports.push(json!({
            "case": case.label,
            "query": case.text,
            "scope": format!("{:?}", case.scope),
            "median_ns": summary.median_ns,
            "p95_ns": summary.p95_ns,
            "worst_ns": summary.worst_ns,
            "passed": case_passed,
            "samples": samples.iter().map(|sample| json!({
                "elapsed_ns": sample.elapsed_ns,
                "matches": sample.matches,
                "hits": sample.hits,
                "checksum": sample.checksum,
            })).collect::<Vec<_>>(),
        }));
    }
    Ok((reports, passed, worst_ns))
}

fn nanoseconds(started: Instant) -> Result<u64, String> {
    u64::try_from(started.elapsed().as_nanos())
        .map_err(|_| "Benchmark duration exceeds the report's numeric range.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpus_has_exact_distinct_mixed_rows_and_reserved_addresses() -> Result<(), String> {
        let mut targets = HashSet::new();
        let mut counts = [0; 4];
        let mut unicode = 0;
        let mut long = 0;
        let mut uppercase = 0;
        for position in 0..ROWS {
            let row = entry((position * 7_919) % ROWS)?;
            assert!(targets.insert(row.target));
            match row.target {
                Target::Tab(_) => counts[0] += 1,
                Target::Saved(_) => counts[1] += 1,
                Target::History(_) => counts[2] += 1,
                Target::Command(_) => counts[3] += 1,
                Target::Plugin { .. } => return Err("the base corpus holds no plugin row".into()),
            }
            unicode += usize::from(!row.title.is_ascii());
            long += usize::from(row.title.len() >= 256);
            uppercase += usize::from(row.title.bytes().any(|byte| byte.is_ascii_uppercase()));
            assert!(row.address.is_empty() || row.address.contains(".test/"));
        }
        assert_eq!(counts, [TABS, SAVED, HISTORY, COMMANDS]);
        assert_eq!(targets.len(), ROWS);
        assert_eq!(unicode, ROWS / 5);
        assert_eq!(long, ROWS / 10);
        assert_eq!(uppercase, ROWS.div_ceil(101));
        Ok(())
    }

    #[test]
    fn p95_retains_slow_tail_without_replacing_it_with_the_mean() -> Result<(), String> {
        let samples: Vec<_> = (1..=40)
            .rev()
            .map(|elapsed_ns| Sample {
                elapsed_ns,
                matches: 0,
                hits: 0,
                checksum: 0,
            })
            .collect();
        assert_eq!(
            distribution(&samples)?,
            Distribution {
                median_ns: 20,
                p95_ns: 38,
                worst_ns: 40,
            }
        );
        assert!(distribution(&[]).is_err());
        Ok(())
    }

    #[test]
    fn highlight_validation_rejects_split_unicode_and_overlapping_ranges() {
        assert!(valid_ranges("café🧭", &[3..5, 5..9]));
        assert!(!valid_ranges("café🧭", std::slice::from_ref(&(3..4))));
        assert!(!valid_ranges("café🧭", &[3..5, 3..5]));
        assert!(!valid_ranges("café🧭", std::slice::from_ref(&(5..10))));
        assert!(!valid_ranges("café🧭", std::slice::from_ref(&(3..3))));
    }

    /// One case's samples: 1 ms each, but `slow_ns` from pass `slow_from` on.
    fn timed(slow_from: usize, slow_ns: u64) -> Measured {
        let samples = (0..PASSES)
            .map(|index| Sample {
                elapsed_ns: if index >= slow_from {
                    slow_ns
                } else {
                    1_000_000
                },
                matches: ROWS,
                hits: LIMIT,
                checksum: 0,
            })
            .collect();
        Measured {
            samples: vec![samples],
            build_ns: 0,
        }
    }

    #[test]
    fn acceptance_checks_each_case_p95_and_keeps_worst_samples() -> Result<(), String> {
        let cases = vec![Case {
            label: "synthetic-acceptance",
            text: String::new(),
            scope: Scope::Everything,
        }];
        let within = timed(38, 9_000_000);
        let (passing, passed) = report(&cases, &within, &within)?;
        assert!(passed);
        assert_eq!(passing["worst_ns"], 9_000_000);
        assert_eq!(
            passing["cases"][0]["samples"].as_array().map(Vec::len),
            Some(PASSES)
        );

        let over = timed(37, BUDGET_NS);
        assert!(!report(&cases, &over, &within)?.1);
        let missing = Measured {
            samples: Vec::new(),
            build_ns: 0,
        };
        assert!(report(&cases, &missing, &within).is_err());
        Ok(())
    }

    #[test]
    fn a_slow_plugin_feed_set_is_reported_but_never_fails_the_run() -> Result<(), String> {
        let cases = vec![Case {
            label: "synthetic-plugin-feed",
            text: String::new(),
            scope: Scope::Everything,
        }];
        let (report, passed) = report(&cases, &timed(PASSES, 0), &timed(0, BUDGET_NS))?;
        assert!(passed, "D48's gate reads only the base set");
        assert_eq!(report["passed"], true);
        assert_eq!(report["with_plugin_feed"]["within_budget"], false);
        assert_eq!(report["with_plugin_feed"]["gates_the_run"], false);
        assert_eq!(report["with_plugin_feed"]["plugin_feed_rows"], PLUGIN_ROWS);
        assert!(
            report.get("plugin_feed_rows").is_none(),
            "the base keys as on main"
        );
        Ok(())
    }

    #[test]
    fn checksum_observes_highlights_scores_counts_and_returned_content() -> Result<(), String> {
        let mut catalogue = Catalogue::new();
        catalogue
            .upsert(entry(0)?)
            .map_err(|error| format!("Synthetic setup failed: {error:?}"))?;
        let mut result = catalogue
            .query("amber", Scope::Everything, NOW, policy("amber")?)
            .map_err(|error| format!("Synthetic query failed: {error:?}"))?;
        assert_eq!(result.hits.len(), 1);
        let original = checksum(&result);
        result.hits[0].title_matches.push(0..1);
        assert_ne!(original, checksum(&result));
        let previous = checksum(&result);
        result.hits[0].score += 1;
        assert_ne!(previous, checksum(&result));
        let previous = checksum(&result);
        result.counts.history += 1;
        assert_ne!(previous, checksum(&result));
        let previous = checksum(&result);
        result.hits[0].address.push_str("/changed");
        assert_ne!(previous, checksum(&result));
        Ok(())
    }

    #[test]
    fn validation_rejects_unknown_ids_stale_content_and_impossible_counts() -> Result<(), String> {
        let mut catalogue = Catalogue::new();
        catalogue
            .upsert(entry(0)?)
            .map_err(|error| format!("Synthetic setup failed: {error:?}"))?;
        let case = Case {
            label: "synthetic-validation",
            text: "amber".to_owned(),
            scope: Scope::Tabs,
        };
        let original = catalogue
            .query(&case.text, case.scope, NOW, policy(&case.text)?)
            .map_err(|error| format!("Synthetic query failed: {error:?}"))?;
        validate_result(&catalogue, &case, &original, 0)?;

        let mut changed = original.clone();
        changed.hits[0].target = target(1)?;
        assert!(validate_result(&catalogue, &case, &changed, 0).is_err());
        changed = original.clone();
        changed.hits[0].title.push_str(" stale");
        assert!(validate_result(&catalogue, &case, &changed, 0).is_err());
        changed = original.clone();
        changed.hits[0].address.push_str("/stale");
        assert!(validate_result(&catalogue, &case, &changed, 0).is_err());

        // Changing an unselected scope isolates the count bound from top-K checks.
        changed = original.clone();
        changed.counts.history = HISTORY + 1;
        assert!(validate_result(&catalogue, &case, &changed, 0).is_err());
        changed = original.clone();
        changed.counts.saved = SAVED + 1;
        assert!(validate_result(&catalogue, &case, &changed, 0).is_err());
        changed = original;
        changed.counts.commands = COMMANDS + 1;
        assert!(validate_result(&catalogue, &case, &changed, 0).is_err());
        Ok(())
    }

    #[test]
    fn no_typo_unicode_counts_match_the_independently_inspected_corpus() -> Result<(), String> {
        // D48's corpus alone, then with the plugin feed, which repeats every title.
        for plugin_rows in [0, PLUGIN_ROWS] {
            unicode_counts_match(plugin_rows)?;
        }
        Ok(())
    }

    fn unicode_counts_match(plugin_rows: usize) -> Result<(), String> {
        let mut catalogue = build_catalogue(plugin_rows)?;
        for query in ["東京", "🧭"] {
            let mut expected = syrinx_search::ScopeCounts::default();
            let base = (0..ROWS).map(entry);
            for row in base.chain((0..plugin_rows).map(plugin_entry)) {
                let row = row?;
                if !row.title.contains(query) {
                    continue;
                }
                match row.target {
                    Target::Tab(_) => expected.tabs += 1,
                    Target::Saved(_) => expected.saved += 1,
                    Target::History(_) => expected.history += 1,
                    Target::Command(_) => expected.commands += 1,
                    Target::Plugin { .. } => expected.plugins += 1,
                }
            }
            let repeated = if plugin_rows == 0 { 0 } else { 4_000 };
            assert_eq!(expected.plugins, repeated);
            assert_eq!(expected.total(), 4_000 + repeated);
            let mut policy = policy(query)?;
            policy.max_typos = 0;
            let result = catalogue
                .query(query, Scope::Everything, NOW, policy)
                .map_err(|error| format!("Synthetic Unicode query failed: {error:?}"))?;
            assert_eq!(result.counts, expected);
            validate_result(
                &catalogue,
                &Case {
                    label: "synthetic-unicode-counts",
                    text: query.to_owned(),
                    scope: Scope::Everything,
                },
                &result,
                plugin_rows,
            )?;
        }
        Ok(())
    }
}
