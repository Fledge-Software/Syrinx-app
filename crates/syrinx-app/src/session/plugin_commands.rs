//! Carries the palette's plugin commands and forms to the plugin host and brings each answer
//! back as a session event, without the worker ever waiting on a plugin (#29).

use std::time::Duration;

use futures::channel::mpsc::UnboundedSender;

use syrinx_core::plugin_forms::{
    ChoiceOption, Failure, FailureKind, Field, FieldError, FieldKind, FieldValue, FormSpec,
    LateOutcome, NumberOptions, Outcome, PluginAnswer, PluginCall, PluginReply, Submitted,
    TextFormat, TextOptions,
};
use syrinx_plugin_host::HostError;
use syrinx_plugin_host::bindings::exports::syrinx::plugin::forms as wit;
use syrinx_plugin_host::bindings::syrinx::plugin::types::{Outcome as WitOutcome, PluginError};
use syrinx_plugin_host::owner::{OwnerRuntime, StoreServices};
use syrinx_plugin_host::session::{CallResult, Session};
use syrinx_plugin_host::worker::{Pending, PluginEvent};

use super::{SessionError, SessionEvent, emit};

/// How long the worker waits between looks while a plugin's answer is still to come.
pub(super) const ANSWER_POLL: Duration = Duration::from_millis(15);

/// The plugin is not loaded this session, or its queue is full: nothing was sent to it.
const NOT_AVAILABLE: &str = "That plugin is not available.";
/// The plugin stopped, or was stopped, before it answered a call that has no effect.
const NO_ANSWER: &str = "The plugin stopped before it answered.";
/// The plugin stopped during a submission, so the write may have happened.
const MAY_HAVE_HAPPENED: &str = "The plugin stopped while submitting";

/// The calls queued with plugin workers whose answers are still to come.
#[derive(Default)]
pub(super) struct PluginCalls {
    awaited: Vec<Awaited>,
}

struct Awaited {
    token: u64,
    asked: Asked,
    answer: Pending<PluginReply>,
}

/// Which export a call reaches, which decides what an answer that never came means.
#[derive(Clone, Copy)]
enum Asked {
    Run,
    Form,
    Validate,
    Submit,
}

impl PluginCalls {
    /// Queue `call` with its plugin's worker; one that cannot be queued is answered at once.
    pub(super) fn start(
        &mut self,
        runtime: Option<&mut OwnerRuntime>,
        call: PluginCall,
        events: &UnboundedSender<SessionEvent>,
    ) -> Result<(), SessionError> {
        let token = call_token(&call);
        match queue(runtime, call) {
            Ok((asked, answer)) => {
                self.awaited.push(Awaited {
                    token,
                    asked,
                    answer,
                });
                Ok(())
            }
            Err(reply) => emit(events, SessionEvent::Plugin(PluginAnswer { token, reply })),
        }
    }

    /// Whether an answer is still to come, so the worker looks again soon.
    pub(super) fn is_waiting(&self) -> bool {
        !self.awaited.is_empty()
    }

    /// Send every answer that has arrived; never waits.
    pub(super) fn collect(
        &mut self,
        events: &UnboundedSender<SessionEvent>,
    ) -> Result<(), SessionError> {
        let mut answers = Vec::new();
        self.awaited.retain(|awaited| {
            let Some(answered) = awaited.answer.try_answer() else {
                return true;
            };
            let reply = match answered {
                Ok(reply) => reply,
                Err(_) => awaited.asked.unanswered(),
            };
            answers.push(PluginAnswer {
                token: awaited.token,
                reply,
            });
            false
        });
        answers
            .into_iter()
            .try_for_each(|answer| emit(events, SessionEvent::Plugin(answer)))
    }
}

/// Each write the host settled itself (`PluginEvent::Outcome`), for the palette to match to
/// the submission that answered pending. Other events are the diagnostic report's (#268).
pub(super) fn forward_outcomes(
    raised: &[PluginEvent],
    events: &UnboundedSender<SessionEvent>,
) -> Result<(), SessionError> {
    for event in raised {
        let PluginEvent::Outcome {
            plugin,
            request,
            outcome,
        } = event
        else {
            continue;
        };
        let late = LateOutcome {
            plugin: plugin.clone(),
            request: *request,
            outcome: outcome_from(outcome.clone()),
        };
        emit(events, SessionEvent::PluginOutcome(late))?;
    }
    Ok(())
}

