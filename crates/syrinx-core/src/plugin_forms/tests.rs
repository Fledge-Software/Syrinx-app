//! The form model's rules without a plugin: the host's checks, one submission at a time, and
//! answers and outcomes matched only to the command and request that asked.

use super::*;

fn text_field(name: &str, required: bool, max_length: u32) -> Field {
    Field {
        name: name.to_owned(),
        label: name.to_uppercase(),
        kind: FieldKind::Text(TextOptions {
            max_length,
            placeholder: None,
            format: TextFormat::Plain,
        }),
        required,
        initial: None,
    }
}

fn field(name: &str, kind: FieldKind) -> Field {
    Field {
        name: name.to_owned(),
        label: name.to_uppercase(),
        kind,
        required: false,
        initial: None,
    }
}

/// A synthetic form: a required name, a bounded count, a choice and a toggle.
fn spec() -> FormSpec {
    FormSpec {
        title: "New item".to_owned(),
        fields: vec![
            text_field("name", true, 8),
            field(
                "count",
                FieldKind::Number(NumberOptions {
                    min: Some(0.0),
                    max: Some(10.0),
                    step: None,
                }),
            ),
            field(
                "source",
                FieldKind::Choice(vec![ChoiceOption {
                    value: "web".to_owned(),
                    label: "Web".to_owned(),
                }]),
            ),
            field("urgent", FieldKind::Toggle),
        ],
        submit_label: "Create".to_owned(),
    }
}

fn value(name: &str, value: &str) -> FieldValue {
    FieldValue {
        name: name.to_owned(),
        value: value.to_owned(),
    }
}

fn error(name: &str, message: &str) -> FieldError {
    FieldError {
        name: name.to_owned(),
        message: message.to_owned(),
    }
}

fn answer(token: u64, reply: PluginReply) -> PluginAnswer {
    PluginAnswer { token, reply }
}

fn token(call: &PluginCall) -> u64 {
    call.token
}

/// The synthetic plugin's form, open and editable.
fn open_form(commands: &mut PluginCommands) {
    let chosen = commands.choose("test", "test.new-item", true).0;
    let notice = commands.receive(answer(token(&chosen), PluginReply::Form(Ok(spec()))));
    assert_eq!(
        notice,
        Some(Notice::FormReady {
            title: "New item".to_owned()
        })
    );
}

/// The form open, with a valid name, submitted once.
fn submitted(commands: &mut PluginCommands) -> u64 {
    open_form(commands);
    commands.set_value("name", "Ada").expect("editable");
    token(&commands.submit().expect("sent"))
}

/// `notice`, named for the set-aside call `command`.
fn named(command: &str, notice: Notice) -> Notice {
    Notice::SetAside {
        command: command.to_owned(),
        notice: Box::new(notice),
    }
}

fn uncertain() -> Outcome {
    Outcome::Uncertain("synthetic".to_owned())
}

#[test]
fn the_host_checks_required_lengths_ranges_choices_and_toggles() {
    let checked = |values: &[FieldValue]| host_checks(&spec(), values);
    assert_eq!(checked(&[]), vec![error("name", "Required.")]);
    assert_eq!(
        checked(&[value("name", "   ")]),
        vec![error("name", "Required.")]
    );
    assert_eq!(
        checked(&[value("name", "ninechars")]),
        vec![error("name", "At most 8 characters.")]
    );
    // Eight characters, counted as characters rather than bytes.
    assert!(checked(&[value("name", "éééééééé")]).is_empty());
    assert_eq!(
        checked(&[value("name", "Ada"), value("count", "eleven")]),
        vec![error("count", "Enter a number.")]
    );
    assert_eq!(
        checked(&[value("name", "Ada"), value("count", "NaN")]),
        vec![error("count", "Enter a number.")]
    );
    assert_eq!(
        checked(&[value("name", "Ada"), value("count", "-1")]),
        vec![error("count", "At least 0.")]
    );
    assert_eq!(
        checked(&[value("name", "Ada"), value("count", "10.5")]),
        vec![error("count", "At most 10.")]
    );
    assert_eq!(
        checked(&[value("name", "Ada"), value("source", "mail")]),
        vec![error("source", "Choose one of the options.")]
    );
    assert_eq!(
        checked(&[value("name", "Ada"), value("urgent", "yes")]),
        vec![error("urgent", "Choose on or off.")]
    );
    let valid = [
        value("name", "Ada"),
        value("count", "10"),
        value("source", "web"),
        value("urgent", "true"),
    ];
    assert!(checked(&valid).is_empty());
}

