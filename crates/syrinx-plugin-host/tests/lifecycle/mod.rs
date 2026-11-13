//! Synthetic lifecycle tests for the per-plugin workers (#135, P2-03): lazy activation,
//! deadlines, traps, request identity, the bounded queue and deactivation.

mod grants;
mod services;
mod workload;

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use syrinx_core::CommandRegistry;
use syrinx_plugin_host::bindings::exports::syrinx::plugin::forms::SubmitResult;
use syrinx_plugin_host::bindings::syrinx::plugin::types::{Outcome, PluginError};
use syrinx_plugin_host::deadline::Export;
use syrinx_plugin_host::manifest::{CapabilityKind, DiscoveryOptions, PluginManifest};
use syrinx_plugin_host::session::CallResult;
use syrinx_plugin_host::test_host::{SyntheticGrants, SyntheticHost, TestHost};
use syrinx_plugin_host::worker::{Pending, PluginEvent, PluginStatus, Plugins, StopReason};
use syrinx_plugin_host::{Failure, HostError, PluginHost, PluginRegistry, Runtime, limits};
use wasmtime::Trap;

use crate::package::{manifest_text, register};
use crate::support::{self, FULL};
use services::{Journal, TestServices};

/// Long enough for the first activation to compile the fixture in a debug build.
pub(crate) const WAIT: Duration = Duration::from_secs(60);

/// Workers under `grants`, which the test keeps to grant and revoke.
fn granted_plugins<S: PluginHost>(
    grants: &SyntheticGrants,
    services: impl Fn(&PluginManifest) -> S + Send + Sync + 'static,
) -> (Plugins<S>, Receiver<PluginEvent>) {
    let grants = Arc::new(grants.clone());
    Plugins::new(
        Runtime::new().expect("engine"),
        "0.1.0-worker",
        grants,
        services,
    )
}

/// Workers with nothing granted.
fn plugins_with<S: PluginHost>(
    host_version: &str,
    services: impl Fn(&PluginManifest) -> S + Send + Sync + 'static,
) -> (Plugins<S>, Receiver<PluginEvent>) {
    let grants = Arc::new(SyntheticGrants::new());
    Plugins::new(
        Runtime::new().expect("engine"),
        host_version,
        grants,
        services,
    )
}

fn synthetic(host_version: &str) -> (Plugins<SyntheticHost>, Receiver<PluginEvent>) {
    plugins_with(host_version, |_manifest| SyntheticHost::new())
}

/// Register the full fixture as `id`, with `extra` appended to its manifest, and add it.
fn add<S: PluginHost>(plugins: &mut Plugins<S>, id: &str, extra: &str) {
    let text = manifest_text(id) + extra;
    let manifest =
        PluginManifest::parse(text.as_bytes(), DiscoveryOptions::default()).expect("manifest");
    let mut registry = PluginRegistry::new();
    let mut commands = CommandRegistry::browser();
    let registered = register(&mut registry, manifest, &mut commands).expect("registers");
    plugins
        .add(registered, Arc::from(support::bytes(FULL)))
        .expect("added");
}

pub(crate) fn answer<R>(queued: CallResult<Pending<R>>) -> CallResult<R> {
    queued
        .expect("queued")
        .wait(WAIT)
        .expect("answered in time")
}

fn run<S: PluginHost>(
    plugins: &mut Plugins<S>,
    id: &'static str,
) -> CallResult<Result<(), PluginError>> {
    answer(plugins.call("test", move |session| session.run_command(id)))
}

fn logged(plugins: &mut Plugins<SyntheticHost>) -> Vec<String> {
    let logs = answer(plugins.call("test", |session: &mut TestHost| Ok(session.logs().to_vec())));
    logs.expect("read")
        .into_iter()
        .map(|(_, message)| message)
        .collect()
}

/// Submit the fixture's form so that it answers `pending`; returns the request id.
fn submit_pending<S: PluginHost>(plugins: &mut Plugins<S>) -> u64 {
    let (request, result) = answer(plugins.call("test", |session| {
        session.submit("test.new-item", &[("name", "later")])
    }))
    .expect("submitted");
    assert_eq!(result, SubmitResult::Pending(request));
    request
}

fn next_event(events: &Receiver<PluginEvent>) -> PluginEvent {
    events.recv_timeout(WAIT).expect("an event")
}

