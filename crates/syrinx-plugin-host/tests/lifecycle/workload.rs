//! The synthetic workload behind the per-export deadlines and the queue size
//! (`docs/dev/plugin-runtime-bounds.md`). Run with `--nocapture` to print its figures.

use std::time::{Duration, Instant};

use syrinx_plugin_host::bindings::syrinx::plugin::types::Outcome;
use syrinx_plugin_host::deadline::Export;
use syrinx_plugin_host::test_host::TestHost;
use syrinx_plugin_host::{Runtime, limits};

use super::{add, answer, synthetic};
use crate::support::{self, FULL, fixture_grants, full_declarations};

/// Calls of each export the workload makes; activation and deactivation take a fresh instance.
const CALLS: usize = 200;
const INSTANCES: usize = 20;

/// Every export's slowest call must stay this far inside its deadline.
const HEADROOM: u32 = 10;

fn loaded() -> TestHost {
    TestHost::load_with_grants(&support::bytes(FULL), full_declarations(), fixture_grants())
        .expect("the full fixture loads")
}

fn activated() -> TestHost {
    let mut host = loaded();
    host.activate().expect("runs").expect("activates");
    host
}

/// The 99th percentile and the slowest of `samples`.
fn summary(mut samples: Vec<Duration>) -> (Duration, Duration) {
    samples.sort();
    let p99 = samples[samples.len() * 99 / 100];
    (p99, *samples.last().expect("samples"))
}

fn time(mut call: impl FnMut()) -> Duration {
    let started = Instant::now();
    call();
    started.elapsed()
}

/// One sample per call of `export`, as the workload makes them.
fn measure(export: Export) -> Vec<Duration> {
    match export {
        Export::Activate => {
            let runtime = Runtime::new().expect("engine");
            let prepared = TestHost::prepare(&runtime, &support::bytes(FULL)).expect("compiles");
            (0..INSTANCES)
                .map(|_| {
                    time(|| {
                        let mut host = TestHost::from_prepared(
                            &runtime,
                            &prepared,
                            full_declarations(),
                            fixture_grants(),
                        )
                        .expect("instantiates");
                        assert!(host.activate().expect("runs").is_ok());
                    })
                })
                .collect()
        }
        Export::Deactivate => (0..INSTANCES)
            .map(|_| {
                let mut host = activated();
                time(|| host.deactivate().expect("runs"))
            })
            .collect(),
        _ => {
            let mut host = activated();
            (0..CALLS).map(|_| one_call(&mut host, export)).collect()
        }
    }
}

/// One call of a per-call export; views it opened are closed again outside the timing.
fn one_call(host: &mut TestHost, export: Export) -> Duration {
    let took = timed_call(host, export);
    for view in host.open_views() {
        host.close_view(view).expect("closed");
    }
    took
}

/// Time one call of `export`, with any setup it needs outside the timing.
fn timed_call(host: &mut TestHost, export: Export) -> Duration {
    match export {
        Export::Run => time(|| assert!(host.run_command("test.echo").expect("runs").is_ok())),
        Export::FormFor => time(|| assert!(host.form_for("test.new-item").expect("runs").is_ok())),
        Export::Validate => time(|| {
            let values = [("name", "Ada")];
            assert!(
                host.validate_form("test.new-item", &values)
                    .expect("runs")
                    .is_ok()
            );
        }),
        Export::Submit => time(|| {
            host.submit("test.new-item", &[("name", "Ada")])
                .expect("runs");
        }),
        Export::Deliver => {
            let (request, _) = host
                .submit("test.new-item", &[("name", "later")])
                .expect("runs");
            time(|| {
                host.complete(request, Outcome::Done(None))
                    .expect("delivers")
            })
        }
        Export::Opened => time(|| assert!(host.run_command("test.open").expect("runs").is_ok())),
        Export::Message => {
            host.run_command("test.open").expect("runs").expect("opens");
            let view = *host.open_views().last().expect("open");
            host.queue_view_message(view, "ping", "{\"n\":1}")
                .expect("queued");
            time(|| host.run_deferred().expect("delivered"))
        }
        Export::Closed => {
            host.run_command("test.open").expect("runs").expect("opens");
            let view = *host.open_views().last().expect("open");
            time(|| host.close_view(view).expect("closed"))
        }
        Export::Changes => {
            time(|| assert!(host.changes("test.items", None, 64).expect("runs").is_ok()))
        }
        Export::Resolve => {
            time(|| assert!(host.resolve("test.items", "item-1").expect("runs").is_ok()))
        }
        Export::Activate | Export::Deactivate => unreachable!("measured on fresh instances"),
    }
}

#[test]
fn every_export_in_the_workload_finishes_well_inside_its_deadline() {
    println!("| export | deadline | p99 | slowest |");
    for export in Export::ALL {
        let (p99, slowest) = summary(measure(export));
        let deadline = export.deadline();
        println!(
            "| {} | {} ms | {:.3} ms | {:.3} ms |",
            export.name(),
            deadline.as_millis(),
            p99.as_secs_f64() * 1e3,
            slowest.as_secs_f64() * 1e3
        );
        // Wall-clock: the 99th percentile keeps the headroom, and one preempted call may
        // use it up but must still finish inside the deadline.
        assert!(
            p99 * HEADROOM <= deadline,
            "{} p99 {p99:?}, too close to its {deadline:?} deadline",
            export.name()
        );
        assert!(
            slowest <= deadline,
            "{} took {slowest:?}, past its {deadline:?} deadline",
            export.name()
        );
    }
}

/// A full queue of the workload's most frequent call, a form validation per keystroke, drains
/// inside one validation deadline, so a healthy plugin never fills its queue. The measured
/// figure (about 2 ms) is in the bounds document; the deadline itself leaves room for a
/// loaded machine.
#[test]
fn a_full_queue_of_workload_calls_drains_quickly() {
    let (mut plugins, _events) = synthetic("0.1.0-workload");
    add(&mut plugins, "test", "");
    assert!(answer(plugins.call("test", |session| session.run_command("test.fail"))).is_ok());
    let started = Instant::now();
    let queued: Vec<_> = (0..limits::PLUGIN_QUEUE_CAPACITY)
        .map(|_| {
            plugins.call("test", |session| {
                session.validate_form("test.new-item", &[("name", "Ada")])
            })
        })
        .collect();
    for call in queued {
        assert!(answer(call).is_ok());
    }
    let drained = started.elapsed();
    println!(
        "a full queue of {} validations drained in {:.3} ms",
        limits::PLUGIN_QUEUE_CAPACITY,
        drained.as_secs_f64() * 1e3
    );
    assert!(
        drained <= Export::Validate.deadline(),
        "drained in {drained:?}"
    );
}
