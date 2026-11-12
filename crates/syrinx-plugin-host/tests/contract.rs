//! Contract tests for `syrinx:plugin@0.1.0`: synthetic plugins loaded by the
//! test host, without a window. Each negative test names how it must fail.

mod lifecycle;
mod package;
mod support;

use syrinx_plugin_host::bindings::exports::syrinx::plugin::forms::{
    FieldKind, SubmitResult, TextFormat,
};
use syrinx_plugin_host::bindings::exports::syrinx::plugin::search_feed::Change;
use syrinx_plugin_host::bindings::syrinx::plugin::types::{Icon, Outcome, PluginError};
use syrinx_plugin_host::deadline::Export;
use syrinx_plugin_host::test_host::{SYNTHETIC_NOW, TestHost};
use syrinx_plugin_host::{Failure, HostError, OUTPUT_LIMIT_BYTES, Runtime, limits};
use wasmtime::Trap;

use support::{FULL, MINIMAL, PLAIN_VARIANT, full_declarations, one_command, variant};

fn full() -> TestHost {
    let mut host =
        TestHost::load(&support::bytes(FULL), full_declarations()).expect("the full fixture loads");
    host.activate()
        .expect("activate runs")
        .expect("activate succeeds");
    host
}

fn logged(host: &TestHost) -> Vec<String> {
    host.logs()
        .iter()
        .map(|(_, message)| message.clone())
        .collect()
}

fn incompatible(error: HostError) -> String {
    let HostError::Incompatible(reasons) = error else {
        panic!("expected an incompatible component, got {error}");
    };
    reasons.join("; ")
}

fn load_refused(bytes: &[u8]) -> HostError {
    match TestHost::load(bytes, one_command("variant", "variant.go")) {
        Ok(_) => panic!("the component loaded"),
        Err(error) => error,
    }
}

// Loading and the shape of the component.

#[test]
fn the_full_plugin_loads_and_every_name_carries_its_version() {
    let runtime = Runtime::new().expect("engine");
    let component = runtime.compile(&support::bytes(FULL)).expect("compiles");
    let component_type = component.component_type();
    let exports: Vec<String> = component_type
        .exports(runtime.engine())
        .map(|(name, _)| name.to_owned())
        .collect();
    for interface in [
        "lifecycle",
        "commands",
        "forms",
        "views",
        "search-feed",
        "events",
    ] {
        assert!(
            exports.contains(&format!("syrinx:plugin/{interface}@0.1.0")),
            "{exports:?}"
        );
    }
    let imports: Vec<String> = component_type
        .imports(runtime.engine())
        .map(|(name, _)| name.to_owned())
        .collect();
    assert!(
        imports.contains(&"syrinx:plugin/host@0.1.0".to_owned()),
        "{imports:?}"
    );
    assert!(
        logged(&full())
            .iter()
            .any(|line| line.starts_with("activate 0.1.0-test-host"))
    );
}

#[test]
fn a_plugin_using_only_the_default_stubs_loads_and_answers_not_found() {
    let mut host =
        TestHost::load(&support::bytes(MINIMAL), one_command("mini", "mini.hello")).expect("loads");
    assert_eq!(host.activate().expect("runs"), Ok(()));
    assert_eq!(host.run_command("mini.hello").expect("runs"), Ok(()));
}

#[test]
fn bytes_that_are_not_a_component_are_refused() {
    assert!(matches!(
        load_refused(b"not wasm at all"),
        HostError::Malformed(_)
    ));
    let core_module = b"\0asm\x01\0\0\0";
    assert!(matches!(load_refused(core_module), HostError::Malformed(_)));
}

#[test]
fn precompiled_native_code_is_refused() {
    let runtime = Runtime::new().expect("engine");
    let native = runtime
        .engine()
        .precompile_component(&support::bytes(FULL))
        .expect("precompiles");
    assert_eq!(runtime.compile(&native).err(), Some(HostError::Precompiled));
}

#[test]
fn malformed_declared_ids_are_refused_before_loading() {
    let bytes = support::bytes(MINIMAL);
    for (plugin, command) in [
        ("mini", "Mini.Hello"),
        ("mini", "other.hello"),
        ("", "mini.hello"),
    ] {
        let refused = TestHost::load(&bytes, one_command(plugin, command)).err();
        assert!(
            matches!(refused, Some(HostError::Refused(_))),
            "{plugin} {command}: {refused:?}"
        );
    }
}

// What breaks a plugin (note 10, section 2): one test per row.