fn call_token(call: &PluginCall) -> u64 {
    match call {
        PluginCall::Choose { token, .. }
        | PluginCall::Validate { token, .. }
        | PluginCall::Submit { token, .. } => *token,
    }
}

/// Queue `call`, choosing `commands.run` or `forms.form-for` from the manifest for `Choose`.
/// The error is the reply for a call that was never sent.
fn queue(
    runtime: Option<&mut OwnerRuntime>,
    call: PluginCall,
) -> Result<(Asked, Pending<PluginReply>), PluginReply> {
    let Some(runtime) = runtime else {
        return Err(Asked::Run.refused());
    };
    match call {
        PluginCall::Choose {
            plugin, command, ..
        } => match runtime.command_has_form(&plugin, &command) {
            None => Err(Asked::Run.refused()),
            Some(false) => send(runtime, &plugin, Asked::Run, move |session| {
                let ran = session.run_command(&command)?;
                Ok(PluginReply::Ran(ran.map_err(failure_from)))
            }),
            Some(true) => send(runtime, &plugin, Asked::Form, move |session| {
                let spec = session.form_for(&command)?;
                Ok(PluginReply::Form(spec.map(form_from).map_err(failure_from)))
            }),
        },
        PluginCall::Validate {
            plugin,
            command,
            values,
            ..
        } => send(runtime, &plugin, Asked::Validate, move |session| {
            let checked = session.validate_form(&command, &pairs(&values))?;
            Ok(PluginReply::Validated(
                checked.map(errors_from).map_err(failure_from),
            ))
        }),
        PluginCall::Submit {
            plugin,
            command,
            values,
            ..
        } => send(runtime, &plugin, Asked::Submit, move |session| {
            Ok(PluginReply::Submitted(submit(session, &command, &values)))
        }),
    }
}

fn send(
    runtime: &mut OwnerRuntime,
    plugin: &str,
    asked: Asked,
    call: impl FnOnce(&mut Session<StoreServices>) -> CallResult<PluginReply> + Send + 'static,
) -> Result<(Asked, Pending<PluginReply>), PluginReply> {
    match runtime.call(plugin, call) {
        Ok(answer) => Ok((asked, answer)),
        Err(_) => Err(asked.refused()),
    }
}

/// `forms.validate`, then `forms.submit` only if the plugin found nothing wrong, in one job so
/// the two see the same values. Only a stop during `submit` itself leaves the write uncertain.
fn submit(session: &mut Session<StoreServices>, command: &str, values: &[FieldValue]) -> Submitted {
    let pairs = pairs(values);
    match session.validate_form(command, &pairs) {
        Ok(Ok(errors)) if errors.is_empty() => {}
        Ok(Ok(errors)) => return Submitted::Invalid(errors_from(errors)),
        Ok(Err(error)) => return Submitted::Finished(Outcome::Failed(failure_from(error))),
        Err(_) => return Submitted::Finished(Outcome::Failed(host_failure(NO_ANSWER))),
    }
    match session.submit(command, &pairs) {
        Ok((request, result)) => submitted_from(request, result),
        Err(HostError::SubmitStopped { .. }) => {
            Submitted::Finished(Outcome::Uncertain(MAY_HAVE_HAPPENED.to_owned()))
        }
        Err(_) => Submitted::Finished(Outcome::Failed(host_failure(NO_ANSWER))),
    }
}

impl Asked {
    /// The reply for a call that never reached the plugin: nothing happened.
    fn refused(self) -> PluginReply {
        self.failed(host_failure(NOT_AVAILABLE))
    }

    /// The reply when the worker stopped before answering. A submission may have happened.
    fn unanswered(self) -> PluginReply {
        if let Self::Submit = self {
            let outcome = Outcome::Uncertain(MAY_HAVE_HAPPENED.to_owned());
            return PluginReply::Submitted(Submitted::Finished(outcome));
        }
        self.failed(host_failure(NO_ANSWER))
    }

