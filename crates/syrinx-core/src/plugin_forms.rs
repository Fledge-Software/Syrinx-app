//! The palette's plugin commands and forms as Rust owns them (#29): the command being run, a
//! form's fields and values, the host's own checks and one submission at a time. It draws
//! nothing; the palette reads it, and the session carries its calls to the plugin host.

mod checks;
#[cfg(test)]
mod tests;

pub use checks::host_checks;

/// A form as its plugin described it (`forms.form-spec`), after the host checked its bounds.
#[derive(Clone, Debug, Default, PartialEq)]
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
pub enum PluginCall {
    /// Run a command declared without a form, or fetch the form of one declared with one; the
    /// host decides from the manifest and never calls `commands.run` for a form command.
    Choose {
        token: u64,
        plugin: String,
        command: String,
    },
    /// `forms.validate` as fields change; no side effects.
    Validate {
        token: u64,
        plugin: String,
        command: String,
        values: Vec<FieldValue>,
    },
    /// `forms.validate`, then `forms.submit` if the plugin found nothing wrong.
    Submit {
        token: u64,
        plugin: String,
        command: String,
        values: Vec<FieldValue>,
    },
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
    /// The outcome arrives later, as a `LateOutcome` for this request.
    Pending {
        request: u64,
    },
    /// Nothing happened; the input stays and the first failing field is named.
    Invalid(Vec<FieldError>),
}

/// The outcome of a submission that answered pending, arriving after it.
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
    FormReady {
        title: String,
    },
    /// The first failing field, in the form's order.
    Invalid(FieldError),
    /// Submitted; the outcome is still to come.
    Pending,
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
#[derive(Clone, Debug, Default, PartialEq)]
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
    /// `Choose` was sent. If the palette closed since, a form that arrives is dropped, while a
    /// command that ran is still reported.
    Choosing { token: u64, closed: bool },
    /// The form is open; `validation` is the newest `Validate` still unanswered.
    Editing {
        form: PluginForm,
        validation: Option<u64>,
    },
    /// One submission is on its way to the plugin.
    Submitting { form: PluginForm, token: u64 },
    /// The plugin answered pending; its outcome arrives as a `LateOutcome` for `request`.
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

/// The palette's one plugin command at a time, and the tokens that tie answers to it.
#[derive(Debug)]
pub struct PluginCommands {
    next_token: u64,
    current: Option<Current>,
}

impl Default for PluginCommands {
    fn default() -> Self {
        Self {
            next_token: 1,
            current: None,
        }
    }
}

impl PluginCommands {
    /// The owner chose a plugin command. Whatever was current is abandoned, so its late answers
    /// are dropped.
    pub fn choose(&mut self, plugin: &str, command: &str) -> PluginCall {
        let token = self.token();
        self.current = Some(Current {
            plugin: plugin.to_owned(),
            command: command.to_owned(),
            stage: Stage::Choosing {
                token,
                closed: false,
            },
        });
        PluginCall::Choose {
            token,
            plugin: plugin.to_owned(),
            command: command.to_owned(),
        }
    }