#[test]
fn a_plugin_built_for_0_2_0_is_refused_by_name() {
    let reasons = incompatible(load_refused(&support::bytes(variant("version-0-2-0"))));
    assert!(
        reasons.contains("exports syrinx:plugin/commands@0.2.0"),
        "{reasons}"
    );
    assert!(reasons.contains("runs syrinx:plugin@0.1.x"), "{reasons}");
}

#[test]
fn a_plugin_using_a_since_0_1_1_import_needs_a_0_1_1_host() {
    let reasons = incompatible(load_refused(&support::bytes(variant("since-0-1-1"))));
    assert!(
        reasons.contains("built for syrinx:plugin@0.1.1"),
        "{reasons}"
    );
    assert!(reasons.contains("syrinx:plugin/extra@0.1.1"), "{reasons}");
}

#[test]
fn a_case_added_to_a_variant_is_refused_at_instantiation() {
    let error = load_refused(&support::bytes(variant("extra-case")));
    assert!(matches!(error, HostError::Instantiation(_)), "{error}");
}

#[test]
fn a_renamed_export_function_is_refused_at_instantiation() {
    let error = load_refused(&support::bytes(variant("renamed-export")));
    let HostError::Instantiation(detail) = &error else {
        panic!("expected an instantiation failure, got {error}");
    };
    assert!(detail.contains("run"), "{detail}");
}

#[test]
fn a_plugin_that_opted_into_an_unstable_import_is_refused() {
    let error = load_refused(&support::bytes(variant("widgets")));
    let HostError::Instantiation(detail) = &error else {
        panic!("expected an instantiation failure, got {error}");
    };
    assert!(detail.contains("widget-changed"), "{detail}");
}

#[test]
fn an_import_the_host_does_not_provide_is_refused_by_name() {
    let reasons = incompatible(load_refused(&support::bytes(variant("filesystem"))));
    assert!(reasons.contains("wasi:filesystem/"), "{reasons}");
}

#[test]
fn the_unchanged_variant_loads_so_each_refusal_is_its_change() {
    let mut host = TestHost::load(
        &support::bytes(PLAIN_VARIANT),
        one_command("variant", "variant.go"),
    )
    .expect("the unchanged variant loads");
    assert_eq!(host.run_command("variant.go").expect("runs"), Ok(()));
}

// Round trips through the canonical ABI.

#[test]
fn every_form_field_kind_round_trips() {
    let mut host = full();
    let spec = host
        .form_for("test.new-item")
        .expect("runs")
        .expect("a form");
    assert_eq!(spec.title, "New item");
    let kinds: Vec<&FieldKind> = spec.fields.iter().map(|field| &field.kind).collect();
    assert!(
        matches!(kinds[1], FieldKind::Text(options) if matches!(options.format, TextFormat::Email))
    );
    assert!(
        matches!(kinds[2], FieldKind::Text(options) if matches!(options.format, TextFormat::WebAddress))
    );
    assert!(
        matches!(kinds[4], FieldKind::Number(options) if options.step == Some(0.5) && options.max.is_none())
    );
    assert!(matches!(kinds[5], FieldKind::Money(currency) if currency == "USD"));
    assert!(matches!(kinds[6], FieldKind::Choice(options) if options[0].label == "Web"));
    assert!(matches!(kinds[8], FieldKind::Date));
    assert!(matches!(kinds[9], FieldKind::DateTime));
    assert!(spec.fields[0].required && !spec.fields[1].required);
}

#[test]
fn a_form_over_the_limits_is_a_typed_error() {
    let mut host = full();
    let refused = host.form_for("test.too-big").expect("runs");
    assert!(
        matches!(refused, Err(PluginError::InvalidInput(_))),
        "{refused:?}"
    );
    assert!(!host.is_discarded());
}

#[test]
fn validate_reports_field_errors_and_its_own_failure() {
    let mut host = full();
    let errors = host
        .validate_form("test.new-item", &[("name", "")])
        .expect("runs")
        .expect("validated");
    assert_eq!(errors[0].name, "name");
    assert_eq!(
        host.validate_form("test.new-item", &[("name", "Ada")])
            .expect("runs"),
        Ok(Vec::new())
    );
    let failed = host
        .validate_form("test.new-item", &[("name", "boom")])
        .expect("runs");
    assert!(matches!(failed, Err(PluginError::Internal(_))));
}

