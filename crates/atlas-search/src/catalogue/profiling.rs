//! Attributes synthetic query cost to actual search stages while checking exhaustive equivalence.

use std::{
    num::NonZeroU64,
    time::{Duration, Instant},
};

use super::{Catalogue, QueryObserver, QueryStage, QueryStrategy};
use crate::{BlankQuery, BoostWeights, Entry, Scope, SearchPolicy, SearchResults, Target, Usage};

const ROWS: usize = 100_000;
const COMMANDS: usize = 64;
const NOW: i64 = 1_800_000_000;
const STAGES: usize = 5;
const PASSES: usize = 5;
const LONG_FRAGMENT: &str = "reproducible synthetic catalogue exploration includes detailed release notes and project reference material for keyboard navigation";

struct Recorder {
    started: Instant,
    previous: Instant,
    stages: [Duration; STAGES],
    seen: [bool; STAGES],
    candidates: usize,
    scored: usize,
    computed_fields: usize,
    reused_fields: usize,
    computed_traces: usize,
    reused_traces: usize,
}

impl Recorder {
    fn new() -> Self {
        let started = Instant::now();
        Self {
            started,
            previous: started,
            stages: [Duration::ZERO; STAGES],
            seen: [false; STAGES],
            candidates: 0,
            scored: 0,
            computed_fields: 0,
            reused_fields: 0,
            computed_traces: 0,
            reused_traces: 0,
        }
    }

    fn sample(&self, result: &SearchResults, total: Duration) -> Sample {
        assert!(self.seen.into_iter().all(|seen| seen));
        assert!(self.scored <= self.candidates);
        assert_eq!(self.computed_fields + self.reused_fields, 2 * self.scored);
        assert!(self.computed_traces <= 4 * result.hits.len());
        assert!(self.reused_traces <= 2 * result.hits.len());
        assert!(self.stages.iter().sum::<Duration>() <= total);
        Sample {
            stages: self.stages,
            total,
            matches: result.counts.total(),
            candidates: self.candidates,
            scored: self.scored,
            computed_fields: self.computed_fields,
            reused_fields: self.reused_fields,
            computed_traces: self.computed_traces,
            reused_traces: self.reused_traces,
            hits: result.hits.len(),
            best_score: result.hits.first().map(|hit| hit.score),
            worst_score: result.hits.last().map(|hit| hit.score),
        }
    }
}

impl QueryObserver for Recorder {
    fn stage_finished(&mut self, stage: QueryStage) {
        let now = Instant::now();
        let index = match stage {
            QueryStage::Setup => 0,
            QueryStage::MembershipAndCandidates => 1,
            QueryStage::HeapAndExactScoring => 2,
            QueryStage::FinalistOrdering => 3,
            QueryStage::HighlightsAndOwnedResults => 4,
            QueryStage::RangeMembershipAndRanking => {
                unreachable!("bounded-stage diagnostic does not run range queries")
            }
        };
        assert!(!self.seen[index], "synthetic stage completed twice");
        self.stages[index] = now.duration_since(self.previous);
        self.seen[index] = true;
        self.previous = now;
    }

    fn candidates(&mut self, count: usize) {
        self.candidates = count;
    }

    fn candidate_scored(&mut self) {
        self.scored += 1;
    }

    fn field_scores(&mut self, computed: usize, reused: usize) {
        self.computed_fields = computed;
        self.reused_fields = reused;
    }

    fn highlight_traces(&mut self, computed: usize, reused: usize) {
        self.computed_traces = computed;
        self.reused_traces = reused;
    }
}

struct Sample {
    stages: [Duration; STAGES],
    total: Duration,
    matches: usize,
    candidates: usize,
    scored: usize,
    computed_fields: usize,
    reused_fields: usize,
    computed_traces: usize,
    reused_traces: usize,
    hits: usize,
    best_score: Option<u32>,
    worst_score: Option<u32>,
}

struct Case {
    label: &'static str,
    query: String,
    scope: Scope,
    typos: u16,
}

fn policy(typos: u16) -> SearchPolicy {
    SearchPolicy {
        limit: 50,
        max_typos: typos,
        blank: BlankQuery::All,
        boosts: BoostWeights {
            open: 120,
            frequency: 30,
            recency: 30,
            recency_window_secs: NonZeroU64::new(604_800).expect("synthetic nonzero window"),
        },
    }
}

fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (label, query, typos) in [
        ("broad", "a".to_owned(), 0),
        ("word", "project".to_owned(), 1),
        ("unicode", "café".to_owned(), 1),
        ("long", "reference ".repeat(10), 1),
        ("long-near-match", format!("{}xyz", &LONG_FRAGMENT[..93]), 1),
    ] {
        for scope in [Scope::Everything, Scope::Commands] {
            cases.push(Case {
                label,
                query: query.clone(),
                scope,
                typos,
            });
        }
    }
    cases
}

