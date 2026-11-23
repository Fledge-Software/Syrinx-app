//! The palette's plugin commands and forms as Rust owns them (#29): the command being run, a
//! form's fields and values, the host's own checks and one submission at a time. It draws
//! nothing; the palette reads it, and the session carries its calls to the plugin host.

mod checks;
#[cfg(test)]
mod tests;

pub use checks::host_checks;

/// A form as its plugin described it (`forms.form-spec`), after the host checked its bounds.
#[derive(Clone, Debug, PartialEq)]
pub struct FormSpec {
    pub title: String,
    pub fields: Vec<Field>,
    pub submit_label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub name: String,
    pub label: String,
    pub kind: FieldKind,
    pub required: bool,
    pub initial: Option<String>,
}

/// `forms.field-kind`: what a field holds, which decides the host's checks.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldKind {
    Text(TextOptions),
    Multiline(TextOptions),
    Number(NumberOptions),
    /// An amount in the named currency.
    Money(String),
    Choice(Vec<ChoiceOption>),
    Toggle,
    Date,
    DateTime,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextOptions {
    pub max_length: u32,
    pub placeholder: Option<String>,
    pub format: TextFormat,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextFormat {
    Plain,
    Email,
    WebAddress,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NumberOptions {
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChoiceOption {
    pub value: String,
    pub label: String,
}

/// One field's value as it crosses to the plugin; every kind travels as text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FieldValue {
    pub name: String,
    pub value: String,
}

/// A field that failed a check, the host's or the plugin's.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FieldError {
    pub name: String,
    pub message: String,
}

/// Why a plugin, or the host on its behalf, refused or failed. The message is bounded by the
/// host and shown to the owner; it is never logged.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Failure {
    pub kind: FailureKind,
    pub message: String,
}

/// `types.plugin-error`'s cases, and `Host` for the host's own reasons (the plugin is not
/// loaded, or stopped before answering).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureKind {
    InvalidInput,
    NotFound,
    Denied,
    Revoked,
    Unavailable,
    Internal,
    Cancelled,
    Host,
}

/// `types.outcome`: what a command or a submission came to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Outcome {
    Done(Option<String>),
    Failed(Failure),
    /// It reached the service and may have happened; it is never resubmitted.
    Uncertain(String),
}

/// A call the palette asks the session to make. `token` names it, so its answer is matched to
/// the command and form that asked and to nothing else.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginCall {
    pub token: u64,
    pub plugin: String,
    pub command: String,
    pub kind: CallKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CallKind {
    /// Run a command declared without a form, or fetch the form of one declared with one; the
    /// host decides from the manifest and never calls `commands.run` for a form command.
    Choose,
    /// `forms.validate` as fields change; no side effects.
    Validate(Vec<FieldValue>),
    /// `forms.validate`, then `forms.submit` if the plugin found nothing wrong.
    Submit(Vec<FieldValue>),
}

/// The session's answer to the call `token` named.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginAnswer {
    pub token: u64,
    pub reply: PluginReply,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PluginReply {
    Ran(Result<(), Failure>),
    Form(Result<FormSpec, Failure>),
    Validated(Result<Vec<FieldError>, Failure>),
    Submitted(Submitted),
}

/// `forms.submit-result`, with the host's request id for a pending answer.
#[derive(Clone, Debug, PartialEq)]
pub enum Submitted {
    Finished(Outcome),
    /// The plugin will learn the outcome through `events.deliver`. The palette hears of it only
    /// if the host settles the write itself (`uncertain` once the plugin stops), as a
    /// `LateOutcome` for this request: no service completes a pending write yet.
    Pending {
        request: u64,
    },
    /// Nothing happened; the input stays and the first failing field is named.
    Invalid(Vec<FieldError>),
}

/// The outcome of a submission that answered pending, arriving after it. Today only the host
/// settles such a write, when its plugin stops.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LateOutcome {
    pub plugin: String,
    pub request: u64,
    pub outcome: Outcome,
}

