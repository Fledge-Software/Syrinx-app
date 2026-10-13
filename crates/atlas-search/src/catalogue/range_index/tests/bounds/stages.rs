//! Separates complete range-query costs without changing its workload or stage API.

use std::time::Duration;

use super::*;
use crate::SearchResults;
use crate::catalogue::{QueryObserver, QueryStage};

struct Recorder {
    started: Instant,
    previous: Instant,
    stages: [Duration; 5],
    next_stage: usize,
    range_stage: bool,
    counters: Counters,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Counters {
    candidates: usize,
    scored_rows: usize,
    computed_fields: usize,
    reused_fields: usize,
    computed_traces: usize,
    reused_traces: usize,
}

struct Sample {
    total: Duration,
    stages: [Duration; 5],
    counters: Counters,
    report: QueryReport,
    phases: Option<PhaseTimings>,
    matches: usize,
    hits: usize,
}

impl Recorder {
    fn new() -> Self {
        let started = Instant::now();
        Self {
            started,
            previous: started,
            stages: [Duration::ZERO; 5],
            next_stage: 0,
            range_stage: false,
            counters: Counters::default(),
        }
    }

    fn finish(
        self,
        total: Duration,
        result: &SearchResults,
        report: QueryReport,
        phases: Option<PhaseTimings>,
    ) -> Sample {
        assert_eq!(self.next_stage, 5, "every query stage must finish once");
        assert_eq!(self.range_stage, report.route == Route::Ranges);
        assert!(self.stages.iter().sum::<Duration>() <= total);
        assert!(self.counters.scored_rows <= self.counters.candidates);
        assert!(self.counters.computed_traces <= 4 * result.hits.len());
        assert!(self.counters.reused_traces <= 2 * result.hits.len());
        if self.range_stage {
            let phases = phases.expect("opted-in successful range phases");
            assert!(phases.candidates + phases.expansion <= self.stages[1]);
            assert_eq!(self.counters.candidates, report.candidate_rows);
            assert_eq!(
                self.counters.scored_rows,
                report.range_leaves_ranked + report.fallback_rows_scored
            );
            assert_eq!(result.counts.total(), report.matching_rows);
        } else {
            assert!(phases.is_none(), "fallback has no successful range timings");
            assert_eq!(
                self.counters.computed_fields + self.counters.reused_fields,
                2 * self.counters.scored_rows
            );
        }
        Sample {
            total,
            stages: self.stages,
            counters: self.counters,
            report,
            phases,
            matches: result.counts.total(),
            hits: result.hits.len(),
        }
    }
}

impl QueryObserver for Recorder {
    fn stage_finished(&mut self, stage: QueryStage) {
        let now = Instant::now();
        let index = match stage {
            QueryStage::Setup => 0,
            QueryStage::MembershipAndCandidates => 1,
            QueryStage::RangeMembershipAndRanking => {
                self.range_stage = true;
                1
            }
            QueryStage::HeapAndExactScoring => 2,
            QueryStage::FinalistOrdering => 3,
            QueryStage::HighlightsAndOwnedResults => 4,
        };
        assert_eq!(
            index, self.next_stage,
            "query stage order or duplicate event"
        );
        self.stages[index] = now.duration_since(self.previous);
        self.previous = now;
        self.next_stage += 1;
    }

    fn candidates(&mut self, count: usize) {
        self.counters.candidates = count;
    }

    fn candidate_scored(&mut self) {
        self.counters.scored_rows += 1;
    }

    fn field_scores(&mut self, computed: usize, reused: usize) {
        self.counters.computed_fields = computed;
        self.counters.reused_fields = reused;
    }