#[test]
fn submit_returns_finished_invalid_and_pending() {
    let mut host = full();
    let (_, finished) = host
        .submit("test.new-item", &[("name", "Ada")])
        .expect("runs");
    assert!(
        matches!(finished, SubmitResult::Finished(Outcome::Done(Some(detail))) if detail == "created Ada")
    );
    let (_, invalid) = host.submit("test.new-item", &[("name", "")]).expect("runs");
    assert!(matches!(invalid, SubmitResult::Invalid(errors) if errors.len() == 1));
}

#[test]
fn a_pending_answer_is_delivered_after_the_export_and_only_once() {
    let mut host = full();
    let (request, pending) = host
        .submit("test.new-item", &[("name", "later")])
        .expect("runs");
    assert!(matches!(pending, SubmitResult::Pending(returned) if returned == request));
    host.complete(request, Outcome::Uncertain("reached the service".into()))
        .expect("delivers");
    assert!(logged(&host).contains(&format!("deliver {request} uncertain reached the service")));
    assert!(matches!(
        host.complete(request, Outcome::Done(None)),
        Err(HostError::Refused(_))
    ));
}

#[test]
fn a_pending_answer_with_another_id_is_the_plugins_error() {
    let mut host = full();
    let (_, result) = host
        .submit("test.new-item", &[("name", "wrong-id")])
        .expect("runs");
    assert!(
        matches!(
            result,
            SubmitResult::Finished(Outcome::Failed(PluginError::InvalidInput(_)))
        ),
        "{result:?}"
    );
}

#[test]
fn a_request_the_plugin_cancels_is_never_delivered() {
    let mut host = full();
    let (request, _) = host
        .submit("test.new-item", &[("name", "later")])
        .expect("runs");
    assert_eq!(request, 1, "test.cancel cancels request 1");
    host.run_command("test.cancel")
        .expect("runs")
        .expect("cancelled");
    host.complete(request, Outcome::Done(None))
        .expect("accepted");
    assert!(!logged(&host).iter().any(|line| line.starts_with("deliver")));
}

#[test]
fn a_search_batch_round_trips_and_extra_keywords_are_dropped() {
    let mut host = full();
    let batch = host
        .changes("test.items", None, 10)
        .expect("runs")
        .expect("a batch");
    assert!(matches!(batch.changes[0], Change::Reset));
    let Change::UpsertEntry(entry) = &batch.changes[1] else {
        panic!("an entry")
    };
    assert!(matches!(entry.icon, Icon::List));
    assert_eq!(entry.last_used, Some(SYNTHETIC_NOW));
    assert_eq!(entry.keywords.len(), limits::MAX_KEYWORDS);
    assert!(
        matches!(&batch.changes[2], Change::UpsertDocument(document) if document.body == "Made-up text.")
    );
    assert_eq!(
        logged(&host)
            .iter()
            .filter(|line| line.starts_with("dropped"))
            .count(),
        5
    );
    assert_eq!(
        host.resolve("test.items", "item-1").expect("runs"),
        Ok("syrinx-test:item-1".into())
    );
}

#[test]
fn a_batch_larger_than_asked_is_a_typed_error() {
    let mut host = full();
    let flood = host.changes("test.items", Some("flood"), 4).expect("runs");
    assert!(
        matches!(flood, Err(PluginError::InvalidInput(_))),
        "{flood:?}"
    );
}

#[test]
fn the_plain_host_imports_answer() {
    let mut host = full();
    host.run_command("test.echo")
        .expect("runs")
        .expect("echoed");
    assert!(logged(&host).contains(&format!(
        "echo now={SYNTHETIC_NOW} stored=Some([104, 101, 108, 108, 111])"
    )));
    assert_eq!(host.changed_feeds(), ["test.items"]);
}

// The form rule and declarations.

#[test]
fn a_command_with_a_form_is_never_run_and_nothing_undeclared_reaches_the_plugin() {
    let mut host = full();
    let calls = host.export_calls();
    assert!(matches!(
        host.run_command("test.new-item"),
        Err(HostError::Refused(_))
    ));
    assert!(matches!(
        host.form_for("test.echo"),
        Err(HostError::Refused(_))
    ));
    assert!(matches!(
        host.run_command("test.undeclared"),
        Err(HostError::Refused(_))
    ));
    assert!(matches!(
        host.changes("test.other", None, 1),
        Err(HostError::Refused(_))
    ));
    assert_eq!(host.export_calls(), calls);
}

