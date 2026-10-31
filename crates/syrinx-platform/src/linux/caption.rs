//! Names the caption buttons; on Linux GPUI's own controls handle them without a hit-test fix.

/// The caption buttons, in the order Syrinx draws them at a window's top-right corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptionButton {
    Minimize,
    Maximize,
    Close,
}

impl CaptionButton {
    /// Left to right, ending at the window's right edge.
    pub const LEFT_TO_RIGHT: [Self; 3] = [Self::Minimize, Self::Maximize, Self::Close];
}

/// Does nothing: the Windows `WM_NCHITTEST` problem (#11) has no X11 counterpart.
pub fn install(_window: isize, _button_size: f32) -> Result<(), &'static str> {
    Ok(())
}
