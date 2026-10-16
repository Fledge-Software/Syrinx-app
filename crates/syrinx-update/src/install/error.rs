//! Why staging, applying or recovering an update stopped; no variant carries a path.

use std::fmt;
use std::io;

use crate::error::UpdateError;
use crate::version::Version;

#[cfg(test)]
use super::fault::Step;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallError {
    /// The package, statement or signature was refused by the verifier.
    Update(UpdateError),
    /// A file operation failed; `step` names what Syrinx was doing.
    Io {
        step: &'static str,
        kind: io::ErrorKind,
    },
    /// The program and data folders are not absolute or overlap.
    Layout,
    /// This Syrinx does not run from an installed `Programs\Syrinx` folder.
    NotInstalled,
    /// The chosen folder lacks the signed files of an update package.
    NotAPackage,
    /// Another update step holds the update lock.
    Busy,
    /// Waited too long for `what`.
    TimedOut { what: &'static str },
    /// A folder or file Syrinx would replace is not one Syrinx made, or is a link.
    UnexpectedEntry { what: &'static str },
    /// A local update record is malformed.
    Record { what: &'static str },
    /// The package does not list a file every installation needs.
    MissingProgramFile { name: &'static str },
    /// The disks lack room for two copies of the package and a margin.
    NotEnoughSpace { needed: u64, available: u64 },
    /// The staged copy's recorded manifest is not the one that was verified.
    StagedManifestChanged,
    /// The staged copy is for a different version from the one recorded as pending.
    StagedVersionMismatch { pending: Version, manifest: Version },
    /// Nothing is staged.
    NothingStaged,
    /// The installed `Syrinx` folder could not be moved aside, usually because Syrinx runs.
    ProgramInUse,
    /// Setup moved the installed version to `Syrinx.previous` and could not put it back.
    SetAside(Box<InstallError>),
    /// A staged update is waiting, so its records are not reset under it (DW-66).
    UpdateWaiting,
    /// A step failed after the new version was moved into place; its own records cover it.
    AfterSwap(Box<InstallError>),
    /// Fetching a file from the update host failed (P1-39c).
    Transfer(TransferProblem),
    /// No release key is pinned, so nothing could be verified (DW-30).
    NoReleaseKeys,
    /// A step failed and cleaning up after it also failed.
    Incomplete {
        primary: Box<InstallError>,
        cleanup: Box<InstallError>,
    },
    /// A test's fault hook stopped the work here, as a crash would.
    #[cfg(test)]
    Interrupted(Step),
}

impl From<UpdateError> for InstallError {
    fn from(error: UpdateError) -> Self {
        Self::Update(error)
    }
}

/// Wraps an I/O failure with the step that hit it; a download's own failure keeps its category.
pub(crate) fn io_error(step: &'static str) -> impl Fn(io::Error) -> InstallError {
    move |error| match TransferProblem::carried_by(&error) {
        Some(problem) => InstallError::Transfer(problem),
        None => InstallError::Io {
            step,
            kind: error.kind(),
        },
    }
}

/// Why a download from the update host stopped, as fixed categories with no address or header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferProblem {
    /// The name did not resolve or the connection failed or dropped.
    Unreachable,
    /// The host's certificate or the TLS handshake was refused.
    Certificate,
    /// Connecting took over 30 s, or no bytes arrived for 60 s.
    TimedOut,
    /// The host answered with a redirect, which Syrinx never follows.
    Redirected,
    /// The host answered with a status other than 200.
    Status(u16),
    /// The body was longer than its limit or its listed size.
    TooLong,
    /// The body ended before its listed size.
    Truncated,
    /// The owner cancelled the download.
    Cancelled,
    /// WinHTTP failed in another way; the number is its Windows error code.
    Failed(u32),
}

impl TransferProblem {
    /// Wrap this problem in an `io::Error`, so a download reader can report it through `Read`.
    pub fn into_io(self) -> io::Error {
        io::Error::other(self)
    }

    /// The problem an `io::Error` from a download reader carries, if any.
    pub fn carried_by(error: &io::Error) -> Option<Self> {
        error.get_ref()?.downcast_ref::<Self>().copied()
    }
}

impl fmt::Display for TransferProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreachable => formatter.write_str("the update host could not be reached"),
            Self::Certificate => formatter.write_str("the update host's certificate was refused"),
            Self::TimedOut => formatter.write_str("the update host stopped responding"),
            Self::Redirected => formatter.write_str("the update host answered with a redirect"),
            Self::Status(status) => write!(formatter, "the update host answered {status}"),
            Self::TooLong => formatter.write_str("a download was longer than allowed"),
            Self::Truncated => formatter.write_str("a download ended early"),
            Self::Cancelled => formatter.write_str("the download was cancelled"),
            Self::Failed(code) => write!(formatter, "the download failed (WinHTTP error {code})"),
        }
    }
}

impl std::error::Error for TransferProblem {}

impl fmt::Display for InstallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Update(error) => write!(formatter, "verification refused the update: {error}"),
            Self::Io { step, kind } => write!(formatter, "{step} failed: {kind}"),
            Self::Layout => formatter.write_str("the program and data folders overlap"),
            Self::NotInstalled => formatter.write_str("this Syrinx is not an installed copy"),
            Self::NotAPackage => formatter.write_str("the folder is not a signed update package"),
            Self::Busy => formatter.write_str("another update step is in progress"),
            Self::TimedOut { what } => write!(formatter, "timed out waiting for {what}"),
            Self::UnexpectedEntry { what } => {
                write!(formatter, "{what} is not a folder Syrinx made")
            }
            Self::Record { what } => write!(formatter, "the {what} record is malformed"),
            Self::MissingProgramFile { name } => write!(formatter, "the package lacks {name}"),
            Self::NotEnoughSpace { needed, available } => {
                write!(formatter, "{needed} bytes needed but {available} free")
            }
            Self::StagedManifestChanged => {
                formatter.write_str("the staged manifest differs from the verified one")
            }
            Self::StagedVersionMismatch { pending, manifest } => {
                write!(
                    formatter,
                    "pending {pending} but the manifest installs {manifest}"
                )
            }
            Self::NothingStaged => formatter.write_str("no update is staged"),
            Self::ProgramInUse => formatter.write_str("the installed Syrinx folder is in use"),
            Self::SetAside(error) => write!(
                formatter,
                "the installed version is in Syrinx.previous and could not be put back: {error}"
            ),
            Self::UpdateWaiting => formatter.write_str("a staged update is waiting"),
            Self::AfterSwap(error) => write!(formatter, "after the swap: {error}"),
            Self::NoReleaseKeys => formatter.write_str("no release key is pinned"),
            Self::Transfer(problem) => {
                write!(formatter, "downloading the update failed: {problem}")
            }
            Self::Incomplete { primary, cleanup } => {
                write!(formatter, "{primary}; cleaning up also failed: {cleanup}")
            }
            #[cfg(test)]
            Self::Interrupted(step) => write!(formatter, "interrupted at {step:?}"),
        }
    }
}

impl std::error::Error for InstallError {}