/// The running instance's export calls so far, and its writes in flight.
fn instance_state<S: PluginHost>(plugins: &mut Plugins<S>) -> (u64, Vec<u64>) {
    answer(plugins.call("test", |session| {
        Ok((session.export_calls(), session.writes_in_flight()))
    }))
    .expect("read")
}

fn uncertain(request: u64) -> PluginEvent {
    PluginEvent::Outcome {
        plugin: "test".into(),
        request,
        outcome: Outcome::Uncertain("the plugin stopped before the outcome was known".into()),
    }
}

#[test]
fn nothing_runs_until_first_use_and_activation_gets_no_grants() {
    let (mut plugins, _events) = synthetic("0.1.0-worker");
    add(&mut plugins, "test", "");
    assert_eq!(plugins.status("test"), Some(PluginStatus::Inactive));
    assert_eq!(
        run(&mut plugins, "test.fail"),
        Ok(Err(PluginError::InvalidInput("synthetic failure".into())))
    );
    assert_eq!(plugins.status("test"), Some(PluginStatus::Active));
    assert_eq!(
        logged(&mut plugins).first().map(String::as_str),
        Some("activate 0.1.0-worker granted=0")
    );
}

#[test]
fn a_requested_service_answers_denied_until_it_is_granted() {
    let (mut plugins, _events) = synthetic("0.1.0-worker");
    add(&mut plugins, "test", "");
    let echoed = run(&mut plugins, "test.echo").expect("ran");
    assert!(
        matches!(echoed, Err(PluginError::Denied(ref reason)) if reason.contains("storage")),
        "{echoed:?}"
    );
    assert_eq!(plugins.status("test"), Some(PluginStatus::Active));
}

#[test]
fn a_late_completion_after_cancellation_is_never_delivered() {
    let (mut plugins, _events) = synthetic("0.1.0-worker");
    add(&mut plugins, "test", "");
    let request = submit_pending(&mut plugins);
    answer(plugins.call("test", move |session| {
        session.cancel_request(request);
        Ok(())
    }))
    .expect("cancelled");
    let late = answer(plugins.complete("test", request, Outcome::Done(None)));
    assert!(matches!(late, Err(HostError::Refused(_))), "{late:?}");
    assert!(
        !logged(&mut plugins)
            .iter()
            .any(|line| line.starts_with("deliver"))
    );
}

#[test]
fn a_closed_view_drops_its_messages_and_a_reopened_one_gets_new_request_ids() {
    let (mut plugins, _events) = synthetic("0.1.0-worker");
    add(&mut plugins, "test", "");
    assert_eq!(run(&mut plugins, "test.open"), Ok(Ok(())));
    let (first, reply) = answer(plugins.call("test", |session| {
        let first = session.queue_view_message(1, "ping", "{}")?;
        session.close_view(1)?;
        Ok((first, session.reply(first).cloned()))
    }))
    .expect("closed");
    assert_eq!(reply, None, "a message queued for a closed view is dropped");

    assert_eq!(run(&mut plugins, "test.open"), Ok(Ok(())));
    let (reopened, second, reply) = answer(plugins.call("test", |session| {
        let reopened = *session.open_views().last().expect("a view is open again");
        let second = session.queue_view_message(reopened, "ping", "{\"n\":2}")?;
        session.run_deferred()?;
        Ok((reopened, second, session.reply(second).cloned()))
    }))
    .expect("reopened");
    assert_ne!(reopened, 1);
    assert!(
        second > first,
        "request {second} reused an id at or below {first}"
    );
    assert_eq!(reply, Some(Ok("{\"echo\":{\"n\":2}}".into())));
}

#[test]
fn an_unavailable_host_service_answers_by_the_deadline_and_the_plugin_keeps_running() {
    let journal = Journal::default();
    let (mut plugins, _events) = plugins_with("0.1.0-worker", TestServices::stalling(&journal));
    add(&mut plugins, "test", "");
    let activated = answer(plugins.call("test", |session| session.run_command("test.fail")));
    assert!(activated.is_ok());
    let started = Instant::now();
    let opened = answer(plugins.call("test", |session| session.run_command("test.open")));
    assert!(
        matches!(opened, Ok(Err(PluginError::Unavailable(_)))),
        "{opened:?}"
    );
    assert!(started.elapsed() < Export::Run.deadline() * 3);
    assert_eq!(plugins.status("test"), Some(PluginStatus::Active));
    let failed = answer(plugins.call("test", |session| session.run_command("test.fail")));
    assert!(matches!(failed, Ok(Err(PluginError::InvalidInput(_)))));
}

