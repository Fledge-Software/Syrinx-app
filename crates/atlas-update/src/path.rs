//! Payload paths that are safe to join onto a staging directory on Windows.

use crate::error::PathProblem;

/// Longest path in bytes, kept well under `MAX_PATH` once joined to a staging directory.
pub const MAX_PATH_BYTES: usize = 240;
/// Longest single component in bytes.
pub const MAX_COMPONENT_BYTES: usize = 64;
/// Most components in one path.
pub const MAX_COMPONENTS: usize = 8;

/// Windows device names that address a device whatever the extension or directory.
const DEVICE_NAMES: [&str; 4] = ["CON", "PRN", "AUX", "NUL"];
const NUMBERED_DEVICE_PREFIXES: [&str; 2] = ["COM", "LPT"];

/// A validated relative payload path using `/` separators.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PackagePath(String);

impl PackagePath {
    /// Accepts only relative paths of safe ASCII components; see `docs/p1-38-update-format.md`.
    pub fn parse(text: &str) -> Result<Self, PathProblem> {
        if text.len() > MAX_PATH_BYTES {
            return Err(PathProblem::TooLong);
        }
        let mut count = 0;
        for component in text.split('/') {
            count += 1;
            if count > MAX_COMPONENTS {
                return Err(PathProblem::TooDeep);
            }
            check_component(component)?;
        }
        Ok(Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The spelling Windows treats as the same file: ASCII letter case folded.
    pub(crate) fn folded(&self) -> String {
        self.0.to_ascii_lowercase()
    }
}

/// Refuse a component Windows could read as something other than a plain name.
fn check_component(component: &str) -> Result<(), PathProblem> {
    if component.is_empty() {
        return Err(PathProblem::EmptyComponent);
    }
    if component.len() > MAX_COMPONENT_BYTES {
        return Err(PathProblem::ComponentTooLong);
    }
    if !component.bytes().all(is_allowed_byte) {
        return Err(PathProblem::Character);
    }
    if component == "." || component == ".." {
        return Err(PathProblem::DotComponent);
    }
    if component.ends_with('.') {
        return Err(PathProblem::TrailingDot);
    }
    if is_device_name(component) {
        return Err(PathProblem::DeviceName);
    }
    Ok(())
}

/// ASCII letters, digits, `.`, `_` and `-`; this excludes `:`, `\` and drive letters' colon.
fn is_allowed_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
}

/// Match a reserved device name on the part before the first dot, in any case.
fn is_device_name(component: &str) -> bool {
    let stem = component.split('.').next().unwrap_or(component);
    let upper = stem.to_ascii_uppercase();
    if DEVICE_NAMES.contains(&upper.as_str()) {
        return true;
    }
    NUMBERED_DEVICE_PREFIXES.iter().any(|prefix| {
        upper
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.len() == 1 && rest.bytes().all(|byte| byte.is_ascii_digit()))
    })
}