#[test]
fn money_must_be_a_number_and_dates_are_left_to_the_plugin() {
    let spec = FormSpec {
        fields: vec![
            field("value", FieldKind::Money("USD".to_owned())),
            field("due", FieldKind::Date),
        ],
        ..spec()
    };
    let errors = host_checks(&spec, &[value("value", "ten"), value("due", "someday")]);
    assert_eq!(errors, vec![error("value", "Enter a number.")]);
}

#[test]
fn a_failed_host_check_keeps_the_input_names_the_first_field_and_sends_nothing() {
    let mut commands = PluginCommands::default();
    open_form(&mut commands);
    commands.set_value("count", "99").expect("editable");
    let refused = commands.submit();
    assert_eq!(refused, Err(Refusal::Invalid(error("name", "Required."))));
    let form = commands.form().expect("still open");
    assert_eq!(form.value("count"), Some("99"));
    assert_eq!(
        form.errors(),
        [error("name", "Required."), error("count", "At most 10.")]
    );
    // The form is still editable, and a fixed field loses its error.
    commands.set_value("name", "Ada").expect("editable");
    assert_eq!(
        commands.form().expect("open").errors(),
        [error("count", "At most 10.")]
    );
    assert_eq!(
        commands.validate(),
        Err(Refusal::Invalid(error("count", "At most 10.")))
    );
}

#[test]
fn a_form_starts_with_its_initial_values_and_toggles_off() {
    let mut commands = PluginCommands::default();
    let chosen = commands.choose("test", "test.new-item", true).0;
    let mut with_initial = spec();
    with_initial.fields[0].initial = Some("Grace".to_owned());
    commands.receive(answer(token(&chosen), PluginReply::Form(Ok(with_initial))));
    let form = commands.form().expect("open");
    assert_eq!(form.value("name"), Some("Grace"));
    assert_eq!(form.value("urgent"), Some("false"));
    assert_eq!(form.value("count"), Some(""));
    assert_eq!(
        commands.set_value("missing", "x"),
        Err(Refusal::UnknownField)
    );
}

#[test]
fn the_plugins_validation_reports_its_first_failing_field_in_form_order() {
    let mut commands = PluginCommands::default();
    open_form(&mut commands);
    commands.set_value("name", "Ada").expect("editable");
    let call = commands.validate().expect("sent");
    let CallKind::Validate(values) = &call.kind else {
        panic!("a validation: {call:?}");
    };
    assert!(values.contains(&value("name", "Ada")));
    let errors = vec![error("source", "Pick one."), error("name", "Taken.")];
    let notice = commands.receive(answer(token(&call), PluginReply::Validated(Ok(errors))));
    assert_eq!(notice, Some(Notice::Invalid(error("name", "Taken."))));
    assert_eq!(
        commands.form().expect("open").errors(),
        [error("name", "Taken."), error("source", "Pick one.")]
    );
}

#[test]
fn an_older_validation_answer_is_dropped_once_a_newer_one_was_sent() {
    let mut commands = PluginCommands::default();
    open_form(&mut commands);
    commands.set_value("name", "Ada").expect("editable");
    let older = commands.validate().expect("sent");
    let newer = commands.validate().expect("sent");
    let stale = PluginReply::Validated(Ok(vec![error("name", "Old.")]));
    assert_eq!(commands.receive(answer(token(&older), stale)), None);
    assert!(commands.form().expect("open").errors().is_empty());
    let fresh = PluginReply::Validated(Ok(vec![error("name", "Taken.")]));
    assert_eq!(
        commands.receive(answer(token(&newer), fresh)),
        Some(Notice::Invalid(error("name", "Taken.")))
    );
}