#[test]
fn a_trap_settles_an_armed_write_as_uncertain_once_and_never_resubmits_it() {
    let (mut plugins, events) = synthetic("0.1.0-worker");
    add(&mut plugins, "test", "");
    let request = submit_pending(&mut plugins);

    let spun = run(&mut plugins, "test.spin");
    assert!(
        matches!(
            spun,
            Err(HostError::Trap {
                code: Some(Trap::Interrupt),
                ..
            })
        ),
        "{spun:?}"
    );
    assert_eq!(next_event(&events), uncertain(request));
    let failed = PluginEvent::Failed {
        plugin: "test".into(),
        failure: Failure::DeadlineExpired(Export::Run),
    };
    assert_eq!(next_event(&events), failed);
    assert_eq!(
        plugins.status("test"),
        Some(PluginStatus::Failed(Failure::DeadlineExpired(Export::Run)))
    );

    // A completion for the old instance is dropped, and the next explicit use starts a new one.
    assert!(
        plugins
            .complete("test", request, Outcome::Done(None))
            .is_err()
    );
    assert_eq!(
        run(&mut plugins, "test.fail").map(|ran| ran.is_err()),
        Ok(true)
    );
    let late = answer(plugins.complete("test", request, Outcome::Done(None)));
    assert!(matches!(late, Err(HostError::Refused(_))), "{late:?}");
    let lines = logged(&mut plugins);
    assert!(
        !lines.iter().any(|line| line.starts_with("deliver")),
        "{lines:?}"
    );
    assert!(
        events.try_recv().is_err(),
        "the write was settled exactly once"
    );
    let (calls, in_flight) = instance_state(&mut plugins);
    assert_eq!(
        calls, 2,
        "the new instance ran only lifecycle.activate and test.fail"
    );
    assert_eq!(in_flight, Vec::<u64>::new(), "nothing was carried over");
    let next = submit_pending(&mut plugins);
    assert!(
        next > request,
        "request ids are never reused after re-activation"
    );
}

#[test]
fn a_trap_during_activation_fails_the_plugin_and_answers_queued_calls() {
    let (mut plugins, events) = synthetic("0.1.0-panic-on-activate");
    add(&mut plugins, "test", "");
    let first = plugins
        .call("test", |session| session.run_command("test.fail"))
        .expect("queued");
    assert!(matches!(
        first.wait(WAIT),
        Some(Err(HostError::Unavailable(_)))
    ));
    let PluginEvent::Failed { failure, .. } = next_event(&events) else {
        panic!("expected a failure");
    };
    assert!(
        matches!(
            failure,
            Failure::Trapped {
                export: Export::Activate,
                code: Some(Trap::UnreachableCodeReached)
            }
        ),
        "{failure:?}"
    );
    assert!(matches!(
        plugins.status("test"),
        Some(PluginStatus::Failed(_))
    ));
    // Only the next explicit use tries again. Its call is refused at once if it reaches the
    // failing worker, or answered `unavailable` if it reaches the new one.
    assert!(events.recv_timeout(Duration::from_millis(200)).is_err());
    let again = plugins
        .call("test", |session| session.run_command("test.fail"))
        .and_then(|pending| pending.wait(WAIT).expect("answered in time"));
    assert!(matches!(again, Err(HostError::Unavailable(_))), "{again:?}");
    assert!(matches!(next_event(&events), PluginEvent::Failed { .. }));
}

#[test]
fn a_spinning_plugin_never_delays_the_caller_or_another_plugin() {
    let (mut plugins, _events) = synthetic("0.1.0-worker");
    add(&mut plugins, "test", "");
    add(&mut plugins, "other", "");
    assert!(run(&mut plugins, "test.fail").is_ok());
    assert!(answer(plugins.call("other", |session| session.run_command("other.fail"))).is_ok());

    let queued = Instant::now();
    let spinning = plugins
        .call("test", |session| session.run_command("test.spin"))
        .expect("queued");
    assert!(
        queued.elapsed() < Duration::from_millis(50),
        "queueing never waits for the plugin"
    );

    let mut slowest = Duration::ZERO;
    let mut calls_while_spinning = 0;
    while spinning.try_answer().is_none() {
        calls_while_spinning += 1;
        let started = Instant::now();
        let other = answer(plugins.call("other", |session| session.run_command("other.fail")));
        assert!(
            matches!(other, Ok(Err(PluginError::InvalidInput(_)))),
            "{other:?}"
        );
        slowest = slowest.max(started.elapsed());
    }
    assert!(
        calls_while_spinning >= 1,
        "the other plugin was called during the spin"
    );
    assert!(
        slowest < Duration::from_millis(250),
        "another plugin's call took {slowest:?}"
    );
}

