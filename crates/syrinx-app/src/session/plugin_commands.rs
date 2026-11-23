//! Carries the palette's plugin commands and forms to the plugin host (#29). Each call's job
//! sends its own answer into the session's event stream from the plugin's thread, and writes
//! the host settles itself arrive through the runtime's event watch, so nothing waits or polls.

use futures::channel::mpsc::UnboundedSender;
use syrinx_core::plugin_forms::{
    CallKind, ChoiceOption, Failure, FailureKind, Field, FieldError, FieldKind, FieldValue,
    FormSpec, LateOutcome, NumberOptions, Outcome, PluginAnswer, PluginCall, PluginReply,
    Submitted, TextFormat, TextOptions,
};
use syrinx_plugin_host::HostError;
use syrinx_plugin_host::bindings::exports::syrinx::plugin::forms as wit;
use syrinx_plugin_host::bindings::syrinx::plugin::types::{Outcome as WitOutcome, PluginError};
use syrinx_plugin_host::owner::{OwnerRuntime, StoreServices};
use syrinx_plugin_host::session::Session;
use syrinx_plugin_host::worker::PluginEvent;

use super::SessionEvent;

/// The plugin is not loaded this session, or does not declare the command.
const NOT_AVAILABLE: &str = "That plugin is not available.";
/// The job never ran: the plugin's queue was full, or it stopped or failed to start first.
const NOT_SENT: &str = "The plugin could not take the request, so nothing was sent.";
/// The host refused the call before the plugin saw it.
const REFUSED: &str = "Syrinx refused the request before the plugin saw it.";
/// The plugin trapped or was stopped before it answered a call that has no effect.
const STOPPED: &str = "The plugin stopped before it answered.";
/// The plugin's code could not be loaded to answer.
const CANNOT_START: &str = "The plugin could not be started.";
/// Shown inside "This may have happened (...)": the plugin stopped once `forms.submit` began.
const MAY_HAVE_HAPPENED: &str = "the plugin stopped while saving it";

/// Queue `call` with its plugin's worker. Its answer is sent from the plugin's thread, or at
/// once when the plugin cannot take it.
pub(super) fn start(
    runtime: Option<&mut OwnerRuntime>,
    call: PluginCall,
    events: &UnboundedSender<SessionEvent>,
) {
    let PluginCall {
        token,
        plugin,
        command,
        kind,
    } = call;
    let has_form = runtime
        .as_deref()
        .and_then(|runtime| runtime.command_has_form(&plugin, &command));
    let asked = Asked::of(&kind, has_form == Some(true));
    let reply = Reply::new(token, asked, events.clone());
    let (Some(runtime), Some(_)) = (runtime, has_form) else {
        return reply.send(asked.failed(host_failure(NOT_AVAILABLE)));
    };
    let job = move |session: &mut Session<StoreServices>| {
        let mut reply = reply;
        let answered = answer(session, &command, kind, asked, &mut reply);
        reply.send(answered);
        Ok(())
    };
    // The answer travels through `events`, so the `Pending` is not kept; a job that could not
    // be queued was dropped unrun, and its `Reply` has already said so.
    match runtime.call(&plugin, job) {
        Ok(pending) => drop(pending),
        Err(refused) => drop(refused),
    }
}

/// Let the window hear of each write the host settles itself (`uncertain` once its plugin
/// stops) as it is settled. A job sends its answer from the plugin's thread before that thread
/// settles the job's writes, so an answer always reaches the window ahead of its own outcome.
pub(super) fn watch_outcomes(runtime: &OwnerRuntime, events: &UnboundedSender<SessionEvent>) {
    let events = events.clone();
    runtime.watch_events(move |event| {
        let PluginEvent::Outcome {
            plugin,
            request,
            outcome,
        } = event
        else {
            return;
        };
        let late = LateOutcome {
            plugin: plugin.clone(),
            request: *request,
            outcome: outcome_from(outcome.clone()),
        };
        tell(&events, SessionEvent::PluginOutcome(late));
    });
}

/// Which export a call reaches, which decides what an answer that never came means.
#[derive(Clone, Copy)]
enum Asked {
    Run,
    Form,
    Validate,
    Submit,
}

impl Asked {
    fn of(kind: &CallKind, has_form: bool) -> Self {
        match kind {
            CallKind::Choose if has_form => Self::Form,
            CallKind::Choose => Self::Run,
            CallKind::Validate(_) => Self::Validate,
            CallKind::Submit(_) => Self::Submit,
        }
    }

    /// This call's reply when it failed without effect.
    fn failed(self, failure: Failure) -> PluginReply {
        match self {
            Self::Run => PluginReply::Ran(Err(failure)),
            Self::Form => PluginReply::Form(Err(failure)),
            Self::Validate => PluginReply::Validated(Err(failure)),
            Self::Submit => PluginReply::Submitted(Submitted::Finished(Outcome::Failed(failure))),
        }
    }
}