#[test]
fn an_edit_after_validation_was_sent_makes_its_answer_stale() {
    let mut commands = PluginCommands::default();
    open_form(&mut commands);
    commands.set_value("name", "Ada").expect("editable");
    let sent = commands.validate().expect("sent");
    commands.set_value("name", "Grace").expect("editable");
    let stale = PluginReply::Validated(Ok(vec![error("name", "Ada is taken.")]));
    assert_eq!(commands.receive(answer(token(&sent), stale)), None);
    assert!(commands.form().expect("open").errors().is_empty());

    // A validation the host's checks refuse also leaves an earlier one stale.
    let sent = commands.validate().expect("sent");
    commands.set_value("count", "99").expect("editable");
    assert!(commands.validate().is_err());
    let stale = PluginReply::Validated(Ok(vec![error("source", "Old.")]));
    assert_eq!(commands.receive(answer(token(&sent), stale)), None);
}

#[test]
fn a_repeated_enter_while_a_submission_is_on_its_way_or_pending_does_nothing() {
    let mut commands = PluginCommands::default();
    let first = submitted(&mut commands);
    assert!(commands.is_submitting());
    assert_eq!(commands.submit(), Err(Refusal::Busy));
    assert_eq!(commands.set_value("name", "Grace"), Err(Refusal::Busy));
    let pending = PluginReply::Submitted(Submitted::Pending { request: 41 });
    assert_eq!(
        commands.receive(answer(first, pending)),
        Some(Notice::Pending)
    );
    assert!(commands.is_submitting());
    assert_eq!(commands.submit(), Err(Refusal::Busy));
}

#[test]
fn the_submission_carries_the_owners_values_for_the_chosen_command() {
    let mut commands = PluginCommands::default();
    open_form(&mut commands);
    commands.set_value("name", "Ada").expect("editable");
    let call = commands.submit().expect("sent");
    let PluginCall {
        plugin,
        command,
        kind: CallKind::Submit(values),
        ..
    } = call
    else {
        panic!("a submission");
    };
    assert_eq!(
        (plugin.as_str(), command.as_str()),
        ("test", "test.new-item")
    );
    assert_eq!(
        values,
        [
            value("name", "Ada"),
            value("count", ""),
            value("source", ""),
            value("urgent", "false"),
        ]
    );
}

#[test]
fn done_ends_the_command_and_failed_keeps_the_input_for_another_try() {
    let mut commands = PluginCommands::default();
    let first = submitted(&mut commands);
    let failure = Failure {
        kind: FailureKind::Unavailable,
        message: "synthetic service down".to_owned(),
    };
    let failed = PluginReply::Submitted(Submitted::Finished(Outcome::Failed(failure.clone())));
    assert_eq!(
        commands.receive(answer(first, failed)),
        Some(Notice::Outcome(Outcome::Failed(failure)))
    );
    assert_eq!(commands.form().expect("open").value("name"), Some("Ada"));
    let second = token(&commands.submit().expect("sent again"));
    let done = PluginReply::Submitted(Submitted::Finished(Outcome::Done(Some("made".into()))));
    assert_eq!(
        commands.receive(answer(second, done)),
        Some(Notice::Outcome(Outcome::Done(Some("made".into()))))
    );
    assert!(commands.form().is_none());
}

#[test]
fn an_uncertain_submission_is_shown_and_never_sent_again() {
    let mut commands = PluginCommands::default();
    let first = submitted(&mut commands);
    let reply = PluginReply::Submitted(Submitted::Finished(uncertain()));
    assert_eq!(
        commands.receive(answer(first, reply)),
        Some(Notice::Outcome(uncertain()))
    );
    assert!(commands.form().is_some());
    assert_eq!(commands.submit(), Err(Refusal::Uncertain));
    assert_eq!(commands.validate(), Err(Refusal::Uncertain));
}

