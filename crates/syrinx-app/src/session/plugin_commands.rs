//! Carries the palette's plugin commands and forms to the plugin host (#29). Each call's job
//! sends its own answer into the session's event stream from the plugin's thread, and writes
//! the host settles itself arrive through the runtime's event watch, so nothing waits or polls.

use std::sync::{Arc, Mutex, PoisonError};

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
/// Shown inside "This may have happened (...)": the plugin stopped once `commands.run` began.
const RUN_MAY_HAVE_HAPPENED: &str = "the plugin stopped while running it";

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
    let asked = Asked::of(kind, has_form == Some(true));
    let failed = asked.failure();
    let reply = Reply::new(&asked, token, events.clone());
    let (Some(runtime), Some(_)) = (runtime, has_form) else {
        return reply.send(failed(host_failure(NOT_AVAILABLE)));
    };
    // The job and this thread share the reply: the job takes it when it runs, and if the job
    // is dropped unrun the last holder's drop says so.
    let reply = Arc::new(Mutex::new(Some(reply)));
    let held = Arc::clone(&reply);
    let job = move |session: &mut Session<StoreServices>| {
        if let Some(mut reply) = take(&held) {
            reply.progress = Progress::Running;
            let answered = answer(session, &command, asked, &mut reply);
            reply.send(answered);
        }
        Ok(())
    };
    match runtime.call(&plugin, job) {
        // The answer travels through `events`, so the `Pending` is not kept.
        Ok(pending) => drop(pending),
        // Refused before it was queued: the host's reason is the answer.
        Err(refused) => {
            if let Some(reply) = take(&reply) {
                reply.send(failed(host_reason(&refused, false)));
            }
        }
    }
}

/// The reply, unless a job or this thread already took it.
fn take(reply: &Mutex<Option<Reply>>) -> Option<Reply> {
    reply.lock().unwrap_or_else(PoisonError::into_inner).take()
}

/// Let the window hear of each write the host settles itself (`uncertain` once its plugin
/// stops) as it is settled, and then that the plugin stopped. A job sends its answer from the
/// plugin's thread before that thread settles the job's writes, so an answer always reaches the
/// window ahead of its own outcome.
pub(super) fn watch_outcomes(runtime: &OwnerRuntime, events: &UnboundedSender<SessionEvent>) {
    let events = events.clone();
    runtime.set_event_watch(move |event| {
        let told = match event {
            PluginEvent::Outcome {
                plugin,
                request,
                outcome,
            } => SessionEvent::PluginOutcome(LateOutcome {
                plugin: plugin.clone(),
                request: *request,
                outcome: outcome_from(outcome.clone()),
            }),
            PluginEvent::Failed { plugin, .. } | PluginEvent::Deactivated { plugin, .. } => {
                SessionEvent::PluginStopped {
                    plugin: plugin.clone(),
                }
            }
            PluginEvent::Cache { .. } | PluginEvent::PrepareFailed { .. } => return,
        };
        tell(&events, told);
    });
}

/// The export a call reaches, with the values it carries.
enum Asked {
    Run,
    Form,
    Validate(Vec<FieldValue>),
    Submit(Vec<FieldValue>),
}

/// How a call says it failed without effect.
type Failed = fn(Failure) -> PluginReply;

/// How a call says it stopped once its export may have begun to write.
type Uncertain = fn() -> PluginReply;

impl Asked {
    fn of(kind: CallKind, has_form: bool) -> Self {
        match kind {
            CallKind::Choose if has_form => Self::Form,
            CallKind::Choose => Self::Run,
            CallKind::Validate(values) => Self::Validate(values),
            CallKind::Submit(values) => Self::Submit(values),
        }
    }