/// One call's answer, sent once from whichever thread settles it. Dropped unsent, as when the
/// worker stopped before running the job, it says the job never ran; or, once `forms.submit`
/// had begun, that the write may have happened.
struct Reply {
    token: u64,
    asked: Asked,
    events: UnboundedSender<SessionEvent>,
    /// Set just before `forms.submit` is called.
    submitting: bool,
    sent: bool,
}

impl Reply {
    fn new(token: u64, asked: Asked, events: UnboundedSender<SessionEvent>) -> Self {
        Self {
            token,
            asked,
            events,
            submitting: false,
            sent: false,
        }
    }

    fn send(mut self, reply: PluginReply) {
        self.sent = true;
        let answer = PluginAnswer {
            token: self.token,
            reply,
        };
        tell(&self.events, SessionEvent::Plugin(answer));
    }
}

impl Drop for Reply {
    fn drop(&mut self) {
        if self.sent {
            return;
        }
        let reply = if self.submitting {
            may_have_happened()
        } else {
            self.asked.failed(host_failure(NOT_SENT))
        };
        let answer = PluginAnswer {
            token: self.token,
            reply,
        };
        tell(&self.events, SessionEvent::Plugin(answer));
    }
}

/// Send `event` to the window; once the window has closed there is no one left to tell.
fn tell(events: &UnboundedSender<SessionEvent>, event: SessionEvent) {
    if let Err(unsent) = events.unbounded_send(event) {
        drop(unsent.into_inner());
    }
}

/// Run the call on the plugin's thread and say what came of it.
fn answer(
    session: &mut Session<StoreServices>,
    command: &str,
    kind: CallKind,
    asked: Asked,
    reply: &mut Reply,
) -> PluginReply {
    match (asked, kind) {
        (Asked::Form, _) => form(session, command),
        (_, CallKind::Validate(values)) => validated(session, command, &values),
        (_, CallKind::Submit(values)) => submitted(session, command, &values, reply),
        (_, CallKind::Choose) => ran(session, command),
    }
}
fn ran(session: &mut Session<StoreServices>, command: &str) -> PluginReply {
    match session.run_command(command) {
        Ok(ran) => PluginReply::Ran(ran.map_err(failure_from)),
        Err(error) => PluginReply::Ran(Err(host_reason(session, &error))),
    }
}

fn form(session: &mut Session<StoreServices>, command: &str) -> PluginReply {
    match session.form_for(command) {
        Ok(spec) => PluginReply::Form(spec.map(form_from).map_err(failure_from)),
        Err(error) => PluginReply::Form(Err(host_reason(session, &error))),
    }
}

fn validated(
    session: &mut Session<StoreServices>,
    command: &str,
    values: &[FieldValue],
) -> PluginReply {
    match session.validate_form(command, &pairs(values)) {
        Ok(checked) => PluginReply::Validated(checked.map(errors_from).map_err(failure_from)),
        Err(error) => PluginReply::Validated(Err(host_reason(session, &error))),
    }
}

/// `forms.validate`, then `forms.submit` only if the plugin found nothing wrong, in one job so
/// the two see the same values. Only a stop once `submit` has begun leaves the write uncertain;
/// anything earlier failed with nothing sent.
fn submitted(
    session: &mut Session<StoreServices>,
    command: &str,
    values: &[FieldValue],
    reply: &mut Reply,
) -> PluginReply {
    let pairs = pairs(values);
    let failed = |failure| PluginReply::Submitted(Submitted::Finished(Outcome::Failed(failure)));
    let errors = match session.validate_form(command, &pairs) {
        Ok(Ok(errors)) => errors,
        Ok(Err(error)) => return failed(failure_from(error)),
        Err(error) => return failed(host_reason(session, &error)),
    };
    if !errors.is_empty() {
        return PluginReply::Submitted(Submitted::Invalid(errors_from(errors)));
    }
    reply.submitting = true;
    match session.submit(command, &pairs) {
        Ok((request, result)) => PluginReply::Submitted(submitted_from(request, result)),
        Err(HostError::SubmitStopped { .. }) => may_have_happened(),
        // Refused before the request was armed: nothing reached the plugin.
        Err(error) => failed(host_reason(session, &error)),
    }
}

fn may_have_happened() -> PluginReply {
    let outcome = Outcome::Uncertain(MAY_HAVE_HAPPENED.to_owned());
    PluginReply::Submitted(Submitted::Finished(outcome))
}

/// Why the host got no answer from the plugin, in the owner's words. A refusal from an
/// instance already discarded means the plugin stopped.
fn host_reason(session: &Session<StoreServices>, error: &HostError) -> Failure {
    let message = match error {
        HostError::Trap { .. } | HostError::Discarded | HostError::SubmitStopped { .. } => STOPPED,
        HostError::Refused(_) | HostError::Unavailable(_) if session.is_discarded() => STOPPED,
        HostError::Refused(_) | HostError::Unavailable(_) | HostError::NotGranted(_) => REFUSED,
        HostError::Malformed(_)
        | HostError::Precompiled
        | HostError::Identity(_)
        | HostError::Incompatible(_)
        | HostError::Instantiation(_) => CANNOT_START,
    };
    host_failure(message)
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