#[test]
fn an_invalid_submission_keeps_the_input_and_can_be_sent_again() {
    let mut commands = PluginCommands::default();
    let first = submitted(&mut commands);
    let invalid = PluginReply::Submitted(Submitted::Invalid(vec![error("name", "Taken.")]));
    assert_eq!(
        commands.receive(answer(first, invalid)),
        Some(Notice::Invalid(error("name", "Taken.")))
    );
    assert_eq!(commands.form().expect("open").value("name"), Some("Ada"));
    assert!(commands.submit().is_ok());
}

#[test]
fn a_pending_outcome_counts_only_for_its_own_plugin_and_request() {
    let mut commands = PluginCommands::default();
    let first = submitted(&mut commands);
    let pending = PluginReply::Submitted(Submitted::Pending { request: 41 });
    commands.receive(answer(first, pending));
    let late = |plugin: &str, request| LateOutcome {
        plugin: plugin.to_owned(),
        request,
        outcome: Outcome::Done(None),
    };
    assert_eq!(commands.receive_outcome(late("test", 40)), None);
    assert_eq!(commands.receive_outcome(late("other", 41)), None);
    assert!(commands.is_submitting());
    assert_eq!(
        commands.receive_outcome(late("test", 41)),
        Some(Notice::Outcome(Outcome::Done(None)))
    );
    assert!(commands.form().is_none());
}

#[test]
fn closing_the_palette_drops_the_form_but_still_reports_its_submissions_outcome() {
    let mut commands = PluginCommands::default();
    // Closed while the submission is on its way: its answer is still told, with no form.
    let first = submitted(&mut commands);
    commands.abandon();
    assert!(commands.form().is_none());
    let reply = PluginReply::Submitted(Submitted::Finished(uncertain()));
    assert_eq!(
        commands.receive(answer(first, reply)),
        Some(named("New item", Notice::Outcome(uncertain())))
    );
    assert!(commands.form().is_none());

    // Closed while pending: the host's uncertain outcome is still told.
    let second = submitted(&mut commands);
    let reply = PluginReply::Submitted(Submitted::Pending { request: 42 });
    commands.receive(answer(second, reply));
    commands.abandon();
    let late = |request| LateOutcome {
        plugin: "test".to_owned(),
        request,
        outcome: uncertain(),
    };
    assert_eq!(commands.receive_outcome(late(41)), None);
    assert_eq!(
        commands.receive_outcome(late(42)),
        Some(named("New item", Notice::Outcome(uncertain())))
    );
    assert!(commands.form().is_none());
    // Told once.
    assert_eq!(commands.receive_outcome(late(42)), None);
}

#[test]
fn closing_the_palette_drops_an_unsent_form_and_its_validation() {
    let mut commands = PluginCommands::default();
    open_form(&mut commands);
    commands.set_value("name", "Ada").expect("editable");
    let sent = commands.validate().expect("sent");
    commands.abandon();
    let reply = PluginReply::Validated(Ok(vec![error("name", "Taken.")]));
    assert_eq!(commands.receive(answer(token(&sent), reply)), None);
    assert!(commands.form().is_none());
}

#[test]
fn choosing_another_command_leaves_the_first_forms_outcome_on_the_status_line_only() {
    let mut commands = PluginCommands::default();
    let first = submitted(&mut commands);
    let other = commands.choose("test", "test.echo", true).0;
    let reply = PluginReply::Submitted(Submitted::Finished(Outcome::Done(None)));
    assert_eq!(
        commands.receive(answer(first, reply)),
        Some(named("New item", Notice::Outcome(Outcome::Done(None))))
    );
    assert!(commands.form().is_none());
    // The second command's own answer still counts.
    assert_eq!(
        commands.receive(answer(token(&other), PluginReply::Ran(Ok(())))),
        Some(Notice::Outcome(Outcome::Done(None)))
    );
}