/// What the owner is told, through the palette's status line until forms are drawn.
#[derive(Clone, Debug, PartialEq)]
pub enum Notice {
    /// A command ran, or a submission came to an outcome.
    Outcome(Outcome),
    /// The plugin could not answer a call that has no effect; a form keeps its input.
    Failed(Failure),
    /// The form is open in the palette.
    FormReady { title: String },
    /// The command has a form, but the palette had closed by the time it arrived.
    FormNotShown { title: String },
    /// The first failing field, in the form's order.
    Invalid(FieldError),
    /// Submitted; an outcome comes only if the host settles the write (see `Submitted`).
    Pending,
    /// A set-aside call was forgotten to make room for a newer one, so its result will not be
    /// told. A submission's may have happened; a command's result can no longer be shown. A
    /// choice may have been a `commands.run`, which can write, but the model learns whether it
    /// was a run or a form only from its answer, so it does not claim either.
    Forgotten { may_have_written: bool },
    /// A notice about a call set aside earlier, named so it is not read as the current one's.
    /// The model never nests one `SetAside` inside another.
    SetAside {
        command: String,
        notice: Box<Notice>,
    },
}

/// Why the palette's request to validate or submit was not sent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// No form is open.
    NoForm,
    /// A submission is pending; a repeated Enter does nothing.
    Busy,
    /// The last submission may have happened; it is never sent again from this form.
    Uncertain,
    /// The host's checks failed; the first failing field.
    Invalid(FieldError),
    /// The form has no field of that name.
    UnknownField,
}

/// A form's fields with the owner's values and the errors last reported.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginForm {
    spec: FormSpec,
    /// One value per field, in the form's order.
    values: Vec<String>,
    errors: Vec<FieldError>,
}

impl PluginForm {
    fn new(spec: FormSpec) -> Self {
        let values = spec.fields.iter().map(initial_value).collect();
        Self {
            spec,
            values,
            errors: Vec::new(),
        }
    }

    pub fn spec(&self) -> &FormSpec {
        &self.spec
    }

    /// The value of the field named `name`.
    pub fn value(&self, name: &str) -> Option<&str> {
        let index = self.index(name)?;
        Some(&self.values[index])
    }

    /// The errors last reported, in the form's order.
    pub fn errors(&self) -> &[FieldError] {
        &self.errors
    }

    fn index(&self, name: &str) -> Option<usize> {
        self.spec.fields.iter().position(|field| field.name == name)
    }

    fn field_values(&self) -> Vec<FieldValue> {
        let named = self.spec.fields.iter().zip(&self.values);
        named
            .map(|(field, value)| FieldValue {
                name: field.name.clone(),
                value: value.clone(),
            })
            .collect()
    }

    /// Keep `errors`, in the form's order, and name the first; errors for fields the form does
    /// not have are dropped.
    fn report(&mut self, mut errors: Vec<FieldError>) -> Option<FieldError> {
        errors.retain(|error| self.index(&error.name).is_some());
        errors.sort_by_key(|error| self.index(&error.name));
        self.errors = errors;
        self.errors.first().cloned()
    }

    /// The form's values once the host's checks pass; otherwise the errors are kept and the
    /// first is returned.
    fn checked_values(&mut self) -> Result<Vec<FieldValue>, Refusal> {
        let values = self.field_values();
        let errors = host_checks(&self.spec, &values);
        match self.report(errors) {
            Some(first) => Err(Refusal::Invalid(first)),
            None => Ok(values),
        }
    }
}

/// A toggle starts off; any other field starts with its initial value, or empty.
fn initial_value(field: &Field) -> String {
    if let Some(initial) = &field.initial {
        return initial.clone();
    }
    match field.kind {
        FieldKind::Toggle => "false".to_owned(),
        _ => String::new(),
    }
}

/// Where the current command is.
#[derive(Debug)]
enum Stage {
    /// `Choose` was sent. If the palette was closed then or since, a form that arrives is
    /// reported on the status line instead of opened.
    Choosing {
        token: u64,
        closed: bool,
        /// The command's title as the palette showed it, to name it if it is set aside.
        title: String,
    },
    /// The form is open; `validation` is the newest `Validate` still unanswered.
    Editing {
        form: PluginForm,
        validation: Option<u64>,
    },
    /// One submission is on its way to the plugin.
    Submitting { form: PluginForm, token: u64 },
    /// The plugin answered pending; see `Submitted::Pending` for when it resolves.
    Pending { form: PluginForm, request: u64 },
    /// The submission may have happened; the form stays but never submits again.
    Uncertain { form: PluginForm },
}

