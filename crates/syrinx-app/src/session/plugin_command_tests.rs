//! Proves the palette's plugin commands and forms reach the synthetic test plugin through the
//! session's own route (#29): a form command reaches `forms` and never `commands.run`,
//! validation reports the first failing field, a repeated Enter cannot submit twice, and a
//! late answer or outcome never updates a command or form that was abandoned.

use futures::channel::mpsc::unbounded;
use syrinx_core::plugin_forms::{
    Failure, FailureKind, FieldError, FieldValue, Notice, Outcome, PluginAnswer, PluginCall,
    PluginCommands, PluginReply, Refusal, Submitted,
};
use syrinx_plugin_host::PluginRegistry;
use syrinx_plugin_host::manifest::CapabilityKind;
use syrinx_plugin_host::owner::OwnerRuntime;

use super::plugin_tests::{
    create_database, full_component, install_record, registered_fixture, started,
};
use super::*;

/// The palette's model, the session's calls and the runtime with the test plugin installed,
/// wired as the window and the worker wire them, minus the window.
struct Route {
    fixture: Fixture,
    store: Arc<Store>,
    plugins: plugins::SessionPlugins,
    calls: plugin_commands::PluginCalls,
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
        Self {
            fixture,
            store,
            plugins,
            calls: plugin_commands::PluginCalls::default(),
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
        let runtime = Some(&mut self.plugins.runtime);
        self.calls
            .start(runtime, call, &self.sender)
            .expect("the call is queued or answered");
    }

    /// The next answer, collected at the worker's wake-ups until it arrives.
    fn answer(&mut self) -> PluginAnswer {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            self.calls.collect(&self.sender).expect("answers are sent");
            if let Ok(event) = self.receiver.try_recv() {
                let SessionEvent::Plugin(answer) = event else {
                    panic!("a plugin answer, not {event:?}");
                };
                return answer;
            }
            assert!(Instant::now() < deadline, "the plugin answered in time");
            thread::sleep(plugin_commands::ANSWER_POLL);
        }
    }

    /// Send `call`, then hand its answer to the palette's model, as the window does.
    fn exchange(&mut self, call: PluginCall) -> Option<Notice> {
        self.send(call);
        let answer = self.answer();
        self.commands.receive(answer)
    }

    /// Choose the test plugin's form command and see its form open.
    fn open_form(&mut self) {
        let call = self.commands.choose("test", "test.new-item");
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

    /// Writes the plugin has armed and not yet settled.
    fn writes_in_flight(&mut self) -> Vec<u64> {
        let pending = self
            .runtime()
            .call("test", |session| Ok(session.writes_in_flight()))
            .expect("queued");
        let answered = pending.wait(Duration::from_secs(60)).expect("answered");
        answered.expect("read")
    }

    /// The writes the host settled itself, forwarded as the worker forwards them, once the
    /// runtime has reported at least one.
    fn settled_outcomes(&mut self) -> Vec<SessionEvent> {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let raised = self.plugins.runtime.take_events();
            plugin_commands::forward_outcomes(&raised, &self.sender).expect("forwarded");
            let mut forwarded = Vec::new();
            while let Ok(event) = self.receiver.try_recv() {
                forwarded.push(event);
            }
            if !forwarded.is_empty() {
                return forwarded;
            }
            assert!(
                Instant::now() < deadline,
                "the host settled the write in time"
            );
            thread::sleep(plugin_commands::ANSWER_POLL);
        }
    }

    fn close(self) {
        assert_eq!(plugins::close(Some(self.plugins), self.store), Ok(()));
        self.fixture.cleanup();
    }
}

fn request_of(notice_call: &PluginAnswer) -> u64 {
    match &notice_call.reply {
        PluginReply::Submitted(Submitted::Pending { request }) => *request,
        reply => panic!("a pending submission, not {reply:?}"),
    }
}