#[test]
fn a_command_that_ran_is_reported_after_the_palette_closed_but_a_form_is_not_opened() {
    let mut commands = PluginCommands::default();
    let ran = commands.choose("test", "test.echo", true).0;
    commands.abandon();
    let failure = Failure {
        kind: FailureKind::Host,
        message: "synthetic".to_owned(),
    };
    assert_eq!(
        commands.receive(answer(token(&ran), PluginReply::Ran(Err(failure.clone())))),
        Some(Notice::Outcome(Outcome::Failed(failure)))
    );

    let form = commands.choose("test", "test.new-item", true).0;
    commands.abandon();
    let title = "New item".to_owned();
    assert_eq!(
        commands.receive(answer(token(&form), PluginReply::Form(Ok(spec())))),
        Some(Notice::FormNotShown { title })
    );
    assert!(commands.form().is_none());
}

#[test]
fn a_form_chosen_after_the_palette_entry_ended_is_told_not_opened() {
    // The real order today: `PaletteEvent::Run` ends the entry, then the command is chosen.
    let mut commands = PluginCommands::default();
    commands.abandon();
    let chosen = commands.choose("test", "test.new-item", false).0;
    let title = "New item".to_owned();
    assert_eq!(
        commands.receive(answer(token(&chosen), PluginReply::Form(Ok(spec())))),
        Some(Notice::FormNotShown { title })
    );
    assert!(commands.form().is_none());
    assert_eq!(commands.submit(), Err(Refusal::NoForm));
}

#[test]
fn a_reply_of_the_wrong_kind_for_its_token_changes_nothing() {
    let mut commands = PluginCommands::default();
    let first = submitted(&mut commands);
    let stray = PluginReply::Validated(Ok(vec![error("name", "Stray.")]));
    assert_eq!(commands.receive(answer(first, stray)), None);
    assert!(commands.is_submitting());
}

#[test]
fn a_submission_closed_on_its_way_that_then_answers_pending_still_reports_its_outcome() {
    let mut commands = PluginCommands::default();
    let first = submitted(&mut commands);
    commands.abandon();
    let pending = PluginReply::Submitted(Submitted::Pending { request: 41 });
    assert_eq!(commands.receive(answer(first, pending)), None);
    let late = LateOutcome {
        plugin: "test".to_owned(),
        request: 41,
        outcome: uncertain(),
    };
    assert_eq!(
        commands.receive_outcome(late),
        Some(named("New item", Notice::Outcome(uncertain())))
    );
    assert!(commands.form().is_none());
}

#[test]
fn a_closed_submissions_invalid_answer_names_its_field_or_says_nothing_happened() {
    let mut commands = PluginCommands::default();
    let first = submitted(&mut commands);
    commands.abandon();
    let invalid = PluginReply::Submitted(Submitted::Invalid(vec![error("name", "Taken.")]));
    assert_eq!(
        commands.receive(answer(first, invalid)),
        Some(named("New item", Notice::Invalid(error("name", "Taken."))))
    );

    let second = submitted(&mut commands);
    commands.abandon();
    let unnamed = PluginReply::Submitted(Submitted::Invalid(Vec::new()));
    let Some(Notice::SetAside { notice, .. }) = commands.receive(answer(second, unnamed)) else {
        panic!("an invalid answer naming no field is still told");
    };
    let Notice::Failed(failure) = *notice else {
        panic!("nothing happened");
    };
    assert_eq!(failure.kind, FailureKind::InvalidInput);
}