    /// This call's reply when it failed without effect.
    fn failure(&self) -> Failed {
        match self {
            Self::Run => |failure| PluginReply::Ran(Outcome::Failed(failure)),
            Self::Form => |failure| PluginReply::Form(Err(failure)),
            Self::Validate(_) => |failure| PluginReply::Validated(Err(failure)),
            Self::Submit(_) => {
                |failure| PluginReply::Submitted(Submitted::Finished(Outcome::Failed(failure)))
            }
        }
    }

    /// This call's reply when its plugin stopped once it may have begun to write; only
    /// `commands.run` and `forms.submit` write, so the others failed without effect.
    fn uncertain(&self) -> Uncertain {
        match self {
            Self::Run => run_may_have_happened,
            Self::Submit(_) => may_have_happened,
            Self::Form => || PluginReply::Form(Err(host_failure(STOPPED))),
            Self::Validate(_) => || PluginReply::Validated(Err(host_failure(STOPPED))),
        }
    }
}

/// One call's answer, sent once from whichever thread settles it. Dropped unsent, it says how
/// far the job got (`Progress`).
struct Reply {
    token: u64,
    failed: Failed,
    uncertain: Uncertain,
    events: UnboundedSender<SessionEvent>,
    progress: Progress,
    sent: bool,
}

/// How far a job got, which decides what its reply says if it is dropped unsent.
#[derive(Clone, Copy)]
enum Progress {
    /// Never ran, as when the worker stopped first: nothing was sent.
    Queued,
    /// Ran but never answered, which only unwinding can cause, before any write had begun.
    Running,
    /// `commands.run` or `forms.submit` had begun, so a write may have happened.
    Writing,
}

impl Reply {
    fn new(asked: &Asked, token: u64, events: UnboundedSender<SessionEvent>) -> Self {
        Self {
            token,
            failed: asked.failure(),
            uncertain: asked.uncertain(),
            events,
            progress: Progress::Queued,
            sent: false,
        }
    }

    fn send(mut self, reply: PluginReply) {
        self.emit(reply);
    }