    /// The palette closed: the open form and any submission are abandoned locally, so a late
    /// answer or outcome updates nothing. A command already sent to `commands.run` is still
    /// reported, since its result goes to the status line, not the palette.
    pub fn abandon(&mut self) {
        let Some(current) = &mut self.current else {
            return;
        };
        match &mut current.stage {
            Stage::Choosing { closed, .. } => *closed = true,
            _ => self.current = None,
        }
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
    /// cleared until it is checked again.
    pub fn set_value(&mut self, name: &str, value: &str) -> Result<(), Refusal> {
        let form = self.editing_form()?;
        let index = form.index(name).ok_or(Refusal::UnknownField)?;
        form.values[index] = value.to_owned();
        form.errors.retain(|error| error.name != name);
        Ok(())
    }

    /// The host's checks, then `forms.validate` as fields change. A failed host check is
    /// reported here and nothing is sent.
    pub fn validate(&mut self) -> Result<PluginCall, Refusal> {
        let token = self.token();
        let current = self.current.as_mut().ok_or(Refusal::NoForm)?;
        let Stage::Editing { form, validation } = &mut current.stage else {
            return Err(refusal(&current.stage));
        };
        let values = checked_values(form)?;
        *validation = Some(token);
        Ok(PluginCall::Validate {
            token,
            plugin: current.plugin.clone(),
            command: current.command.clone(),
            values,
        })
    }

    /// Enter: the host's checks, then one submission. While one is pending, or after one that
    /// may have happened, Enter does nothing.
    pub fn submit(&mut self) -> Result<PluginCall, Refusal> {
        let token = self.token();
        let current = self.current.as_mut().ok_or(Refusal::NoForm)?;
        let Stage::Editing { form, .. } = &mut current.stage else {
            return Err(refusal(&current.stage));
        };
        let values = checked_values(form)?;
        let form = std::mem::take(form);
        current.stage = Stage::Submitting { form, token };
        Ok(PluginCall::Submit {
            token,
            plugin: current.plugin.clone(),
            command: current.command.clone(),
            values,
        })
    }

    /// An answer from the session. One for another command, a superseded call or an abandoned
    /// form is dropped and returns `None`.
    pub fn receive(&mut self, answer: PluginAnswer) -> Option<Notice> {
        let current = self.current.take()?;
        let (next, notice) = answered(current, answer);
        self.current = next;
        notice
    }

    /// An outcome that arrived after its submission answered pending. It counts only for the
    /// current submission's plugin and request; any other is dropped.
    pub fn receive_outcome(&mut self, late: LateOutcome) -> Option<Notice> {
        let current = self.current.take()?;
        match current.stage {
            Stage::Pending { form, request }
                if request == late.request && current.plugin == late.plugin =>
            {
                let (stage, notice) = settled(form, late.outcome);
                self.current = stage.map(|stage| Current { stage, ..current });
                Some(notice)
            }
            stage => {
                self.current = Some(Current { stage, ..current });
                None
            }
        }
    }

    fn editing_form(&mut self) -> Result<&mut PluginForm, Refusal> {
        let current = self.current.as_mut().ok_or(Refusal::NoForm)?;
        match &mut current.stage {
            Stage::Editing { form, .. } => Ok(form),
            stage => Err(refusal(stage)),
        }
    }

    fn token(&mut self) -> u64 {
        let token = self.next_token;
        self.next_token = token.checked_add(1).expect("plugin call tokens exhausted");
        token
    }
}

/// Why a stage other than editing refuses input or a submission.
fn refusal(stage: &Stage) -> Refusal {
    match stage {
        Stage::Choosing { .. } => Refusal::NoForm,
        Stage::Submitting { .. } | Stage::Pending { .. } => Refusal::Busy,
        Stage::Uncertain { .. } => Refusal::Uncertain,
        Stage::Editing { .. } => unreachable!("an editable form is never refused"),
    }
}

/// The form's values once the host's checks pass; otherwise the errors are kept and the first
/// is returned.
fn checked_values(form: &mut PluginForm) -> Result<Vec<FieldValue>, Refusal> {
    let values = form.field_values();
    let errors = host_checks(&form.spec, &values);
    match form.report(errors) {
        Some(first) => Err(Refusal::Invalid(first)),
        None => Ok(values),
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
        (Stage::Choosing { token, closed }, reply) if token == answer.token => {
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

/// The answer to `Choose`: a command that ran, or its form. A form for a closed palette, or a
/// reply that does not answer `Choose`, is dropped.
fn chosen(closed: bool, reply: PluginReply) -> (Option<Stage>, Option<Notice>) {
    match reply {
        PluginReply::Ran(Ok(())) => (None, Some(Notice::Outcome(Outcome::Done(None)))),
        PluginReply::Ran(Err(failure)) => (None, Some(Notice::Outcome(Outcome::Failed(failure)))),
        PluginReply::Form(_) if closed => (None, None),
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
            let first = form.report(errors);
            let notice = match first {
                Some(first) => Notice::Invalid(first),
                // Invalid with no field the form has: nothing happened, and nothing to point at.
                None => Notice::Failed(Failure {
                    kind: FailureKind::InvalidInput,
                    message: "The plugin refused the form without naming a field.".to_owned(),
                }),
            };
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
