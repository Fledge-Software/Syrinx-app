//! Atlas's design tokens as plain data, so the chrome draws every colour and size from one place.
//!
//! The crate has no toolkit dependency: `atlas-app` converts [`Color`] to GPUI colours.

pub mod accent;
pub mod color;
pub mod contrast;
pub mod focus;
pub mod metrics;
pub mod palette;
pub mod sidebar;
pub mod sidebar_setting;

pub use accent::{ACCENT_PRESETS, Accent, AccentColors, DEFAULT_ACCENT};
pub use color::{Color, contrast_ratio};
pub use palette::{Ground, Palette, PaletteId, PluginColors, TileColors, WindowMaterial, palette};
pub use sidebar::{SidebarSizing, SidebarWidthState};
pub use sidebar_setting::{
    SIDEBAR_WIDTH_STATE_KEY, parse_sidebar_setting, sidebar_setting, sidebar_setting_value,
};
