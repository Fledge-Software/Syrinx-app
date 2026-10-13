//! Profiles and compares exact search paths on the unchanged acceptance workload.

use std::{hint::black_box, time::Instant};

use super::{Catalogue, QueryStrategy, Unobserved};
use crate::{BlankQuery, BoostWeights, Entry, Scope, SearchPolicy, SearchResults, Target, Usage};

#[path = "../../../xtask/src/benchmark/workload.rs"]
mod workload;
use workload::*;

#[test]
fn partitioned_unchanged_acceptance_workload_comparison() {
    let mut catalogue = build_catalogue().expect("unchanged acceptance corpus");
    let cases = cases();
    let mut revision = 0;
    let mut samples = Vec::new();
    for iteration in 0..WARMUPS + PASSES {
        let pass = if iteration < WARMUPS {
            iteration
        } else {
            iteration - WARMUPS
        };
        for offset in 0..cases.len() {
            let case = &cases[case_index(offset, pass, cases.len())];
            refresh_generation(&mut catalogue, &mut revision).expect("synthetic revision");
            let policy = policy(&case.text).expect("unchanged policy");
            let candidate_first = pass % 2 == 1;
            let mut run = |candidate| {
                let started = Instant::now();
                let result = if candidate {
                    catalogue.query(black_box(case.text.as_str()), case.scope, NOW, policy)
                } else {
                    catalogue.query_with_strategy(
                        black_box(case.text.as_str()),
                        case.scope,
                        NOW,
                        policy,
                        QueryStrategy::Bounded,
                        &mut Unobserved,
                    )
                }
                .expect("complete query");
                (result, started.elapsed().as_nanos())
            };
            let (first, first_ns) = run(candidate_first);
            let (second, second_ns) = run(!candidate_first);
            assert_eq!(first, second, "partition parity for {}", case.label);
            assert_eq!(first.generation, catalogue.generation());
            if iteration == 0 {
                assert_eq!(
                    first,
                    catalogue
                        .query_exhaustive(&case.text, case.scope, NOW, policy,)
                        .expect("exhaustive warmup")
                );
            }
            if iteration >= WARMUPS {
                let (baseline_ns, candidate_ns) = if candidate_first {
                    (second_ns, first_ns)
                } else {
                    (first_ns, second_ns)
                };
                samples.push((
                    case.label,
                    pass,
                    candidate_first,
                    first.generation,
                    baseline_ns,
                    candidate_ns,
                ));
            }
        }
    }
    assert_eq!(samples.len(), 1_040);
    assert_eq!(revision, ((WARMUPS + PASSES) * cases.len()) as u64);
    for (label, pass, candidate_first, generation, baseline_ns, candidate_ns) in samples {
        println!(
            "d48_partition_pair case={label} pass={pass} candidate_first={candidate_first} generation={generation} baseline_ns={baseline_ns} candidate_ns={candidate_ns}"
        );
    }
}

// Attribute the actual acceptance cases without treating a busy-machine profile as acceptance.
#[test]
fn complete_workload_stage_profile() {
    use super::{QueryObserver, QueryStage};
    struct Stages {
        previous: Instant,
        elapsed: [u128; 5],
        candidates: usize,
        scored: usize,
    }
    impl QueryObserver for Stages {
        fn stage_finished(&mut self, stage: QueryStage) {
            let index = match stage {
                QueryStage::Setup => 0,
                QueryStage::MembershipAndCandidates => 1,
                QueryStage::HeapAndExactScoring => 2,
                QueryStage::FinalistOrdering => 3,
                QueryStage::HighlightsAndOwnedResults => 4,
                QueryStage::RangeMembershipAndRanking => unreachable!("bounded profile"),
            };
            let now = Instant::now();
            self.elapsed[index] = now.duration_since(self.previous).as_nanos();
            self.previous = now;
        }
        fn candidates(&mut self, count: usize) {
            self.candidates = count;
        }
        fn candidate_scored(&mut self) {
            self.scored += 1;
        }
    }
    let mut catalogue = build_catalogue().expect("synthetic acceptance corpus");
    let cases = cases();
    let mut revision = 0;
    let mut samples = Vec::new();
    for pass in 0..5 {
        for offset in 0..cases.len() {
            let case = &cases[case_index(offset, pass, cases.len())];
            refresh_generation(&mut catalogue, &mut revision).expect("synthetic mutation");
            let policy = policy(&case.text).expect("synthetic policy");
            let mut stages = Stages {
                previous: Instant::now(),
                elapsed: [0; 5],
                candidates: 0,
                scored: 0,
            };
            let actual = catalogue
                .query_with_strategy(
                    &case.text,
                    case.scope,
                    NOW,
                    policy,
                    QueryStrategy::Bounded,
                    &mut stages,
                )
                .expect("profiled query");
            assert_eq!(
                actual,
                catalogue
                    .query(&case.text, case.scope, NOW, policy)
                    .expect("production query")
            );
            samples.push((case.label, pass, stages));
        }
    }
    for (label, pass, stages) in samples {
        println!(
            "d48_stage case={label} pass={pass} setup_ns={} membership_ns={} ranking_ns={} ordering_ns={} highlights_ns={} candidates={} scored={}",
            stages.elapsed[0],
            stages.elapsed[1],
            stages.elapsed[2],
            stages.elapsed[3],
            stages.elapsed[4],
            stages.candidates,
            stages.scored
        );
    }
}