#[test]
fn a_full_queue_is_unavailable_at_once() {
    let (mut plugins, _events) = synthetic("0.1.0-worker");
    add(&mut plugins, "test", "");
    assert!(run(&mut plugins, "test.fail").is_ok());
    let (started, spin_started) = mpsc::channel();
    let spinning = plugins
        .call("test", move |session| {
            started.send(()).expect("the test waits");
            session.run_command("test.spin")
        })
        .expect("queued");
    spin_started
        .recv_timeout(WAIT)
        .expect("the worker took the spin");
    let waiting: Vec<_> = (0..limits::PLUGIN_QUEUE_CAPACITY)
        .map(|_| {
            plugins
                .call("test", |session| session.run_command("test.fail"))
                .expect("room")
        })
        .collect();
    let started = Instant::now();
    let refused = plugins.call("test", |session| session.run_command("test.fail"));
    assert!(matches!(refused, Err(HostError::Unavailable(_))));
    assert!(started.elapsed() < Duration::from_millis(50));
    assert!(spinning.wait(WAIT).is_some_and(|answer| answer.is_err()));
    for queued in waiting {
        assert!(matches!(
            queued.wait(WAIT),
            Some(Err(HostError::Unavailable(_)))
        ));
    }
}

#[test]
fn the_epoch_does_not_tick_while_every_plugin_is_idle() {
    let runtime = Runtime::new().expect("engine");
    let (mut plugins, _events) = Plugins::new(
        runtime.clone(),
        "0.1.0-worker",
        Arc::new(SyntheticGrants::new()),
        |_manifest: &PluginManifest| SyntheticHost::new(),
    );
    add(&mut plugins, "test", "");
    add(&mut plugins, "other", "");
    assert!(run(&mut plugins, "test.fail").is_ok());
    assert!(answer(plugins.call("other", |session| session.run_command("other.fail"))).is_ok());
    let idle = runtime.epoch_ticks();
    std::thread::sleep(limits::EPOCH_TICK * 20);
    assert_eq!(
        runtime.epoch_ticks(),
        idle,
        "an idle Syrinx never wakes for the epoch"
    );
    assert!(run(&mut plugins, "test.spin").is_err());
    assert!(
        runtime.epoch_ticks() > idle,
        "a running call advances the epoch"
    );
}

#[test]
fn shutdown_deactivates_each_running_plugin_and_settles_its_writes() {
    let (mut plugins, events) = synthetic("0.1.0-worker");
    add(&mut plugins, "test", "");
    add(&mut plugins, "other", "");
    let request = submit_pending(&mut plugins);
    plugins.shutdown();
    let received: Vec<PluginEvent> = events.try_iter().collect();
    assert_eq!(
        received,
        [
            PluginEvent::Outcome {
                plugin: "test".into(),
                request,
                outcome: Outcome::Uncertain(
                    "the plugin stopped before the outcome was known".into()
                ),
            },
            PluginEvent::Deactivated {
                plugin: "test".into(),
                reason: StopReason::Shutdown,
            },
        ],
        "the never-used plugin had nothing to deactivate"
    );
}

#[test]
fn revoking_a_required_capability_deactivates_and_an_optional_one_does_not() {
    let capabilities = "[[capabilities]]\nkind = \"storage\"\nreason = \"Keeps synthetic settings.\"\nrequired = true\n\
                        [[capabilities]]\nkind = \"open-external\"\nreason = \"Opens a synthetic link.\"\nrequired = false\n";
    let grants = SyntheticGrants::new();
    grants.grant("test", CapabilityKind::Storage);
    grants.grant("test", CapabilityKind::OpenExternal);
    let (mut plugins, events) = granted_plugins(&grants, |_manifest| SyntheticHost::new());
    add(&mut plugins, "test", capabilities);
    assert!(run(&mut plugins, "test.fail").is_ok());
    grants.revoke("test", CapabilityKind::OpenExternal);
    plugins
        .revoke("test", CapabilityKind::OpenExternal)
        .expect("optional");
    plugins
        .revoke("test", CapabilityKind::Folder)
        .expect("not requested");
    assert!(
        run(&mut plugins, "test.fail").is_ok(),
        "the plugin still answers"
    );
    assert_eq!(plugins.status("test"), Some(PluginStatus::Active));
    assert!(events.try_recv().is_err(), "nothing was deactivated");
    grants.revoke("test", CapabilityKind::Storage);
    plugins
        .revoke("test", CapabilityKind::Storage)
        .expect("revoked");
    let reason = StopReason::CapabilityRevoked(CapabilityKind::Storage);
    assert_eq!(
        next_event(&events),
        PluginEvent::Deactivated {
            plugin: "test".into(),
            reason: reason.clone()
        }
    );
    assert_eq!(
        plugins.status("test"),
        Some(PluginStatus::Deactivated(reason))
    );
}