fn catalogue() -> Catalogue {
    // This diagnostic corpus is separate from xtask's acceptance workload.
    let long = "reference ".repeat(32);
    let mut catalogue = Catalogue::new();
    for index in 0..ROWS {
        let id = NonZeroU64::new(index as u64 + 1).expect("synthetic identity");
        let target = if index >= ROWS - COMMANDS {
            Target::Command(id)
        } else {
            Target::History(id)
        };
        let title = match index % 10 {
            0 => format!("synthetic project {long}"),
            1 | 2 => format!("synthetic project café 東京 archive {index:06}"),
            3 => format!("synthetic project row-{index:06} {LONG_FRAGMENT} {LONG_FRAGMENT}"),
            _ => format!("synthetic project catalogue reference row {index:06}"),
        };
        catalogue
            .upsert(Entry {
                target,
                title,
                address: format!("https://stage.example.test/reference/{index:06}"),
                usage: Usage {
                    visits: (index % 1_024) as u32,
                    last_used: Some(NOW - (index % 10) as i64 * 86_400),
                },
                open: index.is_multiple_of(17),
            })
            .expect("insert synthetic profiling row");
    }
    assert_eq!(catalogue.len(), ROWS);
    catalogue
}

fn run_case(catalogue: &mut Catalogue, case: &Case, expected: &SearchResults) -> Sample {
    let mut recorder = Recorder::new();
    let actual = catalogue.query_with_strategy(
        &case.query,
        case.scope,
        NOW,
        policy(case.typos),
        QueryStrategy::Bounded,
        &mut recorder,
    );
    let elapsed = recorder.started.elapsed();

    // Complete result equality and reporting are outside every timed stage.
    let actual = actual.expect("synthetic profiled query succeeds");
    assert_eq!(
        &actual, expected,
        "synthetic profile differs from exhaustive"
    );
    recorder.sample(&actual, elapsed)
}

fn median(mut durations: Vec<Duration>) -> f64 {
    assert_eq!(durations.len(), PASSES);
    durations.sort_unstable();
    durations[PASSES / 2].as_secs_f64() * 1_000.0
}

fn report(cases: &[Case], samples: &[Vec<Sample>]) {
    println!(
        "Synthetic stage attribution only; not the acceptance benchmark. Durations are median ms of five passes."
    );
    println!(
        "case scope matches candidates scored_rows computed_fields reused_fields computed_traces reused_traces hits best_score worst_score setup membership_candidates heap_scoring ordering highlights total"
    );
    for (case, samples) in cases.iter().zip(samples) {
        assert_eq!(samples.len(), PASSES);
        let first = &samples[0];
        assert!(samples.iter().all(|sample| {
            (
                sample.matches,
                sample.candidates,
                sample.scored,
                sample.computed_fields,
                sample.reused_fields,
                sample.computed_traces,
                sample.reused_traces,
                sample.hits,
                sample.best_score,
                sample.worst_score,
            ) == (
                first.matches,
                first.candidates,
                first.scored,
                first.computed_fields,
                first.reused_fields,
                first.computed_traces,
                first.reused_traces,
                first.hits,
                first.best_score,
                first.worst_score,
            )
        }));
        let stages: [f64; STAGES] = std::array::from_fn(|stage| {
            median(samples.iter().map(|sample| sample.stages[stage]).collect())
        });
        let total = median(samples.iter().map(|sample| sample.total).collect());
        println!(
            "{} {:?} {} {} {} {} {} {} {} {} {:?} {:?} {:.3} {:.3} {:.3} {:.3} {:.3} {:.3}",
            case.label,
            case.scope,
            first.matches,
            first.candidates,
            first.scored,
            first.computed_fields,
            first.reused_fields,
            first.computed_traces,
            first.reused_traces,
            first.hits,
            first.best_score,
            first.worst_score,
            stages[0],
            stages[1],
            stages[2],
            stages[3],
            stages[4],
            total,
        );
    }
}

#[test]
fn synthetic_stage_profile_preserves_exhaustive_results() {
    let mut catalogue = catalogue();
    let cases = cases();
    let expected: Vec<_> = cases
        .iter()
        .map(|case| {
            catalogue
                .query_exhaustive(&case.query, case.scope, NOW, policy(case.typos))
                .expect("synthetic exhaustive query succeeds")
        })
        .collect();
    // Every diagnostic exercises matching and scoring, including the accepted near match.
    assert!(
        expected
            .iter()
            .all(|result| { result.counts.total() > 0 && !result.hits.is_empty() })
    );
    for pass in 0..2 {
        for offset in 0..cases.len() {
            let index = (offset + pass * 3) % cases.len();
            run_case(&mut catalogue, &cases[index], &expected[index]);
        }
    }

    let mut samples: Vec<Vec<Sample>> = (0..cases.len()).map(|_| Vec::new()).collect();
    for pass in 0..PASSES {
        for offset in 0..cases.len() {
            let index = (offset + pass * 3) % cases.len();
            samples[index].push(run_case(&mut catalogue, &cases[index], &expected[index]));
        }
    }
    report(&cases, &samples);
}