#[derive(Debug)]
struct Current {
    plugin: String,
    command: String,
    stage: Stage,
}

/// A call set aside while it was on its way or pending, because the palette closed or another
/// command was chosen. A command run or a submission still reports its own result, named,
/// since it may have written. A form arriving for a set-aside choice, or its failure, is
/// dropped without a word: it had no effect and nobody can see it (a product choice).
#[derive(Debug)]
struct Orphan {
    plugin: String,
    /// The form's title for a submission; the command's title, as the palette showed it, for a
    /// choice.
    name: String,
    /// Whether it is a submission, which may write; a choice's kind is not known yet.
    submission: bool,
    awaiting: Awaiting,
}

#[derive(Debug, Eq, PartialEq)]
enum Awaiting {
    /// The answer to the call with this token: a command run, or a submission.
    Answer(u64),
    /// The outcome of this request, which answered pending.
    Outcome(u64),
}

/// What a pending submission is told when its plugin stops before the host settles it.
const STOPPED_PENDING: &str = "the plugin stopped before its outcome was known";

/// The most set-aside calls kept for their result; the oldest is forgotten first, and the
/// owner is told (`Notice::Forgotten`).
const MAX_ORPHANS: usize = 16;

/// The palette's one plugin command at a time, and the tokens that tie answers to it.
#[derive(Debug)]
pub struct PluginCommands {
    next_token: u64,
    current: Option<Current>,
    orphans: Vec<Orphan>,
}

impl Default for PluginCommands {
    fn default() -> Self {
        Self {
            next_token: 1,
            current: None,
            orphans: Vec::new(),
        }
    }
}

impl PluginCommands {
    /// The owner chose a plugin command, shown to them as `title`; `in_palette` says whether the
    /// palette is still open to show a form. Whatever was current is set aside or abandoned, as
    /// closing the palette does; the notice is for a set-aside call forgotten to make room.
    pub fn choose(
        &mut self,
        plugin: &str,
        command: &str,
        title: &str,
        in_palette: bool,
    ) -> (PluginCall, Option<Notice>) {
        let forgotten = self.set_aside_choosing();
        let abandoned = self.abandon();
        let call = self.call(plugin.to_owned(), command.to_owned(), CallKind::Choose);
        let stage = Stage::Choosing {
            token: call.token,
            closed: !in_palette,
            title: title.to_owned(),
        };
        self.current = Some(Current {
            plugin: plugin.to_owned(),
            command: command.to_owned(),
            stage,
        });
        (call, forgotten.or(abandoned))
    }

    /// The palette closed: the open form is dropped, so a late answer cannot update it. A
    /// command already sent to `commands.run` and a submission on its way or pending are still
    /// reported, since their results go to the status line and a write may have happened. The
    /// notice is for a set-aside call forgotten to make room.
    pub fn abandon(&mut self) -> Option<Notice> {
        let current = self.current.take()?;
        let (form, awaiting) = match current.stage {
            Stage::Choosing { token, title, .. } => {
                let closed = true;
                let stage = Stage::Choosing {
                    token,
                    closed,
                    title,
                };
                self.current = Some(Current { stage, ..current });
                return None;
            }
            Stage::Submitting { form, token } => (form, Awaiting::Answer(token)),
            Stage::Pending { form, request } => (form, Awaiting::Outcome(request)),
            Stage::Editing { .. } | Stage::Uncertain { .. } => return None,
        };
        self.keep_orphan(Orphan {
            plugin: current.plugin,
            name: form.spec.title,
            submission: true,
            awaiting,
        })
    }

    /// A plugin's worker stopped. It settled its writes in flight before saying so, so a
    /// submission still waiting for an outcome from it, set aside or current, will never get
    /// one. A current one is told it may have happened, since its write may have completed.
    pub fn plugin_stopped(&mut self, plugin: &str) -> Option<Notice> {
        self.orphans.retain(|orphan| {
            orphan.plugin != plugin || !matches!(orphan.awaiting, Awaiting::Outcome(_))
        });
        let outcome = Outcome::Uncertain(STOPPED_PENDING.to_owned());
        self.settle_current_pending(|stopped, _| stopped == plugin, outcome)
    }

