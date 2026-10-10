//! Owns Atlas's application state independently of its interface and browser engine.

mod tabs;

pub use tabs::{
    Activation, SavedPage, SavedPageId, SidebarRow, Tab, TabError, TabId, TabKind, TabMetadata,
    TabModel,
};
