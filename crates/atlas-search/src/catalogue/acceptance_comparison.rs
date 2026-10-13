//! Compares the test-only Unicode candidate with production on the unchanged acceptance workload.

use std::{hint::black_box, time::Instant};

use super::{Catalogue, QueryStrategy, Unobserved};
use crate::{BlankQuery, BoostWeights, Entry, Scope, SearchPolicy, SearchResults, Target, Usage};

#[path = "../../../xtask/src/benchmark/workload.rs"]
mod workload;
use workload::*;

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