#[test]
fn uninstalling_deactivates_and_forgets_the_plugin() {
    let (mut plugins, events) = synthetic("0.1.0-worker");
    add(&mut plugins, "test", "");
    assert!(run(&mut plugins, "test.fail").is_ok());
    plugins.uninstall("test").expect("uninstalled");
    assert_eq!(plugins.status("test"), None);
    assert!(matches!(
        plugins.call("test", |session| session.run_command("test.fail")),
        Err(HostError::Refused(_))
    ));
    assert_eq!(
        next_event(&events),
        PluginEvent::Deactivated {
            plugin: "test".into(),
            reason: StopReason::Uninstalled
        }
    );
}

#[test]
fn a_trap_inside_submit_names_its_request_which_is_settled_uncertain_once() {
    let (mut plugins, events) = synthetic("0.1.0-worker");
    add(&mut plugins, "test", "");
    let submitted = answer(plugins.call("test", |session| {
        session.submit("test.new-item", &[("name", "panic")])
    }));
    let Err(HostError::SubmitStopped { request, cause }) = submitted else {
        panic!("expected the submit to stop, got {submitted:?}");
    };
    assert!(matches!(*cause, HostError::Trap { .. }), "{cause:?}");
    assert_eq!(next_event(&events), uncertain(request));
    let failed = PluginEvent::Failed {
        plugin: "test".into(),
        failure: Failure::Trapped {
            export: Export::Submit,
            code: Some(Trap::UnreachableCodeReached),
        },
    };
    assert_eq!(next_event(&events), failed);
    assert!(run(&mut plugins, "test.fail").is_ok());
    assert_eq!(
        instance_state(&mut plugins),
        (2, Vec::new()),
        "nothing was resubmitted"
    );
    assert!(
        events.try_recv().is_err(),
        "the write was settled exactly once"
    );
}

#[test]
fn a_job_that_keeps_deferring_slow_calls_is_stopped_at_its_budget() {
    let (mut plugins, events) = synthetic("0.1.0-worker");
    add(&mut plugins, "test", "");
    assert!(run(&mut plugins, "test.fail").is_ok());
    let started = Instant::now();
    assert!(run(&mut plugins, "test.slow-chain").is_err());
    let failed = PluginEvent::Failed {
        plugin: "test".into(),
        failure: Failure::OverBudget,
    };
    assert_eq!(next_event(&events), failed);
    let took = started.elapsed();
    assert!(took >= limits::JOB_BUDGET, "stopped after {took:?}");
    assert!(
        took < limits::JOB_BUDGET + Export::Opened.deadline() * 2,
        "stopped after {took:?}"
    );
}

#[test]
fn a_stop_request_interrupts_a_chain_of_deferred_calls() {
    let journal = Journal::default();
    let (mut plugins, events) = plugins_with("0.1.0-worker", TestServices::recording(&journal));
    add(&mut plugins, "test", "");
    assert!(answer(plugins.call("test", |session| session.run_command("test.fail"))).is_ok());
    let (sender, chain_started) = mpsc::channel();
    let chain = plugins
        .call("test", move |session| {
            sender.send(Instant::now()).expect("the test waits");
            session.run_command("test.slow-chain")
        })
        .expect("queued");
    let started = chain_started.recv_timeout(WAIT).expect("the chain started");
    // One link of the chain busy-waits 100 ms in `views.opened`.
    std::thread::sleep(Duration::from_millis(100));
    plugins
        .deactivate("test", StopReason::Shutdown)
        .expect("stopping");
    let deactivated = PluginEvent::Deactivated {
        plugin: "test".into(),
        reason: StopReason::Shutdown,
    };
    assert_eq!(next_event(&events), deactivated);
    assert!(
        started.elapsed() < limits::JOB_BUDGET,
        "the stop waited for the budget"
    );
    assert!(
        journal.count("opened") >= 1,
        "the chain was running: {:?}",
        journal.lines()
    );
    assert_eq!(
        journal.count("deactivate"),
        1,
        "lifecycle.deactivate still ran"
    );
    assert_eq!(
        chain.wait(WAIT),
        Some(Ok(Ok(()))),
        "the command itself had answered"
    );
}