#[test]
fn a_command_without_a_form_runs_and_one_with_a_form_opens_it() {
    let mut route = Route::new();
    let call = route.commands.choose("test", "test.echo");
    assert_eq!(
        route.exchange(call),
        Some(Notice::Outcome(Outcome::Done(None)))
    );
    // `commands.run` would be refused for a form command; reaching `forms.form-for` opens it.
    route.open_form();
    let form = route.commands.form().expect("open");
    assert_eq!(form.spec().fields.len(), 10);
    assert_eq!(form.spec().submit_label, "Create");
    let call = route.commands.choose("test", "test.fail");
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
    assert!(!route.calls.is_waiting());
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
    route.send(PluginCall::Validate {
        token: 900,
        plugin: "test".to_owned(),
        command: "test.new-item".to_owned(),
        values: Vec::new(),
    });
    let reply = route.answer().reply;
    let expected = vec![FieldError {
        name: "name".to_owned(),
        message: "A name is required.".to_owned(),
    }];
    assert_eq!(reply, PluginReply::Validated(Ok(expected)));
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
    let request = request_of(&answer);
    assert_eq!(route.commands.receive(answer), Some(Notice::Pending));
    // And while it is pending.
    assert_eq!(route.commands.submit(), Err(Refusal::Busy));
    assert_eq!(route.writes_in_flight(), [request]);
    route.close();
}

#[test]
fn a_submission_finishes_done_or_may_have_happened_when_the_plugin_stops() {
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
fn a_late_answer_for_a_closed_palette_updates_nothing() {
    let mut route = Route::new();
    let call = route.submit_named("Ada");
    route.send(call);
    // The owner closes the palette before the plugin answers.
    route.commands.abandon();
    let answer = route.answer();
    assert!(matches!(answer.reply, PluginReply::Submitted(_)));
    assert_eq!(route.commands.receive(answer), None);
    assert!(route.commands.form().is_none());
    route.close();
}

#[test]
fn a_pending_outcome_reaches_its_own_form_and_no_other() {
    let mut route = Route::new();
    let call = route.submit_named("later");
    route.send(call);
    let answer = route.answer();
    route.commands.receive(answer);

    // The plugin stops with the write in flight; the host settles it as uncertain.
    route.runtime().remove("test").expect("removed");
    let forwarded = route.settled_outcomes();
    let [SessionEvent::PluginOutcome(late)] = forwarded.as_slice() else {
        panic!("one settled write: {forwarded:?}");
    };
    let notice = route.commands.receive_outcome(late.clone());
    let Some(Notice::Outcome(Outcome::Uncertain(_))) = notice else {
        panic!("the pending form is told it may have happened: {notice:?}");
    };
    route.close();
}

#[test]
fn a_pending_outcome_after_another_command_was_chosen_is_dropped() {
    let mut route = Route::new();
    let call = route.submit_named("later");
    route.send(call);
    let answer = route.answer();
    route.commands.receive(answer);
    // The owner moves on to another command before the outcome arrives.
    let other = route.commands.choose("test", "test.echo");

    route.runtime().remove("test").expect("removed");
    let forwarded = route.settled_outcomes();
    let [SessionEvent::PluginOutcome(late)] = forwarded.as_slice() else {
        panic!("one settled write: {forwarded:?}");
    };
    assert_eq!(route.commands.receive_outcome(late.clone()), None);
    // The other command's own answer still counts: the plugin is gone, so it fails plainly.
    let failure = Failure {
        kind: FailureKind::Host,
        message: "That plugin is not available.".to_owned(),
    };
    assert_eq!(
        route.exchange(other),
        Some(Notice::Outcome(Outcome::Failed(failure)))
    );
    route.close();
}

#[test]
fn a_plugin_command_reaches_the_session_and_fails_plainly_when_its_plugin_is_not_loaded() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    state(&mut events);
    let mut commands = PluginCommands::default();
    let call = commands.choose("test", "test.echo");
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
    let failure = Failure {
        kind: FailureKind::Host,
        message: "That plugin is not available.".to_owned(),
    };
    assert_eq!(
        commands.receive(answer),
        Some(Notice::Outcome(Outcome::Failed(failure)))
    );
    stop(handle, &mut events);
    fixture.cleanup();
}

#[test]
fn values_cross_to_the_plugin_by_field_name() {
    let mut route = Route::new();
    route.send(PluginCall::Validate {
        token: 901,
        plugin: "test".to_owned(),
        command: "test.new-item".to_owned(),
        values: vec![FieldValue {
            name: "name".to_owned(),
            value: "Ada".to_owned(),
        }],
    });
    let answer = route.answer();
    assert_eq!(answer.token, 901);
    assert_eq!(answer.reply, PluginReply::Validated(Ok(Vec::new())));
    route.close();
}