#[test]
fn a_typed_failure_leaves_the_plugin_running() {
    let mut host = full();
    assert_eq!(
        host.run_command("test.fail").expect("runs"),
        Err(PluginError::InvalidInput("synthetic failure".into()))
    );
    assert_eq!(host.run_command("test.echo").expect("runs"), Ok(()));
}

// Views and the delivery order.

#[test]
fn views_open_after_the_export_returns_and_a_refused_view_is_closed() {
    let mut host = full();
    host.run_command("test.open")
        .expect("runs")
        .expect("opened");
    let lines = logged(&host);
    let opening = lines
        .iter()
        .position(|line| line == "opening 1 2")
        .expect("logged");
    let opened = lines
        .iter()
        .position(|line| line.starts_with("opened 1"))
        .expect("delivered");
    assert!(opening < opened, "{lines:?}");
    assert_eq!(host.open_views(), [1]);
    assert!(matches!(
        host.refused_views(),
        [(2, PluginError::Denied(_))]
    ));
    let view = host.view(1).expect("open");
    assert_eq!(
        (view.title.as_str(), view.badge.as_deref()),
        ("One", Some("3"))
    );
    assert_eq!(
        view.events,
        ["{\"v\":1,\"kind\":\"event\",\"topic\":\"test.hello\",\"body\":{\"n\":1}}"]
    );
}

#[test]
fn view_messages_run_in_order_a_cancelled_one_is_dropped_and_closed_comes_last() {
    let mut host = full();
    host.run_command("test.open")
        .expect("runs")
        .expect("opened");
    let first = host
        .queue_view_message(1, "ping", "{\"a\":1}")
        .expect("queued");
    let second = host
        .queue_view_message(1, "ping", "{\"a\":2}")
        .expect("queued");
    let bad = host
        .queue_view_message(1, "bad-json", "{}")
        .expect("queued");
    host.cancel_request(first);
    host.run_deferred().expect("delivered");
    assert_eq!(host.reply(first), None);
    assert_eq!(host.reply(second), Some(&Ok("{\"echo\":{\"a\":2}}".into())));
    assert!(matches!(
        host.reply(bad),
        Some(Err(PluginError::InvalidInput(_)))
    ));
    let undelivered = host.queue_view_message(1, "ping", "{}").expect("queued");
    host.close_view(1).expect("closed");
    assert_eq!(host.reply(undelivered), None);
    assert_eq!(logged(&host).last().map(String::as_str), Some("closed 1"));
}

// Deadlines, memory and traps.