    /// The one way a reply reaches the window, once.
    fn emit(&mut self, reply: PluginReply) {
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
        // `Running` and `Writing` are reached only by unwinding out of the job.
        let reply = match self.progress {
            Progress::Queued => (self.failed)(host_failure(NOT_SENT)),
            Progress::Running => (self.failed)(host_failure(STOPPED)),
            Progress::Writing => (self.uncertain)(),
        };
        self.emit(reply);
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
    asked: Asked,
    reply: &mut Reply,
) -> PluginReply {
    match asked {
        Asked::Run => ran(session, command, reply),
        Asked::Form => form(session, command),
        Asked::Validate(values) => validated(session, command, &values),
        Asked::Submit(values) => submitted(session, command, &values, reply),
    }
}

/// `commands.run` for a command declared without a form. It can write, so a stop once it
/// began leaves its effect unknown, as for a submission.
fn ran(session: &mut Session<StoreServices>, command: &str, reply: &mut Reply) -> PluginReply {
    reply.progress = Progress::Writing;
    let outcome = match session.run_command(command) {
        Ok(Ok(())) => Outcome::Done(None),
        Ok(Err(error)) => Outcome::Failed(failure_from(error)),
        Err(HostError::Trap { .. }) => return run_may_have_happened(),
        Err(error) => Outcome::Failed(host_reason(&error, session.is_discarded())),
    };
    PluginReply::Ran(outcome)
}

/// `forms.form-for` for a command declared with a form.
fn form(session: &mut Session<StoreServices>, command: &str) -> PluginReply {
    match session.form_for(command) {
        Ok(spec) => PluginReply::Form(spec.map(form_from).map_err(failure_from)),
        Err(error) => PluginReply::Form(Err(host_reason(&error, session.is_discarded()))),
    }
}

/// `forms.validate`, as fields change.
fn validated(
    session: &mut Session<StoreServices>,
    command: &str,
    values: &[FieldValue],
) -> PluginReply {
    match session.validate_form(command, &pairs(values)) {
        Ok(checked) => PluginReply::Validated(checked.map(errors_from).map_err(failure_from)),
        Err(error) => PluginReply::Validated(Err(host_reason(&error, session.is_discarded()))),
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
    let failed = reply.failed;
    let errors = match session.validate_form(command, &pairs) {
        Ok(Ok(errors)) => errors,
        Ok(Err(error)) => return failed(failure_from(error)),
        Err(error) => return failed(host_reason(&error, session.is_discarded())),
    };
    if !errors.is_empty() {
        return PluginReply::Submitted(Submitted::Invalid(errors_from(errors)));
    }
    reply.progress = Progress::Writing;
    match session.submit(command, &pairs) {
        Ok((request, result)) => PluginReply::Submitted(submitted_from(request, result)),
        Err(HostError::SubmitStopped { .. }) => may_have_happened(),
        // Refused before the request was armed: nothing reached the plugin.
        Err(error) => failed(host_reason(&error, session.is_discarded())),
    }
}

fn may_have_happened() -> PluginReply {
    let outcome = Outcome::Uncertain(MAY_HAVE_HAPPENED.to_owned());
    PluginReply::Submitted(Submitted::Finished(outcome))
}

fn run_may_have_happened() -> PluginReply {
    PluginReply::Ran(Outcome::Uncertain(RUN_MAY_HAVE_HAPPENED.to_owned()))
}

/// Why the host got no answer from the plugin, in the owner's words. A refusal from an
/// instance already `discarded` means the plugin stopped.
fn host_reason(error: &HostError, discarded: bool) -> Failure {
    let message = match error {
        HostError::Trap { .. } | HostError::Discarded | HostError::SubmitStopped { .. } => STOPPED,
        HostError::Refused(_) | HostError::Unavailable(_) if discarded => STOPPED,
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

#[cfg(test)]
mod tests {
    use futures::channel::mpsc::unbounded;

    use super::*;

    /// What a reply for `asked`, dropped unsent at `progress`, tells the window.
    fn dropped_at(asked: &Asked, progress: Progress) -> PluginReply {
        let (events, mut heard) = unbounded();
        let mut reply = Reply::new(asked, 7, events);
        reply.progress = progress;
        drop(reply);
        let Ok(SessionEvent::Plugin(answer)) = heard.try_recv() else {
            panic!("a dropped reply still answers");
        };
        assert_eq!(answer.token, 7);
        assert!(heard.try_recv().is_err(), "it answers once");
        answer.reply
    }

    #[test]
    fn a_reply_dropped_unsent_says_how_far_its_job_got() {
        let submit = Asked::Submit(Vec::new());
        let failed = |message: &str| {
            let failure = host_failure(message);
            PluginReply::Submitted(Submitted::Finished(Outcome::Failed(failure)))
        };
        assert_eq!(dropped_at(&submit, Progress::Queued), failed(NOT_SENT));
        assert_eq!(dropped_at(&submit, Progress::Running), failed(STOPPED));
        assert_eq!(dropped_at(&submit, Progress::Writing), may_have_happened());
        let run = dropped_at(&Asked::Run, Progress::Writing);
        assert_eq!(run, run_may_have_happened());
    }

    #[test]
    fn each_host_error_is_told_as_stopped_refused_or_could_not_start() {
        let reason = |error: HostError, discarded| host_reason(&error, discarded).message;
        let trap = HostError::Trap {
            code: None,
            message: "synthetic".to_owned(),
        };
        assert_eq!(reason(trap, false), STOPPED);
        assert_eq!(reason(HostError::Discarded, false), STOPPED);
        let refused = || HostError::Refused("synthetic".to_owned());
        assert_eq!(reason(refused(), false), REFUSED);
        assert_eq!(reason(refused(), true), STOPPED);
        let full = HostError::Unavailable("synthetic".to_owned());
        assert_eq!(reason(full, false), REFUSED);
        assert_eq!(reason(HostError::Precompiled, false), CANNOT_START);
    }
}