    /// The open form, while one is.
    pub fn form(&self) -> Option<&PluginForm> {
        match &self.current.as_ref()?.stage {
            Stage::Choosing { .. } => None,
            Stage::Editing { form, .. }
            | Stage::Submitting { form, .. }
            | Stage::Pending { form, .. }
            | Stage::Uncertain { form } => Some(form),
        }
    }

    /// Whether a submission is on its way or pending, so Enter does nothing.
    pub fn is_submitting(&self) -> bool {
        let stage = self.current.as_ref().map(|current| &current.stage);
        matches!(
            stage,
            Some(Stage::Submitting { .. } | Stage::Pending { .. })
        )
    }

    /// The owner changed a field. Only an editable form takes input; the field's last error is
    /// cleared, and a validation still on its way no longer counts, since it checked older input.
    pub fn set_value(&mut self, name: &str, value: &str) -> Result<(), Refusal> {
        let (plugin, command, mut form) = self.take_editing()?;
        let changed = match form.index(name) {
            Some(index) => {
                form.values[index] = value.to_owned();
                form.errors.retain(|error| error.name != name);
                Ok(())
            }
            None => Err(Refusal::UnknownField),
        };
        self.put_editing(plugin, command, form, None);
        changed
    }

    /// The host's checks, then `forms.validate` as fields change. A failed host check is
    /// reported here and nothing is sent.
    pub fn validate(&mut self) -> Result<PluginCall, Refusal> {
        let (plugin, command, form, values) = self.checked_form()?;
        let call = self.call(plugin.clone(), command.clone(), CallKind::Validate(values));
        self.put_editing(plugin, command, form, Some(call.token));
        Ok(call)
    }

    /// Enter: the host's checks, then one submission. While one is on its way or pending, or
    /// after one that may have happened, Enter does nothing.
    pub fn submit(&mut self) -> Result<PluginCall, Refusal> {
        let (plugin, command, form, values) = self.checked_form()?;
        let call = self.call(plugin.clone(), command.clone(), CallKind::Submit(values));
        let token = call.token;
        let stage = Stage::Submitting { form, token };
        self.current = Some(Current {
            plugin,
            command,
            stage,
        });
        Ok(call)
    }

    /// An answer from the session. One for another command, a superseded call or a closed form
    /// is dropped and returns `None`; a closed submission's own outcome is still reported.
    pub fn receive(&mut self, answer: PluginAnswer) -> Option<Notice> {
        if let Some(index) = self.orphan_with_token(answer.token) {
            return self.orphan_answered(index, answer.reply);
        }
        let current = self.current.take()?;
        let (next, notice) = answered(current, answer);
        self.current = next;
        notice
    }

    /// An outcome that arrived after its submission answered pending. It counts only for the
    /// current or a closed submission with the same plugin and request; any other is dropped.
    pub fn receive_outcome(&mut self, late: LateOutcome) -> Option<Notice> {
        let awaited = Awaiting::Outcome(late.request);
        let orphan = self
            .orphans
            .iter()
            .position(|orphan| orphan.plugin == late.plugin && orphan.awaiting == awaited);
        if let Some(index) = orphan {
            let orphan = self.orphans.remove(index);
            return Some(set_aside(orphan.name, Notice::Outcome(late.outcome)));
        }
        let awaited = |plugin: &str, request| plugin == late.plugin && request == late.request;
        self.settle_current_pending(awaited, late.outcome)
    }

    /// Settle the current pending submission with `outcome` when `awaited(plugin, request)`
    /// says it is the one; otherwise nothing changes.
    fn settle_current_pending(
        &mut self,
        awaited: impl Fn(&str, u64) -> bool,
        outcome: Outcome,
    ) -> Option<Notice> {
        let current = self.current.take()?;
        match current.stage {
            Stage::Pending { form, request } if awaited(&current.plugin, request) => {
                let (stage, notice) = settled(form, outcome);
                self.current = stage.map(|stage| Current { stage, ..current });
                Some(notice)
            }
            stage => {
                self.current = Some(Current { stage, ..current });
                None
            }
        }
    }

