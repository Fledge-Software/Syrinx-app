//! Proves the palette's plugin commands and forms reach the synthetic test plugin through the
//! session's own route (#29): a form command reaches `forms` and never `commands.run`, the
//! plugin validates before anything is submitted, a repeated Enter cannot submit twice, and
//! answers and outcomes reach only the command, form or closed submission they belong to.

use futures::channel::mpsc::unbounded;
use syrinx_core::plugin_forms::{
    CallKind, Failure, FailureKind, FieldError, FieldValue, Notice, Outcome, PluginAnswer,
    PluginCall, PluginCommands, PluginReply, Refusal, Submitted,
};
use syrinx_plugin_host::PluginRegistry;
use syrinx_plugin_host::manifest::CapabilityKind;
use syrinx_plugin_host::owner::OwnerRuntime;

use super::plugin_tests::{
    create_database, full_component, install_record, registered_fixture, started,
};
use super::*;

/// How long a test waits between looks at the events; the route itself never polls.
const LOOK: Duration = Duration::from_millis(5);

/// The palette's model and the runtime with the test plugin installed, wired as the window and
/// the worker wire them, minus the window: calls go through `plugin_commands::start`, and the
/// answers and settled writes arrive on `receiver` as the window's events.
struct Route {
    fixture: Fixture,
    store: Arc<Store>,
    plugins: plugins::SessionPlugins,
    commands: PluginCommands,
    sender: UnboundedSender<SessionEvent>,
    receiver: UnboundedReceiver<SessionEvent>,
}

impl Route {
    fn new() -> Self {
        let fixture = Fixture::new();
        create_database(&fixture);
        let (store, mut plugins) = started(&fixture);
        let mut registry = PluginRegistry::new();
        let registered = registered_fixture(&mut registry);
        plugins
            .runtime
            .install(registered, Arc::from(full_component()), install_record())
            .expect("installed");
        // `test.echo` keeps a synthetic setting, so it needs `storage`.
        plugins
            .runtime
            .grant("test", CapabilityKind::Storage, 20)
            .expect("granted");
        let (sender, receiver) = unbounded();
        plugin_commands::watch_outcomes(&plugins.runtime, &sender);
        Self {
            fixture,
            store,
            plugins,
            commands: PluginCommands::default(),
            sender,
            receiver,
        }
    }

    fn runtime(&mut self) -> &mut OwnerRuntime {
        &mut self.plugins.runtime
    }

    /// Queue `call` as the worker does on `SessionCommand::Plugin`.
    fn send(&mut self, call: PluginCall) {
        plugin_commands::start(Some(&mut self.plugins.runtime), call, &self.sender);
    }

    /// The next event the window would get.
    fn next_event(&mut self) -> SessionEvent {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if let Ok(event) = self.receiver.try_recv() {
                return event;
            }
            assert!(Instant::now() < deadline, "the route answered in time");
            thread::sleep(LOOK);
        }
    }

    /// The next answer, passing over word that a plugin stopped, which its own tests check.
    fn answer(&mut self) -> PluginAnswer {
        loop {
            match self.next_event() {
                SessionEvent::Plugin(answer) => return answer,
                SessionEvent::PluginStopped { .. } => continue,
                event => panic!("a plugin answer, not {event:?}"),
            }
        }
    }

    /// Send `call`, then hand its answer to the palette's model, as the window does.
    fn exchange(&mut self, call: PluginCall) -> Option<Notice> {
        self.send(call);
        let answer = self.answer();
        self.commands.receive(answer)
    }

    /// Choose the test plugin's form command in an open palette and see its form open.
    fn open_form(&mut self) {
        let call = self.commands.choose("test", "test.new-item", true);
        let notice = self.exchange(call);
        let title = "New item".to_owned();
        assert_eq!(notice, Some(Notice::FormReady { title }));
    }

    /// Open the form, name the item `name` and press Enter once.
    fn submit_named(&mut self, name: &str) -> PluginCall {
        self.open_form();
        self.commands.set_value("name", name).expect("editable");
        self.commands.submit().expect("the host's checks pass")
    }

    /// A submission the plugin answered pending, and its request id.
    fn pending(&mut self) -> u64 {
        let call = self.submit_named("later");
        self.send(call);
        let answer = self.answer();
        let PluginReply::Submitted(Submitted::Pending { request }) = answer.reply else {
            panic!("a pending submission, not {answer:?}");
        };
        assert_eq!(self.commands.receive(answer), Some(Notice::Pending));
        request
    }

    /// Writes the plugin has armed and not yet settled.
    fn writes_in_flight(&mut self) -> Vec<u64> {
        let pending = self
            .runtime()
            .call("test", |session| Ok(session.writes_in_flight()))
            .expect("queued");
        let answered = pending.wait(Duration::from_secs(60)).expect("answered");
        answered.expect("read")
    }

    fn close(self) {
        assert_eq!(plugins::close(Some(self.plugins), self.store), Ok(()));
        self.fixture.cleanup();
    }
}