    fn highlight_traces(&mut self, computed: usize, reused: usize) {
        self.counters.computed_traces = computed;
        self.counters.reused_traces = reused;
    }
}

fn query_sample(
    catalogue: &mut Catalogue,
    query: &str,
    options: SearchPolicy,
    metadata: bool,
) -> (SearchResults, Sample) {
    catalogue
        .ranges
        .as_mut()
        .expect("index")
        .set_metadata_bounds_enabled(metadata);
    catalogue
        .ranges
        .as_mut()
        .expect("index")
        .set_phase_timing_enabled(true);
    let generation = catalogue.generation();
    let mut recorder = Recorder::new();
    let result = catalogue.query_with_strategy(
        query,
        Scope::Everything,
        NOW,
        options,
        QueryStrategy::Ranges,
        &mut recorder,
    );
    let elapsed = recorder.started.elapsed();
    let result = result.expect("synthetic stage query");
    assert_eq!(catalogue.generation(), generation);
    let report = catalogue.ranges.as_ref().expect("index").last_query;
    let phases = catalogue.ranges.as_ref().expect("index").last_phases;
    let sample = recorder.finish(elapsed, &result, report, phases);
    (result, sample)
}

fn measure_stages(
    catalogue: &mut Catalogue,
    original: &Entry,
    revision: &mut usize,
    query: &str,
    options: SearchPolicy,
) -> [Vec<Sample>; 2] {
    let mut samples: [Vec<_>; 2] = std::array::from_fn(|_| Vec::new());
    for pass in 0..7 {
        *revision += 1;
        let mut changed = original.clone();
        changed.title.push_str(&format!(" revision-{revision}"));
        catalogue.upsert(changed).expect("paired workload revision");
        let generation = catalogue.generation();
        let expected = catalogue
            .query_exhaustive(query, Scope::Everything, NOW, options)
            .expect("synthetic exhaustive result");
        let mut reports = [QueryReport::default(); 2];
        let mut counters = [Counters::default(); 2];
        for metadata in if pass % 2 == 0 {
            [false, true]
        } else {
            [true, false]
        } {
            let (actual, sample) = query_sample(catalogue, query, options, metadata);
            assert_eq!(actual, expected, "same-generation complete stage result");
            assert_eq!(catalogue.generation(), generation);
            let mode = usize::from(metadata);
            reports[mode] = sample.report;
            counters[mode] = sample.counters;
            if pass >= 2 {
                samples[mode].push(sample);
            }
        }
        compare_reports(reports[0], reports[1]);
        assert_eq!(counters[0], counters[1], "identical score and trace work");
    }
    samples
}

fn print_samples(mixed: bool, label: &str, mode: &str, mut samples: Vec<Sample>) {
    samples.sort_unstable_by_key(|sample| sample.total);
    assert_eq!(samples.len(), 5);
    let sample = &samples[2];
    let millis = |duration: Duration| duration.as_secs_f64() * 1_000.;
    let stages = sample.stages.map(millis);
    let report = sample.report;
    if let Some(phases) = sample.phases {
        println!(
            "  median_sample_range_phases mixed={mixed} case={label} mode={mode} candidates_ms={:.3} expansion_ms={:.3} combined_remainder_ms={:.3}",
            millis(phases.candidates),
            millis(phases.expansion),
            millis(sample.stages[1] - phases.candidates - phases.expansion)
        );
    }
    println!(
        "range_stages mixed={mixed} case={label} mode={mode} route={:?} min_ms={:.3} median_ms={:.3} max_ms={:.3} setup_ms={:.3} membership_or_combined_range_ms={:.3} scoring_or_range_cleanup_ms={:.3} ordering_ms={:.3} highlights_owned_ms={:.3} outside_stage_ms={:.3}",
        report.route,
        millis(samples[0].total),
        millis(sample.total),
        millis(samples[4].total),
        stages[0],
        stages[1],
        stages[2],
        stages[3],
        stages[4],
        millis(sample.total - sample.stages.iter().sum::<Duration>())
    );
    println!(
        "  median_sample_work matching={} hits={} available={} attempted={} work_recorded={} pairs_examined={} fallback_examined={} pair_scores={} range_leaves={} fallback_scores={} pops={} bounds={} bound_lookups={} counters={:?}",
        sample.matches,
        sample.hits,
        report.available,
        report.attempted,
        report.work_recorded,
        report.pairs_examined,
        report.fallback_examined,
        report.pairs_scored,
        report.range_leaves_ranked,
        report.fallback_rows_scored,
        report.frontier_pops,
        report.bound_constructions,
        report.bound_position_lookups,
        sample.counters
    );
}

#[test]
fn stage_recorder_preserves_range_and_ordinary_results() {
    let mut catalogue = mutation_catalogue();
    for (query, expected_route) in [("project", Route::Ranges), ("001", Route::DigitQuery)] {
        let expected = catalogue
            .query_exhaustive(query, Scope::Everything, NOW, policy())
            .expect("synthetic stage reference");
        for metadata in [false, true] {
            let (actual, sample) = query_sample(&mut catalogue, query, policy(), metadata);
            assert_eq!(actual, expected);
            assert_eq!(sample.report.route, expected_route);
        }
    }
}

fn range_query(
    catalogue: &mut Catalogue,
    text: &str,
    options: SearchPolicy,
) -> Result<SearchResults, SearchError> {
    catalogue.query_with_strategy(
        text,
        Scope::Everything,
        NOW,
        options,
        QueryStrategy::Ranges,
        &mut Unobserved,
    )
}

fn prime_phases(catalogue: &mut Catalogue) {
    range_query(catalogue, "project", policy()).expect("successful timing primer");
    assert!(
        catalogue
            .ranges
            .as_ref()
            .expect("index")
            .last_phases
            .is_some()
    );
}

#[test]
fn phase_timing_is_opt_in_and_clears_on_bypass_and_error() {
    let mut catalogue = mutation_catalogue();
    let expected = range_query(&mut catalogue, "project", policy()).expect("untimed range");
    let baseline = catalogue.ranges.as_ref().expect("index").last_query;
    assert!(
        catalogue
            .ranges
            .as_ref()
            .expect("index")
            .last_phases
            .is_none()
    );
    catalogue
        .ranges
        .as_mut()
        .expect("index")
        .set_phase_timing_enabled(true);
    let actual = range_query(&mut catalogue, "project", policy()).expect("timed range");
    assert_eq!(actual, expected);
    assert_eq!(
        catalogue.ranges.as_ref().expect("index").last_query,
        baseline
    );
    assert!(
        catalogue
            .ranges
            .as_ref()
            .expect("index")
            .last_phases
            .is_some()
    );

    for (query, route, expect_too_long) in [
        ("001".to_owned(), Route::DigitQuery, false),
        (String::new(), Route::BlankQuery, false),
        (
            "x".repeat(frizbee::Scoring::default().max_needle_len() + 1),
            Route::NotAttempted,
            true,
        ),
    ] {
        prime_phases(&mut catalogue);
        let expected = catalogue.query_exhaustive(&query, Scope::Everything, NOW, policy());
        let actual = range_query(&mut catalogue, &query, policy());
        if expect_too_long {
            assert_eq!(actual, Err(SearchError::QueryTooLong));
        } else {
            assert_eq!(
                actual.as_ref().expect("valid bypass"),
                expected.as_ref().expect("valid reference")
            );
        }
        assert_eq!(actual, expected);
        let index = catalogue.ranges.as_ref().expect("index");
        assert_eq!(index.last_query.route, route);
        assert!(index.last_phases.is_none());
    }

    prime_phases(&mut catalogue);
    let original = catalogue.field_groups[0];
    catalogue.field_groups[0][0] = crate::field_groups::GroupId::new(u32::MAX);
    assert_eq!(
        range_query(&mut catalogue, "project", policy()),
        Err(SearchError::InconsistentMatch)
    );
    let index = catalogue.ranges.as_ref().expect("index");
    assert_eq!(index.last_query.route, Route::Error);
    assert!(index.last_phases.is_none());
    catalogue.field_groups[0] = original;
    prime_phases(&mut catalogue);
    catalogue
        .ranges
        .as_mut()
        .expect("index")
        .set_phase_timing_enabled(false);
    assert_eq!(
        range_query(&mut catalogue, "project", policy()).expect("disabled again"),
        expected
    );
    assert!(
        catalogue
            .ranges
            .as_ref()
            .expect("index")
            .last_phases
            .is_none()
    );
}

#[test]
fn phase_timing_discards_completed_candidates_after_frontier_restart() {
    let mut catalogue = Catalogue::new();
    for id in 1..=8 {
        catalogue
            .upsert(entry(id * 4, "project same", ""))
            .expect("one bucket");
    }
    let mut fallback = entry(5, &format!("project{}", "x".repeat(1_025)), "");
    fallback.open = true;
    catalogue.upsert(fallback).expect("strong fallback");
    enable(&mut catalogue, Limits::default());
    catalogue
        .ranges
        .as_mut()
        .expect("index")
        .set_phase_timing_enabled(true);
    let options = SearchPolicy {
        limit: 3,
        boosts: BoostWeights {
            open: u16::MAX,
            ..policy().boosts
        },
        ..policy()
    };
    for (frontier, route) in [(0, Route::FrontierAdmission), (2, Route::FrontierExpansion)] {
        catalogue.ranges.as_mut().expect("index").limits.frontier = Limits::default().frontier;
        prime_phases(&mut catalogue);
        catalogue.ranges.as_mut().expect("index").limits.frontier = frontier;
        let expected = catalogue
            .query_exhaustive("project", Scope::Everything, NOW, options)
            .expect("reference");
        let mut recorder = Recorder::new();
        let actual = catalogue
            .query_with_strategy(
                "project",
                Scope::Everything,
                NOW,
                options,
                QueryStrategy::Ranges,
                &mut recorder,
            )
            .expect("fallback result");
        let elapsed = recorder.started.elapsed();
        assert_eq!(actual, expected);
        let index = catalogue.ranges.as_ref().expect("index");
        assert_eq!(index.last_query.route, route);
        assert!(index.last_phases.is_none());
        recorder.finish(elapsed, &actual, index.last_query, index.last_phases);
    }
}

#[test]
fn range_stage_workload_diagnostic_preserves_complete_results() {
    println!(
        "Test-only instrumented stage attribution, not D48 acceptance. Both modes share the position-free frontier layout; legacy means entry-based bound construction only. Observer counters, assertions and opt-in phase clock calls are inside total query timing. Candidate timing covers range_candidates; expansion covers the pending loop and final route report. Search allocation and remaining combined-stage work appear in combined_remainder. Phase times are published only for a successful range ranking attempt, independently of later highlights. Reuses unchanged existing/mixed100k fixtures and fourteen cases, two warmups and five retained same-generation pairs in alternating mode order. Revisions and exhaustive equality are outside timing. All stages, phases and work counters belong to the median-total sample; min/max show total spread. Successful ranges report combined membership/ranking then post-range handoff/cache cleanup; ordinary fallback reports membership then exact scoring. Highlight stage includes owned result construction. No timing thresholds or range 2*scored field invariant."
    );
    for mixed in [false, true] {
        let mut catalogue = workload_catalogue(mixed);
        enable(&mut catalogue, Limits::default());
        let target = Target::Command(
            NonZeroU64::new(diagnostic::ROWS as u64).expect("last synthetic target"),
        );
        let original = catalogue.get(target).expect("revision row").clone();
        let mut revision = 0;
        for (label, query) in diagnostic::cases() {
            let options = SearchPolicy {
                max_typos: u16::from(query.chars().count() >= 4),
                ..policy()
            };
            let samples = measure_stages(&mut catalogue, &original, &mut revision, &query, options);
            let expected_route = if crate::field_groups::query_safe(&query) {
                Route::Ranges
            } else {
                Route::DigitQuery
            };
            for (mode, samples) in ["legacy", "metadata"].into_iter().zip(samples) {
                assert!(
                    samples
                        .iter()
                        .all(|sample| sample.report.route == expected_route)
                );
                print_samples(mixed, label, mode, samples);
            }
        }
        check(&catalogue);
    }
}
