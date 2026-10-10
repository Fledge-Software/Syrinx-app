//! Owns Atlas's application state independently of its interface and browser engine.

mod tabs;

pub use tabs::{
    Activation, SavedPage, SavedPageId, SavedPageSnapshot, SidebarRow, SnapshotError, Tab,
    TabError, TabId, TabKind, TabMetadata, TabModel, TabModelSnapshot, TabSnapshot,
};