#[test]
fn a_successor_stopped_while_its_predecessor_stops_never_activates() {
    let journal = Journal::default();
    let (mut plugins, events) = plugins_with("0.1.0-worker", TestServices::recording(&journal));
    add(&mut plugins, "test", "");
    // The predecessor's job holds it until the test releases it, so both stops land first.
    let (started, job_started) = mpsc::channel();
    let (release, released) = mpsc::channel::<()>();
    let held = plugins
        .call("test", move |session| {
            started.send(()).expect("the test waits");
            released
                .recv_timeout(WAIT)
                .expect("the test releases the job");
            session.run_command("test.fail")
        })
        .expect("queued");
    job_started.recv_timeout(WAIT).expect("the job started");
    plugins
        .deactivate("test", StopReason::Uninstalled)
        .expect("stopping the predecessor");
    let queued = plugins
        .call("test", |session| session.run_command("test.fail"))
        .expect("queued for the successor");
    plugins
        .deactivate("test", StopReason::Shutdown)
        .expect("stopping the successor");
    release.send(()).expect("the job waits");
    plugins.shutdown();
    assert_eq!(journal.count("activate"), 1, "{:?}", journal.lines());
    assert!(matches!(
        queued.wait(WAIT),
        Some(Err(HostError::Unavailable(_)))
    ));
    assert!(matches!(
        held.wait(WAIT),
        Some(Ok(Err(PluginError::InvalidInput(_))))
    ));
    let received: Vec<PluginEvent> = events.try_iter().collect();
    assert_eq!(
        received,
        [PluginEvent::Deactivated {
            plugin: "test".into(),
            reason: StopReason::Uninstalled,
        }],
        "only the instance that ran reports"
    );
}

#[test]
fn host_cancel_reaches_the_service_after_the_host_forgot_the_request() {
    let journal = Journal::default();
    let (mut plugins, _events) = plugins_with("0.1.0-worker", TestServices::recording(&journal));
    add(&mut plugins, "test", "");
    let (request, result) = answer(plugins.call("test", |session| {
        session.submit("test.new-item", &[("name", "later")])
    }))
    .expect("submitted");
    assert_eq!(
        (request, result),
        (1, SubmitResult::Pending(1)),
        "test.cancel cancels request 1"
    );
    let cancelled = answer(plugins.call("test", |session| session.run_command("test.cancel")));
    assert_eq!(cancelled, Ok(Ok(())));
    let in_flight =
        answer(plugins.call("test", |session| Ok(session.writes_in_flight()))).expect("read");
    assert_eq!(journal.count("cancel"), 1, "{:?}", journal.lines());
    assert!(
        journal.lines().contains(&"cancel 1".to_owned()),
        "the service saw the cancel"
    );
    assert_eq!(
        in_flight,
        Vec::<u64>::new(),
        "the host had already dropped the request"
    );
    assert_eq!(
        answer(plugins.complete("test", 1, Outcome::Done(None))),
        Ok(())
    );
    assert!(answer(plugins.call("test", |session| session.run_command("test.fail"))).is_ok());
    assert_eq!(journal.count("deliver"), 0, "{:?}", journal.lines());
}

#[test]
fn an_outcome_a_stop_dropped_is_not_reported_delivered_and_open_views_are_closed() {
    let journal = Journal::default();
    let (mut plugins, events) = plugins_with("0.1.0-worker", TestServices::recording(&journal));
    add(&mut plugins, "test", "");
    let (request, _) = answer(plugins.call("test", |session| {
        session.submit("test.new-item", &[("name", "later")])
    }))
    .expect("pending");
    let (sender, job_started) = mpsc::channel();
    let job = plugins
        .call("test", move |session| {
            sender.send(()).expect("the test waits");
            let chain = session.run_command("test.slow-chain");
            let completed = session.complete(request, Outcome::Done(None));
            Ok((chain, completed))
        })
        .expect("queued");
    job_started.recv_timeout(WAIT).expect("the job started");
    // One link of the chain busy-waits 100 ms; the stop lands while the chain runs.
    std::thread::sleep(Duration::from_millis(100));
    plugins
        .deactivate("test", StopReason::Shutdown)
        .expect("stopping");
    let (chain, completed) = job
        .wait(WAIT)
        .expect("answered in time")
        .expect("the job ran");
    assert_eq!(chain, Ok(Ok(())), "the command itself had answered");
    assert!(
        matches!(completed, Err(HostError::Unavailable(_))),
        "a dropped delivery is not a success: {completed:?}"
    );
    assert!(matches!(
        next_event(&events),
        PluginEvent::Deactivated { .. }
    ));
    assert_eq!(journal.count("deliver"), 0, "{:?}", journal.lines());
    assert!(
        journal.count("view-closed") >= 1,
        "views still open were closed: {:?}",
        journal.lines()
    );
}