fn host_failure(message: &str) -> Failure {
    Failure {
        kind: FailureKind::Host,
        message: message.to_owned(),
    }
}

fn validate(token: u64, values: Vec<FieldValue>) -> PluginCall {
    PluginCall {
        token,
        plugin: "test".to_owned(),
        command: "test.new-item".to_owned(),
        kind: CallKind::Validate(values),
    }
}

#[test]
fn a_command_without_a_form_runs_and_one_with_a_form_opens_it() {
    let mut route = Route::new();
    let call = route.commands.choose("test", "test.echo", true);
    assert_eq!(
        route.exchange(call),
        Some(Notice::Outcome(Outcome::Done(None)))
    );
    // `commands.run` would be refused for a form command; reaching `forms.form-for` opens it.
    route.open_form();
    let form = route.commands.form().expect("open");
    assert_eq!(form.spec().fields.len(), 10);
    assert_eq!(form.spec().submit_label, "Create");
    let call = route.commands.choose("test", "test.fail", true);
    let failure = Failure {
        kind: FailureKind::InvalidInput,
        message: "synthetic failure".to_owned(),
    };
    assert_eq!(
        route.exchange(call),
        Some(Notice::Outcome(Outcome::Failed(failure)))
    );
    route.close();
}

#[test]
fn a_form_chosen_after_the_palette_closed_is_told_on_the_status_line() {
    let mut route = Route::new();
    // `PaletteEvent::Run` ends the entry before the command is chosen.
    route.commands.abandon();
    let call = route.commands.choose("test", "test.new-item", false);
    let title = "New item".to_owned();
    assert_eq!(route.exchange(call), Some(Notice::FormNotShown { title }));
    assert!(route.commands.form().is_none());
    route.close();
}

#[test]
fn validation_checks_the_host_rules_first_then_asks_the_plugin() {
    let mut route = Route::new();
    route.open_form();
    // The host's own check: nothing is sent while the required name is empty.
    assert_eq!(
        route.commands.submit(),
        Err(Refusal::Invalid(FieldError {
            name: "name".to_owned(),
            message: "Required.".to_owned(),
        }))
    );
    assert!(route.receiver.try_recv().is_err(), "nothing was sent");
    // The plugin's own refusal of its validation reaches the owner and keeps the input.
    route.commands.set_value("name", "boom").expect("editable");
    let call = route.commands.validate().expect("the host's checks pass");
    let failure = Failure {
        kind: FailureKind::Internal,
        message: "validation failed".to_owned(),
    };
    assert_eq!(route.exchange(call), Some(Notice::Failed(failure)));
    let form = route.commands.form().expect("still open");
    assert_eq!(form.value("name"), Some("boom"));
    // The plugin's field errors come back as such.
    route.send(validate(900, Vec::new()));
    let reply = route.answer().reply;
    let expected = vec![FieldError {
        name: "name".to_owned(),
        message: "A name is required.".to_owned(),
    }];
    assert_eq!(reply, PluginReply::Validated(Ok(expected)));
    route.close();
}

#[test]
fn a_submission_the_plugins_validation_refuses_is_never_submitted() {
    let mut route = Route::new();
    // `validate` fails for "boom", while `submit` would have created it.
    let call = route.submit_named("boom");
    let failure = Failure {
        kind: FailureKind::Internal,
        message: "validation failed".to_owned(),
    };
    assert_eq!(
        route.exchange(call),
        Some(Notice::Outcome(Outcome::Failed(failure)))
    );
    assert_eq!(
        route.commands.form().expect("kept").value("name"),
        Some("boom")
    );

    // With no name the plugin's `validate` answers invalid, and `submit` is never called.
    route.send(PluginCall {
        token: 901,
        plugin: "test".to_owned(),
        command: "test.new-item".to_owned(),
        kind: CallKind::Submit(Vec::new()),
    });
    let expected = vec![FieldError {
        name: "name".to_owned(),
        message: "A name is required.".to_owned(),
    }];
    assert_eq!(
        route.answer().reply,
        PluginReply::Submitted(Submitted::Invalid(expected))
    );
    assert!(route.writes_in_flight().is_empty());
    route.close();
}

