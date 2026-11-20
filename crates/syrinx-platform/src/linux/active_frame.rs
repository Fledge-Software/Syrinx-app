//! Accepts the request to keep a window family drawn active; X11 frames have no Mica fallback.

/// Does nothing: the inactive-frame material fallback is a Windows DWM behaviour.
pub fn install(_window: isize) -> Result<(), &'static str> {
    Ok(())
}
