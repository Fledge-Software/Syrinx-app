//! Why a manifest was refused. Each refusal is named, so the review sheet (#140) can say it.

use std::fmt;

use super::CapabilityKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    /// Larger than `MAX_MANIFEST_BYTES`.
    TooLarge,
    /// Not UTF-8 TOML, or a field missing, misspelt or of the wrong type.
    Syntax(String),
    /// A `schema` this host cannot read; checked before every other field.
    UnsupportedSchema(String),
    /// A `[interface] world` this host does not run.
    UnsupportedWorld(String),
    /// A field whose value breaks its rule.
    InvalidField { field: String, reason: String },
    /// A contributed id that does not start with `<plugin id>.`.
    MissingPrefix { id: String },
    /// The same id contributed twice.
    DuplicateContribution { id: String },
    /// More commands than one plugin may register.
    TooManyCommands { declared: usize, limit: usize },
    /// A `[[contributes.widgets]]` outside a developer-mode plugin with `home-widgets` on (#31).
    WidgetsNotAllowed,
    /// A capability kind this host does not know.
    UnknownCapability(String),
    /// A capability without its one-sentence reason.
    MissingReason(CapabilityKind),
    /// A full-trust kind without `trust = "full"`.
    FullTrustNotDeclared(CapabilityKind),
    /// `trust = "full"` on a kind the host runs sandboxed.
    TrustMismatch(CapabilityKind),
    /// The same kind requested twice.
    DuplicateCapability(CapabilityKind),
    /// A scope field the kind does not take, or one it needs that is missing or wrong.
    InvalidScope {
        kind: CapabilityKind,
        reason: String,
    },
}

impl fmt::Display for ManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => write!(
                formatter,
                "the manifest is larger than {} bytes",
                super::MAX_MANIFEST_BYTES
            ),
            Self::Syntax(detail) => write!(formatter, "the manifest cannot be read: {detail}"),
            Self::UnsupportedSchema(schema) => {
                write!(formatter, "manifest schema {schema} is not supported")
            }
            Self::UnsupportedWorld(world) => {
                write!(
                    formatter,
                    "this Syrinx does not run plugins built for {world}"
                )
            }
            Self::InvalidField { field, reason } => write!(formatter, "{field}: {reason}"),
            Self::MissingPrefix { id } => {
                write!(formatter, "{id} does not start with the plugin's id")
            }
            Self::DuplicateContribution { id } => write!(formatter, "{id} is declared twice"),
            Self::TooManyCommands { declared, limit } => write!(
                formatter,
                "the plugin declares {declared} commands; one plugin may register {limit}"
            ),
            Self::WidgetsNotAllowed => write!(
                formatter,
                "Home widgets are unstable and need a developer-mode plugin with home-widgets on"
            ),
            Self::UnknownCapability(kind) => write!(formatter, "unknown capability {kind:?}"),
            Self::MissingReason(kind) => {
                write!(formatter, "the {kind} capability gives no reason")
            }
            Self::FullTrustNotDeclared(kind) => write!(
                formatter,
                "the {kind} capability can run programs, so it must say trust = \"full\""
            ),
            Self::TrustMismatch(kind) => write!(
                formatter,
                "the {kind} capability is sandboxed; only full-trust kinds say trust = \"full\""
            ),
            Self::DuplicateCapability(kind) => {
                write!(formatter, "the {kind} capability is requested twice")
            }
            Self::InvalidScope { kind, reason } => write!(formatter, "{kind} capability: {reason}"),
        }
    }
}

impl std::error::Error for ManifestError {}