#[test]
fn a_repeated_enter_cannot_submit_the_same_form_twice() {
    let mut route = Route::new();
    let call = route.submit_named("later");
    // Enter again while the first submission is on its way: nothing is sent.
    assert_eq!(route.commands.submit(), Err(Refusal::Busy));
    route.send(call);
    let answer = route.answer();
    let PluginReply::Submitted(Submitted::Pending { request }) = answer.reply else {
        panic!("a pending submission, not {answer:?}");
    };
    assert_eq!(route.commands.receive(answer), Some(Notice::Pending));
    // And while it is pending.
    assert_eq!(route.commands.submit(), Err(Refusal::Busy));
    assert_eq!(route.writes_in_flight(), [request]);
    route.close();
}

#[test]
fn a_submission_finishes_done_or_may_have_happened_when_the_plugin_stops_in_submit() {
    let mut route = Route::new();
    let call = route.submit_named("Ada");
    let done = Outcome::Done(Some("created Ada".to_owned()));
    assert_eq!(route.exchange(call), Some(Notice::Outcome(done)));
    assert!(route.commands.form().is_none());

    let call = route.submit_named("panic");
    let notice = route.exchange(call);
    let Some(Notice::Outcome(Outcome::Uncertain(_))) = notice else {
        panic!("a trap during submit may have written: {notice:?}");
    };
    assert_eq!(route.commands.submit(), Err(Refusal::Uncertain));
    route.close();
}

#[test]
fn a_submission_that_never_ran_failed_with_nothing_sent() {
    let mut route = Route::new();
    let call = route.submit_named("Ada");
    // `test.spin` runs until its deadline stops the plugin; the submission waits behind it.
    let spin = PluginCall {
        token: 960,
        plugin: "test".to_owned(),
        command: "test.spin".to_owned(),
        kind: CallKind::Choose,
    };
    let spin_token = spin.token;
    route.send(spin);
    route.send(call);
    let spun = route.answer();
    assert_eq!(spun.token, spin_token);
    assert_eq!(
        spun.reply,
        PluginReply::Ran(Err(host_failure("The plugin stopped before it answered.")))
    );
    let never_ran = route.answer();
    let failure = host_failure("The plugin could not take the request, so nothing was sent.");
    assert_eq!(
        route.commands.receive(never_ran),
        Some(Notice::Outcome(Outcome::Failed(failure)))
    );
    assert_eq!(
        route.commands.form().expect("kept").value("name"),
        Some("Ada")
    );
    route.close();
}

#[test]
fn a_closed_palettes_submission_still_reports_its_own_outcome() {
    let mut route = Route::new();
    let call = route.submit_named("Ada");
    route.send(call);
    // The owner closes the palette before the plugin answers.
    route.commands.abandon();
    let answer = route.answer();
    let done = Outcome::Done(Some("created Ada".to_owned()));
    assert_eq!(route.commands.receive(answer), Some(Notice::Outcome(done)));
    assert!(route.commands.form().is_none());
    route.close();
}

#[test]
fn an_answer_reaches_the_window_ahead_of_its_own_outcome() {
    let mut route = Route::new();
    let call = route.submit_named("later");
    route.send(call);
    // The plugin traps on the next call, with the pending write still in flight.
    let panic = PluginCall {
        token: 950,
        plugin: "test".to_owned(),
        command: "test.panic".to_owned(),
        kind: CallKind::Choose,
    };
    route.send(panic);
    let first = route.next_event();
    let second = route.next_event();
    let third = route.next_event();
    let fourth = route.next_event();
    let SessionEvent::Plugin(pending) = first else {
        panic!("the submission's answer first, not {first:?}");
    };
    assert!(matches!(
        second,
        SessionEvent::Plugin(PluginAnswer { token: 950, .. })
    ));
    let SessionEvent::PluginOutcome(late) = third else {
        panic!("the settled write next, not {third:?}");
    };
    // The plugin's stop comes only after the writes it settled.
    let SessionEvent::PluginStopped { plugin } = fourth else {
        panic!("the stop last, not {fourth:?}");
    };
    assert_eq!(plugin, "test");
    assert_eq!(route.commands.receive(pending), Some(Notice::Pending));
    let notice = route.commands.receive_outcome(late);
    let Some(Notice::Outcome(Outcome::Uncertain(_))) = notice else {
        panic!("the pending form is told it may have happened: {notice:?}");
    };
    route.close();
}