struct Pair {
    case_index: usize,
    pass: usize,
    generation: u64,
    candidate_first: bool,
    baseline_ns: u128,
    candidate_ns: u128,
    counts: crate::ScopeCounts,
    hits: usize,
}

fn sample(
    catalogue: &mut Catalogue,
    case: &Case,
    policy: SearchPolicy,
    candidate: bool,
) -> (SearchResults, u128) {
    let started = Instant::now();
    let result = if candidate {
        // Include Unicode selection in the timed candidate path.
        let query = black_box(case.text.as_str());
        black_box(catalogue.query_with_strategy(
            query,
            case.scope,
            NOW,
            policy,
            QueryStrategy::MembershipOrder(!query.is_ascii()),
            &mut Unobserved,
        ))
    } else {
        black_box(catalogue.query(black_box(case.text.as_str()), case.scope, NOW, policy))
    };
    let elapsed = started.elapsed().as_nanos();
    (result.expect("complete acceptance query"), elapsed)
}

fn paired_pass(
    catalogue: &mut Catalogue,
    cases: &[Case],
    pass: usize,
    revision: &mut u64,
    exhaustive: bool,
) -> Vec<Pair> {
    let mut pairs = Vec::with_capacity(cases.len());
    for offset in 0..cases.len() {
        let case_index = workload::case_index(offset, pass, cases.len());
        refresh_generation(catalogue, revision).expect("unchanged workload revision");
        let generation = catalogue.generation();
        let case = &cases[case_index];
        let policy = policy(&case.text).expect("unchanged workload policy");
        let candidate_first = pass % 2 == 1;
        let (baseline, baseline_ns, candidate, candidate_ns) = if candidate_first {
            let (candidate, candidate_ns) = sample(catalogue, case, policy, true);
            let (baseline, baseline_ns) = sample(catalogue, case, policy, false);
            (baseline, baseline_ns, candidate, candidate_ns)
        } else {
            let (baseline, baseline_ns) = sample(catalogue, case, policy, false);
            let (candidate, candidate_ns) = sample(catalogue, case, policy, true);
            (baseline, baseline_ns, candidate, candidate_ns)
        };
        // Full equality includes every scope count, score, target, original string and highlight.
        assert_eq!(candidate, baseline, "complete parity for {}", case.label);
        assert_eq!(baseline.generation, generation);
        assert_eq!(catalogue.generation(), generation);
        if exhaustive {
            assert_eq!(
                baseline,
                catalogue
                    .query_exhaustive(&case.text, case.scope, NOW, policy)
                    .expect("exhaustive warmup oracle"),
                "exhaustive parity for {}",
                case.label
            );
        }
        pairs.push(Pair {
            case_index,
            pass,
            generation,
            candidate_first,
            baseline_ns,
            candidate_ns,
            counts: baseline.counts,
            hits: baseline.hits.len(),
        });
    }
    pairs
}

#[test]
fn unicode_selected_unchanged_acceptance_workload_comparison() {
    let mut catalogue = build_catalogue().expect("unchanged acceptance corpus");
    let cases = cases();
    assert_eq!(catalogue.len(), ROWS);
    assert_eq!(cases.len(), 26);
    let mut revision = 0;
    for pass in 0..WARMUPS {
        drop(paired_pass(
            &mut catalogue,
            &cases,
            pass,
            &mut revision,
            pass == 0,
        ));
    }
    // Reset pass rotation exactly as the ordinary benchmark does after its warmups.
    let mut pairs = Vec::with_capacity(cases.len() * PASSES);
    for pass in 0..PASSES {
        pairs.extend(paired_pass(
            &mut catalogue,
            &cases,
            pass,
            &mut revision,
            false,
        ));
    }
    assert_eq!(pairs.len(), 1_040);
    assert_eq!(revision, ((WARMUPS + PASSES) * cases.len()) as u64);
    for case_index in 0..cases.len() {
        assert_eq!(
            pairs
                .iter()
                .filter(|pair| pair.case_index == case_index && pair.candidate_first)
                .count(),
            PASSES / 2
        );
        assert_eq!(
            pairs
                .iter()
                .filter(|pair| pair.case_index == case_index && !pair.candidate_first)
                .count(),
            PASSES / 2
        );
    }
    // Print after every measurement, preserving all samples without interleaved output work.
    println!(
        "Unicode-selected diagnostic versus actual production query, not production promotion or three-run D48 acceptance. Shared unchanged xtask workload: rows={ROWS} tabs={TABS} saved={SAVED} history={HISTORY} commands={COMMANDS} now={NOW} limit={LIMIT} warmups={WARMUPS} passes={PASSES} budget_ns={BUDGET_NS}. Full query setup and owned results timed; fixture construction, policy preparation, revision, validation, output and result destruction excluded. One refresh per same-generation pair; each mode first20 times per case. First warmup also checks exhaustive results outside timing. All1040pairs retained; no timing assertion, failures remain data."
    );
    for pair in pairs {
        let case = &cases[pair.case_index];
        println!(
            "unicode_acceptance_pair case={} scope={:?} pass={} generation={} first={} baseline_ns={} candidate_ns={} tabs={} saved={} history={} commands={} hits={}",
            case.label,
            case.scope,
            pair.pass,
            pair.generation,
            if pair.candidate_first {
                "candidate"
            } else {
                "baseline"
            },
            pair.baseline_ns,
            pair.candidate_ns,
            pair.counts.tabs,
            pair.counts.saved,
            pair.counts.history,
            pair.counts.commands,
            pair.hits
        );
    }
}