#[test]
fn an_endless_loop_is_stopped_by_the_deadline_and_the_store_is_discarded() {
    let mut host = full();
    let started = std::time::Instant::now();
    let error = host.run_command("test.spin").expect_err("trapped");
    assert!(
        matches!(
            error,
            HostError::Trap {
                code: Some(Trap::Interrupt),
                ..
            }
        ),
        "{error}"
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    assert_eq!(host.run_command("test.echo"), Err(HostError::Discarded));
}

#[test]
fn growing_memory_past_the_cap_is_refused_and_the_plugin_survives() {
    let mut host = full();
    assert_eq!(
        host.run_command("test.grow").expect("runs"),
        Err(PluginError::Unavailable("memory growth refused".into()))
    );
    assert_eq!(host.run_command("test.echo").expect("runs"), Ok(()));
}

#[test]
fn allocating_past_the_cap_traps_and_the_store_is_discarded() {
    let mut host = full();
    let error = host.run_command("test.alloc").expect_err("trapped");
    assert!(
        matches!(
            error,
            HostError::Trap {
                code: Some(Trap::UnreachableCodeReached),
                ..
            }
        ),
        "{error}"
    );
    assert_eq!(host.run_command("test.echo"), Err(HostError::Discarded));
}

// Syrinx's own WASI.

#[test]
fn output_lands_in_bounded_buffers() {
    let mut host = full();
    host.run_command("test.print")
        .expect("runs")
        .expect("printed");
    let stdout = &host.wasi().stdout;
    assert_eq!(stdout.text().len(), OUTPUT_LIMIT_BYTES);
    assert!(stdout.dropped() > 0);
    assert_eq!(host.wasi().stderr.text(), "synthetic error line\n");
}

#[test]
fn a_panic_writes_to_stderr_and_traps() {
    let mut host = full();
    assert!(matches!(
        host.run_command("test.panic"),
        Err(HostError::Trap { .. })
    ));
    assert!(host.wasi().stderr.text().contains("synthetic panic"));
}

#[test]
fn exit_and_sleep_stop_the_call_with_a_named_reason() {
    for (command, reason) in [
        ("test.exit", "called exit"),
        ("test.sleep", "tried to sleep"),
    ] {
        let mut host = full();
        let error = host.run_command(command).expect_err("stopped");
        assert!(error.to_string().contains(reason), "{command}: {error}");
        assert_eq!(host.run_command("test.echo"), Err(HostError::Discarded));
    }
}

#[test]
fn clocks_and_the_hash_seed_work() {
    let mut host = full();
    host.run_command("test.clock").expect("runs").expect("ran");
    assert!(logged(&host).contains(&"clock elapsed_ok=true counts=1 wall_ok=true".into()));
}

#[test]
fn a_plugin_cannot_hold_unbounded_views_or_waiting_events() {
    let mut host = full();
    let refused = host.run_command("test.views").expect("runs");
    assert!(
        matches!(refused, Err(PluginError::Unavailable(_))),
        "{refused:?}"
    );
    let views_line = format!("views opened={}", limits::MAX_OPEN_VIEWS);
    assert!(logged(&host).contains(&views_line), "{:?}", logged(&host));
    assert_eq!(host.open_views().len(), limits::MAX_OPEN_VIEWS);

    let refused = host.run_command("test.events").expect("runs");
    assert!(
        matches!(refused, Err(PluginError::Unavailable(_))),
        "{refused:?}"
    );
    let waiting = host.take_view_events(1).expect("view 1 is open");
    let waiting_bytes: usize = waiting.iter().map(String::len).sum();
    assert!(!waiting.is_empty() && waiting_bytes <= limits::MAX_VIEW_EVENT_BYTES);
    // Once the view reads them, the plugin may post again.
    host.run_command("test.events")
        .expect("runs")
        .expect_err("refused again once full");
    assert!(!host.take_view_events(1).expect("open").is_empty());
}

#[test]
fn views_opening_views_stop_at_the_limit_and_the_plugin_keeps_running() {
    let mut host = full();
    host.run_command("test.chain")
        .expect("runs")
        .expect("the first view opens");
    // The last view to open could not open another, so it refused its own `opened`.
    assert_eq!(host.open_views().len(), limits::MAX_OPEN_VIEWS - 1);
    assert!(matches!(
        host.refused_views(),
        [(_, PluginError::Unavailable(_))]
    ));
    assert!(!host.is_discarded());
}

#[test]
fn a_long_error_message_is_cut_not_refused() {
    let mut host = full();
    let error = host.run_command("test.long-error").expect("runs");
    let Err(PluginError::Internal(text)) = error else {
        panic!("{error:?}");
    };
    assert_eq!(text.chars().count(), limits::MAX_MESSAGE_CHARS);
}

// A stop after an answer, and the short deadlines.

#[test]
fn a_trap_in_a_deferred_call_after_submit_answered_keeps_the_answer() {
    let mut host = full();
    let (_, result) = host
        .submit("test.new-item", &[("name", "open-then-done")])
        .expect("the plugin answered before the trap");
    assert_eq!(
        result,
        SubmitResult::Finished(Outcome::Done(Some("opened".into())))
    );
    assert!(host.is_discarded());
    assert_eq!(
        host.failure(),
        Some(&Failure::Trapped {
            export: Export::Opened,
            code: Some(Trap::UnreachableCodeReached)
        })
    );
    assert_eq!(host.writes_in_flight(), Vec::<u64>::new());
}

#[test]
fn a_trap_inside_submit_names_the_request_left_in_flight() {
    let mut host = full();
    let stopped = host.submit("test.new-item", &[("name", "panic")]);
    let Err(HostError::SubmitStopped { request, .. }) = stopped else {
        panic!("expected the submit to stop, got {stopped:?}");
    };
    assert_eq!(host.writes_in_flight(), [request]);
}

#[test]
fn a_spinning_validator_is_stopped_by_its_own_short_deadline() {
    let mut host = full();
    let started = std::time::Instant::now();
    let error = host
        .validate_form("test.new-item", &[("name", "spin")])
        .expect_err("trapped");
    assert!(
        matches!(
            error,
            HostError::Trap {
                code: Some(Trap::Interrupt),
                ..
            }
        ),
        "{error}"
    );
    assert!(
        logged(&host).contains(&"spinning".to_owned()),
        "the loop ran"
    );
    assert_eq!(
        host.failure(),
        Some(&Failure::DeadlineExpired(Export::Validate))
    );
    assert!(started.elapsed() < Export::Run.deadline());
}