#[test]
fn a_command_run_still_waiting_when_another_is_chosen_is_still_reported_by_name() {
    let mut commands = PluginCommands::default();
    let first = commands.choose("test", "test.echo", true).0;
    let second = commands.choose("test", "test.recall", true).0;
    // The first's late answer arrives while the second is current, and names its command.
    assert_eq!(
        commands.receive(answer(token(&first), PluginReply::Ran(Ok(())))),
        Some(named("test.echo", Notice::Outcome(Outcome::Done(None))))
    );
    assert_eq!(
        commands.receive(answer(token(&second), PluginReply::Ran(Ok(())))),
        Some(Notice::Outcome(Outcome::Done(None)))
    );

    // A form for a command set aside the same way is dropped: nobody can see it.
    let form = commands.choose("test", "test.new-item", true).0;
    let (superseding, forgotten) = commands.choose("test", "test.echo", true);
    assert_eq!(forgotten, None);
    assert_ne!(superseding.token, form.token);
    assert_eq!(
        commands.receive(answer(token(&form), PluginReply::Form(Ok(spec())))),
        None
    );
    assert!(commands.form().is_none());
}

#[test]
fn a_stopped_plugin_forgets_its_set_aside_submissions_that_wait_for_an_outcome() {
    let mut commands = PluginCommands::default();
    let first = submitted(&mut commands);
    let pending = PluginReply::Submitted(Submitted::Pending { request: 41 });
    commands.receive(answer(first, pending));
    commands.abandon();
    // One on its way still gets its answer, which the stopped worker always sends.
    let second = submitted(&mut commands);
    commands.abandon();
    assert_eq!(commands.plugin_stopped("other"), None);
    assert_eq!(commands.plugin_stopped("test"), None);
    let late = LateOutcome {
        plugin: "test".to_owned(),
        request: 41,
        outcome: uncertain(),
    };
    assert_eq!(commands.receive_outcome(late), None);
    let reply = PluginReply::Submitted(Submitted::Finished(uncertain()));
    assert_eq!(
        commands.receive(answer(second, reply)),
        Some(named("New item", Notice::Outcome(uncertain())))
    );
}

#[test]
fn a_current_pending_form_whose_plugin_stopped_may_have_happened_and_is_not_busy_for_good() {
    let mut commands = PluginCommands::default();
    let first = submitted(&mut commands);
    let pending = PluginReply::Submitted(Submitted::Pending { request: 41 });
    commands.receive(answer(first, pending));
    assert_eq!(commands.plugin_stopped("other"), None);
    assert!(commands.is_submitting());
    let Some(Notice::Outcome(Outcome::Uncertain(_))) = commands.plugin_stopped("test") else {
        panic!("a pending write whose plugin stopped may have happened");
    };
    assert!(!commands.is_submitting());
    assert_eq!(commands.submit(), Err(Refusal::Uncertain));
    // Nothing was set aside for the stopped instance.
    assert_eq!(commands.abandon(), None);
}

#[test]
fn past_the_limit_the_oldest_set_aside_call_is_forgotten_and_the_owner_is_told() {
    let mut commands = PluginCommands::default();
    let mut forgotten = Vec::new();
    let tokens: Vec<u64> = (0..=MAX_ORPHANS)
        .map(|_| {
            let token = submitted(&mut commands);
            forgotten.extend(commands.abandon());
            token
        })
        .collect();
    let may_have_written = Notice::Forgotten {
        may_have_written: true,
    };
    assert_eq!(forgotten, [named("New item", may_have_written)]);
    let done = || PluginReply::Submitted(Submitted::Finished(Outcome::Done(None)));
    assert_eq!(commands.receive(answer(tokens[0], done())), None);
    for &kept in &tokens[1..] {
        assert_eq!(
            commands.receive(answer(kept, done())),
            Some(named("New item", Notice::Outcome(Outcome::Done(None))))
        );
    }
}

#[test]
fn a_forgotten_choice_is_not_said_to_have_written() {
    let mut commands = PluginCommands::default();
    let mut forgotten = None;
    for _ in 0..=MAX_ORPHANS + 1 {
        forgotten = commands
            .choose("test", "test.new-item", true)
            .1
            .or(forgotten);
    }
    let neutral = Notice::Forgotten {
        may_have_written: false,
    };
    assert_eq!(forgotten, Some(named("test.new-item", neutral)));
}