    /// The editable form and its values once the host's checks pass; a form that fails them
    /// is put back with its errors, and nothing is sent.
    fn checked_form(&mut self) -> Result<(String, String, PluginForm, Vec<FieldValue>), Refusal> {
        let (plugin, command, mut form) = self.take_editing()?;
        match form.checked_values() {
            Ok(values) => Ok((plugin, command, form, values)),
            Err(refusal) => {
                self.put_editing(plugin, command, form, None);
                Err(refusal)
            }
        }
    }

    /// A call under a fresh token.
    fn call(&mut self, plugin: String, command: String, kind: CallKind) -> PluginCall {
        PluginCall {
            token: self.token(),
            plugin,
            command,
            kind,
        }
    }

    /// A choice still waiting for its answer, set aside so a command it ran is still told.
    fn set_aside_choosing(&mut self) -> Option<Notice> {
        let current = self.current.take()?;
        let Stage::Choosing { token, title, .. } = current.stage else {
            self.current = Some(current);
            return None;
        };
        self.keep_orphan(Orphan {
            plugin: current.plugin,
            name: title,
            submission: false,
            awaiting: Awaiting::Answer(token),
        })
    }

    /// The editable form, taken out to be changed; anything else is put back and refused.
    fn take_editing(&mut self) -> Result<(String, String, PluginForm), Refusal> {
        let Current {
            plugin,
            command,
            stage,
        } = self.current.take().ok_or(Refusal::NoForm)?;
        let refusal = match stage {
            Stage::Editing { form, .. } => return Ok((plugin, command, form)),
            Stage::Choosing { .. } => Refusal::NoForm,
            Stage::Submitting { .. } | Stage::Pending { .. } => Refusal::Busy,
            Stage::Uncertain { .. } => Refusal::Uncertain,
        };
        self.current = Some(Current {
            plugin,
            command,
            stage,
        });
        Err(refusal)
    }

    fn put_editing(
        &mut self,
        plugin: String,
        command: String,
        form: PluginForm,
        validation: Option<u64>,
    ) {
        let stage = Stage::Editing { form, validation };
        self.current = Some(Current {
            plugin,
            command,
            stage,
        });
    }

    /// Keep `orphan`; past the limit the oldest is forgotten, and the owner is told.
    fn keep_orphan(&mut self, orphan: Orphan) -> Option<Notice> {
        let forgotten = (self.orphans.len() == MAX_ORPHANS).then(|| {
            let oldest = self.orphans.remove(0);
            let notice = Notice::Forgotten {
                may_have_written: oldest.submission,
            };
            set_aside(oldest.name, notice)
        });
        self.orphans.push(orphan);
        forgotten
    }

    /// A set-aside call still waiting for the answer to `token`.
    fn orphan_with_token(&self, token: u64) -> Option<usize> {
        let awaited = Awaiting::Answer(token);
        self.orphans
            .iter()
            .position(|orphan| orphan.awaiting == awaited)
    }

    /// A set-aside call's answer: a command run or a submission's result is reported, named,
    /// a pending submission waits on, and a form (or its failure) nobody can see is dropped.
    fn orphan_answered(&mut self, index: usize, reply: PluginReply) -> Option<Notice> {
        if let PluginReply::Submitted(Submitted::Pending { request }) = reply {
            self.orphans[index].awaiting = Awaiting::Outcome(request);
            return None;
        }
        let orphan = self.orphans.remove(index);
        let notice = match reply {
            PluginReply::Ran(ran) => ran_notice(ran),
            PluginReply::Submitted(Submitted::Finished(outcome)) => Notice::Outcome(outcome),
            PluginReply::Submitted(Submitted::Invalid(errors)) => {
                invalid_notice(errors.into_iter().next())
            }
            PluginReply::Submitted(Submitted::Pending { .. })
            | PluginReply::Form(_)
            | PluginReply::Validated(_) => return None,
        };
        Some(set_aside(orphan.name, notice))
    }

    fn token(&mut self) -> u64 {
        let token = self.next_token;
        self.next_token = token.checked_add(1).expect("plugin call tokens exhausted");
        token
    }
}

