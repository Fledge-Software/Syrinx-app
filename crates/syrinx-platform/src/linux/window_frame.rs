//! Accepts the request to round a popup's corners; on Linux the compositor draws window frames.

use std::io;

/// Does nothing: corners and shadows belong to the X11 window manager or compositor.
pub fn round_with_shadow(_window: isize) -> io::Result<()> {
    Ok(())
}
