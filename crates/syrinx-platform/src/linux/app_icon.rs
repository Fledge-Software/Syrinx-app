//! Accepts the request to set the hawk icon; on Linux the desktop entry supplies it.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconError {
    InvalidWindow,
    Module,
    Load { size: i32 },
}

impl fmt::Display for IconError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidWindow => write!(formatter, "The window for the Syrinx icon is gone."),
            Self::Module => write!(formatter, "Syrinx's icon resource is unavailable."),
            Self::Load { size } => write!(formatter, "Cannot load the {size} px Syrinx icon."),
        }
    }
}

impl std::error::Error for IconError {}

/// Does nothing on Linux: the window manager takes the icon from `syrinx.desktop`.
pub fn apply(_window: isize) -> Result<(), IconError> {
    Ok(())
}
