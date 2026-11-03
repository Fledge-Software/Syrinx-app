//! Why the host refused or stopped a plugin.

use std::fmt;

/// A failure on the host's side of the boundary. A plugin's own typed
/// failures stay `plugin-error` values and never become a `HostError`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostError {
    /// The bytes are not a component wasmtime can compile.
    Malformed(String),
    /// Native code compiled elsewhere; Syrinx only ever runs code it compiled itself.
    Precompiled,
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
}

impl fmt::Display for HostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(detail) => write!(formatter, "not a loadable component: {detail}"),
            Self::Precompiled => write!(formatter, "the module is precompiled native code"),
            Self::Incompatible(reasons) => {
                write!(formatter, "incompatible component: {}", reasons.join("; "))
            }
            Self::Instantiation(detail) => write!(formatter, "instantiation failed: {detail}"),
            Self::Trap { message, .. } => write!(formatter, "the plugin trapped: {message}"),
            Self::Discarded => write!(formatter, "the plugin trapped earlier and was discarded"),
            Self::Refused(detail) => write!(formatter, "refused: {detail}"),
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