#[test]
fn a_pending_outcome_reaches_its_own_form_as_soon_as_the_host_settles_it() {
    let mut route = Route::new();
    route.pending();
    // The plugin is removed with the write in flight; nothing else wakes the route.
    route.runtime().remove("test").expect("removed");
    let SessionEvent::PluginOutcome(late) = route.next_event() else {
        panic!("the settled write");
    };
    let notice = route.commands.receive_outcome(late);
    let Some(Notice::Outcome(Outcome::Uncertain(_))) = notice else {
        panic!("the pending form is told it may have happened: {notice:?}");
    };
    assert!(route.commands.form().is_some(), "kept, never resubmitted");
    route.close();
}

#[test]
fn a_pending_outcome_after_another_command_was_chosen_reaches_only_the_status_line() {
    let mut route = Route::new();
    route.pending();
    // The owner moves on to another command before the outcome arrives.
    let other = route.commands.choose("test", "test.echo", true);
    route.runtime().remove("test").expect("removed");
    let SessionEvent::PluginOutcome(late) = route.next_event() else {
        panic!("the settled write");
    };
    let notice = route.commands.receive_outcome(late);
    let Some(Notice::Outcome(Outcome::Uncertain(_))) = notice else {
        panic!("the closed submission is still told: {notice:?}");
    };
    assert!(route.commands.form().is_none());
    assert!(matches!(
        route.next_event(),
        SessionEvent::PluginStopped { .. }
    ));
    // The other command's own answer still counts: the plugin is gone, so it fails plainly.
    let failure = host_failure("That plugin is not available.");
    assert_eq!(
        route.exchange(other),
        Some(Notice::Outcome(Outcome::Failed(failure)))
    );
    route.close();
}

#[test]
fn values_cross_to_the_plugin_by_field_name() {
    let mut route = Route::new();
    let values = vec![FieldValue {
        name: "name".to_owned(),
        value: "Ada".to_owned(),
    }];
    route.send(validate(901, values));
    let answer = route.answer();
    assert_eq!(answer.token, 901);
    assert_eq!(answer.reply, PluginReply::Validated(Ok(Vec::new())));
    route.close();
}

#[test]
fn a_plugin_command_reaches_the_session_and_fails_plainly_when_its_plugin_is_not_loaded() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    state(&mut events);
    let mut commands = PluginCommands::default();
    let call = commands.choose("test", "test.echo", false);
    send(&handle, SessionCommand::Plugin(call));
    let answer = loop {
        match next_any_event(&mut events) {
            SessionEvent::Plugin(answer) => break answer,
            event @ (SessionEvent::Failure { .. } | SessionEvent::Stopped) => {
                panic!("a plugin answer, not {event:?}")
            }
            // Reports the worker sends at its own pace are not the answer.
            _ => continue,
        }
    };
    let failure = host_failure("That plugin is not available.");
    assert_eq!(
        commands.receive(answer),
        Some(Notice::Outcome(Outcome::Failed(failure)))
    );
    stop(handle, &mut events);
    fixture.cleanup();
}

#[test]
fn a_submission_closed_before_it_answered_pending_still_hears_its_settled_outcome() {
    let mut route = Route::new();
    let call = route.submit_named("later");
    route.send(call);
    // The owner closes the palette while the submission is on its way.
    route.commands.abandon();
    let answer = route.answer();
    assert_eq!(
        route.commands.receive(answer),
        None,
        "pending: nothing to tell yet"
    );
    route.runtime().remove("test").expect("removed");
    let SessionEvent::PluginOutcome(late) = route.next_event() else {
        panic!("the settled write");
    };
    let notice = route.commands.receive_outcome(late);
    let Some(Notice::Outcome(Outcome::Uncertain(_))) = notice else {
        panic!("the closed submission is told it may have happened: {notice:?}");
    };
    assert!(route.commands.form().is_none());
    route.close();
}