    fn failed(self, failure: Failure) -> PluginReply {
        match self {
            Self::Run => PluginReply::Ran(Err(failure)),
            Self::Form => PluginReply::Form(Err(failure)),
            Self::Validate => PluginReply::Validated(Err(failure)),
            Self::Submit => PluginReply::Submitted(Submitted::Finished(Outcome::Failed(failure))),
        }
    }
}

fn host_failure(message: &str) -> Failure {
    Failure {
        kind: FailureKind::Host,
        message: message.to_owned(),
    }
}

fn pairs(values: &[FieldValue]) -> Vec<(&str, &str)> {
    values
        .iter()
        .map(|value| (value.name.as_str(), value.value.as_str()))
        .collect()
}

/// The host already bounded a plugin's error text (`validate::bound_error`).
fn failure_from(error: PluginError) -> Failure {
    let (kind, message) = match error {
        PluginError::InvalidInput(message) => (FailureKind::InvalidInput, message),
        PluginError::NotFound(message) => (FailureKind::NotFound, message),
        PluginError::Denied(message) => (FailureKind::Denied, message),
        PluginError::Revoked(message) => (FailureKind::Revoked, message),
        PluginError::Unavailable(message) => (FailureKind::Unavailable, message),
        PluginError::Internal(message) => (FailureKind::Internal, message),
        PluginError::Cancelled(message) => (FailureKind::Cancelled, message),
    };
    Failure { kind, message }
}

fn outcome_from(outcome: WitOutcome) -> Outcome {
    match outcome {
        WitOutcome::Done(detail) => Outcome::Done(detail),
        WitOutcome::Failed(error) => Outcome::Failed(failure_from(error)),
        WitOutcome::Uncertain(detail) => Outcome::Uncertain(detail),
    }
}

/// The host has already checked a pending answer's request id (`Session::submit`).
fn submitted_from(request: u64, result: wit::SubmitResult) -> Submitted {
    match result {
        wit::SubmitResult::Finished(outcome) => Submitted::Finished(outcome_from(outcome)),
        wit::SubmitResult::Pending(_) => Submitted::Pending { request },
        wit::SubmitResult::Invalid(errors) => Submitted::Invalid(errors_from(errors)),
    }
}

fn errors_from(errors: Vec<wit::FieldError>) -> Vec<FieldError> {
    errors
        .into_iter()
        .map(|error| FieldError {
            name: error.name,
            message: error.message,
        })
        .collect()
}

fn form_from(spec: wit::FormSpec) -> FormSpec {
    FormSpec {
        title: spec.title,
        fields: spec.fields.into_iter().map(field_from).collect(),
        submit_label: spec.submit_label,
    }
}

fn field_from(field: wit::Field) -> Field {
    Field {
        name: field.name,
        label: field.label,
        kind: kind_from(field.kind),
        required: field.required,
        initial: field.initial,
    }
}

fn kind_from(kind: wit::FieldKind) -> FieldKind {
    match kind {
        wit::FieldKind::Text(options) => FieldKind::Text(text_from(options)),
        wit::FieldKind::Multiline(options) => FieldKind::Multiline(text_from(options)),
        wit::FieldKind::Number(options) => FieldKind::Number(NumberOptions {
            min: options.min,
            max: options.max,
            step: options.step,
        }),
        wit::FieldKind::Money(currency) => FieldKind::Money(currency),
        wit::FieldKind::Choice(options) => FieldKind::Choice(
            options
                .into_iter()
                .map(|option| ChoiceOption {
                    value: option.value,
                    label: option.label,
                })
                .collect(),
        ),
        wit::FieldKind::Toggle => FieldKind::Toggle,
        wit::FieldKind::Date => FieldKind::Date,
        wit::FieldKind::DateTime => FieldKind::DateTime,
    }
}

fn text_from(options: wit::TextOptions) -> TextOptions {
    let format = match options.format {
        wit::TextFormat::Plain => TextFormat::Plain,
        wit::TextFormat::Email => TextFormat::Email,
        wit::TextFormat::WebAddress => TextFormat::WebAddress,
    };
    TextOptions {
        max_length: options.max_length,
        placeholder: options.placeholder,
        format,
    }
}