/// The current command after `answer`, and what to tell the owner. An answer whose token is
/// not the one the current stage awaits changes nothing.
fn answered(current: Current, answer: PluginAnswer) -> (Option<Current>, Option<Notice>) {
    let Current {
        plugin,
        command,
        stage,
    } = current;
    let (stage, notice) = match (stage, answer.reply) {
        (Stage::Choosing { token, closed, .. }, reply) if token == answer.token => {
            chosen(closed, reply)
        }
        (
            Stage::Editing {
                mut form,
                validation,
            },
            PluginReply::Validated(result),
        ) if validation == Some(answer.token) => {
            let notice = validated(&mut form, result);
            let stage = Stage::Editing {
                form,
                validation: None,
            };
            (Some(stage), notice)
        }
        (Stage::Submitting { form, token }, PluginReply::Submitted(submitted))
            if token == answer.token =>
        {
            let (stage, notice) = submission(form, submitted);
            (stage, Some(notice))
        }
        (stage, _) => (Some(stage), None),
    };
    let current = stage.map(|stage| Current {
        plugin,
        command,
        stage,
    });
    (current, notice)
}

/// The answer to `Choose`: a command that ran, or its form, which opens only in an open
/// palette. A reply that does not answer `Choose` is dropped.
fn chosen(closed: bool, reply: PluginReply) -> (Option<Stage>, Option<Notice>) {
    match reply {
        PluginReply::Ran(ran) => (None, Some(ran_notice(ran))),
        PluginReply::Form(Ok(spec)) if closed => {
            let title = spec.title;
            (None, Some(Notice::FormNotShown { title }))
        }
        PluginReply::Form(Ok(spec)) => {
            let title = spec.title.clone();
            let form = PluginForm::new(spec);
            let stage = Stage::Editing {
                form,
                validation: None,
            };
            (Some(stage), Some(Notice::FormReady { title }))
        }
        PluginReply::Form(Err(failure)) => (None, Some(Notice::Failed(failure))),
        PluginReply::Validated(_) | PluginReply::Submitted(_) => (None, None),
    }
}

/// The plugin's validation: its errors are kept and the first named.
fn validated(form: &mut PluginForm, result: Result<Vec<FieldError>, Failure>) -> Option<Notice> {
    match result {
        Ok(errors) => form.report(errors).map(Notice::Invalid),
        Err(failure) => Some(Notice::Failed(failure)),
    }
}

/// What a submission's answer leaves: pending waits for its outcome, invalid keeps the input.
fn submission(form: PluginForm, submitted: Submitted) -> (Option<Stage>, Notice) {
    match submitted {
        Submitted::Finished(outcome) => settled(form, outcome),
        Submitted::Pending { request } => (Some(Stage::Pending { form, request }), Notice::Pending),
        Submitted::Invalid(errors) => {
            let mut form = form;
            let notice = invalid_notice(form.report(errors));
            let stage = Stage::Editing {
                form,
                validation: None,
            };
            (Some(stage), notice)
        }
    }
}

/// A submission's outcome: done ends the command, failed keeps the input for another try, and
/// uncertain keeps the form but never submits it again.
fn settled(form: PluginForm, outcome: Outcome) -> (Option<Stage>, Notice) {
    let stage = match &outcome {
        Outcome::Done(_) => None,
        Outcome::Failed(_) => Some(Stage::Editing {
            form,
            validation: None,
        }),
        Outcome::Uncertain(_) => Some(Stage::Uncertain { form }),
    };
    (stage, Notice::Outcome(outcome))
}

/// What the owner is told of a command that ran.
fn ran_notice(ran: Result<(), Failure>) -> Notice {
    match ran {
        Ok(()) => Notice::Outcome(Outcome::Done(None)),
        Err(failure) => Notice::Outcome(Outcome::Failed(failure)),
    }
}

/// `notice`, named for the set-aside call it is about.
fn set_aside(command: String, notice: Notice) -> Notice {
    Notice::SetAside {
        command,
        notice: Box::new(notice),
    }
}

/// What the owner is told of a submission the plugin found invalid: its first failing field, or,
/// when it named none, that nothing happened.
fn invalid_notice(first: Option<FieldError>) -> Notice {
    match first {
        Some(first) => Notice::Invalid(first),
        None => Notice::Failed(Failure {
            kind: FailureKind::InvalidInput,
            message: "The plugin refused the form without naming a field.".to_owned(),
        }),
    }
}
