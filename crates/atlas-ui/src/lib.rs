//! Atlas's design tokens as plain data, so the chrome draws every colour and size from one place.
//!
//! The crate has no toolkit dependency: `atlas-app` converts [`Color`] to GPUI colours.

pub mod accent;
pub mod color;
pub mod contrast;
pub mod metrics;
pub mod palette;
pub mod sidebar;

pub use accent::{ACCENT_PRESETS, Accent, AccentColors, DEFAULT_ACCENT};
pub use color::{Color, contrast_ratio};
pub use palette::{Ground, Palette, PaletteId, PluginColors, TileColors, WindowMaterial, palette};
pub use sidebar::SidebarWidthState;