#[test]
fn a_view_message_a_stop_dropped_is_answered_unavailable() {
    let journal = Journal::default();
    let (mut plugins, _events) = plugins_with("0.1.0-worker", TestServices::recording(&journal));
    add(&mut plugins, "test", "");
    let opened = answer(plugins.call("test", |session| session.run_command("test.open")));
    assert_eq!(opened, Ok(Ok(())));
    let (started, job_started) = mpsc::channel();
    let (stop_raised, wait_for_stop) = mpsc::channel::<()>();
    let job = plugins
        .call("test", move |session| {
            started.send(()).expect("the test waits");
            wait_for_stop
                .recv_timeout(WAIT)
                .expect("the test raised the stop");
            let message = session.queue_view_message(1, "ping", "{}")?;
            session.run_deferred()?;
            Ok(session.reply(message).cloned())
        })
        .expect("queued");
    job_started.recv_timeout(WAIT).expect("the job started");
    plugins
        .deactivate("test", StopReason::Shutdown)
        .expect("stopping");
    stop_raised.send(()).expect("the job waits");
    let reply = job
        .wait(WAIT)
        .expect("answered in time")
        .expect("the job ran");
    assert!(
        matches!(reply, Some(Err(PluginError::Unavailable(_)))),
        "a dropped message is answered: {reply:?}"
    );
    assert_eq!(journal.count("message"), 0, "{:?}", journal.lines());
}

#[test]
fn views_still_open_are_closed_with_the_services_after_a_trap() {
    let journal = Journal::default();
    let (mut plugins, events) = plugins_with("0.1.0-worker", TestServices::recording(&journal));
    add(&mut plugins, "test", "");
    let opened = answer(plugins.call("test", |session| session.run_command("test.open")));
    assert_eq!(opened, Ok(Ok(())));
    assert_eq!(journal.count("view-closed 1"), 0, "view 1 is open");
    assert!(answer(plugins.call("test", |session| session.run_command("test.spin"))).is_err());
    assert!(matches!(next_event(&events), PluginEvent::Failed { .. }));
    assert_eq!(journal.count("view-closed 1"), 1, "{:?}", journal.lines());
}

#[test]
fn a_trap_while_delivering_an_outcome_fails_the_plugin_once_and_reports_no_uncertain_outcome() {
    let (mut plugins, events) = synthetic("0.1.0-worker");
    add(&mut plugins, "test", "");
    let request = submit_pending(&mut plugins);
    let completed = answer(plugins.complete("test", request, Outcome::Done(Some("trap".into()))));
    assert!(
        matches!(completed, Err(HostError::Trap { .. })),
        "{completed:?}"
    );
    let failed = PluginEvent::Failed {
        plugin: "test".into(),
        failure: Failure::Trapped {
            export: Export::Deliver,
            code: Some(Trap::UnreachableCodeReached),
        },
    };
    assert_eq!(next_event(&events), failed);
    assert!(
        events.recv_timeout(Duration::from_millis(200)).is_err(),
        "the first event was the only one: no second failure and no uncertain outcome"
    );
}

#[test]
fn an_outcome_completed_after_the_job_budget_is_not_delivered() {
    let journal = Journal::default();
    let (mut plugins, events) = plugins_with("0.1.0-worker", TestServices::recording(&journal));
    add(&mut plugins, "test", "");
    let (request, _) = answer(plugins.call("test", |session| {
        session.submit("test.new-item", &[("name", "later")])
    }))
    .expect("pending");
    let completed = answer(plugins.call("test", move |session| {
        // The job spends its whole budget before the outcome arrives.
        std::thread::sleep(limits::JOB_BUDGET);
        session.complete(request, Outcome::Done(None))
    }));
    assert!(
        matches!(completed, Err(HostError::Unavailable(_))),
        "{completed:?}"
    );
    let failed = PluginEvent::Failed {
        plugin: "test".into(),
        failure: Failure::OverBudget,
    };
    assert_eq!(next_event(&events), failed);
    assert_eq!(journal.count("deliver"), 0, "{:?}", journal.lines());
}

