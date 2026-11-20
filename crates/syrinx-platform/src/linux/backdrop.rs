//! Accepts the Windows 11 backdrop requests on Linux, where the compositor owns window materials.

use std::io;

/// The Windows materials Syrinx asks for; on Linux the window keeps its own painted fill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backdrop {
    Mica,
    MicaAlt,
    Acrylic,
}

/// Does nothing: X11 has no system backdrop, so Syrinx paints its window fill itself.
pub fn apply(_window: isize, _backdrop: Backdrop, _dark: bool) -> io::Result<()> {
    Ok(())
}
