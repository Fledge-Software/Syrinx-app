//! Why the host refused or stopped a plugin.

use std::fmt;

use crate::bindings::syrinx::plugin::types::PluginError;
use crate::deadline::Export;
use crate::identity::IdentityMismatch;

/// A failure on the host's side of the boundary. A plugin's own typed
/// failures stay `plugin-error` values and never become a `HostError`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostError {
    /// The bytes are not a component wasmtime can compile.
    Malformed(String),
    /// Native code compiled elsewhere; Syrinx only ever runs code it compiled itself.
    Precompiled,
    /// The component's interface names disagree with its registered manifest (#134).
    Identity(IdentityMismatch),
    /// The component targets a contract this host does not offer. Every reason is listed.
    Incompatible(Vec<String>),
    /// Linking or instantiation failed after the checks passed.
    Instantiation(String),
    /// The plugin trapped; its store is discarded.
    Trap {
        /// wasmtime's trap code, when the trap had one (an expired deadline is `Interrupt`).
        code: Option<wasmtime::Trap>,
        message: String,
    },
    /// The plugin trapped earlier, so the host refuses to call it again.
    Discarded,
    /// The host refused a request before it reached the plugin.
    Refused(String),
    /// The plugin cannot take the call now: its queue is full, or it stopped before answering.
    Unavailable(String),
    /// The plugin stopped while `forms.submit` ran for `request`, so the write's outcome is
    /// unknown; the worker reports it `uncertain` under the same id. `cause` is the trap
    /// (`Trap`) or the refusal of a discarded instance (`Discarded`), never another
    /// `SubmitStopped`.
    SubmitStopped { request: u64, cause: Box<HostError> },
}

impl fmt::Display for HostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(detail) => write!(formatter, "not a loadable component: {detail}"),
            Self::Precompiled => write!(formatter, "the module is precompiled native code"),
            Self::Identity(mismatch) => write!(formatter, "{mismatch}"),
            Self::Incompatible(reasons) => {
                write!(formatter, "incompatible component: {}", reasons.join("; "))
            }
            Self::Instantiation(detail) => write!(formatter, "instantiation failed: {detail}"),
            Self::Trap { message, .. } => write!(formatter, "the plugin trapped: {message}"),
            Self::Discarded => write!(formatter, "the plugin trapped earlier and was discarded"),
            Self::Refused(detail) => write!(formatter, "refused: {detail}"),
            Self::Unavailable(detail) => write!(formatter, "unavailable: {detail}"),
            Self::SubmitStopped { request, cause } => {
                write!(
                    formatter,
                    "the plugin stopped during submit {request}: {cause}"
                )
            }
        }
    }
}

impl std::error::Error for HostError {}

impl HostError {
    /// Turn an error from an export call into a trap, keeping wasmtime's code.
    pub(crate) fn from_trap(error: &wasmtime::Error) -> Self {
        Self::Trap {
            code: error.downcast_ref::<wasmtime::Trap>().copied(),
            message: format!("{error:#}"),
        }
    }
}

/// Why a plugin was stopped, as the owner is shown it. Only an activation refusal carries
/// text the plugin wrote; the host shows that and never logs it.
#[derive(Debug, Clone, PartialEq)]
pub enum Failure {
    /// An export ran past its deadline (`Trap::Interrupt`).
    DeadlineExpired(Export),
    /// The plugin trapped in an export: a panic, an out-of-bounds access, a malformed value.
    Trapped {
        export: Export,
        code: Option<wasmtime::Trap>,
    },
    /// The plugin kept queueing deferred calls after one export.
    Flooded,
    /// One call and the calls it deferred ran past the job budget (`limits::JOB_BUDGET`).
    OverBudget,
    /// The component could not be compiled, linked or instantiated.
    Load(HostError),
    /// `lifecycle.activate` answered with an error.
    ActivationRefused(PluginError),
}

impl Failure {
    /// The failure behind an instantiation error: a trap there is a trap in activation,
    /// anything else means the component could not be started.
    pub(crate) fn from_instantiation(error: HostError) -> Self {
        if matches!(error, HostError::Trap { .. }) {
            return Self::from_trap(Export::Activate, &error);
        }
        Self::Load(error)
    }

    /// The failure behind a trap from `export`; an expired deadline is named as such.
    pub(crate) fn from_trap(export: Export, stopped: &HostError) -> Self {
        let code = match stopped {
            HostError::Trap { code, .. } => *code,
            _ => None,
        };
        if code == Some(wasmtime::Trap::Interrupt) {
            return Self::DeadlineExpired(export);
        }
        Self::Trapped { export, code }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeadlineExpired(export) => {
                write!(formatter, "{} ran past its deadline", export.name())
            }
            Self::Trapped {
                export,
                code: Some(code),
            } => write!(formatter, "{} trapped: {code}", export.name()),
            Self::Trapped { export, code: None } => {
                write!(formatter, "{} trapped", export.name())
            }
            Self::Flooded => write!(formatter, "the plugin kept queueing deferred calls"),
            Self::OverBudget => write!(formatter, "one call and its deferred calls took too long"),
            Self::Load(error) => write!(formatter, "the plugin could not start: {error}"),
            Self::ActivationRefused(_) => write!(formatter, "the plugin refused to activate"),
        }
    }
}