#[test]
fn a_deactivate_that_never_returns_is_stopped_at_its_deadline() {
    // The shutdown bound in docs/dev/plugin-runtime-bounds.md assumes this deadline.
    assert!(Export::Deactivate.deadline() <= Duration::from_millis(250));
    let (mut plugins, events) = synthetic("0.1.0-spin-on-deactivate");
    add(&mut plugins, "test", "");
    let request = submit_pending(&mut plugins);
    let started = Instant::now();
    plugins.shutdown();
    let took = started.elapsed();
    assert!(
        took < Export::Deactivate.deadline() * 3,
        "shutdown took {took:?}"
    );
    let received: Vec<PluginEvent> = events.try_iter().collect();
    let failed = PluginEvent::Failed {
        plugin: "test".into(),
        failure: Failure::DeadlineExpired(Export::Deactivate),
    };
    assert_eq!(received, [uncertain(request), failed]);
}

#[test]
fn a_plugin_added_again_after_uninstalling_waits_for_its_old_instance_to_stop() {
    let journal = Journal::default();
    let (mut plugins, _events) = plugins_with("0.1.0-worker", TestServices::recording(&journal));
    add(&mut plugins, "test", "");
    // The old instance's job holds it until the test releases it.
    let (started, job_started) = mpsc::channel();
    let (release, released) = mpsc::channel::<()>();
    let held = plugins
        .call("test", move |session| {
            started.send(()).expect("the test waits");
            released
                .recv_timeout(WAIT)
                .expect("the test releases the job");
            session.run_command("test.fail")
        })
        .expect("queued");
    job_started.recv_timeout(WAIT).expect("the job started");
    plugins.uninstall("test").expect("uninstalled");
    add(&mut plugins, "test", "");
    let queued = plugins
        .call("test", |session| session.run_command("test.fail"))
        .expect("queued for the new instance");
    assert!(
        queued.wait(Duration::from_secs(5)).is_none(),
        "the new instance ran while the old one was still running: {:?}",
        journal.lines()
    );
    release.send(()).expect("the job waits");
    assert!(matches!(
        held.wait(WAIT),
        Some(Ok(Err(PluginError::InvalidInput(_))))
    ));
    assert!(matches!(
        queued.wait(WAIT),
        Some(Ok(Err(PluginError::InvalidInput(_))))
    ));
    plugins.shutdown();
    let lines = journal.lines();
    let deactivated = lines.iter().position(|line| line == "deactivate");
    let reactivated = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.starts_with("activate"))
        .nth(1)
        .map(|(index, _)| index);
    assert!(
        deactivated.is_some() && deactivated < reactivated,
        "the old instance must deactivate before the new one activates: {lines:?}"
    );
}

#[test]
fn a_view_closed_while_stopping_never_reaches_the_plugin() {
    let journal = Journal::default();
    let (mut plugins, _events) = plugins_with("0.1.0-worker", TestServices::recording(&journal));
    add(&mut plugins, "test", "");
    assert!(answer(plugins.call("test", |session| session.run_command("test.fail"))).is_ok());
    let (started, job_started) = mpsc::channel();
    let (stop_raised, wait_for_stop) = mpsc::channel::<()>();
    let job = plugins
        .call("test", move |session| {
            started.send(()).expect("the test waits");
            wait_for_stop
                .recv_timeout(WAIT)
                .expect("the test raised the stop");
            // The export runs; the `opened` calls it deferred are dropped by the stop.
            let opened = session.run_command("test.open")?;
            for view in session.open_views() {
                session.close_view(view)?;
            }
            Ok(opened)
        })
        .expect("queued");
    job_started.recv_timeout(WAIT).expect("the job started");
    plugins
        .deactivate("test", StopReason::Shutdown)
        .expect("stopping");
    stop_raised.send(()).expect("the job waits");
    assert_eq!(job.wait(WAIT), Some(Ok(Ok(()))));
    plugins.shutdown();
    let lines = journal.lines();
    assert_eq!(journal.count("opened"), 0, "{lines:?}");
    assert_eq!(journal.count("closed"), 0, "{lines:?}");
    assert_eq!(journal.count("view-closed"), 2, "{lines:?}");
}
