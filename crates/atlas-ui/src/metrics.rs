//! Spacing, corner radius and type sizes, in logical pixels, as the design canvas uses them.
//!
//! Values are the canvas's own (`design/canvas/project/*.dc.html`); the names group the
//! repeated ones so later slices stop typing raw numbers.

/// Gaps and padding, from the canvas's most-used `gap` and `padding` values.
pub mod space {
    pub const XXS: f32 = 2.;
    pub const XS: f32 = 4.;
    pub const S: f32 = 6.;
    pub const M: f32 = 8.;
    pub const L: f32 = 10.;
    pub const XL: f32 = 12.;
    pub const XXL: f32 = 16.;
}

/// Corner radii.
pub mod radius {
    /// Small marks and the palette's thumbnails.
    pub const XS: f32 = 4.;
    /// Keyboard hints such as `Ctrl K`.
    pub const KEY_HINT: f32 = 5.;
    /// Icon buttons, the address field and sidebar rows.
    pub const CONTROL: f32 = 8.;
    /// Tiles and cards inside a page.
    pub const TILE: f32 = 10.;
    /// The web page card and the peeking sidebar's outer edge.
    pub const PAGE_CARD: f32 = 12.;
    /// The page card around Atlas's own pages (home, plugins).
    pub const HOME_CARD: f32 = 14.;
}

/// Font sizes and weights.
pub mod text {
    pub const CAPTION: f32 = 11.;
    pub const SMALL: f32 = 12.;
    pub const ADDRESS: f32 = 12.5;
    pub const BODY: f32 = 13.;
    pub const PALETTE_ROW: f32 = 13.5;
    pub const SUBTITLE: f32 = 15.;
    pub const TITLE: f32 = 20.;
    pub const DISPLAY: f32 = 26.;

    pub const WEIGHT_REGULAR: u16 = 400;
    pub const WEIGHT_MEDIUM: u16 = 500;
    pub const WEIGHT_SEMIBOLD: u16 = 600;
    pub const WEIGHT_BOLD: u16 = 700;

    /// The chrome uses the operating system's UI font (`design/README.md`).
    pub const UI_FONT_FAMILIES: [&str; 2] = ["Segoe UI Variable Text", "Segoe UI"];

    /// Only the palette's `>` `@` `/` hints are monospace. The canvas loads JetBrains Mono
    /// from the web; Atlas fetches nothing, so it uses installed Windows faces.
    pub const MONO_FONT_FAMILIES: [&str; 2] = ["Cascadia Mono", "Consolas"];
}

/// Fixed chrome sizes.
pub mod chrome {
    /// The title bar above the page card.
    pub const TITLE_BAR_HEIGHT: f32 = 36.;
    /// Back, forward, reload and the other square icon buttons.
    pub const ICON_BUTTON: f32 = 30.;
    /// The gap between the page card and the window's right and bottom edges.
    pub const CARD_INSET: f32 = 8.;
}
